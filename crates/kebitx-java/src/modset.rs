//! A set of mods, as the loader sees a `mods/` folder: each mod analysed
//! against the game and the others, and what the loader itself refuses.
//!
//! What the loaders refuse to start with, and is checked here:
//!   - two mods with the same id (or one providing another's id);
//!   - a required dependency no mod of the set provides (jars bundled in
//!     a mod count: the loader loads them as mods);
//!   - a dependency, `minecraft` included, whose version does not satisfy
//!     the requirement (Fabric predicates, Maven ranges for Forge and
//!     NeoForge);
//!   - a mod another declares it breaks (Fabric `breaks`, NeoForge
//!     `incompatible`), in a version the rule names.
//!
//! The loaders' own versions (`fabricloader`, `neoforge`, ...) and Java are
//! not in the folder: those requirements are not judged.
//!
//! Two Mixins that `@Overwrite` the same method of the game is information:
//! Mixin keeps one (by priority) and logs the other as a conflict, so one
//! mod silently loses its change.

use crate::analyze::{analyze_in, JavaReport};
use crate::code::{mixin_targets, Context, GameIndex};
use crate::game::JavaJar;
use crate::source::Source;
use crate::version::{fabric_any, maven_matches};
use kebitx_core::json::{self, Json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A version requirement, in the loader's own syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Req {
    /// Fabric / Quilt predicates, any of them.
    Fabric(Vec<String>),
    /// A Maven range (Forge, NeoForge).
    Maven(String),
}

impl Req {
    pub fn matches(&self, version: &str) -> Option<bool> {
        match self {
            Req::Fabric(p) => fabric_any(p, version),
            Req::Maven(r) => maven_matches(r, version),
        }
    }
}

impl std::fmt::Display for Req {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Req::Fabric(p) => write!(f, "{}", p.join(" || ")),
            Req::Maven(r) => write!(f, "{r}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub id: String,
    pub req: Req,
}

/// What a mod's metadata declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModMeta {
    pub loader: &'static str,
    pub id: String,
    pub version: String,
    pub provides: Vec<String>,
    pub depends: Vec<Dep>,
    pub breaks: Vec<Dep>,
    /// `(id, version)` of the mods bundled inside it.
    pub bundled: Vec<(String, String)>,
}

