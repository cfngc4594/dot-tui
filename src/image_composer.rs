//! Cropped original PromptWidget image state used by the dot composer.
//! Upstream fixed commit / ownership adaptations are listed in IMAGE_PORT.md.
use crate::{prompt_images::PastedImage, theme::Theme};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use xai_ratatui_textarea::{ElementId, ElementKind, TextArea};
const KIND_IMAGE: ElementKind = ElementKind(3);
const PROMPT_IMAGES_TRACING_TARGET: &str = "dot_tui::prompt_images";
pub struct ImageComposer {
    pub textarea: TextArea,
    pub images: Vec<PastedImage>,
    image_undo_stash: Vec<PastedImage>,
    image_counter: usize,
    post_insert_image_preview: Option<(ElementId, usize)>,
    hovered_image_element_id: Option<ElementId>,
}
impl Default for ImageComposer {
    fn default() -> Self {
        Self {
            textarea: TextArea::new(),
            images: vec![],
            image_undo_stash: vec![],
            image_counter: 0,
            post_insert_image_preview: None,
            hovered_image_element_id: None,
        }
    }
}
impl ImageComposer {
    /// Original PromptWidget entry transition for editing keys, explicit text
    /// paste and prompt mouse dispatch. App-level hover/motion must not call it.
    /// The post-insert latch lasts until an actual widget interaction.
    pub fn begin_interaction(&mut self) {
        self.post_insert_image_preview = None;
    }
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        self.begin_interaction();
        self.textarea.input(key);
    }
    /// Retains upstream textarea click snapping and PromptWidget hover events.
    pub fn handle_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        area: ratatui::layout::Rect,
        state: xai_ratatui_textarea::TextAreaState,
    ) -> xai_ratatui_textarea::MouseAction {
        self.begin_interaction();
        let action = self.textarea.handle_mouse(mouse, area, state);
        self.update_hover();
        action
    }
    /// Original PromptWidget::set_cursor; app shortcuts must not bypass its
    /// post-insert latch transition via DerefMut to the underlying textarea.
    pub fn set_cursor(&mut self, pos: usize) {
        self.post_insert_image_preview = None;
        self.textarea.set_cursor(pos);
    }
    /// Explicit local staging boundary. This does no uploading and has no account
    /// dependency. Live UI wiring must execute decode/preview preparation off-thread.
    pub fn stage_bytes(
        &mut self,
        bytes: Vec<u8>,
        source_path: Option<std::path::PathBuf>,
    ) -> Result<(), String> {
        crate::attachments::decode_bounded(&bytes).map_err(str::to_owned)?;
        let (w, h, mime) = crate::attachments::dimensions_checked(&bytes).map_err(str::to_owned)?;
        let image = PastedImage {
            element_id: ElementId::from_raw(0),
            display_number: 0,
            mime_type: mime.into(),
            dimensions: Some((w, h)),
            byte_len: bytes.len(),
            encoded_bytes: Some(bytes.into()),
            source_path: source_path.map(|p| {
                std::path::PathBuf::from(crate::sanitize::strip_control_chars(
                    &p.to_string_lossy(),
                    false,
                ))
            }),
            staged_temp_path: None,
            session_image_path: None,
            preview: Default::default(),
        };
        self.insert_image(image)
    }
    pub fn stage_explicit_path(&mut self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| "cannot open selected image")?;
        let meta = file
            .metadata()
            .map_err(|_| "cannot inspect selected image")?;
        if !meta.is_file() || meta.len() > crate::attachments::MAX_FILE_BYTES as u64 {
            return Err("selected image is not a regular file within 20 MiB".into());
        }
        let mut bytes = Vec::new();
        file.take(crate::attachments::MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "cannot read selected image")?;
        self.stage_bytes(bytes, Some(path.into()))
    }
    pub fn stage_clipboard(&mut self, clipboard: &mut impl ImageClipboard) -> Result<bool, String> {
        let Some(bytes) = clipboard.image_bytes()? else {
            return Ok(false);
        };
        self.stage_bytes(bytes, None)?;
        Ok(true)
    }
    pub const IMAGE_CAP: usize = usize::MAX;
    pub fn cap_reached_toast() -> String {
        format!("Image limit reached (max {})", Self::IMAGE_CAP)
    }
    fn cursor(&self) -> usize {
        self.textarea.cursor()
    }
    pub fn insert_image(&mut self, image: PastedImage) -> Result<(), String> {
        if self.images.len() >= Self::IMAGE_CAP {
            return Err(Self::cap_reached_toast());
        }
        self.image_counter += 1;
        let display_number = self.image_counter;
        let placeholder = crate::prompt_images::display_text(display_number);
        let display_line = chip_line(format!("Image #{display_number}"));

        let buf_len_before = self.textarea.text().len();
        self.textarea.begin_undo_group();
        let element_id = self
            .textarea
            .insert_element(&placeholder, KIND_IMAGE, Some(display_line));
        self.textarea.insert_str(" ");
        self.textarea.end_undo_group();
        self.post_insert_image_preview = Some((element_id, self.textarea.cursor()));
        let buf_len_after = self.textarea.text().len();

        tracing::debug!(
            target: PROMPT_IMAGES_TRACING_TARGET,
            display_number,
            element_id = ?element_id,
            images_len_after = self.images.len() + 1,
            image_counter_after = self.image_counter,
            buf_len_before,
            buf_len_after,
            "prompt_widget: insert_image",
        );

        let mut img = image;
        img.element_id = element_id;
        img.display_number = display_number;
        self.images.push(img);

        Ok(())
    }

    pub(crate) fn sync_images_with_textarea(&mut self) {
        use std::collections::HashMap;
        use std::collections::hash_map::Entry;

        self.rechip_orphan_image_placeholders();

        let live_image_elements: Vec<(ElementId, std::ops::Range<usize>)> = self
            .textarea
            .elements()
            .iter()
            .filter(|e| e.kind == KIND_IMAGE)
            .map(|e| (e.id, e.range.clone()))
            .collect();
        let stored_len_before = self.images.len();
        let counter_before = self.image_counter;
        let stash_len_before = self.image_undo_stash.len();

        // Primary lookup keyed by `ElementId` (unique by construction).
        // Keying by `display_number` would silently drop one record when two images share a number
        // The warn branch below makes a future duplicate-id regression visible instead of hiding it
        let mut stored_by_id: HashMap<ElementId, PastedImage> =
            HashMap::with_capacity(self.images.len());
        for img in self.images.drain(..) {
            let id = img.element_id;
            let display_number = img.display_number;
            match stored_by_id.entry(id) {
                Entry::Vacant(v) => {
                    v.insert(img);
                }
                Entry::Occupied(_) => {
                    tracing::warn!(
                        target: PROMPT_IMAGES_TRACING_TARGET,
                        element_id = ?id,
                        display_number,
                        "sync_images_with_textarea: duplicate element_id \
                         in stored images — dropping later entry to \
                         preserve the first (invariant violation)",
                    );
                }
            }
        }

        // Fallback for elements restored by an undo/redo cycle. Keying by `display_number` is safe here
        // because the monotonic counter never recycles numbers within a prompt lifetime.
        // A same-number duplicate is no redo target but still owns files, so it stays in the stash.
        let mut stash_by_number: HashMap<usize, PastedImage> =
            HashMap::with_capacity(self.image_undo_stash.len());
        let mut stash_duplicates: Vec<PastedImage> = Vec::new();
        for img in self.image_undo_stash.drain(..) {
            let display_number = img.display_number;
            let element_id = img.element_id;
            match stash_by_number.entry(display_number) {
                Entry::Vacant(v) => {
                    v.insert(img);
                }
                Entry::Occupied(_) => {
                    tracing::warn!(
                        target: PROMPT_IMAGES_TRACING_TARGET,
                        display_number,
                        element_id = ?element_id,
                        "sync_images_with_textarea: duplicate display_number \
                         in undo stash — kept aside, not a redo target",
                    );
                    stash_duplicates.push(img);
                }
            }
        }

        let mut synced = Vec::with_capacity(live_image_elements.len());

        for (id, range) in &live_image_elements {
            // Primary: match by element_id (collision-free, stable for non-undo/redo edits)
            if let Some(mut img) = stored_by_id.remove(id) {
                // Refresh display_number from the live buffer text in case the chip was renumbered externally (sanity)
                let parsed = self
                    .textarea
                    .get_range(range.clone())
                    .and_then(parse_image_display_number)
                    .unwrap_or(img.display_number);
                img.element_id = *id;
                img.display_number = parsed;
                synced.push(img);
                continue;
            }
            // Fallback (redo path): match by display_number from the stash, then rebind to the freshly-issued `element_id`
            let display_number = self
                .textarea
                .get_range(range.clone())
                .and_then(parse_image_display_number)
                .unwrap_or(0);
            if let Some(mut img) = stash_by_number.remove(&display_number) {
                img.element_id = *id;
                img.display_number = display_number;
                synced.push(img);
            }
        }

        // Images whose elements are gone move to the undo stash so they can be recovered if the user
        // redoes. Cap the stash at `IMAGE_CAP * 2` so a long edit session can't unboundedly grow it.
        let mut new_stash: Vec<PastedImage> = stored_by_id
            .into_values()
            .chain(stash_by_number.into_values())
            .chain(stash_duplicates)
            .collect();
        let stash_cap = Self::IMAGE_CAP.saturating_mul(2);
        if new_stash.len() > stash_cap {
            // Oldest means the lowest `display_number` (monotonic within a prompt lifetime). Sort ascending, then drop the front.
            new_stash.sort_by_key(|img| img.display_number);
            let evict_count = new_stash.len() - stash_cap;
            for img in new_stash.drain(..evict_count) {
                // Stash eviction
                // Host stores bytes only. Dropping a stash record releases its Arc.
                drop(img);
            }
        }
        self.image_undo_stash = new_stash;
        self.images = synced;
        if self
            .hovered_image_element_id
            .is_some_and(|id| !self.images.iter().any(|img| img.element_id == id))
        {
            self.hovered_image_element_id = None;
        }
        if self
            .post_insert_image_preview
            .is_some_and(|(id, _)| !self.images.iter().any(|img| img.element_id == id))
        {
            self.post_insert_image_preview = None;
        }
        // Monotonic high-water mark within a prompt lifetime: the counter only ever advances upward
        // Deleting a chip therefore never lets a later insert reuse a number that already appeared
        // The counter is reset only by explicit prompt clears (`set_text("")`, Ctrl+C) which call [`crate::prompt_images::clear`]
        self.image_counter = self.image_counter.max(images_high_water(&self.images));

        // Only log when the stored count or stash size actually changed, otherwise every post-paste keystroke would emit a debug line
        let stash_len_after = self.image_undo_stash.len();
        let stored_changed = self.images.len() != stored_len_before;
        let stash_changed = stash_len_after != stash_len_before;
        if stored_changed || stash_changed {
            let stashed_numbers: Vec<usize> = self
                .image_undo_stash
                .iter()
                .map(|img| img.display_number)
                .collect();
            tracing::debug!(
                target: PROMPT_IMAGES_TRACING_TARGET,
                live_count = live_image_elements.len(),
                stored_len_before,
                synced_len = self.images.len(),
                counter_before,
                counter_after = self.image_counter,
                stash_len_before,
                stash_len_after,
                ?stashed_numbers,
                "prompt_widget: sync_images_with_textarea",
            );
        }
    }

    /// Look up the image record for the element the cursor is currently on. Positions at or past
    /// `range.end` (the trailing separator space and beyond) are intentionally excluded.
    pub fn image_at_cursor(&self) -> Option<&PastedImage> {
        let elem = self.textarea.element_at_cursor()?;
        if elem.kind != KIND_IMAGE {
            return None;
        }
        self.images.iter().find(|img| img.element_id == elem.id)
    }

    /// Image element the cursor is on, or exactly at its right edge. An on-chip match of any kind wins
    /// (the chip under the cursor owns the position). So a cursor sitting on a non-image chip yields
    /// `None` even when an image chip ends at the cursor.
    fn image_element_near_cursor(&self) -> Option<&xai_ratatui_textarea::TextElement> {
        if let Some(elem) = self.textarea.element_at_cursor() {
            return (elem.kind == KIND_IMAGE).then_some(elem);
        }
        let cursor = self.textarea.cursor();
        // Right edge only, not the spacer after (`range.end + 1`)
        self.textarea
            .elements()
            .iter()
            .filter(|e| e.kind == KIND_IMAGE)
            .find(|e| e.range.end == cursor)
    }

    /// Image record for the preview overlay: cursor, hover, or fresh insertion.
    pub(crate) fn image_for_preview(&self) -> Option<&PastedImage> {
        if let Some(elem) = self.image_element_near_cursor() {
            return self.images.iter().find(|img| img.element_id == elem.id);
        }
        if let Some(id) = self.hovered_image_element_id {
            return self.images.iter().find(|img| img.element_id == id);
        }
        let (id, cursor) = self.post_insert_image_preview?;
        if self.cursor() != cursor {
            return None;
        }
        self.images.iter().find(|img| img.element_id == id)
    }
}
pub(crate) fn chip_line(label: String) -> Line<'static> {
    let theme = Theme::current();
    let bracket = Style::default().fg(theme.paste_dim).bg(theme.paste_bg);
    // User-requested image accent from GrokNight's original success/green
    // palette. Keep paste_dim brackets and the original chip construction.
    let text = Style::default().fg(theme.accent_success).bg(theme.paste_bg);
    Line::from(vec![
        Span::styled("[", bracket),
        Span::styled(label, text),
        Span::styled("]", bracket),
    ])
}

