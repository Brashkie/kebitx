//! Graphics diagnostics: why a model or texture LOOKS wrong.
//!
//! - **UV outside the canvas**: a cube's UV area reaches past the canvas the
//!   model declares (`texture_width` × `texture_height`). Those faces sample
//!   pixels that are not there: transparent, or wrapped garbage.
//! - **Stretched texture**: the image's proportions differ from the declared
//!   canvas. UVs are divided by the declared size, so a 64×64 image on a 64×32
//!   canvas is stretched to twice its height. A uniform scale (128×64 on 64×32,
//!   an HD texture) is fine.
//!
//! Precision first. A canvas the file does not declare is not guessed. Box UV
//! with fractional sizes rounds in ways the format leaves open, so only an
//! overflow larger than one texel counts. A texture is paired with a geometry
//! only when a render controller names both directly; arrays and Molang
//! expressions are not paired, and neither is a controller with `uv_anim`
//! (a flipbook strip of 75 frames is 75 times taller on purpose).

use crate::filesystem::{
    all_files, asset_stem, discover_animation_files, discover_entities, discover_models,
    discover_render_controllers,
};
use crate::image::dimensions;
use crate::json::{self, Json};
use crate::manifest::{Pack, PackKind};
use crate::par::par_map;
use crate::parser::model::{parse_models, CubeUv, ModelDef};
use crate::parser::render_controller::{classify, Slot, SlotRef};
use crate::parser::{parse_client_entity, ClientEntityDecl};
use crate::registry::controller::ControllerRegistry;
use crate::vanilla::VanillaCatalog;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// Box UV overflow tolerated, in texels (fractional sizes round loosely).
const BOX_TOLERANCE: f64 = 1.0;
/// Per-face UVs are exact; this only absorbs float noise.
const FACE_TOLERANCE: f64 = 0.01;
/// Relative difference of proportions tolerated before calling it stretched.
const ASPECT_TOLERANCE: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub enum GraphicsIssue {
    UvOutside {
        geometry: String,
        file: String,
        /// Cubes with at least one face outside.
        cubes: usize,
        /// Worst overflow, in texels.
        worst: f64,
        /// Bone of the worst cube.
        bone: String,
    },
    Stretched {
        geometry: String,
        texture: String,
        declared: [f64; 2],
        image: (u32, u32),
        entities: Vec<String>,
    },
    /// An animation none of whose bones exist in any of the entity's models:
    /// the whole animation does nothing. Usually a bone renamed in the model
    /// (`cabeza` instead of `head`) or an animation meant for another model.
    AnimationBones {
        entity: String,
        animation: String,
        missing: Vec<String>,
    },
    /// A bone whose `parent` names no bone of the model (nor of the model it
    /// inherits from): it hangs from the root instead, and does not follow
    /// the body it was meant to follow.
    BoneParentMissing {
        geometry: String,
        file: String,
        bone: String,
        parent: String,
    },
    /// Two bones with the same name (case ignored) in one model: an animation
    /// or a `part_visibility` reaches only one of them.
    DuplicateBones {
        geometry: String,
        file: String,
        bones: Vec<String>,
    },
    /// Cubes turned inside out: a negative size, or an `inflate` that shrinks
    /// a side below zero. Their faces point inwards and are culled.
    InvertedCubes {
        geometry: String,
        file: String,
        cubes: usize,
        bone: String,
    },
    /// Cubes of size 0 × 0 × 0 with no inflate: they never render.
    EmptyCubes {
        geometry: String,
        file: String,
        cubes: usize,
    },
    /// An animation scales a bone to 0 (it vanishes while the animation plays)
    /// or to 20× or more. Often intended — hiding a weapon — so information.
    AnimationScale {
        animation: String,
        file: String,
        bone: String,
        value: [f64; 3],
    },
    /// Compiled shaders (`*.material.bin`): only a modified client loads them;
    /// the unmodified game ignores them, so the pack does nothing visible.
    ShaderNeedsModifiedClient { pack: String, files: usize },
    /// Vibrant Visuals content (`atmospherics/`, `lighting/`, `pbr`, ...): it
    /// shows only on a device that supports Vibrant Visuals, with it enabled.
    VibrantVisuals { pack: String, parts: Vec<String> },
}

