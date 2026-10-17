//! Orphan pass for definitions: geometry, animations, animation controllers and
//! render controllers of resource packs that nothing names.
//!
//! Same rule as `unused_asset` — when in doubt, do not report. A definition is
//! used if:
//!
//! - any text file of any pack in the world names it, except the files of its
//!   own kind (a model file "names" the geometry it defines; that is not a
//!   use). Identifiers are found inside literals too, so an identifier inside
//!   a longer string still counts;
//! - another model inherits from it (`"geometry.child:geometry.parent"`);
//! - the game defines or uses the same identifier: the pack is overriding it,
//!   and the game's own entities use it.
//!
//! Comparison ignores case: a false match can only hide an orphan.

use crate::filesystem::all_files;
use crate::json::{self, Json};
use crate::manifest::{Pack, PackKind};
use crate::par::par_map;
use crate::vanilla::VanillaCatalog;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefKind {
    Geometry,
    Animation,
    AnimationController,
    RenderController,
}

impl DefKind {
    pub fn name(self) -> &'static str {
        match self {
            DefKind::Geometry => "geometry",
            DefKind::Animation => "animation",
            DefKind::AnimationController => "animation controller",
            DefKind::RenderController => "render controller",
        }
    }
    /// The resource-pack folder whose files define this kind.
    fn folder(self) -> &'static str {
        match self {
            DefKind::Geometry => "models",
            DefKind::Animation => "animations",
            DefKind::AnimationController => "animation_controllers",
            DefKind::RenderController => "render_controllers",
        }
    }
    const ALL: [DefKind; 4] = [
        DefKind::Geometry,
        DefKind::Animation,
        DefKind::AnimationController,
        DefKind::RenderController,
    ];
}

/// Files whose text can name a definition: data, scripts, and functions.
const TEXT_EXTS: [&str; 4] = [".json", ".js", ".ts", ".mcfunction"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusedDefinition {
    pub kind: DefKind,
    pub id: String,
    /// `pack/relative/file` where it is defined.
    pub file: String,
}

#[derive(Debug, Default)]
pub struct UnusedDefinitions {
    pub defined: usize,
    pub unused: Vec<UnusedDefinition>,
}

pub fn run(packs: &[Pack], catalog: &VanillaCatalog) -> UnusedDefinitions {
    // Identifiers named per category: the definition folder a file sits in, or
    // `None` for every other file (entities, attachables, blocks, BP, ...).
    let mut named: HashMap<Option<DefKind>, HashSet<String>> = HashMap::new();
    let mut inherited: HashSet<String> = HashSet::new();
    let mut defs: Vec<(DefKind, String, String)> = Vec::new();

    // every text file of the world, with the definition folder it sits in
    let mut files: Vec<(&Pack, PathBuf, Option<DefKind>)> = Vec::new();
    for p in packs {
        let rp = p.manifest.kind == PackKind::Resources;
        for f in all_files(&p.root) {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            // `.mcfunction`: `/playanimation @s animation.gun.fire` plays an
            // animation that no entity file names
            if !TEXT_EXTS.iter().any(|e| name.ends_with(e)) {
                continue;
            }
            let category = if rp { kind_of_file(&p.root, &f) } else { None };
            files.push((p, f, category));
        }
    }

    // One read per file, on every core: the identifiers it names, and — for a
    // definition file — the ones it defines.
    type Defined = Vec<(DefKind, String, Option<String>)>;
    let parts: Vec<(Option<DefKind>, Vec<String>, Defined, String)> =
        par_map(&files, |(p, f, category)| {
            let Ok(bytes) = crate::snapshot::read(f) else {
                return (*category, Vec::new(), Vec::new(), String::new());
            };
            let text = String::from_utf8_lossy(&bytes);
            let ids = identifiers_in(&text);
            let mut defined = Vec::new();
            let is_json = f
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"));
            if let (Some(kind), true) = (category, is_json) {
                if let Ok(root) = json::parse(&text) {
                    defined = defined_in(*kind, &root)
                        .into_iter()
                        .map(|(id, parent)| (*kind, id, parent))
                        .collect();
                }
            }
            let rel = f
                .strip_prefix(&p.root)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/");
            (*category, ids, defined, format!("{}/{rel}", p.folder))
        });
    for (category, ids, defined, place) in parts {
        named.entry(category).or_default().extend(ids);
        for (kind, id, parent) in defined {
            if let Some(parent) = parent {
                inherited.insert(parent.to_ascii_lowercase());
            }
            defs.push((kind, id, place.clone()));
        }
    }

    let mut report = UnusedDefinitions {
        defined: defs.len(),
        ..Default::default()
    };
    for (kind, id, file) in defs {
        let lower = id.to_ascii_lowercase();
        let named_elsewhere = named
            .iter()
            .any(|(cat, set)| *cat != Some(kind) && set.contains(&lower));
        let used = named_elsewhere
            || (kind == DefKind::Geometry && inherited.contains(&lower))
            || by_game(kind, &id, catalog);
        if !used {
            report.unused.push(UnusedDefinition { kind, id, file });
        }
    }
    report
        .unused
        .sort_by(|a, b| (a.kind.name(), &a.file, &a.id).cmp(&(b.kind.name(), &b.file, &b.id)));
    report
}

fn by_game(kind: DefKind, id: &str, catalog: &VanillaCatalog) -> bool {
    match kind {
        DefKind::Geometry => catalog.has_geometry(id),
        DefKind::Animation => catalog.has_animation(id),
        DefKind::AnimationController => catalog.has_animation_controller(id),
        DefKind::RenderController => catalog.has_render_controller(id),
    }
}

/// Which definition folder a resource-pack file sits in, if any.
fn kind_of_file(pack_root: &Path, f: &Path) -> Option<DefKind> {
    let rel = f.strip_prefix(pack_root).ok()?;
    let top = rel
        .components()
        .next()?
        .as_os_str()
        .to_string_lossy()
        .to_ascii_lowercase();
    DefKind::ALL.into_iter().find(|k| k.folder() == top)
}

/// Identifiers a file defines, each with the parent it inherits from (old
/// model format only).
fn defined_in(kind: DefKind, root: &Json) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    match kind {
        DefKind::Geometry => {
            for g in root
                .get("minecraft:geometry")
                .and_then(Json::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if let Some(id) = g
                    .get("description")
                    .and_then(|d| d.get("identifier"))
                    .and_then(Json::as_str)
                {
                    out.push((id.to_string(), None));
                }
            }
            for k in root.as_object().into_iter().flat_map(|o| o.keys()) {
                if k.starts_with("geometry.") {
                    let mut parts = k.splitn(2, ':');
                    let id = parts.next().unwrap_or(k).trim().to_string();
                    let parent = parts.next().map(|s| s.trim().to_string());
                    out.push((id, parent));
                }
            }
        }
        DefKind::Animation => keys(root, "animations", &mut out),
        DefKind::AnimationController => keys(root, "animation_controllers", &mut out),
        DefKind::RenderController => keys(root, "render_controllers", &mut out),
    }
    out
}

