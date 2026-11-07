//! A world uses what its lists activate, from the game's folders too.

use kebitx_core::compat::{check_world, CompatIssue};
use kebitx_core::manifest::{packs_in, world_packs};
use std::fs;
use std::path::{Path, PathBuf};

fn pack(root: &Path, uuid: &str, kind: &str, depends_on: Option<&str>) {
    fs::create_dir_all(root).unwrap();
    let dependencies = depends_on
        .map(|d| format!(r#","dependencies":[{{"uuid":"{d}","version":[1,0,0]}}]"#))
        .unwrap_or_default();
    fs::write(
        root.join("manifest.json"),
        format!(
            r#"{{"format_version":2,"header":{{"name":"{uuid}","uuid":"{uuid}","version":[1,0,0]}},
                "modules":[{{"type":"{kind}","uuid":"{uuid}-m","version":[1,0,0]}}]{dependencies}}}"#
        ),
    )
    .unwrap();
}

/// The current launcher's layout: worlds per player, packs in `Shared`.
fn game(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("kebitx-lists-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let shared = tmp.join("Users/Shared/games/com.mojang");
    let world = tmp.join("Users/123/games/com.mojang/minecraftWorlds/w");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("level.dat"), "").unwrap();
    (tmp, shared, world)
}

fn ids(world: &Path) -> Vec<String> {
    let mut out: Vec<String> = world_packs(world)
        .into_iter()
        .map(|p| p.manifest.uuid)
        .collect();
    out.sort();
    out
}

#[test]
fn listed_packs_and_what_they_need_come_from_the_games_folders() {
    let (tmp, shared, world) = game("found");
    pack(&world.join("resource_packs/own"), "own", "resources", None);
    pack(
        &shared.join("behavior_packs/a"),
        "a",
        "data",
        Some("needed"),
    );
    pack(&shared.join("behavior_packs/n"), "needed", "data", None);
    pack(&shared.join("resource_packs/b"), "b", "resources", None);
    pack(
        &shared.join("resource_packs/x"),
        "not_listed",
        "resources",
        None,
    );
    fs::write(
        world.join("world_behavior_packs.json"),
        r#"[{"pack_id":"A"}]"#,
    )
    .unwrap();
    fs::write(
        world.join("world_resource_packs.json"),
        r#"[{"pack_id":"b"},{"pack_id":"own"}]"#,
    )
    .unwrap();

    assert_eq!(ids(&world), ["a", "b", "needed", "own"]);
    // what a repair sees stays what the world carries
    assert_eq!(packs_in(&world).len(), 1);

    let compat = check_world(&world, None);
    assert_eq!(compat.issues, Vec::new(), "{:#?}", compat.issues);
    fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn a_pack_installed_nowhere_is_still_missing() {
    let (tmp, _shared, world) = game("missing");
    fs::write(
        world.join("world_resource_packs.json"),
        r#"[{"pack_id":"gone"}]"#,
    )
    .unwrap();
    assert!(check_world(&world, None)
        .issues
        .contains(&CompatIssue::MissingListedPack {
            list: "world_resource_packs.json".into(),
            uuid: "gone".into(),
        }));
    fs::remove_dir_all(&tmp).unwrap();
}

/// A world elsewhere is a copy: it has what it carries, whatever lies around it.
#[test]
fn a_copy_of_a_world_elsewhere_reads_only_itself() {
    let tmp = std::env::temp_dir().join(format!("kebitx-lists-copy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let world = tmp.join("games/com.mojang/copies/w");
    pack(
        &tmp.join("games/com.mojang/resource_packs/b"),
        "b",
        "resources",
        None,
    );
    fs::create_dir_all(&world).unwrap();
    fs::write(
        world.join("world_resource_packs.json"),
        r#"[{"pack_id":"b"}]"#,
    )
    .unwrap();
    assert!(ids(&world).is_empty());
    fs::remove_dir_all(&tmp).unwrap();
}
