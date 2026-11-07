//! Repairs through the C ABI: the same plan, backup, second analysis and undo as
//! `kebitx repair`, with the safe fixes only: wrong case in a Bedrock pack or
//! world, and duplicate jars in a Java mods folder. Deleting files or taking
//! out incompatible mods stays in the CLI, where it must be asked for by name.
//!
//! What is applied must be what was shown: the preview gives a fingerprint of
//! its changes, and applying plans again and refuses if they differ.

use crate::Lines;
use kebitx_core::codec::crc32;
use kebitx_core::i18n::{tf, tr};
use kebitx_core::json::Json;
use kebitx_core::vanilla::VanillaCatalog;
use kebitx_java::modset::ModFolder;
use kebitx_java::repair as mods;
use kebitx_repair::{Change, Fix};
use std::path::Path;

/// What a folder's repair would do, as lines, and how to carry it out.
enum Planned {
    Bedrock {
        plan: kebitx_repair::Plan,
        catalog: Box<VanillaCatalog>,
    },
    Mods {
        removals: Vec<mods::Removal>,
        jar: Option<kebitx_java::JavaJar>,
    },
    /// Nothing here can be repaired this way: why.
    Unsupported(&'static str),
}

fn planned(root: &Path) -> Planned {
    if kebitx_java::is_mod_folder(root) {
        let folder = ModFolder::open(root);
        let jar = kebitx_java::JavaAssets::discover().and_then(|j| folder.game_for(&j));
        let report = folder.analyze(jar.as_ref());
        return Planned::Mods {
            removals: mods::plan(&report, &[mods::Fix::Duplicates]),
            jar,
        };
    }
    if kebitx_java::is_java(root) {
        return Planned::Unsupported(tr(
            "Repairs exist for Bedrock packs and worlds, and for Java mods folders.",
        ));
    }
    if !root.is_dir() {
        return Planned::Unsupported(tr("Not a folder."));
    }
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    Planned::Bedrock {
        plan: kebitx_repair::plan(root, &catalog, &[Fix::Case]),
        catalog: Box::new(catalog),
    }
}

fn change_lines(root: &Path, planned: &Planned) -> Vec<String> {
    match planned {
        Planned::Bedrock { plan, .. } => plan
            .changes
            .iter()
            .map(|c| {
                let file = c.file().strip_prefix(root).unwrap_or(c.file()).display();
                match c {
                    Change::Edit {
                        old,
                        new,
                        occurrences,
                        reason,
                        ..
                    } => tf(
                        "Edit {0}: {1} -> {2} ({3}x). {4}",
                        &[&file, old, new, occurrences, reason],
                    ),
                    Change::Delete { reason, .. } => tf("Delete {0}. {1}", &[&file, reason]),
                }
            })
            .collect(),
        Planned::Mods { removals, .. } => removals
            .iter()
            .map(|r| tf("Take out {0}: {1}", &[&r.jar, &r.reason]))
            .collect(),
        Planned::Unsupported(_) => Vec::new(),
    }
}

/// Changes in another order or with another word are other changes.
fn fingerprint(lines: &[String]) -> String {
    format!("{:08x}-{}", crc32(lines.join("\n").as_bytes()), lines.len())
}

fn result(kind: &str, summary: Vec<String>, problems: Vec<String>) -> Lines {
    Lines {
        kind: kind.to_string(),
        broken: problems.len(),
        summary,
        problems,
        notes: Vec::new(),
        extra: Vec::new(),
    }
}

/// The preview: nothing is written.
pub fn plan_json(root: &Path) -> Option<String> {
    let planned = planned(root);
    let lines = change_lines(root, &planned);
    let summary =
        match &planned {
            Planned::Unsupported(why) => vec![why.to_string()],
            _ if lines.is_empty() => vec![tr("Nothing kebitx can repair safely here.").to_string()],
            _ => vec![
            tr("Every file is backed up first, and the result stays only if it is strictly better.")
                .to_string(),
        ],
        };
    let mut report = result(tr("Repair preview"), summary, Vec::new());
    report
        .extra
        .push(("fingerprint".to_string(), Json::Str(fingerprint(&lines))));
    report.broken = lines.len();
    report.problems = lines;
    Some(report.json(root))
}

/// Carries out the repair the preview with `expected` showed.
pub fn apply_json(root: &Path, expected: &str) -> Option<String> {
    let planned = planned(root);
    let lines = change_lines(root, &planned);
    if lines.is_empty() || fingerprint(&lines) != expected {
        return Some(
            result(
                tr("Nothing was changed"),
                vec![
                    tr("The folder changed since the preview. Preview the repair again.")
                        .to_string(),
                ],
                Vec::new(),
            )
            .json(root),
        );
    }
    let outcome = match &planned {
        Planned::Bedrock { plan, catalog } => kebitx_repair::apply(root, plan, catalog).map(|a| {
            let count = |s: &kebitx_repair::Score| s.broken + s.case_mismatches + s.compat_errors;
            (
                count(&a.before),
                count(&a.after),
                a.kept,
                a.backup,
                Vec::new(),
            )
        }),
        Planned::Mods { removals, jar } => mods::apply(root, removals, jar.as_ref())
            .map(|a| (a.before, a.after, a.kept, a.backup, a.new_problems)),
        Planned::Unsupported(why) => Err(why.to_string()),
    };
    let (before, after, kept, backup, new_problems) = match outcome {
        Ok(done) => done,
        Err(e) => return Some(result(tr("The repair failed"), vec![e], Vec::new()).json(root)),
    };
    let counts = tf("Problems: {0} before, {1} after.", &[&before, &after]);
    let mut report = if kept {
        result(
            tr("Repair kept"),
            vec![counts, tf("Backup: {0}", &[&backup.display()])],
            Vec::new(),
        )
    } else {
        result(
            tr("Repair rolled back"),
            vec![
                counts,
                tr("It was not strictly better, so every file is as it was.").to_string(),
            ],
            new_problems,
        )
    };
    report.extra.push(("kept".to_string(), Json::Bool(kept)));
    if kept {
        report.extra.push((
            "backup".to_string(),
            Json::Str(backup.to_string_lossy().to_string()),
        ));
    }
    Some(report.json(root))
}

/// Puts back what a kept repair changed.
pub fn undo_json(backup: &Path) -> Option<String> {
    Some(match kebitx_repair::restore(backup) {
        Ok(n) => result(
            tr("Repair undone"),
            vec![tf("Restored {0} file(s).", &[&n])],
            Vec::new(),
        )
        .json(backup),
        Err(e) => result(tr("Undo failed"), vec![e], Vec::new()).json(backup),
    })
}
