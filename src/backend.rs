//! Fixed-origin adapter for the unofficial ChatGPT dot protocol.
//! Explicit send only; no token logging, retries, redirects, proxy or token refresh.
use reqwest::{
    Url,
    blocking::Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde_json::Value;
use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const BASE: &str = "https://chatgpt.com/backend-api";
const MAX_AUTH: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dot {
    pub id: String,
    pub name: String,
    pub room: Option<String>,
    pub speakers: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub created_at: String,
    pub files: Vec<String>,
}

impl Message {
    /// Both live parser and offline fixture labels represent human messages.
    /// Unknown authors stay expanded; do not infer the current user's identity.
    pub fn is_human_message(&self) -> bool {
        matches!(self.role.as_str(), "you" | "other / you")
    }
}

pub struct MessagePage {
    pub messages: Vec<Message>,
    pub raw_count: usize,
    pub next_before: Option<String>,
}

/// A local rejection guarantees no request was attempted. All post-attempt failures
/// are uncertain: the caller must retain its pending lock and must not auto-retry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    Rejected(String),
    /// Upload may have occurred, but the message endpoint was never attempted.
    NotSent(String),
    Uncertain(String),
}

// Deliberately not Debug: default headers contain an account credential.
#[derive(Clone)]
pub struct Backend {
    client: Client,
}

