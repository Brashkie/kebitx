//! `analyze`: walks one resource pack, or a whole world of packs, and fills reports.
//!
//! Scope: DIRECT reference resolution for entity textures, geometry, animations,
//! animation controllers, render controllers and texture atlases, with the
//! vanilla catalog deciding what is External.
//! Not yet: orphan detection (where BoundedOpaque bites).
//!
//! WORLD MODE. A world is not one pack: it is a stack of them, and the game
//! resolves a texture path or a geometry identifier against ALL of them at once.
//! Analysing packs one by one gets this wrong in both directions — a reference
//! satisfied by another pack looks broken, and nobody notices. So in world mode
//! the registries are the UNION of every resource pack, and each pack's
//! references are resolved against that union. The report is still per pack.

use crate::filesystem::all_files;
use crate::filesystem::{
    discover_animation_files, discover_entities, discover_models, discover_render_controllers,
    discover_textures,
};
use crate::json::{self, Json};
use crate::manifest::{packs_in, PackKind};
use crate::par::par_map;
use crate::parser::atlas::parse_atlas;
use crate::parser::material::material_names;
use crate::parser::parse_client_entity;
use crate::parser::render_controller::Slot;
use crate::registry::animation::AnimationRegistry;
use crate::registry::controller::ControllerRegistry;
use crate::registry::geometry::GeometryRegistry;
use crate::registry::texture::TextureRegistry;
use crate::report::{AnalysisReport, Issue};
use crate::resolver::animation::{
    resolve_animation_controller, resolve_animation_ref, resolve_render_controller, IdResolution,
};
use crate::resolver::geometry::{resolve_geometry, GeometryResolution};
use crate::resolver::render_controller::{check_all, EntitySlots, SlotCheck};
use crate::resolver::texture::{TextureResolution, TextureResolver};
use crate::vanilla::VanillaCatalog;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const ATLASES: [&str; 2] = [
    "textures/terrain_texture.json",
    "textures/item_texture.json",
];

/// Analyze a pack against the installed game. Finding no install is not an
/// error: the catalog is simply empty and behaviour degrades to the old one.
pub fn analyze(rp_root: &Path) -> AnalysisReport {
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    analyze_with(rp_root, &catalog)
}

pub fn analyze_with(rp_root: &Path, catalog: &VanillaCatalog) -> AnalysisReport {
    analyze_packs(&[rp_root.to_path_buf()], catalog)
        .into_iter()
        .next()
        .map(|(_, r)| r)
        .unwrap_or_default()
}

/// Resource packs inside a world folder, in either layout (see
/// `manifest::packs_in`). Behavior packs, skin packs and loose folders are skipped.
pub fn resource_packs_in(world: &Path) -> Vec<PathBuf> {
    packs_in(world)
        .into_iter()
        .filter(|p| p.manifest.kind == PackKind::Resources)
        .map(|p| p.root)
        .collect()
}

pub fn is_resource_pack(dir: &Path) -> bool {
    let Ok(src) = crate::snapshot::read_to_string(dir.join("manifest.json")) else {
        return false;
    };
    let Ok(root) = json::parse(&src) else {
        return false;
    };
    root.get("modules")
        .and_then(Json::as_array)
        .map(|mods| {
            mods.iter()
                .any(|m| m.get("type").and_then(Json::as_str) == Some("resources"))
        })
        .unwrap_or(false)
}

/// World mode entry point: every resource pack, resolved against the union.
pub fn analyze_world_with(world: &Path, catalog: &VanillaCatalog) -> Vec<(String, AnalysisReport)> {
    analyze_packs(&resource_packs_in(world), catalog)
}

/// Core: build the registries over ALL given packs (Index Pass), then resolve
/// each pack against them.
pub fn analyze_packs(packs: &[PathBuf], catalog: &VanillaCatalog) -> Vec<(String, AnalysisReport)> {
    // Index Pass over every pack. Texture paths are pack-relative, so the union
    // is just the union of the relative paths.
    let mut all_textures = Vec::new();
    let mut all_models = Vec::new();
    let mut all_animations = Vec::new();
    let mut all_render = Vec::new();
    for p in packs {
        all_textures.extend(discover_textures(p));
        all_models.extend(discover_models(p));
        all_animations.extend(discover_animation_files(p));
        all_render.extend(discover_render_controllers(p));
    }
    let idx = Indexes {
        textures: TextureRegistry::from_files(all_textures),
        geometry: GeometryRegistry::build(&all_models),
        animations: AnimationRegistry::build(&all_animations),
        render: ControllerRegistry::build(&all_render),
        materials: pack_materials(packs),
    };

    // each pack against the shared indexes, on every core; reports in pack order
    par_map(packs, |p| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        (name, analyze_one(p, &idx, catalog))
    })
}

