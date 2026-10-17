//! `kebitx build` end to end, on a copy of `fixtures/build_project`, with the
//! compiled example filter as a real external program.

use kebitx_build::fsutil::{copy_dir, MARKER};
use kebitx_build::{build, BuildOptions, Project, Target};
use kebitx_core::json;
use kebitx_core::vanilla::VanillaCatalog;
use std::fs;
use std::path::{Path, PathBuf};

const FILTER: &str = env!("CARGO_BIN_EXE_kebitx-filter-example");

/// A fresh copy of the fixture in the temp folder, with a project file whose
/// filters run the example filter with `filter_args`.
fn project(test: &str, filter_args: &[&str], export: &str) -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!("kebitx-build-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/build_project/packs");
    copy_dir(&fixture, &root.join("packs")).unwrap();
    let filters: Vec<String> = filter_args
        .iter()
        .map(|a| {
            format!(
                r#"{{ "name": "example {a}", "run": [{:?}, {a:?}] }}"#,
                FILTER
            )
        })
        .collect();
    let src = format!(
        r#"{{ "name": "demo", "packs": {{ "behavior": "packs/BP", "resource": "packs/RP" }},
             "filters": [{}], "export": {export} }}"#,
        filters.join(", ")
    );
    fs::write(root.join("kebitx.json"), &src).unwrap();
    let p = Project::load(&root).unwrap();
    (root, p)
}

fn empty() -> VanillaCatalog {
    VanillaCatalog::empty()
}

