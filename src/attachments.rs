//! Dot-specific bounded decoding around the original Grok image validator.
//! No file, clipboard, network, or account access occurs in this module.
use std::io::Cursor;

pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
const MAX_SIDE: u32 = 8192;

pub fn dimensions_checked(bytes: &[u8]) -> Result<(u32, u32, &'static str), &'static str> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("image exceeds 20 MiB");
    }
    let (w, h, mime) = xai_grok_image::validate_image_bytes_with(bytes, false)
        .map_err(|_| "invalid or unsupported image")?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS
    {
        return Err("image exceeds local preview pixel limit");
    }
    Ok((w, h, mime))
}

pub fn mime_from_bytes(bytes: &[u8]) -> &'static str {
    // Sniffing is used only for encoder selection; validation remains mandatory
    // before staging. This accepts the same magic-byte-only protocol fixtures
    // as the original Grok encoder tests.
    match image::guess_format(bytes).ok() {
        Some(image::ImageFormat::Png) => "image/png",
        Some(image::ImageFormat::Jpeg) => "image/jpeg",
        Some(image::ImageFormat::Gif) => "image/gif",
        Some(image::ImageFormat::WebP) => "image/webp",
        Some(image::ImageFormat::Bmp) => "image/bmp",
        Some(image::ImageFormat::Tiff) => "image/tiff",
        _ => "application/octet-stream",
    }
}

pub fn decode_bounded(bytes: &[u8]) -> Result<image::DynamicImage, &'static str> {
    let (w, h, _) = dimensions_checked(bytes)?;
    if image::guess_format(bytes).ok() == Some(image::ImageFormat::Jpeg)
        && !xai_grok_image::jpeg_reaches_eoi(bytes)
    {
        return Err("truncated JPEG");
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "unrecognized image")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "invalid image pixel data")?;
    if image.width() != w || image.height() != h {
        return Err("image dimensions changed");
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(w, h)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }
    #[test]
    fn accepts_synthetic_png_and_decodes() {
        let b = png(16, 12);
        assert_eq!(dimensions_checked(&b), Ok((16, 12, "image/png")));
        assert_eq!(decode_bounded(&b).unwrap().width(), 16);
    }
    #[test]
    fn rejects_encoded_size_before_header() {
        assert_eq!(
            dimensions_checked(&vec![0; MAX_FILE_BYTES + 1]),
            Err("image exceeds 20 MiB")
        );
    }
    #[test]
    fn rejects_pixel_bomb_before_decode() {
        let b = png(4001, 4000);
        assert_eq!(
            dimensions_checked(&b),
            Err("image exceeds local preview pixel limit")
        );
    }
    #[test]
    fn rejects_corruption_and_unknown_data() {
        assert!(decode_bounded(b"not an image").is_err());
        let mut b = png(16, 16);
        b.truncate(b.len() / 2);
        assert!(decode_bounded(&b).is_err());
    }
}
