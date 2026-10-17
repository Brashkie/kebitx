//! Proofs by pixels and by English text, end to end: a Bedrock install, a
//! Java `.jar` and a pack, all made in a temporary folder with real PNGs.

use kebitx_core::codec::png::{self, Rgba};
use kebitx_core::codec::zip::ZipArchive;
use kebitx_core::vanilla::VanillaCatalog;
use kebitx_migrate::content::{BedrockPixels, JavaPixels};
use kebitx_migrate::lang::BedrockLang;
use kebitx_migrate::{convert, inspect, JavaJar, References, Verdict};
use std::fs;
use std::path::{Path, PathBuf};

/// A picture no other test picture equals: every pixel from `seed`.
fn pic(w: u32, h: u32, seed: u8) -> Rgba {
    let mut img = Rgba::new(w, h);
    for (i, p) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.copy_from_slice(&[
            (i as u8).wrapping_mul(7).wrapping_add(seed),
            seed,
            (i as u8) ^ seed,
            255,
        ]);
    }
    img
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, bytes).unwrap();
    p
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pixels_and_strings_prove_what_names_cannot() {
    let root = std::env::temp_dir().join(format!("kebitx-proofs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let _scratch = Scratch(root.clone());

    // the Java game's own pictures
    let zombie = pic(8, 8, 1); // Bedrock's zombie is its top half
    let angry = pic(4, 4, 2); // a cell of Bedrock's particle sheet
    let icon = pic(6, 6, 3); // Bedrock has it whole, at another path
    let jar_src = root.join("jar_src");
    let t = "assets/minecraft/textures";
    let mut entries = Vec::new();
    for (rel, img) in [
        ("entity/zombie/zombie.png", &zombie),
        ("particle/angry.png", &angry),
        ("gui/sprites/icon.png", &icon),
    ] {
        let name = format!("{t}/{rel}");
        entries.push((name.clone(), put(&jar_src, &name, &png::encode(img))));
    }
    let en =
        r#"{"block.minecraft.stone":"Stone","item.minecraft.apple":"Apple","gui.done":"Done"}"#;
    entries.push((
        "assets/minecraft/lang/en_us.json".into(),
        put(&jar_src, "en_us.json", en.as_bytes()),
    ));
    for (name, body) in [
        (
            "assets/minecraft/models/block/stone.json",
            r#"{"parent":"block/cube_all","textures":{"all":"block/stone"}}"#,
        ),
        (
            "assets/minecraft/models/block/dirt.json",
            r#"{"parent":"block/cube_all","textures":{"all":"block/dirt"}}"#,
        ),
        (
            "assets/minecraft/models/block/cube_all.json",
            r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"north":{"texture":"#all"},"up":{"texture":"#all"}}}]}"##,
        ),
    ] {
        entries.push((name.into(), put(&jar_src, name, body.as_bytes())));
    }
    let jar_path = root.join("game.jar");
    kebitx_build::zip::write_zip(&jar_path, &entries).unwrap();
    let jar = JavaJar {
        id: "test".into(),
        format: 97,
        exact: true,
        zip: ZipArchive::open(&jar_path).unwrap(),
    };

    // the Bedrock game
    let game = root.join("install/data/resource_packs/vanilla");
    let mut sheet = pic(8, 8, 9);
    sheet.paste(&angry, 4, 4);
    put(
        &game,
        "textures/particle/particles.png",
        &png::encode(&sheet),
    );
    put(
        &game,
        "textures/entity/zombie/zombie.png",
        &png::encode(&zombie.crop(0, 0, 8, 4).unwrap()),
    );
    put(&game, "textures/ui/the_icon.png", &png::encode(&icon));
    put(&game, "texts/en_US.lang", b"tile.stone.name=Stone\t#\nitem.apple.name=Apple\njoincode.icon_text.12=Apple\ngui.done=Done\n");
    put(
        &game,
        "texts/es_ES.lang",
        b"tile.stone.name=Piedra\ngui.done=Listo\n",
    );
    put(&game, "texts/languages.json", br#"["en_US","es_ES"]"#);
    let install = root.join("install");
    let catalog = VanillaCatalog::from_install(&install);
    let bp = BedrockPixels::from_install(&install);
    let jp = JavaPixels::from_jar(&jar);
    let bl = BedrockLang::from_install(&install);

    // the pack: every picture changed, and Spanish strings
    let pack = root.join("pack");
    put(
        &pack,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":97,"description":"p"}}"#,
    );
    let changed = |img: &Rgba| {
        let mut c = img.clone();
        c.pixels[0] ^= 0xff;
        c
    };
    let zombie2 = changed(&zombie);
    let angry2 = changed(&angry);
    put(
        &pack,
        &format!("{t}/entity/zombie/zombie.png"),
        &png::encode(&zombie2),
    );
    put(
        &pack,
        &format!("{t}/particle/angry.png"),
        &png::encode(&angry2),
    );
    put(
        &pack,
        &format!("{t}/gui/sprites/icon.png"),
        &png::encode(&changed(&icon)),
    );
    put(
        &pack,
        "assets/minecraft/lang/es_es.json",
        r#"{"block.minecraft.stone":"Roca","item.minecraft.apple":"Manzana","gui.done":"Listo","my.key":"x"}"#.as_bytes(),
    );

    // the game's own model, written another way; a changed one; a new one
    let m = "assets/minecraft/models/block";
    put(
        &pack,
        &format!("{m}/stone.json"),
        b"{\n  \"textures\": {\"all\": \"block/stone\"},\n  \"parent\": \"block/cube_all\"\n}\n",
    );
    put(
        &pack,
        &format!("{m}/dirt.json"),
        br#"{"parent":"block/cube_all","textures":{"all":"block/gravel"}}"#,
    );
    put(
        &pack,
        &format!("{m}/mine.json"),
        br#"{"parent":"block/cube_all"}"#,
    );

    let refs = References {
        java_jar: Some(&jar),
        java_pixels: Some(&jp),
        bedrock_pixels: Some(&bp),
        bedrock_lang: Some(&bl),
        ..References::catalog_only(&catalog)
    };
    let r = inspect(&pack, refs);
    let reason = |f: &str| {
        r.entries
            .iter()
            .find(|e| e.java == f)
            .map(|e| (e.verdict, e.reason))
            .unwrap()
    };
    assert_eq!(
        reason(&format!("{m}/stone.json")).0,
        Verdict::Converts,
        "the game's own model: nothing to do"
    );
    assert!(reason(&format!("{m}/dirt.json"))
        .1
        .contains("custom block is drafted"));
    assert!(reason(&format!("{m}/mine.json"))
        .1
        .contains("custom block is drafted"));
    let names: Vec<&str> = r.models.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["dirt", "mine"]);
    // dirt now shows gravel, which this fake game does not have: not proven
    assert_eq!(r.models[0].textures.get("all"), Some(&None));
    let to = |f: &str| {
        let e = r
            .entries
            .iter()
            .find(|e| e.java == f)
            .unwrap_or_else(|| panic!("{f}"));
        (e.verdict, e.bedrock.clone())
    };
    assert_eq!(
        to(&format!("{t}/gui/sprites/icon.png")),
        (Verdict::Converts, Some("textures/ui/the_icon.png".into()))
    );
    assert_eq!(
        to(&format!("{t}/entity/zombie/zombie.png")),
        (
            Verdict::Converts,
            Some("textures/entity/zombie/zombie.png".into())
        )
    );
    assert_eq!(
        to(&format!("{t}/particle/angry.png")),
        (
            Verdict::Converts,
            Some("textures/particle/particles.png".into())
        )
    );

    // the cut: the pack's top half; the sheet: the game's, with the pack's sprite
    assert_eq!(
        r.images["textures/entity/zombie/zombie.png"],
        zombie2.crop(0, 0, 8, 4).unwrap()
    );
    let mut expected = sheet.clone();
    expected.paste(&angry2, 4, 4);
    assert_eq!(r.images["textures/particle/particles.png"], expected);

    // strings: proven by identical English, the family breaking the "Apple"
    // tie; "Listo" is what the game already says
    let es = &r.texts["es_ES"];
    assert_eq!(es.get("tile.stone.name").map(String::as_str), Some("Roca"));
    assert_eq!(
        es.get("item.apple.name").map(String::as_str),
        Some("Manzana")
    );
    assert!(
        !es.contains_key("gui.done"),
        "the same as the game's: not written"
    );
    assert_eq!(es.len(), 2);

    let out = root.join("out");
    convert(&pack, &r, &out).unwrap();
    for generated in [
        "models/blocks/kebitx_dirt.geo.json",
        "kebitx_drafts/behavior_pack/blocks/dirt.json",
        "kebitx_drafts/README.txt",
    ] {
        assert!(out.join(generated).is_file(), "{generated}");
    }
    assert_eq!(
        png::decode(&fs::read(out.join("textures/particle/particles.png")).unwrap()).unwrap(),
        expected
    );
    let lang = fs::read_to_string(out.join("texts/es_ES.lang")).unwrap();
    assert!(
        lang.contains("tile.stone.name=Roca\n") && lang.contains("item.apple.name=Manzana\n"),
        "{lang}"
    );
    assert_eq!(
        fs::read_to_string(out.join("texts/languages.json")).unwrap(),
        r#"["es_ES"]"#
    );
}
