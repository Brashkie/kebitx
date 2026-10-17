//! The vanilla catalog, built from a fake install instead of the real game, so
//! the verdict is the same on every machine.
//!
//! `fake_install` has one render controller in its resource packs and one
//! animation controller in its behavior packs. `copied_cow_pack` is what an
//! author gets by copying the cow entity and trimming it too far.

mod common;
use common::fixture;
use kebitx_core::analyze::analyze_with;
use kebitx_core::report::Issue;
use kebitx_core::vanilla::VanillaCatalog;

#[test]
fn game_render_controllers_have_their_slots_checked() {
    let catalog = VanillaCatalog::from_install(&fixture("fake_install"));
    let r = analyze_with(&fixture("copied_cow_pack"), &catalog);

    assert_eq!(
        r.render_controllers_external, 1,
        "controller.render.cow comes from the game"
    );
    assert_eq!(r.slots_checked, 3);
    assert_eq!(r.slots_resolved, 1, "Geometry.default is declared");
    let mut missing: Vec<&str> = r
        .issues
        .iter()
        .filter_map(|i| match i {
            Issue::Missing {
                kind, reference, ..
            } if kind.ends_with("slot") => Some(reference.as_str()),
            _ => None,
        })
        .collect();
    missing.sort();
    assert_eq!(
        missing,
        [
            "controller.render.cow: Material.default",
            "controller.render.cow: Texture.default"
        ]
    );
}

#[test]
fn behavior_pack_of_the_game_does_not_satisfy_the_client() {
    let catalog = VanillaCatalog::from_install(&fixture("fake_install"));
    assert!(catalog.has_render_controller("controller.render.cow"));
    assert!(!catalog.has_animation_controller("controller.animation.server_only"));

    let r = analyze_with(&fixture("copied_cow_pack"), &catalog);
    assert_eq!(r.animations_external, 0);
    assert_eq!(r.animations_missing, 1);
}
