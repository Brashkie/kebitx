//! Java block and item models (`elements`: cubes with per-face UV) →
//! Bedrock geometry.
//!
//! The rules are not guessed: the Bedrock game ships its own conversion of
//! Java's `straw_bed_foot` and `straw_bed_head`, and they were read off it
//! (the test `mojangs_own_conversion_is_reproduced` checks them against the
//! installed games):
//!
//! - positions: `origin = [8 - to.x, from.y, from.z - 8]` (Bedrock's x runs
//!   the other way and its origin is the block's bottom centre),
//!   `size = to - from`;
//! - side faces: `uv = [u1, v1]`, `uv_size = [u2 - u1, v2 - v1]`;
//!   `up` / `down`: `uv = [u2, v2]`, `uv_size = [u1 - u2, v1 - v2]`;
//! - a face's `rotation` is its `uv_rotation`; its texture `#all` is the
//!   `material_instance` `all`; faces a Java element lacks are left out, as
//!   Bedrock then draws nothing, like Java;
//! - the texture space is 16×16, as Java's UVs are.
//!
//! What Mojang's pair does NOT show, and is therefore reported as unverified:
//! ELEMENT rotations (the sign under the mirrored x axis) and `rescale`,
//! which Bedrock has no field for.

use crate::java::JavaJar;
use kebitx_core::json::{Json, JsonMap};
use std::collections::BTreeMap;
use std::path::Path;

/// A model with its parents applied.
#[derive(Debug, Default, Clone)]
pub struct Resolved {
    /// texture variable -> value (`#other` or a texture id), parents first
    pub textures: BTreeMap<String, String>,
    pub elements: Option<Vec<Json>>,
    pub display: BTreeMap<String, Json>,
    /// The chain ended in `builtin/generated`: a flat item sprite.
    pub generated: bool,
}

/// `minecraft:block/stone` or `block/stone` -> `assets/minecraft/models/block/stone.json`.
pub fn model_path(id: &str) -> Option<String> {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    if path.is_empty() || path.contains("..") {
        return None;
    }
    Some(format!("assets/{ns}/models/{path}.json"))
}

/// Read a model from the pack, else from the Java game.
fn read_model(path: &str, pack: &Path, jar: Option<&JavaJar>) -> Option<Json> {
    let text = match std::fs::read(pack.join(path)) {
        Ok(b) => b,
        Err(_) => jar?.read(path)?,
    };
    kebitx_core::json::parse(&String::from_utf8_lossy(&text)).ok()
}

