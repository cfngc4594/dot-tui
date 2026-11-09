//! Collect resolved normal/build dependency licenses without executing dependencies.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing string: {key}"))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .with_context(|| format!("Missing array: {key}"))
}
// Match Python read_text's universal newline handling for existing notice output.
fn text(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}
fn children(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = fs::read_dir(path)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    result.sort();
    Ok(result)
}
fn recursive_files(path: &Path, result: &mut BTreeSet<PathBuf>) -> Result<()> {
    for child in children(path)? {
        let metadata = fs::symlink_metadata(&child)?;
        if child.is_file() {
            result.insert(child.canonicalize()?);
        } else if metadata.is_dir() {
            recursive_files(&child, result)?;
        }
    }
    Ok(())
}
fn is_notice(name: &str) -> bool {
    ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"]
        .iter()
        .any(|p| name.starts_with(p))
        || name.to_ascii_lowercase().starts_with("acknowledg")
}
fn relative(path: &Path, base: &Path) -> Result<String> {
    let path = path
        .strip_prefix(base)
        .context("License path outside package")?;
    // Publication metadata is portable, independent of host path separators.
    path.iter()
        .map(|part| part.to_str().context("License path is not UTF-8"))
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

pub fn collect(metadata: &Value, root: &Path) -> Result<(String, Value)> {
    let root = root.canonicalize()?;
    let packages: BTreeMap<_, _> = array(metadata, "packages")?
        .iter()
        .map(|p| Ok((string(p, "id")?, p)))
        .collect::<Result<_>>()?;
    let resolve = metadata
        .get("resolve")
        .context("Missing dependency resolution")?;
    let nodes: BTreeMap<_, _> = array(resolve, "nodes")?
        .iter()
        .map(|p| Ok((string(p, "id")?, p)))
        .collect::<Result<_>>()?;
    let root_id = string(resolve, "root")?;
    let mut pending = vec![root_id];
    let mut selected = BTreeSet::new();
    while let Some(key) = pending.pop() {
        if !selected.insert(key) {
            continue;
        }
        let node = nodes.get(key).context("Missing dependency node")?;
        for dep in array(node, "deps")? {
            if array(dep, "dep_kinds")?
                .iter()
                .any(|k| k.get("kind").is_none_or(Value::is_null) || k["kind"] == "build")
            {
                pending.push(string(dep, "pkg")?);
            }
        }
    }
    let mut selected = selected
        .into_iter()
        .map(|id| {
            let package = packages.get(id).context("Missing package metadata")?;
            Ok((
                string(package, "name")?,
                string(package, "version")?,
                id,
                *package,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    selected.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    let mut sections = vec!["dot-tui bundled third-party license texts\nIncludes direct/transitive runtime and build dependencies in the resolved target graph.\nLicense expressions are package metadata, not a claim that all listed alternatives apply.\n".to_owned()];
    let mut inventory = Vec::new();
    for (name, version, id, package) in selected {
        if id == root_id {
            continue;
        }
        let manifest = Path::new(string(package, "manifest_path")?);
        let mut directory = manifest
            .parent()
            .context("Manifest lacks parent")?
            .canonicalize()?;
        let mut paths = BTreeSet::new();
        if let Some(file) = package
            .get("license_file")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            paths.insert(directory.join(file).canonicalize()?);
        }
        for path in children(&directory)? {
            if path.is_file()
                && path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(is_notice)
            {
                paths.insert(path.canonicalize()?);
            }
        }
        for folder in ["licenses", "LICENSES"] {
            recursive_files(&directory.join(folder), &mut paths)?;
        }
        if paths.is_empty() {
            let override_dir = root
                .join("licenses/crates")
                .join(format!("{name}-{version}"));
            ensure!(
                override_dir.is_dir(),
                "Missing license texts: {name} {version}"
            );
            directory = override_dir.canonicalize()?;
            for path in children(&directory)? {
                if path.is_file()
                    && path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| n.starts_with("LICENSE"))
                {
                    paths.insert(path.canonicalize()?);
                }
            }
            ensure!(
                !paths.is_empty(),
                "Empty license override: {name} {version}"
            );
        }
        let license = package
            .get("license")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("see license file");
        let source = package
            .get("repository")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                package
                    .get("source")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("vendored source");
        let mut texts = Vec::new();
        let mut files = Vec::new();
        for path in paths {
            let file = relative(&path, &directory)
                .with_context(|| format!("License path outside package: {name}"))?;
            texts.push(format!("--- {file} ---\n{}", text(&path)?));
            files.push(file);
        }
        sections.push(format!(
            "{}\n{name} {version}\nSPDX: {license}\nSource: {source}\n\n{}",
            "=".repeat(72),
            texts.join("\n\n")
        ));
        inventory.push(json!({"name":name,"version":version,"license":package.get("license").unwrap_or(&Value::Null),"files":files}));
    }
    for path in children(&root.join("licenses"))? {
        if path.is_file() {
            sections.push(format!(
                "{}\n{}\n\n{}",
                "=".repeat(72),
                relative(&path, &root)?,
                text(&path)?
            ));
        }
    }
    let provenance = root.join("licenses/crates/PROVENANCE.json");
    if provenance.is_file() {
        sections.push(format!(
            "Exact-version supplemental license sources\n{}",
            text(&provenance)?
        ));
    }
    let asset_root = root.join("licenses/assets").canonicalize()?;
    let asset_provenance = asset_root.join("PROVENANCE.json");
    let provenance_text = text(&asset_provenance)?;
    let assets: Value = serde_json::from_str(&provenance_text)?;
    let required = array(&assets, "required_packages")?;
    let asset_packages = assets
        .get("packages")
        .and_then(Value::as_object)
        .context("Missing asset packages")?;
    for package in &mut inventory {
        let name = string(package, "name")?;
        let key = format!("{name}-{}", string(package, "version")?);
        let record = asset_packages.get(&key);
        if required.iter().any(|v| v.as_str() == Some(name)) && record.is_none() {
            bail!("Missing exact-version embedded asset notices: {key}");
        }
        let Some(record) = record else {
            continue;
        };
        let files = record
            .get("files")
            .and_then(Value::as_object)
            .context("Missing embedded asset files")?;
        let mut names = files.keys().collect::<Vec<_>>();
        names.sort();
        let mut notice_files = Vec::new();
        for name in names {
            let info = &files[name];
            let path = asset_root.join(&key).join(name).canonicalize()?;
            ensure!(
                path.starts_with(&asset_root),
                "Asset notice path escapes its directory"
            );
            let data = fs::read(&path)?;
            ensure!(
                format!("{:x}", Sha256::digest(&data)) == string(info, "sha256")?,
                "Embedded asset notice hash differs: {key}/{name}"
            );
            let notice = String::from_utf8(data).context("Embedded asset notice is not UTF-8")?;
            sections.push(format!(
                "{}\nEmbedded assets: {key}/{name}\nSource: {}\nScope: {}\n\n{notice}",
                "=".repeat(72),
                string(info, "source")?,
                string(record, "scope")?
            ));
            notice_files.push(relative(&path, &root)?);
        }
        package["asset_notice_files"] = json!(notice_files);
    }
    sections.push(format!(
        "Embedded asset notice provenance\n{provenance_text}"
    ));
    Ok((sections.join("\n\n") + "\n", Value::Array(inventory)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_relative_paths_use_forward_slashes() {
        let base = PathBuf::from("package");
        let nested = base.join("licenses").join("assets").join("NOTICE");
        assert_eq!(relative(&nested, &base).unwrap(), "licenses/assets/NOTICE");
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = tempfile::Builder::new()
                .prefix("dot-license-test-")
                .tempdir()
                .unwrap()
                .keep();
            fs::create_dir_all(path.join("licenses/assets")).unwrap();
            fs::create_dir_all(path.join("dependency")).unwrap();
            fs::create_dir_all(path.join("development")).unwrap();
            fs::write(
                path.join("licenses/assets/PROVENANCE.json"),
                "{\"required_packages\":[],\"packages\":{}}\n",
            )
            .unwrap();
            fs::write(path.join("dependency/LICENSE"), "license body\r\n").unwrap();
            Self(path)
        }
        fn metadata(&self) -> Value {
            json!({"packages":[{"id":"root","name":"root","version":"1.0.0","manifest_path":self.0.join("Cargo.toml")},{"id":"dep","name":"dependency","version":"1.2.3","license":"MIT","repository":"https://example.test/dependency","manifest_path":self.0.join("dependency/Cargo.toml")},{"id":"dev","name":"development","version":"1.0.0","manifest_path":self.0.join("development/Cargo.toml")}],"resolve":{"root":"root","nodes":[{"id":"root","deps":[{"pkg":"dep","dep_kinds":[{"kind":null}]},{"pkg":"dev","dep_kinds":[{"kind":"dev"}]}]},{"id":"dep","deps":[]},{"id":"dev","deps":[]}]}})
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn normal_and_build_closure_excludes_dev_and_preserves_notice_format() {
        let f = Fixture::new();
        let (out, inventory) = collect(&f.metadata(), &f.0).unwrap();
        assert_eq!(
            inventory,
            json!([{"name":"dependency","version":"1.2.3","license":"MIT","files":["LICENSE"]}])
        );
        assert!(out.contains("dependency 1.2.3\nSPDX: MIT\nSource: https://example.test/dependency\n\n--- LICENSE ---\nlicense body\n"));
        assert!(!out.contains("development"));
        let mut metadata = f.metadata();
        metadata["resolve"]["nodes"][0]["deps"][0]["dep_kinds"][0]["kind"] = json!("build");
        assert_eq!(collect(&metadata, &f.0).unwrap(), (out, inventory));
    }
    #[test]
    fn missing_license_fails_and_only_exact_version_override_works() {
        let f = Fixture::new();
        fs::remove_file(f.0.join("dependency/LICENSE")).unwrap();
        assert!(
            collect(&f.metadata(), &f.0)
                .unwrap_err()
                .to_string()
                .contains("Missing license texts")
        );
        let p = f.0.join("licenses/crates/dependency-1.2.3");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("LICENSE-MIT"), "override").unwrap();
        assert!(collect(&f.metadata(), &f.0).unwrap().0.contains("override"));
    }
    #[test]
    fn required_asset_notices_need_exact_version_and_hash() {
        let f = Fixture::new();
        let provenance = f.0.join("licenses/assets/PROVENANCE.json");
        fs::write(
            &provenance,
            r#"{"required_packages":["dependency"],"packages":{}}"#,
        )
        .unwrap();
        assert!(
            collect(&f.metadata(), &f.0)
                .unwrap_err()
                .to_string()
                .contains("Missing exact-version")
        );
        let dir = f.0.join("licenses/assets/dependency-1.2.3");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("NOTICE"), "assets").unwrap();
        let mut p = json!({"required_packages":["dependency"],"packages":{"dependency-1.2.3":{"scope":"test assets","files":{"NOTICE":{"sha256":"wrong","source":"https://example.test/assets"}}}}});
        fs::write(&provenance, p.to_string()).unwrap();
        assert!(
            collect(&f.metadata(), &f.0)
                .unwrap_err()
                .to_string()
                .contains("hash differs")
        );
        p["packages"]["dependency-1.2.3"]["files"]["NOTICE"]["sha256"] =
            json!(format!("{:x}", Sha256::digest(b"assets")));
        fs::write(&provenance, p.to_string()).unwrap();
        let (out, inventory) = collect(&f.metadata(), &f.0).unwrap();
        assert!(out.contains("Embedded assets: dependency-1.2.3/NOTICE"));
        assert_eq!(
            inventory[0]["asset_notice_files"],
            json!(["licenses/assets/dependency-1.2.3/NOTICE"])
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_license_and_asset_escapes_are_rejected() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        fs::write(f.0.join("outside"), "outside").unwrap();
        fs::remove_file(f.0.join("dependency/LICENSE")).unwrap();
        symlink(f.0.join("outside"), f.0.join("dependency/LICENSE")).unwrap();
        assert!(
            collect(&f.metadata(), &f.0)
                .unwrap_err()
                .to_string()
                .contains("outside package")
        );
        fs::remove_file(f.0.join("dependency/LICENSE")).unwrap();
        fs::write(f.0.join("dependency/LICENSE"), "ok").unwrap();
        let dir = f.0.join("licenses/assets/dependency-1.2.3");
        fs::create_dir(&dir).unwrap();
        symlink(f.0.join("outside"), dir.join("NOTICE")).unwrap();
        fs::write(f.0.join("licenses/assets/PROVENANCE.json"),json!({"required_packages":[],"packages":{"dependency-1.2.3":{"scope":"test","files":{"NOTICE":{"source":"test","sha256":"none"}}}}}).to_string()).unwrap();
        assert!(
            collect(&f.metadata(), &f.0)
                .unwrap_err()
                .to_string()
                .contains("escapes")
        );
    }
}
