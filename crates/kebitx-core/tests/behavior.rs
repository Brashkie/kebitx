//! Behavior packs. `behavior_good` names everything in every way there is, and
//! nothing may be reported; `behavior_broken` breaks each one, and each must be.

mod common;
use common::fixture;
use kebitx_core::analyze::analyze_with;
use kebitx_core::behavior::{self, is_behavior_pack, LoadedPack, PackFile, Scope};
use kebitx_core::report::{AnalysisReport, Issue};
use kebitx_core::vanilla::files::{map_files_in, PackKind};
use kebitx_core::vanilla::{install_dir, VanillaCatalog};
use std::collections::HashSet;

fn game() -> Option<VanillaCatalog> {
    let install = install_dir()?;
    Some(VanillaCatalog::from_install(&install))
}

fn dead(r: &AnalysisReport) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = r
        .issues
        .iter()
        .filter_map(|i| match i {
            Issue::Dead {
                kind, reference, ..
            } => Some((kind.to_string(), reference.clone())),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

fn missing(r: &AnalysisReport) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = r
        .issues
        .iter()
        .filter_map(|i| match i {
            Issue::Missing {
                kind, reference, ..
            } => Some((kind.to_string(), reference.clone())),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_correct_pack_has_nothing_reported() {
    let r = analyze_with(&fixture("behavior_good"), &VanillaCatalog::empty());
    assert!(r.behavior_pack);
    assert_eq!(
        missing(&r),
        Vec::<(String, String)>::new(),
        "{:#?}",
        r.issues
    );
    assert!(r.issues.is_empty(), "{:#?}", r.issues);
    assert_eq!(r.broken(), 0);
    // the game's tables, and another pack's item: not proven, not reported
    assert_eq!(r.behavior_unverified, 3);
    assert!(r.behavior_refs > 30, "{}", r.behavior_refs);
}

#[test]
fn every_kind_of_break_is_found() {
    let r = analyze_with(&fixture("behavior_broken"), &VanillaCatalog::empty());
    let got = missing(&r);
    let has = |kind: &str, reference: &str| got.iter().any(|(k, v)| k == kind && v == reference);
    let gone = dead(&r);
    for (kind, reference) in [
        ("component group", "no_such_group"),
        ("event", "kx:no_such_event"),
        ("event", "kx:no_such_trigger"),
        ("item", "kx:no_such_item"),
        ("item", "kx:no_such_result"),
        ("block", "kx:no_such_block"),
        ("entity", "kx:no_such_mob"),
        ("entity", "kx:no_such_spawn"),
        ("feature", "kx:no_such_feature"),
    ] {
        assert!(
            gone.iter().any(|(k, v)| k == kind && v == reference),
            "{kind} {reference} in {gone:#?}"
        );
    }
    assert_eq!(r.behavior_dead, gone.len());
    for (kind, reference) in [
        ("script file", "scripts/gone.js"),
        ("script file", "scripts/gone_import.js"),
        ("script module", "@minecraft/server-ui"),
        ("function", "functions/kx/gone.mcfunction"),
        ("function", "functions/kx/gone_from_tick.mcfunction"),
        ("function", "functions/kx/gone_from_event.mcfunction"),
        ("behavior controller", "controller.animation.kx.gone"),
        ("behavior animation", "animation.kx.gone"),
        ("animation name", "undeclared"),
        ("recipe key", "X"),
    ] {
        assert!(has(kind, reference), "{kind} {reference} in {got:#?}");
    }
    // no game: tables may be the game's, so they are not judged
    assert!(
        !gone
            .iter()
            .any(|(k, _)| k == "loot table" || k == "trade table"),
        "{gone:#?}"
    );
    // a namespace nobody here owns may be a pack that is not here
    assert!(
        !gone.iter().any(|(_, v)| v.starts_with("other_pack:")),
        "{gone:#?}"
    );
    assert_eq!(r.broken(), got.len());
    // works on Windows, fails on phones
    assert_eq!(r.case_mismatches, 2, "{:#?}", r.issues);
    assert!(r.issues.iter().any(
        |i| matches!(i, Issue::CaseMismatch { on_disk, .. } if on_disk == "scripts/helper.js")
    ));
    assert!(r.issues.iter().any(|i| matches!(i, Issue::CaseMismatch { on_disk, .. } if on_disk == "loot_tables/kx/real.json")));
    // a file that cannot be read is said, and is no break
    assert_eq!(r.unreadable, 1);
}

#[test]
fn a_resource_pack_is_not_a_behavior_pack() {
    assert!(is_behavior_pack(&fixture("behavior_good")));
    assert!(!is_behavior_pack(&fixture("atlas_pack")));
    assert!(!analyze_with(&fixture("atlas_pack"), &VanillaCatalog::empty()).behavior_pack);
}

/// The game's own files are correct by definition. Anything said about them is a
/// mistake of this checker. Skipped where the game is not installed.
#[test]
fn the_games_own_files_have_nothing_reported() {
    let (Some(install), Some(catalog)) = (install_dir(), game()) else {
        return;
    };
    let wanted = |p: &str| {
        p.ends_with(".json")
            && [
                "entities/",
                "items/",
                "blocks/",
                "recipes/",
                "loot_tables/",
                "trading/",
                "spawn_rules/",
                "feature_rules/",
                "features/",
            ]
            .iter()
            .any(|f| p.starts_with(f))
    };
    let files = map_files_in(&install, PackKind::Behavior, wanted, |_, bytes| {
        String::from_utf8(bytes.to_vec()).ok()
    });
    assert!(files.len() > 500, "only {} files", files.len());
    let paths: Vec<String> = files.keys().cloned().collect();
    let pack = LoadedPack {
        name: "vanilla".into(),
        files: files
            .into_iter()
            .map(|(rel, text)| PackFile {
                rel,
                text: Ok(text),
            })
            .collect(),
        paths,
        script_entries: Vec::new(),
        script_modules: HashSet::new(),
    };
    let scope = Scope::of(std::slice::from_ref(&pack), false);
    let r = behavior::check_pack(&pack, &scope, &catalog);
    assert!(r.behavior_refs > 5000, "{}", r.behavior_refs);
    assert_eq!(r.unreadable, 0, "{:#?}", r.issues);
    // nothing the game does may read as broken
    assert_eq!(missing(&r), Vec::<(String, String)>::new());
    assert_eq!(r.broken(), 0);
    assert_eq!(r.case_mismatches, 0, "{:#?}", r.issues);
    // what it does carry on from: only these kinds, the reason they are not breaks
    for (kind, reference) in dead(&r) {
        assert!(
            ["component group", "event", "loot table"].contains(&kind.as_str()),
            "{kind} {reference}"
        );
    }
    // the tables really were looked up in the game
    assert!(r.behavior_resolved + r.behavior_external > 5000);
    assert!(catalog.has_behavior_file("loot_tables/chests/simple_dungeon.json"));
}

/// With the game, a table that is nowhere is known to be nowhere.
#[test]
fn with_the_game_a_missing_table_is_dead() {
    let Some(catalog) = game() else { return };
    let r = analyze_with(&fixture("behavior_broken"), &catalog);
    let gone = dead(&r);
    for (kind, reference) in [
        ("loot table", "loot_tables/kx/gone.json"),
        ("loot table", "loot_tables/kx/gone_nested.json"),
        ("trade table", "trading/gone.json"),
    ] {
        assert!(
            gone.iter().any(|(k, v)| k == kind && v == reference),
            "{gone:#?}"
        );
    }
    let good = analyze_with(&fixture("behavior_good"), &catalog);
    assert_eq!(good.issues.len(), 0, "{:#?}", good.issues);
    assert_eq!(good.behavior_unverified, 1);
}
