//! Unused and duplicated assets, on `world_assets`: one resource pack with a
//! file for every rule that makes an asset "used", two that nothing uses, a
//! second resource pack, and a behavior pack whose script builds sound paths.

mod common;
use common::fixture;
use kebitx_core::manifest::packs_in;
use kebitx_core::passes::unused_definition::{self, DefKind};
use kebitx_core::passes::{duplicate_resource, unused_asset};
use kebitx_core::vanilla::VanillaCatalog;

#[test]
fn only_what_nothing_can_reach_is_unused() {
    let packs = packs_in(&fixture("world_assets"));
    let catalog = VanillaCatalog::from_install(&fixture("fake_install"));
    let r = unused_asset::run(&packs, &catalog);

    let unused: Vec<&str> = r.unused.iter().map(|u| u.path.as_str()).collect();
    // Used, each by a different rule:
    //   used.png      exact reference         Upper.png   reference ignoring case
    //   maps/desert   UI binding prefix        pbr/*       texture set
    //   cubemap_0     read by the engine       blocks/stone replaces the game's
    //   mob/a         sound definition         mob/b       script builds the path
    //   comment.ogg   named in a comment       dup/*       entity textures
    // Unused: one named by nothing, one named only by the preload list.
    // Largest first.
    assert_eq!(
        unused,
        ["textures/listed_only.png", "textures/entity/orphan.png"]
    );
    assert_eq!(r.unused[1].bytes, "orphan bytes".len() as u64);
    assert_eq!(
        r.bytes(),
        "png:textures/listed_only".len() as u64 + "orphan bytes".len() as u64
    );
    assert_eq!(
        r.replace_game, 2,
        "blocks/stone (game file) and cubemap_0 (engine path)"
    );
    assert_eq!(
        r.scanned, 16,
        "12 textures + 3 sounds in rp, 1 texture in rp2"
    );
}

#[test]
fn definitions_nothing_names_are_unused() {
    let packs = packs_in(&fixture("world_assets"));
    let catalog = VanillaCatalog::from_install(&fixture("fake_install"));
    let r = unused_definition::run(&packs, &catalog);

    // 5 geometries, 3 animations, 2 animation controllers, 2 render controllers
    assert_eq!(r.defined, 12);
    // Used: geometry.thing / geometry.child / walk / move / render.thing by the
    // entity; geometry.parent by inheritance; geometry.cow by the game;
    // animation.thing.fire by a function (`/playanimation`).
    let unused: Vec<(DefKind, &str)> = r.unused.iter().map(|u| (u.kind, u.id.as_str())).collect();
    assert_eq!(
        unused,
        [
            (DefKind::Animation, "animation.thing.unused"),
            (
                DefKind::AnimationController,
                "controller.animation.thing.unused"
            ),
            (DefKind::Geometry, "geometry.orphan_geo"),
            (DefKind::RenderController, "controller.render.unused"),
        ]
    );
    assert_eq!(r.unused[2].file, "rp/models/entity/thing.geo.json");
}

#[test]
fn identical_bytes_are_grouped_across_packs_and_proved() {
    let packs = packs_in(&fixture("world_assets"));
    let groups = duplicate_resource::run(&packs);
    // c.png has the same size as a.png but one different byte: not a duplicate.
    // 0-byte placeholders are never grouped.
    assert_eq!(groups.len(), 1, "{groups:#?}");
    assert_eq!(
        groups[0].files,
        [
            "rp/textures/dup/a.png",
            "rp/textures/dup/b.png",
            "rp2/textures/dup/a.png"
        ]
    );
    assert_eq!(groups[0].bytes, 9);
    assert_eq!(groups[0].wasted(), 18);
}
