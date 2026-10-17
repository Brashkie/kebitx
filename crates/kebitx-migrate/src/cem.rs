//! OptiFine custom entity models (`optifine/cem/*.jem`, `*.jpm`) against
//! Bedrock's entity geometry.
//!
//! Java's entity models live in the game's code, so a `.jem` part (`head`,
//! `leg1`) has no file to be compared with. Its boxes do say which pixels
//! they paint: a texture offset and a size, laid out as box UV — the layout
//! Bedrock's cubes use too. A part whose boxes paint exactly the regions one
//! bone's cubes paint, in a geometry with the same texture size, IS that
//! bone. That mapping is what this proves, part by part.
//!
//! What is not converted, and why: positions (OptiFine's axes, `translate`
//! and `invertAxis` have no pair in the games to check a transform against)
//! and animations (OptiFine expressions, not Molang). Both are reported.

use kebitx_core::json::Json;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// (u, v, width, height, depth) of a box-UV cube.
pub type BoxUv = (i64, i64, i64, i64, i64);

#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub texture: (i64, i64),
    /// bone -> its cubes' box UVs, sorted
    pub bones: BTreeMap<String, Vec<BoxUv>>,
}

/// Bedrock's entity geometries and which entity uses which.
#[derive(Default)]
pub struct BedrockGeometry {
    /// geometry id -> geometry
    pub geometries: HashMap<String, Geometry>,
    /// entity identifier (`minecraft:cow`) -> its geometry ids
    pub entities: HashMap<String, Vec<String>>,
}

fn int(v: Option<&Json>) -> Option<i64> {
    match v? {
        Json::Num(n) if n.fract() == 0.0 => Some(*n as i64),
        _ => None,
    }
}

fn geometry_of(v: &Json) -> Geometry {
    let desc = v.get("description");
    let texture = (
        int(desc
            .and_then(|d| d.get("texture_width"))
            .or_else(|| v.get("texturewidth")))
        .unwrap_or(64),
        int(desc
            .and_then(|d| d.get("texture_height"))
            .or_else(|| v.get("textureheight")))
        .unwrap_or(64),
    );
    let mut bones = BTreeMap::new();
    for b in v
        .get("bones")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = b.get("name").and_then(Json::as_str) else {
            continue;
        };
        let mut cubes = Vec::new();
        for c in b
            .get("cubes")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let uv = c.get("uv").and_then(Json::as_array);
            let size = c.get("size").and_then(Json::as_array);
            if let (Some(uv), Some(size)) = (uv, size) {
                if let (Some(u), Some(v), Some(w), Some(h), Some(d)) = (
                    int(uv.first()),
                    int(uv.get(1)),
                    int(size.first()),
                    int(size.get(1)),
                    int(size.get(2)),
                ) {
                    cubes.push((u, v, w, h, d));
                }
            }
        }
        cubes.sort();
        bones.insert(name.to_string(), cubes);
    }
    Geometry { texture, bones }
}

impl BedrockGeometry {
    pub fn from_install(install: &Path) -> Self {
        let parse = |b: &[u8]| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok();
        let files = kebitx_core::vanilla::files::map_files(
            install,
            |p| {
                (p.starts_with("models/entity/") || p.starts_with("entity/"))
                    && p.ends_with(".json")
            },
            |_, b| parse(b),
        );
        let mut g = BedrockGeometry::default();
        for (path, json) in &files {
            if path.starts_with("entity/") {
                let Some(desc) = json
                    .get("minecraft:client_entity")
                    .and_then(|c| c.get("description"))
                else {
                    continue;
                };
                let (Some(id), Some(geos)) = (
                    desc.get("identifier").and_then(Json::as_str),
                    desc.get("geometry").and_then(Json::as_object),
                ) else {
                    continue;
                };
                let list = g.entities.entry(id.to_string()).or_default();
                for v in geos.values().filter_map(Json::as_str) {
                    if !list.contains(&v.to_string()) {
                        list.push(v.to_string());
                    }
                }
                continue;
            }
            if let Some(arr) = json.get("minecraft:geometry").and_then(Json::as_array) {
                for geo in arr {
                    if let Some(id) = geo
                        .get("description")
                        .and_then(|d| d.get("identifier"))
                        .and_then(Json::as_str)
                    {
                        g.geometries.insert(id.to_string(), geometry_of(geo));
                    }
                }
            } else if let Some(obj) = json.as_object() {
                for (k, geo) in obj.iter().filter(|(k, _)| k.starts_with("geometry.")) {
                    g.geometries.insert(
                        k.split(':').next().unwrap_or(k).to_string(),
                        geometry_of(geo),
                    );
                }
            }
        }
        g
    }
}

/// A `.jem` file and what it proves.
#[derive(Debug, Clone)]
pub struct CemReport {
    /// The entity (`cow`, from `cow.jem`).
    pub entity: String,
    /// The Bedrock geometry the parts were matched in.
    pub geometry: Option<String>,
    /// part -> the Bedrock bone it is proven to be
    pub parts: BTreeMap<String, Option<String>>,
    /// animation expressions in the file (not converted)
    pub animations: usize,
}

