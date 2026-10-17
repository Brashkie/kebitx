//! Texture identity proven by PIXELS.
//!
//! A name is a hint; the picture is the proof. When the Java game's own
//! texture and one of the Bedrock game's are the same picture, they are the
//! same texture, whatever their paths. Three shapes of that proof:
//!
//! - **whole**: the Java picture is a Bedrock picture at another path (a GUI
//!   icon, a texture Bedrock files elsewhere);
//! - **crop**: the Bedrock picture is an aligned region of the Java one —
//!   Java's 64×64 `zombie.png` whose top half is Bedrock's 64×32 one: the
//!   pack's texture is cut the same way;
//! - **sheet**: the Java picture is an aligned cell of a Bedrock sheet —
//!   Java's particle sprites inside Bedrock's `particles.png`: the pack's
//!   sprite is pasted into a copy of that sheet.
//!
//! Every proof must be unique on both sides: a picture the Java game has
//! twice, or Bedrock has in two places, proves nothing (blank and one-colour
//! pictures fall out this way). Pixels are compared as SEEN: colour under
//! full transparency is ignored. Pictures are compared by a 64-bit hash.

use crate::java::JavaJar;
use kebitx_core::codec::png::{self, Rgba};
use kebitx_core::codec::tga;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Bedrock folders that pack many pictures into one sheet, by Java kind.
pub const SHEET_FOLDERS: [(&str, &[&str]); 2] = [
    ("particle", &["textures/particle/"]),
    ("gui", &["textures/ui/", "textures/gui/"]),
];

/// A picture's identity: its size and visible pixels.
pub fn hash(img: &Rgba) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    img.width
        .to_le_bytes()
        .into_iter()
        .chain(img.height.to_le_bytes())
        .for_each(&mut eat);
    img.pixels.iter().for_each(|&b| eat(b));
    h
}

/// A Bedrock material map (Vibrant Visuals: `_mers`, `_normal`,
/// `_heightmap`): not a colour picture, never a match for one.
pub fn is_material_map(stem: &str) -> bool {
    ["_mers", "_mer", "_normal", "_heightmap"]
        .iter()
        .any(|s| stem.ends_with(s))
        || stem.contains(".texture_set")
}

/// The SHAPE of what a picture draws — its size and which pixels are not
/// transparent — when that shape says something: at least 5 % of the canvas
/// drawn and 5 % left empty, and 32 pixels drawn. Two pictures with the same
/// shape lay their parts out the same way, whatever their colours.
pub fn mask_hash(img: &Rgba) -> Option<u64> {
    let total = img.pixels.len() / 4;
    let drawn = img.pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
    if drawn < 32 || drawn * 20 < total || (total - drawn) * 20 < total {
        return None;
    }
    let mut h: u64 = 0x6a09_e667_f3bc_c909;
    let mut eat = |b: u8| h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    img.width
        .to_le_bytes()
        .into_iter()
        .chain(img.height.to_le_bytes())
        .for_each(&mut eat);
    for chunk in img.pixels.chunks(32) {
        let bits = chunk
            .chunks_exact(4)
            .enumerate()
            .fold(0u8, |b, (i, p)| b | (((p[3] > 0) as u8) << i));
        eat(bits);
    }
    Some(h)
}

/// Nothing visible at all.
pub fn is_blank(img: &Rgba) -> bool {
    img.pixels.chunks_exact(4).all(|p| p[3] == 0)
}

/// Decode by content: PNG by its signature, TGA when the name says so.
pub fn decode(name: &str, bytes: &[u8]) -> Option<Rgba> {
    let img = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        png::decode(bytes).ok()?
    } else if name.to_ascii_lowercase().ends_with(".tga") {
        tga::decode(bytes).ok()?
    } else {
        return None;
    };
    Some(img.visible())
}

pub fn decode_file(path: &Path) -> Option<Rgba> {
    let bytes =
        kebitx_core::filesystem::read_limited(path, kebitx_core::filesystem::MAX_ASSET_BYTES)
            .ok()?;
    decode(&path.to_string_lossy(), &bytes)
}

/// The Bedrock game's pictures.
#[derive(Default)]
pub struct BedrockPixels {
    /// stem -> (width, height, hash)
    pub stems: HashMap<String, (u32, u32, u64)>,
    /// hash -> stems with that picture
    pub by_hash: HashMap<u64, Vec<String>>,
    /// shape -> stems with that shape (see `mask_hash`)
    pub by_mask: HashMap<u64, Vec<String>>,
    /// The pictures of sheet folders, kept for cutting cells and pasting.
    pub sheets: HashMap<String, Rgba>,
    /// (kind, cell width, cell height) -> cell hash -> where: built on first
    /// use, once per cell size.
    cells: Mutex<HashMap<(String, u32, u32), Arc<CellIndex>>>,
}

