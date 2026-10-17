//! The migration report's rules, one case each, on a Java pack and a game
//! install built in a temporary folder. The images are only PNG headers: the
//! rules read sizes, never pixels.

use kebitx_core::json::{self, Json};
use kebitx_core::vanilla::VanillaCatalog;
use kebitx_migrate::{convert, inspect, MigrationReport, References, Verdict};
use std::fs;
use std::path::{Path, PathBuf};

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut p = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    p.extend(b"IHDR");
    p.extend(w.to_be_bytes());
    p.extend(h.to_be_bytes());
    p
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The Java game's own sounds.json, as the reference for event mapping.
const JAVA_SOUNDS: &str = r#"{
    "entity.cow.ambient": {"sounds": ["mob/cow/say1"]},
    "entity.chicken.ambient": {"sounds": ["mob/chicken/say1"]},
    "block.stone.step": {"sounds": ["step/stone1"]},
    "block.stone.break": {"sounds": ["step/stone1"]}
}"#;

fn setup(name: &str) -> (Scratch, PathBuf, VanillaCatalog) {
    let root = std::env::temp_dir().join(format!("kebitx-migrate-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let game = root.join("install/data/resource_packs/vanilla");
    for (rel, w, h) in [
        ("textures/blocks/stone.png", 16, 16),
        ("textures/blocks/planks_oak.png", 16, 16),
        ("textures/blocks/stone_andesite.png", 16, 16),
        ("textures/blocks/magma.png", 16, 48),
        ("textures/blocks/sea_lantern.png", 16, 80),
        ("textures/entity/cow/cow.png", 64, 32),
        ("textures/entity/rabbit/gold.png", 64, 32),
        ("textures/entity/shield_patterns/cross.png", 64, 64),
        ("textures/items/candles/red_candle.png", 16, 16),
        ("textures/entity/dolphin.png", 64, 64),
        ("textures/entity/alex.png", 64, 64),
    ] {
        put(&game, rel, &png(w, h));
    }
    // the atlas shows planks_oak, magma and sea_lantern; stone_andesite is a
    // file nothing shows (a rename to it is not confirmed)
    put(
        &game,
        "textures/terrain_texture.json",
        br#"{"texture_name": "atlas.terrain", "texture_data": {
            "planks": {"textures": ["textures/blocks/planks_oak"]},
            "magma": {"textures": "textures/blocks/magma"},
            "sea_lantern": {"textures": "textures/blocks/sea_lantern"}
        }}"#,
    );
    put(
        &game,
        "textures/flipbook_textures.json",
        br#"[{"flipbook_texture": "textures/blocks/magma", "atlas_tile": "magma", "ticks_per_frame": 10}]"#,
    );
    put(
        &game,
        "sounds/sound_definitions.json",
        br#"{"format_version": "1.14.0", "sound_definitions": {
            "mob.cow.say": {"category": "neutral", "sounds": ["sounds/mob/cow/say1"], "subtitle": "s.cow"},
            "mob.chicken.say": {"category": "neutral", "sounds": [{"name": "sounds/mob/chicken/say1", "volume": 0.5}]},
            "step.stone": {"category": "player", "sounds": ["sounds/step/stone1"]},
            "dig.stone": {"category": "block", "sounds": ["sounds/step/stone1"]}
        }}"#,
    );
    put(&game, "sounds/mob/cow/say1.ogg", b"OggS");

    let java = root.join("java");
    put(
        &java,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":69,"description":{"text":"Test ","extra":[{"text":"pack"}]}}}"#,
    );
    put(&java, "pack.png", &png(64, 64));
    let t = "assets/minecraft/textures";
    for (rel, w, h) in [
        ("block/stone.png", 16, 16),                             // same path
        ("block/oak_planks.png", 16, 16),                        // renamed, confirmed
        ("block/andesite.png", 16, 16),                          // renamed, not on the atlas
        ("block/magma.png", 16, 48),                             // animated, game flipbook
        ("block/sea_lantern.png", 32, 16),                       // animated, frames side by side
        ("block/lava_still.png", 16, 512),                       // animated, no Bedrock file
        ("entity/cow/cow.png", 64, 64),                          // same path, other shape
        ("item/red_candle.png", 16, 16),                         // unique name, kind root
        ("entity/dolphin/dolphin.png", 128, 128),                // unique name, HD scale
        ("entity/equipment/humanoid_leggings/gold.png", 64, 32), // a rabbit's gold: no
        ("entity/banner/cross.png", 64, 64),                     // not a shield
        ("entity/shield/cross.png", 64, 64),                     // shield -> shield_patterns
        ("entity/player/slim/alex.png", 64, 64),                 // two claims ...
        ("entity/player/wide/alex.png", 64, 64),                 // ... on one file
        ("gui/sprites/heart.png", 9, 9),                         // a kind with no mapping
    ] {
        put(&java, &format!("{t}/{rel}"), &png(w, h));
    }
    // frame 2 lasts twice as long: one tick of 8, frames spelled out
    put(
        &java,
        &format!("{t}/block/magma.png.mcmeta"),
        br#"{"animation":{"frametime":8,"interpolate":true,"frames":[0,{"index":2,"time":16},1]}}"#,
    );
    put(
        &java,
        &format!("{t}/block/sea_lantern.png.mcmeta"),
        br#"{"animation":{}}"#,
    );
    put(
        &java,
        &format!("{t}/block/lava_still.png.mcmeta"),
        br#"{"animation":{}}"#,
    );
    // settings, not an animation: the texture is a still one
    put(
        &java,
        &format!("{t}/block/stone.png.mcmeta"),
        br#"{"texture":{"blur":true}}"#,
    );

    put(
        &java,
        "assets/minecraft/sounds.json",
        br#"{
            "entity.cow.ambient": {"replace": true, "sounds": ["custom/moo", {"name": "custom/moo", "pitch": 0.8}]},
            "entity.chicken.ambient": {"sounds": ["mob/chicken/say1"]},
            "block.stone.step": {"sounds": ["custom/moo"]},
            "my.new.event": {"sounds": ["custom/moo"]}
        }"#,
    );
    put(&java, "assets/minecraft/sounds/custom/moo.ogg", b"OggS");
    put(&java, "assets/minecraft/sounds/custom/unused.ogg", b"OggS");
    put(&java, "assets/minecraft/sounds/mob/cow/say1.ogg", b"OggS");
    put(&java, "assets/minecraft/shaders/core/x.vsh", b"");
    put(&java, "assets/mymod/textures/item/gear.png", &png(16, 16));

    let catalog = VanillaCatalog::from_install(&root.join("install"));
    (Scratch(root), java, catalog)
}