/// Scale at or above which an animated bone is reported as huge.
const HUGE_SCALE: f64 = 20.0;

impl GraphicsIssue {
    /// Information explains a look that may well be intended; a warning is a
    /// defect in the files.
    pub fn is_info(&self) -> bool {
        matches!(
            self,
            GraphicsIssue::EmptyCubes { .. }
                | GraphicsIssue::AnimationScale { .. }
                | GraphicsIssue::VibrantVisuals { .. }
        )
    }
}

#[derive(Debug, Default)]
pub struct GraphicsReport {
    pub models: usize,
    pub models_without_canvas: usize,
    pub cubes: usize,
    pub pairs_checked: usize,
    /// (entity, animation) pairs whose bones could be compared.
    pub animations_checked: usize,
    /// Entities skipped because a model of theirs is not in the packs.
    pub entities_unknown_bones: usize,
    /// Animations that move SOME bones the model lacks: normal for shared
    /// animations, counted, not reported.
    pub animations_partial: usize,
    pub issues: Vec<GraphicsIssue>,
}

pub fn check(packs: &[Pack], catalog: &VanillaCatalog) -> GraphicsReport {
    let rps: Vec<&Pack> = packs
        .iter()
        .filter(|p| p.manifest.kind == PackKind::Resources)
        .collect();
    let mut report = GraphicsReport::default();

    // ---- every model. One identifier can be defined in several files (and
    // packs); which one the game keeps depends on pack order, so ALL are kept.
    let mut models: Models = HashMap::new();
    let files: Vec<(&Pack, PathBuf)> = rps
        .iter()
        .flat_map(|p| discover_models(&p.root).into_iter().map(move |f| (*p, f)))
        .collect();
    // parse on every core; check and index in file order, as before
    let parsed = par_map(&files, |(p, f)| {
        let rel = f
            .strip_prefix(&p.root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let defs = crate::snapshot::read_to_string(f)
            .ok()
            .and_then(|src| json::parse(&src).ok())
            .map(|root| parse_models(&root))
            .unwrap_or_default();
        (format!("{}/{rel}", p.folder), defs)
    });
    // bone parents are checked once every model is known: a child model may
    // hang its bones from the bones of the model it inherits from
    struct Pending {
        geometry: String,
        file: String,
        /// The old-format model it inherits from, if any.
        inherits: Option<String>,
        /// (bone, parent) pairs whose parent is not a bone of the model itself.
        wanted: Vec<(String, String)>,
    }
    let mut parents_to_check: Vec<Pending> = Vec::new();
    for (place, defs) in parsed {
        for m in defs {
            report.models += 1;
            uv_bounds(&m, &place, &mut report);
            structure(&m, &place, &mut report);
            let own: HashSet<String> = m
                .bones
                .iter()
                .map(|b| b.name.to_ascii_lowercase())
                .collect();
            let wanted: Vec<(String, String)> = m
                .bones
                .iter()
                .filter_map(|b| b.parent.as_ref().map(|p| (b.name.clone(), p.clone())))
                .filter(|(_, p)| !own.contains(&p.to_ascii_lowercase()))
                .collect();
            if !wanted.is_empty() {
                parents_to_check.push(Pending {
                    geometry: m.identifier.clone(),
                    file: place.clone(),
                    inherits: m.parent.clone(),
                    wanted,
                });
            }
            models
                .entry(m.identifier.to_ascii_lowercase())
                .or_default()
                .push(m);
        }
    }
    for Pending {
        geometry,
        file,
        inherits,
        wanted,
    } in parents_to_check
    {
        // an inherited model outside the packs: its bones are unknown
        let inherited = match &inherits {
            Some(parent) => match model_bones(parent, &models) {
                Some(b) => b,
                None => continue,
            },
            None => HashSet::new(),
        };
        for (bone, parent) in wanted {
            if !inherited.contains(&parent.to_ascii_lowercase()) {
                report.issues.push(GraphicsIssue::BoneParentMissing {
                    geometry: geometry.clone(),
                    file: file.clone(),
                    bone,
                    parent,
                });
            }
        }
    }

    shaders(&rps, &mut report);

    // every client entity, parsed once on every core, kept in file order
    let entity_files: Vec<PathBuf> = rps
        .iter()
        .flat_map(|p| discover_entities(&p.root))
        .collect();
    let entities: Vec<ClientEntityDecl> = par_map(&entity_files, |f| {
        crate::snapshot::read_to_string(f)
            .ok()
            .and_then(|s| parse_client_entity(&s).ok())
    })
    .into_iter()
    .flatten()
    .collect();

    stretched(&rps, &models, &entities, catalog, &mut report);
    animation_bones(&rps, &models, &entities, &mut report);
    report
}

/// Model identifier (lowercased) -> every definition of it.
type Models = HashMap<String, Vec<ModelDef>>;

/// Checks that need only the model itself: repeated bone names, cubes turned
/// inside out, cubes that never render.
fn structure(m: &ModelDef, place: &str, report: &mut GraphicsReport) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for b in &m.bones {
        *seen.entry(b.name.to_ascii_lowercase()).or_default() += 1;
    }
    let mut dups: Vec<String> = seen
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(b, _)| b)
        .collect();
    if !dups.is_empty() {
        dups.sort();
        report.issues.push(GraphicsIssue::DuplicateBones {
            geometry: m.identifier.clone(),
            file: place.to_string(),
            bones: dups,
        });
    }

    let (mut inverted, mut first_inverted, mut empty) = (0, String::new(), 0);
    for b in &m.bones {
        for c in &b.cubes {
            let negative = c.size.iter().any(|&s| s < 0.0);
            // a side that exists but is shrunk past zero by a negative inflate
            let shrunk = c.size.iter().any(|&s| s > 0.0 && s + 2.0 * c.inflate < 0.0);
            if negative || shrunk {
                inverted += 1;
                if first_inverted.is_empty() {
                    first_inverted = b.name.clone();
                }
            } else if c.size == [0.0; 3] && c.inflate <= 0.0 {
                empty += 1;
            }
        }
    }
    if inverted > 0 {
        report.issues.push(GraphicsIssue::InvertedCubes {
            geometry: m.identifier.clone(),
            file: place.to_string(),
            cubes: inverted,
            bone: first_inverted,
        });
    }
    if empty > 0 {
        report.issues.push(GraphicsIssue::EmptyCubes {
            geometry: m.identifier.clone(),
            file: place.to_string(),
            cubes: empty,
        });
    }
}