type CellIndex = HashMap<u64, Vec<(String, u32, u32)>>;

impl BedrockPixels {
    /// Every texture of the installed game, decoded once.
    pub fn from_install(install: &Path) -> Self {
        let wanted =
            |p: &str| p.starts_with("textures/") && (p.ends_with(".png") || p.ends_with(".tga"));
        let decoded = kebitx_core::vanilla::files::map_files(install, wanted, |p, bytes| {
            let stem = kebitx_core::filesystem::asset_stem(p);
            if is_material_map(&stem) {
                return None;
            }
            let img = decode(p, bytes)?;
            let keep = SHEET_FOLDERS
                .iter()
                .flat_map(|(_, f)| f.iter())
                .any(|f| stem.starts_with(f));
            Some((
                stem,
                (img.width, img.height, hash(&img)),
                mask_hash(&img),
                keep.then_some(img),
            ))
        });
        let mut px = BedrockPixels::default();
        // a .png and a .tga of one stem: the game uses the .png
        let mut entries: Vec<_> = decoded.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut masks: HashMap<String, u64> = HashMap::new();
        for (path, (stem, info, mask, img)) in entries {
            if px.stems.contains_key(&stem) && path.ends_with(".tga") {
                continue;
            }
            px.stems.insert(stem.clone(), info);
            match mask {
                Some(m) => masks.insert(stem.clone(), m),
                None => masks.remove(&stem),
            };
            if let Some(img) = img {
                px.sheets.insert(stem.clone(), img);
            }
        }
        for (stem, &(_, _, h)) in &px.stems {
            px.by_hash.entry(h).or_default().push(stem.clone());
        }
        for (stem, m) in masks {
            px.by_mask.entry(m).or_default().push(stem);
        }
        px
    }

    /// Every aligned `w`×`h` cell of the sheets of `kind`, by picture.
    fn cells(&self, kind: &str, w: u32, h: u32) -> Option<Arc<CellIndex>> {
        let folders = SHEET_FOLDERS.iter().find(|(k, _)| *k == kind)?.1;
        let key = (kind.to_string(), w, h);
        let mut cache = self.cells.lock().ok()?;
        if let Some(index) = cache.get(&key) {
            return Some(index.clone());
        }
        let mut index = CellIndex::new();
        for (stem, sheet) in &self.sheets {
            let fits = sheet.width.is_multiple_of(w)
                && sheet.height.is_multiple_of(h)
                && (sheet.width, sheet.height) != (w, h);
            if !fits || !folders.iter().any(|f| stem.starts_with(f)) {
                continue;
            }
            for y in (0..sheet.height).step_by(h as usize) {
                for x in (0..sheet.width).step_by(w as usize) {
                    if let Some(cell) = sheet.crop(x, y, w, h).filter(|c| !is_blank(c)) {
                        index
                            .entry(hash(&cell))
                            .or_default()
                            .push((stem.clone(), x, y));
                    }
                }
            }
        }
        let index = Arc::new(index);
        cache.insert(key, index.clone());
        Some(index)
    }
}

/// The Java game's pictures, from its `.jar`.
pub struct JavaPixels {
    /// hash -> how many Java textures are that picture
    count: HashMap<u64, usize>,
    /// shape -> how many Java textures have that shape
    mask_count: HashMap<u64, usize>,
}

impl JavaPixels {
    pub fn from_jar(jar: &JavaJar) -> Self {
        let names: Vec<String> = jar
            .zip
            .names()
            .filter(|n| n.starts_with("assets/minecraft/textures/") && n.ends_with(".png"))
            .cloned()
            .collect();
        let hashes = kebitx_core::par::par_map(&names, |n| {
            jar.read(n)
                .and_then(|b| decode(n, &b))
                .map(|i| (hash(&i), mask_hash(&i)))
        });
        let (mut count, mut mask_count) = (HashMap::new(), HashMap::new());
        for (h, m) in hashes.into_iter().flatten() {
            *count.entry(h).or_insert(0) += 1;
            if let Some(m) = m {
                *mask_count.entry(m).or_insert(0) += 1;
            }
        }
        JavaPixels { count, mask_count }
    }

