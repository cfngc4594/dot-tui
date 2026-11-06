//! Original Grok macOS file-URL/raster AppleScript probe, fixed 2bdd1d6.
//! Host uses bounded private files/child deadline; invoked only by explicit paste.
use std::os::unix::{
    fs::{DirBuilderExt, OpenOptionsExt},
    process::CommandExt,
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const FURL_MARKER: &str = "<<<FURL>>>";
const IMAGE_MARKER: &str = "<<<IMAGE>>>";
#[derive(Clone, Copy)]
enum RasterClass {
    Png,
    Tiff,
    Jpeg,
}
impl RasterClass {
    const ALL: [Self; 3] = [Self::Png, Self::Tiff, Self::Jpeg];
    fn code(self) -> &'static str {
        match self {
            Self::Png => "PNGf",
            Self::Tiff => "TIFF",
            Self::Jpeg => "JPEG",
        }
    }
}
static STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static OWNED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
pub fn cleanup_owned() {
    STOPPING.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut paths) = OWNED.lock() {
        for path in paths.drain(..) {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
pub struct ProbeTemps(PathBuf);
impl ProbeTemps {
    fn new() -> Result<Self, String> {
        let path =
            std::env::temp_dir().join(format!("dot-image-clipboard-{}", uuid::Uuid::new_v4()));
        let mut owned = OWNED.lock().map_err(|_| "clipboard staging unavailable")?;
        if STOPPING.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("application is closing".into());
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| "cannot create private clipboard staging")?;
        owned.push(path.clone());
        drop(owned);
        let me = Self(path);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(me.path())
            .map_err(|_| "cannot create clipboard staging file")?;
        Ok(me)
    }
    fn path(&self) -> PathBuf {
        self.0.join("probe")
    }
}
impl Drop for ProbeTemps {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        if let Ok(mut owned) = OWNED.lock() {
            owned.retain(|p| p != &self.0);
        }
    }
}
fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Nested `try` chain coercing each class in order into this probe's file; the code of the one that wrote lands in the caller's `imageOut`. Guillemets are required for `«class …»`.
pub fn raster_try_chain(temps: &ProbeTemps) -> String {
    let path = applescript_string(&temps.path().display().to_string());
    let mut script = String::new();
    for class in RasterClass::ALL {
        script.push_str(&format!(
            "try\n\
                 set imgData to the clipboard as \u{00AB}class {code}\u{00BB}\n\
                 set filePath to POSIX file {path} as text\n\
                 set fRef to open for access file filePath with write permission\n\
                 set eof of fRef to 0\n\
                 write imgData to fRef\n\
                 close access fRef\n\
                 set imageOut to \"{code}\"\n\
                 on error\n",
            code = class.code(),
        ));
    }
    for _ in RasterClass::ALL {
        script.push_str("end try\n");
    }
    script
}

/// Prints the class code that wrote, else `none`.
#[allow(dead_code)] // Original standalone helper retained; combined probe is used.
pub fn image_osascript(temps: &ProbeTemps) -> String {
    format!(
        "set imageOut to \"none\"\n{}return imageOut",
        raster_try_chain(temps)
    )
}

/// File URLs first, rasters only when none are present; without `temps` the raster half is skipped so a Finder paste still works. Stdout follows [`parse_attachments_output`].
pub fn attachments_osascript(temps: Option<&ProbeTemps>) -> String {
    let image_probe = temps.map_or_else(String::new, |temps| {
        format!(
            "if furlOut is \"none\" then\n{}end if\n",
            raster_try_chain(temps)
        )
    });
    format!(
        "set furlOut to \"none\"\n\
             try\n\
             set urlList to the clipboard as list\n\
             set out to \"\"\n\
             repeat with u in urlList\n\
             try\n\
             set itemRef to contents of u as \u{00AB}class furl\u{00BB}\n\
             set out to out & POSIX path of itemRef & \"\\n\"\n\
             end try\n\
             end repeat\n\
             if out is not \"\" then\n\
             set furlOut to out\n\
             else\n\
             try\n\
             set urlRef to the clipboard as \u{00AB}class furl\u{00BB}\n\
             set furlOut to POSIX path of urlRef\n\
             on error\n\
             end try\n\
             end if\n\
             on error\n\
             try\n\
             set urlRef to the clipboard as \u{00AB}class furl\u{00BB}\n\
             set furlOut to POSIX path of urlRef\n\
             on error\n\
             end try\n\
             end try\n\
             set imageOut to \"NONE\"\n\
             {image_probe}\
             return \"{furl_marker}\" & linefeed & furlOut & linefeed & \"{image_marker}\" & linefeed & \"IMAGE:\" & imageOut",
        furl_marker = FURL_MARKER,
        image_marker = IMAGE_MARKER,
    )
}

