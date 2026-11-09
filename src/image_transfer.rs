//! Dot attachment adapter. App owns the immutable submission; Backend implements the
//! fixed-origin transport. Tests inject a fake and never access a real service.
//! Schema: official static front-end evidence supplied by the parent task.
use crate::{attachments, backend::Dot};
use reqwest::Url;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct LocalImage {
    bytes: Arc<[u8]>,
    mime: &'static str,
    previewed: bool,
}
impl LocalImage {
    pub fn stage(bytes: Vec<u8>) -> Result<Self, &'static str> {
        attachments::decode_bounded(&bytes)?;
        let (_, _, mime) = attachments::dimensions_checked(&bytes)?;
        Ok(Self {
            bytes: bytes.into(),
            mime,
            previewed: false,
        })
    }
    /// Call only after the original preview renderer reports a painted overlay.
    pub fn record_preview(&mut self, painted: bool) {
        self.previewed |= painted;
    }
}

pub struct BoundRoom {
    room: String,
}
impl BoundRoom {
    pub fn from_unique(dots: &[Dot], verified_room: &str) -> Result<Self, &'static str> {
        let [dot] = dots else {
            return Err("requires one verified dot");
        };
        if dot.room.as_deref() != Some(verified_room) {
            return Err("room binding changed");
        }
        endpoint(verified_room, None)?;
        Ok(Self {
            room: verified_room.to_owned(),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    NotSent(String),
    Uncertain(String),
}
impl Failure {
    pub fn into_send_error(self) -> crate::backend::SendError {
        match self {
            Self::NotSent(e) => crate::backend::SendError::NotSent(e),
            Self::Uncertain(e) => crate::backend::SendError::Uncertain(e),
        }
    }
}
pub trait Transport {
    /// Multipart POST: one part named `file`, with the validated MIME. No retry.
    fn upload(&mut self, url: Url, field: &str, mime: &str, bytes: &[u8]) -> Result<Value, String>;
    fn message(&mut self, url: Url, body: &Value) -> Result<Value, String>;
}

fn endpoint(room: &str, file: Option<&str>) -> Result<Url, &'static str> {
    if !valid_segment(room) || file.is_some_and(|s| !valid_segment(s)) {
        return Err("invalid room/file identifier");
    }
    let mut url = Url::parse("https://chatgpt.com/backend-api/messaging/rooms/").unwrap();
    {
        let mut path = url.path_segments_mut().unwrap();
        path.pop_if_empty().push(room).push("files");
        if let Some(id) = file {
            path.push(id);
        }
    }
    Ok(url)
}
fn valid_segment(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && !matches!(s, "." | "..") && !s.chars().any(char::is_control)
}
fn ack_id(value: &Value) -> Option<&str> {
    value.get("id")?.as_str().filter(|s| valid_segment(s))
}

/// One explicit submission. Never retries either the upload or message request.
/// Upload failure is distinguished from an attempted, uncertain message POST.
pub fn transmit(
    transport: &mut impl Transport,
    room: &BoundRoom,
    text: &str,
    images: &[LocalImage],
    request_id: &str,
) -> Result<String, Failure> {
    if images.is_empty()
        || images.iter().any(|i| !i.previewed)
        || uuid::Uuid::parse_str(request_id)
            .ok()
            .is_none_or(|id| id.to_string() != request_id)
    {
        return Err(Failure::NotSent(
            "Invalid image submission; message not submitted".into(),
        ));
    }
    let files = endpoint(&room.room, None).map_err(|e| Failure::NotSent(e.into()))?;
    let mut ids = Vec::new();
    for image in images {
        let value=transport.upload(files.clone(),"file",image.mime,&image.bytes)
            .map_err(|e|Failure::NotSent(format!("Image upload failed ({e}). Message not submitted; an unattached upload may exist. No retry.")))?;
        ids.push(ack_id(&value).ok_or_else(||Failure::NotSent("Invalid upload acknowledgement; message not submitted. An unattached upload may exist.".into()))?.to_owned());
    }
    let mut messages = files;
    messages.path_segments_mut().unwrap().pop().push("messages");
    let body = json!({"content":{"text":text,"attachments":ids.iter().map(|id|json!({"type":"file","file_id":id})).collect::<Vec<_>>()},"request_id":request_id,"idempotency_token":request_id});
    let value = transport.message(messages, &body).map_err(|e| {
        Failure::Uncertain(format!(
            "Message submission not confirmed ({e}). Check the official app; do not resend."
        ))
    })?;
    Ok(ack_id(&value)
        .ok_or_else(|| {
            Failure::Uncertain(
                "Invalid message acknowledgement; check the official app before retrying.".into(),
            )
        })?
        .to_owned())
}

pub struct Metadata {
    pub mime: String,
    pub name: Option<String>,
    pub dimensions: Option<(u32, u32)>,
    pub download: Url,
}
pub fn metadata_endpoint(room: &BoundRoom, file_id: &str) -> Result<Url, &'static str> {
    endpoint(&room.room, Some(file_id))
}
pub fn parse_metadata(value: &Value) -> Result<Metadata, &'static str> {
    // Dot human uploads may omit a specific MIME. This is permission to
    // inspect bounded bytes, not proof of image content; fetch validates them
    // with the original Grok validator and a full bounded pixel decode.
    let mime = value
        .get("mime_type")
        .and_then(Value::as_str)
        .filter(|s| {
            matches!(
                *s,
                "application/octet-stream"
                    | "image/png"
                    | "image/jpeg"
                    | "image/webp"
                    | "image/gif"
                    | "image/bmp"
                    | "image/tiff"
            )
        })
        .ok_or("unsupported image metadata")?
        .to_owned();
    let download = Url::parse(
        value
            .get("download_url")
            .and_then(Value::as_str)
            .ok_or("missing download URL")?,
    )
    .map_err(|_| "invalid download URL")?;
    validate_download_url(&download)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(|s| crate::sanitize::strip_control_chars(s, false));
    let dimensions = value.get("image_metadata").and_then(|v| {
        Some((
            u32::try_from(v.get("width")?.as_u64()?).ok()?,
            u32::try_from(v.get("height")?.as_u64()?).ok()?,
        ))
    });
    Ok(Metadata {
        mime,
        name,
        dimensions,
        download,
    })
}