impl Backend {
    pub fn new(auth_path: PathBuf) -> Result<Self, String> {
        let (token, account) = read_auth(&auth_path)?;
        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "Invalid login fields.".to_owned())?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        let mut account_header =
            HeaderValue::from_str(&account).map_err(|_| "Invalid login fields.".to_owned())?;
        account_header.set_sensitive(true);
        headers.insert("ChatGPT-Account-Id", account_header);
        headers.insert("User-Agent", HeaderValue::from_static("Codex Desktop"));
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));
        headers.insert("OAI-Product-Sku", HeaderValue::from_static("CODEX"));
        headers.insert("Accept", HeaderValue::from_static("application/json"));
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(true)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "Cannot initialize secure transport.".to_owned())?;
        Ok(Self { client })
    }

    fn get(&self, url: Url) -> Result<Value, String> {
        // Defense in depth: this client has no caller-selected endpoint or verb.
        if url.scheme() != "https"
            || url.host_str() != Some("chatgpt.com")
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || !url.path().starts_with("/backend-api/")
        {
            return Err("Unexpected service destination refused.".into());
        }
        let stage = if url.path().ends_with("/tbo") {
            crate::diagnostics::Stage::DotList
        } else if url.path().contains("/files/") {
            crate::diagnostics::Stage::ReadImage
        } else {
            crate::diagnostics::Stage::ReadMessages
        };
        let mut trace = crate::diagnostics::Request::start(stage, None);
        let mut response = self.client.get(url).send().map_err(|e| {
            trace.done(false, Some(crate::diagnostics::classify(&e)), None);
            if e.is_timeout() {
                "Read request timed out (20s limit). No automatic retry.".to_owned()
            } else if e.is_connect() {
                "Connection could not be established. No automatic retry.".to_owned()
            } else {
                "Network request failed. No automatic retry.".to_owned()
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            trace.done(
                false,
                Some(crate::diagnostics::Error::Http),
                Some(status.as_u16()),
            );
        }
        if status.is_redirection() {
            return Err("Redirect refused. Check access in the official app.".into());
        }
        if status.as_u16() == 401 {
            return Err("Session expired. Sign in with official Codex, then restart.".into());
        }
        if status.as_u16() == 429 {
            return Err("Rate limited. Pause refresh and try later.".into());
        }
        if !status.is_success() {
            return Err(format!(
                "Service returned HTTP {}. No automatic retry.",
                status.as_u16()
            ));
        }
        let mut bytes = Vec::new();
        response.read_to_end(&mut bytes).map_err(|_| {
            trace.done(
                false,
                Some(crate::diagnostics::Error::Body),
                Some(status.as_u16()),
            );
            "Cannot read service response.".to_owned()
        })?;
        let result =
            serde_json::from_slice(&bytes).map_err(|_| "Unexpected response format.".into());
        trace.done(
            result.is_ok(),
            if result.is_err() {
                Some(crate::diagnostics::Error::Decode)
            } else {
                None
            },
            Some(status.as_u16()),
        );
        result
    }

    /// Submit exactly once to an existing room. The returned ID acknowledges
    /// server acceptance only, not delivery or a reply. Caller owns explicit
    /// submission and retains the consumed prompt in session history.
    pub fn send(&self, dot: &Dot, text: &str, request_id: &str) -> Result<String, SendError> {
        send_with_transport(dot, text, request_id, |url, payload| {
            let mut trace = crate::diagnostics::Request::start(
                crate::diagnostics::Stage::SendMessage,
                Some(request_id),
            );
            let response = self.client.post(url).json(payload).send().map_err(|e| {
                let kind = crate::diagnostics::classify(&e);
                trace.done(false, Some(kind), None);
                format!(
                    "Message submission {}. Check the official app before retrying.",
                    kind.name()
                )
            })?;
            read_ack(response, &mut trace)
        })
    }

    pub fn send_images(
        &self,
        dot: &Dot,
        text: &str,
        images: &[crate::prompt_images::PastedImage],
        request_id: &str,
    ) -> Result<String, SendError> {
        let room = crate::image_transfer::BoundRoom::from_unique(
            std::slice::from_ref(dot),
            dot.room.as_deref().unwrap_or(""),
        )
        .map_err(|e| SendError::Rejected(e.into()))?;
        let mut staged = Vec::new();
        for image in images {
            let bytes = image
                .encoded_bytes
                .as_ref()
                .ok_or_else(|| SendError::Rejected("Image bytes unavailable".into()))?;
            let mut local = crate::image_transfer::LocalImage::stage(bytes.to_vec())
                .map_err(|e| SendError::Rejected(e.into()))?;
            local.record_preview(true); // App checked flushed preview identity before creating this immutable plan.
            staged.push(local);
        }
        let mut wire = ImageTransport(self, request_id);
        crate::image_transfer::transmit(&mut wire, &room, text, &staged, request_id)
            .map_err(|e| e.into_send_error())
    }

    pub fn view_image(
        &self,
        dot: &Dot,
        file: &str,
    ) -> Result<crate::prompt_images::PastedImage, String> {
        let room = crate::image_transfer::BoundRoom::from_unique(
            std::slice::from_ref(dot),
            dot.room.as_deref().unwrap_or(""),
        )
        .map_err(str::to_owned)?;
        crate::image_download::fetch(&mut ImageViewTransport(self), &room, file)
    }
    pub fn list_dots(&self) -> Result<Vec<Dot>, String> {
        let url = Url::parse(&format!("{BASE}/tbo")).map_err(|_| "Invalid fixed service URL.")?;
        parse_dots(&self.get(url)?)
    }

    /// Request one page; parse every returned item without client-side truncation.
    pub fn message_page(&self, dot: &Dot, before: Option<&str>) -> Result<MessagePage, String> {
        let room = dot
            .room
            .as_deref()
            .filter(|r| !r.is_empty())
            .ok_or("No confirmed message room is available.")?;
        let value = self.get(messages_url(room, before)?)?;
        let items = value
            .get("items")
            .and_then(Value::as_array)
            .ok_or("Unexpected message format.")?;
        // Observed adapter protocol: `items` and message-ID `before` only.
        // Do not invent has_more/next_cursor or use filtered counts as EOF.
        let next_before = items
            .iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?;
                if id.is_empty() {
                    return None;
                }
                let timestamp = item.get("created_at").and_then(Value::as_str).unwrap_or("");
                Some((timestamp, id))
            })
            .min()
            .map(|(_, id)| id.to_owned());
        Ok(MessagePage {
            raw_count: items.len(),
            next_before,
            messages: parse_messages(&value, dot)?,
        })
    }

    /// Compatibility path; production pagination uses message_page metadata.
    pub fn messages(&self, dot: &Dot, before: Option<&str>) -> Result<Vec<Message>, String> {
        let Some(room) = dot.room.as_deref().filter(|s| !s.is_empty()) else {
            return Err("No message room is available; this is not a confirmed empty history. Open the official app first.".into());
        };
        let url = messages_url(room, before)?;
        parse_messages(&self.get(url)?, dot)
    }
}

fn valid_request_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

