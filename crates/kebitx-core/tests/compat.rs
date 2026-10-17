//! Compatibility between the packs of a world, on `world_compat`: a real
//! world layout (`behavior_packs/`, `resource_packs/`, `world_*_packs.json`)
//! with one instance of every defect, and an identical-copy of each kind of
//! duplicate that must NOT be reported.

mod common;
use common::fixture;
use kebitx_core::analyze::resource_packs_in;
use kebitx_core::compat::{check_world, CompatIssue};

const GAME: [u32; 3] = [1, 26, 52];

fn issues() -> Vec<CompatIssue> {
    check_world(&fixture("world_compat"), Some(GAME)).issues
}

#[test]
fn errors_the_game_would_hit() {
    let r = check_world(&fixture("world_compat"), Some(GAME));
    assert_eq!(r.packs, 3);
    let i = &r.issues;
    assert!(i.contains(&CompatIssue::DuplicateUuid {
        uuid: "aaaaaaaa-0000-0000-0000-000000000002".into(),
        places: vec!["bp_a (module)".into(), "bp_b (header)".into()],
    }));
    assert!(i.contains(&CompatIssue::EngineTooNew {
        pack: "bp_a".into(),
        needs: [1, 30, 0],
        game: GAME
    }));
    assert!(i.contains(&CompatIssue::MissingDependency {
        pack: "bp_a".into(),
        uuid: "dddddddd-0000-0000-0000-00000000dead".into(),
    }));
    assert!(i.contains(&CompatIssue::MissingListedPack {
        list: "world_behavior_packs.json".into(),
        uuid: "eeeeeeee-0000-0000-0000-000000000404".into(),
    }));
    // the string version "1.20.0", the upper-case listed UUID and the
    // `@minecraft/server` module dependency are all fine
    assert_eq!(r.errors(), 4, "{i:#?}");
}

#[test]
fn warnings_only_when_the_versions_differ() {
    let i = issues();
    assert!(i.contains(&CompatIssue::NotActivated {
        pack: "rp_a".into()
    }));
    assert!(i.contains(&CompatIssue::ConflictingFile {
        path: "loot_tables/chest.json".into(),
        packs: vec!["bp_a".into(), "bp_b".into()],
    }));
    assert!(i.contains(&CompatIssue::ConflictingDefinition {
        kind: "entity",
        id: "test:z".into(),
        places: vec!["bp_a/entities/z.json".into(), "bp_b/entities/z.json".into()],
    }));
    // loot_tables/same.json and item test:same are identical copies (the item
    // differs only in comments, formatting and key order): not reported
    let warnings = i
        .iter()
        .filter(|x| x.severity() == kebitx_core::compat::Severity::Warning)
        .count();
    assert_eq!(warnings, 3, "{i:#?}");
}

#[test]
fn unknown_game_version_skips_the_engine_check() {
    let r = check_world(&fixture("world_compat"), None);
    assert!(!r
        .issues
        .iter()
        .any(|x| matches!(x, CompatIssue::EngineTooNew { .. })));
}

#[test]
fn real_world_layout_finds_the_resource_packs() {
    let rps = resource_packs_in(&fixture("world_compat"));
    assert_eq!(rps.len(), 1);
    assert!(rps[0].ends_with("resource_packs/rp_a"));
}
