//! Repairs for a `mods/` folder, with the same rules as Bedrock's: a preview
//! first, a backup outside the folder, a second analysis that decides, and
//! `kebitx repair --undo` to put everything back (the backup has the same
//! format, so the same command restores it).
//!
//! The one change a repair makes is to take a jar out of the folder:
//!   - `duplicates` (the default): of two jars with the same mod id, which
//!     the loader refuses, the highest version the game accepts stays; the
//!     others go. When that cannot be decided — versions that do not read,
//!     a tie, none the game accepts — nothing is proposed for that id;
//!   - `incompatible` (only when asked): mods whose `minecraft` requirement
//!     the game's version does not satisfy.
//!
//! The result is kept only if it is strictly better: every problem left
//! was there before, and at least one is gone. Taking out a mod another one
//! needs is therefore undone on its own.

use crate::game::JavaJar;
use crate::modset::{analyze_set, ModMeta, SetFinding, SetReport};
use crate::version::Version;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    Duplicates,
    Incompatible,
}

/// One jar to take out of the folder, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub jar: String,
    pub reason: String,
}

fn accepts_game(m: &ModMeta, game: Option<&str>) -> Option<bool> {
    let game = game?;
    let mut all = true;
    for d in m.depends.iter().filter(|d| d.id == "minecraft") {
        all &= d.req.matches(game)?;
    }
    Some(all)
}

/// What the fixes would take out of the folder. Nothing is written.
pub fn plan(report: &SetReport, fixes: &[Fix]) -> Vec<Removal> {
    let game = report.game.as_ref().map(|(id, _)| id.as_str());
    let meta_of = |jar: &str| {
        report
            .mods
            .iter()
            .find(|m| m.jar == jar)
            .and_then(|m| m.meta.as_ref())
    };
    let mut out: Vec<Removal> = Vec::new();
    let mut taken = BTreeSet::new();
    if fixes.contains(&Fix::Duplicates) {
        for f in &report.findings {
            let SetFinding::Duplicate { id, jars } = f else {
                continue;
            };
            // candidates: the copies the game accepts (or that do not say)
            let candidates: Vec<(&String, Version)> = jars
                .iter()
                .filter_map(|j| {
                    let m = meta_of(j)?;
                    if accepts_game(m, game) == Some(false) {
                        return None;
                    }
                    Some((j, Version::parse(&m.version)?))
                })
                .collect();
            let Some(best) = candidates.iter().map(|(_, v)| v).max() else {
                continue;
            };
            let keepers: Vec<&&String> = candidates
                .iter()
                .filter(|(_, v)| v == best)
                .map(|(j, _)| j)
                .collect();
            if keepers.len() != 1 {
                continue;
            }
            let keep = keepers[0];
            for j in jars.iter().filter(|j| *j != *keep) {
                if taken.insert(j.clone()) {
                    out.push(Removal {
                        jar: j.clone(),
                        reason: format!("a second copy of {id}; {keep} stays"),
                    });
                }
            }
        }
    }
    if fixes.contains(&Fix::Incompatible) {
        for m in &report.mods {
            let Some(meta) = &m.meta else { continue };
            if accepts_game(meta, game) == Some(false) && taken.insert(m.jar.clone()) {
                let wanted: Vec<String> = meta
                    .depends
                    .iter()
                    .filter(|d| d.id == "minecraft")
                    .map(|d| d.req.to_string())
                    .collect();
                out.push(Removal {
                    jar: m.jar.clone(),
                    reason: format!(
                        "needs minecraft {}, the game is {}",
                        wanted.join(", "),
                        game.unwrap_or("?")
                    ),
                });
            }
        }
    }
    out
}

/// Every problem of a set, as comparable text: what the loader refuses and
/// what is broken in each mod.
pub fn problems(r: &SetReport) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = r
        .findings
        .iter()
        .filter(|f| !f.is_info())
        .map(|f| format!("{f:?}"))
        .collect();
    for m in &r.mods {
        for f in m.report.findings.iter().filter(|f| !f.is_info()) {
            out.insert(format!("{}: {f:?}", m.jar));
        }
        for f in m
            .report
            .code
            .iter()
            .flat_map(|c| &c.findings)
            .filter(|f| !f.is_info())
        {
            out.insert(format!("{}: {f:?}", m.jar));
        }
    }
    out
}

#[derive(Debug)]
pub struct Applied {
    pub before: usize,
    pub after: usize,
    /// Problems the change brought that were not there before.
    pub new_problems: Vec<String>,
    /// Kept: strictly better. Otherwise every jar is back.
    pub kept: bool,
    pub backup: PathBuf,
}

/// Takes the jars out (after backing them up), analyses the folder again
/// and keeps the result only if it is strictly better.
pub fn apply(dir: &Path, removals: &[Removal], jar: Option<&JavaJar>) -> Result<Applied, String> {
    if removals.is_empty() {
        return Err("nothing to repair".into());
    }
    let dir = fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dir = plain(dir);
    for r in removals {
        if r.jar.contains(['/', '\\']) || r.jar.contains("..") || !dir.join(&r.jar).is_file() {
            return Err(format!("{} is not a jar of {}", r.jar, dir.display()));
        }
    }
    let before_report = analyze_set(&dir, jar);
    let before = problems(&before_report);

    // 1. a full backup before anything changes
    let backup = kebitx_repair::apply::backup_dir(&dir)?;
    let mut entries = Vec::new();
    for r in removals {
        let saved = backup.join("files").join(&r.jar);
        fs::create_dir_all(saved.parent().unwrap_or(&backup)).map_err(|e| e.to_string())?;
        fs::copy(dir.join(&r.jar), &saved).map_err(|e| format!("backup of {}: {e}", r.jar))?;
        entries.push((r.jar.clone(), "delete"));
    }
    kebitx_repair::apply::write_index(&backup, &dir, &entries)?;

    // 2. take them out; a failure puts everything back
    for r in removals {
        if let Err(e) = fs::remove_file(dir.join(&r.jar)) {
            kebitx_repair::restore(&backup)
                .map_err(|x| format!("{}: {e}; and restoring failed too: {x}", r.jar))?;
            return Err(format!("{}: {e}", r.jar));
        }
    }

    // 3. the second analysis decides
    let after = problems(&analyze_set(&dir, jar));
    let new_problems: Vec<String> = after.difference(&before).cloned().collect();
    let kept = new_problems.is_empty() && after.len() < before.len();
    if !kept {
        kebitx_repair::restore(&backup)?;
    }
    Ok(Applied {
        before: before.len(),
        after: after.len(),
        new_problems,
        kept,
        backup,
    })
}

/// `\\?\C:\...` (what `canonicalize` gives on Windows) without its prefix.
fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}
