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

/// The image's rows without their PNG filters: `raw` is `height` rows of
/// one filter byte and `stride` bytes; `bpp` is the bytes per pixel (at
/// least 1). With the `native` feature, the Zig kernel (the same bytes:
/// see the differential tests); otherwise `unfilter_rust`.
pub fn unfilter(raw: &[u8], stride: usize, height: usize, bpp: usize) -> Result<Vec<u8>, String> {
    #[cfg(feature = "native")]
    if let Some(r) = kebitx_native::unfilter(raw, stride, height, bpp) {
        return r.map_err(str::to_string);
    }
    unfilter_rust(raw, stride, height, bpp)
}

/// kebitx's own unfiltering: a loop per filter, std only.
pub fn unfilter_rust(
    raw: &[u8],
    stride: usize,
    height: usize,
    bpp: usize,
) -> Result<Vec<u8>, String> {
    if raw.len() != (stride + 1) * height {
        return Err("PNG data of the wrong size".into());
    }
    let bpp = bpp.min(stride).max(1);
    let mut rows = vec![0u8; stride * height];
    // the row above the first is all zeros (PNG's rule)
    let zeros = vec![0u8; stride];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, cur) = rows.split_at_mut(y * stride);
        let prior = if y == 0 {
            &zeros[..]
        } else {
            &done[(y - 1) * stride..]
        };
        unfilter_row(filter, src, prior, &mut cur[..stride], bpp)?;
    }
    Ok(rows)
}

/// One row: a loop per filter, so nothing is decided per byte.
fn unfilter_row(
    filter: u8,
    src: &[u8],
    prior: &[u8],
    cur: &mut [u8],
    bpp: usize,
) -> Result<(), String> {
    let n = cur.len();
    let (src, prior) = (&src[..n], &prior[..n]);
    match filter {
        0 => cur.copy_from_slice(src),
        1 => {
            cur[..bpp].copy_from_slice(&src[..bpp]);
            for i in bpp..n {
                cur[i] = src[i].wrapping_add(cur[i - bpp]);
            }
        }
        2 => {
            for ((c, &s), &p) in cur.iter_mut().zip(src).zip(prior) {
                *c = s.wrapping_add(p);
            }
        }
        3 => {
            for i in 0..bpp {
                cur[i] = src[i].wrapping_add(prior[i] >> 1);
            }
            for i in bpp..n {
                let avg = ((cur[i - bpp] as u16 + prior[i] as u16) >> 1) as u8;
                cur[i] = src[i].wrapping_add(avg);
            }
        }
        4 => {
            // on the left edge a = c = 0, so Paeth predicts b
            for i in 0..bpp {
                cur[i] = src[i].wrapping_add(prior[i]);
            }
            for i in bpp..n {
                cur[i] = src[i].wrapping_add(paeth(cur[i - bpp], prior[i], prior[i - bpp]));
            }
        }
        _ => return Err("unknown PNG filter".into()),
    }
    Ok(())
}

/// The Paeth predictor (PNG 9.4), without branches on the data.
#[inline(always)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a16, b16, c16) = (a as i16, b as i16, c as i16);
    let pa = (b16 - c16).abs();
    let pb = (a16 - c16).abs();
    let pc = (a16 + b16 - 2 * c16).abs();
    let use_a = pa <= pb && pa <= pc;
    let use_b = pb <= pc;
    if use_a {
        a
    } else if use_b {
        b
    } else {
        c
    }
}

