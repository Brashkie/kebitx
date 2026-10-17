//! Regression checks: `horse_pack` (healthy) against `horse_pack_broken`
//! (one texture file gone), in both directions, and a copy against itself.

mod common;
use common::fixture;
use kebitx_core::regression::{compare, snapshot};
use kebitx_core::vanilla::VanillaCatalog;

#[test]
fn a_removed_texture_is_a_regression_and_its_return_a_fix() {
    let catalog = VanillaCatalog::empty();
    let healthy = snapshot(&fixture("horse_pack"), &catalog);
    let broken = snapshot(&fixture("horse_pack_broken"), &catalog);

    let r = compare(&healthy, &broken);
    assert!(r.is_regression());
    assert_eq!(r.new_problems.len(), 1);
    assert!(
        r.new_problems[0].contains("horse_armor_leather"),
        "{:?}",
        r.new_problems
    );
    assert!(r.fixed_problems.is_empty());
    assert_eq!(
        r.removed,
        [(
            "texture or sound",
            "textures/entity/horse2/armor/horse_armor_leather".to_string()
        )]
    );
    assert!(r.added.is_empty());

    // the other way round: nothing new, one fixed, one added
    let back = compare(&broken, &healthy);
    assert!(!back.is_regression());
    assert_eq!(back.fixed_problems.len(), 1);
    assert_eq!(back.added.len(), 1);
}

#[test]
fn the_same_project_in_another_folder_is_no_change() {
    let catalog = VanillaCatalog::empty();
    let dir = std::env::temp_dir().join(format!("kebitx-regression-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixture("atlas_pack"), &dir.join("elsewhere"));
    let r = compare(
        &snapshot(&fixture("atlas_pack"), &catalog),
        &snapshot(&dir.join("elsewhere"), &catalog),
    );
    assert_eq!(
        r,
        Default::default(),
        "identical content compares equal: {r:#?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a world converter that drops the packs (Chunker, Bedrock -> Java ->
/// Bedrock) leaves: chunks and no packs. Its problems are not fixed, they are
/// lost with the pack, and the missing pack is the regression.
#[test]
fn a_pack_that_disappears_is_a_regression_not_a_fix() {
    let catalog = VanillaCatalog::empty();
    let dir = std::env::temp_dir().join(format!("kebitx-regression-gone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let world = dir.join("world");
    resource_pack(&world.join("resource_packs/horse"));
    let stripped = dir.join("stripped");
    std::fs::create_dir_all(stripped.join("db")).unwrap();

    let before = snapshot(&world, &catalog);
    assert!(
        !before.problems.is_empty(),
        "the fixture has a broken texture"
    );
    let r = compare(&before, &snapshot(&stripped, &catalog));
    assert!(r.is_regression(), "{r:#?}");
    assert_eq!(r.removed_packs, ["horse"]);
    assert!(r.fixed_problems.is_empty(), "nothing was fixed: {r:#?}");
    assert_eq!(r.lost_problems.len(), before.problems.len());

    // renaming the pack's folder keeps its UUID: no pack gone, nothing lost
    let renamed = dir.join("renamed");
    resource_pack(&renamed.join("resource_packs/horse_v2"));
    let r = compare(&before, &snapshot(&renamed, &catalog));
    assert!(
        r.removed_packs.is_empty() && r.lost_problems.is_empty(),
        "{r:#?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `horse_pack_broken` as a world's resource pack, with a manifest.
fn resource_pack(dst: &std::path::Path) {
    copy(&fixture("horse_pack_broken"), dst);
    std::fs::write(
        dst.join("manifest.json"),
        r#"{"format_version":2,"header":{"name":"horse","uuid":"6f1c1a2e-0000-4000-8000-000000000001","version":[1,0,0],"min_engine_version":[1,20,0]},"modules":[{"type":"resources","uuid":"6f1c1a2e-0000-4000-8000-000000000002","version":[1,0,0]}]}"#,
    )
    .unwrap();
}

fn copy(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}
