//! Dot private downloaded-image cache backing Grok terminal placement/viewers.
//! No external application is launched. Files remain private and bounded.
use crate::prompt_images::PastedImage;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub const MAX_RESIDENT: usize = 64 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 32;
pub struct Prepared {
    pub image: PastedImage,
    transmission: Option<std::sync::Arc<str>>,
    path: PathBuf,
    dir: PathBuf,
}
impl Prepared {
    pub fn transmission(&self) -> Option<&str> {
        self.transmission.as_deref()
    }
    fn overhead(&self) -> usize {
        metadata_cost(&self.image)
    }
    fn allocations(&self) -> Vec<(usize, usize)> {
        let mut a = Vec::new();
        if let Some(bytes) = &self.image.encoded_bytes {
            a.push((bytes.as_ptr() as usize, bytes.len()));
        }
        if let Some((bytes, _)) = self.image.preview.prepared() {
            a.push((bytes.as_ptr() as usize, bytes.len()));
        }
        if let Some(bytes) = &self.transmission {
            a.push((bytes.as_ptr() as usize, bytes.len()));
        }
        a
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn metadata_cost(image: &PastedImage) -> usize {
    4096 + image.mime_type.capacity()
        + [
            &image.source_path,
            &image.staged_temp_path,
            &image.session_image_path,
        ]
        .into_iter()
        .map(|p| p.as_ref().map_or(0, PathBuf::capacity))
        .sum::<usize>()
}
pub fn prepare(image: PastedImage) -> Result<Prepared, String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let bytes = image.encoded_bytes.as_ref().ok_or("图片数据已不可用")?;
    if bytes.len() > crate::attachments::MAX_FILE_BYTES {
        return Err("图片超过本地限制".into());
    }
    // Preparation runs on the image worker, not in paint. Count original and
    // converted buffers separately, while PNG's shared Arc is counted once.
    let preview = image.preview.prepared().map(|(b, _)| b);
    let resident = bytes.len()
        + preview
            .filter(|b| b.as_ptr() != bytes.as_ptr())
            .map_or(0, |b| b.len());
    let estimated = preview.map_or(0, |b| {
        b.len().div_ceil(3) * 4 + b.len().div_ceil(3072) * 40 + 128
    });
    if resident
        .saturating_add(estimated)
        .saturating_add(metadata_cost(&image))
        > MAX_RESIDENT
    {
        return Err("图片原始/转换及传输数据超过64MiB会话缓存预算".into());
    }
    let transmission = preview
        .and_then(|b| crate::terminal::image::transmit_inline_image(b, 0))
        .map(std::sync::Arc::<str>::from);
    let dir = std::env::temp_dir().join(format!("dot-open-image-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|_| "无法建立私有图片缓存")?;
    let extension = match image.mime_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => {
            let _ = std::fs::remove_dir(&dir);
            return Err("不支持的图片格式".into());
        }
    };
    let path = dir.join(format!("image.{extension}"));
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .and_then(|mut f| f.write_all(bytes));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("无法准备本地图片查看文件".into());
    }
    Ok(Prepared {
        image,
        transmission,
        path,
        dir,
    })
}
pub struct Store {
    files: Vec<(String, Prepared)>,
    visible: std::collections::HashSet<String>,
    blocked: std::collections::HashSet<String>,
    budget: usize,
    max_entries: usize,
    pub viewer_requests: usize,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            visible: Default::default(),
            blocked: Default::default(),
            budget: MAX_RESIDENT,
            max_entries: MAX_ENTRIES,
            viewer_requests: 0,
        }
    }
}
impl Store {
    #[cfg(test)]
    pub fn with_test_limits(budget: usize, max_entries: usize) -> Self {
        Self {
            budget,
            max_entries,
            ..Self::default()
        }
    }
    pub fn set_visible(&mut self, visible: std::collections::HashSet<String>) {
        self.blocked.retain(|k| visible.contains(k));
        self.visible = visible;
    }
    pub fn can_load_auto(&self, key: &str) -> bool {
        self.cached(key).is_none() && !self.blocked.contains(key)
    }
    pub fn prepared(&self, key: &str) -> Option<&Prepared> {
        self.files.iter().find(|(k, _)| k == key).map(|(_, p)| p)
    }
    pub fn image(&self, key: &str) -> Option<&PastedImage> {
        self.prepared(key).map(|p| &p.image)
    }
    pub fn cached(&self, key: &str) -> Option<&Path> {
        self.prepared(key).map(Prepared::path)
    }
    fn cost(&self, extra: Option<&Prepared>) -> usize {
        let mut seen = std::collections::HashSet::new();
        let mut total = 0usize;
        for p in self.files.iter().map(|(_, p)| p).chain(extra) {
            total = total.saturating_add(p.overhead()); // payloads dedup below; owned paths included
            for (ptr, len) in p.allocations() {
                if seen.insert((ptr, len)) {
                    total = total.saturating_add(len);
                }
            }
        }
        total
    }
    #[cfg(test)]
    pub fn resident_bytes(&self) -> usize {
        self.cost(None)
    }
    fn admit(&mut self, key: String, file: Prepared, explicit: bool) -> bool {
        let mut seen = std::collections::HashSet::new();
        let own = file.overhead()
            + file
                .allocations()
                .into_iter()
                .filter(|a| seen.insert(*a))
                .map(|(_, n)| n)
                .sum::<usize>();
        if own > self.budget || self.max_entries == 0 {
            self.blocked.insert(key);
            return false;
        }
        // Replace atomically in cache policy, cleaning the old private directory.
        self.files.retain(|(k, _)| k != &key);
        while self.files.len() >= self.max_entries || self.cost(Some(&file)) > self.budget {
            let victim = self
                .files
                .iter()
                .position(|(k, _)| explicit || !self.visible.contains(k));
            let Some(index) = victim else {
                self.blocked.insert(key);
                return false;
            };
            let (evicted, _) = self.files.remove(index);
            if self.visible.contains(&evicted) {
                self.blocked.insert(evicted);
            }
        }
        self.blocked.remove(&key);
        self.files.push((key, file));
        true
    }
    pub fn retain_auto(&mut self, key: String, file: Prepared) -> bool {
        self.admit(key, file, false)
    }
    pub fn retain(&mut self, key: String, file: Prepared) -> bool {
        self.admit(key, file, true)
    }
    pub fn view(&mut self, key: &str) -> Option<PastedImage> {
        let index = self.files.iter().position(|(k, _)| k == key)?;
        let item = self.files.remove(index);
        let image = item.1.image.clone();
        self.files.push(item);
        self.viewer_requests += 1;
        Some(image)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_file_is_retained_for_viewer_and_cleaned_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let mut images = crate::image_ingress::stage(
            crate::image_ingress::Input::Bytes(crate::image_ingress::synthetic()),
            true,
        )
        .unwrap()
        .images;
        let p = prepare(images.remove(0)).unwrap();
        let path = p.path.clone();
        assert_eq!(
            std::fs::metadata(&p.dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut store = Store::default();
        store.retain("room/file".into(), p);
        assert!(store.view("room/file").is_some());
        assert_eq!(store.viewer_requests, 1);
        drop(store);
        assert!(!path.exists());
    }
}
#[cfg(test)]
mod budget_tests {
    use super::*;
    use std::{collections::HashSet, sync::Arc};
    fn small() -> Prepared {
        let mut c = crate::image_composer::ImageComposer::default();
        c.stage_bytes(crate::image_ingress::synthetic(), None)
            .unwrap();
        let image = c.images.remove(0);
        image.prepare_preview_blocking();
        prepare(image).unwrap()
    }
    fn visible(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }
    #[test]
    fn visible_over_64mib_never_cycles_admission_and_recovers_after_leaving_view() {
        let mut s = Store::default();
        let keys: Vec<_> = (0..9).map(|i| format!("file-{i}")).collect();
        s.set_visible(keys.iter().cloned().collect());
        let mut denied = None;
        for key in &keys {
            let mut p = small(); // Isolated cache-policy fixture for an expanded transport buffer.
            p.transmission = Some(Arc::from("x".repeat(8 * 1024 * 1024)));
            if !s.retain_auto(key.clone(), p) {
                denied = Some(key.clone());
            }
            assert!(s.resident_bytes() <= MAX_RESIDENT);
            assert!(s.files.len() <= MAX_ENTRIES);
        }
        let denied = denied.unwrap();
        let first = s.files[0].0.clone();
        let first_path = s.cached(&first).unwrap().to_owned();
        for _ in 0..100 {
            s.set_visible(keys.iter().cloned().collect());
            assert!(!s.can_load_auto(&denied));
            assert!(s.cached(&first).is_some());
        }
        s.set_visible(visible(&[&denied]));
        assert!(!s.can_load_auto(&denied));
        s.set_visible(HashSet::new());
        s.set_visible(visible(&[&denied]));
        assert!(s.can_load_auto(&denied));
        let mut p = small();
        p.transmission = Some(Arc::from("x".repeat(8 * 1024 * 1024)));
        assert!(s.retain_auto(denied, p));
        assert!(!first_path.exists());
        let paths: Vec<_> = s.files.iter().map(|(_, p)| p.path.clone()).collect();
        drop(s);
        assert!(paths.iter().all(|p| !p.exists()));
    }
    #[test]
    fn jpeg_converted_png_and_cached_transmission_are_all_counted_without_shared_arc_double_count()
    {
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        let picture = image::RgbImage::from_fn(128, 128, |x, y| {
            image::Rgb([
                ((x * 17 + y * 31) % 256) as u8,
                ((x * 41 + y * 7) % 256) as u8,
                ((x * y * 13) % 256) as u8,
            ])
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 10)
            .encode_image(&picture)
            .unwrap();
        let mut c = crate::image_composer::ImageComposer::default();
        c.stage_bytes(jpeg, None).unwrap();
        let image = c.images.remove(0);
        image.prepare_preview_blocking();
        let original = image.encoded_bytes.as_ref().unwrap();
        let prepared = image.preview.prepared().unwrap().0;
        assert_ne!(original.as_ptr(), prepared.as_ptr());
        assert!(prepared.len() > original.len());
        let original_len = original.len();
        let prepared_len = prepared.len();
        let a = prepare(image.clone()).unwrap();
        let b = prepare(image).unwrap();
        let transport = b.transmission.as_ref().unwrap().len();
        let overhead = b.overhead();
        let mut s = Store::default();
        assert!(s.retain("a".into(), a));
        let one = s.resident_bytes();
        assert!(one >= original_len + prepared_len + transport);
        assert!(s.retain("b".into(), b));
        assert_eq!(s.resident_bytes() - one, overhead + transport);
    }
    #[test]
    fn many_small_files_replacement_explicit_priority_and_rejected_files_clean_directories() {
        let mut s = Store::with_test_limits(MAX_RESIDENT, 3);
        for i in 0..40 {
            assert!(s.retain(format!("key-{i}"), small()));
            assert!(s.files.len() <= 3);
        }
        let old = s.cached("key-39").unwrap().to_owned();
        assert!(s.retain("key-39".into(), small()));
        assert!(!old.exists());
        s.set_visible(visible(&["key-37", "key-38", "key-39", "new"]));
        let p = small();
        let rejected = p.path.clone();
        assert!(!s.retain_auto("new".into(), p));
        assert!(!rejected.exists());
        assert!(!s.can_load_auto("new"));
        assert!(s.retain("new".into(), small()));
        assert!(!s.can_load_auto("key-37"));
        assert!(s.open("new", |p| p.is_file()));
        let files: Vec<_> = s.files.iter().map(|(_, p)| p.dir.clone()).collect();
        drop(s);
        assert!(files.iter().all(|p| !p.exists()));
    }
}