/// Decode a PNG file's bytes.
pub fn decode(data: &[u8]) -> Result<Rgba, String> {
    if !data.starts_with(&SIGNATURE) {
        return Err("not a PNG".into());
    }
    let mut pos = 8;
    let mut header: Option<(u32, u32, u8, u8, u8)> = None;
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
        pos += 12 + len;
        if be32(crc) != crc32(&data[pos - 8 - len..pos - 4]) {
            // a critical chunk (upper-case first letter) is the image: its
            // damage is an error. An ancillary one (a colour profile, text)
            // may be ignored, as the PNG specification allows and libpng
            // does: six of the game's own UI textures have a damaged iCCP
            if kind[0].is_ascii_uppercase() {
                return Err("PNG chunk CRC mismatch".into());
            }
            continue;
        }
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
                if interlace > 1 {
                    return Err("unknown PNG interlace method".into());
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
                header = Some((w, h, depth, color, interlace));
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
    let (w, h, depth, color, interlace) = header.ok_or("PNG without IHDR")?;
    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let fmt = Format {
        depth,
        color,
        channels,
        palette: &palette,
        trns: &trns,
    };
    let bits_pp = channels * depth as usize;
    let bpp = bits_pp.div_ceil(8).max(1);
    let stride_of = |width: u32| (width as usize * bits_pp).div_ceil(8);
    if interlace == 0 {
        let stride = stride_of(w);
        let raw = zlib_decompress(&idat, (stride + 1) * h as usize).map_err(|e| e.to_string())?;
        if raw.len() != (stride + 1) * h as usize {
            return Err("PNG data of the wrong size".into());
        }
        let rows = unfilter(&raw, stride, h as usize, bpp)?;
        return to_rgba(&rows, w, h, stride, &fmt);
    }
    // Adam7 (PNG 8.2): seven reduced images, each filtered on its own, whose
    // pixels go to every `dx`-th column and `dy`-th row from (`x0`, `y0`);
    // an empty pass has no bytes at all
    const PASSES: [(u32, u32, u32, u32); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let size = |start: u32, total: u32, step: u32| total.saturating_sub(start).div_ceil(step);
    let passes: Vec<(u32, u32, usize)> = PASSES
        .iter()
        .map(|&(x0, y0, dx, dy)| {
            let (pw, ph) = (size(x0, w, dx), size(y0, h, dy));
            (pw, ph, stride_of(pw))
        })
        .collect();
    let total: usize = passes
        .iter()
        .filter(|(pw, ph, _)| *pw > 0 && *ph > 0)
        .map(|(_, ph, stride)| (stride + 1) * *ph as usize)
        .sum();
    let raw = zlib_decompress(&idat, total).map_err(|e| e.to_string())?;
    if raw.len() != total {
        return Err("PNG data of the wrong size".into());
    }
    let mut out = Rgba::new(w, h);
    let mut at = 0;
    for (&(x0, y0, dx, dy), &(pw, ph, stride)) in PASSES.iter().zip(&passes) {
        if pw == 0 || ph == 0 {
            continue;
        }
        let len = (stride + 1) * ph as usize;
        let rows = unfilter(&raw[at..at + len], stride, ph as usize, bpp)?;
        at += len;
        let pass = to_rgba(&rows, pw, ph, stride, &fmt)?;
        for j in 0..ph as usize {
            for i in 0..pw as usize {
                let (x, y) = (x0 as usize + i * dx as usize, y0 as usize + j * dy as usize);
                let to = (y * w as usize + x) * 4;
                let from = (j * pw as usize + i) * 4;
                out.pixels[to..to + 4].copy_from_slice(&pass.pixels[from..from + 4]);
            }
        }
    }
    Ok(out)
}

/// What the rows' samples mean.
struct Format<'a> {
    depth: u8,
    color: u8,
    channels: usize,
    palette: &'a [[u8; 4]],
    trns: &'a [u8],
}

/// Unfiltered rows (`stride` bytes each) -> RGBA pixels.
fn to_rgba(rows: &[u8], w: u32, h: u32, stride: usize, fmt: &Format) -> Result<Rgba, String> {
    let Format {
        depth,
        color,
        channels,
        palette,
        trns,
    } = *fmt;
    // the two formats nearly every texture uses (the game's: 98 % of its
    // pixels), without a decision per pixel
    if depth == 8 && color == 6 && stride == w as usize * 4 {
        return Ok(Rgba {
            width: w,
            height: h,
            pixels: rows[..stride * h as usize].to_vec(),
        });
    }
    if depth == 8 && color == 2 && trns.is_empty() && stride == w as usize * 3 {
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for &[r, g, b] in rows[..stride * h as usize].as_chunks::<3>().0 {
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
        return Ok(Rgba {
            width: w,
            height: h,
            pixels,
        });
    }
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

    /// A PNG from chunks, each with its CRC (or a wrong one when asked).
    fn png_of(chunks: &[(&[u8; 4], Vec<u8>, bool)]) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        for (kind, body, good_crc) in chunks {
            out.extend((body.len() as u32).to_be_bytes());
            let start = out.len();
            out.extend(*kind);
            out.extend(body);
            let crc = crc32(&out[start..]) ^ if *good_crc { 0 } else { 1 };
            out.extend(crc.to_be_bytes());
        }
        out
    }

    /// Adam7 written by hand, as an encoder does: seven reduced images, each
    /// row behind its filter byte (Sub and Up, so unfiltering runs per pass).
    fn interlaced(img: &Rgba, color: u8) -> Vec<u8> {
        let ch = if color == 6 { 4 } else { 3 };
        let passes = [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ];
        let mut raw = Vec::new();
        for (x0, y0, dx, dy) in passes {
            let pw = img.width.saturating_sub(x0).div_ceil(dx);
            let ph = img.height.saturating_sub(y0).div_ceil(dy);
            if pw == 0 || ph == 0 {
                continue;
            }
            let mut prior = vec![0u8; pw as usize * ch];
            for j in 0..ph {
                let mut row = Vec::new();
                for i in 0..pw {
                    let p = img.pixel(x0 + i * dx, y0 + j * dy);
                    row.extend(&p[..ch]);
                }
                let up = j % 2 == 1;
                raw.push(if up { 2 } else { 1 });
                for k in 0..row.len() {
                    let pred = if up {
                        prior[k]
                    } else if k >= ch {
                        row[k - ch]
                    } else {
                        0
                    };
                    raw.push(row[k].wrapping_sub(pred));
                }
                prior = row;
            }
        }
        let mut ihdr = Vec::new();
        ihdr.extend(img.width.to_be_bytes());
        ihdr.extend(img.height.to_be_bytes());
        ihdr.extend([8, color, 0, 0, 1]);
        png_of(&[
            (b"IHDR", ihdr, true),
            (b"IDAT", zlib_compress(&raw), true),
            (b"IEND", Vec::new(), true),
        ])
    }

    #[test]
    fn interlaced_images_decode_to_their_pixels() {
        for (w, h) in [
            (1, 1),
            (2, 1),
            (1, 9),
            (3, 5),
            (8, 8),
            (9, 9),
            (13, 7),
            (33, 17),
        ] {
            let mut img = Rgba::new(w, h);
            for (i, p) in img.pixels.iter_mut().enumerate() {
                *p = (i * 53 % 251) as u8;
            }
            assert_eq!(decode(&interlaced(&img, 6)).unwrap(), img, "{w}x{h} RGBA");
            // RGB: the same picture, opaque
            let mut opaque = img.clone();
            for p in opaque.pixels.as_chunks_mut::<4>().0 {
                p[3] = 255;
            }
            assert_eq!(
                decode(&interlaced(&opaque, 2)).unwrap(),
                opaque,
                "{w}x{h} RGB"
            );
            // the same pixels as the plain (not interlaced) encoding
            assert_eq!(decode(&encode(&img)).unwrap(), img);
        }
    }

    #[test]
    fn a_damaged_ancillary_chunk_is_ignored_a_critical_one_is_not() {
        let img = sample();
        let plain = encode(&img);
        // the encoder's IHDR, IDAT and IEND, with an ancillary chunk between
        let ihdr_end = 8 + 12 + 13;
        let mut with_text = plain[..ihdr_end].to_vec();
        with_text.extend(png_of(&[(b"iCCP", b"profile".to_vec(), false)])[8..].to_vec());
        with_text.extend(&plain[ihdr_end..]);
        assert_eq!(
            decode(&with_text).unwrap(),
            img,
            "a damaged iCCP is skipped"
        );

        // a damaged IDAT is the image itself: refused
        let mut bad_idat = plain.clone();
        let idat_crc = bad_idat.len() - 12 - 4;
        bad_idat[idat_crc] ^= 1;
        assert_eq!(decode(&bad_idat), Err("PNG chunk CRC mismatch".into()));
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
