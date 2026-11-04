//! Minimal durable ambiguity marker. No message text, account ID or token is stored.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub struct Journal {
    dir: PathBuf,
}
impl Journal {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        #[cfg(not(unix))]
        {
            let _ = dir;
            return Err("Sending requires Unix secure state permissions.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            match fs::create_dir(&dir) {
                Ok(()) => {
                    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                        .map_err(|_| "Cannot secure send state directory")?;
                    sync_dir(dir.parent().ok_or("Send state directory needs a parent")?)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("Cannot create send state directory".into()),
            }
            let m =
                fs::symlink_metadata(&dir).map_err(|_| "Cannot inspect send state directory")?;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o077 != 0
            {
                return Err(
                    "Send state directory must be private, owned by you, and not a symlink".into(),
                );
            }
            Ok(Self { dir })
        }
    }
    fn path(&self) -> PathBuf {
        self.dir.join("pending-send")
    }
    pub fn blocked(&self) -> bool {
        fs::symlink_metadata(self.path()).is_ok()
    }
    pub fn begin(&self, id: &str) -> Result<(), String> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err("Invalid request key".into());
        }
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut f=opts.open(self.path()).map_err(|_|"Pending send exists or state cannot be written. Verify the official app before sending again.")?;
        f.write_all(id.as_bytes())
            .and_then(|_| f.sync_all())
            .map_err(|_| "Cannot persist send state; sending is blocked")?;
        sync_dir(&self.dir)?;
        Ok(())
    }
    /// Clear only the exact request owned by this process, after a definitive local rejection or server acceptance.
    pub fn complete(&self, id: &str) -> Result<(), String> {
        let mut opts = OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let f = opts
            .open(self.path())
            .map_err(|_| "Cannot inspect pending send; remains blocked")?;
        let m = f.metadata().map_err(|_| "Cannot inspect pending send")?;
        if !m.is_file() || m.len() > 64 {
            return Err("Unexpected pending state; remains blocked".into());
        }
        let mut stored = String::new();
        f.take(65)
            .read_to_string(&mut stored)
            .map_err(|_| "Cannot read pending send")?;
        if stored != id {
            return Err("Pending request changed; remains blocked".into());
        }
        fs::remove_file(self.path())
            .map_err(|_| "Cannot clear accepted send marker; remains blocked")?;
        sync_dir(&self.dir)
    }
}
fn sync_dir(dir: &Path) -> Result<(), String> {
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Cannot sync send state directory".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!("dot-send-test-{}", uuid::Uuid::new_v4()))
    }
    #[test]
    fn uncertain_survives_restart_and_no_duplicate() {
        let d = dir();
        let j = Journal::new(d.clone()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        j.begin(&id).unwrap();
        assert!(Journal::new(d.clone()).unwrap().blocked());
        assert!(j.begin(&uuid::Uuid::new_v4().to_string()).is_err());
        assert!(j.complete("wrong").is_err());
        assert!(j.blocked());
        j.complete(&id).unwrap();
        assert!(!j.blocked());
        fs::remove_dir(d).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_directory_rejected() {
        use std::os::unix::fs::symlink;
        let d = dir();
        let l = dir();
        let _j = Journal::new(d.clone()).unwrap();
        symlink(&d, &l).unwrap();
        assert!(Journal::new(l.clone()).is_err());
        fs::remove_file(l).unwrap();
        fs::remove_dir(d).unwrap();
    }
}
