//! `kebitx.json`, the project file.
//!
//! ```json
//! {
//!   "name": "my_addon",
//!   "packs": { "behavior": "packs/BP", "resource": "packs/RP" },
//!   "filters": [
//!     { "name": "minify", "run": ["node", "filters/minify.js"] }
//!   ],
//!   "check": true,
//!   "export": { "target": "development" }
//! }
//! ```
//!
//! `target` is one of:
//! - `local` (default): `build/<name>_BP`, `build/<name>_RP`;
//! - `development`: the game's `development_*_packs`;
//! - `mcaddon`: `build/<name>.mcaddon`, both packs in one archive;
//! - `mcpack`: `build/<name>_BP.mcpack` and `build/<name>_RP.mcpack`, one each;
//! - `mcworld`: `build/<name>.mcworld`, the world template in `export.world`
//!   (a folder with `level.dat`) with the packs inside and activated.
//!
//! `export.com_mojang` overrides where the game's folder is.

use kebitx_core::json::{self, Json};
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "kebitx.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    /// Program and arguments. A program given as a path relative to the
    /// project (`filters/x.exe`, `./x`) is resolved against the project root;
    /// a bare name (`node`) is looked up on PATH.
    pub run: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Local,
    Development,
    McAddon,
    McPack,
    McWorld,
}

impl Target {
    pub const NAMES: &'static str = "local, development, mcaddon, mcpack, mcworld";

    pub fn parse(s: &str) -> Option<Target> {
        Some(match s {
            "local" => Target::Local,
            "development" => Target::Development,
            "mcaddon" => Target::McAddon,
            "mcpack" => Target::McPack,
            "mcworld" => Target::McWorld,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub behavior: Option<PathBuf>,
    pub resource: Option<PathBuf>,
    pub filters: Vec<Filter>,
    pub check: bool,
    pub target: Target,
    pub com_mojang: Option<PathBuf>,
    /// World template for `mcworld`: a folder with `level.dat` (and `db/`).
    pub world: Option<PathBuf>,
}

impl Project {
    pub fn load(root: &Path) -> Result<Project, String> {
        let file = root.join(PROJECT_FILE);
        let src = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let v = json::parse(&src)
            .map_err(|e| format!("{}: {} at byte {}", file.display(), e.msg, e.pos))?;
        Self::from_json(root, &v)
    }

    pub fn from_json(root: &Path, v: &Json) -> Result<Project, String> {
        let name = v
            .get("name")
            .and_then(Json::as_str)
            .ok_or("\"name\" is required")?
            .trim()
            .to_string();
        if !safe_name(&name) {
            return Err(format!(
                "\"name\" must be letters, digits, '_', '-', '.' or spaces (it becomes a folder name): {name:?}"
            ));
        }
        let packs = v.get("packs").ok_or("\"packs\" is required")?;
        let pack = |key: &str| -> Result<Option<PathBuf>, String> {
            let Some(rel) = packs.get(key).and_then(Json::as_str) else {
                return Ok(None);
            };
            let p = root.join(rel);
            if !p.join("manifest.json").is_file() {
                return Err(format!("packs.{key}: no manifest.json in {}", p.display()));
            }
            Ok(Some(p))
        };
        let behavior = pack("behavior")?;
        let resource = pack("resource")?;
        if behavior.is_none() && resource.is_none() {
            return Err("\"packs\" needs \"behavior\", \"resource\" or both".into());
        }

        let mut filters = Vec::new();
        for (i, f) in v
            .get("filters")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let run: Vec<String> = f
                .get("run")
                .and_then(Json::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Json::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if run.is_empty() {
                return Err(format!(
                    "filters[{i}]: \"run\" must be a non-empty list of strings"
                ));
            }
            let name = f
                .get("name")
                .and_then(Json::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| run[0].clone());
            filters.push(Filter { name, run });
        }

        let export = v.get("export");
        let name_of_target = export
            .and_then(|e| e.get("target"))
            .and_then(Json::as_str)
            .unwrap_or("local");
        let target = Target::parse(name_of_target).ok_or_else(|| {
            format!(
                "export.target: unknown {name_of_target:?} ({})",
                Target::NAMES
            )
        })?;
        let com_mojang = export
            .and_then(|e| e.get("com_mojang"))
            .and_then(Json::as_str)
            .map(|p| root.join(p));
        let world = export
            .and_then(|e| e.get("world"))
            .and_then(Json::as_str)
            .map(|p| root.join(p));

        Ok(Project {
            root: root.to_path_buf(),
            name,
            behavior,
            resource,
            filters,
            check: v
                .get("check")
                .map(|c| *c != Json::Bool(false))
                .unwrap_or(true),
            target,
            com_mojang,
            world,
        })
    }
}

/// A name that is safe as ONE folder name: no separators, no `..`.
fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-. ".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cannot_escape_their_folder() {
        assert!(safe_name("my_addon v2.1"));
        for bad in ["", "..", "../x", "a/b", "a\\b", "C:", "x..y"] {
            assert!(!safe_name(bad), "{bad:?}");
        }
    }
}
