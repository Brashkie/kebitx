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
use crate::meta::{meta_of, ModMeta};
use crate::source::Source;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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

/// The jars of a folder, opened, with what each declares.
pub struct ModFolder {
    sources: Vec<(String, Option<Source>)>,
    metas: Vec<Option<ModMeta>>,
}

/// Analyses every jar of `dir` against `jar` (the game) and each other.
pub fn analyze_set(dir: &Path, jar: Option<&JavaJar>) -> SetReport {
    ModFolder::open(dir).analyze(jar)
}

impl ModFolder {
    pub fn open(dir: &Path) -> Self {
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
        let metas = sources
            .iter()
            .map(|(_, s)| s.as_ref().and_then(meta_of))
            .collect();
        Self { sources, metas }
    }

    /// The installed version every mod accepts, newest first (stable versions
    /// before snapshots); `exact` is false when none does and the newest
    /// stable one stands in.
    pub fn game_for(&self, java: &crate::game::JavaAssets) -> Option<JavaJar> {
        let accepts = |id: &str| {
            self.metas.iter().flatten().all(|m| {
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

    /// Every jar against `jar` (the game) and each other.
    pub fn analyze(&self, jar: Option<&JavaJar>) -> SetReport {
        analyze_folder(&self.sources, &self.metas, jar)
    }
}

fn analyze_folder(
    sources: &[(String, Option<Source>)],
    metas: &[Option<ModMeta>],
    jar: Option<&JavaJar>,
) -> SetReport {
    // what loads together: every mod's classes and the ids it provides
    let index = jar.map(GameIndex::from_jar);
    let mut ctx = Context::default();
    for ((_, s), m) in sources.iter().zip(metas) {
        if let (Some(s), Some(m)) = (s, m) {
            let mut ids = vec![m.id.clone()];
            ids.extend(m.provides.iter().cloned());
            ids.extend(m.bundled.iter().map(|(id, _)| id.clone()));
            ctx.add_mod(s, &s.files(), &ids);
        }
    }

    let mut findings = BTreeSet::new();
    // id -> (version, jar) of everything the set provides
    let mut provided: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut top: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ((name, _), m) in sources.iter().zip(metas) {
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
    for ((name, _), m) in sources.iter().zip(metas) {
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
                meta: meta.clone(),
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
