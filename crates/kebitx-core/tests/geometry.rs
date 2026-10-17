//! Geometry lives in IDENTIFIER space and requires an index pass first. This verifies the index
//! handles BOTH model formats (old top-level key + new minecraft:geometry array), that an absent
//! identifier is not found, and that at analyze level a missing geometry is reported precisely.

mod common;
use common::fixture;
use kebitx_core::analyze::analyze;
use kebitx_core::filesystem::discover_models;
use kebitx_core::registry::geometry::GeometryRegistry;

#[test]
fn index_handles_both_model_formats() {
    let models = discover_models(&fixture("models_both_formats"));
    let reg = GeometryRegistry::build(&models);

    assert!(
        reg.resolve("geometry.old_one").is_some(),
        "old top-level-key format"
    );
    assert!(
        reg.resolve("geometry.new_one").is_some(),
        "new minecraft:geometry array format"
    );
    assert!(
        reg.resolve("geometry.nope").is_none(),
        "absent identifier -> not found"
    );
    assert_eq!(reg.len(), 2);
}

#[test]
fn missing_geometry_reported_exactly_once() {
    // Entity references geometry.missing_one; models/ only defines geometry.present_one.
    let r = analyze(&fixture("geo_missing_pack"));
    assert_eq!(r.geometry_declared, 1);
    assert_eq!(r.geometry_resolved, 0);
    assert_eq!(r.geometry_missing, 1, "exactly one missing geometry");
    assert_eq!(
        r.textures_missing, 0,
        "texture is fine; only geometry is missing"
    );
}
