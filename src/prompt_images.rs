use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use xai_ratatui_textarea::ElementId;
#[derive(Clone)]
pub struct PastedImage {
    /// The [`ElementId`] of the corresponding `KIND_IMAGE` element in the `TextArea` buffer. Used to reconcile live elements against stored images.
    pub element_id: ElementId,

    /// 1-based display number for the current prompt (e.g. the `1` in `[Image #1]`). Reset when the prompt is cleared.
    pub display_number: usize,

    /// MIME type of the encoded image (e.g. `"image/png"`).
    pub mime_type: String,

    /// Header-validated dimensions used for pre-insertion/send policy.
    pub dimensions: Option<(u32, u32)>,

    /// Size of the encoded image bytes.
    pub byte_len: usize,

    /// Encoded image bytes kept in memory.
    /// Set to `None` once the image is durably written to [`session_image_path`](Self::session_image_path), to avoid holding large buffers.
    pub encoded_bytes: Option<Arc<[u8]>>,

    /// Original user-visible path for file-path pastes.
    ///
    /// This remains stable after persistence so previews show the path the user pasted. Model/send loading uses `session_image_path` first.
    pub source_path: Option<PathBuf>,

    /// Temporary staging path before the image is finalized into the session directory. Cleaned up when the chip is removed or on send.
    pub staged_temp_path: Option<PathBuf>,

    /// Final durable path under `session_dir(info)/images/`.
    /// Once set, this is the canonical on-disk location and [`encoded_bytes`](Self::encoded_bytes) may be released.
    pub session_image_path: Option<PathBuf>,

    /// Shared preview preparation state; draw only reads its resolved result.
    pub preview: PromptImagePreview,
}

impl PastedImage {
    pub fn preview_preparation(&self) -> Option<PromptImagePreviewPreparation> {
        if !self.preview.is_pending() {
            return None;
        }
        Some(PromptImagePreviewPreparation {
            preview: self.preview.clone(),
            source: self.encoded_bytes.as_ref()?.clone(),
            protocol: crate::terminal::image::prompt_preview_graphics_protocol(),
            dimensions: self.dimensions,
        })
    }

    pub fn prepare_preview_blocking(&self) {
        if let Some(preparation) = self.preview_preparation() {
            preparation.run();
        }
    }

    pub fn preview_dimensions(&self) -> Option<(u32, u32)> {
        self.dimensions.or_else(|| self.preview.dimensions())
    }
}

/// Shared readiness of the terminal pixel payload for a pasted image.
#[derive(Clone)]
pub struct PromptImagePreview {
    identity: u64,
    result: Arc<OnceLock<PromptImagePreviewResult>>,
}

enum PromptImagePreviewResult {
    Ready {
        bytes: Arc<[u8]>,
        dimensions: (u32, u32),
    },
    Failed,
    Unsupported {
        dimensions: (u32, u32),
    },
}

impl Default for PromptImagePreview {
    fn default() -> Self {
        Self {
            identity: crate::terminal::overlay::next_owner_id(),
            result: Arc::new(OnceLock::new()),
        }
    }
}

impl PromptImagePreview {
    pub fn identity(&self) -> u64 {
        self.identity
    }

    pub fn is_pending(&self) -> bool {
        self.result.get().is_none()
    }

    pub fn is_failed(&self) -> bool {
        matches!(self.result.get(), Some(PromptImagePreviewResult::Failed))
    }

    pub fn prepared(&self) -> Option<(&[u8], (u32, u32))> {
        match self.result.get()? {
            PromptImagePreviewResult::Ready { bytes, dimensions } => {
                Some((bytes.as_ref(), *dimensions))
            }
            PromptImagePreviewResult::Failed | PromptImagePreviewResult::Unsupported { .. } => None,
        }
    }

    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self.result.get()? {
            PromptImagePreviewResult::Ready { dimensions, .. }
            | PromptImagePreviewResult::Unsupported { dimensions } => Some(*dimensions),
            PromptImagePreviewResult::Failed => None,
        }
    }

    fn finish(&self, result: PromptImagePreviewResult) {
        let _ = self.result.set(result);
    }

    pub fn mark_failed(&self) {
        self.finish(PromptImagePreviewResult::Failed);
    }

    #[cfg(any(test, feature = "dev-demo"))]
    pub fn ready_for_test(bytes: Vec<u8>, dimensions: (u32, u32)) -> Self {
        let preview = Self::default();
        preview.finish(PromptImagePreviewResult::Ready {
            bytes: Arc::from(bytes),
            dimensions,
        });
        preview
    }
}

/// Off-thread work required to prepare one prompt image preview.
#[derive(Clone)]
pub struct PromptImagePreviewPreparation {
    preview: PromptImagePreview,
    source: Arc<[u8]>,
    protocol: crate::terminal::image::GraphicsProtocol,
    dimensions: Option<(u32, u32)>,
}

impl PromptImagePreviewPreparation {
    pub fn preview(&self) -> PromptImagePreview {
        self.preview.clone()
    }

    pub fn run(self) {
        let Some(dimensions) = self
            .dimensions
            .or_else(|| decode_image_dimensions(&self.source))
        else {
            self.preview.finish(PromptImagePreviewResult::Failed);
            return;
        };
        let result = match self.protocol {
            crate::terminal::image::GraphicsProtocol::Kitty => {
                let bytes =
                    if crate::terminal::image::kitty_format_from_bytes(&self.source).is_some() {
                        self.source
                    } else {
                        let Some(converted) =
                            crate::terminal::image::prepare_kitty_overlay_image_bytes(&self.source)
                        else {
                            self.preview.finish(PromptImagePreviewResult::Failed);
                            return;
                        };
                        Arc::from(converted)
                    };
                PromptImagePreviewResult::Ready { bytes, dimensions }
            }
            crate::terminal::image::GraphicsProtocol::ITerm2 => PromptImagePreviewResult::Ready {
                bytes: self.source,
                dimensions,
            },
            crate::terminal::image::GraphicsProtocol::None => {
                PromptImagePreviewResult::Unsupported { dimensions }
            }
        };
        self.preview.finish(result);
    }
}

// -------------------------------------------------------------------------
// Display helpers
// -------------------------------------------------------------------------

/// Build the buffer text for an image chip.
///
/// Always path-free: `[Image #1]`. Filepaths live on the [`PastedImage`] record and appear only in the hover/cursor preview overlay.
pub fn display_text(display_number: usize) -> String {
    format!("[Image #{display_number}]")
}

pub fn decode_image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    crate::attachments::dimensions_checked(bytes)
        .ok()
        .map(|(w, h, _)| (w, h))
}

// Host adaptation: encoded bytes must never enter debug/log output.
impl std::fmt::Debug for PromptImagePreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptImagePreview")
            .field("pending", &self.is_pending())
            .field("dimensions", &self.dimensions())
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for PromptImagePreviewPreparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromptImagePreviewPreparation")
            .field("protocol", &self.protocol)
            .field("dimensions", &self.dimensions)
            .finish_non_exhaustive()
    }
}
