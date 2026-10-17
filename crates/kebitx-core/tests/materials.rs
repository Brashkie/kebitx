//! Materials named by client entities, on `materials_pack`: one the game
//! defines (`entity_alphatest`, in `fake_install`), one the pack defines in
//! `materials/custom.material` (named with other capitals), one nobody defines.

mod common;
use common::fixture;
use kebitx_core::analyze::analyze_with;
use kebitx_core::report::Issue;
use kebitx_core::vanilla::VanillaCatalog;

#[test]
fn a_material_nobody_defines_is_missing() {
    let catalog = VanillaCatalog::from_install(&fixture("fake_install"));
    assert!(catalog.has_material("entity_alphatest"));
    assert!(catalog.has_material("entity_static"));

    let r = analyze_with(&fixture("materials_pack"), &catalog);
    assert_eq!(r.materials_declared, 3);
    assert_eq!(r.materials_resolved, 1, "My_Glow: the pack's, case ignored");
    assert_eq!(r.materials_external, 1, "entity_alphatest: the game's");
    assert_eq!(r.materials_missing, 1);
    assert!(r.issues.contains(&Issue::Missing {
        kind: "material",
        owner: "test:lamp".into(),
        reference: "ghost_material".into(),
    }));
}

#[test]
fn without_the_game_catalog_nothing_is_claimed() {
    let r = analyze_with(&fixture("materials_pack"), &VanillaCatalog::empty());
    assert_eq!(
        r.materials_resolved, 1,
        "the pack's own material is still known"
    );
    assert_eq!(r.materials_unchecked, 2);
    assert_eq!(r.materials_missing, 0);
}
