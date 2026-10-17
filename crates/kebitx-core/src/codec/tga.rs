//! TGA pixels: true-colour and grey, raw or run-length encoded, 8/16(grey+
//! alpha)/24/32 bits, either origin. Colour-mapped TGA is refused.

use super::png::{Rgba, MAX_PIXELS};

pub fn decode(data: &[u8]) -> Result<Rgba, String> {
    let h = data.get(..18).ok_or("TGA header cut short")?;
    let (id_len, cmap_type, kind) = (h[0] as usize, h[1], h[2]);
    if cmap_type != 0 {
        return Err("colour-mapped TGA not supported".into());
    }
    let (w, ht) = (
        u16::from_le_bytes([h[12], h[13]]) as u32,
        u16::from_le_bytes([h[14], h[15]]) as u32,
    );
    let (depth, desc) = (h[16], h[17]);
    if w == 0 || ht == 0 || w as u64 * ht as u64 > MAX_PIXELS {
        return Err("TGA size out of range".into());
    }
    let grey = matches!(kind, 3 | 11);
    let rle = matches!(kind, 10 | 11);
    if !matches!(kind, 2 | 3 | 10 | 11) {
        return Err("TGA image type not supported".into());
    }
    let bpp = match (grey, depth) {
        (true, 8) => 1,
        (true, 16) => 2,
        (false, 24) => 3,
        (false, 32) => 4,
        _ => return Err("TGA depth not supported".into()),
    };
    let n = w as usize * ht as usize;
    let mut src = data.get(18 + id_len..).ok_or("TGA cut short")?;
    let mut raw: Vec<u8> = Vec::with_capacity(n * bpp);
    if rle {
        while raw.len() < n * bpp {
            let (&head, rest) = src.split_first().ok_or("TGA RLE cut short")?;
            let count = (head & 0x7f) as usize + 1;
            if head & 0x80 != 0 {
                let px = rest.get(..bpp).ok_or("TGA RLE cut short")?;
                for _ in 0..count {
                    raw.extend_from_slice(px);
                }
                src = &rest[bpp..];
            } else {
                let px = rest.get(..count * bpp).ok_or("TGA RLE cut short")?;
                raw.extend_from_slice(px);
                src = &rest[count * bpp..];
            }
        }
        raw.truncate(n * bpp);
    } else {
        raw.extend_from_slice(src.get(..n * bpp).ok_or("TGA pixel data cut short")?);
    }
    let top_down = desc & 0x20 != 0;
    let right_left = desc & 0x10 != 0;
    let mut out = Rgba::new(w, ht);
    for i in 0..n {
        let p = &raw[i * bpp..(i + 1) * bpp];
        let px = match bpp {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[2], p[1], p[0], 255],
            _ => [p[2], p[1], p[0], p[3]],
        };
        let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
        let x = if right_left { w - 1 - x } else { x };
        let y = if top_down { y } else { ht - 1 - y };
        let o = (y as usize * w as usize + x as usize) * 4;
        out.pixels[o..o + 4].copy_from_slice(&px);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_and_rle_bottom_up() {
        // 2x1, 32-bit, bottom-left origin (a single row: same either way)
        let mut t = vec![0u8; 18];
        t[2] = 2;
        t[12] = 2;
        t[14] = 1;
        t[16] = 32;
        t.extend([1, 2, 3, 4, 5, 6, 7, 8]);
        let img = decode(&t).unwrap();
        assert_eq!(img.pixel(0, 0), [3, 2, 1, 4]);
        assert_eq!(img.pixel(1, 0), [7, 6, 5, 8]);

        // 1x2 RLE 24-bit bottom-up: one run of two blue pixels... then flipped rows
        let mut r = vec![0u8; 18];
        r[2] = 10;
        r[12] = 1;
        r[14] = 2;
        r[16] = 24;
        r.extend([0x00, 10, 20, 30, 0x00, 40, 50, 60]);
        let img = decode(&r).unwrap();
        assert_eq!(
            img.pixel(0, 1),
            [30, 20, 10, 255],
            "first stored row is the bottom one"
        );
        assert_eq!(img.pixel(0, 0), [60, 50, 40, 255]);
        for cut in 0..r.len() {
            let _ = decode(&r[..cut]);
        }
    }
}