/// Folders and capabilities that are Vibrant Visuals content.
const VIBRANT_VISUALS: [&str; 5] = [
    "atmospherics",
    "lighting",
    "color_grading",
    "water",
    "point_lights",
];

fn shaders(rps: &[&Pack], report: &mut GraphicsReport) {
    for p in rps {
        let bins = all_files(&p.root)
            .iter()
            .filter(|f| {
                f.to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".material.bin")
            })
            .count();
        if bins > 0 {
            report
                .issues
                .push(GraphicsIssue::ShaderNeedsModifiedClient {
                    pack: p.folder.clone(),
                    files: bins,
                });
        }
        let mut parts: Vec<String> = VIBRANT_VISUALS
            .iter()
            .filter(|d| p.root.join(d).is_dir())
            .map(|d| d.to_string())
            .collect();
        let pbr = crate::snapshot::read_to_string(p.root.join("manifest.json"))
            .ok()
            .and_then(|s| json::parse(&s).ok())
            .and_then(|m| m.get("capabilities").and_then(Json::as_array).cloned())
            .is_some_and(|caps| caps.iter().any(|c| c.as_str() == Some("pbr")));
        if pbr {
            parts.push("pbr".into());
        }
        if !parts.is_empty() {
            report.issues.push(GraphicsIssue::VibrantVisuals {
                pack: p.folder.clone(),
                parts,
            });
        }
    }
}

/// Every constant scale in an animation's `scale` value: a number, a
/// `[x, y, z]` of numbers, or keyframes of those (with `pre` / `post`). Molang
/// strings are not evaluated, so they yield nothing.
fn constant_scales(v: &Json, out: &mut Vec<[f64; 3]>) {
    match v {
        Json::Num(n) => out.push([*n; 3]),
        Json::Arr(a) if a.len() == 3 => {
            if let (Some(Json::Num(x)), Some(Json::Num(y)), Some(Json::Num(z))) =
                (a.first(), a.get(1), a.get(2))
            {
                out.push([*x, *y, *z]);
            }
        }
        Json::Obj(m) => m.values().for_each(|x| constant_scales(x, out)),
        _ => {}
    }
}

