//! PNG pixels: decoding to 8-bit RGBA, encoding from it.
//!
//! Decoded: every colour type (grey, RGB, palette, grey+alpha, RGBA), bit
//! depths 1 to 16 (16-bit keeps the high byte), `tRNS` transparency, all five
//! row filters. Not decoded: interlaced images (Adam7) — rare in packs; an
//! error, never a wrong picture. Every chunk's CRC is checked, sizes are
//! checked before anything is allocated.

use super::{crc32, zlib_compress, zlib_decompress};

/// Above this many pixels an image is refused (8192×8192).
pub const MAX_PIXELS: u64 = 8192 * 8192;
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// An image as 8-bit RGBA, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn new(width: u32, height: u32) -> Self {
        Rgba {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// The picture as it is SEEN: a fully transparent pixel has no colour, so
    /// two images that differ only under alpha 0 compare equal.
    pub fn visible(mut self) -> Self {
        for p in self.pixels.as_chunks_mut::<4>().0 {
            if p[3] == 0 {
                *p = [0, 0, 0, 0];
            }
        }
        self
    }

    /// The `w`×`h` region at (`x`, `y`), if it is inside the image.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Rgba> {
        if x.checked_add(w)? > self.width || y.checked_add(h)? > self.height {
            return None;
        }
        let mut out = Rgba::new(w, h);
        let row = w as usize * 4;
        for r in 0..h as usize {
            let src = ((y as usize + r) * self.width as usize + x as usize) * 4;
            out.pixels[r * row..(r + 1) * row].copy_from_slice(&self.pixels[src..src + row]);
        }
        Some(out)
    }

    /// Copy `src` into this image at (`x`, `y`); `false` if it does not fit.
    pub fn paste(&mut self, src: &Rgba, x: u32, y: u32) -> bool {
        if x.checked_add(src.width).is_none_or(|r| r > self.width)
            || y.checked_add(src.height).is_none_or(|b| b > self.height)
        {
            return false;
        }
        let row = src.width as usize * 4;
        for r in 0..src.height as usize {
            let dst = ((y as usize + r) * self.width as usize + x as usize) * 4;
            self.pixels[dst..dst + row].copy_from_slice(&src.pixels[r * row..(r + 1) * row]);
        }
        true
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Decode a PNG file's bytes.
pub fn decode(data: &[u8]) -> Result<Rgba, String> {
    if !data.starts_with(&SIGNATURE) {
        return Err("not a PNG".into());
    }
    let mut pos = 8;
    let mut header: Option<(u32, u32, u8, u8)> = None;
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    loop {
        let head = data.get(pos..pos + 8).ok_or("PNG ends before IEND")?;
        let len = be32(&head[..4]) as usize;
        let kind: [u8; 4] = [head[4], head[5], head[6], head[7]];
        let body = data
            .get(pos + 8..pos + 8 + len)
            .ok_or("PNG chunk past the end")?;
        let crc = data
            .get(pos + 8 + len..pos + 12 + len)
            .ok_or("PNG chunk without CRC")?;
        if be32(crc) != crc32(&data[pos + 4..pos + 8 + len]) {
            return Err("PNG chunk CRC mismatch".into());
        }
        pos += 12 + len;
        match &kind {
            b"IHDR" => {
                if body.len() != 13 {
                    return Err("bad IHDR".into());
                }
                let (w, h) = (be32(&body[0..4]), be32(&body[4..8]));
                let (depth, color, interlace) = (body[8], body[9], body[12]);
                if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
                    return Err("PNG size out of range".into());
                }
                if interlace != 0 {
                    return Err("interlaced PNG not supported".into());
                }
                let ok = match color {
                    0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(depth, 8 | 16),
                    _ => false,
                };
                if !ok {
                    return Err("PNG colour type / depth not allowed".into());
                }
                header = Some((w, h, depth, color));
            }
            b"PLTE" => {
                palette = body
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|&[r, g, b]| [r, g, b, 255])
                    .collect()
            }
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
    }
    let (w, h, depth, color) = header.ok_or("PNG without IHDR")?;
    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let bits_pp = channels * depth as usize;
    let stride = (w as usize * bits_pp).div_ceil(8);
    let bpp = bits_pp.div_ceil(8).max(1);
    let raw = zlib_decompress(&idat, (stride + 1) * h as usize).map_err(|e| e.to_string())?;
    if raw.len() != (stride + 1) * h as usize {
        return Err("PNG data of the wrong size".into());
    }
    // unfilter in place, row by row
    let mut rows = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, cur) = rows.split_at_mut(y * stride);
        let prior = if y == 0 {
            None
        } else {
            Some(&done[(y - 1) * stride..])
        };
        let cur = &mut cur[..stride];
        for i in 0..stride {
            let a = if i >= bpp { cur[i - bpp] as i16 } else { 0 };
            let b = prior.map_or(0, |p| p[i] as i16);
            let c = if i >= bpp {
                prior.map_or(0, |p| p[i - bpp] as i16)
            } else {
                0
            };
            let pred = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => return Err("unknown PNG filter".into()),
            };
            cur[i] = src[i].wrapping_add(pred as u8);
        }
    }
    // to RGBA
    let mut out = Rgba::new(w, h);
    let sample = |row: &[u8], x: usize, ch: usize| -> u16 {
        match depth {
            16 => {
                let i = (x * channels + ch) * 2;
                u16::from_be_bytes([row[i], row[i + 1]])
            }
            8 => row[x * channels + ch] as u16,
            d => {
                let bit = x * d as usize;
                ((row[bit / 8] >> (8 - d as usize - bit % 8)) & ((1 << d) - 1)) as u16
            }
        }
    };
    let to8 = |v: u16| -> u8 {
        match depth {
            16 => (v >> 8) as u8,
            8 => v as u8,
            d => (v as u32 * 255 / ((1u32 << d) - 1)) as u8,
        }
    };
    let trns_key = |ch: usize| {
        trns.get(ch * 2..ch * 2 + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    for y in 0..h as usize {
        let row = &rows[y * stride..(y + 1) * stride];
        for x in 0..w as usize {
            let px = match color {
                3 => {
                    let i = sample(row, x, 0) as usize;
                    let mut p = *palette.get(i).ok_or("PNG palette index out of range")?;
                    if let Some(&a) = trns.get(i) {
                        p[3] = a;
                    }
                    p
                }
                0 => {
                    let v = sample(row, x, 0);
                    let g = to8(v);
                    [g, g, g, if trns_key(0) == Some(v) { 0 } else { 255 }]
                }
                4 => {
                    let g = to8(sample(row, x, 0));
                    [g, g, g, to8(sample(row, x, 1))]
                }
                2 => {
                    let (r, g, b) = (sample(row, x, 0), sample(row, x, 1), sample(row, x, 2));
                    let clear =
                        trns_key(0) == Some(r) && trns_key(1) == Some(g) && trns_key(2) == Some(b);
                    [to8(r), to8(g), to8(b), if clear { 0 } else { 255 }]
                }
                _ => [
                    to8(sample(row, x, 0)),
                    to8(sample(row, x, 1)),
                    to8(sample(row, x, 2)),
                    to8(sample(row, x, 3)),
                ],
            };
            let i = (y * w as usize + x) * 4;
            out.pixels[i..i + 4].copy_from_slice(&px);
        }
    }
    Ok(out)
}

/// Encode as an 8-bit RGBA PNG.
pub fn encode(img: &Rgba) -> Vec<u8> {
    let row = img.width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * img.height as usize);
    for y in 0..img.height as usize {
        // Sub filter: neighbouring pixels of pixel art are often equal
        raw.push(1);
        let src = &img.pixels[y * row..(y + 1) * row];
        for i in 0..row {
            raw.push(src[i].wrapping_sub(if i >= 4 { src[i - 4] } else { 0 }));
        }
    }
    let mut out = SIGNATURE.to_vec();
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend((body.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(body);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(img.width.to_be_bytes());
    ihdr.extend(img.height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &zlib_compress(&raw));
    chunk(b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Rgba {
        let mut img = Rgba::new(5, 3);
        for (i, p) in img.pixels.iter_mut().enumerate() {
            *p = (i * 37 % 256) as u8;
        }
        img
    }

    #[test]
    fn encode_then_decode_is_the_identity() {
        let img = sample();
        assert_eq!(decode(&encode(&img)).unwrap(), img);
    }

    #[test]
    fn crop_paste_and_visible() {
        let img = sample();
        let part = img.crop(1, 1, 3, 2).unwrap();
        assert_eq!(part.pixel(0, 0), img.pixel(1, 1));
        assert!(img.crop(4, 0, 2, 1).is_none(), "past the edge");
        let mut blank = Rgba::new(5, 3);
        assert!(blank.paste(&part, 1, 1));
        assert_eq!(blank.crop(1, 1, 3, 2).unwrap(), part);
        assert!(!blank.paste(&part, 3, 2));
        let mut hidden = Rgba::new(1, 1);
        hidden.pixels = vec![9, 9, 9, 0];
        assert_eq!(hidden.visible().pixels, [0, 0, 0, 0]);
    }

    #[test]
    fn damage_is_an_error_never_a_panic() {
        let png = encode(&sample());
        for cut in 0..png.len() {
            assert!(decode(&png[..cut]).is_err());
        }
        for i in 8..png.len() {
            let mut d = png.clone();
            d[i] ^= 0x41;
            let _ = decode(&d);
        }
    }
}
