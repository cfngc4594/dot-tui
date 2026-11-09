//! Linux/Windows use the same arboard backend as Grok Build 2bdd1d6
//! xai-grok-shared/src/clipboard.rs (Apache-2.0). Dot adapts it to one bounded
//! worker: keeps X11 clipboard ownership alive and serializes Windows access.
//! No clipboard access at construction; only explicit paste/copy requests.
use std::sync::{OnceLock, mpsc};
use std::time::Duration;
type Reply = Result<Content, String>;
pub enum Content {
    Text(Option<String>),
    Image(Option<Vec<u8>>),
    Written,
}
enum Op {
    Text,
    Image,
    Write(String),
}
struct Request(Op, mpsc::Sender<Reply>);
fn service() -> &'static Result<mpsc::SyncSender<Request>, String> {
    static SERVICE: OnceLock<Result<mpsc::SyncSender<Request>, String>> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Request>(1);
        std::thread::Builder::new().name("dot-clipboard".into()).spawn(move || {
            let mut clipboard = None;
            while let Ok(Request(op, reply)) = rx.recv() {
                if clipboard.is_none() {
                    match arboard::Clipboard::new() {
                        Ok(c) => clipboard = Some(c),
                        Err(_) => { let _ = reply.send(Err("System clipboard unavailable; use terminal paste or @ to select a file".into())); continue; }
                    }
                }
                let c = clipboard.as_mut().unwrap();
                let result = match op {
                    Op::Text => match c.get_text() {
                        Ok(s) if s.len() <= 1024 * 1024 => Ok(Content::Text((!s.is_empty()).then_some(s))),
                        Ok(_) => Err("Clipboard text exceeds 1 MiB".into()),
                        Err(arboard::Error::ContentNotAvailable) => Ok(Content::Text(None)),
                        Err(_) => Err("Cannot read system clipboard".into()),
                    },
                    Op::Image => match c.get_image() {
                        Ok(i) => encode(i.width, i.height, &i.bytes).map(|b| Content::Image(Some(b))),
                        Err(arboard::Error::ContentNotAvailable) => Ok(Content::Image(None)),
                        Err(_) => Err("Cannot read clipboard image; use @ to select a file".into()),
                    },
                    Op::Write(s) => c.set_text(s).map(|_| Content::Written).map_err(|_| "Cannot write system clipboard".into()),
                };
                let _ = reply.send(result);
            }
        }).map_err(|_| "Cannot start clipboard worker".to_string())?;
        Ok(tx)
    })
}
fn request(op: Op) -> Reply {
    let (tx, rx) = mpsc::channel();
    service()
        .as_ref()
        .map_err(Clone::clone)?
        .try_send(Request(op, tx))
        .map_err(|_| "Clipboard is busy".to_string())?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Clipboard timed out; use terminal paste or @ to select a file".to_string())?
}
pub fn get_text() -> Result<Option<String>, String> {
    match request(Op::Text)? {
        Content::Text(s) => Ok(s),
        _ => unreachable!(),
    }
}
pub fn get_image() -> Result<Option<Vec<u8>>, String> {
    match request(Op::Image)? {
        Content::Image(b) => Ok(b),
        _ => unreachable!(),
    }
}
pub fn set_text(text: &str) -> Result<(), String> {
    request(Op::Write(text.into())).map(|_| ())
}
fn encode(width: usize, height: usize, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let size = width.checked_mul(height).and_then(|n| n.checked_mul(4));
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || size != Some(bytes.len())
        || bytes.len() > 64 * 1024 * 1024
    {
        return Err("Clipboard image dimensions exceed supported limits".into());
    }
    let image = image::RgbaImage::from_raw(width as u32, height as u32, bytes.to_vec())
        .ok_or("Invalid clipboard image")?;
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|_| "Cannot encode clipboard image")?;
    Ok(out.into_inner())
}
#[cfg(test)]
mod tests {
    #[test]
    fn image_bytes_are_bounded_and_validated() {
        assert!(super::encode(1, 1, &[1, 2, 3, 255]).is_ok());
        assert!(super::encode(1, 1, &[]).is_err());
        assert!(super::encode(usize::MAX, 2, &[]).is_err());
    }
}
