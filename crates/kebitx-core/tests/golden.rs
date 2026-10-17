//! Golden tests over the `AnalysisReport` (no hand counting). Villager and horse as real on-disk
//! packs, now including models so geometry resolves too.

mod common;
use common::fixture;
use kebitx_core::vanilla::VanillaCatalog;
/// Tests use an EMPTY catalog on purpose: a fixture must not change its
/// result depending on which Minecraft happens to be installed.
static VACIO: std::sync::LazyLock<VanillaCatalog> = std::sync::LazyLock::new(VanillaCatalog::empty);

use kebitx_core::analyze::analyze_with;
use kebitx_core::parser::parse_client_entity;
use kebitx_core::registry::texture::TextureRegistry;
use kebitx_core::resolver::texture::{TextureResolution, TextureResolver};

const EMBEDDED: &str = r#"{
  "minecraft:client_entity": { "description": {
    "identifier": "kebitx:demo",
    "textures": {
      // three "base" share one file
      "base": "textures/demo/body", "base2": "textures/demo/body",
      "base3": "textures/demo/body", "hat": "textures/demo/hat"
    }
  }}
}"#;

#[test]
fn parser_collapses_shortnames_to_files() {
    let decl = parse_client_entity(EMBEDDED).expect("parse");
    assert_eq!(decl.declared_texture_count(), 4);
    assert_eq!(decl.unique_texture_files(), 2);
}

#[test]
fn villager_pack_report() {
    let r = analyze_with(&fixture("villager_pack"), &VACIO);
    assert_eq!(r.entities, 1);
    assert_eq!(r.textures_declared, 33);
    assert_eq!(r.texture_files, 28, "base*6 -> 1 file");
    assert_eq!(r.textures_resolved, 33);
    assert_eq!(r.textures_missing, 0);
    assert_eq!(r.geometry_declared, 1);
    assert_eq!(
        r.geometry_resolved, 1,
        "geometry.villager_v2 indexed from models/"
    );
    assert_eq!(r.geometry_missing, 0);
}

#[test]
fn horse_pack_report() {
    let r = analyze_with(&fixture("horse_pack"), &VACIO);
    assert_eq!(r.textures_declared, 17);
    assert_eq!(r.textures_resolved, 17, "includes horse_armor_leather.tga");
    assert_eq!(r.textures_missing, 0);
    assert_eq!(r.geometry_resolved, 1);
    assert_eq!(r.geometry_missing, 0);
}

#[test]
fn texture_resolver_probes_png_then_tga() {
    let base = "textures/entity/horse2/armor";
    let reg = TextureRegistry::from_files([
        format!("{base}/horse_armor_diamond.png"),
        format!("{base}/horse_armor_leather.tga"),
    ]);
    let mut r = TextureResolver::new(&reg, &VACIO);
    assert!(matches!(
        r.resolve(&format!("{base}/horse_armor_leather")),
        TextureResolution::Resolved(_)
    ));
    assert_eq!(
        r.resolve(&format!("{base}/horse_armor_emerald")),
        TextureResolution::KnownMissing
    );
}
