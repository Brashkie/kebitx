//! A Java data pack, end to end, on games made in a temporary folder: a
//! changed vanilla recipe replaces Bedrock's own (same identifier), a new
//! one is converted with the names the games prove, an unknown item stays.

use kebitx_core::codec::zip::ZipArchive;
use kebitx_core::vanilla::VanillaCatalog;
use kebitx_migrate::recipes::RecipeRefs;
use kebitx_migrate::{convert, inspect, JavaJar, References, Verdict};
use std::fs;
use std::path::{Path, PathBuf};

fn put(root: &Path, rel: &str, body: &str) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, body).unwrap();
    p
}

#[test]
fn recipes_convert_override_or_stay() {
    let root = std::env::temp_dir().join(format!("kebitx-datapack-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    // the Java game: a stick recipe, and paper from sugar cane
    let src = root.join("jar");
    let stick = r####"{"type":"minecraft:crafting_shaped","key":{"#":"#minecraft:planks"},"pattern":["#","#"],"result":{"id":"minecraft:stick","count":4}}"####;
    let paper = r####"{"type":"minecraft:crafting_shaped","key":{"#":"minecraft:sugar_cane"},"pattern":["###"],"result":{"id":"minecraft:paper","count":3}}"####;
    let entries = vec![
        (
            "data/minecraft/recipe/stick.json".to_string(),
            put(&src, "stick.json", stick),
        ),
        (
            "data/minecraft/recipe/paper.json".to_string(),
            put(&src, "paper.json", paper),
        ),
        (
            "data/minecraft/loot_table/entities/cow.json".to_string(),
            put(&src, "cow.json", COW_JAVA),
        ),
    ];
    let jar_path = root.join("game.jar");
    kebitx_build::zip::write_zip(&jar_path, &entries).unwrap();
    let jar = JavaJar {
        id: "t".into(),
        format: 97,
        exact: true,
        zip: ZipArchive::open(&jar_path).unwrap(),
    };

    // the Bedrock game: the same two recipes, Bedrock's names
    let bp = root.join("install/data/behavior_packs/vanilla");
    put(
        &bp,
        "recipes/stick.json",
        r#"{"minecraft:recipe_shaped":{"description":{"identifier":"minecraft:stick"},"tags":["crafting_table"],"pattern":["A","A"],"key":{"A":{"tag":"minecraft:planks"}},"result":{"item":"stick","count":4}}}"#,
    );
    put(
        &bp,
        "recipes/paper.json",
        r#"{"minecraft:recipe_shaped":{"description":{"identifier":"minecraft:paper"},"tags":["crafting_table"],"pattern":["AAA"],"key":{"A":{"item":"minecraft:reeds"}},"result":{"item":"minecraft:paper","count":3}}}"#,
    );
    put(&bp, "loot_tables/entities/cow.json", COW_BEDROCK);
    fs::create_dir_all(root.join("install/data/resource_packs/vanilla")).unwrap();
    let install = root.join("install");
    let catalog = VanillaCatalog::from_install(&install);
    let recipes = RecipeRefs::build(&install, &jar);
    let loot = kebitx_migrate::loot::LootRefs::build(&install, &jar);
    assert!(
        loot.same.contains("entities/cow.json"),
        "the two cows mean the same"
    );

    // the pack
    let pack = root.join("pack");
    put(
        &pack,
        "pack.mcmeta",
        r#"{"pack":{"pack_format":97,"description":"d"}}"#,
    );
    put(
        &pack,
        "data/minecraft/recipe/stick.json",
        &stick.replace("\"count\":4", "\"count\":8"),
    );
    put(&pack, "data/minecraft/recipe/paper.json", paper);
    put(
        &pack,
        "data/demo/recipe/cane.json",
        r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:sugar_cane"],"result":{"id":"minecraft:paper"}}"#,
    );
    put(
        &pack,
        "data/demo/recipe/odd.json",
        r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:no_such_thing"],"result":{"id":"minecraft:paper"}}"#,
    );

    let refs = References {
        java_jar: Some(&jar),
        recipes: Some(&recipes),
        loot: Some(&loot),
        ..References::catalog_only(&catalog)
    };
    // a cow that drops more leather, and a recipe using the pack's own tag
    put(
        &pack,
        "data/minecraft/loot_table/entities/cow.json",
        &COW_JAVA.replace("\"max\":2", "\"max\":5"),
    );
    put(
        &pack,
        "data/demo/tags/item/sweet.json",
        r#"{"values":["minecraft:sugar_cane","minecraft:stick"]}"#,
    );
    put(
        &pack,
        "data/demo/recipe/sweet.json",
        r##"{"type":"minecraft:crafting_shapeless","ingredients":["#demo:sweet"],"result":{"id":"minecraft:paper"}}"##,
    );
    let r = inspect(&pack, refs);
    let v = |f: &str| {
        r.entries
            .iter()
            .find(|e| e.java == f)
            .map(|e| (e.verdict, e.bedrock.clone(), e.reason))
            .unwrap()
    };
    assert_eq!(
        v("data/minecraft/recipe/paper.json").0,
        Verdict::Converts,
        "unchanged"
    );
    assert_eq!(
        v("data/minecraft/recipe/paper.json").1,
        None,
        "nothing to write"
    );
    let (verdict, to, reason) = v("data/minecraft/recipe/stick.json");
    assert_eq!(
        (verdict, to.as_deref()),
        (
            Verdict::Converts,
            Some("behavior_pack/recipes/minecraft_stick.json")
        )
    );
    assert!(reason.contains("replaces"), "{reason}");
    assert_eq!(v("data/demo/recipe/cane.json").0, Verdict::Converts);
    assert_eq!(v("data/demo/recipe/odd.json").0, Verdict::Adapts);

    let cane = kebitx_core::json::to_string(&r.behavior["recipes/demo_cane.json"]);
    assert!(
        cane.contains(r#""item":"minecraft:reeds""#),
        "sugar cane is reeds in Bedrock: {cane}"
    );
    let stick_out = kebitx_core::json::to_string(&r.behavior["recipes/minecraft_stick.json"]);
    assert!(
        stick_out.contains(r#""identifier":"minecraft:stick""#)
            && stick_out.contains(r#""count":8"#),
        "{stick_out}"
    );

    let out = root.join("out");
    let (verdict, to, reason) = v("data/minecraft/loot_table/entities/cow.json");
    assert_eq!(
        (verdict, to.as_deref()),
        (
            Verdict::Converts,
            Some("behavior_pack/loot_tables/entities/cow.json")
        )
    );
    assert!(reason.contains("the Java original means"), "{reason}");
    assert!(
        kebitx_core::json::to_string(&r.behavior["loot_tables/entities/cow.json"])
            .contains(r#""max":5"#)
    );
    // the tag spelled out: one recipe per item, Bedrock's names
    assert!(
        r.behavior.contains_key("recipes/demo_sweet_1.json")
            && r.behavior.contains_key("recipes/demo_sweet_2.json")
    );
    let both = kebitx_core::json::to_string(&r.behavior["recipes/demo_sweet_1.json"])
        + &kebitx_core::json::to_string(&r.behavior["recipes/demo_sweet_2.json"]);
    assert!(
        both.contains("minecraft:reeds") && both.contains("minecraft:stick"),
        "{both}"
    );
    let c = convert(&pack, &r, &out).unwrap();
    let bp_out = c.behavior_out.expect("a behavior pack");
    assert!(
        bp_out.join("manifest.json").is_file() && bp_out.join("recipes/demo_cane.json").is_file()
    );
    let _ = fs::remove_dir_all(&root);
}

const COW_JAVA: &str = r#"{"type":"minecraft:entity","pools":[{"entries":[{"type":"minecraft:item","modifier":[{"type":"minecraft:set_count","count":{"type":"minecraft:uniform","max":2,"min":0}}],"name":"minecraft:leather"}],"rolls":1}]}"#;
const COW_BEDROCK: &str = r#"{"pools":[{"rolls":1,"entries":[{"type":"item","name":"minecraft:leather","weight":1,"functions":[{"function":"set_count","count":{"min":0,"max":2}}]}]}]}"#;