// Private fakeable boundary tests serialization and no-retry semantics without
// a real HTTP server or credentials. No caller-provided URL can reach transport.
fn send_with_transport<F>(
    dot: &Dot,
    text: &str,
    request_id: &str,
    transport: F,
) -> Result<String, SendError>
where
    F: FnOnce(Url, &Value) -> Result<Value, String>,
{
    if text.trim().is_empty() {
        return Err(SendError::Rejected("Message must not be blank.".into()));
    }
    if !valid_request_id(request_id) {
        return Err(SendError::Rejected(
            "Request ID must be a canonical UUID.".into(),
        ));
    }
    let room = dot
        .room
        .as_deref()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| {
            SendError::Rejected(
                "No existing message room. Open this dot in the official app first.".into(),
            )
        })?;
    let mut url = messages_url(room, None).map_err(SendError::Rejected)?;
    url.set_query(None);
    // Defense in depth for the only permitted mutation destination.
    let segments: Vec<_> = url.path().split('/').collect();
    if url.scheme() != "https"
        || url.host_str() != Some("chatgpt.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || segments.len() != 6
        || segments[1] != "backend-api"
        || segments[2] != "messaging"
        || segments[3] != "rooms"
        || segments[4].is_empty()
        || segments[5] != "messages"
    {
        return Err(SendError::Rejected(
            "Unexpected send destination refused.".into(),
        ));
    }
    let body = serde_json::json!({
        "content": {"text": text, "attachments": []},
        "request_id": request_id,
        "idempotency_token": request_id,
    });
    let value = transport(url, &body).map_err(SendError::Uncertain)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(|| {
            SendError::Uncertain(
                "Send acknowledgement has no valid message ID. Check the official app.".into(),
            )
        })?;
    Ok(id.to_owned())
}

fn messages_url(room: &str, before: Option<&str>) -> Result<Url, String> {
    if room.is_empty() || matches!(room, "." | "..") {
        return Err("Invalid message room identifier.".into());
    }
    let mut url = Url::parse(BASE).map_err(|_| "Invalid fixed service URL.")?;
    url.path_segments_mut()
        .map_err(|_| "Invalid fixed service URL.")?
        .extend(["messaging", "rooms", room, "messages"]);
    url.query_pairs_mut().append_pair("limit", "32");
    if let Some(id) = before.filter(|s| !s.is_empty()) {
        url.query_pairs_mut().append_pair("before", id);
    }
    Ok(url)
}

fn read_auth(path: &Path) -> Result<(String, String), String> {
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("Live login is currently supported only on Unix; Windows ACL validation is not implemented.".into())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        // O_NOFOLLOW rejects the final symlink; descriptor metadata avoids a check/open race.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| {
                "Cannot securely open login file. Use official Codex login first.".to_owned()
            })?;
        let meta = file
            .metadata()
            .map_err(|_| "Cannot inspect login file.".to_owned())?;
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return Err(
                "Login must be a regular file owned by you with no group/other permissions.".into(),
            );
        }
        if meta.len() > MAX_AUTH {
            return Err("Login file exceeds size limit.".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_AUTH + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read login file.".to_owned())?;
        if bytes.len() as u64 > MAX_AUTH {
            return Err("Login file exceeds size limit.".into());
        }
        // Support UTF-8 BOM, matching the reference client.
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        parse_auth(bytes)
    }
}

fn parse_auth(bytes: &[u8]) -> Result<(String, String), String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid login file format.".to_owned())?;
    if value.get("auth_mode").and_then(Value::as_str) != Some("chatgpt") {
        return Err("ChatGPT login required; API-key login is not supported.".into());
    }
    let token = value
        .pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let account = value
        .pointer("/tokens/account_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    match (token, account) {
        (Some(t), Some(a)) if !t.chars().chain(a.chars()).any(char::is_control) => {
            Ok((t.to_owned(), a.to_owned()))
        }
        _ => Err("Login fields are missing or invalid.".into()),
    }
}