/// The first constant scale of a bone that makes it vanish (all zero) or
/// grow huge.
fn extreme_scale(bone: &Json) -> Option<[f64; 3]> {
    let mut found = Vec::new();
    constant_scales(bone.get("scale")?, &mut found);
    found
        .into_iter()
        .find(|s| *s == [0.0; 3] || s.iter().any(|c| c.abs() >= HUGE_SCALE))
}

/// The canvas of a model, only if every definition of it agrees.
fn canvas(id: &str, models: &Models) -> Option<[f64; 2]> {
    let defs = models.get(&id.to_ascii_lowercase())?;
    let first = defs.first()?.texture_size?;
    defs.iter()
        .all(|d| d.texture_size == Some(first))
        .then_some(first)
}

/// Bones a model can have: every definition of it and of its old-format
/// parents, lowercased. `None` if any model in the chain is not in the packs.
/// Several definitions give the UNION: a bone counts as present if any
/// version of the model has it, since any version may be the one in game.
fn model_bones(id: &str, models: &Models) -> Option<HashSet<String>> {
    let mut out = HashSet::new();
    let mut todo = vec![id.to_ascii_lowercase()];
    let mut visited = HashSet::new();
    while let Some(cur) = todo.pop() {
        // bounded: a cycle of parents must not loop forever
        if !visited.insert(cur.clone()) || visited.len() > 64 {
            continue;
        }
        for m in models.get(&cur)? {
            out.extend(m.bones.iter().map(|b| b.name.to_ascii_lowercase()));
            if let Some(parent) = &m.parent {
                todo.push(parent.to_ascii_lowercase());
            }
        }
    }
    Some(out)
}

fn animation_bones(
    rps: &[&Pack],
    models: &Models,
    entities: &[ClientEntityDecl],
    report: &mut GraphicsReport,
) {
    // animation id (lowercased) -> bone names it moves (lowercased); parsed on
    // every core, inserted in file order (the first definition wins, as before)
    let files: Vec<(&Pack, PathBuf)> = rps
        .iter()
        .flat_map(|p| {
            discover_animation_files(&p.root)
                .into_iter()
                .map(move |f| (*p, f))
        })
        .collect();
    type Moved = Vec<(String, Vec<String>)>;
    let per_file: Vec<(Moved, Vec<GraphicsIssue>)> = par_map(&files, |(p, f)| {
        let Ok(src) = crate::snapshot::read_to_string(f) else {
            return (Vec::new(), Vec::new());
        };
        let Ok(root) = json::parse(&src) else {
            return (Vec::new(), Vec::new());
        };
        let rel = f
            .strip_prefix(&p.root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let mut moved = Vec::new();
        let mut scales = Vec::new();
        for (id, body) in root
            .get("animations")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let bones = body.get("bones").and_then(Json::as_object);
            moved.push((
                id.to_ascii_lowercase(),
                bones
                    .map(|b| b.keys().map(|k| k.to_ascii_lowercase()).collect())
                    .unwrap_or_default(),
            ));
            for (bone, channels) in bones.into_iter().flatten() {
                if let Some(value) = extreme_scale(channels) {
                    scales.push(GraphicsIssue::AnimationScale {
                        animation: id.clone(),
                        file: format!("{}/{rel}", p.folder),
                        bone: bone.clone(),
                        value,
                    });
                }
            }
        }
        (moved, scales)
    });
    let mut anims: HashMap<String, Vec<String>> = HashMap::new();
    for (moved, scales) in per_file {
        for (id, bones) in moved {
            anims.entry(id).or_insert(bones);
        }
        report.issues.extend(scales);
    }

    let mut seen: HashSet<(String, String)> = HashSet::new();
    {
        for decl in entities {
            if decl.geometry.is_empty() {
                continue;
            }
            // every model the entity can show; one unknown model and the
            // entity's bones are unknown
            let mut bones = HashSet::new();
            let mut known = true;
            for g in decl.geometry.values() {
                match model_bones(g, models) {
                    Some(b) => bones.extend(b),
                    None => known = false,
                }
            }
            if !known {
                report.entities_unknown_bones += 1;
                continue;
            }
            for anim in decl.animations.values() {
                let Some(moved) = anims.get(&anim.to_ascii_lowercase()) else {
                    continue;
                };
                if moved.is_empty() {
                    continue;
                }
                // one entity declared in two files is one entity
                if !seen.insert((
                    decl.identifier.to_ascii_lowercase(),
                    anim.to_ascii_lowercase(),
                )) {
                    continue;
                }
                report.animations_checked += 1;
                let mut missing: Vec<String> = moved
                    .iter()
                    .filter(|b| !bones.contains(*b))
                    .cloned()
                    .collect();
                if missing.is_empty() {
                    continue;
                }
                // Shared animations (`look_at_target` moves head AND arms) are
                // written for many models: a buffalo without arms is fine.
                // Only an animation that moves NOTHING is a defect.
                if missing.len() < moved.len() {
                    report.animations_partial += 1;
                    continue;
                }
                missing.sort();
                report.issues.push(GraphicsIssue::AnimationBones {
                    entity: decl.identifier.clone(),
                    animation: anim.clone(),
                    missing,
                });
            }
        }
    }
}

