//! Java animated textures (`.png` + `.png.mcmeta`) → Bedrock flipbooks
//! (`textures/flipbook_textures.json`).
//!
//! What is translated, exactly:
//! - Java frames are `width`×`height` from the `.mcmeta`, else squares of the
//!   image's smaller side, read left to right, top to bottom. A Bedrock
//!   flipbook is one column of squares as wide as the image. Anything else is
//!   left for a person: the frames would have to be cut and stacked.
//! - `frametime` (ticks, default 1) and per-frame `time` become Bedrock's one
//!   `ticks_per_frame`: the greatest common divisor of all the times, each
//!   frame repeated time/gcd times in `frames`. The timing is the same tick
//!   for tick.
//! - `interpolate` becomes `blend_frames`, always written: Bedrock blends by
//!   default, Java does not.
//! - The flipbook needs the terrain tile that shows the texture: the game's
//!   own flipbook for it, else the one tile of `terrain_texture.json` that
//!   shows it. Bedrock animates only the terrain atlas, so only block
//!   textures qualify.

use kebitx_core::json::{Json, JsonMap};
use kebitx_core::vanilla::{TileRef, VanillaCatalog};
use std::path::Path;

/// More repeated frames than this is no animation a person wrote.
const MAX_FRAMES: usize = 4096;

/// The flipbook entry for `png` (in the Java pack) placed at `target_stem`
/// (`textures/blocks/lava_still`), or why there is none.
pub fn flipbook(
    java_root: &Path,
    png: &str,
    target_stem: &str,
    catalog: &VanillaCatalog,
) -> Result<Json, &'static str> {
    if !target_stem.starts_with("textures/blocks/") {
        return Err(
            "animated texture outside the blocks: Bedrock flipbooks animate the terrain atlas only",
        );
    }
    let tile: TileRef = match catalog.flipbook_tile(target_stem) {
        Some(t) => t.clone(),
        None => match catalog.terrain_tiles(target_stem) {
            [one] => one.clone(),
            [] => return Err(
                "animated texture no terrain tile of the game shows: no atlas_tile for a flipbook",
            ),
            _ => return Err(
                "animated texture shown by several terrain tiles: which one to animate is a choice",
            ),
        },
    };
    let meta = kebitx_core::filesystem::read_text(&java_root.join(format!("{png}.mcmeta")))
        .ok()
        .and_then(|s| kebitx_core::json::parse(&s).ok())
        .ok_or("animation (.mcmeta) that cannot be read")?;
    let anim = meta
        .get("animation")
        .ok_or("a .mcmeta without an animation")?;
    let (w, h) = kebitx_core::image::dimensions(&java_root.join(png))
        .ok_or("animated texture whose size cannot be read")?;
    let num = |k: &str| match anim.get(k) {
        None => Ok(None),
        Some(Json::Num(n)) if *n >= 1.0 && n.fract() == 0.0 && *n <= u32::MAX as f64 => {
            Ok(Some(*n as u32))
        }
        Some(_) => Err("animation with a size or time that is not a whole number"),
    };
    let (fw, fh) = match (num("width")?, num("height")?) {
        (Some(fw), Some(fh)) => (fw, fh),
        (Some(fw), None) => (fw, h),
        (None, Some(fh)) => (w, fh),
        (None, None) => (w.min(h), w.min(h)),
    };
    if fw != fh || fw != w || !h.is_multiple_of(fh) {
        return Err("frames that are not one column of squares: Bedrock flipbooks need one");
    }
    let count = h / fh;
    let frametime = num("frametime")?.unwrap_or(1);

    // (frame index, ticks) in play order
    let mut seq: Vec<(u32, u32)> = Vec::new();
    match anim.get("frames") {
        None => seq.extend((0..count).map(|i| (i, frametime))),
        Some(Json::Arr(frames)) => {
            for fr in frames {
                let (index, time) = match fr {
                    Json::Num(_) => (fr, None),
                    Json::Obj(_) => (
                        fr.get("index").ok_or("a frame without an index")?,
                        fr.get("time"),
                    ),
                    _ => return Err("a frame that is not an index or an object"),
                };
                let whole = |v: &Json, min: f64| match v {
                    Json::Num(n) if *n >= min && n.fract() == 0.0 && *n <= u32::MAX as f64 => {
                        Ok(*n as u32)
                    }
                    _ => Err("a frame index or time that is not a whole number"),
                };
                let i = whole(index, 0.0)?;
                if i >= count {
                    return Err("a frame index beyond the image");
                }
                let t = time
                    .map(|t| whole(t, 1.0))
                    .transpose()?
                    .unwrap_or(frametime);
                seq.push((i, t));
            }
        }
        Some(_) => return Err("frames that are not a list"),
    }
    if seq.is_empty() {
        return Err("an animation without frames");
    }
    let tick = seq.iter().fold(0, |g, &(_, t)| gcd(g, t));
    let frames: Vec<u32> = seq
        .iter()
        .flat_map(|&(i, t)| std::iter::repeat_n(i, (t / tick) as usize))
        .collect();
    if frames.len() > MAX_FRAMES {
        return Err("an animation too long to spell out frame by frame");
    }
    let interpolate = matches!(anim.get("interpolate"), Some(Json::Bool(true)));

    let mut e = vec![
        (
            "flipbook_texture".to_string(),
            Json::Str(target_stem.to_string()),
        ),
        ("atlas_tile".to_string(), Json::Str(tile.tile)),
        ("ticks_per_frame".to_string(), Json::Num(tick as f64)),
        ("blend_frames".to_string(), Json::Bool(interpolate)),
    ];
    if let Some(i) = tile.index {
        e.push(("atlas_index".into(), Json::Num(i as f64)));
    }
    // the plain order 0..count is Bedrock's default: written only when not
    if frames != (0..count).collect::<Vec<_>>() {
        e.push((
            "frames".into(),
            Json::Arr(frames.into_iter().map(|i| Json::Num(i as f64)).collect()),
        ));
    }
    Ok(Json::Obj(JsonMap::from_entries(e)))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// The `flipbook_textures.json` text for these entries.
pub fn flipbooks_file(entries: &[Json]) -> String {
    kebitx_core::json::to_string(&Json::Arr(entries.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::gcd;

    #[test]
    fn gcd_of_frame_times() {
        assert_eq!([2, 4, 6].iter().fold(0, |g, &t| gcd(g, t)), 2);
        assert_eq!([3].iter().fold(0, |g, &t| gcd(g, t)), 3);
        assert_eq!([5, 7].iter().fold(0, |g, &t| gcd(g, t)), 1);
    }
}
