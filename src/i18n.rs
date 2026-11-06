//! UI-only catalog. Callers must never pass transcript or draft message bodies.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Chinese,
}
static LANGUAGE: AtomicU8 = AtomicU8::new(0);
pub fn current() -> Language {
    if LANGUAGE.load(Ordering::Relaxed) == 1 {
        Language::Chinese
    } else {
        Language::English
    }
}
pub fn set(language: Language) {
    LANGUAGE.store(
        if language == Language::Chinese { 1 } else { 0 },
        Ordering::Relaxed,
    );
}
pub fn label(en: &'static str, zh: &'static str) -> &'static str {
    if current() == Language::English {
        en
    } else {
        zh
    }
}
struct Pattern {
    regex: regex::Regex,
    target: String,
}
struct Catalog {
    exact: HashMap<String, String>,
    patterns: Vec<Pattern>,
}
fn pattern(template: &str) -> String {
    let token = regex::Regex::new(r"\{(\d+)\}").unwrap();
    let mut out = String::from("(?s)^");
    let mut at = 0;
    for capture in token.captures_iter(template) {
        let m = capture.get(0).unwrap();
        out.push_str(&regex::escape(&template[at..m.start()]));
        out.push_str(&format!("(?P<v{}>.*?)", &capture[1]));
        at = m.end();
    }
    out.push_str(&regex::escape(&template[at..]));
    out.push('$');
    out
}
fn catalog(chinese: bool) -> &'static Catalog {
    static EN: OnceLock<Catalog> = OnceLock::new();
    static ZH: OnceLock<Catalog> = OnceLock::new();
    (if chinese { &ZH } else { &EN }).get_or_init(|| {
        let entries: Vec<[String; 2]> =
            serde_json::from_str(include_str!("i18n_catalog.json")).expect("bundled UI catalog");
        let mut exact = HashMap::new();
        let mut patterns = Vec::new();
        let mut insert = |source: String, target: String| {
            if source.contains("{0}") {
                patterns.push(Pattern {
                    regex: regex::Regex::new(&pattern(&source)).unwrap(),
                    target,
                });
            } else {
                exact.insert(source, target);
            }
        };
        for [en, zh] in entries {
            if chinese {
                let localized = zh.replace("Open Image", "打开图片");
                // Source strings can already be Chinese while their dynamic
                // error/phase argument is English. Match both templates so
                // only recognized UI arguments are recursively localized.
                insert(zh, localized.clone());
                insert(en, localized);
            } else {
                insert(zh, en.clone());
                if en.contains("{0}") {
                    insert(en.clone(), en);
                }
            }
        }
        Catalog { exact, patterns }
    })
}
pub fn ui(text: &str) -> String {
    translate(text, 3)
}
fn translate(text: &str, depth: usize) -> String {
    let c = catalog(current() == Language::Chinese);
    if let Some(value) = c.exact.get(text) {
        return value.clone();
    }
    if depth > 0 {
        for p in &c.patterns {
            if let Some(caps) = p.regex.captures(text) {
                let token = regex::Regex::new(r"\{(\d+)\}").unwrap();
                return token
                    .replace_all(&p.target, |m: &regex::Captures<'_>| {
                        caps.name(&format!("v{}", &m[1]))
                            .map(|v| translate(v.as_str(), depth - 1))
                            .unwrap_or_default()
                    })
                    .into_owned();
            }
        }
    }
    text.to_owned()
}
pub fn load() -> (Option<PathBuf>, Option<String>) {
    set(Language::English);
    let Some(home) = std::env::var_os("HOME") else {
        return (None, Some("Cannot locate language preferences".into()));
    };
    let path = PathBuf::from(home).join(".dot-tui/language");
    match read(&path) {
        Ok(Some(lang)) => set(lang),
        Ok(None) => {}
        Err(e) => return (Some(path), Some(e)),
    };
    (Some(path), None)
}
fn private(path: &Path, directory: bool) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).map_err(|_| "Cannot inspect language preferences")?;
    if m.file_type().is_symlink()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || (directory && !m.is_dir())
        || (!directory && !m.is_file())
    {
        return Err("Language preferences must be private regular files".into());
    }
    Ok(())
}
fn read(path: &Path) -> Result<Option<Language>, String> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let parent = path.parent().ok_or("Cannot locate language preferences")?;
    match std::fs::symlink_metadata(parent) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect language preferences".into()),
        Ok(_) => private(parent, true)?,
    };
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect language preferences".into()),
        Ok(_) => private(path, false)?,
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "Cannot read language preferences")?;
    let mut text = String::new();
    file.take(33)
        .read_to_string(&mut text)
        .map_err(|_| "Cannot read language preferences")?;
    match text.trim() {
        "en" => Ok(Some(Language::English)),
        "zh" => Ok(Some(Language::Chinese)),
        _ => Err("Invalid language preference; using English".into()),
    }
}
pub fn save(path: &Path, language: Language) -> Result<(), String> {
    use std::{
        io::Write,
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    };
    let parent = path.parent().ok_or("Cannot locate language preferences")?;
    match std::fs::DirBuilder::new().mode(0o700).create(parent) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("Cannot create language preferences".into()),
    };
    private(parent, true)?;
    if path.exists() {
        private(path, false)?;
    }
    let temporary = parent.join(format!(".language-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|_| "Cannot save language preference")?;
        file.write_all(if language == Language::English {
            b"en\n"
        } else {
            b"zh\n"
        })
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot save language preference")?;
        std::fs::rename(&temporary, path).map_err(|_| "Cannot save language preference")?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temporary);
    result
}
