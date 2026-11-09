//! New inode + same-directory rename. Never truncate a signed executable in place.
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::Command,
};
fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut sha = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
    }
    Ok(format!("{:x}", sha.finalize()))
}
fn verify(path: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        crate::run(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict", "--verbose=2"])
                .arg(path),
        )?;
    }
    Ok(())
}
fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & 0x400 != 0
        }
        #[cfg(not(windows))]
        {
            m.file_type().is_symlink()
        }
    })
}
pub fn publish(source: &Path, destination: &Path) -> Result<serde_json::Value> {
    publish_with(source, destination, verify)
}
fn publish_with(
    source: &Path,
    destination: &Path,
    verify: impl Fn(&Path) -> Result<()>,
) -> Result<serde_json::Value> {
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let source = source.canonicalize()?;
    let destination = std::path::absolute(destination)?;
    if source == destination || is_link(&destination) {
        bail!("Use separate source and non-symlink destination");
    }
    fs::create_dir_all(destination.parent().context("Destination needs a parent")?)?;
    #[cfg(unix)]
    let previous = fs::metadata(&destination).ok().map(|m| m.ino());
    #[cfg(not(unix))]
    let previous: Option<u64> = None;
    let expected = digest(&source)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".dot-tui-publish-")
        .tempfile_in(destination.parent().unwrap())?;
    std::io::copy(&mut fs::File::open(&source)?, &mut staged)?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    #[cfg(unix)]
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    if digest(staged.path())? != expected {
        bail!("Staged executable differs from source");
    }
    verify(staged.path())?;
    // Recheck the destination immediately before replacement; never follow its symlink.
    if is_link(&destination) {
        bail!("Symlink destination refused");
    }
    staged.persist(&destination).map_err(|e| e.error)?;
    verify(&destination)?;
    #[cfg(unix)]
    let current = Some(fs::metadata(&destination)?.ino());
    #[cfg(not(unix))]
    let current: Option<u64> = None;
    if previous.is_some() && previous == current {
        bail!("Executable inode did not change");
    }
    Ok(
        serde_json::json!({"source":source,"destination":destination,"sha256":digest(&destination)?,"previous_inode":previous,"current_inode":current,"signature_verified":cfg!(target_os="macos"),"program_launched":false}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_destination_has_verified_bytes_and_failed_verification_preserves_it() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dst = d.path().join("dst");
        fs::write(&src, b"first").unwrap();
        let result = publish_with(&src, &dst, |_| Ok(())).unwrap();
        assert_eq!(result["sha256"], crate::sha(b"first"));
        assert_eq!(fs::read(&dst).unwrap(), b"first");
        fs::write(&src, b"second").unwrap();
        assert!(publish_with(&src, &dst, |_| bail!("rejected")).is_err());
        assert_eq!(fs::read(&dst).unwrap(), b"first");
        publish_with(&src, &dst, |_| Ok(())).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"second");
    }

    #[test]
    #[cfg(unix)]
    fn replaces_inode_and_preserves_old_open_file() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dst = d.path().join("dst");
        fs::write(&src, b"new").unwrap();
        fs::write(&dst, b"old").unwrap();
        let mut old = fs::File::open(&dst).unwrap();
        let result = publish_with(&src, &dst, |_| Ok(())).unwrap();
        assert_ne!(result["previous_inode"], result["current_inode"]);
        assert_eq!(fs::read(&dst).unwrap(), b"new");
        let mut bytes = Vec::new();
        old.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"old");
    }
    #[test]
    #[cfg(unix)]
    fn verification_failure_and_symlink_leave_destination_untouched() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dst = d.path().join("dst");
        fs::write(&src, b"new").unwrap();
        fs::write(&dst, b"old").unwrap();
        assert!(publish_with(&src, &dst, |_| bail!("invalid signature")).is_err());
        assert_eq!(fs::read(&dst).unwrap(), b"old");
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&dst, &link).unwrap();
        assert!(publish_with(&src, &link, |_| Ok(())).is_err());
    }
}
