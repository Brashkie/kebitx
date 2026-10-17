//! Repairs on copies of the fixtures: what is kept, what is rolled back, and
//! that a backup restores the files byte for byte.

use kebitx_core::vanilla::VanillaCatalog;
use kebitx_repair::{apply, plan, restore, score, Change, Fix, Plan};
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn copy(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to);
        } else {
            fs::copy(e.path(), to).unwrap();
        }
    }
}

/// A fresh copy of a fixture, in its own temp folder (backups go next to it).
fn scratch(test: &str, fixture_name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("kebitx-repair-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join(fixture_name);
    copy(&fixture(fixture_name), &root);
    root
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out: Vec<(PathBuf, Vec<u8>)> = kebitx_core::filesystem::all_files(root)
        .into_iter()
        .map(|f| {
            (
                f.strip_prefix(root).unwrap().to_path_buf(),
                fs::read(&f).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_case_repair_changes_only_the_wrong_literal_and_can_be_undone() {
    let root = scratch("case", "atlas_pack");
    let catalog = VanillaCatalog::empty();
    let original = snapshot(&root);
    let atlas = root.join("textures/terrain_texture.json");
    let atlas_before = fs::read_to_string(&atlas).unwrap();

    let p = plan(&root, &catalog, &[Fix::Case]);
    assert_eq!(p.changes.len(), 1, "{p:#?}");
    let Change::Edit {
        old,
        new,
        occurrences,
        ..
    } = &p.changes[0]
    else {
        panic!("an edit")
    };
    assert_eq!(
        (old.as_str(), new.as_str(), *occurrences),
        (
            "\"textures/blocks/desert/desert_3\"",
            "\"textures/blocks/desert/Desert_3\"",
            1
        )
    );
    assert_eq!(snapshot(&root), original, "planning writes nothing");

    let r = apply(&root, &p, &catalog).unwrap();
    assert!(r.kept, "{r:?}");
    assert_eq!((r.before.case_mismatches, r.after.case_mismatches), (1, 0));
    // byte for byte: the file is the original with that one literal replaced
    assert_eq!(
        fs::read_to_string(&atlas).unwrap(),
        atlas_before.replace(old.as_str(), new)
    );

    assert_eq!(restore(&r.backup).unwrap(), 1);
    assert_eq!(snapshot(&root), original, "restored exactly");
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn a_change_that_makes_things_worse_is_rolled_back() {
    let root = scratch("worse", "horse_pack");
    let catalog = VanillaCatalog::empty();
    let original = snapshot(&root);
    let used = root.join("textures/entity/horse2/horse_black.png");
    assert!(used.is_file());
    // deleting a texture the entity uses: one more broken reference
    let p = Plan {
        changes: vec![Change::Delete {
            file: used.clone(),
            bytes: 0,
            reason: "test".into(),
        }],
    };
    let r = apply(&root, &p, &catalog).unwrap();
    assert!(!r.kept);
    assert_eq!(r.after.broken, r.before.broken + 1);
    assert_eq!(
        snapshot(&root),
        original,
        "rolled back: every file as it was"
    );
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn a_file_changed_after_the_plan_is_refused() {
    let root = scratch("stale", "atlas_pack");
    let catalog = VanillaCatalog::empty();
    let p = plan(&root, &catalog, &[Fix::Case]);
    let atlas = root.join("textures/terrain_texture.json");
    let edited = fs::read_to_string(&atlas)
        .unwrap()
        .replace("desert_3", "desert_3_moved");
    fs::write(&atlas, &edited).unwrap();
    assert!(apply(&root, &p, &catalog)
        .unwrap_err()
        .contains("changed since the plan"));
    assert_eq!(
        fs::read_to_string(&atlas).unwrap(),
        edited,
        "left as the user had it"
    );
    let _ = fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn unused_files_are_deleted_only_when_asked_and_come_back_on_restore() {
    let root = scratch("unused", "world_assets");
    let catalog = VanillaCatalog::from_install(&fixture("fake_install"));
    let original = snapshot(&root);
    // the fixture has a case mismatch on purpose (`upper` / `Upper.png`): the
    // case repair fixes it, and deletes nothing
    let case_only = plan(&root, &catalog, &[Fix::Case]);
    assert!(!case_only.changes.is_empty());
    assert!(
        case_only
            .changes
            .iter()
            .all(|c| matches!(c, Change::Edit { .. })),
        "not asked: nothing deleted"
    );

    let p = plan(&root, &catalog, &[Fix::Unused]);
    let mut deleted: Vec<String> = p
        .changes
        .iter()
        .map(|c| {
            c.file()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    deleted.sort();
    assert_eq!(
        deleted,
        [
            "rp/textures/entity/orphan.png",
            "rp/textures/listed_only.png"
        ]
    );

    let before = score(&root, &catalog);
    let r = apply(&root, &p, &catalog).unwrap();
    assert!(r.kept);
    assert_eq!(r.after.unused_assets, 0);
    assert_eq!(
        r.after.broken, before.broken,
        "nothing that was used went away"
    );
    assert!(!root.join("rp/textures/entity/orphan.png").exists());

    restore(&r.backup).unwrap();
    assert_eq!(snapshot(&root), original);
    let _ = fs::remove_dir_all(root.parent().unwrap());
}
