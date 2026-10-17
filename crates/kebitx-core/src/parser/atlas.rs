//! Texture atlases: `textures/terrain_texture.json` and `textures/item_texture.json`.
//!
//! Each entry maps a SHORTNAME to one or more texture paths. The value takes many
//! shapes, and every one of them shows up in real packs:
//!
//! ```text
//! "textures": "textures/blocks/stone"
//! "textures": ["a", "b"]
//! "textures": {"path": "a", "overlay_color": "#cef586"}
//! "textures": [{"path": "a", "tint_color": "#..."}, ...]
//! "textures": {"variations": [{"path": "a", "weight": 10}, ...]}
//! "textures": [{"variations": [...]}, {"variations": [...]}]     (per face)
//! ```
//!
//! A reader that knows only the first two shapes returns NOTHING for the rest —
//! and if anything downstream rebuilds the entry from what was read, the entry
//! ends up as `"textures": []`. That is a block declared with zero textures: the
//! pink checkerboard, guaranteed. It happened to 86 blocks on a real world.
//!
//! So: every shape is walked, and an entry that yields zero paths is reported on
//! its own. It is not "unknown", it is provably broken.
//!
//! Strings starting with `#` are tint colours, not paths, and are skipped.

use crate::json::{self, Json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasEntry {
    pub shortname: String,
    /// Texture paths WITHOUT extension, in declaration order. May repeat.
    pub paths: Vec<String>,
}

#[derive(Debug)]
pub struct AtlasError(pub String);

/// Parse a whole atlas file into its entries.
pub fn parse_atlas(source: &str) -> Result<Vec<AtlasEntry>, AtlasError> {
    let root =
        json::parse(source).map_err(|e| AtlasError(format!("{} @ byte {}", e.msg, e.pos)))?;
    let data = root
        .get("texture_data")
        .and_then(Json::as_object)
        .ok_or_else(|| AtlasError("missing texture_data".into()))?;
    let mut out = Vec::with_capacity(data.len());
    for (shortname, value) in data {
        let mut paths = Vec::new();
        collect_paths(value, &mut paths);
        out.push(AtlasEntry {
            shortname: shortname.clone(),
            paths,
        });
    }
    Ok(out)
}

/// Every texture path inside an entry value, whatever its shape. Read-only.
pub fn collect_paths(v: &Json, out: &mut Vec<String>) {
    if let Some(s) = v.as_str() {
        if !s.starts_with('#') {
            out.push(strip_ext(s));
        }
        return;
    }
    if let Some(arr) = v.as_array() {
        for x in arr {
            collect_paths(x, out);
        }
        return;
    }
    if v.as_object().is_some() {
        for key in ["textures", "path", "variations"] {
            if let Some(x) = v.get(key) {
                collect_paths(x, out);
            }
        }
    }
}

/// Drop a real image extension only. A plain `rsplit('.')` is wrong here:
/// `entity/dexter/ac.dexter_head_hanged` has no extension, and cutting at the
/// dot would leave `entity/dexter/ac`, which does not exist.
fn strip_ext(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    for ext in [".png", ".tga", ".jpg", ".jpeg"] {
        if lower.ends_with(ext) {
            return s[..s.len() - ext.len()].to_string();
        }
    }
    s.to_string()
}