fn java_sounds() -> Json {
    json::parse(JAVA_SOUNDS).unwrap()
}

fn verdict<'a>(r: &'a MigrationReport, java: &str) -> (Verdict, Option<&'a str>, &'static str) {
    let e = r
        .entries
        .iter()
        .find(|e| e.java == java)
        .unwrap_or_else(|| panic!("{java} not in the report"));
    (e.verdict, e.bedrock.as_deref(), e.reason)
}

#[test]
fn every_rule_gives_its_verdict() {
    let (_s, java, catalog) = setup("rules");
    let js = java_sounds();
    let r = inspect(&java, with_sounds(&catalog, &js));
    assert!(r.is_java_pack);
    assert_eq!(r.description, "Test pack");
    assert_eq!(r.pack_format, Some(69));

    let t = "assets/minecraft/textures";
    for (f, to) in [
        ("block/stone.png", "textures/blocks/stone.png"),
        ("block/oak_planks.png", "textures/blocks/planks_oak.png"),
        ("block/magma.png", "textures/blocks/magma.png"),
        ("block/magma.png.mcmeta", "textures/flipbook_textures.json"),
        (
            "item/red_candle.png",
            "textures/items/candles/red_candle.png",
        ),
        ("entity/dolphin/dolphin.png", "textures/entity/dolphin.png"),
        (
            "entity/shield/cross.png",
            "textures/entity/shield_patterns/cross.png",
        ),
    ] {
        assert_eq!(verdict(&r, &format!("{t}/{f}")).1, Some(to), "{f}");
    }
    assert_eq!(
        verdict(&r, "assets/minecraft/sounds/mob/cow/say1.ogg").1,
        Some("sounds/mob/cow/say1.ogg")
    );

    for (f, why) in [
        ("block/andesite.png", "no Bedrock file"),
        ("block/sea_lantern.png", "one column of squares"),
        ("block/sea_lantern.png.mcmeta", "does not convert"),
        ("block/lava_still.png", "no Bedrock file"),
        ("block/stone.png.mcmeta", "rendering hints"),
        ("entity/cow/cow.png", "other proportions"),
        (
            "entity/equipment/humanoid_leggings/gold.png",
            "no Bedrock file",
        ),
        ("entity/banner/cross.png", "no Bedrock file"),
        ("entity/player/slim/alex.png", "two Java textures"),
        ("entity/player/wide/alex.png", "two Java textures"),
        ("gui/sprites/heart.png", "organises differently"),
    ] {
        let (v, b, reason) = verdict(&r, &format!("{t}/{f}"));
        assert_eq!((v, b), (Verdict::Adapts, None), "{f}");
        assert!(reason.contains(why), "{f}: {reason}");
    }
    assert_eq!(
        verdict(&r, "assets/minecraft/shaders/core/x.vsh").0,
        Verdict::Rebuild
    );
    assert_eq!(
        verdict(&r, "assets/mymod/textures/item/gear.png").0,
        Verdict::Rebuild
    );
}

#[test]
fn an_animation_becomes_the_same_flipbook_tick_for_tick() {
    let (_s, java, catalog) = setup("flipbook");
    let r = inspect(&java, References::catalog_only(&catalog));
    assert_eq!(r.flipbooks.len(), 1);
    let fb = &r.flipbooks[0];
    assert_eq!(
        fb.get("flipbook_texture").and_then(Json::as_str),
        Some("textures/blocks/magma")
    );
    assert_eq!(fb.get("atlas_tile").and_then(Json::as_str), Some("magma"));
    assert_eq!(fb.get("ticks_per_frame"), Some(&Json::Num(8.0)));
    assert_eq!(fb.get("blend_frames"), Some(&Json::Bool(true)));
    let frames: Vec<f64> = fb
        .get("frames")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|f| match f {
            Json::Num(n) => *n,
            _ => panic!(),
        })
        .collect();
    assert_eq!(frames, [0.0, 2.0, 2.0, 1.0]);
}

