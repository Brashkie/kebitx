//! Compatibility between the packs of a world.
//!
//! Every pack can be fine on its own and the world still broken: two packs
//! share a UUID, one needs a newer game, one depends on a pack that is not
//! there, or two packs ship different versions of something the game keeps
//! only one of. These checks need the whole world, behavior packs included.
//!
//! Two severities, kept apart on purpose:
//! - **Error**: the game refuses or drops something. Provable from the files.
//! - **Warning**: the game keeps one of two different versions, silently.
//!   Often intended (a compatibility pack overriding an addon), never a guess:
//!   it is only reported when the two versions actually DIFFER.

use crate::filesystem::{all_files, json_files};
use crate::json::{self, Json};
use crate::manifest::{packs_in, Dependency, Pack, PackKind};
use crate::nbt::{parse_level_dat, Nbt};
use crate::par::par_map;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatIssue {
    /// One UUID used by more than one header or module: the game loads one.
    DuplicateUuid { uuid: String, places: Vec<String> },
    /// The pack asks for a newer game than the installed one: it is refused.
    EngineTooNew {
        pack: String,
        needs: [u32; 3],
        game: [u32; 3],
    },
    /// A dependency on a pack UUID that no pack in the world has.
    MissingDependency { pack: String, uuid: String },
    /// `world_*_packs.json` activates a pack that is not in the world.
    MissingListedPack { list: String, uuid: String },
    /// A pack in the world that the world's pack lists do not activate.
    NotActivated { pack: String },
    /// A file the game does not merge, in several packs, with different content.
    ConflictingFile { path: String, packs: Vec<String> },
    /// One identifier defined differently in several places.
    ConflictingDefinition {
        kind: &'static str,
        id: String,
        places: Vec<String>,
    },
    /// A script-module version that is not `major.minor.patch[-tag]` (or the
    /// legacy `"beta"`): the game refuses the pack's scripts.
    InvalidModuleVersion {
        pack: String,
        module: String,
        version: String,
    },
    /// One pack depending on the same module twice, with different versions.
    ConflictingModuleVersions {
        pack: String,
        module: String,
        versions: Vec<String>,
    },
    /// A beta module (`1.12.0-beta`) in a world whose `level.dat` has the Beta
    /// APIs experiment off: the pack's scripts do not run.
    BetaApisOff {
        pack: String,
        module: String,
        version: String,
    },
    /// A beta module, and no `level.dat` to tell whether Beta APIs is on.
    NeedsBetaApis {
        pack: String,
        module: String,
        version: String,
    },
}

impl CompatIssue {
    pub fn severity(&self) -> Severity {
        match self {
            CompatIssue::DuplicateUuid { .. }
            | CompatIssue::EngineTooNew { .. }
            | CompatIssue::MissingDependency { .. }
            | CompatIssue::MissingListedPack { .. }
            | CompatIssue::InvalidModuleVersion { .. }
            | CompatIssue::ConflictingModuleVersions { .. }
            | CompatIssue::BetaApisOff { .. } => Severity::Error,
            CompatIssue::NotActivated { .. }
            | CompatIssue::ConflictingFile { .. }
            | CompatIssue::ConflictingDefinition { .. }
            | CompatIssue::NeedsBetaApis { .. } => Severity::Warning,
        }
    }
}

#[derive(Debug, Default)]
pub struct CompatReport {
    pub packs: usize,
    pub issues: Vec<CompatIssue>,
}

impl CompatReport {
    pub fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity() == Severity::Error)
            .count()
    }
    pub fn warnings(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity() == Severity::Warning)
            .count()
    }
}

/// Files the game takes from ONE pack instead of merging. If two packs ship
/// different versions, the other one is silently ignored.
const NOT_MERGED_RP: [&str; 4] = ["ui/", "materials/", "biomes_client.json", "sounds.json"];
const NOT_MERGED_BP: [&str; 2] = ["loot_tables/", "trading/"];

/// Check a world. `game` is the installed version, if known; without it the
/// engine-version check is skipped rather than guessed.
pub fn check_world(world: &Path, game: Option<[u32; 3]>) -> CompatReport {
    let packs = packs_in(world);
    let mut report = CompatReport {
        packs: packs.len(),
        ..Default::default()
    };
    duplicate_uuids(&packs, &mut report);
    if let Some(game) = game {
        engine_versions(&packs, game, &mut report);
    }
    dependencies(&packs, &mut report);
    script_modules(world, &packs, &mut report);
    world_lists(world, &packs, &mut report);
    conflicting_files(&packs, &mut report);
    conflicting_definitions(&packs, &mut report);
    report
}

