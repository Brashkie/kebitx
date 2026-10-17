//! What `kebitx migrate` is given: a folder, or a `.jar` / `.zip` (a mod,
//! a zipped pack), extracted to a temporary folder first.
//!
//! The archive is untrusted: an entry whose name would land outside the
//! folder (`..`, an absolute path, a drive letter) is refused, as is an
//! archive too large to be a pack or a mod.

use kebitx_core::codec::zip::ZipArchive;
use std::path::{Path, PathBuf};

/// Above this much, an archive is not a pack or a mod.
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRY: u64 = 256 * 1024 * 1024;

/// The folder to read: `path` itself, or where its archive was extracted.
pub struct Input {
    pub root: PathBuf,
    /// Extracted here, removed when dropped.
    temp: bool,
}

impl Drop for Input {
    fn drop(&mut self) {
        if self.temp {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Is this entry name safe to write under a folder?
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.starts_with('\\')
        && !name.contains(':')
        && !name.split(['/', '\\']).any(|part| part == "..")
}

pub fn open(path: &Path) -> Result<Input, String> {
    if path.is_dir() {
        return Ok(Input {
            root: path.to_path_buf(),
            temp: false,
        });
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "jar" | "zip") {
        return Err(format!("{}: not a folder, .jar or .zip", path.display()));
    }
    let zip = ZipArchive::open(path)?;
    let seed = kebitx_core::codec::crc32(path.to_string_lossy().as_bytes());
    let root =
        std::env::temp_dir().join(format!("kebitx-migrate-{seed:08x}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let input = Input {
        root: root.clone(),
        temp: true,
    };
    let mut total = 0u64;
    let mut names: Vec<&String> = zip.names().collect();
    names.sort();
    for name in names {
        if name.ends_with('/') {
            continue;
        }
        if !safe_name(name) {
            return Err(format!(
                "{}: entry {name:?} would land outside the folder: refused",
                path.display()
            ));
        }
        let bytes = zip.read(name, MAX_ENTRY)?;
        total += bytes.len() as u64;
        if total > MAX_TOTAL {
            return Err(format!(
                "{}: larger than a pack or a mod can be",
                path.display()
            ));
        }
        let dst = root.join(name);
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dst, bytes).map_err(|e| e.to_string())?;
    }
    Ok(input)
}

/// A mod's loader and name, from its metadata file.
pub fn mod_meta(root: &Path) -> Option<(&'static str, String)> {
    let read = |p: &str| kebitx_core::filesystem::read_text(&root.join(p)).ok();
    if let Some(t) = read("fabric.mod.json") {
        let j = kebitx_core::json::parse(&t).ok();
        let name = j
            .as_ref()
            .and_then(|j| j.get("name").or_else(|| j.get("id")))
            .and_then(|v| v.as_str())
            .unwrap_or("Fabric mod");
        return Some(("Fabric", name.to_string()));
    }
    if let Some(t) = read("quilt.mod.json") {
        let j = kebitx_core::json::parse(&t).ok();
        let name = j
            .as_ref()
            .and_then(|j| j.get("quilt_loader"))
            .and_then(|q| {
                q.get("metadata")
                    .and_then(|m| m.get("name"))
                    .or_else(|| q.get("id"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("Quilt mod");
        return Some(("Quilt", name.to_string()));
    }
    for (file, loader) in [
        ("META-INF/neoforge.mods.toml", "NeoForge"),
        ("META-INF/mods.toml", "Forge"),
    ] {
        if let Some(t) = read(file) {
            // TOML: the first `displayName = "..."`
            let name = t
                .lines()
                .find_map(|l| {
                    l.trim().strip_prefix("displayName").map(|r| {
                        r.trim_start_matches([' ', '='])
                            .trim_matches('"')
                            .to_string()
                    })
                })
                .unwrap_or_else(|| format!("{loader} mod"));
            return Some((loader, name));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_entry_names_are_refused() {
        for bad in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            "C:/windows/x",
            "\\\\server\\x",
            "..\\x",
            "",
        ] {
            assert!(!safe_name(bad), "{bad}");
        }
        for good in [
            "assets/demo/lang/en_us.json",
            "fabric.mod.json",
            "a/b/c.class",
        ] {
            assert!(safe_name(good), "{good}");
        }
    }
}
