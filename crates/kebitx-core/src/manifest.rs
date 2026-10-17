//! `manifest.json`: what a pack is, what it needs, and how to find the packs of
//! a world.

use crate::json::{self, Json};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Resources,
    Data,
    Skins,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dependency {
    /// Another pack, by its header UUID.
    Pack { uuid: String },
    /// A script module provided by the game (`@minecraft/server`), with the
    /// version as written: `"1.19.0"`, `"1.12.0-beta"`, `[0, 1, 0]` -> `"0.1.0"`.
    /// `None` if absent or not a string/array.
    Module {
        name: String,
        version: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub uuid: String,
    pub min_engine_version: Option<[u32; 3]>,
    pub kind: PackKind,
    pub module_uuids: Vec<String>,
    pub dependencies: Vec<Dependency>,
}

/// One pack of a world.
#[derive(Debug, Clone)]
pub struct Pack {
    pub root: PathBuf,
    /// Folder name: how the reports refer to the pack.
    pub folder: String,
    pub manifest: Manifest,
}

pub fn read_manifest(pack_root: &Path) -> Option<Manifest> {
    let src = crate::filesystem::read_text(&pack_root.join("manifest.json")).ok()?;
    parse_manifest(&json::parse(&src).ok()?)
}

pub fn parse_manifest(root: &Json) -> Option<Manifest> {
    let header = root.get("header")?;
    let uuid = header
        .get("uuid")
        .and_then(Json::as_str)?
        .trim()
        .to_ascii_lowercase();
    let modules = root
        .get("modules")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let kind = modules
        .iter()
        .filter_map(|m| m.get("type").and_then(Json::as_str))
        .map(|t| match t {
            "resources" => PackKind::Resources,
            "data" | "script" | "javascript" => PackKind::Data,
            "skin_pack" => PackKind::Skins,
            _ => PackKind::Other,
        })
        // a pack with a `data` module and a `script` module is a behavior pack
        .min_by_key(|k| *k as u8)
        .unwrap_or(PackKind::Other);
    let module_uuids = modules
        .iter()
        .filter_map(|m| m.get("uuid").and_then(Json::as_str))
        .map(|u| u.trim().to_ascii_lowercase())
        .collect();
    let dependencies = root
        .get("dependencies")
        .and_then(Json::as_array)
        .map(|deps| {
            deps.iter()
                .filter_map(|d| {
                    if let Some(u) = d.get("uuid").and_then(Json::as_str) {
                        Some(Dependency::Pack {
                            uuid: u.trim().to_ascii_lowercase(),
                        })
                    } else {
                        d.get("module_name")
                            .and_then(Json::as_str)
                            .map(|n| Dependency::Module {
                                name: n.to_string(),
                                version: d.get("version").and_then(raw_version),
                            })
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Manifest {
        name: header
            .get("name")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        uuid,
        min_engine_version: header.get("min_engine_version").and_then(version),
        kind,
        module_uuids,
        dependencies,
    })
}

/// A version kept as text, so a pre-release tag (`-beta`) survives. Arrays of
/// integers become `"a.b.c"`; anything else in an array is kept visible.
fn raw_version(v: &Json) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.trim().to_string());
    }
    let parts: Vec<String> = v
        .as_array()?
        .iter()
        .map(|x| match x {
            Json::Num(n) if n.fract() == 0.0 && *n >= 0.0 => format!("{}", *n as u64),
            other => format!("{other:?}"),
        })
        .collect();
    Some(parts.join("."))
}

/// `[1, 21, 0]` or `"1.21.0"` (format 3).
pub fn version(v: &Json) -> Option<[u32; 3]> {
    if let Some(a) = v.as_array() {
        let n = |i: usize| match a.get(i) {
            Some(Json::Num(x)) if *x >= 0.0 => Some(*x as u32),
            None => Some(0),
            _ => None,
        };
        return Some([n(0)?, n(1)?, n(2)?]);
    }
    parse_version(v.as_str()?)
}

/// `"1.26.52"` -> `[1, 26, 52]`. Missing parts are 0; anything else is `None`.
pub fn parse_version(s: &str) -> Option<[u32; 3]> {
    let mut out = [0u32; 3];
    for (i, part) in s.trim().split('.').enumerate() {
        if i >= 3 {
            break;
        }
        // tolerate pre-release suffixes: "1.21.0-beta"
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        out[i] = digits.parse().ok()?;
    }
    Some(out)
}

/// Every pack of a world, in either layout: packs as direct subfolders (a
/// build folder), or under `behavior_packs/` and `resource_packs/` (a real
/// world). Folders without a readable manifest are skipped.
pub fn packs_in(world: &Path) -> Vec<Pack> {
    let mut dirs = subdirs(world);
    for sub in ["behavior_packs", "resource_packs"] {
        dirs.extend(subdirs(&world.join(sub)));
    }
    let mut out: Vec<Pack> = dirs
        .into_iter()
        .filter_map(|root| {
            let manifest = read_manifest(&root)?;
            let folder = root.file_name()?.to_string_lossy().to_string();
            Some(Pack {
                root,
                folder,
                manifest,
            })
        })
        .collect();
    out.sort_by(|a, b| a.root.cmp(&b.root));
    out
}

/// One pack on its own. A folder without a readable manifest (an author's work
/// folder) is taken as a resource pack, which is what single-pack analysis
/// assumes everywhere else.
pub fn pack_at(root: &Path) -> Pack {
    let folder = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let manifest = read_manifest(root).unwrap_or_else(|| Manifest {
        name: folder.clone(),
        uuid: String::new(),
        min_engine_version: None,
        kind: PackKind::Resources,
        module_uuids: Vec::new(),
        dependencies: Vec::new(),
    });
    Pack {
        root: root.to_path_buf(),
        folder,
        manifest,
    }
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_in_both_formats() {
        assert_eq!(
            version(&json::parse("[1, 21, 0]").unwrap()),
            Some([1, 21, 0])
        );
        assert_eq!(
            version(&json::parse("\"1.21.30\"").unwrap()),
            Some([1, 21, 30])
        );
        assert_eq!(parse_version("1.26.52"), Some([1, 26, 52]));
        assert_eq!(parse_version("2.0.0-beta"), Some([2, 0, 0]));
        assert_eq!(parse_version("unknown"), None);
    }
}
