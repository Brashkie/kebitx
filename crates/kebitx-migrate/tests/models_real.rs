//! The model converter against Mojang's own conversion: the Bedrock game ships
//! `straw_bed_foot` geometry converted from the Java game's model of the same
//! name. Converting the Java model must give the same cubes and faces.
//! (`straw_bed_head` is hand-finished in Bedrock — faces against the other
//! half removed, frills nudged — so it is not a rule to reproduce.)
//! Skipped where either game is not installed.

use kebitx_core::json::Json;
use kebitx_migrate::models::{resolve, to_geometry};

fn cubes(geo: &Json) -> Vec<(String, Json)> {
    let bones = geo
        .get("minecraft:geometry")
        .and_then(|g| g.as_array())
        .and_then(|g| g.first())
        .and_then(|g| g.get("bones"))
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    bones
        .iter()
        .map(|b| {
            let name = b
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string();
            let cube = b
                .get("cubes")
                .and_then(Json::as_array)
                .and_then(|c| c.first())
                .cloned()
                .unwrap_or(Json::Null);
            (name, cube)
        })
        .collect()
}

fn same_number(a: Option<&Json>, b: Option<&Json>) -> bool {
    match (a, b) {
        (Some(Json::Arr(x)), Some(Json::Arr(y))) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_number(Some(p), Some(q)))
        }
        (Some(Json::Num(x)), Some(Json::Num(y))) => (x - y).abs() < 1e-9,
        (a, b) => a == b,
    }
}

#[test]
fn mojangs_own_conversion_is_reproduced() {
    let (Some(install), Some(java)) = (
        kebitx_core::vanilla::install_dir(),
        kebitx_migrate::JavaAssets::discover(),
    ) else {
        return;
    };
    let Some(jar) = java.jar_for(None) else {
        return;
    };
    let theirs = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("models/blocks/straw_bed"),
        |_, b| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok(),
    );
    let culling = kebitx_core::vanilla::files::map_files(
        &install,
        |p| p.starts_with("block_culling/"),
        |_, b| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok(),
    );
    let mut checked = 0;
    for name in ["straw_bed_foot"] {
        let Some(bedrock) = theirs.get(&format!("models/blocks/{name}.geo.json")) else {
            continue;
        };
        let Ok(model) = resolve(
            &format!("assets/minecraft/models/block/{name}.json"),
            std::path::Path::new("<none>"),
            Some(&jar),
        ) else {
            continue;
        };
        let converted = to_geometry("geometry.x", &model).unwrap();
        // the culling rules: Mojang's are exactly the Java cullfaces
        if let Some(rules) = culling.get(&format!("block_culling/{name}.culling.json")) {
            let mut theirs: Vec<(String, String, String)> = rules
                .get("minecraft:block_culling_rules")
                .and_then(|r| r.get("rules"))
                .and_then(Json::as_array)
                .unwrap()
                .iter()
                .map(|r| {
                    let p = r.get("geometry_part").unwrap();
                    let t = |v: Option<&Json>| v.and_then(Json::as_str).unwrap_or("").to_string();
                    (t(p.get("bone")), t(p.get("face")), t(r.get("direction")))
                })
                .collect();
            let mut ours = converted.culling.clone();
            theirs.sort();
            ours.sort();
            assert_eq!(ours, theirs, "{name}: culling rules");
            println!("{} culling rules match Mojang's", ours.len());
        }
        let ours = converted.json;
        let (ours, bedrock) = (cubes(&ours), cubes(bedrock));
        assert_eq!(ours.len(), bedrock.len(), "{name}: bones");
        for ((on, oc), (bn, bc)) in ours.iter().zip(&bedrock) {
            assert_eq!(on, bn, "{name}: bone order");
            for k in ["origin", "size"] {
                assert!(
                    same_number(oc.get(k), bc.get(k)),
                    "{name}/{on} {k}: {:?} vs {:?}",
                    oc.get(k),
                    bc.get(k)
                );
            }
            let (ou, bu) = (oc.get("uv").unwrap(), bc.get("uv").unwrap());
            for face in ["north", "east", "south", "west", "up", "down"] {
                let (of, bf) = (ou.get(face), bu.get(face));
                assert_eq!(of.is_some(), bf.is_some(), "{name}/{on} {face} present");
                if let (Some(of), Some(bf)) = (of, bf) {
                    for k in ["uv", "uv_size", "uv_rotation", "material_instance"] {
                        assert!(
                            same_number(of.get(k), bf.get(k)),
                            "{name}/{on} {face}.{k}: {:?} vs {:?}",
                            of.get(k),
                            bf.get(k)
                        );
                    }
                }
            }
            checked += 1;
        }
    }
    println!("{checked} cubes compared with Mojang's conversion");
    assert!(checked == 0 || checked >= 4, "{checked} cubes compared");
}
