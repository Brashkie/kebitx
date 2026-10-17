//! File operations of a build, with one safety rule: a folder is deleted only
//! if a build created it. Every folder a build writes gets a marker file; a
//! folder without it is somebody's work, and is refused, never deleted.

use std::fs;
use std::io;
use std::path::Path;

/// Marker a build leaves in every folder it creates.
pub const MARKER: &str = ".kebitx-build";

/// Copy `src` into `dst` recursively (`dst` is created). Symbolic links are
/// skipped: following one could copy, or later delete, something outside.
pub fn copy_dir(src: &Path, dst: &Path) -> io::Result<u64> {
    fs::create_dir_all(dst)?;
    let mut files = 0;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_symlink() {
            continue;
        } else if ty.is_dir() {
            files += copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
            files += 1;
        }
    }
    Ok(files)
}

/// Make `dir` an empty folder owned by the build. If it exists it must carry
/// the marker; otherwise this refuses and touches nothing.
pub fn fresh_owned_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        if !dir.join(MARKER).is_file() {
            return Err(format!(
                "{} exists and was not created by kebitx build; refusing to replace it",
                dir.display()
            ));
        }
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    fs::write(
        dir.join(MARKER),
        "created by kebitx build; safe for it to replace\n",
    )
    .map_err(|e| format!("{}: {e}", dir.display()))
}

/// Replace `dst` with a copy of `src`, under the ownership rule.
pub fn replace_owned_dir(src: &Path, dst: &Path) -> Result<u64, String> {
    fresh_owned_dir(dst)?;
    copy_dir(src, dst).map_err(|e| format!("{} -> {}: {e}", src.display(), dst.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kebitx-fsutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_folder_without_the_marker_is_never_deleted() {
        let d = scratch("refuse");
        let target = d.join("someones_pack");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("precious.json"), "{}").unwrap();
        assert!(fresh_owned_dir(&target).is_err());
        assert!(target.join("precious.json").is_file(), "still there");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_owned_folder_is_replaced() {
        let d = scratch("replace");
        let target = d.join("built");
        fresh_owned_dir(&target).unwrap();
        fs::write(target.join("old.json"), "{}").unwrap();
        fresh_owned_dir(&target).unwrap();
        assert!(!target.join("old.json").exists());
        assert!(target.join(MARKER).is_file());
        let _ = fs::remove_dir_all(&d);
    }
}
