//! Image format and dimensions from the header alone — no decoding, no
//! dependencies.
//!
//! The format is told by the CONTENT, not the extension: real packs ship JPEG
//! files named `.png` (21 in the reference world), and reading them as PNG
//! gave absurd sizes. PNG: signature, then IHDR with width and height as
//! big-endian u32 at bytes 16..24. JPEG: `FF D8`, then markers up to a
//! start-of-frame, which holds height and width. TGA has no signature: only a
//! `.tga` file is read as one (18-byte header, little-endian u16 sizes at
//! 12..16). Anything that does not check out is `None`: a size that cannot be
//! read is never guessed.

use std::fs::File;
use std::io::Read;
use std::path::Path;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// How much of a file is read to find a JPEG's frame header (EXIF data can
/// come first and be large).
const JPEG_HEAD: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Tga,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: Format,
    pub width: u32,
    pub height: u32,
}

/// `(width, height)` in pixels.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    probe(path).map(|i| (i.width, i.height))
}

/// Size of an image already in memory, told by its content; `is_tga` says
/// whether to try TGA, which has no signature.
pub fn size_of_bytes(b: &[u8], is_tga: bool) -> Option<(u32, u32)> {
    if b.starts_with(&PNG_SIGNATURE) {
        png(b)
    } else if b.starts_with(&[0xFF, 0xD8]) {
        jpeg(b)
    } else if is_tga {
        tga(b)
    } else {
        None
    }
}

/// Format (by content) and size.
pub fn probe(path: &Path) -> Option<ImageInfo> {
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; 24];
    let n = read_up_to(&mut f, &mut head)?;
    let head = &head[..n];
    if head.starts_with(&PNG_SIGNATURE) {
        let (width, height) = png(head)?;
        return Some(ImageInfo {
            format: Format::Png,
            width,
            height,
        });
    }
    if head.starts_with(&[0xFF, 0xD8]) {
        let mut rest = vec![0u8; JPEG_HEAD];
        let m = read_up_to(&mut f, &mut rest)?;
        let mut all = head.to_vec();
        all.extend_from_slice(&rest[..m]);
        let (width, height) = jpeg(&all)?;
        return Some(ImageInfo {
            format: Format::Jpeg,
            width,
            height,
        });
    }
    let is_tga = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tga"));
    if is_tga {
        let (width, height) = tga(head)?;
        return Some(ImageInfo {
            format: Format::Tga,
            width,
            height,
        });
    }
    None
}

/// Width and height from a JPEG's start-of-frame marker (any SOFn except the
/// DHT/JPG/DAC codes that share the range). Every segment length is checked.
pub fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    loop {
        // markers may be padded with extra 0xFF bytes
        while *b.get(i)? == 0xFF && *b.get(i + 1)? == 0xFF {
            i += 1;
        }
        if *b.get(i)? != 0xFF {
            return None;
        }
        let marker = *b.get(i + 1)?;
        let len = u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]) as usize;
        if len < 2 {
            return None;
        }
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let h = u16::from_be_bytes([*b.get(i + 5)?, *b.get(i + 6)?]) as u32;
            let w = u16::from_be_bytes([*b.get(i + 7)?, *b.get(i + 8)?]) as u32;
            return (w > 0 && h > 0).then_some((w, h));
        }
        if marker == 0xDA {
            return None; // image data began before any frame header
        }
        i = i.checked_add(2 + len)?;
    }
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> Option<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]).ok()? {
            0 => break,
            k => n += k,
        }
    }
    Some(n)
}

pub fn png(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..8)? != PNG_SIGNATURE || b.get(12..16)? != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(b.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(b.get(20..24)?.try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

pub fn tga(b: &[u8]) -> Option<(u32, u32)> {
    // image type 2/3/10/11 (true-colour / grey, raw or RLE) are what textures use
    if !matches!(*b.get(2)?, 2 | 3 | 10 | 11) {
        return None;
    }
    let w = u16::from_le_bytes(b.get(12..14)?.try_into().ok()?) as u32;
    let h = u16::from_le_bytes(b.get(14..16)?.try_into().ok()?) as u32;
    (w > 0 && h > 0).then_some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_and_tga_headers() {
        let mut p = PNG_SIGNATURE.to_vec();
        p.extend([0, 0, 0, 13]);
        p.extend(b"IHDR");
        p.extend(64u32.to_be_bytes());
        p.extend(32u32.to_be_bytes());
        assert_eq!(png(&p), Some((64, 32)));

        let mut t = vec![0u8; 18];
        t[2] = 2;
        t[12..14].copy_from_slice(&128u16.to_le_bytes());
        t[14..16].copy_from_slice(&64u16.to_le_bytes());
        assert_eq!(tga(&t), Some((128, 64)));
    }

    #[test]
    fn jpeg_size_from_its_frame_header() {
        // SOI, an APP0 segment, then SOF0 with height 32 and width 64
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0xAA, 0xBB];
        j.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x20, 0x00, 0x40, 0x03]);
        assert_eq!(jpeg(&j), Some((64, 32)));
        // truncated anywhere: None, never a panic or a guess
        for cut in 0..j.len() {
            let _ = jpeg(&j[..cut]);
        }
        assert_eq!(jpeg(&j[..12]), None);
    }

    #[test]
    fn bad_headers_are_none() {
        assert_eq!(png(b""), None);
        assert_eq!(png(b"not a png at all, not at all"), None);
        assert_eq!(tga(&[0u8; 5]), None);
        assert_eq!(tga(&[0u8; 18]), None, "type 0: no image");
    }
}