pub fn validate_download_url(url: &Url) -> Result<(), &'static str> {
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("unsafe image download URL");
    }
    let host = url.host_str().ok_or("missing image download host")?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return Err("local image download host refused");
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>()
        && !crate::image_download::public_ip(ip)
    {
        return Err("private image download address refused");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        calls: Vec<String>,
        fail_upload: bool,
        fail_send: bool,
        bad_upload_ack: bool,
        bad_message_ack: bool,
    }
    impl Transport for Fake {
        fn upload(
            &mut self,
            url: Url,
            field: &str,
            mime: &str,
            bytes: &[u8],
        ) -> Result<Value, String> {
            self.calls.push("upload".into());
            assert_eq!(
                url.as_str(),
                "https://chatgpt.com/backend-api/messaging/rooms/room/files"
            );
            assert_eq!(field, "file");
            assert_eq!(mime, "image/png");
            attachments::decode_bounded(bytes).unwrap();
            if self.fail_upload {
                Err("synthetic transport failure".into())
            } else if self.bad_upload_ack {
                Ok(json!({}))
            } else {
                Ok(json!({"id":"file-1"}))
            }
        }
        fn message(&mut self, url: Url, body: &Value) -> Result<Value, String> {
            self.calls.push("message".into());
            assert_eq!(url.path(), "/backend-api/messaging/rooms/room/messages");
            assert_eq!(
                body["content"]["attachments"],
                json!([{"type":"file","file_id":"file-1"}])
            );
            assert_eq!(body["request_id"], body["idempotency_token"]);
            if self.fail_send {
                Err("synthetic transport failure".into())
            } else if self.bad_message_ack {
                Ok(json!({}))
            } else {
                Ok(json!({"id":"message-1"}))
            }
        }
    }
    fn fixture() -> (Fake, BoundRoom, LocalImage, String) {
        let dot = Dot {
            id: "dot".into(),
            name: "test".into(),
            room: Some("room".into()),
            speakers: vec![],
        };
        let room = BoundRoom::from_unique(&[dot], "room").unwrap();
        let mut b = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(16, 16)
            .write_to(&mut b, image::ImageFormat::Png)
            .unwrap();
        (
            Fake {
                calls: vec![],
                fail_upload: false,
                fail_send: false,
                bad_upload_ack: false,
                bad_message_ack: false,
            },
            room,
            LocalImage::stage(b.into_inner()).unwrap(),
            uuid::Uuid::new_v4().to_string(),
        )
    }
    #[test]
    fn synthetic_chip_painted_preview_upload_and_attachment_message_end_to_end() {
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::None,
        );
        let (mut service, room, mut image, request_id) = fixture();
        let mut composer = crate::image_composer::ImageComposer::default();
        composer.stage_bytes(image.bytes.to_vec(), None).unwrap();
        assert_eq!(composer.textarea.text(), "[Image #1] ");
        let chip = &composer.images[0];
        chip.prepare_preview_blocking();
        let area = ratatui::layout::Rect::new(2, 3, 100, 30);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        let (painted, escapes) = crate::render::image_overlay::render_image_overlay_checked(
            &mut buf,
            area,
            chip,
            ratatui::style::Color::Black,
            ratatui::style::Color::White,
            ratatui::style::Color::Gray,
        );
        assert!(painted);
        assert!(escapes.is_none());
        assert!(
            buf.content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("Image #1")
        );
        image.record_preview(painted);
        assert!(service.calls.is_empty());
        assert_eq!(
            transmit(&mut service, &room, "合成图片", &[image], &request_id),
            Ok("message-1".into())
        );
        assert_eq!(service.calls, vec!["upload", "message"]);
    }
    #[test]
    fn staging_and_unpainted_preview_never_upload() {
        let (mut f, r, mut i, id) = fixture();
        assert!(f.calls.is_empty());
        i.record_preview(false);
        assert!(transmit(&mut f, &r, "", &[i], &id).is_err());
        assert!(f.calls.is_empty());
    }
    #[test]
    fn upload_then_attachment_message_exact_schema() {
        let (mut f, r, mut i, id) = fixture();
        i.record_preview(true);
        assert_eq!(
            transmit(&mut f, &r, "图片", &[i], &id),
            Ok("message-1".into())
        );
        assert_eq!(f.calls, vec!["upload", "message"]);
    }
    #[test]
    fn upload_failure_never_submits_message_and_unlocks_for_explicit_retry() {
        let (mut f, r, mut i, id) = fixture();
        i.record_preview(true);
        f.fail_upload = true;
        assert!(transmit(&mut f, &r, "", std::slice::from_ref(&i), &id).is_err());
        assert_eq!(f.calls, vec!["upload"]); // No automatic retry or message POST.
    }
    #[test]
    fn send_uncertain_never_automatically_repeats_upload() {
        let (mut f, r, mut i, id) = fixture();
        i.record_preview(true);
        f.fail_send = true;
        assert!(transmit(&mut f, &r, "", std::slice::from_ref(&i), &id).is_err());
        assert_eq!(f.calls, vec!["upload", "message"]);
    }
    #[test]
    fn invalid_ack_preserves_exact_attempt_stage() {
        let (mut f, r, mut i, id) = fixture();
        i.record_preview(true);
        f.bad_upload_ack = true;
        assert!(matches!(
            transmit(&mut f, &r, "", std::slice::from_ref(&i), &id),
            Err(Failure::NotSent(_))
        ));
        assert_eq!(f.calls, vec!["upload"]);
        let (mut f, r, mut i, id) = fixture();
        i.record_preview(true);
        f.bad_message_ack = true;
        assert!(matches!(
            transmit(&mut f, &r, "", std::slice::from_ref(&i), &id),
            Err(Failure::Uncertain(_))
        ));
        assert_eq!(f.calls, vec!["upload", "message"]);
    }
    #[test]
    fn metadata_stays_room_scoped_and_external_origin_rejected() {
        let (_, r, _, _) = fixture();
        assert_eq!(
            metadata_endpoint(&r, "f/1").unwrap().path(),
            "/backend-api/messaging/rooms/room/files/f%2F1"
        );
        assert!(
            parse_metadata(&json!({"mime_type":"image/png","download_url":"http://example.org/x"}))
                .is_err()
        );
        assert!(
            parse_metadata(
                &json!({"mime_type":"image/png","download_url":"https://u@chatgpt.com/x"})
            )
            .is_err()
        );
        let m=parse_metadata(&json!({"mime_type":"image/png","name":"x\u{1b}.png","download_url":"https://chatgpt.com/x","image_metadata":{"width":16,"height":12}})).unwrap();
        assert_eq!(m.name.as_deref(), Some("x .png"));
        assert_eq!(m.dimensions, Some((16, 12)));
    }
    #[test]
    fn ambiguous_or_changed_room_rejected() {
        assert!(BoundRoom::from_unique(&[], "room").is_err());
        let d = Dot {
            id: "d".into(),
            name: "n".into(),
            room: Some("room".into()),
            speakers: vec![],
        };
        assert!(BoundRoom::from_unique(&[d.clone(), d.clone()], "room").is_err());
        assert!(BoundRoom::from_unique(&[d], "other").is_err());
    }
}
