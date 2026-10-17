//! What a repair would change. Describing only: nothing here writes.

use crate::score::layout;
use kebitx_core::analyze::analyze_packs;
use kebitx_core::filesystem::all_files;
use kebitx_core::passes::unused_asset;
use kebitx_core::report::Issue;
use kebitx_core::vanilla::VanillaCatalog;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    /// Write a reference with the same case as the file on disk.
    Case,
    /// Delete texture and sound files the orphan pass proved unused.
    Unused,
}

impl Fix {
    pub fn parse(s: &str) -> Option<Fix> {
        match s {
            "case" => Some(Fix::Case),
            "unused" => Some(Fix::Unused),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Replace every occurrence of the quoted literal `old` with `new` in
    /// `file`. `occurrences` is how many there were when planned: if the file
    /// changed since, the repair refuses instead of guessing.
    Edit {
        file: PathBuf,
        old: String,
        new: String,
        occurrences: usize,
        reason: String,
    },
    Delete {
        file: PathBuf,
        bytes: u64,
        reason: String,
    },
}

impl Change {
    pub fn file(&self) -> &Path {
        match self {
            Change::Edit { file, .. } | Change::Delete { file, .. } => file,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub changes: Vec<Change>,
}

pub fn plan(root: &Path, catalog: &VanillaCatalog, fixes: &[Fix]) -> Plan {
    let l = layout(root);
    let mut changes = Vec::new();

    if fixes.contains(&Fix::Case) {
        let reports = analyze_packs(&l.resource_roots, catalog);
        for (pack_root, (_, report)) in l.resource_roots.iter().zip(&reports) {
            // (as written, as on disk) — one per distinct reference
            let mut fixes: BTreeSet<(String, String)> = BTreeSet::new();
            for i in &report.issues {
                if let Issue::CaseMismatch {
                    reference, on_disk, ..
                } = i
                {
                    if let Some(right) = same_path_as_on_disk(reference, on_disk) {
                        fixes.insert((reference.clone(), right));
                    }
                }
            }
            if fixes.is_empty() {
                continue;
            }
            for file in json_files(pack_root) {
                let Ok(text) = kebitx_core::filesystem::read_text(&file) else {
                    continue;
                };
                for (wrong, right) in &fixes {
                    let old = format!("\"{wrong}\"");
                    let occurrences = text.matches(&old).count();
                    if occurrences > 0 {
                        changes.push(Change::Edit {
                            file: file.clone(),
                            old,
                            new: format!("\"{right}\""),
                            occurrences,
                            reason: format!("'{wrong}' is '{right}' on disk: works on Windows, missing texture on Android/iOS"),
                        });
                    }
                }
            }
        }
    }

    if fixes.contains(&Fix::Unused) {
        for u in unused_asset::run(&l.packs, catalog).unused {
            let Some(pack) = l.packs.iter().find(|p| p.folder == u.pack) else {
                continue;
            };
            changes.push(Change::Delete {
                file: pack.root.join(&u.path),
                bytes: u.bytes,
                reason: "nothing in the world references it".into(),
            });
        }
    }
    Plan { changes }
}

/// The reference rewritten with the file's real case, keeping the reference's
/// own form (with or without the extension). `None` unless the two differ in
/// case only — anything else is not this repair's business.
fn same_path_as_on_disk(reference: &str, on_disk: &str) -> Option<String> {
    let ext = |s: &str| {
        [".png", ".tga", ".jpg", ".jpeg"]
            .iter()
            .find(|e| s.to_ascii_lowercase().ends_with(*e))
            .map(|e| e.len())
            .unwrap_or(0)
    };
    let right = if ext(reference) > 0 {
        on_disk.to_string()
    } else {
        on_disk[..on_disk.len() - ext(on_disk)].to_string()
    };
    (right != reference && right.eq_ignore_ascii_case(reference)).then_some(right)
}

fn json_files(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = all_files(root)
        .into_iter()
        .filter(|f| {
            f.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        })
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fix_keeps_the_reference_form_and_changes_only_case() {
        assert_eq!(
            same_path_as_on_disk("textures/x/desert_3", "textures/x/Desert_3.png").as_deref(),
            Some("textures/x/Desert_3")
        );
        assert_eq!(
            same_path_as_on_disk("textures/x/desert_3.png", "textures/x/Desert_3.png").as_deref(),
            Some("textures/x/Desert_3.png")
        );
        assert_eq!(
            same_path_as_on_disk("textures/x/a", "textures/x/a.png"),
            None,
            "already right"
        );
        assert_eq!(
            same_path_as_on_disk("textures/x/a", "textures/y/A.png"),
            None,
            "not the same path"
        );
    }
}
