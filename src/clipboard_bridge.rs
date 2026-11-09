//! ClipboardProvider adapter. Source: Grok Build 2bdd1d6, pager-render clipboard
//! SystemClipboard and xai-grok-shared clipboard macOS text paths; Apache-2.0.
//! Text provider; explicit image paste uses image_clipboard/image_ingress.
//! Dot omits OSC52/tmux/telemetry and uses one shared injectable text provider.
//! Constructing the system provider does not read or write the pasteboard.
use std::{cell::RefCell, rc::Rc};
use xai_ratatui_textarea::{ClipboardProvider, InternalClipboard};
#[derive(Clone, Debug)]
pub struct Clipboard(Rc<RefCell<Box<dyn ClipboardProvider>>>);
impl Default for Clipboard {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(
            Box::new(InternalClipboard::default()),
        )))
    }
}
impl Clipboard {
    pub fn replace(&self, provider: Box<dyn ClipboardProvider>) {
        *self.0.borrow_mut() = provider;
    }
    pub fn get(&self) -> Option<String> {
        self.0.borrow_mut().get()
    }
    pub fn provider(&self) -> Box<dyn ClipboardProvider> {
        Box::new(self.clone())
    }
}
impl ClipboardProvider for Clipboard {
    fn get(&mut self) -> Option<String> {
        self.0.borrow_mut().get()
    }
    fn set(&mut self, text: &str) {
        self.0.borrow_mut().set(text);
    }
}
#[derive(Debug)]
pub struct SystemClipboard;
impl ClipboardProvider for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            macos::get_text().ok().flatten()
        }
        #[cfg(not(target_os = "macos"))]
        {
            crate::clipboard_portable::get_text().ok().flatten()
        }
    }
    fn set(&mut self, text: &str) {
        #[cfg(target_os = "macos")]
        {
            let _ = macos::set_text(text);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = crate::clipboard_portable::set_text(text);
        }
    }
}
#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use std::{
        fs::{File, OpenOptions},
        io::{self, Read, Seek, SeekFrom, Write},
        os::unix::{fs::OpenOptionsExt, process::CommandExt},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    // Original 2s clipboard-child deadline/15ms wait cadence. Plain text only.
    fn wait_with_deadline(
        child: &mut Child,
        deadline: Duration,
    ) -> io::Result<std::process::ExitStatus> {
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if start.elapsed() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Clipboard helper timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    // Same unlinked regular-file spooling contract as upstream NamedTempFile;
    // no added dependency. 0600 + create_new + NOFOLLOW, immediately unlinked.
    fn spool() -> io::Result<File> {
        let path = std::env::temp_dir().join(format!("dot-clipboard-{}", uuid::Uuid::new_v4()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        std::fs::remove_file(path)?;
        Ok(file)
    }
    pub(crate) fn get_text() -> io::Result<Option<String>> {
        let mut output = spool()?;
        let mut command = Command::new("/usr/bin/pbpaste");
        command
            .arg("-Prefer")
            .arg("txt")
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.try_clone()?))
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn()?;
        if !wait_with_deadline(&mut child, Duration::from_secs(2))?.success() {
            return Err(io::Error::other("Clipboard text helper failed"));
        }
        output.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        output.take(1024 * 1024).read_to_end(&mut bytes)?;
        if bytes.is_empty() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }
    pub(super) fn set_text(text: &str) -> io::Result<()> {
        let mut input = spool()?;
        input.write_all(text.as_bytes())?;
        input.seek(SeekFrom::Start(0))?;
        let mut command = Command::new("/usr/bin/pbcopy");
        command
            .stdin(Stdio::from(input))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn()?;
        if wait_with_deadline(&mut child, Duration::from_secs(2))?.success() {
            Ok(())
        } else {
            Err(io::Error::other("Clipboard helper failed"))
        }
    }
}