pub(crate) fn parse_image_display_number(placeholder: &str) -> Option<usize> {
    let inner = placeholder.strip_prefix("[Image #")?.strip_suffix(']')?;
    // Split at the first `:` (path-suffix form) or use the whole string.
    let number_str = inner.split_once(':').map(|(n, _)| n).unwrap_or(inner);
    number_str.trim().parse().ok()
}

fn images_high_water(images: &[PastedImage]) -> usize {
    images.iter().map(|i| i.display_number).max().unwrap_or(0)
}

mod image_state;

/// Injectable binary image clipboard. No system implementation is installed.
/// Tests and the isolated demo use synthetic bytes only.
pub trait ImageClipboard {
    fn image_bytes(&mut self) -> Result<Option<Vec<u8>>, String>;
}

impl std::ops::Deref for ImageComposer {
    type Target = TextArea;
    fn deref(&self) -> &TextArea {
        &self.textarea
    }
}
impl std::ops::DerefMut for ImageComposer {
    fn deref_mut(&mut self) -> &mut TextArea {
        &mut self.textarea
    }
}
impl From<TextArea> for ImageComposer {
    fn from(textarea: TextArea) -> Self {
        Self {
            textarea,
            ..Self::default()
        }
    }
}
impl ImageComposer {
    pub fn submission_text(&self) -> String {
        let mut out = String::new();
        let mut previous = 0;
        for e in self
            .textarea
            .elements()
            .iter()
            .filter(|e| e.kind == KIND_IMAGE)
        {
            if let Some(gap) = self.textarea.text().get(previous..e.range.start) {
                out.push_str(gap);
            }
            previous = e.range.end;
        }
        if let Some(tail) = self.textarea.text().get(previous..) {
            out.push_str(tail);
        }
        out
    }
    #[allow(clippy::collapsible_match)] // Original PromptWidget element-hover event precedence.
    pub fn update_hover(&mut self) {
        use xai_ratatui_textarea::TextElementEventKind;
        while let Some(e) = self.textarea.poll_element_event() {
            match e.kind {
                TextElementEventKind::HoverEnter => {
                    let is_image = self
                        .textarea
                        .elements()
                        .iter()
                        .any(|element| element.id == e.id && element.kind == KIND_IMAGE);
                    if is_image {
                        self.hovered_image_element_id = Some(e.id);
                    }
                }
                TextElementEventKind::HoverLeave => {
                    if self.hovered_image_element_id == Some(e.id) {
                        self.hovered_image_element_id = None;
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn image() -> PastedImage {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(16, 12)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        let bytes = out.into_inner();
        crate::attachments::decode_bounded(&bytes).unwrap();
        PastedImage {
            element_id: ElementId::from_raw(0),
            display_number: 0,
            mime_type: "image/png".into(),
            dimensions: Some((16, 12)),
            byte_len: bytes.len(),
            encoded_bytes: Some(Arc::from(bytes)),
            source_path: None,
            staged_temp_path: None,
            session_image_path: None,
            preview: Default::default(),
        }
    }
    #[test]
    fn chip_is_atomic_path_free_and_immediately_previewable() {
        let mut c = ImageComposer::default();
        c.insert_image(image()).unwrap();
        assert_eq!(c.textarea.text(), "[Image #1] ");
        assert_eq!(c.textarea.elements().len(), 1);
        assert_eq!(c.image_for_preview().unwrap().display_number, 1);
        let line = c.textarea.elements()[0].display.as_ref().unwrap();
        assert_eq!(
            line.spans[0].style.fg,
            Some(ratatui::style::Color::Rgb(65, 65, 65))
        );
    }
    #[test]
    fn undo_redo_reconciles_original_image_records() {
        let mut c = ImageComposer::default();
        c.insert_image(image()).unwrap();
        assert!(c.textarea.undo());
        c.sync_images_with_textarea();
        assert!(c.images.is_empty());
        assert!(c.textarea.redo());
        c.sync_images_with_textarea();
        assert_eq!(c.images.len(), 1);
        assert_eq!(c.images[0].display_number, 1);
    }
    #[test]
    fn plain_fake_placeholder_never_acquires_bytes() {
        let mut c = ImageComposer::default();
        c.textarea.set_text("[Image #1]");
        c.sync_images_with_textarea();
        assert!(c.images.is_empty());
        assert_eq!(c.unbound_image_placeholders(), vec![1]);
    }
    #[test]
    fn removed_chip_is_excluded_and_numbers_not_recycled() {
        let mut c = ImageComposer::default();
        c.insert_image(image()).unwrap();
        c.textarea.set_text("");
        c.sync_images_with_textarea();
        assert!(c.images.is_empty());
        c.insert_image(image()).unwrap();
        assert_eq!(c.images[0].display_number, 2);
    }
    #[test]
    fn original_composer_does_not_have_legacy_ten_image_cap() {
        let mut c = ImageComposer::default();
        for _ in 0..10 {
            c.insert_image(image()).unwrap();
        }
        let before = c.textarea.text().to_owned();
        c.insert_image(image()).unwrap();
        assert!(c.textarea.text().starts_with(&before));
        assert_eq!(c.images.len(), 11);
    }
    #[test]
    fn actual_synthetic_image_renders_original_metadata_overlay() {
        let _guard = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::None,
        );
        let mut c = ImageComposer::default();
        c.insert_image(image()).unwrap();
        let image = c.image_for_preview().unwrap();
        image.prepare_preview_blocking();
        assert_eq!(image.preview_dimensions(), Some((16, 12)));
        let area = ratatui::layout::Rect::new(3, 2, 80, 24);
        let mut b = ratatui::buffer::Buffer::empty(area);
        let (painted, pixels) = crate::render::image_overlay::render_image_overlay_checked(
            &mut b,
            area,
            image,
            ratatui::style::Color::Black,
            ratatui::style::Color::White,
            ratatui::style::Color::Gray,
        );
        assert!(painted);
        assert!(pixels.is_none());
        let text = b.content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(text.contains("Image #1"));
    }
    #[test]
    fn fake_clipboard_to_chip_to_original_preview() {
        struct Fake {
            bytes: Option<Vec<u8>>,
            reads: usize,
        }
        impl ImageClipboard for Fake {
            fn image_bytes(&mut self) -> Result<Option<Vec<u8>>, String> {
                self.reads += 1;
                Ok(self.bytes.take())
            }
        }
        let mut source = Fake {
            bytes: Some(image().encoded_bytes.unwrap().to_vec()),
            reads: 0,
        };
        let mut c = ImageComposer::default();
        assert!(c.stage_clipboard(&mut source).unwrap());
        assert_eq!(source.reads, 1);
        c.image_for_preview().unwrap().prepare_preview_blocking();
        assert_eq!(c.images.len(), 1);
        assert_eq!(c.images[0].preview_dimensions(), Some((16, 12)));
    }
    #[test]
    fn explicit_synthetic_local_path_to_chip_preserves_source_file() {
        let path =
            std::env::temp_dir().join(format!("dot-selected-image-{}.png", uuid::Uuid::new_v4()));
        let bytes = image().encoded_bytes.unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut c = ImageComposer::default();
        c.stage_explicit_path(&path).unwrap();
        assert_eq!(c.images[0].source_path.as_deref(), Some(path.as_path()));
        assert_eq!(c.textarea.text(), "[Image #1] ");
        assert_eq!(std::fs::read(&path).unwrap(), bytes.as_ref());
        std::fs::remove_file(path).unwrap();
    }
}