fn safe_text(s: &str) -> String {
    s.chars().filter(|&c| (c == '\n' || c == '\t' || !c.is_control())
        && !matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')).collect()
}
fn parse_dots(value: &Value) -> Result<Vec<Dot>, String> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or("Unexpected dot list format.")?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_owned();
            if id.is_empty() {
                return None;
            }
            let name = safe_text(
                item.get("display_name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("dot"),
            );
            let room = item
                .get("messaging_room_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let speakers = item
                .pointer("/messaging_room_preview/members")
                .and_then(Value::as_array)
                .map(|members| {
                    members
                        .iter()
                        .filter_map(|m| {
                            if m.get("aeon_id")
                                .and_then(Value::as_str)
                                .is_some_and(|s| !s.is_empty())
                            {
                                m.get("account_user_id")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned)
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Dot {
                id,
                name,
                room,
                speakers,
            })
        })
        .collect())
}
fn parse_messages(value: &Value, dot: &Dot) -> Result<Vec<Message>, String> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or("Unexpected message format.")?;
    let mut messages: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_owned();
            if id.is_empty() {
                return None;
            }
            let mut text = safe_text(
                item.pointer("/content/text")
                    .and_then(Value::as_str)
                    .unwrap_or("此消息包含非文本内容，请在官方应用查看。"),
            );
            let files = item
                .pointer("/content/attachments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|a| a.get("type").and_then(Value::as_str) == Some("file"))
                .filter_map(|a| a.get("file_id").and_then(Value::as_str))
                .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if files.is_empty()
                && item
                    .pointer("/content/attachments")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty())
            {
                text.push_str("\n附件请在官方应用查看。");
            }
            let role = if dot.speakers.is_empty() {
                "unknown"
            } else if item
                .get("account_user_id")
                .and_then(Value::as_str)
                .is_some_and(|id| dot.speakers.iter().any(|s| s == id))
            {
                "dot"
            } else {
                "other / you"
            }
            .to_owned();
            let created_at =
                safe_text(item.get("created_at").and_then(Value::as_str).unwrap_or(""));
            Some(Message {
                id,
                role,
                text,
                created_at,
                files,
            })
        })
        .collect();
    messages.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    Ok(messages)
}

// Same credential-bearing fixed-origin/no-redirect client as text sends. Only
// immutable room-scoped URLs built by image_transfer reach this transport.
struct ImageTransport<'a>(&'a Backend, &'a str);
impl crate::image_transfer::Transport for ImageTransport<'_> {
    fn upload(&mut self, url: Url, field: &str, mime: &str, bytes: &[u8]) -> Result<Value, String> {
        image_origin(&url).map_err(|_| "Unexpected image service destination")?;
        let part = reqwest::blocking::multipart::Part::bytes(bytes.to_vec())
            .file_name(match mime {
                "image/png" => "image.png",
                "image/jpeg" => "image.jpg",
                "image/webp" => "image.webp",
                "image/gif" => "image.gif",
                "image/bmp" => "image.bmp",
                "image/tiff" => "image.tiff",
                _ => return Err("Unsupported image MIME".into()),
            })
            .mime_str(mime)
            .map_err(|_| "Invalid image MIME")?;
        let mut trace = crate::diagnostics::Request::start(
            crate::diagnostics::Stage::UploadImage,
            Some(self.1),
        );
        let response = self
            .0
            .client
            .post(url)
            // Unlike a small history GET, uploading a validated image can need
            // longer. Still bounded, and neither timeout nor HTTP errors retry.
            .timeout(Duration::from_secs(120))
            .multipart(reqwest::blocking::multipart::Form::new().part(field.to_owned(), part))
            .send()
            .map_err(|e| {
                let kind = crate::diagnostics::classify(&e);
                trace.done(false, Some(kind), None);
                format!("upload {}", kind.name())
            })?;
        read_ack(response, &mut trace)
    }
    fn message(&mut self, url: Url, body: &Value) -> Result<Value, String> {
        image_origin(&url).map_err(|_| "Unexpected message service destination")?;
        let mut trace = crate::diagnostics::Request::start(
            crate::diagnostics::Stage::SendMessage,
            Some(self.1),
        );
        let response = self.0.client.post(url).json(body).send().map_err(|e| {
            let kind = crate::diagnostics::classify(&e);
            trace.done(false, Some(kind), None);
            format!("message {}", kind.name())
        })?;
        read_ack(response, &mut trace)
    }
}
fn image_origin(url: &Url) -> Result<(), ()> {
    if url.scheme() != "https"
        || url.host_str() != Some("chatgpt.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/backend-api/messaging/rooms/")
    {
        Err(())
    } else {
        Ok(())
    }
}
fn read_ack(
    mut response: reqwest::blocking::Response,
    trace: &mut crate::diagnostics::Request,
) -> Result<Value, String> {
    use crate::diagnostics::Error;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        trace.done(false, Some(Error::Http), Some(status));
        return Err(format!("HTTP {status}"));
    }
    let mut bytes = Vec::new();
    response.read_to_end(&mut bytes).map_err(|_| {
        trace.done(false, Some(Error::Body), Some(status));
        "Cannot read acknowledgement".to_owned()
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        trace.done(false, Some(Error::Decode), Some(status));
        "Invalid acknowledgement JSON".to_owned()
    })?;
    if value
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(|id| id.is_empty() || id.chars().any(char::is_control))
    {
        trace.done(false, Some(Error::InvalidAck), Some(status));
        return Err("Acknowledgement missing valid ID".into());
    }
    trace.done(true, None, Some(status));
    Ok(value)
}

