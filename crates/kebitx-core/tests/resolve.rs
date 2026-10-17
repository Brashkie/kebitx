//! Engine precision, via `analyze` over real packs. A healthy pack: 0 missing. A pack missing
//! exactly one texture: 1 texture missing, the rest intact, geometry unaffected.

mod common;
use common::fixture;
use kebitx_core::vanilla::VanillaCatalog;
/// Empty catalog on purpose: a fixture must not change its verdict
/// depending on which Minecraft happens to be installed on the machine.
static VACIO: std::sync::LazyLock<VanillaCatalog> = std::sync::LazyLock::new(VanillaCatalog::empty);

use kebitx_core::analyze::analyze_with;

#[test]
fn healthy_pack_has_no_missing() {
    let r = analyze_with(&fixture("horse_pack"), &VACIO);
    assert_eq!(r.textures_missing, 0);
    assert_eq!(r.textures_resolved, 17);
    assert_eq!(r.geometry_missing, 0);
    // every kind together: a healthy fixture must not be broken in any way,
    // including references (render controllers) added after it was written
    assert_eq!(r.broken(), 0, "{:?}", r.issues);
}

#[test]
fn missing_one_texture_reports_exactly_one() {
    let r = analyze_with(&fixture("horse_pack_broken"), &VACIO);
    assert_eq!(r.textures_declared, 17);
    assert_eq!(r.textures_missing, 1, "exactly one broken: the absent .tga");
    assert_eq!(r.textures_resolved, 16, "the other 16 intact");
    assert_eq!(r.geometry_missing, 0, "geometry still present, unaffected");
    assert_eq!(r.broken(), 1, "{:?}", r.issues);
}
