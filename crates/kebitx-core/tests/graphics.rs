//! Graphics diagnostics on `world_graphics`: five models (healthy, box UV
//! overflowing, a flat plane, per-face UV overflowing, no declared canvas) and
//! four entities pairing `geometry.ok` (64 x 32) with images of every kind.

mod common;
use common::fixture;
use kebitx_core::graphics::{check, GraphicsIssue};
use kebitx_core::manifest::packs_in;
use kebitx_core::vanilla::VanillaCatalog;

static VACIO: std::sync::LazyLock<VanillaCatalog> = std::sync::LazyLock::new(VanillaCatalog::empty);

#[test]
fn uv_outside_the_declared_canvas() {
    let r = check(&packs_in(&fixture("world_graphics")), &VACIO);
    // 5 in all.geo.json, 2 legacy, 2 definitions of geometry.dup, 1 structure
    assert_eq!(r.models, 10);
    assert_eq!(
        r.models_without_canvas, 6,
        "no canvas declared: skipped, not guessed"
    );
    assert_eq!(r.cubes, 4);

    let uv: Vec<(&str, usize, f64, &str)> = r
        .issues
        .iter()
        .filter_map(|i| match i {
            GraphicsIssue::UvOutside {
                geometry,
                cubes,
                worst,
                bone,
                ..
            } => Some((geometry.as_str(), *cubes, *worst, bone.as_str())),
            _ => None,
        })
        .collect();
    // overflow: 4x4x4 box at [56, 0] on 64 wide — its south face ends at 72.
    // perface: the north face ends at 68; the flipped south face is inside.
    // plane: 27 x 0 x 10 — only its two real faces count, and they fit; the
    // whole box layout (74 wide) would not.
    assert_eq!(
        uv,
        [
            ("geometry.overflow", 1, 8.0, "arm"),
            ("geometry.perface", 1, 4.0, "head")
        ]
    );
}

#[test]
fn only_an_animation_that_moves_nothing_is_reported() {
    let r = check(&packs_in(&fixture("world_graphics")), &VACIO);
    // inherits: look (Head comes from the old-format parent; case ignored),
    //           partial (head yes, leftarm no: a shared animation, fine),
    //           nothing (backpack: no model has it)
    // two_models: look — head only in the baby model; the union counts
    // duplicated: dup — bone b is in ONE of the two definitions of geometry.dup
    // unknown: its model is not in the packs, so its bones are not guessed
    assert_eq!(r.animations_checked, 5);
    assert_eq!(r.animations_partial, 1);
    assert_eq!(r.entities_unknown_bones, 1);
    let bones: Vec<&GraphicsIssue> = r
        .issues
        .iter()
        .filter(|i| matches!(i, GraphicsIssue::AnimationBones { .. }))
        .collect();
    assert_eq!(
        bones,
        [&GraphicsIssue::AnimationBones {
            entity: "test:inherits".into(),
            animation: "animation.t.nothing".into(),
            missing: vec!["backpack".into()],
        }]
    );
}

#[test]
fn model_structure_defects() {
    let r = check(&packs_in(&fixture("world_graphics")), &VACIO);
    let of = |f: fn(&GraphicsIssue) -> bool| -> Vec<&GraphicsIssue> {
        r.issues.iter().filter(|i| f(i)).collect()
    };
    // `leg` hangs from `hip`, which nothing has; `tailtip` hangs from `head`,
    // which the old-format model it inherits from has (other capitals): fine
    assert_eq!(
        of(|i| matches!(i, GraphicsIssue::BoneParentMissing { .. })),
        [&GraphicsIssue::BoneParentMissing {
            geometry: "geometry.structure".into(),
            file: "rp/models/entity/structure.geo.json".into(),
            bone: "leg".into(),
            parent: "hip".into(),
        }]
    );
    assert_eq!(
        of(|i| matches!(i, GraphicsIssue::DuplicateBones { .. })),
        [&GraphicsIssue::DuplicateBones {
            geometry: "geometry.structure".into(),
            file: "rp/models/entity/structure.geo.json".into(),
            bones: vec!["head".into()],
        }]
    );
    // a negative size, and an inflate that shrinks a side of 4 past zero;
    // inflate -0.5 on a side of 2 is fine
    assert_eq!(
        of(|i| matches!(i, GraphicsIssue::InvertedCubes { .. })),
        [&GraphicsIssue::InvertedCubes {
            geometry: "geometry.structure".into(),
            file: "rp/models/entity/structure.geo.json".into(),
            cubes: 2,
            bone: "arm".into(),
        }]
    );
    assert_eq!(
        of(|i| matches!(i, GraphicsIssue::EmptyCubes { .. })).len(),
        1
    );
}

#[test]
fn extreme_scales_and_shaders_are_explained() {
    let r = check(&packs_in(&fixture("world_graphics")), &VACIO);
    let mut scales: Vec<(&str, [f64; 3])> = r
        .issues
        .iter()
        .filter_map(|i| match i {
            GraphicsIssue::AnimationScale {
                animation, value, ..
            } => Some((animation.as_str(), *value)),
            _ => None,
        })
        .collect();
    scales.sort_by(|a, b| a.0.cmp(b.0));
    // a Molang scale is not evaluated; one flat axis is not "vanishing"
    assert_eq!(
        scales,
        [
            ("animation.s.big", [25.0, 1.0, 1.0]),
            ("animation.s.hide", [0.0, 0.0, 0.0])
        ]
    );

    assert!(r
        .issues
        .contains(&GraphicsIssue::ShaderNeedsModifiedClient {
            pack: "rp".into(),
            files: 1
        }));
    assert!(r.issues.contains(&GraphicsIssue::VibrantVisuals {
        pack: "rp".into(),
        parts: vec!["atmospherics".into()]
    }));
    // what only explains a look is information, not a defect
    for i in &r.issues {
        let info = matches!(
            i,
            GraphicsIssue::AnimationScale { .. }
                | GraphicsIssue::EmptyCubes { .. }
                | GraphicsIssue::VibrantVisuals { .. }
        );
        assert_eq!(i.is_info(), info, "{i:?}");
        assert!(!kebitx_core::explain::graphics(i).fix.is_empty());
    }
}

#[test]
fn stretched_only_when_proportions_differ() {
    let r = check(&packs_in(&fixture("world_graphics")), &VACIO);
    // ok (64x32), hd (128x64, uniform scale) and stretched (64x64) are paired;
    // flip (64x3200) is under `uv_anim`, so it is not.
    assert_eq!(r.pairs_checked, 3);
    let stretched: Vec<&GraphicsIssue> = r
        .issues
        .iter()
        .filter(|i| matches!(i, GraphicsIssue::Stretched { .. }))
        .collect();
    assert_eq!(
        stretched,
        [&GraphicsIssue::Stretched {
            geometry: "geometry.ok".into(),
            texture: "textures/entity/stretched".into(),
            declared: [64.0, 32.0],
            image: (64, 64),
            entities: vec!["test:stretched".into()],
        }]
    );
}