/// Everything the Index Pass produced, over the union of the packs.
struct Indexes {
    textures: TextureRegistry,
    geometry: GeometryRegistry,
    animations: AnimationRegistry,
    render: ControllerRegistry,
    /// Material names (lowercased) the packs define in `materials/*.material`.
    materials: HashSet<String>,
}

fn pack_materials(packs: &[PathBuf]) -> HashSet<String> {
    let mut out = HashSet::new();
    for p in packs {
        for f in all_files(&p.join("materials")) {
            if !f
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("material"))
            {
                continue;
            }
            let Ok(src) = crate::snapshot::read_to_string(&f) else {
                continue;
            };
            let Ok(root) = json::parse(&src) else {
                continue;
            };
            out.extend(material_names(&root).iter().map(|m| m.to_ascii_lowercase()));
        }
    }
    out
}

/// Count one identifier-space resolution and record the issue if it is missing.
fn count_id(
    report: &mut AnalysisReport,
    r: IdResolution,
    kind: &'static str,
    owner: &str,
    id: &str,
) {
    let render = kind == "render";
    match r {
        IdResolution::Resolved if render => report.render_controllers_resolved += 1,
        IdResolution::Resolved => report.animations_resolved += 1,
        IdResolution::External if render => report.render_controllers_external += 1,
        IdResolution::External => report.animations_external += 1,
        IdResolution::KnownMissing => {
            if render {
                report.render_controllers_missing += 1;
            } else {
                report.animations_missing += 1;
            }
            report.issues.push(Issue::Missing {
                kind,
                owner: owner.to_string(),
                reference: id.to_string(),
            });
        }
    }
}

/// Count the slot checks of one (entity, render controller) pair.
fn check_slots(
    report: &mut AnalysisReport,
    owner: &str,
    rc: &str,
    checks: &[(Slot, String, SlotCheck)],
) {
    for (slot, raw, c) in checks {
        report.slots_checked += 1;
        match c {
            SlotCheck::Resolved => report.slots_resolved += 1,
            SlotCheck::Bounded => report.slots_bounded += 1,
            SlotCheck::Opaque => report.slots_opaque += 1,
            SlotCheck::Missing => {
                report.slots_missing += 1;
                report.issues.push(Issue::Missing {
                    kind: match slot {
                        Slot::Texture => "texture slot",
                        Slot::Geometry => "geometry slot",
                        Slot::Material => "material slot",
                    },
                    owner: owner.to_string(),
                    reference: format!("{rc}: {raw}"),
                });
            }
        }
    }
}

