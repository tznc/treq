//! Images downloaded at run time, such as avatars.

use std::sync::Arc;

use gpui::{Image, ImageFormat};

/// The format `bytes` start with: PNG, JPEG, GIF or WebP.
pub fn format(bytes: &[u8]) -> Option<ImageFormat> {
  match bytes {
    [0x89, b'P', b'N', b'G', ..] => Some(ImageFormat::Png),
    [0xff, 0xd8, ..] => Some(ImageFormat::Jpeg),
    [b'G', b'I', b'F', b'8', ..] => Some(ImageFormat::Gif),
    [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some(ImageFormat::Webp),
    _ => None,
  }
}

/// `bytes` as an image GPUI can draw, when they are one it knows.
pub fn decode(bytes: Vec<u8>) -> Option<Arc<Image>> {
  let format = format(&bytes)?;
  Some(Arc::new(Image::from_bytes(format, bytes)))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decodes_known_formats_only() {
    assert_eq!(format(b"\x89PNG\r\n"), Some(ImageFormat::Png));
    assert_eq!(format(&[0xff, 0xd8, 0xff]), Some(ImageFormat::Jpeg));
    assert_eq!(format(b"RIFF\0\0\0\0WEBPVP8"), Some(ImageFormat::Webp));
    assert!(decode(b"<html>".to_vec()).is_none());
  }
}
