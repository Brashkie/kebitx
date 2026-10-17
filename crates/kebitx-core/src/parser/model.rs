//! The inside of a model file: texture canvas, bones, cubes and their UVs.
//!
//! Both formats:
//! - new (1.12+): `"minecraft:geometry": [{ "description": { "identifier",
//!   "texture_width", "texture_height" }, "bones": [...] }]`
//! - old (1.8): `"geometry.x": { "texturewidth", "textureheight", "bones": [...] }`,
//!   the key optionally carrying a parent: `"geometry.x:geometry.parent"`.
//!
//! What is not in the file stays unknown (`None`): a canvas size the game would
//! default is not guessed, so a check that needs it is simply skipped.

use crate::json::Json;

#[derive(Debug, Clone, PartialEq)]
pub enum CubeUv {
    /// Box UV: one offset, the six faces unfolded around it.
    Box([f64; 2]),
    /// Per-face UV: `(face, [u, v], [w, h])`; a negative size flips the face.
    PerFace(Vec<(String, [f64; 2], [f64; 2])>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cube {
    pub size: [f64; 3],
    pub uv: Option<CubeUv>,
    /// Grows (or, negative, shrinks) the cube on every side; 0 if absent.
    pub inflate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    /// The bone it hangs from, as written; `None` for a root bone.
    pub parent: Option<String>,
    pub cubes: Vec<Cube>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelDef {
    pub identifier: String,
    /// Declared canvas in UV units; `None` when the file does not say.
    pub texture_size: Option<[f64; 2]>,
    pub bones: Vec<Bone>,
    /// Old format only: `"geometry.child:geometry.parent"` inherits the
    /// parent's bones.
    pub parent: Option<String>,
}

pub fn parse_models(root: &Json) -> Vec<ModelDef> {
    let mut out = Vec::new();
    for g in root
        .get("minecraft:geometry")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let Some(desc) = g.get("description") else {
            continue;
        };
        let Some(id) = desc.get("identifier").and_then(Json::as_str) else {
            continue;
        };
        out.push(ModelDef {
            identifier: id.to_string(),
            texture_size: pair(desc.get("texture_width"), desc.get("texture_height")),
            bones: bones(g.get("bones")),
            parent: None,
        });
    }
    for (key, g) in root.as_object().into_iter().flatten() {
        if !key.starts_with("geometry.") {
            continue;
        }
        let mut parts = key.splitn(2, ':');
        out.push(ModelDef {
            identifier: parts.next().unwrap_or(key).trim().to_string(),
            texture_size: pair(g.get("texturewidth"), g.get("textureheight")),
            bones: bones(g.get("bones")),
            parent: parts.next().map(|p| p.trim().to_string()),
        });
    }
    out
}

fn num(v: Option<&Json>) -> Option<f64> {
    match v? {
        Json::Num(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}

fn pair(a: Option<&Json>, b: Option<&Json>) -> Option<[f64; 2]> {
    Some([num(a)?, num(b)?])
}

fn vec_n<const N: usize>(v: Option<&Json>) -> Option<[f64; N]> {
    let a = v?.as_array()?;
    if a.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (i, x) in a.iter().enumerate() {
        out[i] = num(Some(x))?;
    }
    Some(out)
}

fn bones(v: Option<&Json>) -> Vec<Bone> {
    let mut out = Vec::new();
    for b in v.and_then(Json::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        let Some(name) = b.get("name").and_then(Json::as_str) else {
            continue;
        };
        let cubes = b
            .get("cubes")
            .and_then(Json::as_array)
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| {
                        Some(Cube {
                            size: vec_n::<3>(c.get("size"))?,
                            uv: uv(c.get("uv")),
                            inflate: num(c.get("inflate")).unwrap_or(0.0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let parent = b.get("parent").and_then(Json::as_str).map(str::to_string);
        out.push(Bone {
            name: name.to_string(),
            parent,
            cubes,
        });
    }
    out
}

fn uv(v: Option<&Json>) -> Option<CubeUv> {
    let v = v?;
    if v.as_array().is_some() {
        return vec_n::<2>(Some(v)).map(CubeUv::Box);
    }
    let faces = v
        .as_object()?
        .iter()
        .filter_map(|(face, f)| {
            Some((
                face.clone(),
                vec_n::<2>(f.get("uv"))?,
                vec_n::<2>(f.get("uv_size"))?,
            ))
        })
        .collect();
    Some(CubeUv::PerFace(faces))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn both_formats_and_both_uv_kinds() {
        let new = json::parse(
            r#"{"minecraft:geometry": [{"description": {"identifier": "geometry.a", "texture_width": 64, "texture_height": 32},
                "bones": [{"name": "body", "cubes": [
                    {"origin": [0,0,0], "size": [8, 12, 4], "uv": [16, 16]},
                    {"origin": [0,0,0], "size": [1, 1, 1], "uv": {"north": {"uv": [0, 0], "uv_size": [1, -1]}}}
                ]}]}]}"#,
        )
        .unwrap();
        let m = &parse_models(&new)[0];
        assert_eq!(m.texture_size, Some([64.0, 32.0]));
        assert_eq!(m.bones[0].cubes[0].uv, Some(CubeUv::Box([16.0, 16.0])));
        assert_eq!(
            m.bones[0].cubes[1].uv,
            Some(CubeUv::PerFace(vec![(
                "north".into(),
                [0.0, 0.0],
                [1.0, -1.0]
            )]))
        );

        let old =
            json::parse(r#"{"geometry.b:geometry.a": {"bones": [{"name": "head"}]}}"#).unwrap();
        let m = &parse_models(&old)[0];
        assert_eq!(m.identifier, "geometry.b");
        assert_eq!(m.parent.as_deref(), Some("geometry.a"));
        assert_eq!(
            m.texture_size, None,
            "not in the file: unknown, not guessed"
        );
    }
}