fn analyze_one(rp_root: &Path, idx: &Indexes, catalog: &VanillaCatalog) -> AnalysisReport {
    let mut report = AnalysisReport::default();
    let mut tex_resolver = TextureResolver::new(&idx.textures, catalog);
    let mut unique_files: BTreeSet<String> = BTreeSet::new();

    // ---- client entities
    for entity_path in discover_entities(rp_root) {
        let Ok(src) = crate::snapshot::read_to_string(&entity_path) else {
            continue;
        };
        let decl = match parse_client_entity(&src) {
            Ok(d) => d,
            Err(e) => {
                // A file with no client_entity is just not an entity file; only
                // a genuine parse failure is worth reporting.
                if !e.0.starts_with("missing minecraft:client_entity") {
                    report.unreadable += 1;
                    report.issues.push(Issue::Unreadable {
                        file: entity_path.display().to_string(),
                        why: e.0,
                    });
                }
                continue;
            }
        };
        report.entities += 1;
        let owner = decl.identifier.clone();

        report.textures_declared += decl.textures.len();
        for path in decl.textures.values() {
            unique_files.insert(path.clone());
            match tex_resolver.resolve(path) {
                TextureResolution::Resolved(_) => report.textures_resolved += 1,
                TextureResolution::External => report.textures_external += 1,
                TextureResolution::CaseMismatch(on_disk) => {
                    report.case_mismatches += 1;
                    report.issues.push(Issue::CaseMismatch {
                        owner: owner.clone(),
                        reference: path.clone(),
                        on_disk,
                    });
                }
                TextureResolution::KnownMissing => {
                    report.textures_missing += 1;
                    report.issues.push(Issue::Missing {
                        kind: "texture",
                        owner: owner.clone(),
                        reference: path.clone(),
                    });
                }
            }
        }

        report.geometry_declared += decl.geometry.len();
        for identifier in decl.geometry.values() {
            match resolve_geometry(&idx.geometry, catalog, identifier) {
                GeometryResolution::Resolved(_) => report.geometry_resolved += 1,
                GeometryResolution::External => report.geometry_external += 1,
                GeometryResolution::KnownMissing => {
                    report.geometry_missing += 1;
                    report.issues.push(Issue::Missing {
                        kind: "geometry",
                        owner: owner.clone(),
                        reference: identifier.clone(),
                    });
                }
            }
        }

        report.animations_declared += decl.animations.len() + decl.animation_controllers.len();
        for id in decl.animations.values() {
            let r = resolve_animation_ref(&idx.animations, catalog, id);
            count_id(&mut report, r, "animation", &owner, id);
        }
        for (_, id) in &decl.animation_controllers {
            let r = resolve_animation_controller(&idx.animations, catalog, id);
            count_id(&mut report, r, "animation_controller", &owner, id);
        }

        report.materials_declared += decl.materials.len();
        for m in decl.materials.values() {
            if idx.materials.contains(&m.to_ascii_lowercase()) {
                report.materials_resolved += 1;
            } else if catalog.is_empty() {
                // the game's materials are unknown: a missing one is not provable
                report.materials_unchecked += 1;
            } else if catalog.has_material(m) {
                report.materials_external += 1;
            } else {
                report.materials_missing += 1;
                report.issues.push(Issue::Missing {
                    kind: "material",
                    owner: owner.clone(),
                    reference: m.clone(),
                });
            }
        }

        report.render_controllers_declared += decl.render_controllers.len();
        let slots = EntitySlots::of(&decl);
        for id in &decl.render_controllers {
            let r = resolve_render_controller(&idx.render, catalog, id);
            count_id(&mut report, r, "render", &owner, id);
            // A pack's definition sits on top of the game's and replaces it.
            // A controller the engine provides without a file has no body to
            // open, so its slots are not checked.
            let defs = match idx.render.resolve(id) {
                Some(cid) => idx.render.definitions(cid),
                None => catalog.render_controller_defs(id),
            };
            check_slots(&mut report, &owner, id, &check_all(defs, &slots));
        }
    }

    // ---- texture atlases
    for atlas in ATLASES {
        let Ok(src) = crate::snapshot::read_to_string(rp_root.join(atlas)) else {
            continue;
        };
        let entries = match parse_atlas(&src) {
            Ok(e) => e,
            Err(e) => {
                report.unreadable += 1;
                report.issues.push(Issue::Unreadable {
                    file: atlas.to_string(),
                    why: e.0,
                });
                continue;
            }
        };
        for e in entries {
            report.atlas_entries += 1;
            if e.paths.is_empty() {
                report.atlas_empty += 1;
                report.issues.push(Issue::EmptyTextureSet {
                    file: atlas.to_string(),
                    shortname: e.shortname.clone(),
                });
                continue;
            }
            for path in &e.paths {
                report.atlas_paths += 1;
                match tex_resolver.resolve(path) {
                    TextureResolution::Resolved(_) => report.atlas_resolved += 1,
                    TextureResolution::External => report.atlas_external += 1,
                    TextureResolution::CaseMismatch(on_disk) => {
                        report.case_mismatches += 1;
                        report.issues.push(Issue::CaseMismatch {
                            owner: e.shortname.clone(),
                            reference: path.clone(),
                            on_disk,
                        });
                    }
                    TextureResolution::KnownMissing => {
                        report.atlas_missing += 1;
                        report.issues.push(Issue::Missing {
                            kind: "atlas",
                            owner: e.shortname.clone(),
                            reference: path.clone(),
                        });
                    }
                }
            }
        }
    }

    report.texture_files = unique_files.len();
    report
}
