//! backup → write → analyse again → keep, or roll back.

use crate::plan::{Change, Plan};
use crate::score::{score, Score};
use kebitx_core::json::{self, Json, JsonMap};
use kebitx_core::vanilla::VanillaCatalog;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const INDEX: &str = "kebitx-backup.json";

#[derive(Debug)]
pub struct Applied {
    pub before: Score,
    pub after: Score,
    /// The repair made things strictly better and was kept. If false, every
    /// file is back as it was.
    pub kept: bool,
    /// Where the originals are; `restore` puts them back.
    pub backup: PathBuf,
}

/// Apply `plan` to the world or pack at `root`. Errors leave the files as they
/// were (anything already written is restored from the backup).
pub fn apply(root: &Path, plan: &Plan, catalog: &VanillaCatalog) -> Result<Applied, String> {
    if plan.changes.is_empty() {
        return Err("nothing to repair".into());
    }
    let root = plain(fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?);
    for c in &plan.changes {
        if !c.file().starts_with(&root)
            && !fs::canonicalize(c.file()).is_ok_and(|f| plain(f).starts_with(&root))
        {
            return Err(format!(
                "{} is outside {}",
                c.file().display(),
                root.display()
            ));
        }
    }
    let before = score(&root, catalog);

    // ---- 1. backup, completely, before anything is written
    let backup = backup_dir(&root)?;
    let mut entries = Vec::new();
    for c in &plan.changes {
        let file =
            plain(fs::canonicalize(c.file()).map_err(|e| format!("{}: {e}", c.file().display()))?);
        let rel = file
            .strip_prefix(&root)
            .map_err(|_| format!("{} is outside the world", file.display()))?;
        let saved = backup.join("files").join(rel);
        if !saved.exists() {
            fs::create_dir_all(saved.parent().unwrap_or(&backup)).map_err(|e| e.to_string())?;
            fs::copy(&file, &saved).map_err(|e| format!("backup of {}: {e}", file.display()))?;
        }
        let action = if matches!(c, Change::Delete { .. }) {
            "delete"
        } else {
            "edit"
        };
        entries.push((rel.to_string_lossy().replace('\\', "/"), action));
    }
    write_index(&backup, &root, &entries)?;

    // ---- 2. write; any failure puts everything back
    if let Err(e) = write_changes(plan) {
        restore(&backup).map_err(|r| format!("{e}; and restoring failed too: {r}"))?;
        return Err(e);
    }

    // ---- 3. the second analysis decides
    let after = score(&root, catalog);
    let kept = after.strictly_better_than(&before);
    if !kept {
        restore(&backup)?;
    }
    Ok(Applied {
        before,
        after,
        kept,
        backup,
    })
}

fn write_changes(plan: &Plan) -> Result<(), String> {
    for c in &plan.changes {
        match c {
            Change::Edit {
                file,
                old,
                new,
                occurrences,
                ..
            } => {
                let text =
                    fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
                let found = text.matches(old.as_str()).count();
                if found != *occurrences {
                    return Err(format!(
                        "{} changed since the plan ({found} occurrences of {old}, expected {occurrences}); nothing kept",
                        file.display()
                    ));
                }
                write_atomically(file, text.replace(old.as_str(), new).as_bytes())?;
            }
            Change::Delete { file, .. } => {
                if file.exists() {
                    fs::remove_file(file).map_err(|e| format!("{}: {e}", file.display()))?;
                }
            }
        }
    }
    Ok(())
}

/// `canonicalize` on Windows answers `\\?\C:\...`; the same path without the
/// prefix is what people read and type (and what `--undo` gets pasted).
fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}

/// Write to a temporary file next to the target, then rename over it: a
/// crash leaves the old file or the new one, never half of each.
fn write_atomically(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = file.with_extension(format!("kebitx-tmp{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, file).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", file.display())
    })
}

/// `<parent>/<world>.kebitx-backups/<seconds>`: next to the world, not in it,
/// so a backup is never packaged with the world.
fn backup_dir(root: &Path) -> Result<PathBuf, String> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "world".into());
    let parent = root
        .parent()
        .ok_or("the world has no parent folder for its backups")?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut dir = parent
        .join(format!("{name}.kebitx-backups"))
        .join(secs.to_string());
    let mut n = 1;
    while dir.exists() {
        dir = parent
            .join(format!("{name}.kebitx-backups"))
            .join(format!("{secs}-{n}"));
        n += 1;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn write_index(backup: &Path, root: &Path, entries: &[(String, &str)]) -> Result<(), String> {
    let files = entries
        .iter()
        .map(|(path, action)| {
            Json::Obj(JsonMap::from_entries(vec![
                ("action".into(), Json::Str(action.to_string())),
                ("path".into(), Json::Str(path.clone())),
            ]))
        })
        .collect();
    let index = Json::Obj(JsonMap::from_entries(vec![
        ("root".into(), Json::Str(root.to_string_lossy().to_string())),
        ("files".into(), Json::Arr(files)),
    ]));
    fs::write(backup.join(INDEX), json::to_string(&index)).map_err(|e| e.to_string())
}

/// Put every file of a backup back where it was: edited files get their
/// original bytes, deleted files are recreated.
pub fn restore(backup: &Path) -> Result<usize, String> {
    let src =
        fs::read_to_string(backup.join(INDEX)).map_err(|e| format!("{}: {e}", backup.display()))?;
    let index = json::parse(&src).map_err(|e| format!("{INDEX}: {}", e.msg))?;
    let root = PathBuf::from(
        index
            .get("root")
            .and_then(Json::as_str)
            .ok_or("backup index without root")?,
    );
    let mut restored = 0;
    let mut seen = std::collections::HashSet::new();
    for f in index
        .get("files")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let Some(rel) = f.get("path").and_then(Json::as_str) else {
            continue;
        };
        // a relative path that climbs out of the world is not ours to write
        if rel.split('/').any(|c| c == "..") || Path::new(rel).is_absolute() {
            return Err(format!(
                "backup index names a path outside the world: {rel}"
            ));
        }
        if !seen.insert(rel.to_string()) {
            continue;
        }
        let target = root.join(rel);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::copy(backup.join("files").join(rel), &target)
            .map_err(|e| format!("{}: {e}", target.display()))?;
        restored += 1;
    }
    Ok(restored)
}
