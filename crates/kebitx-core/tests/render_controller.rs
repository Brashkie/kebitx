//! The slots a render controller asks of each entity that uses it.
//!
//! Fixture `world_rc`: `rp_main` has one entity declaring `default` for
//! texture, geometry and material, and two controllers — one healthy, one with
//! every shape of defect. `rp_override` redefines the broken controller, so in
//! world mode the definitions disagree on some slots.

mod common;
use common::fixture;
use kebitx_core::analyze::{analyze_with, analyze_world_with};
use kebitx_core::report::{AnalysisReport, Issue};
use kebitx_core::vanilla::VanillaCatalog;

/// Empty catalog on purpose: a fixture must not change its verdict
/// depending on which Minecraft happens to be installed on the machine.
static VACIO: std::sync::LazyLock<VanillaCatalog> = std::sync::LazyLock::new(VanillaCatalog::empty);

fn missing_slots(r: &AnalysisReport) -> Vec<String> {
    let mut v: Vec<String> = r
        .issues
        .iter()
        .filter_map(|i| match i {
            Issue::Missing {
                kind, reference, ..
            } if kind.ends_with("slot") => Some(reference.clone()),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_shape_gets_the_verdict_the_evidence_allows() {
    let r = analyze_with(&fixture("world_rc/rp_main"), &VACIO);
    // ok: Geometry.default, Material.default, Texture.default (texture.DEFAULT is
    // the same name: Molang ignores case, so it is one reference)
    // bad: 8 distinct values
    assert_eq!(r.slots_checked, 11);
    assert_eq!(r.slots_resolved, 3);
    assert_eq!(
        r.slots_bounded, 1,
        "Array.skins: one candidate present, one not"
    );
    assert_eq!(r.slots_opaque, 2, "the ternary, and the per-bone material");
    assert_eq!(
        missing_slots(&r),
        [
            "controller.render.painted_bad: Array.undefined[0]",
            "controller.render.painted_bad: Geometry.missing",
            "controller.render.painted_bad: Material.nope",
            "controller.render.painted_bad: Texture.nope",
            "controller.render.painted_bad: array.none[query.variant]",
        ]
    );
    assert_eq!(r.slots_missing, 5);
}

#[test]
fn disagreeing_definitions_keep_only_what_is_missing_under_all() {
    let packs = analyze_world_with(&fixture("world_rc"), &VACIO);
    let r = &packs.iter().find(|(n, _)| n == "rp_main").unwrap().1;
    // rp_override's definition also lacks Texture.nope and Material.nope, but
    // not the others: which definition wins depends on pack order.
    assert_eq!(
        missing_slots(r),
        [
            "controller.render.painted_bad: Material.nope",
            "controller.render.painted_bad: Texture.nope",
        ]
    );
    assert_eq!(r.slots_opaque, 5);
}