pub enum Payload {
    Paths(Vec<PathBuf>),
    Bytes(Vec<u8>),
    None,
}
pub fn read_explicit() -> Result<Payload, String> {
    read_probe(false)
}
pub fn read_image_only() -> Result<Payload, String> {
    read_probe(true)
}
fn read_probe(image_only: bool) -> Result<Payload, String> {
    if let Some(image) = crate::clipboard_native::native_image_read() {
        return Ok(Payload::Bytes(image.data));
    }
    let temps = ProbeTemps::new()?;
    let output_path = temps.0.join("output");
    let mut output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&output_path)
        .map_err(|_| "cannot create clipboard response file")?;
    std::fs::remove_file(output_path).map_err(|_| "cannot unlink clipboard response")?;
    let script = if image_only {
        image_osascript(&temps)
    } else {
        attachments_osascript(Some(&temps))
    };
    let mut command = Command::new("/usr/bin/osascript");
    command
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            output
                .try_clone()
                .map_err(|_| "clipboard response unavailable")?,
        ))
        .stderr(Stdio::null())
        .process_group(0);
    // Limit only this child. Never request keyboard/window permissions.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: (crate::attachments::MAX_FILE_BYTES + 1) as libc::rlim_t,
                rlim_max: (crate::attachments::MAX_FILE_BYTES + 1) as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|_| "clipboard helper unavailable")?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|_| "clipboard helper failed")? {
            if !status.success() {
                return Err("clipboard image read failed; no image uploaded".into());
            }
            break;
        }
        if started.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("clipboard helper timed out; no image uploaded".into());
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    output
        .seek(SeekFrom::Start(0))
        .map_err(|_| "clipboard response unavailable")?;
    let mut raw = String::new();
    output
        .take(16385)
        .read_to_string(&mut raw)
        .map_err(|_| "invalid clipboard response")?;
    if raw.len() > 16384 {
        return Err("clipboard file list too large".into());
    }
    if image_only {
        raw = format!("{FURL_MARKER}\nnone\n{IMAGE_MARKER}\nIMAGE:{}", raw.trim());
    }
    let (paths, image) = raw
        .split_once(IMAGE_MARKER)
        .ok_or("invalid clipboard response")?;
    let paths = paths.trim().strip_prefix(FURL_MARKER).unwrap_or("").trim();
    if paths != "none" && !paths.is_empty() {
        let paths = paths
            .lines()
            .map(|s| PathBuf::from(s.trim()))
            .collect::<Vec<_>>();
        if paths.len() > 10 {
            return Err("at most ten images may be pasted".into());
        }
        return Ok(Payload::Paths(paths));
    }
    if image
        .trim()
        .strip_prefix("IMAGE:")
        .is_some_and(|s| matches!(s, "PNGf" | "TIFF" | "JPEG"))
    {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(temps.path())
            .map_err(|_| "clipboard image unavailable")?;
        if !f
            .metadata()
            .map_err(|_| "clipboard image unavailable")?
            .is_file()
        {
            return Err("clipboard image is not a file".into());
        }
        let mut bytes = Vec::new();
        f.take(crate::attachments::MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "cannot read clipboard image")?;
        return Ok(Payload::Bytes(bytes));
    }
    Ok(Payload::None)
}
