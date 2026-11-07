//! What Minecraft keeps on this computer, so an app can offer it without anyone
//! having to know where the game hides its folders. Reads names and nothing else.

use crate::Lines;
use kebitx_build::mojang;
use kebitx_core::analyze::is_resource_pack;
use kebitx_core::behavior::is_behavior_pack;
use kebitx_core::filesystem::read_limited;
use kebitx_core::manifest::{pack_at, world_packs};
use kebitx_java::{is_java, is_mod_folder, JavaAssets};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Bedrock's folders, the ones people work on first.
const BEDROCK: [(&str, &str); 5] = [
    ("development_behavior_packs", "Dev behavior pack"),
    ("development_resource_packs", "Dev resource pack"),
    ("minecraftWorlds", "World"),
    ("behavior_packs", "Behavior pack"),
    ("resource_packs", "Resource pack"),
];

/// A name is a line of a text file: this much is more than enough.
const NAME_MAX_BYTES: u64 = 4096;

/// Something that can be analysed.
pub struct Place {
    pub what: &'static str,
    pub name: String,
    pub path: PathBuf,
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// A name as people read it: no colour codes and no tabs. `None` when nothing is
/// left, or it is a translation key and not a name.
fn readable(name: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        match c {
            '§' => {
                chars.next();
            }
            '\t' | '\r' | '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    let out = out.trim();
    (!out.is_empty() && !out.starts_with("pack.")).then(|| out.to_string())
}

fn world_name(world: &Path) -> String {
    read_limited(&world.join("levelname.txt"), NAME_MAX_BYTES)
        .ok()
        .and_then(|bytes| readable(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_else(|| folder_name(world))
}

fn pack_name(pack: &Path) -> String {
    readable(&pack_at(pack).manifest.name).unwrap_or_else(|| folder_name(pack))
}

/// Everything under these game folders that can be given a verdict or read, so a
/// world only if it uses packs or has saved data: the ones people use most first, and by name
/// within each kind.
pub fn find(bedrock: &[PathBuf], java: &[PathBuf]) -> Vec<Place> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |out: &mut Vec<Place>, mut found: Vec<Place>| {
        found.retain(|p| seen.insert(p.path.clone()));
        found.sort_by_key(|p| p.name.to_lowercase());
        out.append(&mut found);
    };

    let mods = java
        .iter()
        .map(|dir| dir.join("mods"))
        .filter(|mods| is_mod_folder(mods))
        .map(|path| Place {
            what: "Mods folder",
            name: kebitx_core::i18n::tr("Java mods").to_string(),
            path,
        })
        .collect();
    add(&mut out, mods);

    for (folder, what) in BEDROCK {
        let is_world = folder == "minecraftWorlds";
        let found = bedrock
            .iter()
            .flat_map(|root| children(&root.join(folder)))
            .filter(|p| {
                if is_world {
                    p.is_dir()
                        && (!world_packs(p).is_empty() || p.join("db").join("CURRENT").is_file())
                } else {
                    is_resource_pack(p) || is_behavior_pack(p)
                }
            })
            .map(|path| Place {
                what,
                name: if is_world {
                    world_name(&path)
                } else {
                    pack_name(&path)
                },
                path,
            })
            .collect();
        add(&mut out, found);
    }

    let packs = java
        .iter()
        .flat_map(|dir| children(&dir.join("resourcepacks")))
        .filter(|p| is_java(p))
        .map(|path| Place {
            what: "Java resource pack",
            name: folder_name(&path),
            path,
        })
        .collect();
    add(&mut out, packs);
    out
}

/// The report of `kebitx_places`: one note per place, as `what\tname\tpath`. A
/// `what` of `Shortcut` is a game folder to browse from, and not to analyse.
pub fn places_json() -> String {
    let java = JavaAssets::launcher_dirs();
    let mut notes: Vec<String> = find(&mojang::all_roots(), &java)
        .into_iter()
        .map(|p| format!("{}\t{}\t{}", p.what, p.name, p.path.display()))
        .collect();
    let shortcuts = mojang::com_mojang()
        .into_iter()
        .chain(java.into_iter().find(|dir| dir.is_dir()));
    notes
        .extend(shortcuts.map(|dir| format!("Shortcut\t{}\t{}", folder_name(&dir), dir.display())));
    Lines {
        kind: "Places".to_string(),
        broken: 0,
        summary: Vec::new(),
        problems: Vec::new(),
        notes,
        extra: Vec::new(),
    }
    .json(Path::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: PathBuf, text: &str) {
        fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        fs::write(path, text).expect("a file");
    }

    fn pack_of(kind: &str, root: &Path, name: &str) {
        write(
            root.join("manifest.json"),
            &format!(
                r#"{{"header":{{"name":"{name}","uuid":"u"}},"modules":[{{"type":"{kind}"}}]}}"#
            ),
        );
    }

    fn pack(root: &Path, name: &str) {
        pack_of("resources", root, name);
    }

    #[test]
    fn finds_what_can_be_analysed_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("kebitx-places-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mojang = dir.join("com.mojang");
        pack(&mojang.join("development_resource_packs/z"), "§aZed");
        pack(&mojang.join("development_resource_packs/a"), "Able");
        pack(&mojang.join("resource_packs/keyed"), "pack.name");
        pack_of("data", &mojang.join("behavior_packs/logic"), "Logic");
        // no manifest: not a pack, whatever its folder says
        fs::create_dir_all(mojang.join("development_resource_packs/empty")).expect("a folder");
        write(mojang.join("minecraftWorlds/Xy12/level.dat"), "");
        write(
            mojang.join("minecraftWorlds/Xy12/levelname.txt"),
            "My world\n",
        );
        pack(
            &mojang.join("minecraftWorlds/Xy12/resource_packs/own"),
            "Own",
        );
        write(mojang.join("minecraftWorlds/nameless/level.dat"), "");
        pack(
            &mojang.join("minecraftWorlds/nameless/resource_packs/own"),
            "Own",
        );
        // a world with no packs in its folder: the same
        write(mojang.join("minecraftWorlds/bare/level.dat"), "");
        // its lists are all it has, and they name an installed pack
        write(
            mojang.join("minecraftWorlds/listed/world_resource_packs.json"),
            r#"[{"pack_id": "u"}]"#,
        );

        let java = dir.join(".minecraft");
        write(java.join("mods/a.jar"), "");
        write(java.join("resourcepacks/faithful.zip"), "");
        write(java.join("resourcepacks/notes.txt"), "");

        // the same root twice stays one list
        let places = find(&[mojang.clone(), mojang], &[java]);
        let got: Vec<(&str, &str)> = places.iter().map(|p| (p.what, p.name.as_str())).collect();
        assert_eq!(
            got,
            [
                ("Mods folder", "Java mods"),
                ("Dev resource pack", "Able"),
                ("Dev resource pack", "Zed"),
                ("World", "listed"),
                ("World", "My world"),
                ("World", "nameless"),
                ("Behavior pack", "Logic"),
                ("Resource pack", "keyed"),
                ("Java resource pack", "faithful.zip"),
            ]
        );
        fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn no_game_is_an_empty_list() {
        let missing = std::env::temp_dir().join("kebitx-places-not-there");
        assert!(find(
            std::slice::from_ref(&missing),
            std::slice::from_ref(&missing)
        )
        .is_empty());
    }
}
