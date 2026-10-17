//! The loot-table converter against both games' own tables. Many differ for
//! real (Bedrock's chests hold other things, it splits double slabs into
//! their own tables), but a good share must mean the same, and the names
//! learned from them must be right. Skipped where either game is missing.

use kebitx_migrate::loot::{same_as_bedrock, LootRefs};

#[test]
fn java_tables_mean_bedrocks_own_and_names_are_right() {
    let (Some(install), Some(java)) = (
        kebitx_core::vanilla::install_dir(),
        kebitx_migrate::JavaAssets::discover(),
    ) else {
        return;
    };
    let Some(jar) = java.jar_for(None) else {
        return;
    };
    let refs = LootRefs::build(&install, &jar);
    println!(
        "{} Java tables mean Bedrock's at the same path; names {:?}",
        refs.same.len(),
        refs.names
    );
    assert!(
        refs.same.len() >= 80,
        "only {} tables agree",
        refs.same.len()
    );
    assert_eq!(
        refs.names.get("minecraft:cod").map(String::as_str),
        Some("minecraft:fish")
    );
    // lined up only because the editions list entries in another order:
    // never learned
    assert_ne!(
        refs.names
            .get("minecraft:iron_horse_armor")
            .map(String::as_str),
        Some("minecraft:copper_horse_armor")
    );
    // the cow, the same in both
    let cow = &refs.bedrock["entities/cow.json"];
    assert!(same_as_bedrock(cow, cow));
    assert!(refs.same.contains("entities/cow.json"));
}