fn duplicate_uuids(packs: &[Pack], report: &mut CompatReport) {
    let mut seen: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for p in packs {
        seen.entry(&p.manifest.uuid)
            .or_default()
            .push(format!("{} (header)", p.folder));
        for m in &p.manifest.module_uuids {
            seen.entry(m)
                .or_default()
                .push(format!("{} (module)", p.folder));
        }
    }
    for (uuid, places) in seen {
        if places.len() > 1 {
            report.issues.push(CompatIssue::DuplicateUuid {
                uuid: uuid.to_string(),
                places,
            });
        }
    }
}

fn engine_versions(packs: &[Pack], game: [u32; 3], report: &mut CompatReport) {
    for p in packs {
        if let Some(needs) = p.manifest.min_engine_version {
            if needs > game {
                report.issues.push(CompatIssue::EngineTooNew {
                    pack: p.folder.clone(),
                    needs,
                    game,
                });
            }
        }
    }
}

fn dependencies(packs: &[Pack], report: &mut CompatReport) {
    let present: HashSet<&str> = packs.iter().map(|p| p.manifest.uuid.as_str()).collect();
    for p in packs {
        for d in &p.manifest.dependencies {
            // Script modules (`@minecraft/server`) are provided by the game.
            if let Dependency::Pack { uuid } = d {
                if !present.contains(uuid.as_str()) {
                    report.issues.push(CompatIssue::MissingDependency {
                        pack: p.folder.clone(),
                        uuid: uuid.clone(),
                    });
                }
            }
        }
    }
}

/// Script-module dependencies. Whether the INSTALLED game supports a given
/// stable version cannot be proved: the game ships no list of its module
/// versions, and its executable is not readable. A table per game version
/// would go stale with every update. So only what the files prove is checked.
fn script_modules(world: &Path, packs: &[Pack], report: &mut CompatReport) {
    // `None`: no level.dat (a build folder), so the experiment is unknown.
    let beta_apis: Option<bool> = crate::filesystem::read_limited(
        &world.join("level.dat"),
        crate::filesystem::MAX_TEXT_BYTES,
    )
    .ok()
    .and_then(|b| parse_level_dat(&b).ok())
    .map(|root| {
        root.get("experiments")
            .and_then(|e| e.get("gametest"))
            .and_then(Nbt::as_i64)
            .is_some_and(|v| v != 0)
    });
    for p in packs {
        let mut seen: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for d in &p.manifest.dependencies {
            let Dependency::Module { name, version } = d else {
                continue;
            };
            let version = version.clone().unwrap_or_default();
            let entry = seen.entry(name).or_default();
            if !entry.contains(&version) {
                entry.push(version.clone());
            }
            if !valid_module_version(&version) {
                report.issues.push(CompatIssue::InvalidModuleVersion {
                    pack: p.folder.clone(),
                    module: name.clone(),
                    version,
                });
                continue;
            }
            if version.ends_with("-beta") || version == "beta" {
                let (pack, module) = (p.folder.clone(), name.clone());
                match beta_apis {
                    Some(true) => {}
                    Some(false) => report.issues.push(CompatIssue::BetaApisOff {
                        pack,
                        module,
                        version,
                    }),
                    None => report.issues.push(CompatIssue::NeedsBetaApis {
                        pack,
                        module,
                        version,
                    }),
                }
            }
        }
        for (module, versions) in seen {
            if versions.len() > 1 {
                report.issues.push(CompatIssue::ConflictingModuleVersions {
                    pack: p.folder.clone(),
                    module: module.to_string(),
                    versions,
                });
            }
        }
    }
}

/// `1.19.0`, `1.12.0-beta`, `2.0.0-rc`, or the legacy bare `beta`.
fn valid_module_version(v: &str) -> bool {
    if v == "beta" {
        return true;
    }
    let (core, tag) = match v.split_once('-') {
        Some((c, t)) => (c, Some(t)),
        None => (v, None),
    };
    let nums: Vec<&str> = core.split('.').collect();
    nums.len() == 3
        && nums
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && tag.is_none_or(|t| {
            !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        })
}

