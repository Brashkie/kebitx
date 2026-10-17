//! The recipe converter against both games' own recipes: Java's vanilla
//! recipes, converted (with the names the two games' recipes prove), must
//! mean what Bedrock's own recipes mean. They are not all the same — the
//! editions differ (Java dyes wool stairs, Bedrock makes dyes from its own
//! flowers) — but most must be, or the converter is wrong. Skipped where
//! either game is not installed.

use kebitx_migrate::recipes::{merge_cooking, parse_java, Recipe, RecipeRefs};

#[test]
fn most_java_recipes_convert_to_bedrocks_own() {
    let (Some(install), Some(java)) = (
        kebitx_core::vanilla::install_dir(),
        kebitx_migrate::JavaAssets::discover(),
    ) else {
        return;
    };
    let Some(jar) = java.jar_for(None) else {
        return;
    };
    let refs = RecipeRefs::build(&install, &jar);
    let parse = |b: &[u8]| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok();
    let names: Vec<String> = jar
        .zip
        .names()
        .filter(|n| n.starts_with("data/minecraft/recipe/"))
        .cloned()
        .collect();
    let java: Vec<Recipe> = merge_cooking(
        names
            .iter()
            .filter_map(|n| parse(&jar.read(n)?).and_then(|j| parse_java(&j)))
            .collect(),
    );
    let same = java
        .iter()
        .filter(|r| refs.bedrock.contains_key(&refs.to_bedrock_names(r)))
        .count();
    println!(
        "{same} of {} Java recipes are Bedrock's own, {} names learned",
        java.len(),
        refs.names.len()
    );
    assert!(same >= 800, "only {same} of {} recipes agree", java.len());
    // a few names the two games' recipes prove
    let item = |s: &str| kebitx_migrate::recipes::Alt::Item(s.into());
    assert_eq!(
        refs.names.get(&item("minecraft:sugar_cane")),
        Some(&item("minecraft:reeds"))
    );
    assert_eq!(
        refs.names.get(&item("minecraft:bricks")),
        Some(&item("minecraft:brick_block"))
    );
}
