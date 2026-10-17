//! Orphan pass: texture and sound files that nothing uses.
//!
//! "When in doubt, do not report" decides every rule here. A file is used if:
//!
//! - any text file of any pack in the world (JSON, scripts, functions,
//!   materials) contains its path as a string — keys included, comments
//!   included, unreadable JSON included: the text is scanned for quoted
//!   strings, not parsed, so a reference can only be over-counted, never lost;
//! - a string names a folder it lives in (`"textures/entity/zombie/"`), or a
//!   script string is a prefix of it (`"textures/entity/" + name`): a path
//!   built at runtime protects everything it could build;
//! - a `*.texture_set.json` next to it names it (PBR layers are relative names);
//! - the game ships or names that path: the pack is replacing the game's file,
//!   and nothing in the pack has to name it for it to be used.
//!
//! Lists of files are not uses: `textures_list.json` (a preload list) and
//! `contents.json` (a manifest of the pack's files) name everything.

use crate::filesystem::{all_files, asset_stem, is_asset};
use crate::json::{self, Json};
use crate::manifest::{Pack, PackKind};
use crate::par::par_map;
use crate::vanilla::VanillaCatalog;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusedAsset {
    pub pack: String,
    /// Pack-relative path, as on disk.
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Default)]
pub struct UnusedReport {
    /// Texture and sound files looked at.
    pub scanned: usize,
    /// Of those, the ones the game uses without the pack naming them: they
    /// replace a file the game ships or names, or sit where the engine reads.
    pub replace_game: usize,
    pub unused: Vec<UnusedAsset>,
}

impl UnusedReport {
    pub fn bytes(&self) -> u64 {
        self.unused.iter().map(|u| u.bytes).sum()
    }
}

/// Paths the engine reads by convention, with no file naming them and no
/// default in the game for the catalog to see. `overworld_cubemap/cubemap_0..5`
/// is a custom sky: a pack only has to put the six faces there.
const ENGINE_READS: [&str; 1] = ["textures/environment/overworld_cubemap/"];

/// Text files whose quoted strings count as references.
const TEXT_EXTS: [&str; 6] = [".json", ".js", ".ts", ".mcfunction", ".material", ".lang"];
/// Files that list assets without using them.
const LISTS: [&str; 2] = ["textures_list.json", "contents.json"];

pub fn run(packs: &[Pack], catalog: &VanillaCatalog) -> UnusedReport {
    let refs = References::collect(packs);
    let mut report = UnusedReport::default();
    for p in packs
        .iter()
        .filter(|p| p.manifest.kind == PackKind::Resources)
    {
        for dir in ["textures", "sounds"] {
            for f in all_files(&p.root.join(dir)) {
                let Ok(rel) = f.strip_prefix(&p.root) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if !is_asset(&rel.to_ascii_lowercase()) {
                    continue;
                }
                report.scanned += 1;
                let stem = asset_stem(&rel);
                if catalog.has_asset_stem(&stem) || ENGINE_READS.iter().any(|p| stem.starts_with(p))
                {
                    report.replace_game += 1;
                    continue;
                }
                if refs.covers(&stem) {
                    continue;
                }
                let bytes = fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                report.unused.push(UnusedAsset {
                    pack: p.folder.clone(),
                    path: rel,
                    bytes,
                });
            }
        }
    }
    report
        .unused
        .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    report
}

#[derive(Default)]
struct References {
    exact: HashSet<String>,
    prefixes: Vec<String>,
}

impl References {
    fn collect(packs: &[Pack]) -> Self {
        let mut files = Vec::new();
        for p in packs {
            for f in all_files(&p.root) {
                let name = f
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if LISTS.contains(&name.as_str()) || !TEXT_EXTS.iter().any(|e| name.ends_with(e)) {
                    continue;
                }
                files.push((f, p.root.clone(), name));
            }
        }
        // every file on its own core; the sets are merged afterwards
        let parts = par_map(&files, |(f, root, name)| {
            let mut r = Self::default();
            let Ok(bytes) = crate::snapshot::read(f) else {
                return r;
            };
            let text = String::from_utf8_lossy(&bytes);
            let is_script = name.ends_with(".js") || name.ends_with(".ts");
            for s in quoted_strings(&text) {
                r.add(s, is_script);
            }
            if name.ends_with(".texture_set.json") {
                r.add_texture_set(f, root, &text);
            }
            r
        });
        let mut out = Self::default();
        for p in parts {
            out.exact.extend(p.exact);
            out.prefixes.extend(p.prefixes);
        }
        out
    }

