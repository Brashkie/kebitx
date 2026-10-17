//! Identifier-space resolution for animations, animation controllers and render
//! controllers, over a world with one resource pack and one behavior pack.
//!
//! The fixture reproduces a defect found in a real world: a client entity names
//! an animation controller that only a BEHAVIOR pack defines. The client never
//! sees behavior packs, so that reference is missing — it must not be satisfied
//! by the BP definition.

mod common;
use common::fixture;
use kebitx_core::analyze::analyze_world_with;
use kebitx_core::report::{AnalysisReport, Issue};
use kebitx_core::vanilla::VanillaCatalog;

/// Empty catalog on purpose: a fixture must not change its verdict
/// depending on which Minecraft happens to be installed on the machine.
static VACIO: std::sync::LazyLock<VanillaCatalog> = std::sync::LazyLock::new(VanillaCatalog::empty);

fn world() -> AnalysisReport {
    let packs = analyze_world_with(&fixture("world_anims"), &VACIO);
    assert_eq!(packs.len(), 1, "only the resource pack is analysed");
    packs.into_iter().next().unwrap().1
}

fn missing(r: &AnalysisReport, kind: &str) -> Vec<String> {
    r.issues
        .iter()
        .filter_map(|i| match i {
            Issue::Missing {
                kind: k, reference, ..
            } if *k == kind => Some(reference.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn animations_resolve_in_either_index() {
    let r = world();
    // 4 in the animations map + 1 in the legacy controllers array
    assert_eq!(r.animations_declared, 5);
    // walk (animation), move (controller via the map), move (legacy array)
    assert_eq!(r.animations_resolved, 3);
    assert_eq!(r.animations_external, 0);
}

#[test]
fn behavior_pack_controller_does_not_satisfy_the_client() {
    let r = world();
    let mut m = missing(&r, "animation");
    m.sort();
    assert_eq!(
        m,
        [
            "animation.walker.ghost",
            "controller.animation.walker.thrown"
        ]
    );
    assert_eq!(r.animations_missing, 2);
}

#[test]
fn render_controllers_string_and_object_forms() {
    let r = world();
    assert_eq!(r.render_controllers_declared, 2);
    assert_eq!(r.render_controllers_resolved, 1);
    assert_eq!(missing(&r, "render"), ["controller.render.walker_missing"]);
    assert_eq!(r.broken(), 3);
}