fn keys(root: &Json, section: &str, out: &mut Vec<(String, Option<String>)>) {
    for k in root
        .get(section)
        .and_then(Json::as_object)
        .into_iter()
        .flat_map(|o| o.keys())
    {
        out.push((k.clone(), None));
    }
}

/// Every identifier-looking token in `text`: a run starting at `geometry.`,
/// `animation.` or `controller.`, lowercased. Both sides of a `:` count, so
/// `"geometry.a:geometry.b"` names both. Works on raw text, so comments,
/// keys and unparseable files count too.
fn identifiers_in(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    for root in ["geometry.", "animation.", "controller."] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(root) {
            let start = from + at;
            from = start + root.len();
            // a token starts at a boundary: `animation.x` inside
            // `controller.animation.x` is the controller, not the animation
            let prev = lower[..start].chars().next_back();
            if prev.is_some_and(|c| c.is_ascii_alphanumeric() || "_.".contains(c)) {
                continue;
            }
            let len = lower[start..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || "_.-:".contains(c)))
                .unwrap_or(lower.len() - start);
            for part in lower[start..start + len].split(':') {
                let part = part.trim_end_matches('.');
                if !part.is_empty() {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_found_anywhere_in_the_text() {
        let ids = identifiers_in(
            r#"{"geometry": {"default": "Geometry.Cow"}, "x": "q ? animation.a.walk : 0"}
                                    "geometry.child:geometry.parent" // controller.render.old"#,
        );
        for id in [
            "geometry.cow",
            "animation.a.walk",
            "geometry.child",
            "geometry.parent",
            "controller.render.old",
        ] {
            assert!(ids.contains(&id.to_string()), "{id} in {ids:?}");
        }
    }

    #[test]
    fn a_token_inside_another_is_not_a_reference() {
        let ids = identifiers_in(r#""controller.animation.x.walk", "my_geometry.y""#);
        assert_eq!(ids, ["controller.animation.x.walk"]);
    }
}