#[test]
fn sound_events_map_only_by_identical_files() {
    let (_s, java, catalog) = setup("sounds");
    let js = java_sounds();
    let r = inspect(&java, with_sounds(&catalog, &js));
    let ev = |name: &str| r.sound_events.iter().find(|e| e.java == name).unwrap();
    assert_eq!(
        (
            ev("entity.cow.ambient").verdict,
            ev("entity.cow.ambient").bedrock.as_deref()
        ),
        (Verdict::Converts, Some("mob.cow.say"))
    );
    // the pack lists exactly the Java game's sounds: nothing changes
    assert_eq!(ev("entity.chicken.ambient").verdict, Verdict::Converts);
    assert!(ev("entity.chicken.ambient")
        .reason
        .contains("nothing to write"));
    // stone step and stone digging play the same file: no proof which is which
    assert_eq!(ev("block.stone.step").verdict, Verdict::Adapts);
    assert_eq!(ev("my.new.event").verdict, Verdict::Adapts);

    assert_eq!(
        r.sound_definitions.keys().collect::<Vec<_>>(),
        ["mob.cow.say"]
    );
    let cow = &r.sound_definitions["mob.cow.say"];
    assert_eq!(
        cow.get("category").and_then(Json::as_str),
        Some("neutral"),
        "Bedrock's category kept"
    );
    assert_eq!(
        cow.get("subtitle").and_then(Json::as_str),
        Some("s.cow"),
        "Bedrock's subtitle kept"
    );
    let sounds = cow.get("sounds").and_then(Json::as_array).unwrap();
    assert_eq!(sounds.len(), 2, "replace: only the pack's sounds");
    assert_eq!(sounds[0], Json::Str("sounds/custom/moo".into()));
    assert_eq!(sounds[1].get("pitch"), Some(&Json::Num(0.8)));

    let s = "assets/minecraft/sounds";
    assert_eq!(
        verdict(&r, &format!("{s}/custom/moo.ogg")).1,
        Some("sounds/custom/moo.ogg")
    );
    assert_eq!(
        verdict(&r, &format!("{s}/custom/unused.ogg")).0,
        Verdict::Adapts
    );

    // without the Java game's sounds.json, no event can be proven
    let blind = inspect(&java, References::catalog_only(&catalog));
    assert!(blind
        .sound_events
        .iter()
        .all(|e| e.verdict == Verdict::Adapts));
    assert!(blind.sound_definitions.is_empty());
}

#[test]
fn converting_writes_only_what_converts_and_never_into_a_used_folder() {
    let (s, java, catalog) = setup("convert");
    let js = java_sounds();
    let r = inspect(&java, with_sounds(&catalog, &js));
    let out = s.0.join("out");
    let c = convert(&java, &r, &out).expect("converts");

    let mut written: Vec<String> = kebitx_core::filesystem::all_files(&out)
        .iter()
        .map(|f| {
            f.strip_prefix(&out)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    written.sort();
    assert_eq!(
        written,
        [
            "manifest.json",
            "pack_icon.png",
            "sounds/custom/moo.ogg",
            "sounds/mob/cow/say1.ogg",
            "sounds/sound_definitions.json",
            "textures/blocks/magma.png",
            "textures/blocks/planks_oak.png",
            "textures/blocks/stone.png",
            "textures/entity/dolphin.png",
            "textures/entity/shield_patterns/cross.png",
            "textures/flipbook_textures.json",
            "textures/items/candles/red_candle.png",
        ]
    );
    assert_eq!(c.files, written.len());
    assert_eq!(c.skipped_duplicates, 0);
    for generated in [
        "manifest.json",
        "sounds/sound_definitions.json",
        "textures/flipbook_textures.json",
    ] {
        let text = fs::read_to_string(out.join(generated)).unwrap();
        assert!(json::parse(&text).is_ok(), "{generated}: {text}");
    }
    let manifest = fs::read_to_string(out.join("manifest.json")).unwrap();
    assert!(manifest.contains("Test pack (converted from Java by kebitx)"));

    // the same pack converted again: the same manifest, so the game sees an update
    let again = s.0.join("again");
    convert(&java, &r, &again).unwrap();
    assert_eq!(
        fs::read_to_string(again.join("manifest.json")).unwrap(),
        manifest
    );

    // a folder with something in it is refused, and left as it was
    assert!(convert(&java, &r, &out).is_err());
    assert_eq!(
        kebitx_core::filesystem::all_files(&out).len(),
        written.len()
    );
}

fn with_sounds<'a>(catalog: &'a VanillaCatalog, sounds: &'a Json) -> References<'a> {
    References {
        java_sounds: Some(sounds),
        ..References::catalog_only(catalog)
    }
}
