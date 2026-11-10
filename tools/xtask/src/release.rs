//! Verify and combine native CI artifacts without credentials or network access.
use anyhow::{Context, Result, bail, ensure};
use std::{collections::BTreeMap, fs, path::Path};

fn package_version(root: &Path) -> Result<String> {
    let package: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let version = package["package"]["version"]
        .as_str()
        .context("Missing package version")?;
    ensure!(
        version.split('.').count() == 3
            && version.split('.').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|b| b.is_ascii_digit())
                    && (part == "0" || !part.starts_with('0'))
            }),
        "Release version must be MAJOR.MINOR.PATCH"
    );
    Ok(version.to_owned())
}

pub fn check_tag(root: &Path, tag: &str) -> Result<String> {
    let version = package_version(root)?;
    ensure!(tag == format!("v{version}"), "Tag must match v{version}");
    let lock: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.lock"))?)?;
    let locked = lock["package"]
        .as_array()
        .context("Missing locked packages")?
        .iter()
        .find(|p| p["name"].as_str() == Some("dot-tui"))
        .context("Missing dot-tui lock entry")?;
    ensure!(
        locked["version"].as_str() == Some(version.as_str()),
        "Cargo.lock version differs"
    );
    let shell = fs::read_to_string(root.join("install.sh"))?;
    ensure!(
        shell
            .lines()
            .any(|line| line.trim() == format!("DEFAULT_VERSION={version}")),
        "Shell installer version differs"
    );
    let powershell = fs::read_to_string(root.join("install.ps1"))?;
    ensure!(
        powershell.contains(&format!("[string]$Version = '{version}'")),
        "PowerShell installer version differs"
    );
    Ok(version.to_owned())
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "Expected regular file: {}",
        path.display()
    );
    fs::read(path).with_context(|| format!("Read {}", path.display()))
}

fn verify_bundle(
    directory: &Path,
    version: &str,
    target: &str,
    sources: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    ensure!(
        fs::symlink_metadata(directory)?.file_type().is_dir(),
        "Expected native bundle directory"
    );
    let archive = format!("dot-tui-v{version}-{target}.tar.gz");
    let names = [
        archive.as_str(),
        "install.sh",
        "install.ps1",
        "SOURCE_FILES.txt",
        "release-manifest.json",
    ];
    let mut files = BTreeMap::new();
    for name in names {
        files.insert(name.to_owned(), read_file(&directory.join(name))?);
    }
    // Accept only the exact set emitted by build-release, with no unverified extras.
    ensure!(
        fs::read_dir(directory)?.count() == files.len() + 1,
        "Unexpected bundle files"
    );
    let sums = files
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", crate::sha(bytes)))
        .collect::<String>();
    ensure!(
        read_file(&directory.join("SHA256SUMS"))? == sums.as_bytes(),
        "Bundle checksums differ: {target}"
    );
    let manifest: serde_json::Value = serde_json::from_slice(&files["release-manifest.json"])?;
    ensure!(
        manifest["version"].as_str() == Some(version),
        "Bundle version differs"
    );
    ensure!(
        manifest["target"].as_str() == Some(target),
        "Bundle target differs"
    );
    ensure!(
        manifest["private_build_paths_scan_passed"] == true,
        "Private build path scan missing"
    );
    ensure!(
        manifest["source_sha256"] == serde_json::to_value(sources)?,
        "Bundle sources differ from release checkout"
    );
    let asset_sums = files
        .iter()
        .filter(|(name, _)| name.as_str() != "release-manifest.json")
        .map(|(name, bytes)| (name.clone(), crate::sha(bytes)))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        manifest["assets_sha256"] == serde_json::to_value(asset_sums)?,
        "Manifest asset hashes differ"
    );
    let inventory = sources
        .keys()
        .map(|name| format!("{name}\n"))
        .collect::<String>();
    ensure!(
        files["SOURCE_FILES.txt"] == inventory.as_bytes(),
        "Source inventory differs"
    );
    for installer in ["install.sh", "install.ps1"] {
        ensure!(
            sources.get(installer) == Some(&crate::sha(&files[installer])),
            "Installer differs from source"
        );
    }
    let manifest = files
        .remove("release-manifest.json")
        .context("Missing manifest")?;
    files.insert(format!("release-manifest-{target}.json"), manifest);
    Ok(files)
}

fn merge_files(
    destination: &mut BTreeMap<String, Vec<u8>>,
    incoming: BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    for (name, bytes) in incoming {
        if let Some(existing) = destination.get(&name) {
            ensure!(
                existing == &bytes,
                "Shared asset differs between targets: {name}"
            );
        } else {
            destination.insert(name, bytes);
        }
    }
    Ok(())
}