fn uv_bounds(m: &ModelDef, place: &str, report: &mut GraphicsReport) {
    let Some([w, h]) = m.texture_size else {
        report.models_without_canvas += 1;
        return;
    };
    let mut cubes_out = 0;
    let mut worst = 0.0_f64;
    let mut worst_bone = String::new();
    for bone in &m.bones {
        for cube in &bone.cubes {
            report.cubes += 1;
            let over = match &cube.uv {
                Some(CubeUv::Box(uv)) => box_faces(*uv, cube.size)
                    .into_iter()
                    .map(|(us, vs)| overflow(us, vs, w, h))
                    .filter(|o| *o > BOX_TOLERANCE)
                    .fold(0.0, f64::max),
                Some(CubeUv::PerFace(faces)) => faces
                    .iter()
                    .map(|(_, [u, v], [fw, fh])| {
                        overflow(
                            (u.min(u + fw), u.max(u + fw)),
                            (v.min(v + fh), v.max(v + fh)),
                            w,
                            h,
                        )
                    })
                    .filter(|o| *o > FACE_TOLERANCE)
                    .fold(0.0, f64::max),
                None => 0.0,
            };
            if over > 0.0 {
                cubes_out += 1;
                if over > worst {
                    worst = over;
                    worst_bone = bone.name.clone();
                }
            }
        }
    }
    if cubes_out > 0 {
        report.issues.push(GraphicsIssue::UvOutside {
            geometry: m.identifier.clone(),
            file: place.to_string(),
            cubes: cubes_out,
            worst,
            bone: worst_bone,
        });
    }
}

/// The faces of a box-UV cube that can render, as `(u range, v range)`.
/// Box layout for size (x, y, z) at offset (u, v):
///
/// ```text
///            u    u+z    u+z+x  u+2z+x  u+2z+2x
///   v             | up   | down |
///   v+z      | east | north | west | south |
///   v+z+y
/// ```
///
/// A face with zero area (the sides of a flat plane) never renders, so where
/// its UV would land does not matter.
fn box_faces([u, v]: [f64; 2], size: [f64; 3]) -> Vec<((f64, f64), (f64, f64))> {
    let [x, y, z] = size.map(f64::abs);
    [
        (u + z, v, x, z),               // up
        (u + z + x, v, x, z),           // down
        (u, v + z, z, y),               // east
        (u + z, v + z, x, y),           // north
        (u + z + x, v + z, z, y),       // west
        (u + 2.0 * z + x, v + z, x, y), // south
    ]
    .into_iter()
    .filter(|&(_, _, fw, fh)| fw > 0.0 && fh > 0.0)
    .map(|(fu, fv, fw, fh)| ((fu, fu + fw), (fv, fv + fh)))
    .collect()
}

/// How far a UV rectangle reaches outside `[0, w] × [0, h]`, in texels.
fn overflow((u0, u1): (f64, f64), (v0, v1): (f64, f64), w: f64, h: f64) -> f64 {
    [-u0, u1 - w, -v0, v1 - h].into_iter().fold(0.0, f64::max)
}

