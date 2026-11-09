//! Adapted from Grok Build 2bdd1d6 app/effects/mod.rs send.start/send.done
//! (Apache-2.0): request correlation, elapsed_ms, ok. Dot transport adds an
//! allowlisted stage/error and numeric HTTP status. Never log URLs or payloads.
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Instant,
};
// Original entry layout and make_entry body from pager/unified_log.rs and
// telemetry/logs/unified_log.rs (2bdd1d6, xAI, Apache-2.0). Adaptations:
// use this binary's version; no ACP/session identifiers or external forwarding;
// the private local sink below accepts only fixed stages and error categories.
#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum LogLevel {
    Info,
    Warn,
}
#[derive(serde::Serialize)]
struct ClientLogEntry {
    ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ver: Option<String>,
    lvl: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    sid: Option<String>,
    msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ctx: Option<serde_json::Value>,
}
fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn make_entry(
    lvl: LogLevel,
    msg: &str,
    sid: Option<&str>,
    ctx: Option<serde_json::Value>,
) -> ClientLogEntry {
    ClientLogEntry {
        ts: now_ts(),
        pid: Some(std::process::id()),
        ver: Some(env!("CARGO_PKG_VERSION").to_owned()),
        lvl,
        sid: sid.map(Into::into),
        msg: msg.into(),
        ctx,
    }
}
static LOG: OnceLock<Mutex<File>> = OnceLock::new();
#[derive(Clone, Copy)]
pub enum Stage {
    DotList,
    ReadMessages,
    ReadImage,
    UploadImage,
    SendMessage,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::DotList => "dot_list",
            Self::ReadMessages => "read_messages",
            Self::ReadImage => "read_image",
            Self::UploadImage => "upload_image",
            Self::SendMessage => "send_message",
        }
    }
}
#[derive(Clone, Copy)]
pub enum Error {
    Timeout,
    Connect,
    Transport,
    Http,
    Body,
    Decode,
    InvalidAck,
    Interrupted,
}
impl Error {
    pub fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Transport => "transport",
            Self::Http => "http",
            Self::Body => "body",
            Self::Decode => "decode",
            Self::InvalidAck => "invalid_ack",
            Self::Interrupted => "interrupted",
        }
    }
}
pub fn classify(e: &reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else if e.is_connect() {
        Error::Connect
    } else {
        Error::Transport
    }
}
pub fn path() -> Option<PathBuf> {
    crate::platform_fs::home().map(|p| p.join(".dot-tui-send-state/diagnostics.jsonl"))
}
pub fn init() -> Result<(), String> {
    let path = path().ok_or("Cannot locate diagnostics")?;
    let dir = path.parent().unwrap();
    crate::platform_fs::directory(dir, true)
        .map_err(|_| "Cannot create or validate private diagnostics directory")?;
    let f = open(&path)?;
    let _ = LOG.set(Mutex::new(f));
    Ok(())
}
fn open(path: &Path) -> Result<File, String> {
    crate::platform_fs::append(path).map_err(|_| "Cannot open private diagnostics log".into())
}
fn record(entry: ClientLogEntry) {
    if let Some(log) = LOG.get()
        && let Ok(mut f) = log.lock()
    {
        // Keep history bounded; never replace another file or discard past evidence.
        if f.metadata().is_ok_and(|m| m.len() < 4 * 1024 * 1024) {
            if let Ok(line) = serde_json::to_string(&entry) {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}
pub struct Request {
    stage: Stage,
    id: Option<String>,
    start: Instant,
    done: bool,
}
impl Request {
    pub fn start(stage: Stage, id: Option<&str>) -> Self {
        let id = id
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(|u| u.to_string());
        record(make_entry(
            LogLevel::Info,
            "prompt.http_send.start",
            None,
            Some(serde_json::json!({"stage":stage.name(),"request_id":id})),
        ));
        Self {
            stage,
            id,
            start: Instant::now(),
            done: false,
        }
    }
    pub fn done(&mut self, ok: bool, error: Option<Error>, http: Option<u16>) {
        if self.done {
            return;
        }
        self.done = true;
        record(self.value(ok, error, http));
    }
    fn value(&self, ok: bool, error: Option<Error>, http: Option<u16>) -> ClientLogEntry {
        make_entry(
            if ok { LogLevel::Info } else { LogLevel::Warn },
            "prompt.http_send.done",
            None,
            Some(
                serde_json::json!({"stage":self.stage.name(),"request_id":self.id,"elapsed_ms":self.start.elapsed().as_millis() as u64,"ok":ok,"http_status":http,"error_kind":error.map(Error::name)}),
            ),
        )
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        if !self.done {
            self.done(false, Some(Error::Interrupted), None)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[test]
    fn sensitive_strings_cannot_enter_log() {
        let r = Request::start(
            Stage::UploadImage,
            Some("Bearer secret https://private/path text filename.png"),
        );
        let s = serde_json::to_string(&r.value(false, Some(Error::Http), Some(413))).unwrap();
        for secret in ["Bearer", "secret", "private", "filename", "text"] {
            assert!(!s.contains(secret));
        }
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["ctx"]["http_status"], 413);
        assert_eq!(v["lvl"], "warn");
        assert_eq!(v["ver"], env!("CARGO_PKG_VERSION"));
        assert!(v["pid"].is_number());
        assert!(v.get("sid").is_none());
        assert!(v["ctx"]["request_id"].is_null());
    }
    #[test]
    #[cfg(unix)]
    fn private_file_rejects_symlink() {
        use std::os::unix::fs::{MetadataExt, symlink};
        let dir = std::env::temp_dir().join(format!("dot-log-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let p = dir.join("log");
        let f = open(&p).unwrap();
        assert_eq!(f.metadata().unwrap().mode() & 0o777, 0o600);
        drop(f);
        symlink(&p, dir.join("link")).unwrap();
        assert!(open(&dir.join("link")).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