/// The model at `path` with its parent chain applied (at most 32 deep: a
/// loop is an error, not a hang).
pub fn resolve(path: &str, pack: &Path, jar: Option<&JavaJar>) -> Result<Resolved, String> {
    let mut chain = Vec::new();
    let mut next = Some(path.to_string());
    let mut generated = false;
    while let Some(p) = next.take() {
        if chain.len() >= 32 {
            return Err("model parents loop or nest too deep".into());
        }
        let model = read_model(&p, pack, jar).ok_or_else(|| format!("model not found: {p}"))?;
        if let Some(parent) = model.get("parent").and_then(Json::as_str) {
            let bare = parent.trim_start_matches("minecraft:");
            if bare == "builtin/generated" {
                generated = true;
            } else if !bare.starts_with("builtin/") {
                next = Some(model_path(parent).ok_or("bad parent")?);
            }
        }
        chain.push(model);
    }
    let mut r = Resolved {
        generated,
        ..Default::default()
    };
    for model in chain.iter().rev() {
        if let Some(t) = model.get("textures").and_then(Json::as_object) {
            for (k, v) in t.iter() {
                if let Some(v) = v.as_str() {
                    r.textures.insert(k.clone(), v.to_string());
                }
            }
        }
        if let Some(e) = model.get("elements").and_then(Json::as_array) {
            r.elements = Some(e.clone());
        }
        if let Some(d) = model.get("display").and_then(Json::as_object) {
            for (k, v) in d.iter() {
                r.display.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(r)
}

impl Resolved {
    /// `#side` followed to a texture id, if it ends in one.
    pub fn texture(&self, reference: &str) -> Option<String> {
        let mut cur = reference.to_string();
        for _ in 0..32 {
            match cur.strip_prefix('#') {
                Some(name) => cur = self.textures.get(name)?.clone(),
                None => return Some(cur),
            }
        }
        None
    }
}

/// The geometry and what in it could not be checked against the game.
#[derive(Debug)]
pub struct Geometry {
    pub json: Json,
    /// material instance -> Java texture id it shows
    pub materials: BTreeMap<String, String>,
    /// elements whose rotation or rescale is converted without proof
    pub unverified: usize,
    /// Bedrock culling rules from Java's `cullface`: (bone, face, direction).
    /// Mojang's `straw_bed_foot` rules are exactly its Java cullfaces.
    pub culling: Vec<(String, String, String)>,
}

fn num(v: Option<&Json>) -> Option<f64> {
    match v? {
        Json::Num(n) => Some(*n),
        _ => None,
    }
}

fn vec3(v: Option<&Json>) -> Option<[f64; 3]> {
    let a = v?.as_array()?;
    Some([num(a.first())?, num(a.get(1))?, num(a.get(2))?])
}

fn nums(v: &[f64]) -> Json {
    Json::Arr(v.iter().map(|&x| Json::Num(x)).collect())
}

fn obj(entries: Vec<(&str, Json)>) -> Json {
    Json::Obj(JsonMap::from_entries(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    ))
}

/// Java's automatic UV for a face without one: the element's own extent.
fn default_uv(face: &str, f: [f64; 3], t: [f64; 3]) -> [f64; 4] {
    match face {
        "north" => [16.0 - t[0], 16.0 - t[1], 16.0 - f[0], 16.0 - f[1]],
        "south" => [f[0], 16.0 - t[1], t[0], 16.0 - f[1]],
        "east" => [16.0 - t[2], 16.0 - t[1], 16.0 - f[2], 16.0 - f[1]],
        "west" => [f[2], 16.0 - t[1], t[2], 16.0 - f[1]],
        "up" => [f[0], f[2], t[0], t[2]],
        _ => [f[0], 16.0 - t[2], t[0], 16.0 - f[2]],
    }
}

const FACES: [&str; 6] = ["north", "east", "south", "west", "up", "down"];

/// The Bedrock geometry `identifier` for a resolved model with elements.
pub fn to_geometry(identifier: &str, model: &Resolved) -> Result<Geometry, &'static str> {
    let elements = model.elements.as_ref().ok_or("a model without elements")?;
    let mut bones = Vec::new();
    let mut materials = BTreeMap::new();
    let mut unverified = 0;
    let mut culling = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        let (Some(from), Some(to)) = (vec3(e.get("from")), vec3(e.get("to"))) else {
            return Err("an element without from/to");
        };
        let mut uv = Vec::new();
        let mut culls: Vec<(String, String)> = Vec::new();
        for face in FACES {
            let Some(fd) = e.get("faces").and_then(|f| f.get(face)) else {
                continue;
            };
            let [u1, v1, u2, v2] = match fd.get("uv").and_then(Json::as_array) {
                Some(a) => [num(a.first()), num(a.get(1)), num(a.get(2)), num(a.get(3))]
                    .map(|x| x.unwrap_or(0.0)),
                None => default_uv(face, from, to),
            };
            let (at, size) = if face == "up" || face == "down" {
                ([u2, v2], [u1 - u2, v1 - v2])
            } else {
                ([u1, v1], [u2 - u1, v2 - v1])
            };
            let mut f = vec![("uv", nums(&at)), ("uv_size", nums(&size))];
            if let Some(r) = num(fd.get("rotation")).filter(|r| *r != 0.0) {
                f.push(("uv_rotation", Json::Num(r)));
            }
            if let Some(tex) = fd.get("texture").and_then(Json::as_str) {
                let name = tex.trim_start_matches('#').to_string();
                // a reference that resolves to nothing is kept, empty: the
                // report then says the texture is not proven
                materials.insert(name.clone(), model.texture(tex).unwrap_or_default());
                f.push(("material_instance", Json::Str(name)));
            }
            if let Some(dir) = fd.get("cullface").and_then(Json::as_str) {
                culls.push((face.to_string(), dir.to_string()));
            }
            uv.push((face.to_string(), obj(f)));
        }
        let mut cube = vec![
            ("origin", nums(&[8.0 - to[0], from[1], from[2] - 8.0])),
            (
                "size",
                nums(&[to[0] - from[0], to[1] - from[1], to[2] - from[2]]),
            ),
        ];
        if let Some(rot) = e.get("rotation") {
            let angle = num(rot.get("angle")).unwrap_or(0.0);
            if angle != 0.0 {
                // the mirrored x axis keeps a rotation about x and reverses
                // one about y or z; not shown by Mojang's pair: unverified
                let o = vec3(rot.get("origin")).unwrap_or([8.0, 8.0, 8.0]);
                let r = match rot.get("axis").and_then(Json::as_str) {
                    Some("x") => [angle, 0.0, 0.0],
                    Some("y") => [0.0, -angle, 0.0],
                    Some("z") => [0.0, 0.0, -angle],
                    _ => return Err("an element rotation without an axis"),
                };
                cube.push(("pivot", nums(&[8.0 - o[0], o[1], o[2] - 8.0])));
                cube.push(("rotation", nums(&r)));
                unverified += 1;
            } else if matches!(rot.get("rescale"), Some(Json::Bool(true))) {
                unverified += 1;
            }
        }
        cube.push(("uv", Json::Obj(JsonMap::from_entries(uv))));
        let name = e
            .get("name")
            .and_then(Json::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("element_{i}"));
        for (face, dir) in culls {
            culling.push((name.clone(), face, dir));
        }
        bones.push(obj(vec![
            ("name", Json::Str(name)),
            ("cubes", Json::Arr(vec![obj(cube)])),
        ]));
    }
    let desc = vec![
        ("identifier", Json::Str(identifier.to_string())),
        ("texture_width", Json::Num(16.0)),
        ("texture_height", Json::Num(16.0)),
    ];
    let mut geo = vec![("description", obj(desc)), ("bones", Json::Arr(bones))];
    if let Some(t) = display_transforms(model) {
        geo.push(("item_display_transforms", t));
    }
    let json = obj(vec![
        ("format_version", Json::Str("1.21.0".into())),
        ("minecraft:geometry", Json::Arr(vec![obj(geo)])),
    ]);
    Ok(Geometry {
        culling,
        json,
        materials,
        unverified,
    })
}

/// Java `display` -> Bedrock `item_display_transforms`, as Mojang's pair
/// shows: every slot written in full (rotation 0, translation 0, scale 1 by
/// default), a missing left hand taking the right hand's values; slots
/// Bedrock has no name for (`on_shelf`) dropped.
fn display_transforms(model: &Resolved) -> Option<Json> {
    if model.display.is_empty() {
        return None;
    }
    let slot = |name: &str| -> Option<Json> {
        let d = model.display.get(name)?;
        let r = vec3(d.get("rotation")).unwrap_or([0.0; 3]);
        let t = vec3(d.get("translation")).unwrap_or([0.0; 3]);
        let s = vec3(d.get("scale")).unwrap_or([1.0; 3]);
        Some(obj(vec![
            ("rotation", nums(&r)),
            ("translation", nums(&t)),
            ("scale", nums(&s)),
        ]))
    };
    let mut out = Vec::new();
    for (name, fallback) in [
        ("firstperson_lefthand", Some("firstperson_righthand")),
        ("firstperson_righthand", None),
        ("thirdperson_lefthand", Some("thirdperson_righthand")),
        ("thirdperson_righthand", None),
        ("head", None),
        ("gui", None),
        ("ground", None),
        ("fixed", None),
    ] {
        if let Some(v) = slot(name).or_else(|| fallback.and_then(slot)) {
            out.push((name.to_string(), v));
        }
    }
    Some(Json::Obj(JsonMap::from_entries(out)))
}

/// Bedrock's `block_culling` file for these rules.
pub fn culling_file(identifier: &str, rules: &[(String, String, String)]) -> Json {
    let s = |v: &str| Json::Str(v.to_string());
    let obj = |e: Vec<(&str, Json)>| {
        Json::Obj(JsonMap::from_entries(
            e.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        ))
    };
    let rules: Vec<Json> = rules
        .iter()
        .map(|(bone, face, dir)| {
            obj(vec![
                (
                    "geometry_part",
                    obj(vec![
                        ("bone", s(bone)),
                        ("cube", Json::Num(0.0)),
                        ("face", s(face)),
                    ]),
                ),
                ("direction", s(dir)),
            ])
        })
        .collect();
    obj(vec![
        ("format_version", s("1.20.60")),
        (
            "minecraft:block_culling_rules",
            obj(vec![
                ("description", obj(vec![("identifier", s(identifier))])),
                ("rules", Json::Arr(rules)),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_and_tops_follow_mojangs_rules() {
        let m = Resolved {
            elements: Some(vec![kebitx_core::json::parse(
                r##"{"name":"base","from":[16,0,0],"to":[19,4,16],"faces":{
                    "north":{"uv":[4,10,8,11],"texture":"#all"},
                    "up":{"uv":[8,10,4,6],"texture":"#all","rotation":180}}}"##,
            )
            .unwrap()]),
            textures: [("all".to_string(), "block/straw_bed".to_string())].into(),
            ..Default::default()
        };
        let g = to_geometry("geometry.test", &m).unwrap();
        let text = kebitx_core::json::to_string(&g.json);
        assert!(text.contains(r#""origin":[-11,0,-8]"#), "{text}");
        assert!(
            text.contains(r#""north":{"material_instance":"all","uv":[4,10],"uv_size":[4,1]}"#),
            "{text}"
        );
        assert!(
            text.contains(
                r#""up":{"material_instance":"all","uv":[4,6],"uv_rotation":180,"uv_size":[4,4]}"#
            ),
            "{text}"
        );
        assert_eq!(g.materials["all"], "block/straw_bed");
        assert_eq!(g.unverified, 0);
    }

    #[test]
    fn texture_references_resolve_through_the_chain() {
        let m = Resolved {
            textures: [("all", "#side"), ("side", "block/stone")]
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            ..Default::default()
        };
        assert_eq!(m.texture("#all").as_deref(), Some("block/stone"));
        assert_eq!(m.texture("#missing"), None);
        assert_eq!(
            model_path("minecraft:block/cube_all").as_deref(),
            Some("assets/minecraft/models/block/cube_all.json")
        );
        assert_eq!(model_path("block/../x"), None);
    }
}