/// Every script module the world's packs use: module -> version -> packs.
/// Information: the reader can compare it with what their game supports.
pub fn module_inventory(world: &Path) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut out: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for p in packs_in(world) {
        for d in &p.manifest.dependencies {
            if let Dependency::Module { name, version } = d {
                out.entry(name.clone())
                    .or_default()
                    .entry(version.clone().unwrap_or_default())
                    .or_default()
                    .push(p.folder.clone());
            }
        }
    }
    out
}

/// `world_behavior_packs.json` / `world_resource_packs.json`: what the world
/// actually activates. Absent in a build folder; then nothing is checked.
fn world_lists(world: &Path, packs: &[Pack], report: &mut CompatReport) {
    let mut any_list = false;
    let mut listed: HashSet<String> = HashSet::new();
    for list in ["world_behavior_packs.json", "world_resource_packs.json"] {
        let Ok(src) = crate::snapshot::read_to_string(world.join(list)) else {
            continue;
        };
        let Ok(root) = json::parse(&src) else {
            continue;
        };
        any_list = true;
        for entry in root.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let Some(id) = entry.get("pack_id").and_then(Json::as_str) else {
                continue;
            };
            let id = id.trim().to_ascii_lowercase();
            if !packs.iter().any(|p| p.manifest.uuid == id) {
                report.issues.push(CompatIssue::MissingListedPack {
                    list: list.to_string(),
                    uuid: id.clone(),
                });
            }
            listed.insert(id);
        }
    }
    if !any_list {
        return;
    }
    for p in packs {
        let activatable = matches!(p.manifest.kind, PackKind::Resources | PackKind::Data);
        if activatable && !listed.contains(&p.manifest.uuid) {
            report.issues.push(CompatIssue::NotActivated {
                pack: p.folder.clone(),
            });
        }
    }
}

fn conflicting_files(packs: &[Pack], report: &mut CompatReport) {
    // relative path (lowercased) -> [(pack, content fingerprint)]
    let mut files: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    for p in packs {
        let watched: &[&str] = match p.manifest.kind {
            PackKind::Resources => &NOT_MERGED_RP,
            PackKind::Data => &NOT_MERGED_BP,
            _ => continue,
        };
        for w in watched {
            let candidates = if w.ends_with('/') {
                all_files(&p.root.join(w))
            } else {
                vec![p.root.join(w)]
            };
            for f in candidates {
                let Ok(bytes) = crate::snapshot::read(&f) else {
                    continue;
                };
                let Ok(rel) = f.strip_prefix(&p.root) else {
                    continue;
                };
                let rel = rel
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase();
                files
                    .entry(rel)
                    .or_default()
                    .push((p.folder.clone(), fingerprint_bytes(&bytes)));
            }
        }
    }
    for (path, owners) in files {
        if owners.len() > 1 && owners.iter().any(|(_, f)| *f != owners[0].1) {
            report.issues.push(CompatIssue::ConflictingFile {
                path,
                packs: owners.into_iter().map(|(p, _)| p).collect(),
            });
        }
    }
}

/// What to extract from a definition file.
#[derive(Clone, Copy)]
enum Role {
    /// The body under this key, as this kind (`minecraft:entity`,
    /// `minecraft:client_entity`, ...).
    Body(&'static str, &'static str),
    Models,
    /// Animation, animation-controller and render-controller files.
    Keyed,
}

/// `(kind, identifier, fingerprint)` of every definition in one file.
fn definitions_in(f: &Path, role: Role) -> Vec<(&'static str, String, u64)> {
    let Some(root) = read_json(f) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match role {
        Role::Body(key, kind) => {
            if let Some(body) = root.get(key) {
                if let Some(id) = identifier(body) {
                    out.push((kind, id.to_string(), fingerprint(body)));
                }
            }
        }
        Role::Models => {
            for g in root
                .get("minecraft:geometry")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if let Some(id) = identifier(g) {
                    out.push(("geometry", id.to_string(), fingerprint(g)));
                }
            }
            for (k, body) in root.as_object().into_iter().flatten() {
                if k.starts_with("geometry.") {
                    out.push((
                        "geometry",
                        k.split(':').next().unwrap_or(k).to_string(),
                        fingerprint(body),
                    ));
                }
            }
        }
        Role::Keyed => {
            for (section, kind) in [
                ("animations", "animation"),
                ("animation_controllers", "animation controller"),
                ("render_controllers", "render controller"),
            ] {
                for (id, body) in root
                    .get(section)
                    .and_then(Json::as_object)
                    .into_iter()
                    .flatten()
                {
                    out.push((kind, id.clone(), fingerprint(body)));
                }
            }
        }
    }
    out
}

