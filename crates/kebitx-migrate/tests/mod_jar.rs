//! A Java mod `.jar`, end to end, on games made in a temporary folder: its
//! flat items and its stateless block become Bedrock custom ones, its
//! recipe and loot table name them, its names reach the language files, a
//! block with states and its code go to the task list.

use kebitx_core::codec::png::{self, Rgba};
use kebitx_core::codec::zip::ZipArchive;
use kebitx_core::vanilla::VanillaCatalog;
use kebitx_migrate::adapters::BedrockDefs;
use kebitx_migrate::recipes::RecipeRefs;
use kebitx_migrate::{convert, input, inspect, JavaJar, References, Verdict};
use std::fs;
use std::path::{Path, PathBuf};

fn put(root: &Path, rel: &str, body: &[u8]) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, body).unwrap();
    p
}

fn opaque(seed: u8) -> Vec<u8> {
    let mut img = Rgba::new(16, 16);
    for (i, p) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.copy_from_slice(&[seed, i as u8, 7, 255]);
    }
    png::encode(&img)
}

#[test]
fn a_mod_jar_becomes_custom_items_and_blocks() {
    let root = std::env::temp_dir().join(format!("kebitx-modjar-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    // the Java game: the parent models the mod's models lean on
    let src = root.join("jarsrc");
    let mut entries = Vec::new();
    for (name, body) in [
        (
            "assets/minecraft/models/item/generated.json",
            r#"{"parent":"builtin/generated"}"#,
        ),
        (
            "assets/minecraft/models/item/handheld.json",
            r#"{"parent":"item/generated"}"#,
        ),
        (
            "assets/minecraft/models/block/cube_all.json",
            r##"{"parent":"block/cube","textures":{"particle":"#all","down":"#all","up":"#all","north":"#all","east":"#all","south":"#all","west":"#all"}}"##,
        ),
        (
            "assets/minecraft/models/block/cube.json",
            r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"down":{"texture":"#down","cullface":"down"},"up":{"texture":"#up","cullface":"up"},"north":{"texture":"#north","cullface":"north"},"south":{"texture":"#south","cullface":"south"},"west":{"texture":"#west","cullface":"west"},"east":{"texture":"#east","cullface":"east"}}}]}"##,
        ),
    ] {
        entries.push((name.to_string(), put(&src, name, body.as_bytes())));
    }
    let jar_path = root.join("game.jar");
    kebitx_build::zip::write_zip(&jar_path, &entries).unwrap();
    let jar = JavaJar {
        id: "t".into(),
        format: 97,
        exact: true,
        zip: ZipArchive::open(&jar_path).unwrap(),
    };

    // the Bedrock game: one item and one block of its own, for the
    // components its definitions use
    let bp = root.join("install/data/behavior_packs/vanilla");
    put(&bp, "items/apple.json", br#"{"minecraft:item":{"description":{"identifier":"minecraft:apple"},"components":{"minecraft:icon":{"textures":{"default":"apple"}},"minecraft:display_name":{"value":"item.apple.name"},"minecraft:hand_equipped":false}}}"#);
    put(&bp, "blocks/shrub.json", br#"{"minecraft:block":{"description":{"identifier":"minecraft:shrub"},"components":{"minecraft:geometry":{"identifier":"x"},"minecraft:material_instances":{},"minecraft:loot":"x"}}}"#);
    put(&bp, "recipes/stick.json", br#"{"minecraft:recipe_shaped":{"description":{"identifier":"minecraft:stick"},"tags":["crafting_table"],"pattern":["A"],"key":{"A":{"item":"minecraft:oak_planks"}},"result":{"item":"minecraft:stick"}}}"#);
    fs::create_dir_all(root.join("install/data/resource_packs/vanilla")).unwrap();
    let install = root.join("install");
    let catalog = VanillaCatalog::from_install(&install);
    let defs = BedrockDefs::from_install(&install);
    let recipes = RecipeRefs::build(&install, &jar);
    let loot = kebitx_migrate::loot::LootRefs::build(&install, &jar);

    // the mod
    let m = root.join("modsrc");
    let mut files = Vec::new();
    for (rel, body) in [
        ("fabric.mod.json", br#"{"id":"demo","name":"Demo Mod"}"#.to_vec()),
        ("assets/demo/lang/en_us.json", br#"{"item.demo.ruby":"Ruby","block.demo.ruby_block":"Block of Ruby"}"#.to_vec()),
        ("assets/demo/models/item/ruby.json", br#"{"parent":"minecraft:item/generated","textures":{"layer0":"demo:item/ruby"}}"#.to_vec()),
        ("assets/demo/models/item/ruby_sword.json", br#"{"parent":"minecraft:item/handheld","textures":{"layer0":"demo:item/ruby_sword"}}"#.to_vec()),
        ("assets/demo/models/item/ruby_block.json", br#"{"parent":"demo:block/ruby_block"}"#.to_vec()),
        ("assets/demo/models/block/ruby_block.json", br#"{"parent":"minecraft:block/cube_all","textures":{"all":"demo:block/ruby_block"}}"#.to_vec()),
        ("assets/demo/blockstates/ruby_block.json", br#"{"variants":{"":{"model":"demo:block/ruby_block"}}}"#.to_vec()),
        ("assets/demo/blockstates/ruby_ore.json", br#"{"variants":{"lit=true":{"model":"demo:block/ruby_block"},"lit=false":{"model":"demo:block/ruby_block"}}}"#.to_vec()),
        ("assets/demo/textures/item/ruby.png", opaque(1)),
        ("assets/demo/textures/item/ruby_sword.png", opaque(2)),
        ("assets/demo/textures/block/ruby_block.png", opaque(3)),
        ("data/demo/recipe/ruby_block.json", br###"{"type":"minecraft:crafting_shaped","key":{"#":"demo:ruby"},"pattern":["##","##"],"result":{"id":"demo:ruby_block"}}"###.to_vec()),
        ("data/demo/loot_table/blocks/ruby_block.json", br#"{"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"demo:ruby_block"}]}]}"#.to_vec()),
        ("com/demo/Mod.class", vec![0xca, 0xfe, 0xba, 0xbe]),
    ] {
        files.push((rel.to_string(), put(&m, rel, &body)));
    }
    let mod_jar = root.join("demo_mod.jar");
    kebitx_build::zip::write_zip(&mod_jar, &files).unwrap();

    let opened = input::open(&mod_jar).unwrap();
    let refs = References {
        java_jar: Some(&jar),
        recipes: Some(&recipes),
        loot: Some(&loot),
        bedrock_defs: Some(&defs),
        ..References::catalog_only(&catalog)
    };
    let mut r = inspect(&opened.root, refs);
    r.name = "demo_mod".into();
    assert_eq!(
        r.mod_info.as_ref().map(|(l, n)| (l.as_str(), n.as_str())),
        Some(("Fabric", "Demo Mod"))
    );
    let v = |f: &str| {
        r.entries
            .iter()
            .find(|e| e.java == f)
            .map(|e| (e.verdict, e.reason))
            .unwrap()
    };
    assert_eq!(v("assets/demo/models/item/ruby.json").0, Verdict::Converts);
    assert_eq!(
        v("data/demo/recipe/ruby_block.json").0,
        Verdict::Converts,
        "the recipe names the mod's own items"
    );
    assert_eq!(
        v("data/demo/loot_table/blocks/ruby_block.json").0,
        Verdict::Converts
    );
    assert!(v("assets/demo/blockstates/ruby_ore.json")
        .1
        .contains("permutations"));
    assert_eq!(v("com/demo/Mod.class").0, Verdict::Rebuild);
    // the block with states: drafted with a permutation per state
    let ore =
        kebitx_core::json::to_string(&r.draft_files["behavior_pack/blocks/demo_ruby_ore.json"]);
    assert!(
        ore.contains("query.block_state('demo:lit')==true")
            && ore.contains(r#""demo:lit":[false,true]"#),
        "{ore}"
    );

    let block = kebitx_core::json::to_string(&r.behavior["blocks/demo_ruby_block.json"]);
    assert!(
        block.contains(r#""minecraft:loot":"loot_tables/demo/blocks/ruby_block.json""#),
        "{block}"
    );
    assert!(
        block.contains(r#""culling":"kebitx:culling.demo_ruby_block""#),
        "{block}"
    );
    assert!(
        !block.contains("render_method"),
        "an opaque texture: Bedrock's default"
    );
    let sword = kebitx_core::json::to_string(&r.behavior["items/demo_ruby_sword.json"]);
    assert!(
        sword.contains(r#""minecraft:hand_equipped":true"#),
        "{sword}"
    );
    assert_eq!(
        r.texts["en_US"]
            .get("tile.demo:ruby_block.name")
            .map(String::as_str),
        Some("Block of Ruby")
    );

    let out = root.join("out");
    let c = convert(&opened.root, &r, &out).unwrap();
    for f in [
        "manifest.json",
        "textures/item_texture.json",
        "textures/terrain_texture.json",
        "textures/items/demo/ruby.png",
        "texts/en_US.lang",
        "kebitx_drafts/mod_tasks.txt",
    ] {
        assert!(out.join(f).is_file(), "{f}");
    }
    let bp_out = c.behavior_out.unwrap();
    for f in [
        "items/demo_ruby.json",
        "blocks/demo_ruby_block.json",
        "recipes/demo_ruby_block.json",
        "loot_tables/demo/blocks/ruby_block.json",
    ] {
        assert!(bp_out.join(f).is_file(), "{f}");
    }
    let tasks = fs::read_to_string(out.join("kebitx_drafts/mod_tasks.txt")).unwrap();
    assert!(
        tasks.contains("Demo Mod (Fabric mod)")
            && tasks.contains("ruby_ore")
            && tasks.contains("1 Java class"),
        "{tasks}"
    );
    drop(opened);
    let _ = fs::remove_dir_all(&root);
}
