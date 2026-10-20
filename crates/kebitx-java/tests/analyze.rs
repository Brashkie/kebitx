//! A Fabric mod with one defect of each kind, analysed as a folder and as a
//! `.jar`: every defect is found, nothing else is, and a reference into
//! another mod's namespace is counted as unchecked, never reported. No game
//! is needed: a mod's own namespace is proven by the mod alone.

use kebitx_core::codec::png::{encode, Rgba};
use kebitx_java::{analyze, Finding, Kind, Source};
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn png(w: u32, h: u32) -> Vec<u8> {
    encode(&Rgba::new(w, h))
}

fn demo_mod(root: &Path) {
    write(
        root,
        "fabric.mod.json",
        br#"{"schemaVersion":1,"id":"demo","version":"1.0.0","depends":{"minecraft":">=26.3"}}"#,
    );
    // a blockstate: one model exists, one does not
    write(
        root,
        "assets/demo/blockstates/lamp.json",
        br#"{"variants":{"lit=true":{"model":"demo:block/lamp_on"},"lit=false":{"model":"demo:block/lamp_off"}}}"#,
    );
    // the used model: its faces ask for #side (set) and #top (never set)
    write(
        root,
        "assets/demo/models/block/lamp_on.json",
        br##"{"parent":"demo:block/base","textures":{"side":"demo:block/lamp_side"},
             "elements":[{"from":[0,0,0],"to":[16,16,16],
               "faces":{"north":{"texture":"#side"},"up":{"texture":"#top"}}}]}"##,
    );
    write(
        root,
        "assets/demo/models/block/base.json",
        br##"{"textures":{"particle":"#side"}}"##,
    );
    // a parent that does not exist; a texture of another mod (unprovable)
    write(
        root,
        "assets/demo/models/block/orphan.json",
        br#"{"parent":"demo:block/nowhere","textures":{"all":"othermod:block/stone"}}"#,
    );
    // an item: its model names a texture the mod lacks
    write(
        root,
        "assets/demo/items/ghost.json",
        br#"{"model":{"type":"minecraft:model","model":"demo:item/ghost"}}"#,
    );
    write(
        root,
        "assets/demo/models/item/ghost.json",
        br#"{"parent":"demo:item/flat","textures":{"layer0":"demo:item/ghost"}}"#,
    );
    write(
        root,
        "assets/demo/models/item/flat.json",
        br#"{"parent":"builtin/generated"}"#,
    );
    // an item whose model is missing, nested in a condition
    write(
        root,
        "assets/demo/items/wand.json",
        br#"{"model":{"type":"minecraft:condition","property":"minecraft:using_item",
             "on_true":{"type":"minecraft:model","model":"demo:item/wand_on"},
             "on_false":{"type":"minecraft:model","model":"demo:item/ghost"}}}"#,
    );
    // an animation: 16x40 is not a strip of 16x16 frames; 16x48 is
    write(
        root,
        "assets/demo/textures/block/lamp_side.png",
        &png(16, 40),
    );
    write(
        root,
        "assets/demo/textures/block/lamp_side.png.mcmeta",
        br#"{"animation":{"frametime":2}}"#,
    );
    write(root, "assets/demo/textures/block/ok.png", &png(16, 48));
    write(
        root,
        "assets/demo/textures/block/ok.png.mcmeta",
        br#"{"animation":{}}"#,
    );
    // a JPEG saved as .png: the game refuses anything but a PNG
    write(
        root,
        "assets/demo/textures/item/photo.png",
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00",
    );
    // languages: Spanish lacks one key of two
    write(root, "assets/demo/lang/en_us.json", br#"{"a":"A","b":"B"}"#);
    write(root, "assets/demo/lang/es_es.json", br#"{"a":"A"}"#);
}

fn expected() -> Vec<Finding> {
    let s = String::from;
    let mut v = vec![
        Finding::MissingModel {
            file: s("assets/demo/blockstates/lamp.json"),
            model: s("demo:block/lamp_off"),
        },
        Finding::MissingModel {
            file: s("assets/demo/items/wand.json"),
            model: s("demo:item/wand_on"),
        },
        Finding::MissingModel {
            file: s("assets/demo/models/block/orphan.json"),
            model: s("demo:block/nowhere"),
        },
        Finding::MissingTexture {
            file: s("assets/demo/models/item/ghost.json"),
            texture: s("demo:item/ghost"),
        },
        Finding::UnboundTexture {
            model: s("demo:block/lamp_on"),
            variable: s("top"),
            used_by: s("assets/demo/blockstates/lamp.json"),
        },
        Finding::BadAnimation {
            texture: s("assets/demo/textures/block/lamp_side.png"),
            size: (16, 40),
            frame: (16, 16),
        },
        Finding::NotPng {
            texture: s("assets/demo/textures/item/photo.png"),
        },
        Finding::MissingLangKeys {
            file: s("assets/demo/lang/es_es.json"),
            missing: 1,
            of: 2,
        },
    ];
    v.sort();
    v
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kebitx-java-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn every_defect_and_nothing_else_folder_and_jar() {
    let dir = temp("mod");
    demo_mod(&dir);

    let r = analyze(&Source::open(&dir).unwrap(), None);
    assert_eq!(
        r.kind,
        Kind::Mod {
            loader: "Fabric",
            ids: vec!["demo".into()],
            minecraft: Some(">=26.3".into()),
            depends: vec!["minecraft".into()],
        }
    );
    assert_eq!(r.findings, expected(), "{:#?}", r.findings);
    assert_eq!(r.broken(), 7);
    assert_eq!(
        (
            r.blockstates,
            r.items,
            r.models,
            r.animations,
            r.textures,
            r.languages
        ),
        (1, 2, 5, 2, 3, 2)
    );
    // another mod's texture cannot be proven from here
    assert_eq!(r.unchecked, 1, "{r:#?}");

    // the same mod as a .jar, read in place
    let jar = dir.with_extension("jar");
    let entries: Vec<(String, PathBuf)> = Source::open(&dir)
        .unwrap()
        .files()
        .into_iter()
        .map(|f| (f.clone(), dir.join(&f)))
        .collect();
    kebitx_build::zip::write_zip(&jar, &entries).unwrap();
    let z = analyze(&Source::open(&jar).unwrap(), None);
    assert_eq!(z.findings, r.findings);
    assert_eq!((z.checked, z.unchecked), (r.checked, r.unchecked));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&jar);
}

/// In a resource pack nothing but the game's namespace can be proven: a
/// missing texture of a mod's namespace is the mod's to provide.
#[test]
fn a_resource_pack_claims_nothing_about_other_namespaces() {
    let dir = temp("rp");
    write(
        &dir,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":97,"description":"x"}}"#,
    );
    write(
        &dir,
        "assets/somemod/models/block/thing.json",
        br#"{"parent":"somemod:block/base","textures":{"all":"somemod:block/thing"}}"#,
    );
    let r = analyze(&Source::open(&dir).unwrap(), None);
    assert_eq!(
        r.kind,
        Kind::ResourcePack {
            formats: Some((97, 97))
        }
    );
    assert!(r.findings.is_empty(), "{:#?}", r.findings);
    assert_eq!((r.checked, r.unchecked), (0, 2));
    let _ = std::fs::remove_dir_all(&dir);
}
