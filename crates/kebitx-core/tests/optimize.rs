//! Optimization checks on `optimize_pack`: a 1200×1082 pack icon, a
//! 4096×2048 texture, a small one, a JPEG named `.png`, and a model of 500
//! cubes next to a light one.

mod common;
use common::fixture;
use kebitx_core::image::Format;
use kebitx_core::manifest::pack_at;
use kebitx_core::optimize::{run, OptIssue};

#[test]
fn what_costs_a_phone_is_measured() {
    let r = run(&[pack_at(&fixture("optimize_pack"))]);
    assert_eq!(r.images, 3);
    assert_eq!(r.unreadable_images, 0, "the JPEG's size is read too");
    // 4096×2048 + 64×64 + 64×32, 4 bytes a pixel
    assert_eq!(r.video_bytes, (4096 * 2048 + 64 * 64 + 64 * 32) * 4);
    assert_eq!((r.models, r.cubes), (2, 501));

    assert!(r.issues.contains(&OptIssue::BigIcon {
        pack: "optimize_pack".into(),
        width: 1200,
        height: 1082,
        bytes: 24,
    }));
    assert!(r.issues.contains(&OptIssue::BigTexture {
        pack: "optimize_pack".into(),
        path: "textures/entity/huge.png".into(),
        width: 4096,
        height: 2048,
        video_bytes: 4096 * 2048 * 4,
    }));
    assert!(r.issues.contains(&OptIssue::MislabelledImage {
        pack: "optimize_pack".into(),
        path: "textures/entity/photo.png".into(),
        content: Format::Jpeg,
    }));
    assert!(r.issues.contains(&OptIssue::HeavyModel {
        geometry: "geometry.heavy".into(),
        file: "optimize_pack/models/entity/models.geo.json".into(),
        cubes: 500,
        bones: 2,
    }));
    assert_eq!(
        r.issues.len(),
        4,
        "the small texture and the light model are fine: {:#?}",
        r.issues
    );
    // the costliest first: the 32 MB texture leads
    assert!(matches!(r.issues[0], OptIssue::BigTexture { .. }));
}