fn predicates(v: &Json) -> Vec<String> {
    match v {
        Json::Str(s) => vec![s.clone()],
        Json::Arr(a) => a
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn fabric_deps(j: &Json, key: &str) -> Vec<Dep> {
    j.get(key)
        .and_then(Json::as_object)
        .map(|o| {
            o.iter()
                .map(|(id, v)| Dep {
                    id: id.clone(),
                    req: Req::Fabric(predicates(v)),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `fabric.mod.json`: id, version, provides, depends, breaks.
fn fabric_meta(j: &Json) -> Option<ModMeta> {
    Some(ModMeta {
        loader: "Fabric",
        id: j.get("id")?.as_str()?.to_string(),
        version: j
            .get("version")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        provides: j
            .get("provides")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        depends: fabric_deps(j, "depends"),
        breaks: fabric_deps(j, "breaks"),
        bundled: Vec::new(),
    })
}

/// The tables of a `mods.toml`: `(header, key -> value)`, values unquoted.
fn toml_tables(t: &str) -> Vec<(String, BTreeMap<String, String>)> {
    let mut out: Vec<(String, BTreeMap<String, String>)> = vec![(String::new(), BTreeMap::new())];
    for l in t.lines().map(str::trim) {
        if l.starts_with('[') {
            let head = l.split('#').next().unwrap_or("").trim();
            out.push((
                head.trim_start_matches('[')
                    .trim_end_matches(']')
                    .trim()
                    .to_string(),
                BTreeMap::new(),
            ));
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim();
            // a quoted value ends at its quote; otherwise at a comment
            let v = if let Some(q) = v.strip_prefix('"') {
                q.split('"').next().unwrap_or("")
            } else {
                v.split('#').next().unwrap_or("").trim()
            };
            if let Some(last) = out.last_mut() {
                last.1.insert(k.trim().to_string(), v.to_string());
            }
        }
    }
    out
}

/// `META-INF/(neoforge.)mods.toml`: the first `[[mods]]`, and its
/// `[[dependencies.<id>]]` (`type = "required"` / `"incompatible"` on
/// NeoForge, `mandatory = true` on Forge).
fn toml_meta(t: &str, loader: &'static str, manifest_version: Option<String>) -> Option<ModMeta> {
    let tables = toml_tables(t);
    let (_, m) = tables.iter().find(|(h, _)| h == "mods")?;
    let mut version = m.get("version").cloned().unwrap_or_default();
    if version.contains("${") {
        version = manifest_version.unwrap_or_default();
    }
    let mut meta = ModMeta {
        loader,
        id: m.get("modId")?.clone(),
        version,
        ..Default::default()
    };
    for (h, kv) in &tables {
        if !h.starts_with("dependencies") {
            continue;
        }
        let (Some(id), range) = (kv.get("modId"), kv.get("versionRange")) else {
            continue;
        };
        let dep = Dep {
            id: id.clone(),
            req: Req::Maven(range.cloned().unwrap_or_default()),
        };
        let kind = kv.get("type").map(|s| s.to_ascii_lowercase());
        let required = match kind.as_deref() {
            Some("required") => true,
            Some(_) => false,
            None => kv.get("mandatory").is_some_and(|v| v == "true"),
        };
        if required {
            meta.depends.push(dep);
        } else if kind.as_deref() == Some("incompatible") {
            meta.breaks.push(dep);
        }
    }
    Some(meta)
}

/// `Implementation-Version` of a jar's manifest (what `${file.jarVersion}`
/// stands for).
fn manifest_version(src: &Source) -> Option<String> {
    let m = String::from_utf8_lossy(&src.read("META-INF/MANIFEST.MF")?).to_string();
    m.lines()
        .find_map(|l| l.strip_prefix("Implementation-Version:"))
        .map(|v| v.trim().to_string())
}

/// Bundled jars nest: this deep is plenty.
const MAX_NESTING: usize = 3;

/// `(id, version)` of every Fabric mod bundled in `src` (and in them).
fn bundled_mods(src: &Source, depth: usize, out: &mut Vec<(String, String)>) {
    if depth >= MAX_NESTING {
        return;
    }
    let files = src.files();
    for jar in files
        .iter()
        .filter(|f| f.starts_with("META-INF/jars/") && f.ends_with(".jar"))
    {
        let Some(inner) = src
            .read(jar)
            .and_then(|b| kebitx_core::codec::zip::ZipArchive::from_bytes(b).ok())
            .map(|z| Source::Zip(Box::new(z)))
        else {
            continue;
        };
        if let Some(m) = inner
            .read("fabric.mod.json")
            .and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok())
            .as_ref()
            .and_then(fabric_meta)
        {
            out.push((m.id.clone(), m.version.clone()));
            out.extend(m.provides.iter().map(|p| (p.clone(), m.version.clone())));
        }
        bundled_mods(&inner, depth + 1, out);
    }
}

/// A mod's metadata, from whichever loader's file it has.
pub fn meta_of(src: &Source) -> Option<ModMeta> {
    let json_of = |name: &str| {
        src.read(name)
            .and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok())
    };
    let mut meta = if let Some(j) = json_of("fabric.mod.json") {
        fabric_meta(&j)?
    } else if let Some(j) = json_of("quilt.mod.json") {
        let q = j.get("quilt_loader")?;
        let deps = |key: &str| -> Vec<Dep> {
            q.get(key)
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(|d| match d {
                    Json::Str(id) => Some(Dep {
                        id: id.clone(),
                        req: Req::Fabric(vec!["*".into()]),
                    }),
                    o => Some(Dep {
                        id: o.get("id")?.as_str()?.to_string(),
                        req: Req::Fabric(
                            o.get("versions")
                                .map(predicates)
                                .unwrap_or_else(|| vec!["*".into()]),
                        ),
                    }),
                })
                .collect()
        };
        ModMeta {
            loader: "Quilt",
            id: q.get("id")?.as_str()?.to_string(),
            version: q
                .get("version")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            depends: deps("depends"),
            breaks: deps("breaks"),
            ..Default::default()
        }
    } else {
        let mut found = None;
        for (file, loader) in [
            ("META-INF/neoforge.mods.toml", "NeoForge"),
            ("META-INF/mods.toml", "Forge"),
        ] {
            if let Some(b) = src.read(file) {
                found = toml_meta(&String::from_utf8_lossy(&b), loader, manifest_version(src));
                break;
            }
        }
        found?
    };
    bundled_mods(src, 0, &mut meta.bundled);
    Some(meta)
}

/// Requirements on these are not judged: the loaders and Java are not in
/// the mods folder (Fabric Loader also ships MixinExtras).
const NOT_IN_FOLDER: [&str; 8] = [
    "fabricloader",
    "fabric-loader",
    "quilt_loader",
    "java",
    "neoforge",
    "forge",
    "fml",
    "mixinextras",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SetFinding {
    /// The same mod id in several jars: the loader refuses to start.
    Duplicate { id: String, jars: Vec<String> },
    /// A required dependency no mod of the set provides.
    Missing {
        jar: String,
        id: String,
        dependency: String,
        wanted: String,
    },
    /// A dependency present in a version the requirement does not accept.
    WrongVersion {
        jar: String,
        id: String,
        dependency: String,
        wanted: String,
        found: String,
    },
    /// A mod this one declares it breaks, in a version the rule names.
    Breaks {
        jar: String,
        id: String,
        other: String,
        rule: String,
        found: String,
    },
    /// Information: Mixins of several mods `@Overwrite` the same method;
    /// only one change survives.
    OverwriteConflict {
        target: String,
        method: String,
        mods: Vec<String>,
    },
    /// Information: mods for several loaders in one folder; each loader
    /// loads only its own.
    MixedLoaders { loaders: Vec<String> },
    /// Information: a jar with no mod metadata (a library, or not a mod).
    NotAMod { jar: String },
}

impl SetFinding {
    pub fn is_info(&self) -> bool {
        matches!(
            self,
            Self::OverwriteConflict { .. } | Self::MixedLoaders { .. } | Self::NotAMod { .. }
        )
    }
}

pub struct SetMod {
    pub jar: String,
    pub meta: Option<ModMeta>,
    pub report: JavaReport,
}

pub struct SetReport {
    pub game: Option<(String, bool)>,
    pub mods: Vec<SetMod>,
    pub findings: Vec<SetFinding>,
}

impl SetReport {
    pub fn broken(&self) -> usize {
        self.findings.iter().filter(|f| !f.is_info()).count()
            + self.mods.iter().map(|m| m.report.broken()).sum::<usize>()
    }
}

/// The `.jar` files of a folder (not recursive: the loaders do not look
/// further either).
pub fn jars_in(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")))
        .collect();
    out.sort();
    out
}

/// The installed version every mod accepts, newest first (stable versions
/// before snapshots); `exact` is false when none does and the newest
/// stable one stands in.
pub fn game_for(java: &crate::game::JavaAssets, metas: &[&ModMeta]) -> Option<JavaJar> {
    let accepts = |id: &str| {
        metas.iter().all(|m| {
            m.depends
                .iter()
                .filter(|d| d.id == "minecraft")
                .all(|d| d.req.matches(id) != Some(false))
        })
    };
    let installed = java.installed();
    let pick = installed
        .iter()
        .filter(|(_, stable)| *stable)
        .chain(installed.iter().filter(|(_, stable)| !*stable))
        .find(|(id, _)| accepts(id));
    match pick {
        Some((id, _)) => java.jar_by_id(id, true),
        None => java.jar_for(None),
    }
}

/// Analyses every jar of `dir` against `jar` (the game) and each other.
pub fn analyze_set(dir: &Path, jar: Option<&JavaJar>) -> SetReport {
    let sources: Vec<(String, Option<Source>)> = jars_in(dir)
        .into_iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            (name, Source::open(&p).ok())
        })
        .collect();
    let metas: Vec<Option<ModMeta>> = sources
        .iter()
        .map(|(_, s)| s.as_ref().and_then(meta_of))
        .collect();

    // what loads together (every mod's classes and the ids it provides), and
    // the game's class index: independent, so built at the same time
    let (ctx, index) = std::thread::scope(|scope| {
        let index = scope.spawn(|| jar.map(GameIndex::from_jar));
        let mut ctx = Context::default();
        for ((_, s), m) in sources.iter().zip(&metas) {
            if let (Some(s), Some(m)) = (s, m) {
                let mut ids = vec![m.id.clone()];
                ids.extend(m.provides.iter().cloned());
                ids.extend(m.bundled.iter().map(|(id, _)| id.clone()));
                ctx.add_mod(s, &s.files(), &ids);
            }
        }
        // a panic in the index thread is a bug: let it surface as one
        let index = index
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e));
        (ctx, index)
    });

    let mut findings = BTreeSet::new();
    // id -> (version, jar) of everything the set provides
    let mut provided: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut top: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ((name, _), m) in sources.iter().zip(&metas) {
        let Some(m) = m else {
            findings.insert(SetFinding::NotAMod { jar: name.clone() });
            continue;
        };
        for id in std::iter::once(&m.id).chain(&m.provides) {
            top.entry(id.clone()).or_default().push(name.clone());
            provided
                .entry(id.clone())
                .or_default()
                .push((m.version.clone(), name.clone()));
        }
        for (id, v) in &m.bundled {
            provided
                .entry(id.clone())
                .or_default()
                .push((v.clone(), name.clone()));
        }
    }
    for (id, jars) in &top {
        if jars.len() > 1 {
            findings.insert(SetFinding::Duplicate {
                id: id.clone(),
                jars: jars.clone(),
            });
        }
    }
    let loaders: BTreeSet<&str> = metas.iter().flatten().map(|m| m.loader).collect();
    if loaders.len() > 1 {
        findings.insert(SetFinding::MixedLoaders {
            loaders: loaders.iter().map(|l| l.to_string()).collect(),
        });
    }

    let game_version = jar.map(|j| j.id.clone());
    for ((name, _), m) in sources.iter().zip(&metas) {
        let Some(m) = m else { continue };
        for d in &m.depends {
            if NOT_IN_FOLDER.contains(&d.id.as_str()) {
                continue;
            }
            let found: Vec<String> = if d.id == "minecraft" {
                game_version.iter().cloned().collect()
            } else {
                provided
                    .get(&d.id)
                    .map(|v| v.iter().map(|(ver, _)| ver.clone()).collect())
                    .unwrap_or_default()
            };
            if found.is_empty() {
                if d.id != "minecraft" {
                    findings.insert(SetFinding::Missing {
                        jar: name.clone(),
                        id: m.id.clone(),
                        dependency: d.id.clone(),
                        wanted: d.req.to_string(),
                    });
                }
                continue;
            }
            let results: Vec<Option<bool>> = found.iter().map(|v| d.req.matches(v)).collect();
            if results.iter().all(|r| *r == Some(false)) {
                findings.insert(SetFinding::WrongVersion {
                    jar: name.clone(),
                    id: m.id.clone(),
                    dependency: d.id.clone(),
                    wanted: d.req.to_string(),
                    found: found.join(", "),
                });
            }
        }
        for b in &m.breaks {
            for (v, other_jar) in provided.get(&b.id).into_iter().flatten() {
                if other_jar != name && b.req.matches(v) == Some(true) {
                    findings.insert(SetFinding::Breaks {
                        jar: name.clone(),
                        id: m.id.clone(),
                        other: b.id.clone(),
                        rule: b.req.to_string(),
                        found: format!("{v} in {other_jar}"),
                    });
                }
            }
        }
    }

    // Mixins of several mods overwriting the same method of the game (each
    // mod's own classes, already read for the context)
    let mut overwrites: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for (mod_id, range) in &ctx.own {
        for c in &ctx.classes[range.clone()] {
            let Some(targets) = mixin_targets(c) else {
                continue;
            };
            for need in crate::mixin::needs(c) {
                if let crate::mixin::Need::Member {
                    how: "@Overwrite",
                    names,
                    descriptor,
                    ..
                } = need
                {
                    for t in &targets {
                        overwrites
                            .entry((t.replace('/', "."), format!("{}{descriptor}", names[0])))
                            .or_default()
                            .insert(mod_id.clone());
                    }
                }
            }
        }
    }
    for ((target, method), mods) in overwrites {
        if mods.len() > 1 {
            findings.insert(SetFinding::OverwriteConflict {
                target,
                method,
                mods: mods.into_iter().collect(),
            });
        }
    }

    let mods = sources
        .iter()
        .zip(metas)
        .filter_map(|((name, s), meta)| {
            let s = s.as_ref()?;
            Some(SetMod {
                jar: name.clone(),
                meta,
                report: analyze_in(s, jar, index.as_ref(), &ctx),
            })
        })
        .collect();
    SetReport {
        game: jar.map(|j| (j.id.clone(), j.exact)),
        mods,
        findings: findings.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_metadata_with_its_dependency_kinds() {
        let t = r#"
modLoader="javafml"
[[mods]] # the mod
modId="farmersdelight"
version="${file.jarVersion}"
[[dependencies.farmersdelight]]
    modId="neoforge"
    type="required"
    versionRange="[21.1.219,)"
[[dependencies.farmersdelight]]
    modId = "minecraft"
    type = "required"
    versionRange = "[1.21.1]"
[[dependencies.farmersdelight]]
    modId="crafttweaker"
    type="optional"
    versionRange="[14.0.6,)"
[[dependencies.farmersdelight]]
    modId="badmod"
    type="incompatible"
    versionRange="[1.0,)"
"#;
        let m = toml_meta(t, "NeoForge", Some("1.21.1-1.3.4".into())).unwrap();
        assert_eq!(
            (m.id.as_str(), m.version.as_str()),
            ("farmersdelight", "1.21.1-1.3.4")
        );
        let ids: Vec<&str> = m.depends.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["neoforge", "minecraft"]);
        assert_eq!(m.breaks[0].id, "badmod");
        assert_eq!(m.depends[1].req.matches("1.21.1"), Some(true));
        assert_eq!(m.depends[1].req.matches("26.3"), Some(false));
    }
}