    fn unique(&self, h: u64) -> bool {
        self.count.get(&h) == Some(&1)
    }
}

/// Drawn pixels a shape needs before it proves a layout.
pub const LAYOUT_MIN_DRAWN: usize = 256;

/// The one Bedrock texture laid out like the Java game's picture: the same
/// shape, unique in both games. The colours may differ — Bedrock redrew it —
/// so the pack's texture, drawn on the Java layout, fits Bedrock's.
pub fn prove_layout(java: &Rgba, jp: &JavaPixels, bp: &BedrockPixels) -> Option<String> {
    // a small icon's outline can coincide (an attack indicator and the
    // strength effect are both a sword): a layout needs room to be a proof
    if java.pixels.chunks_exact(4).filter(|p| p[3] > 0).count() < LAYOUT_MIN_DRAWN {
        return None;
    }
    let m = mask_hash(java)?;
    if jp.mask_count.get(&m) != Some(&1) {
        return None;
    }
    match bp.by_mask.get(&m).map(Vec::as_slice) {
        Some([only]) => Some(only.clone()),
        _ => None,
    }
}

/// Where a Java texture's picture is in Bedrock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    /// The same picture, whole, at this stem.
    Whole(String),
    /// Bedrock's picture at `stem` is the Java one's region at (x, y).
    Crop {
        stem: String,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    /// The Java picture is the cell at (x, y) of Bedrock's sheet at `stem`.
    Cell { stem: String, x: u32, y: u32 },
}

/// The proof for the Java game's picture `java` (of kind `kind`: `particle`,
/// `entity`, ...), if there is exactly one. `name_candidates` are Bedrock
/// stems of the same file name, the only ones a crop is looked for in.
pub fn prove(
    java: &Rgba,
    kind: &str,
    name_candidates: &[String],
    jp: &JavaPixels,
    bp: &BedrockPixels,
) -> Option<Proof> {
    if is_blank(java) {
        return None;
    }
    let h = hash(java);
    let unique_in_java = jp.unique(h);
    if unique_in_java {
        if let Some([only]) = bp.by_hash.get(&h).map(Vec::as_slice) {
            return Some(Proof::Whole(only.clone()));
        }
    }
    // crop: a Bedrock picture of the same name inside the Java one
    let mut crops = Vec::new();
    for stem in name_candidates {
        let Some(&(bw, bh, bh_hash)) = bp.stems.get(stem) else {
            continue;
        };
        if (bw, bh) == (java.width, java.height)
            || !java.width.is_multiple_of(bw)
            || !java.height.is_multiple_of(bh)
        {
            continue;
        }
        for y in (0..java.height).step_by(bh as usize) {
            for x in (0..java.width).step_by(bw as usize) {
                if let Some(region) = java.crop(x, y, bw, bh) {
                    if !is_blank(&region) && hash(&region) == bh_hash {
                        crops.push(Proof::Crop {
                            stem: stem.clone(),
                            x,
                            y,
                            w: bw,
                            h: bh,
                        });
                    }
                }
            }
        }
    }
    if crops.len() == 1 {
        return crops.pop();
    }
    if !crops.is_empty() || !unique_in_java {
        return None;
    }
    // sheet: the Java picture as a cell of a Bedrock sheet of its kind
    let cells = bp.cells(kind, java.width, java.height)?;
    match cells.get(&h).map(Vec::as_slice) {
        Some([(stem, x, y)]) => Some(Proof::Cell {
            stem: stem.clone(),
            x: *x,
            y: *y,
        }),
        _ => None,
    }
}

