//! Texture atlases, case sensitivity and world mode.
//!
//! Every case here reproduces a defect found on a real world:
//!   - `variations` not understood -> 86 blocks rebuilt as `"textures": []` (pink)
//!   - `desert_3` declared, `Desert_3.png` on disk -> pink on Android only
//!   - NPC weapon icons pointing at another pack's textures -> "broken" when a
//!     pack is analysed alone, fine in the actual world

mod common;
use common::fixture;
use kebitx_core::analyze::{analyze_with, analyze_world_with};
use kebitx_core::json;
use kebitx_core::parser::atlas::{collect_paths, parse_atlas};
use kebitx_core::report::Issue;
use kebitx_core::vanilla::VanillaCatalog;

/// Empty catalog on purpose: a fixture must not change its verdict depending on
/// which Minecraft happens to be installed on the machine.
static VACIO: std::sync::LazyLock<VanillaCatalog> = std::sync::LazyLock::new(VanillaCatalog::empty);

fn paths_of(src: &str) -> Vec<String> {
    let v = json::parse(src).expect("valid json");
    let mut out = Vec::new();
    collect_paths(&v, &mut out);
    out
}

#[test]
fn every_atlas_shape_yields_its_paths() {
    assert_eq!(paths_of(r#""textures/a""#), ["textures/a"]);
    assert_eq!(
        paths_of(r#"["textures/a", "textures/b"]"#),
        ["textures/a", "textures/b"]
    );
    assert_eq!(
        paths_of(r##"{"path": "textures/a", "overlay_color": "#cef586"}"##),
        ["textures/a"]
    );
    // the shape that a naive reader returns NOTHING for
    assert_eq!(
        paths_of(
            r#"{"variations": [{"path": "textures/a", "weight": 10}, {"path": "textures/b"}]}"#
        ),
        ["textures/a", "textures/b"],
        "variations must not read as empty"
    );
    // one variation set per face
    assert_eq!(
        paths_of(r#"[{"variations": [{"path": "a"}]}, {"variations": [{"path": "b"}]}]"#),
        ["a", "b"]
    );
}

#[test]
fn trailing_commas_are_accepted() {
    // real packs ship them; rejecting them skipped a whole atlas in silence
    assert!(json::parse(r#"{"a": [1, 2,], "b": {"c": 1,},}"#).is_ok());
}

#[test]
fn tint_colours_are_not_paths() {
    assert_eq!(paths_of(r##"["#8ab689", "textures/a"]"##), ["textures/a"]);
}

#[test]
fn a_dot_in_the_name_is_not_an_extension() {
    // cutting at the dot would leave `entity/dexter/ac`, which does not exist
    assert_eq!(
        paths_of(r#""entity/dexter/ac.dexter_head_hanged""#),
        ["entity/dexter/ac.dexter_head_hanged"]
    );
    assert_eq!(paths_of(r#""textures/a.png""#), ["textures/a"]);
}

#[test]
fn parse_atlas_keeps_empty_entries_visible() {
    let src = r#"{ "texture_data": { "ok": {"textures": "a"}, "emptied": {"textures": []} } }"#;
    let entries = parse_atlas(src).unwrap();
    let emptied = entries.iter().find(|e| e.shortname == "emptied").unwrap();
    assert!(
        emptied.paths.is_empty(),
        "an emptied entry must surface, not vanish"
    );
}

#[test]
fn atlas_pack_reports_exactly_the_real_defects() {
    let r = analyze_with(&fixture("atlas_pack"), &VACIO);
    assert_eq!(r.atlas_entries, 8);
    assert_eq!(
        r.atlas_empty, 1,
        "the `[]` entry is a pink block, guaranteed"
    );
    assert_eq!(r.case_mismatches, 1, "desert_3 vs Desert_3.png");
    assert_eq!(
        r.atlas_missing, 0,
        "nothing else is broken: variations and tints resolve"
    );
    assert!(r.issues.iter().any(|i| matches!(i,
        Issue::EmptyTextureSet { shortname, .. } if shortname == "emptied")));
    assert!(r.issues.iter().any(|i| matches!(i,
        Issue::CaseMismatch { on_disk, .. } if on_disk.ends_with("Desert_3.png"))));
}

#[test]
fn world_mode_resolves_across_packs() {
    // alone, rp_a points at a texture it does not have
    let alone = analyze_with(&fixture("world_two_packs/rp_a"), &VACIO);
    assert_eq!(alone.atlas_missing, 1);

    // in the world, rp_b provides it, and the behavior pack is ignored
    let world = analyze_world_with(&fixture("world_two_packs"), &VACIO);
    let names: Vec<&str> = world.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["rp_a", "rp_b"], "only resource packs, sorted");
    let total_broken: usize = world.iter().map(|(_, r)| r.broken()).sum();
    assert_eq!(
        total_broken, 0,
        "a reference satisfied by another pack is not broken"
    );
}