    /// Every asset path inside one string literal. A literal that is exactly a
    /// path is an exact reference. A path embedded in more text — a UI binding
    /// `"('textures/ui/maps/' + #name)"`, a template `` `textures/x/${n}` `` —
    /// is the fixed part of a path built at runtime: everything under it is
    /// protected. So is a path ending in `/`, and any path in a script.
    fn add(&mut self, s: &str, is_script: bool) {
        // Most literals are not paths: rule them out before allocating. A
        // backslash path (`textures\x`) is checked too, so nothing is lost.
        if !(contains_ci(s, b"textures") || contains_ci(s, b"sounds")) {
            return;
        }
        let lower = s.to_ascii_lowercase().replace('\\', "/");
        for root in ["textures/", "sounds/"] {
            let mut from = 0;
            while let Some(at) = lower[from..].find(root) {
                let start = from + at;
                let run_len = lower[start..]
                    .find(|c: char| !(c.is_ascii_alphanumeric() || "_-./ ".contains(c)))
                    .unwrap_or(lower.len() - start);
                let run = &lower[start..start + run_len];
                let whole = lower.trim() == run.trim_end();
                let stem = asset_stem(run);
                if !whole || is_script || stem.ends_with('/') {
                    self.prefixes.push(stem.clone());
                }
                self.exact.insert(stem);
                from = start + root.len();
            }
        }
    }

    /// `{"minecraft:texture_set": {"color": "stone", "normal": "stone_n"}}`:
    /// names relative to the texture set's own folder.
    fn add_texture_set(&mut self, file: &Path, pack_root: &Path, text: &str) {
        let Ok(root) = json::parse(text) else { return };
        let Some(set) = root.get("minecraft:texture_set").and_then(Json::as_object) else {
            return;
        };
        let Some(folder) = file.parent().and_then(|d| d.strip_prefix(pack_root).ok()) else {
            return;
        };
        let folder = folder.to_string_lossy().replace('\\', "/");
        for name in set.values().filter_map(Json::as_str) {
            self.exact.insert(asset_stem(&format!("{folder}/{name}")));
        }
    }

    fn covers(&self, stem: &str) -> bool {
        self.exact.contains(stem) || self.prefixes.iter().any(|p| stem.starts_with(p.as_str()))
    }
}

/// `needle` (lowercase ASCII) somewhere in `hay`, ignoring ASCII case, without
/// allocating.
fn contains_ci(hay: &str, needle: &[u8]) -> bool {
    hay.as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// Every `"..."`, `'...'` and `` `...` `` literal in `text`, escapes kept raw.
/// Deliberately naive: a false string only adds a reference, which can only
/// hide an orphan, never invent one. Each quote kind is scanned on its own and
/// the results merged, so an apostrophe in `// don't use "textures/a"` cannot
/// swallow the real string after it.
fn quoted_strings(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for &q in b"\"'`" {
        out.extend(quoted_by(text, q));
    }
    out
}

fn quoted_by(text: &str, q: u8) -> Vec<&str> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == q {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != q && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = j.min(b.len());
            if let Some(s) = text.get(start..end) {
                out.push(s);
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_strings_of_every_kind() {
        let t = r#"{"a": "textures/x"} // 'textures/y' `textures/z/${n}` "esc\"aped""#;
        let s = quoted_strings(t);
        assert!(s.contains(&"textures/x"));
        assert!(s.contains(&"textures/y"));
        assert!(s.contains(&"textures/z/${n}"));
        assert!(s.contains(&r#"esc\"aped"#));
    }

    /// A real pair from the reference world: the atlas names
    /// `"textures/items/food/raw_burger "` and the file is `raw_burger .png`.
    /// Deleting it as "unused" broke a reference; the repair's second analysis
    /// caught it and rolled back.
    #[test]
    fn a_space_before_the_extension_is_the_same_file() {
        let mut r = References::default();
        r.add("textures/items/food/raw_burger ", false);
        assert!(r.covers(&asset_stem("textures/items/food/raw_burger .png")));
    }

    #[test]
    fn paths_built_at_runtime_protect_what_they_could_build() {
        let mut r = References::default();
        r.add("textures/blocks/stone", false);
        r.add("('textures/ui/maps/' + #name)", false);
        r.add("textures/ram/saved_locations/${dimName}", true);
        r.add("sounds/mob/zombie/", false);
        assert!(r.covers("textures/blocks/stone"));
        assert!(
            !r.covers("textures/blocks/stone_brick"),
            "an exact path protects only itself"
        );
        assert!(r.covers("textures/ui/maps/desert"));
        assert!(r.covers("textures/ram/saved_locations/overworld"));
        assert!(r.covers("sounds/mob/zombie/say1"));
        assert!(!r.covers("sounds/mob/skeleton/say1"));
    }

    #[test]
    fn an_apostrophe_does_not_hide_a_string() {
        let s = quoted_strings(r#"// don't use "textures/a" here"#);
        assert!(s.contains(&"textures/a"));
    }

    #[test]
    fn unterminated_quote_does_not_panic() {
        let _ = quoted_strings("\"abc");
        let _ = quoted_strings("\"abc\\");
        let _ = quoted_strings("'");
    }
}