/// `pack` is `java` scaled by a whole factor (1 for the same size).
pub fn scale_of(pack: (u32, u32), java: (u32, u32)) -> Option<u32> {
    let k = pack.0 / java.0;
    (k >= 1 && pack.0 == java.0 * k && pack.1 == java.1 * k).then_some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, seed: u8) -> Rgba {
        let mut i = Rgba::new(w, h);
        for (n, p) in i.pixels.iter_mut().enumerate() {
            *p = if n % 4 == 3 {
                255
            } else {
                (n as u8).wrapping_mul(31).wrapping_add(seed)
            };
        }
        i
    }

    fn bedrock(entries: &[(&str, Rgba, bool)]) -> BedrockPixels {
        let mut bp = BedrockPixels::default();
        for (stem, i, sheet) in entries {
            bp.stems
                .insert(stem.to_string(), (i.width, i.height, hash(i)));
            bp.by_hash
                .entry(hash(i))
                .or_default()
                .push(stem.to_string());
            if let Some(m) = mask_hash(i) {
                bp.by_mask.entry(m).or_default().push(stem.to_string());
            }
            if *sheet {
                bp.sheets.insert(stem.to_string(), i.clone());
            }
        }
        bp
    }

    fn java(pics: &[&Rgba]) -> JavaPixels {
        let mut count = HashMap::new();
        for p in pics {
            *count.entry(hash(p)).or_insert(0) += 1;
        }
        let mut mask_count = HashMap::new();
        for p in pics {
            if let Some(m) = mask_hash(p) {
                *mask_count.entry(m).or_insert(0) += 1;
            }
        }
        JavaPixels { count, mask_count }
    }

    #[test]
    fn whole_crop_and_cell_proofs() {
        let icon = img(16, 16, 1);
        let bp = bedrock(&[("textures/ui/heart", icon.clone(), true)]);
        assert_eq!(
            prove(&icon, "gui", &[], &java(&[&icon]), &bp),
            Some(Proof::Whole("textures/ui/heart".into()))
        );
        // the same picture twice in Java: no proof
        assert_eq!(prove(&icon, "gui", &[], &java(&[&icon, &icon]), &bp), None);

        // crop: Bedrock's 8x4 is the Java 8x8's lower half
        let tall = img(8, 8, 2);
        let half = tall.crop(0, 4, 8, 4).unwrap();
        let bp = bedrock(&[("textures/entity/z", half, false)]);
        assert_eq!(
            prove(
                &tall,
                "entity",
                &["textures/entity/z".into()],
                &java(&[&tall]),
                &bp
            ),
            Some(Proof::Crop {
                stem: "textures/entity/z".into(),
                x: 0,
                y: 4,
                w: 8,
                h: 4
            })
        );

        // cell: a 4x4 sprite at (4, 0) of an 8x8 particle sheet
        let mut sheet = img(8, 8, 3);
        let sprite = img(4, 4, 9);
        sheet.paste(&sprite, 4, 0);
        let bp = bedrock(&[("textures/particle/particles", sheet, true)]);
        assert_eq!(
            prove(&sprite, "particle", &[], &java(&[&sprite]), &bp),
            Some(Proof::Cell {
                stem: "textures/particle/particles".into(),
                x: 4,
                y: 0
            })
        );
        // not a sheet kind: no cell search
        assert_eq!(prove(&sprite, "item", &[], &java(&[&sprite]), &bp), None);
        // blank pictures prove nothing
        assert_eq!(
            prove(&Rgba::new(4, 4), "particle", &[], &java(&[]), &bp),
            None
        );
    }

    #[test]
    fn a_layout_is_proven_by_the_shape_of_what_is_drawn() {
        // a 32x32 picture drawn on its left half: 512 pixels, enough shape
        let shape = |seed: u8| {
            let mut i = img(32, 32, seed);
            for (n, p) in i.pixels.chunks_exact_mut(4).enumerate() {
                if n % 32 >= 16 {
                    p.copy_from_slice(&[0, 0, 0, 0]);
                }
            }
            i
        };
        let java_pic = shape(1);
        let bedrock_pic = shape(77); // other colours, the same shape
        let bp = bedrock(&[(
            "textures/entity/banner/banner_cross",
            bedrock_pic.clone(),
            false,
        )]);
        assert_eq!(
            prove_layout(&java_pic, &java(&[&java_pic]), &bp).as_deref(),
            Some("textures/entity/banner/banner_cross")
        );
        // two Bedrock pictures of that shape: no proof
        let two = bedrock(&[
            ("textures/a", bedrock_pic.clone(), false),
            ("textures/b", bedrock_pic, false),
        ]);
        assert_eq!(prove_layout(&java_pic, &java(&[&java_pic]), &two), None);
        // a small icon: too little shape to prove anything
        let mut icon = img(8, 8, 5);
        icon.pixels[3] = 0;
        let bp = bedrock(&[("textures/ui/icon", icon.clone(), false)]);
        assert_eq!(prove_layout(&icon, &java(&[&icon]), &bp), None);
        assert!(is_material_map("textures/entity/enderman/enderman_mers"));
    }

    #[test]
    fn scale_is_whole_and_uniform() {
        assert_eq!(scale_of((32, 32), (16, 16)), Some(2));
        assert_eq!(scale_of((16, 16), (16, 16)), Some(1));
        assert_eq!(scale_of((32, 16), (16, 16)), None);
        assert_eq!(scale_of((24, 24), (16, 16)), None);
    }
}