fn conflicting_definitions(packs: &[Pack], report: &mut CompatReport) {
    // every definition file, in a fixed order (pack, folder, file)
    let mut files: Vec<(&Pack, PathBuf, Role)> = Vec::new();
    for p in packs {
        let mut add = |dir: &str, role: Role| {
            for f in json_files(&p.root.join(dir)) {
                files.push((p, f, role));
            }
        };
        match p.manifest.kind {
            PackKind::Data => {
                add("entities", Role::Body("minecraft:entity", "entity"));
                add("items", Role::Body("minecraft:item", "item"));
                add("blocks", Role::Body("minecraft:block", "block"));
            }
            PackKind::Resources => {
                add(
                    "entity",
                    Role::Body("minecraft:client_entity", "client entity"),
                );
                add("models", Role::Models);
                add("animations", Role::Keyed);
                add("animation_controllers", Role::Keyed);
                add("render_controllers", Role::Keyed);
            }
            _ => {}
        }
    }
    // parse and fingerprint on every core; merged in file order, as before
    let found = par_map(&files, |(_, f, role)| definitions_in(f, *role));

    // (kind, id) -> [(place, body fingerprint)]
    let mut defs: BTreeMap<(&'static str, String), Vec<(String, u64)>> = BTreeMap::new();
    for ((p, f, _), list) in files.iter().zip(found) {
        let rel = f
            .strip_prefix(&p.root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        for (kind, id, fp) in list {
            defs.entry((kind, id))
                .or_default()
                .push((format!("{}/{rel}", p.folder), fp));
        }
    }
    for ((kind, id), places) in defs {
        if places.len() > 1 && places.iter().any(|(_, f)| *f != places[0].1) {
            report.issues.push(CompatIssue::ConflictingDefinition {
                kind,
                id,
                places: places.into_iter().map(|(p, _)| p).collect(),
            });
        }
    }
}

fn read_json(f: &Path) -> Option<Json> {
    json::parse(&crate::snapshot::read_to_string(f).ok()?).ok()
}

fn identifier(body: &Json) -> Option<&str> {
    body.get("description")?.get("identifier")?.as_str()
}

fn fingerprint_bytes(b: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

/// Structural fingerprint: two bodies that differ only in formatting,
/// comments or key order are the same definition.
fn fingerprint(v: &Json) -> u64 {
    let mut h = DefaultHasher::new();
    hash_json(v, &mut h);
    h.finish()
}

fn hash_json(v: &Json, h: &mut DefaultHasher) {
    match v {
        Json::Null => 0u8.hash(h),
        Json::Bool(b) => (1u8, b).hash(h),
        Json::Num(n) => (2u8, n.to_bits()).hash(h),
        Json::Str(s) => (3u8, s).hash(h),
        Json::Arr(a) => {
            (4u8, a.len()).hash(h);
            a.iter().for_each(|x| hash_json(x, h));
        }
        // BTreeMap: keys already sorted, so key order in the file is irrelevant
        Json::Obj(m) => {
            (5u8, m.len()).hash(h);
            for (k, x) in m {
                k.hash(h);
                hash_json(x, h);
            }
        }
    }
}

/// Group a report's issues for display: kind name -> count.
pub fn summary(report: &CompatReport) -> HashMap<&'static str, usize> {
    let mut out = HashMap::new();
    for i in &report.issues {
        let k = match i {
            CompatIssue::DuplicateUuid { .. } => "duplicate uuid",
            CompatIssue::EngineTooNew { .. } => "needs newer game",
            CompatIssue::MissingDependency { .. } => "missing dependency",
            CompatIssue::MissingListedPack { .. } => "listed pack missing",
            CompatIssue::NotActivated { .. } => "pack not activated",
            CompatIssue::ConflictingFile { .. } => "conflicting file",
            CompatIssue::ConflictingDefinition { .. } => "conflicting definition",
            CompatIssue::InvalidModuleVersion { .. } => "invalid module version",
            CompatIssue::ConflictingModuleVersions { .. } => "module required twice",
            CompatIssue::BetaApisOff { .. } => "beta module, Beta APIs off",
            CompatIssue::NeedsBetaApis { .. } => "beta module",
        };
        *out.entry(k).or_insert(0) += 1;
    }
    out
}
