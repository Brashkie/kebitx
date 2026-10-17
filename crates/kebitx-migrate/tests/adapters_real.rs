//! The permutation rules against Bedrock's own `shelf_mushroom`, the one
//! block both games define as data with states: every Java variant (age ×
//! facing, rotated by `y`), converted, must be Bedrock's permutation for the
//! same direction — same rotation. Skipped where either game is missing.

use kebitx_core::json::Json;
use kebitx_migrate::adapters::bedrock_rotation;

#[test]
fn java_facing_and_y_are_bedrocks_cardinal_direction_and_rotation() {
    let (Some(install), Some(java)) = (
        kebitx_core::vanilla::install_dir(),
        kebitx_migrate::JavaAssets::discover(),
    ) else {
        return;
    };
    let Some(jar) = java.jar_for(None) else {
        return;
    };
    let parse = |b: &[u8]| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok();
    let Some(java_state) = jar
        .read("assets/minecraft/blockstates/shelf_mushroom.json")
        .and_then(|b| parse(&b))
    else {
        return;
    };
    let bedrock = kebitx_core::vanilla::files::map_files_in(
        &install,
        kebitx_core::vanilla::files::PackKind::Behavior,
        |p| p == "blocks/shelf_mushroom_block.json",
        |_, b| parse(b),
    );
    let Some(bedrock) = bedrock.values().next() else {
        return;
    };

    // Bedrock: (direction, growth) -> rotation about y
    let mut theirs = std::collections::HashMap::new();
    for p in bedrock
        .get("minecraft:block")
        .and_then(|b| b.get("permutations"))
        .and_then(Json::as_array)
        .unwrap()
    {
        let cond: String = p
            .get("condition")
            .and_then(Json::as_str)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let Some(dir) = ["north", "south", "east", "west"]
            .into_iter()
            .find(|d| cond.contains(&format!("'{d}'")))
        else {
            continue;
        };
        let growth = if cond.contains("('growth')==1") { 1 } else { 0 };
        let rot = p
            .get("components")
            .and_then(|c| c.get("minecraft:transformation"))
            .and_then(|t| t.get("rotation"))
            .and_then(Json::as_array)
            .and_then(|r| match r.get(1) {
                Some(Json::Num(n)) => Some(*n as u32),
                _ => None,
            });
        // several permutations may match one state: the rotation is in the one
        // that has a transformation
        let slot = theirs.entry((dir.to_string(), growth)).or_insert(0);
        if let Some(r) = rot {
            *slot = r;
        }
    }
    let mut checked = 0;
    for (key, v) in java_state
        .get("variants")
        .and_then(Json::as_object)
        .unwrap()
        .iter()
    {
        let facing = key
            .split(',')
            .find_map(|kv| kv.strip_prefix("facing="))
            .unwrap()
            .to_string();
        let age: u32 = key
            .split(',')
            .find_map(|kv| kv.strip_prefix("age="))
            .unwrap()
            .parse()
            .unwrap();
        let y = match v.get("y") {
            Some(Json::Num(n)) => *n as u32,
            _ => 0,
        };
        // Bedrock writes no permutation for growth 0 facing north: no rotation
        let expected = theirs.get(&(facing.clone(), age)).copied().unwrap_or(0);
        assert_eq!(
            bedrock_rotation(y),
            expected,
            "facing={facing} age={age} y={y}"
        );
        checked += 1;
    }
    println!("{checked} Java variants rotate as Bedrock's permutations");
    assert_eq!(checked, 8);
}
