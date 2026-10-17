//! Regression checks between two versions of a project.
//!
//! Each version is analysed with the same game catalog and reduced to a
//! `Snapshot`: the problems found (references, compatibility errors) and the
//! inventory of what it defines (textures, models, animations, controllers,
//! client entities). `compare` then says what is NEW (worked before, fails
//! now: the regression), what was FIXED, and what DISAPPEARED — something
//! other packs or worlds may still rely on even if this project no longer
//! does.
//!
//! Problems are compared by their content, never by position: a report that
//! merely lists the same problems in another order is no change. File paths
//! inside a problem are made relative to the project, so two copies in
//! different folders compare equal. Compatibility errors name packs by their
//! folder: renaming a pack's folder shows as one problem fixed and one new.
//!
//! A problem that is no longer reported is FIXED only if its pack is still
//! there. When the whole pack is gone (a converter that drops the packs, a
//! bad copy), its problems went with it: they are LOST, not fixed, and the
//! missing pack is itself the regression. Packs are known by their manifest
//! UUID, so renaming a folder loses nothing.

use crate::analyze::{analyze_packs, is_resource_pack, resource_packs_in};
use crate::compat::{check_world, Severity};
use crate::filesystem::{
    all_files, asset_stem, discover_animation_files, discover_entities, discover_models,
    discover_render_controllers, is_asset,
};
use crate::json::{self, Json};
use crate::manifest::{packs_in, parse_version, read_manifest};
use crate::parser::model::parse_models;
use crate::parser::parse_client_entity;
use crate::report::Issue;
use crate::vanilla::VanillaCatalog;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Kinds of things a project defines, in report order.
pub const KINDS: [&str; 6] = [
    "client entity",
    "geometry",
    "animation",
    "animation controller",
    "render controller",
    "texture or sound",
];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Every problem, as text: the same problem gives the same text.
    pub problems: BTreeSet<String>,
    /// kind -> identifiers (or asset paths) it defines
    pub defines: BTreeMap<&'static str, BTreeSet<String>>,
    /// pack key (manifest UUID, else `folder:<name>`) -> folder name
    pub packs: BTreeMap<String, String>,
    /// problem -> key of the pack it was found in
    pub problem_packs: BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Regression {
    pub new_problems: Vec<String>,
    /// no longer reported, and their pack is still there
    pub fixed_problems: Vec<String>,
    /// no longer reported because their pack is gone: not fixed
    pub lost_problems: Vec<String>,
    /// packs (folder names) there before and not after
    pub removed_packs: Vec<String>,
    /// packs (folder names) there after and not before
    pub added_packs: Vec<String>,
    /// (kind, identifier) defined before and not after
    pub removed: Vec<(&'static str, String)>,
    /// (kind, identifier) defined after and not before
    pub added: Vec<(&'static str, String)>,
}

impl Regression {
    /// Anything that worked before and fails now: a new problem, or a pack
    /// that is no longer there at all.
    pub fn is_regression(&self) -> bool {
        !self.new_problems.is_empty() || !self.removed_packs.is_empty()
    }
}

pub fn snapshot(root: &Path, catalog: &VanillaCatalog) -> Snapshot {
    let rps = resource_packs_in(root);
    let single = is_resource_pack(root) || rps.is_empty();
    let roots: Vec<PathBuf> = if single {
        vec![root.to_path_buf()]
    } else {
        rps
    };

    let key_of = |dir: &Path| -> String {
        match read_manifest(dir) {
            Some(m) if !m.uuid.is_empty() => m.uuid.to_ascii_lowercase(),
            _ => format!("folder:{}", folder_name(dir)),
        }
    };
    let mut packs = BTreeMap::new();
    if single {
        if read_manifest(root).is_some() {
            packs.insert(key_of(root), folder_name(root));
        }
    } else {
        for p in packs_in(root) {
            packs.insert(key_of(&p.root), p.folder);
        }
    }

    let mut problems = BTreeSet::new();
    let mut problem_packs = BTreeMap::new();
    for (rp, (_, r)) in roots.iter().zip(analyze_packs(&roots, catalog)) {
        let pack = key_of(rp);
        for i in &r.issues {
            let i = match i {
                // an unreadable file is named by its full path: keep only
                // the part inside the project
                Issue::Unreadable { file, why } => {
                    let rel = Path::new(file)
                        .strip_prefix(root)
                        .map(|p| p.to_string_lossy().replace('\\', "/"));
                    Issue::Unreadable {
                        file: rel.unwrap_or_else(|_| file.clone()),
                        why: why.clone(),
                    }
                }
                other => other.clone(),
            };
            let text = format!("{i:?}");
            problem_packs.insert(text.clone(), pack.clone());
            problems.insert(text);
        }
    }
    if !single {
        let game = if catalog.is_empty() {
            None
        } else {
            parse_version(&catalog.version)
        };
        for i in check_world(root, game).issues {
            if i.severity() == Severity::Error {
                problems.insert(format!("{i:?}"));
            }
        }
    }

    let mut defines: BTreeMap<&'static str, BTreeSet<String>> =
        KINDS.iter().map(|k| (*k, BTreeSet::new())).collect();
    for rp in &roots {
        let mut add = |kind: &'static str, id: String| {
            defines.entry(kind).or_default().insert(id);
        };
        for f in discover_entities(rp) {
            if let Some(d) = crate::snapshot::read_to_string(&f)
                .ok()
                .and_then(|s| parse_client_entity(&s).ok())
            {
                add("client entity", d.identifier);
            }
        }
        for f in discover_models(rp) {
            for m in read_json(&f).map(|r| parse_models(&r)).unwrap_or_default() {
                add("geometry", m.identifier);
            }
        }
        for f in discover_animation_files(rp) {
            let Some(root) = read_json(&f) else { continue };
            for (section, kind) in [
                ("animations", "animation"),
                ("animation_controllers", "animation controller"),
            ] {
                for k in root
                    .get(section)
                    .and_then(Json::as_object)
                    .into_iter()
                    .flat_map(|o| o.keys())
                {
                    add(kind, k.clone());
                }
            }
        }
        for f in discover_render_controllers(rp) {
            let Some(root) = read_json(&f) else { continue };
            for k in root
                .get("render_controllers")
                .and_then(Json::as_object)
                .into_iter()
                .flat_map(|o| o.keys())
            {
                add("render controller", k.clone());
            }
        }
        for dir in ["textures", "sounds"] {
            for f in all_files(&rp.join(dir)) {
                let Ok(rel) = f.strip_prefix(rp) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if is_asset(&rel.to_ascii_lowercase()) {
                    add("texture or sound", asset_stem(&rel));
                }
            }
        }
    }
    Snapshot {
        problems,
        defines,
        packs,
        problem_packs,
    }
}

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn read_json(f: &Path) -> Option<Json> {
    json::parse(&crate::snapshot::read_to_string(f).ok()?).ok()
}

pub fn compare(before: &Snapshot, after: &Snapshot) -> Regression {
    let diff =
        |a: &BTreeSet<String>, b: &BTreeSet<String>| a.difference(b).cloned().collect::<Vec<_>>();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let empty = BTreeSet::new();
    for kind in KINDS {
        let b = before.defines.get(kind).unwrap_or(&empty);
        let a = after.defines.get(kind).unwrap_or(&empty);
        removed.extend(b.difference(a).map(|id| (kind, id.clone())));
        added.extend(a.difference(b).map(|id| (kind, id.clone())));
    }
    let gone = |p: &String| {
        before
            .problem_packs
            .get(p)
            .is_some_and(|pack| before.packs.contains_key(pack) && !after.packs.contains_key(pack))
    };
    let (lost_problems, fixed_problems): (Vec<String>, Vec<String>) =
        diff(&before.problems, &after.problems)
            .into_iter()
            .partition(gone);
    let packs_only = |a: &Snapshot, b: &Snapshot| {
        a.packs
            .iter()
            .filter(|(k, _)| !b.packs.contains_key(*k))
            .map(|(_, folder)| folder.clone())
            .collect::<Vec<_>>()
    };
    Regression {
        new_problems: diff(&after.problems, &before.problems),
        fixed_problems,
        lost_problems,
        removed_packs: packs_only(before, after),
        added_packs: packs_only(after, before),
        removed,
        added,
    }
}
