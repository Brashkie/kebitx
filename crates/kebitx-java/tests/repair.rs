//! Repairing a mods folder: the older of two copies goes, unless taking it
//! out breaks something else — then everything is put back on its own. And
//! `restore` (what `kebitx repair --undo` runs) brings the jar back.

use kebitx_java::modset::analyze_set;
use kebitx_java::repair::{apply, plan, Fix};
use std::path::{Path, PathBuf};

fn jar(dir: &Path, name: &str, fabric_mod_json: &str) -> PathBuf {
    let stage = dir.join(format!("{name}.stage"));
    std::fs::create_dir_all(&stage).unwrap();
    let meta = stage.join("fabric.mod.json");
    std::fs::write(&meta, fabric_mod_json).unwrap();
    let out = dir.join(name);
    kebitx_build::zip::write_zip(&out, &[("fabric.mod.json".into(), meta)]).unwrap();
    std::fs::remove_dir_all(&stage).unwrap();
    out
}

fn mods(name: &str, with_user_of_old: bool) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kebitx-jrepair-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mods = root.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    jar(
        &mods,
        "e-1.0.jar",
        r#"{"schemaVersion":1,"id":"e","version":"1.0.0"}"#,
    );
    jar(
        &mods,
        "e-2.0.jar",
        r#"{"schemaVersion":1,"id":"e","version":"2.0.0"}"#,
    );
    if with_user_of_old {
        // x only works with the old e
        jar(
            &mods,
            "x.jar",
            r#"{"schemaVersion":1,"id":"x","version":"1.0.0","depends":{"e":"<2.0"}}"#,
        );
    }
    mods
}

#[test]
fn the_older_copy_goes_and_undo_brings_it_back() {
    let dir = mods("dup", false);
    let report = analyze_set(&dir, None);
    let removals = plan(&report, &[Fix::Duplicates]);
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0].jar, "e-1.0.jar");

    let applied = apply(&dir, &removals, None).unwrap();
    assert!(applied.kept, "{applied:?}");
    assert_eq!((applied.before, applied.after), (1, 0));
    assert!(!dir.join("e-1.0.jar").exists() && dir.join("e-2.0.jar").exists());
    // the backup is next to the folder, never in it (a jar there would load)
    assert!(!applied.backup.starts_with(&dir));

    let restored = kebitx_repair::restore(&applied.backup).unwrap();
    assert_eq!(restored, 1);
    assert!(dir.join("e-1.0.jar").exists());
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn a_removal_that_breaks_another_mod_is_rolled_back() {
    let dir = mods("needed", true);
    let report = analyze_set(&dir, None);
    let removals = plan(&report, &[Fix::Duplicates]);
    assert_eq!(removals[0].jar, "e-1.0.jar");

    let applied = apply(&dir, &removals, None).unwrap();
    assert!(!applied.kept, "{applied:?}");
    // x would now find only e 2.0: a new problem, so nothing is kept
    assert!(
        applied
            .new_problems
            .iter()
            .any(|p| p.contains("WrongVersion")),
        "{:?}",
        applied.new_problems
    );
    assert!(dir.join("e-1.0.jar").exists(), "the jar is back");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}