#[test]
fn local_build_runs_filters_on_a_copy_and_exports() {
    let (root, p) = project("local", &["stamp"], r#"{ "target": "local" }"#);
    let r = build(&p, &BuildOptions::default(), &empty()).unwrap();
    assert_eq!(r.filters.len(), 1);
    assert!(r.checked);
    assert_eq!((r.broken, r.compat_errors), (0, 0));

    let bp = root.join("build/demo_BP");
    assert_eq!(
        fs::read_to_string(bp.join("built_by.txt")).unwrap(),
        "demo\n"
    );
    assert!(bp.join(MARKER).is_file());
    assert!(root
        .join("build/demo_RP/textures/entity/crate.png")
        .is_file());
    // the source is never modified
    assert!(!root.join("packs/BP/built_by.txt").exists());

    // a second build replaces its own output
    build(&p, &BuildOptions::default(), &empty()).unwrap();
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_failing_filter_stops_the_build() {
    let (root, p) = project("fail", &["stamp", "fail"], r#"{ "target": "local" }"#);
    let e = build(&p, &BuildOptions::default(), &empty()).unwrap_err();
    assert_eq!(e.stage, "filter");
    assert_eq!(
        e.report.filters.len(),
        1,
        "the first filter ran, the second failed"
    );
    assert!(!root.join("build").exists(), "nothing exported");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_analysis_gates_the_export() {
    let (root, p) = project("check", &[], r#"{ "target": "local" }"#);
    // a client entity naming a texture nobody has
    fs::write(
        root.join("packs/RP/entity/ghost.entity.json"),
        r#"{"format_version": "1.10.0", "minecraft:client_entity": {"description": {
            "identifier": "demo:ghost", "textures": {"default": "textures/entity/ghost"}, "geometry": {}}}}"#,
    )
    .unwrap();
    let e = build(&p, &BuildOptions::default(), &empty()).unwrap_err();
    assert_eq!(e.stage, "check");
    assert_eq!(e.report.broken, 1);
    assert!(!root.join("build").exists());

    // `--no-check` lets it through
    let opts = BuildOptions {
        check: Some(false),
        ..Default::default()
    };
    build(&p, &opts, &empty()).unwrap();
    assert!(root
        .join("build/demo_RP/entity/ghost.entity.json")
        .is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn development_export_never_deletes_what_it_did_not_create() {
    let (root, p) = project(
        "dev",
        &[],
        r#"{ "target": "development", "com_mojang": "fake_com_mojang" }"#,
    );
    let com = root.join("fake_com_mojang");
    let r = build(&p, &BuildOptions::default(), &empty()).unwrap();
    assert_eq!(
        r.exported,
        [
            com.join("development_behavior_packs/demo_BP"),
            com.join("development_resource_packs/demo_RP")
        ]
    );

    // somebody's own pack sits where ours would go: refused, untouched
    let theirs = com.join("development_behavior_packs/demo_BP");
    fs::remove_dir_all(&theirs).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    fs::write(theirs.join("precious.json"), "{}").unwrap();
    let e = build(&p, &BuildOptions::default(), &empty()).unwrap_err();
    assert_eq!(e.stage, "export");
    assert!(theirs.join("precious.json").is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn mcaddon_is_a_zip_with_both_packs() {
    let (root, p) = project("mcaddon", &["stamp"], r#"{ "target": "local" }"#);
    let opts = BuildOptions {
        target: Some(Target::McAddon),
        ..Default::default()
    };
    let r = build(&p, &opts, &empty()).unwrap();
    let file = root.join("build/demo.mcaddon");
    assert_eq!(r.exported, std::slice::from_ref(&file));
    let z = fs::read(&file).unwrap();
    assert_eq!(&z[..2], b"PK");
    // BP: manifest, entity, built_by.txt; RP: manifest, entity, texture
    let end = &z[z.len() - 22..];
    assert_eq!(u16::from_le_bytes([end[10], end[11]]), 6);
    let text = String::from_utf8_lossy(&z);
    assert!(text.contains("demo_BP/built_by.txt"));
    assert!(text.contains("demo_RP/manifest.json"));
    let _ = fs::remove_dir_all(&root);
}

/// The entries of a STORED zip, in archive order: (name, contents).
fn unzip(path: &Path) -> Vec<(String, Vec<u8>)> {
    let z = fs::read(path).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while z[i..].starts_with(&0x0403_4b50u32.to_le_bytes()) {
        let u16_at = |o: usize| u16::from_le_bytes([z[i + o], z[i + o + 1]]) as usize;
        let size = u32::from_le_bytes(z[i + 18..i + 22].try_into().unwrap()) as usize;
        let (name_len, extra) = (u16_at(26), u16_at(28));
        let name = String::from_utf8(z[i + 30..i + 30 + name_len].to_vec()).unwrap();
        let data = i + 30 + name_len + extra;
        out.push((name, z[data..data + size].to_vec()));
        i = data + size;
    }
    out
}

#[test]
fn mcpack_is_one_archive_per_pack_with_files_at_the_root() {
    let (root, p) = project("mcpack", &["stamp"], r#"{ "target": "mcpack" }"#);
    let r = build(&p, &BuildOptions::default(), &empty()).unwrap();
    assert_eq!(
        r.exported,
        [
            root.join("build/demo_BP.mcpack"),
            root.join("build/demo_RP.mcpack")
        ]
    );
    let names: Vec<String> = unzip(&r.exported[0]).into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        ["built_by.txt", "entities/crate.json", "manifest.json"]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn mcworld_puts_the_packs_in_the_template_and_activates_them() {
    let (root, p) = project(
        "mcworld",
        &[],
        r#"{ "target": "mcworld", "world": "world" }"#,
    );
    let fixture_world =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/build_project/world");
    copy_dir(&fixture_world, &root.join("world")).unwrap();
    let p = Project {
        world: Some(root.join("world")),
        ..p
    };

    let r = build(&p, &BuildOptions::default(), &empty()).unwrap();
    assert_eq!(r.exported, [root.join("build/demo.mcworld")]);
    let entries = unzip(&r.exported[0]);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    for want in [
        "level.dat",
        "levelname.txt",
        "db/CURRENT",
        "behavior_packs/demo_BP/manifest.json",
        "resource_packs/demo_RP/textures/entity/crate.png",
        "world_behavior_packs.json",
        "world_resource_packs.json",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    assert!(
        !names.contains(&"behavior_packs/demo_BP/old.json"),
        "the template's stale copy is dropped"
    );

    let list = |name: &str| -> json::Json {
        let (_, bytes) = entries.iter().find(|(n, _)| n == name).unwrap();
        json::parse(std::str::from_utf8(bytes).unwrap()).unwrap()
    };
    // somebody else's pack is kept; our old version is replaced by the built one
    assert_eq!(
        json::to_string(&list("world_behavior_packs.json")),
        r#"[{"pack_id":"99999999-0000-0000-0000-000000000001","version":[2,1,0]},{"pack_id":"77777777-0000-0000-0000-000000000001","version":[1,0,0]}]"#
    );
    assert_eq!(
        json::to_string(&list("world_resource_packs.json")),
        r#"[{"pack_id":"88888888-0000-0000-0000-000000000001","version":[1,0,0]}]"#
    );

    // without a template, the target explains what it needs
    let no_world = Project { world: None, ..p };
    let e = build(&no_world, &BuildOptions::default(), &empty()).unwrap_err();
    assert!(e.message.contains("export.world"), "{}", e.message);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn project_file_errors_are_explained() {
    let root = Path::new("/nowhere");
    let bad = |s: &str| Project::from_json(root, &json::parse(s).unwrap()).unwrap_err();
    assert!(bad(r#"{ "packs": {} }"#).contains("name"));
    assert!(bad(r#"{ "name": "../escape", "packs": {} }"#).contains("folder name"));
    assert!(
        bad(r#"{ "name": "x", "packs": { "behavior": "missing" } }"#).contains("manifest.json")
    );
}
