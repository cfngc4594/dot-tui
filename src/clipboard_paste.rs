//! Pure paste routing/comparison extracted from Grok Build 2bdd1d6 pager-render/clipboard/mod.rs (Apache-2.0).
fn lone_http_url_trimmed(t: &str) -> bool {
    !t.is_empty() && !t.contains('\n') && (t.starts_with("http://") || t.starts_with("https://"))
}
pub(crate) fn plain_text_skips_furl_probe(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }
    if lone_http_url_trimmed(trimmed) {
        return true;
    }
    if trimmed.contains("file://") {
        return true;
    }
    if trimmed.len() >= 4096 {
        return true;
    }
    if trimmed.lines().count() > 4 && !trimmed.contains("://") {
        return true;
    }
    !trimmed.contains("://")
}
pub fn paste_payload_needs_clipboard_attachment_probe(payload: &str) -> bool {
    if payload.is_empty() {
        return true;
    }
    let t = payload.trim();
    if lone_http_url_trimmed(t) {
        return false;
    }
    if t.len() >= 4096 {
        return false;
    }
    let line_count = t.lines().count().max(1);
    line_count <= 4 || t.contains("://")
}
pub fn bracketed_payload_matches_clipboard_text(
    payload: &str,
    clipboard_text: Option<&str>,
) -> bool {
    // Terminals rewrite pasted newlines as `\r` (Windows clipboards carry `\r\n`); some copy sources append a trailing newline the paste omits
    fn normalized(s: &str) -> String {
        s.replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim_end()
            .to_owned()
    }
    match clipboard_text {
        None => payload.trim().is_empty(),
        Some(text) => normalized(payload) == normalized(text),
    }
}
pub fn attachment_probe_route(clipboard_text: Option<&str>) -> AttachmentProbeRoute {
    let trimmed = clipboard_text.map(str::trim).unwrap_or("");
    if !trimmed.is_empty() && lone_http_url_trimmed(trimmed) {
        return AttachmentProbeRoute::Skip;
    }
    if trimmed.is_empty() || !plain_text_skips_furl_probe(trimmed) {
        AttachmentProbeRoute::FileUrlsThenImage
    } else {
        AttachmentProbeRoute::ImageOnly
    }
}
pub fn should_run_attachment_probe(
    route: AttachmentProbeRoute,
    snapshot_supported: bool,
    snapshot_available: bool,
    snapshot_has_image: bool,
) -> bool {
    match route {
        AttachmentProbeRoute::Skip => false,
        AttachmentProbeRoute::FileUrlsThenImage => true,
        AttachmentProbeRoute::ImageOnly => {
            // Skip only when the snapshot is available and shows no raster (unavailable can't rule one out).
            let snapshot_rules_out_raster =
                snapshot_supported && snapshot_available && !snapshot_has_image;
            !snapshot_rules_out_raster
        }
    }
}

#[derive(Clone, Copy)]
pub enum AttachmentProbeRoute {
    Skip,
    FileUrlsThenImage,
    ImageOnly,
}

/// Original metadata-only gate; Dot's macOS adapter supplies the native snapshot.
#[cfg(target_os = "macos")]
pub fn attachment_probe_gate(clipboard_text: Option<&str>) -> Option<Option<u64>> {
    let (snapshot_change_count, snapshot_has_image) =
        crate::clipboard_native::clipboard_image_snapshot();
    should_run_attachment_probe(
        attachment_probe_route(clipboard_text),
        true,
        snapshot_change_count.is_some(),
        snapshot_has_image,
    )
    .then_some(snapshot_change_count)
}