pub fn assemble(root: &Path, input: &Path, output: &Path, tag: Option<&str>) -> Result<()> {
    let default_tag = format!("v{}", package_version(root)?);
    let version = check_tag(root, tag.unwrap_or(&default_tag))?;
    ensure!(
        fs::symlink_metadata(output).is_err(),
        "Output already exists"
    );
    let sources = crate::build::source_hashes(root)?;
    let mut files = BTreeMap::new();
    for target in crate::build::TARGETS {
        let bundle = input.join(format!("dot-tui-{target}"));
        merge_files(
            &mut files,
            verify_bundle(&bundle, &version, target, &sources)?,
        )?;
    }
    let sums = files
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", crate::sha(bytes)))
        .collect::<String>();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".dot-tui-assembled-")
        .tempdir_in(parent)?;
    let staged = temporary.path().join("release");
    fs::create_dir(&staged)?;
    for (name, bytes) in &files {
        fs::write(staged.join(name), bytes)?;
    }
    if fs::symlink_metadata(output).is_ok() {
        bail!("Output appeared during assembly");
    }
    fs::rename(staged, output)?;
    println!(
        "Verified all five native targets; prepared {} release assets locally.",
        files.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source_fixture(root: &Path) {
        for (file, content) in [
            (
                "Cargo.toml",
                "[package]\nname = 'dot-tui'\nversion = '1.2.3'\n",
            ),
            (
                "Cargo.lock",
                "[[package]]\nname = 'dot-tui'\nversion = '1.2.3'\n",
            ),
            ("install.sh", "DEFAULT_VERSION=1.2.3\n"),
            ("install.ps1", "[string]$Version = '1.2.3'\n"),
        ] {
            fs::write(root.join(file), content).unwrap();
        }
    }
    fn bundle_fixture(directory: &Path, target: &str) -> BTreeMap<String, String> {
        fs::create_dir_all(directory).unwrap();
        let sources = BTreeMap::from([
            ("install.sh".into(), crate::sha(b"shell")),
            ("install.ps1".into(), crate::sha(b"powershell")),
        ]);
        let mut files = BTreeMap::from([
            (
                format!("dot-tui-v1.2.3-{target}.tar.gz"),
                b"archive".to_vec(),
            ),
            ("install.sh".into(), b"shell".to_vec()),
            ("install.ps1".into(), b"powershell".to_vec()),
            (
                "SOURCE_FILES.txt".into(),
                b"install.ps1\ninstall.sh\n".to_vec(),
            ),
        ]);
        let assets = files
            .iter()
            .map(|(name, bytes)| (name.clone(), crate::sha(bytes)))
            .collect::<BTreeMap<_, _>>();
        let manifest = serde_json::json!({"version":"1.2.3", "target":target, "source_sha256":sources,
            "assets_sha256":assets, "private_build_paths_scan_passed":true});
        files.insert(
            "release-manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let sums = files
            .iter()
            .map(|(name, bytes)| format!("{}  {name}\n", crate::sha(bytes)))
            .collect::<String>();
        files.insert("SHA256SUMS".into(), sums.into_bytes());
        for (name, bytes) in files {
            fs::write(directory.join(name), bytes).unwrap();
        }
        sources
    }
    #[test]
    fn tag_and_all_version_sources_must_match() {
        let dir = tempfile::tempdir().unwrap();
        source_fixture(dir.path());
        assert_eq!(check_tag(dir.path(), "v1.2.3").unwrap(), "1.2.3");
        for tag in ["v1.2.4", "1.2.3", "v1.2.3-rc1", "v1.2.3\n"] {
            assert!(check_tag(dir.path(), tag).is_err());
        }
        for file in ["Cargo.lock", "install.sh", "install.ps1"] {
            source_fixture(dir.path());
            let text = fs::read_to_string(dir.path().join(file))
                .unwrap()
                .replace("1.2.3", "1.2.4");
            fs::write(dir.path().join(file), text).unwrap();
            assert!(check_tag(dir.path(), "v1.2.3").is_err());
        }
    }
    #[test]
    fn bundle_verifies_hashes_target_version_and_sources() {
        let dir = tempfile::tempdir().unwrap();
        let target = crate::build::TARGETS[0];
        let sources = bundle_fixture(dir.path(), target);
        let files = verify_bundle(dir.path(), "1.2.3", target, &sources).unwrap();
        assert!(files.contains_key(&format!("release-manifest-{target}.json")));
        assert!(!files.contains_key("release-manifest.json"));
        assert!(verify_bundle(dir.path(), "1.2.4", target, &sources).is_err());
        assert!(verify_bundle(dir.path(), "1.2.3", crate::build::TARGETS[1], &sources).is_err());
        assert!(verify_bundle(dir.path(), "1.2.3", target, &BTreeMap::new()).is_err());
        fs::write(dir.path().join("install.sh"), b"changed").unwrap();
        assert!(verify_bundle(dir.path(), "1.2.3", target, &sources).is_err());
    }
    #[test]
    fn unexpected_files_and_missing_bundles_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let target = crate::build::TARGETS[0];
        let sources = bundle_fixture(dir.path(), target);
        fs::write(dir.path().join("unexpected.txt"), b"extra").unwrap();
        assert!(verify_bundle(dir.path(), "1.2.3", target, &sources).is_err());
        assert!(verify_bundle(&dir.path().join("missing"), "1.2.3", target, &sources).is_err());
    }
    #[test]
    fn combine_preserves_all_archives_and_manifests_and_rejects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = BTreeMap::new();
        for target in crate::build::TARGETS {
            let bundle = dir.path().join(target);
            let sources = bundle_fixture(&bundle, target);
            merge_files(
                &mut files,
                verify_bundle(&bundle, "1.2.3", target, &sources).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(files.len(), 13); // Five archives, five manifests and three shared files.
        assert_eq!(
            files
                .keys()
                .filter(|name| name.ends_with(".tar.gz"))
                .count(),
            5
        );
        assert!(
            merge_files(
                &mut files,
                BTreeMap::from([("install.sh".into(), b"different".to_vec())])
            )
            .is_err()
        );
    }
    #[test]
    #[cfg(unix)]
    fn symlink_bundle_files_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let target = crate::build::TARGETS[0];
        let sources = bundle_fixture(dir.path(), target);
        fs::remove_file(dir.path().join("install.sh")).unwrap();
        std::os::unix::fs::symlink("install.ps1", dir.path().join("install.sh")).unwrap();
        assert!(verify_bundle(dir.path(), "1.2.3", target, &sources).is_err());
    }
}