struct ImageViewTransport<'a>(&'a Backend);
impl crate::image_download::Transport for ImageViewTransport<'_> {
    fn metadata(&mut self, url: Url) -> Result<Value, String> {
        self.0.get(url)
    }
    fn anonymous(&mut self, url: Url) -> Result<Vec<u8>, String> {
        crate::image_download::download_public(url)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn sample_dot() -> Dot {
        Dot {
            id: "d".into(),
            name: "dot".into(),
            room: Some("r".into()),
            speakers: vec!["bot".into()],
        }
    }
    const REQUEST_ID: &str = "01345678-abcd-4abc-8abc-0123456789ab";
    #[test]
    fn send_serializes_once_without_credentials_or_room_creation() {
        let mut calls = 0;
        let id = send_with_transport(&sample_dot(), "你好\ntext", REQUEST_ID, |url, body| {
            calls += 1;
            assert_eq!(url.as_str(), "https://chatgpt.com/backend-api/messaging/rooms/r/messages");
            assert_eq!(body, &json!({"content":{"text":"你好\ntext","attachments":[]},"request_id":REQUEST_ID,"idempotency_token":REQUEST_ID}));
            Ok(json!({"id":"accepted-message-id"}))
        }).unwrap();
        assert_eq!(calls, 1);
        assert_eq!(id, "accepted-message-id");
    }
    #[test]
    fn send_local_rejections_never_call_transport() {
        let missing = Dot {
            room: None,
            ..sample_dot()
        };
        for (dot, text, key) in [
            (&sample_dot(), "  ", REQUEST_ID),
            (&sample_dot(), "hi", "bad"),
            (&missing, "hi", REQUEST_ID),
        ] {
            let outcome = send_with_transport(dot, text, key, |_, _| panic!("must not send"));
            assert!(matches!(outcome, Err(SendError::Rejected(_))));
        }
        let huge = "字".repeat(16_001);
        assert!(
            send_with_transport(&sample_dot(), &huge, REQUEST_ID, |_, body| {
                assert_eq!(body["content"]["text"], huge);
                Ok(json!({"id":"ok"}))
            })
            .is_ok()
        );
        let exact = "字".repeat(16_000);
        assert!(
            send_with_transport(&sample_dot(), &exact, REQUEST_ID, |_, _| Ok(
                json!({"id":"ok"})
            ))
            .is_ok()
        );
    }
    #[test]
    fn send_failures_are_uncertain_without_retry() {
        let mut calls = 0;
        let result = send_with_transport(&sample_dot(), "hi", REQUEST_ID, |_, _| {
            calls += 1;
            Err("Fixed transport error".into())
        });
        assert_eq!(calls, 1);
        assert!(matches!(result, Err(SendError::Uncertain(_))));
        for invalid in [
            json!({}),
            json!({"id":5}),
            json!({"id":""}),
            json!({"id":"bad\n"}),
        ] {
            assert!(matches!(
                send_with_transport(&sample_dot(), "hi", REQUEST_ID, |_, _| Ok(invalid)),
                Err(SendError::Uncertain(_))
            ));
        }
    }
    #[test]
    fn uuid_shape_and_send_path_are_checked() {
        assert!(valid_request_id(REQUEST_ID));
        assert!(!valid_request_id("01345678-abcd-4abc-8abc-0123456789ag"));
        assert!(!valid_request_id("01345678xabcd-4abc-8abc-0123456789ab"));
        let traversal = Dot {
            room: Some("..".into()),
            ..sample_dot()
        };
        assert!(matches!(
            send_with_transport(&traversal, "hi", REQUEST_ID, |_, _| panic!("must not send")),
            Err(SendError::Rejected(_))
        ));
        let encoded = Dot {
            room: Some("r/../../x?foo=1".into()),
            ..sample_dot()
        };
        send_with_transport(&encoded, "hi", REQUEST_ID, |url, _| {
            assert_eq!(url.host_str(), Some("chatgpt.com"));
            assert!(url.query().is_none());
            assert!(url.path().contains("%2F"));
            Ok(json!({"id":"ok"}))
        })
        .unwrap();
    }

    #[test]
    fn malformed_auth_is_private() {
        for input in [
            br#"{"secret":"do-not-print"}"#.as_slice(),
            br#"{"auth_mode":"api_key"}"#.as_slice(),
            b"bad-secret-json".as_slice(),
        ] {
            let err = parse_auth(input).unwrap_err();
            assert!(!err.contains("secret"));
        }
    }
    #[test]
    fn auth_requires_chatgpt_strings() {
        assert!(
            parse_auth(
                br#"{"auth_mode":"chatgpt","tokens":{"access_token":"fake","account_id":"a"}}"#
            )
            .is_ok()
        );
        assert!(parse_auth(
            br#"{"auth_mode":"chatgpt","tokens":{"access_token":"fake\nX: y","account_id":"a"}}"#
        )
        .is_err());
        assert!(
            parse_auth(br#"{"auth_mode":"chatgpt","tokens":{"access_token":3,"account_id":"a"}}"#)
                .is_err()
        );
    }
    #[test]
    fn url_encodes_identifiers_and_stays_on_origin() {
        let url = messages_url("r/../../evil?x=1", Some("a&b=2")).unwrap();
        assert!(messages_url("..", None).is_err());
        assert_eq!(url.host_str(), Some("chatgpt.com"));
        assert!(url.path().contains("r%2F..%2F..%2Fevil%3Fx=1"));
        assert_eq!(
            url.query_pairs().find(|(k, _)| k == "before").unwrap().1,
            "a&b=2"
        );
    }
    #[test]
    fn dots_tolerate_bad_optional_fields() {
        let dots = parse_dots(&json!({"items":[null,{"id":7},{"id":"d","display_name":[],"messaging_room_preview":false}]})).unwrap();
        assert_eq!(dots.len(), 1);
        assert_eq!(dots[0].name, "dot");
        assert!(dots[0].speakers.is_empty());
        assert!(parse_dots(&json!({"items":"bad"})).is_err());
    }
    #[test]
    fn message_order_roles_and_attachment_notice() {
        let rows=parse_messages(&json!({"items":[{"id":"2","created_at":"2026-02","account_user_id":"bot","content":{"text":"中文","attachments":[{}]}},{"id":"1","created_at":"2026-01","content":{"text":4}}]}),&sample_dot()).unwrap();
        assert_eq!(rows[0].id, "1");
        assert_eq!(rows[0].text, "此消息包含非文本内容，请在官方应用查看。");
        assert_eq!(rows[1].role, "dot");
        assert!(rows[1].text.contains("附件"));
    }
    #[test]
    fn response_preserves_complete_pages_and_long_message_text() {
        let items: Vec<_> = (0..100).map(|i| json!({"id":i.to_string(), "created_at":"2026-01", "content":{"text":"a".repeat(17000)}})).collect();
        let rows = parse_messages(&json!({"items":items}), &sample_dot()).unwrap();
        assert_eq!(rows.len(), 100);
        assert_eq!(rows[0].text, "a".repeat(17000));
    }
    #[test]
    fn control_characters_never_reach_terminal() {
        let clean = safe_text("\x1b]52;c;abc\x07hi\u{202e}\n中文\t");
        assert!(!clean.contains('\x1b'));
        assert!(!clean.contains('\x07'));
        assert!(!clean.contains('\u{202e}'));
        assert!(clean.contains("\n中文\t"));
    }
    #[cfg(unix)]
    #[test]
    fn secure_auth_rejects_world_permissions_and_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let base = std::env::temp_dir().join(format!("dot-tui-auth-test-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let p = base.join("synthetic.json");
        std::fs::write(&p,br#"{"auth_mode":"chatgpt","tokens":{"access_token":"synthetic-test-only","account_id":"fake"}}"#).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_auth(&p).is_err());
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_auth(&p).is_ok());
        let link = base.join("link");
        symlink(&p, &link).unwrap();
        assert!(read_auth(&link).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }
}