/// The boxes of a part (its own, else its submodels'), sorted, following a
/// `model` reference to a `.jpm` next to the `.jem`.
fn part_boxes(part: &Json, dir: &Path) -> Vec<BoxUv> {
    let loaded;
    let part = match part.get("model").and_then(Json::as_str) {
        Some(file) if !file.contains("..") => {
            match kebitx_core::filesystem::read_text(&dir.join(file))
                .ok()
                .and_then(|s| kebitx_core::json::parse(&s).ok())
            {
                Some(j) => {
                    loaded = j;
                    &loaded
                }
                None => part,
            }
        }
        _ => part,
    };
    let mut out = Vec::new();
    let add = |b: &Json, out: &mut Vec<BoxUv>| {
        let off = b.get("textureOffset").and_then(Json::as_array);
        let c = b.get("coordinates").and_then(Json::as_array);
        if let (Some(off), Some(c)) = (off, c) {
            if let (Some(u), Some(v), Some(w), Some(h), Some(d)) = (
                int(off.first()),
                int(off.get(1)),
                int(c.get(3)),
                int(c.get(4)),
                int(c.get(5)),
            ) {
                out.push((u, v, w, h, d));
            }
        }
    };
    for b in part
        .get("boxes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        add(b, &mut out);
    }
    if out.is_empty() {
        for s in part
            .get("submodels")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            for b in s
                .get("boxes")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                add(b, &mut out);
            }
        }
    }
    out.sort();
    out
}

/// Match a `.jem`'s parts to the bones of the entity's Bedrock geometry.
pub fn inspect(jem_path: &Path, bg: &BedrockGeometry) -> Option<CemReport> {
    let jem = kebitx_core::json::parse(&kebitx_core::filesystem::read_text(jem_path).ok()?).ok()?;
    let entity = jem_path.file_stem()?.to_string_lossy().to_string();
    let dir = jem_path.parent()?;
    let size = jem.get("textureSize").and_then(Json::as_array);
    let texture = (
        int(size.and_then(|s| s.first())).unwrap_or(64),
        int(size.and_then(|s| s.get(1))).unwrap_or(32),
    );
    let models = jem
        .get("models")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let parts: Vec<(String, Vec<BoxUv>)> = models
        .iter()
        .filter_map(|m| {
            Some((
                m.get("part").and_then(Json::as_str)?.to_string(),
                part_boxes(m, dir),
            ))
        })
        .collect();
    let animations = models
        .iter()
        .flat_map(|m| {
            m.get("animations")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|a| a.as_object().map(|o| o.len()).unwrap_or(0))
        .sum();

    // the geometry of that entity, with the same texture size, in which the
    // most parts are proven — if one is best
    let mut best: Vec<(usize, String, BTreeMap<String, Option<String>>)> = Vec::new();
    for id in bg
        .entities
        .get(&format!("minecraft:{entity}"))
        .into_iter()
        .flatten()
    {
        let Some(geo) = bg.geometries.get(id).filter(|g| g.texture == texture) else {
            continue;
        };
        let mut mapping = BTreeMap::new();
        for (part, boxes) in &parts {
            let bones: Vec<&String> = geo
                .bones
                .iter()
                .filter(|(_, c)| !boxes.is_empty() && *c == boxes)
                .map(|(b, _)| b)
                .collect();
            mapping.insert(
                part.clone(),
                match bones.as_slice() {
                    [one] => Some((*one).clone()),
                    _ => None,
                },
            );
        }
        let proven = mapping.values().filter(|b| b.is_some()).count();
        best.push((proven, id.clone(), mapping));
    }
    best.sort_by_key(|b| std::cmp::Reverse(b.0));
    let unique_best = match best.as_slice() {
        [first, second, ..] if first.0 == second.0 => None,
        [first, ..] if first.0 > 0 => Some(first.clone()),
        _ => None,
    };
    let (geometry, parts) = match unique_best {
        Some((_, id, mapping)) => (Some(id), mapping),
        None => (None, parts.iter().map(|(p, _)| (p.clone(), None)).collect()),
    };
    Some(CemReport {
        entity,
        geometry,
        parts,
        animations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_matched_by_the_pixels_their_boxes_paint() {
        let mut bg = BedrockGeometry::default();
        let geo = kebitx_core::json::parse(
            r#"{"description":{"identifier":"geometry.cow.v2","texture_width":64,"texture_height":32},"bones":[
                {"name":"head","cubes":[{"origin":[0,0,0],"size":[8,8,6],"uv":[0,0]}]},
                {"name":"leg0","cubes":[{"origin":[0,0,0],"size":[4,12,4],"uv":[0,16]}]},
                {"name":"leg1","cubes":[{"origin":[0,0,0],"size":[4,12,4],"uv":[0,16]}]}]}"#,
        )
        .unwrap();
        bg.geometries
            .insert("geometry.cow.v2".into(), geometry_of(&geo));
        bg.entities
            .insert("minecraft:cow".into(), vec!["geometry.cow.v2".into()]);
        let dir = std::env::temp_dir().join(format!("kebitx-cem-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jem = dir.join("cow.jem");
        std::fs::write(
            &jem,
            r#"{"textureSize":[64,32],"models":[
                {"part":"head","boxes":[{"coordinates":[-4,16,-14,8,8,6],"textureOffset":[0,0]}],"animations":[{"head.rx":"sin(time)"}]},
                {"part":"leg1","boxes":[{"coordinates":[-6,0,5,4,12,4],"textureOffset":[0,16]}]},
                {"part":"body","boxes":[{"coordinates":[0,0,0,12,18,10],"textureOffset":[18,4]}]}]}"#,
        )
        .unwrap();
        let r = inspect(&jem, &bg).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(r.geometry.as_deref(), Some("geometry.cow.v2"));
        assert_eq!(r.parts["head"].as_deref(), Some("head"));
        assert_eq!(
            r.parts["leg1"], None,
            "two legs paint the same pixels: no proof which"
        );
        assert_eq!(
            r.parts["body"], None,
            "a box Bedrock's geometry does not have"
        );
        assert_eq!(r.animations, 1);
    }
}
