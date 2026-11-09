//! Background staging. No upload occurs here. Only explicitly selected/pasted
//! paths are read; there is no recursive or whole-disk scan.
use crate::{image_composer::ImageComposer, prompt_images::PastedImage};
use std::path::PathBuf;
pub enum Input {
    #[allow(dead_code)] // Existing injected unit-fixture boundary; live path snapshots metadata.
    Clipboard,
    ClipboardSnapshot(Option<u64>),
    Bracketed {
        payload: String,
        change_count: Option<u64>,
    },
    Paths(Vec<PathBuf>),
    #[allow(dead_code)] // Explicit fake binary clipboard boundary used in dev/tests.
    Bytes(Vec<u8>),
}
pub struct Staged {
    pub images: Vec<PastedImage>,
    pub text: Option<String>,
}
pub fn stage(input: Input, fake: bool) -> Result<Staged, String> {
    let mut composer = ImageComposer::default();
    let mut text = None;
    match input {
        Input::Bytes(bytes) => composer.stage_bytes(bytes, None)?,
        Input::Paths(paths) => {
            if paths.len() > 10 {
                return Err("一次最多选择十张图片".into());
            }
            for path in paths {
                composer.stage_explicit_path(&path)?;
            }
        }
        Input::Clipboard | Input::ClipboardSnapshot(_) | Input::Bracketed { .. } => {
            if fake {
                return Ok(Staged {
                    images: vec![],
                    text: None,
                });
            }
            #[cfg(target_os = "macos")]
            {
                use crate::clipboard_paste::{self as paste, AttachmentProbeRoute};
                let (bracketed, baseline) = match &input {
                    Input::Bracketed {
                        payload,
                        change_count,
                    } => (Some(payload.as_str()), *change_count),
                    Input::ClipboardSnapshot(count) => (None, *count),
                    _ => (None, None),
                };
                // Original bracketed-insert contract: text was inserted synchronously;
                // deferred completion must never insert it twice or probe stale data.
                if bracketed.is_some() {
                    text = Some(String::new());
                }
                let changed = || {
                    baseline.is_some_and(|n| {
                        crate::clipboard_native::clipboard_change_count() != Some(n)
                    })
                };
                if changed() {
                    return if bracketed.is_some() {
                        Ok(Staged {
                            images: vec![],
                            text,
                        })
                    } else {
                        Err("Clipboard changed before the paste could be read; paste again".into())
                    };
                }
                let clipboard_text = crate::clipboard_bridge::macos::get_text()
                    .map_err(|e| format!("Clipboard text read failed: {e}"))?;
                if let Some(payload) = bracketed {
                    if !paste::bracketed_payload_matches_clipboard_text(
                        payload,
                        clipboard_text.as_deref(),
                    ) {
                        return Ok(Staged {
                            images: vec![],
                            text,
                        });
                    }
                }
                if let Some(paths) = clipboard_text.as_deref().and_then(explicit_paths) {
                    for path in paths {
                        composer.stage_explicit_path(&path)?;
                    }
                } else {
                    let route = paste::attachment_probe_route(clipboard_text.as_deref());
                    let (count, has_image) = crate::clipboard_native::clipboard_image_snapshot();
                    if paste::should_run_attachment_probe(route, true, count.is_some(), has_image) {
                        let payload = match route {
                            AttachmentProbeRoute::Skip => crate::image_clipboard::Payload::None,
                            AttachmentProbeRoute::FileUrlsThenImage => {
                                crate::image_clipboard::read_explicit()?
                            }
                            AttachmentProbeRoute::ImageOnly => {
                                crate::image_clipboard::read_image_only()?
                            }
                        };
                        match payload {
                            crate::image_clipboard::Payload::Bytes(b) => {
                                composer.stage_bytes(b, None)?
                            }
                            crate::image_clipboard::Payload::Paths(paths) => {
                                for path in paths {
                                    composer.stage_explicit_path(&path)?;
                                }
                            }
                            crate::image_clipboard::Payload::None => {}
                        }
                    }
                    if composer.images.is_empty() && bracketed.is_none() {
                        text = clipboard_text;
                    }
                }
                if changed() {
                    return if bracketed.is_some() {
                        Ok(Staged {
                            images: vec![],
                            text,
                        })
                    } else {
                        Err("Clipboard changed during paste; paste again".into())
                    };
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                // A bracketed paste already inserted text; never probe unrelated clipboard.
                if matches!(input, Input::Bracketed { .. }) {
                    text = Some(String::new());
                } else {
                    let pasted = crate::clipboard_portable::get_text()?;
                    if let Some(paths) = pasted.as_deref().and_then(explicit_paths) {
                        for path in paths {
                            composer.stage_explicit_path(&path)?;
                        }
                    } else if pasted.as_deref().is_some_and(|s| !s.is_empty()) {
                        text = pasted;
                    } else if let Some(bytes) = crate::clipboard_portable::get_image()? {
                        composer.stage_bytes(bytes, None)?;
                    }
                }
            }
        }
    }
    for image in &composer.images {
        image.prepare_preview_blocking();
    }
    Ok(Staged {
        images: composer.images,
        text,
    })
}
pub fn explicit_paths(text: &str) -> Option<Vec<PathBuf>> {
    let mut result = Vec::new();
    for line in text.trim().lines() {
        let line = line.trim().trim_matches('"').trim_matches('\'');
        let path = if line.starts_with("file://") {
            let url = reqwest::Url::parse(line).ok()?;
            if url.host_str().is_some_and(|s| s != "localhost") {
                return None;
            }
            url.to_file_path().ok()?
        } else {
            PathBuf::from(line)
        };
        if !path.is_absolute() || !is_image_path(&path) {
            return None;
        }
        result.push(path);
    }
    (!result.is_empty() && result.len() <= 10).then_some(result)
}
pub fn is_image_path(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"
        )
    })
}

#[cfg(any(test, feature = "dev-demo"))]
pub fn synthetic() -> Vec<u8> {
    let mut b = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(32, 24)
        .write_to(&mut b, image::ImageFormat::Png)
        .unwrap();
    b.into_inner()
}

#[cfg(any(test, feature = "dev-demo"))]
pub fn synthetic_media(file: &str) -> Vec<u8> {
    if !file.starts_with("fixture-") {
        return synthetic();
    }
    if file.starts_with("fixture-large") {
        let mut seed = 17u32;
        let image = image::RgbaImage::from_fn(512, 384, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            image::Rgba([seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 255])
        });
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        return bytes.into_inner();
    }
    let (w, h) = match file {
        "fixture-2" => (480, 96),
        "fixture-3" => (96, 400),
        _ => (240, 160),
    };
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let on = (x / 20 + y / 20) % 2 == 0;
        let alpha = if file == "fixture-1" && x > w / 2 {
            80
        } else {
            255
        };
        image::Rgba(if on {
            [235, 35, 75, alpha]
        } else {
            [25, 195, 235, alpha]
        })
    });
    let mut b = std::io::Cursor::new(Vec::new());
    img.write_to(&mut b, image::ImageFormat::Png).unwrap();
    b.into_inner()
}
