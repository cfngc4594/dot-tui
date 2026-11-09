use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
const TARGET: &str = "aarch64-apple-darwin";
const ROOT_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    ".gitignore",
    "install.sh",
    "README.md",
    "LICENSE",
    "THIRD_PARTY_NOTICES.md",
    "SECURITY.md",
    "UI_PORT.md",
    "IMAGE_PORT.md",
];
fn scan(dir: &Path, root: &Path, names: &mut BTreeSet<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if ["target", "dist", ".git", "__pycache__"].contains(&name.as_ref())
            || name.starts_with("target-")
            || name == ".DS_Store"
            || ["pyc", "pyo", "log"]
                .iter()
                .any(|ext| path.extension().is_some_and(|s| s == *ext))
        {
            continue;
        }
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            bail!("Publication source cannot contain symlinks");
        }
        if meta.is_dir() {
            scan(&path, root, names)?;
        } else if meta.is_file() {
            names.insert(
                path.strip_prefix(root)?
                    .to_str()
                    .context("Non UTF-8 source path")?
                    .to_owned(),
            );
        }
    }
    Ok(())
}
fn source_hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut names: BTreeSet<String> = ROOT_FILES.iter().map(|s| s.to_string()).collect();
    for folder in ["src", "vendor", "licenses", "tools", "docs", ".github"] {
        scan(&root.join(folder), root, &mut names)?;
    }
    let mut hashes = BTreeMap::new();
    for name in names {
        if name.contains(['\n', '\r']) {
            bail!("Publication names cannot contain line breaks");
        }
        let p = root.join(&name);
        if fs::symlink_metadata(&p)?.file_type().is_symlink() {
            bail!("Publication source cannot contain symlinks");
        }
        hashes.insert(name, crate::sha(&fs::read(p)?));
    }
    Ok(hashes)
}
fn remap(root: &Path) -> Result<(BTreeMap<String, String>, Vec<Vec<u8>>)> {
    let mut vars: BTreeMap<String, String> = env::vars().collect();
    vars.insert("MACOSX_DEPLOYMENT_TARGET".into(), "11.0".into());
    let home = PathBuf::from(env::var_os("HOME").context("HOME is required")?);
    let cargo = vars
        .get("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cargo"));
    let sysroot =
        PathBuf::from(crate::output(Command::new("rustc").args(["--print", "sysroot"]))?.trim());
    let mut mappings = vec![
        (home, "/build/user"),
        (root.to_path_buf(), "/build/dot-tui"),
        (cargo, "/build/cargo"),
        (sysroot, "/build/rust"),
    ];
    if let Some(tmp) = vars.get("TMPDIR") {
        mappings.push((PathBuf::from(tmp), "/build/tmp"));
    }
    let mut flags = if let Some(flags) = vars
        .get("CARGO_ENCODED_RUSTFLAGS")
        .filter(|s| !s.is_empty())
    {
        flags.split('\x1f').map(str::to_owned).collect()
    } else {
        shell_words::split(vars.get("RUSTFLAGS").map_or("", String::as_str))?
    };
    let mut private = BTreeSet::new();
    let mut native = Vec::new();
    for (path, logical) in mappings {
        let lexical = std::path::absolute(&path)?;
        let resolved = path.canonicalize()?;
        for p in [lexical, resolved].into_iter().collect::<BTreeSet<_>>() {
            let p = p.to_str().context("Build path must be UTF-8")?;
            private.insert(p.as_bytes().to_vec());
            flags.push(format!("--remap-path-prefix={p}={logical}"));
            native.push(format!("-ffile-prefix-map={p}={logical}"));
        }
    }
    vars.insert("CARGO_ENCODED_RUSTFLAGS".into(), flags.join("\x1f"));
    vars.remove("RUSTFLAGS");
    vars.insert("CC_SHELL_ESCAPED_FLAGS".into(), "1".into());
    for key in ["CFLAGS", "CXXFLAGS"] {
        vars.insert(
            key.into(),
            format!(
                "{} {}",
                vars.get(key).map_or("", String::as_str),
                shell_words::join(&native)
            )
            .trim()
            .to_owned(),
        );
    }
    Ok((vars, private.into_iter().collect()))
}
fn no_private_paths(binary: &[u8], prefixes: &[Vec<u8>]) -> Result<()> {
    if prefixes
        .iter()
        .any(|p| !p.is_empty() && binary.windows(p.len()).any(|v| v == p))
    {
        bail!("Private build directory remains in executable; no package created");
    }
    for prefix in [b"/Users/".as_slice(), b"/home/".as_slice()] {
        for (i, _) in binary
            .windows(prefix.len())
            .enumerate()
            .filter(|(_, v)| *v == prefix)
        {
            let tail = &binary[i + prefix.len()..];
            let n = tail
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || b"_.-".contains(c))
                .count();
            if n > 0 && tail.get(n) == Some(&b'/') {
                bail!("A user home path remains in executable; no package created");
            }
        }
    }
    Ok(())
}
fn archive(path: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let gzip = flate2::GzBuilder::new()
        .mtime(0)
        .write(fs::File::create(path)?, flate2::Compression::default());
    let mut tar = tar::Builder::new(gzip);
    for (name, data) in files {
        let mut h = tar::Header::new_ustar();
        h.set_size(data.len() as u64);
        h.set_mode(if name == "dot-tui" { 0o755 } else { 0o644 });
        h.set_mtime(0);
        h.set_uid(0);
        h.set_gid(0);
        h.set_cksum();
        tar.append_data(&mut h, name, data.as_slice())?;
    }
    tar.into_inner()?.finish()?.sync_all()?;
    Ok(())
}
// Resolve the existing prefix so a symlinked output parent cannot enter sources.
fn resolved_future_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().context("Output needs a parent")?;
    let name = path.file_name().context("Output needs a directory name")?;
    Ok(resolved_future_path(parent)?.join(name))
}
fn validate_output(root: &Path, output: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let resolved = resolved_future_path(output)?;
    for folder in [
        "src", "vendor", "licenses", "tools", "docs", ".github", ".git",
    ] {
        if resolved.starts_with(root.join(folder)) {
            bail!(
                "Output must not be inside publication source directories; use dist or an external directory"
            );
        }
    }
    Ok(())
}
pub fn build(root: &Path, output: Option<PathBuf>, offline: bool) -> Result<()> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        bail!("Build this release natively on Apple Silicon macOS");
    }
    // Also reject a translated x86 invocation if this tool is copied between hosts.
    if crate::output(Command::new("uname").arg("-m"))?.trim() != "arm64" {
        bail!("Native arm64 host required");
    }
    let package: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let version = package["package"]["version"]
        .as_str()
        .context("Missing package version")?;
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        bail!("Release version must contain three numeric components");
    }
    let installer = fs::read(root.join("install.sh"))?;
    if !String::from_utf8_lossy(&installer).contains(&format!("DEFAULT_VERSION={version}\n")) {
        bail!("Installer default and Cargo package version differ");
    }
    let output = std::path::absolute(
        output.unwrap_or_else(|| root.join("dist").join(format!("v{version}"))),
    )?;
    if fs::symlink_metadata(&output).is_ok() {
        bail!("Output already exists; use a fresh directory");
    }
    validate_output(root, &output)?;
    let (vars, private) = remap(root)?;
    let mut build = Command::new("cargo");
    build.current_dir(root).env_clear().envs(&vars).args([
        "build",
        "--release",
        "--locked",
        "--target",
        TARGET,
    ]);
    if offline {
        build.arg("--offline");
    }
    crate::run(&mut build)?;
    let target = vars
        .get("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    let binary = target.join(TARGET).join("release/dot-tui");
    crate::run(
        Command::new("/usr/bin/lipo")
            .arg(&binary)
            .args(["-verify_arch", "arm64"]),
    )?;
    crate::run(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict"])
            .arg(&binary),
    )?;
    let mut metadata = Command::new("cargo");
    metadata.current_dir(root).env_clear().envs(&vars).args([
        "metadata",
        "--locked",
        "--format-version=1",
        "--filter-platform",
        TARGET,
    ]);
    if offline {
        metadata.arg("--offline");
    }
    let metadata = serde_json::from_str(&crate::output(&mut metadata)?)?;
    let (notices, inventory) = crate::licenses::collect(&metadata, root)?;
    let binary = fs::read(binary)?;
    no_private_paths(&binary, &private)?;
    let mut files = BTreeMap::from([
        ("dot-tui".into(), binary.clone()),
        ("VERSION".into(), format!("{version}\n").into_bytes()),
        ("THIRD_PARTY_LICENSES.txt".into(), notices.into_bytes()),
    ]);
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md"] {
        files.insert(name.into(), fs::read(root.join(name))?);
    }
    let asset = format!("dot-tui-v{version}-{TARGET}.tar.gz");
    fs::create_dir_all(output.parent().context("Output parent missing")?)?;
    let temp = tempfile::Builder::new()
        .prefix(".dot-tui-release-")
        .tempdir_in(output.parent().unwrap())?;
    let stage = temp.path().join("bundle");
    fs::create_dir(&stage)?;
    archive(&stage.join(&asset), &files)?;
    fs::write(stage.join("install.sh"), installer)?;
    let sources = source_hashes(root)?;
    fs::write(
        stage.join("SOURCE_FILES.txt"),
        sources.keys().map(|s| format!("{s}\n")).collect::<String>(),
    )?;
    let mut sums = BTreeMap::new();
    for name in [&asset, "install.sh", "SOURCE_FILES.txt"] {
        sums.insert(name.to_owned(), crate::sha(&fs::read(stage.join(name))?));
    }
    let manifest = json!({"version":version,"target":TARGET,"minimum_macos_declared":"11.0","binary_sha256":crate::sha(&binary),"assets_sha256":sums,"rustc":crate::output(Command::new("rustc").arg("--version"))?.trim(),"cargo":crate::output(Command::new("cargo").arg("--version"))?.trim(),"license_inventory":inventory,"source_sha256":sources,"private_build_paths_scan_passed":true,"path_remapping":"Rust and native compiler paths remapped to /build roots; no private input paths recorded","program_launched":false,"tests_run":false,"published":false,"signature":"locally verified; current build uses ad-hoc signature, not Developer ID identity"});
    fs::write(
        stage.join("release-manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    sums.insert(
        "release-manifest.json".into(),
        crate::sha(&fs::read(stage.join("release-manifest.json"))?),
    );
    fs::write(
        stage.join("SHA256SUMS"),
        sums.iter()
            .map(|(name, sum)| format!("{sum}  {name}\n"))
            .collect::<String>(),
    )?;
    if fs::symlink_metadata(&output).is_ok() {
        bail!("Output appeared during build; refusing replacement");
    }
    fs::rename(&stage, &output)?;
    println!(
        "Prepared locally: {}\n{}  dot-tui\nNo release was uploaded or published; no installer or application was run.",
        output.display(),
        crate::sha(&binary)
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_rejects_source_subdirectories_including_symlinked_parent() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("tools")).unwrap();
        assert!(validate_output(d.path(), &d.path().join("tools/future/release")).is_err());
        std::os::unix::fs::symlink(d.path().join("tools"), d.path().join("alias")).unwrap();
        assert!(validate_output(d.path(), &d.path().join("alias/release")).is_err());
        assert!(validate_output(d.path(), &d.path().join("dist/v1.0.0")).is_ok());
    }
    #[test]
    fn private_path_scan_fails_closed() {
        assert!(no_private_paths(b"/Users/alice/work", &[]).is_err());
        assert!(no_private_paths(b"/home/bob/src", &[]).is_err());
        assert!(no_private_paths(b"xxx/private/root/yyy", &[b"/private/root".to_vec()]).is_err());
        assert!(no_private_paths(b"/build/dot-tui/src", &[]).is_ok());
    }
    #[test]
    fn archives_are_deterministic_and_flat() {
        let d = tempfile::tempdir().unwrap();
        let files = BTreeMap::from([
            ("dot-tui".into(), b"binary".to_vec()),
            ("VERSION".into(), b"1.2.3\n".to_vec()),
        ]);
        let a = d.path().join("a");
        let b = d.path().join("b");
        archive(&a, &files).unwrap();
        archive(&b, &files).unwrap();
        assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
        let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(a).unwrap()));
        for e in ar.entries().unwrap() {
            let e = e.unwrap();
            assert_eq!(e.header().mtime().unwrap(), 0);
            assert_eq!(e.header().uid().unwrap(), 0);
            assert_eq!(e.path().unwrap().components().count(), 1);
        }
    }
    #[test]
    fn inventory_excludes_build_caches_and_rejects_symlinks() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("tools/xtask/target")).unwrap();
        fs::write(d.path().join("tools/xtask/target/secret"), b"x").unwrap();
        fs::write(d.path().join("tools/a.rs"), b"x").unwrap();
        let mut names = BTreeSet::new();
        scan(&d.path().join("tools"), d.path(), &mut names).unwrap();
        assert_eq!(names, BTreeSet::from(["tools/a.rs".into()]));
        std::os::unix::fs::symlink("a.rs", d.path().join("tools/link")).unwrap();
        assert!(scan(&d.path().join("tools"), d.path(), &mut names).is_err());
    }
}