fn stretched(
    rps: &[&Pack],
    models: &Models,
    decls: &[ClientEntityDecl],
    catalog: &VanillaCatalog,
    report: &mut GraphicsReport,
) {
    // texture stem -> image file, across every resource pack
    let mut images: HashMap<String, PathBuf> = HashMap::new();
    for p in rps {
        for f in all_files(&p.root.join("textures")) {
            if let Ok(rel) = f.strip_prefix(&p.root) {
                images
                    .entry(asset_stem(&rel.to_string_lossy()))
                    .or_insert(f);
            }
        }
    }
    let rc_files: Vec<PathBuf> = rps
        .iter()
        .flat_map(|p| discover_render_controllers(&p.root))
        .collect();
    let rcs = ControllerRegistry::build(&rc_files);

    // (geometry, texture path) -> entities using that pair
    let mut pairs: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    {
        for decl in decls {
            let find = |map: &BTreeMap<String, String>, name: &str| {
                map.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone())
            };
            for rc in &decl.render_controllers {
                let defs = match rcs.resolve(rc) {
                    Some(id) => rcs.definitions(id),
                    None => catalog.render_controller_defs(rc),
                };
                // a flipbook strip under `uv_anim` is tall on purpose
                for def in defs.iter().filter(|d| !d.uv_anim) {
                    let mut geometry = None;
                    let mut textures = Vec::new();
                    for u in &def.uses {
                        match (u.slot, classify(&u.raw)) {
                            (Slot::Geometry, SlotRef::Direct(_, n)) => {
                                geometry = find(&decl.geometry, &n)
                            }
                            (Slot::Texture, SlotRef::Direct(_, n)) => {
                                textures.extend(find(&decl.textures, &n))
                            }
                            _ => {}
                        }
                    }
                    let Some(geometry) = geometry else { continue };
                    for t in textures {
                        pairs
                            .entry((geometry.clone(), t))
                            .or_default()
                            .push(decl.identifier.clone());
                    }
                }
            }
        }
    }

    for ((geometry, texture), mut entities) in pairs {
        let Some([tw, th]) = canvas(&geometry, models) else {
            continue;
        };
        let Some(file) = images.get(&asset_stem(&texture)) else {
            continue;
        };
        let Some((iw, ih)) = dimensions(file) else {
            continue;
        };
        if tw <= 0.0 || th <= 0.0 {
            continue;
        }
        report.pairs_checked += 1;
        let declared = tw / th;
        let actual = iw as f64 / ih as f64;
        if ((actual - declared) / declared).abs() > ASPECT_TOLERANCE {
            entities.sort();
            entities.dedup();
            report.issues.push(GraphicsIssue::Stretched {
                geometry,
                texture,
                declared: [tw, th],
                image: (iw, ih),
                entities,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_measures_the_worst_side() {
        assert_eq!(overflow((0.0, 64.0), (0.0, 32.0), 64.0, 32.0), 0.0);
        assert_eq!(overflow((60.0, 70.0), (0.0, 8.0), 64.0, 32.0), 6.0);
        assert_eq!(overflow((-2.0, 4.0), (30.0, 40.0), 64.0, 32.0), 8.0);
    }

    #[test]
    fn a_flat_plane_has_only_its_two_faces() {
        // the real sign: 34 x 0 x 30 at [0, 22] on a 64 x 64 canvas
        let faces = box_faces([0.0, 22.0], [34.0, 0.0, 30.0]);
        assert_eq!(
            faces,
            [((30.0, 64.0), (22.0, 52.0)), ((64.0, 98.0), (22.0, 52.0))]
        );
        let worst = faces
            .into_iter()
            .map(|(u, v)| overflow(u, v, 64.0, 64.0))
            .fold(0.0, f64::max);
        assert_eq!(
            worst, 34.0,
            "the back of the sign is outside; the front is not"
        );
    }

    #[test]
    fn a_standard_box_fits_its_canvas() {
        // vanilla body: 8 x 12 x 4 at [16, 16] on 64 x 32 fills exactly to 40 x 32
        let worst = box_faces([16.0, 16.0], [8.0, 12.0, 4.0])
            .into_iter()
            .map(|(u, v)| overflow(u, v, 64.0, 32.0))
            .fold(0.0, f64::max);
        assert_eq!(worst, 0.0);
    }
}
