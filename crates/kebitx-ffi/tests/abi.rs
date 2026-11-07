//! The C ABI called exactly as C would call it: raw pointers in and out.

use kebitx_core::json::{self, Json};
use kebitx_ffi::{kebitx_analyze, kebitx_string_free, kebitx_version};
use std::ffi::{CStr, CString};
use std::path::Path;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// Call `kebitx_analyze` like a C program, and free the result.
fn analyze(path: &str) -> Option<Json> {
    let c = CString::new(path).unwrap();
    // SAFETY: `c` is a valid NUL-terminated string alive across the call.
    let out = unsafe { kebitx_analyze(c.as_ptr()) };
    if out.is_null() {
        return None;
    }
    // SAFETY: not NULL, returned by kebitx_analyze, NUL-terminated.
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    // SAFETY: returned by kebitx_analyze and not freed yet.
    unsafe { kebitx_string_free(out) };
    Some(json::parse(&text).expect("valid JSON"))
}

#[test]
fn version_is_a_static_string() {
    // SAFETY: kebitx_version returns a static NUL-terminated string.
    let v = unsafe { CStr::from_ptr(kebitx_version()) }
        .to_str()
        .unwrap();
    assert_eq!(v, env!("CARGO_PKG_VERSION"));
}

#[test]
fn bad_input_is_null_never_a_crash() {
    // SAFETY: NULL is an accepted input.
    assert!(unsafe { kebitx_analyze(std::ptr::null()) }.is_null());
    assert!(analyze("Z:/this/does/not/exist").is_none());
    // SAFETY: NULL is accepted and ignored.
    unsafe { kebitx_string_free(std::ptr::null_mut()) };
}

#[test]
fn a_report_carries_counts_and_explained_issues() {
    let r = analyze(&fixture("atlas_pack")).expect("a report");
    assert_eq!(r.get("mode").and_then(Json::as_str), Some("pack"));
    assert_eq!(r.get("case_mismatches"), Some(&Json::Num(1.0)));
    let issues = r.get("issues").and_then(Json::as_array).unwrap();
    assert!(!issues.is_empty());
    for i in issues {
        for key in ["kind", "owner", "reference", "confidence", "fix"] {
            assert!(
                i.get(key)
                    .and_then(Json::as_str)
                    .is_some_and(|s| !s.is_empty() || key == "pack"),
                "{key} in {i:?}"
            );
        }
    }
    let w = analyze(&fixture("world_compat")).expect("a report");
    assert_eq!(w.get("mode").and_then(Json::as_str), Some("world"));
    // three errors the files decide alone, plus a `min_engine_version` above
    // the installed game: only checked when there is a game with a known
    // version (none on a CI machine), as the report itself says
    let game_known = w
        .get("game_version")
        .and_then(Json::as_str)
        .and_then(kebitx_core::manifest::parse_version)
        .is_some();
    let expected = if game_known { 4.0 } else { 3.0 };
    assert_eq!(
        w.get("compat_errors"),
        Some(&Json::Num(expected)),
        "game version: {:?}",
        w.get("game_version")
    );
}

/// `kebitx_report` like a C program.
fn report(path: &str) -> Option<Json> {
    let c = CString::new(path).unwrap();
    // SAFETY: `c` is a valid NUL-terminated string alive across the call.
    let out = unsafe { kebitx_ffi::kebitx_report(c.as_ptr()) };
    if out.is_null() {
        return None;
    }
    // SAFETY: not NULL, returned by kebitx_report, NUL-terminated.
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    // SAFETY: returned by kebitx_report and not freed yet.
    unsafe { kebitx_string_free(out) };
    Some(json::parse(&text).expect("valid JSON"))
}

#[test]
fn a_report_says_what_it_read_and_gives_lines() {
    let r = report(&fixture("atlas_pack")).expect("a pack");
    assert_eq!(r.get("kind").and_then(Json::as_str), Some("Bedrock pack"));
    let problems = r.get("problems").and_then(Json::as_array).unwrap();
    let broken = match r.get("broken") {
        Some(Json::Num(n)) => *n as usize,
        _ => panic!("no count"),
    };
    // one empty texture set and one case mismatch: the count is what is listed
    assert_eq!(broken, 2, "{problems:?}");
    assert_eq!(problems.len(), broken);
    assert!(!r
        .get("summary")
        .and_then(Json::as_array)
        .unwrap()
        .is_empty());
    assert!(report("no/such/folder").is_none());
}

/// A wrong folder finds nothing wrong, and that is no verdict: it must never read
/// as "OK".
#[test]
fn a_folder_with_nothing_in_it_says_so() {
    let is_nothing = |r: &Json| r.get("nothing") == Some(&Json::Bool(true));
    let dir = std::env::temp_dir().join(format!("kebitx-abi-nothing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("photos")).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a pack").unwrap();
    let r = report(dir.to_str().unwrap()).expect("a report");
    assert!(is_nothing(&r), "{r:?}");
    assert_eq!(
        r.get("kind").and_then(Json::as_str),
        Some("No pack, world or mods here")
    );

    // a folder of behavior packs: it says which way to go
    let pack = dir.join("my_pack");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(
        pack.join("manifest.json"),
        r#"{"format_version":2,"header":{"name":"p","uuid":"u","version":[1,0,0]},
            "modules":[{"type":"data","uuid":"m","version":[1,0,0]}]}"#,
    )
    .unwrap();
    let r = report(dir.to_str().unwrap()).expect("a report");
    assert!(is_nothing(&r), "{r:?}");
    let summary = r.get("summary").and_then(Json::as_array).unwrap();
    assert_eq!(
        summary[0].as_str(),
        Some("This folder holds 1 pack: open that one.")
    );
    // the pack itself: a behavior pack that names nothing has no verdict
    let r = report(pack.to_str().unwrap()).unwrap();
    assert!(is_nothing(&r), "{r:?}");
    assert_eq!(
        r.get("kind").and_then(Json::as_str),
        Some("Behavior pack with nothing to check")
    );
    std::fs::remove_dir_all(&dir).unwrap();

    // a world with no packs: nothing was checked, so no verdict
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("level.dat"), "").unwrap();
    let r = report(dir.to_str().unwrap()).expect("a report");
    assert!(is_nothing(&r), "{r:?}");
    assert_eq!(
        r.get("kind").and_then(Json::as_str),
        Some("World with no packs")
    );
    std::fs::remove_dir_all(&dir).unwrap();

    // and everything that is a pack or a world stays one, with or without a manifest
    for name in [
        "atlas_pack",
        "horse_pack",
        "copied_cow_pack",
        "materials_pack",
        "models_both_formats",
        "world_anims",
        "world_assets",
        "world_compat",
        "world_graphics",
        "world_rc",
        "world_scripts",
        "world_two_packs",
    ] {
        let r = report(&fixture(name)).expect("a report");
        assert!(!is_nothing(&r), "{name} is not nothing");
    }
}

#[test]
fn places_is_a_report_whatever_this_computer_has() {
    let out = kebitx_ffi::kebitx_places();
    assert!(!out.is_null());
    // SAFETY: not NULL, returned by kebitx_places, NUL-terminated.
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    // SAFETY: returned by kebitx_places and not freed yet.
    unsafe { kebitx_string_free(out) };
    let r = json::parse(&text).expect("valid JSON");
    assert_eq!(r.get("kind").and_then(Json::as_str), Some("Places"));
    for line in r.get("notes").and_then(Json::as_array).unwrap() {
        let parts: Vec<&str> = line.as_str().unwrap().splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3, "{line:?}");
        assert!(Path::new(parts[2]).exists(), "{line:?}");
    }
}

#[test]
fn a_java_mod_reports_through_the_same_call() {
    let dir = std::env::temp_dir().join(format!("kebitx-ffi-java-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/demo/blockstates")).unwrap();
    std::fs::write(
        dir.join("fabric.mod.json"),
        r#"{"schemaVersion":1,"id":"demo","version":"1"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("assets/demo/blockstates/a.json"),
        r#"{"variants":{"":{"model":"demo:block/missing"}}}"#,
    )
    .unwrap();
    let r = report(&dir.to_string_lossy()).expect("a Java mod");
    assert_eq!(
        r.get("kind").and_then(Json::as_str),
        Some("Fabric mod: demo")
    );
    let problems = r.get("problems").and_then(Json::as_array).unwrap();
    assert!(
        problems
            .iter()
            .any(|p| p.as_str().is_some_and(|p| p.contains("demo:block/missing"))),
        "{problems:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A copy of a fixture to repair, alone in its own temporary folder (repairs
/// leave their backups next to it).
fn copy_of(fixture_name: &str, tag: &str) -> std::path::PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(e.file_name());
            if e.path().is_dir() {
                copy(&e.path(), &target);
            } else {
                std::fs::copy(e.path(), &target).unwrap();
            }
        }
    }
    let base = std::env::temp_dir().join(format!("kebitx-ffi-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let pack = base.join("pack");
    copy(Path::new(&fixture(fixture_name)), &pack);
    pack
}

/// A function of the ABI like a C program, and free the result.
unsafe fn answer(out: *mut std::ffi::c_char) -> Json {
    assert!(!out.is_null());
    // SAFETY: not NULL, returned by the ABI, NUL-terminated.
    let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    // SAFETY: returned by the ABI and not freed yet.
    unsafe { kebitx_string_free(out) };
    json::parse(&text).expect("valid JSON")
}

fn plan(path: &str) -> Json {
    let c = CString::new(path).unwrap();
    // SAFETY: a valid string alive across the call, and what comes back is the ABI's.
    unsafe { answer(kebitx_ffi::kebitx_repair_plan(c.as_ptr())) }
}

fn apply(path: &str, fingerprint: &str) -> Json {
    let (p, f) = (
        CString::new(path).unwrap(),
        CString::new(fingerprint).unwrap(),
    );
    // SAFETY: valid strings alive across the call, and what comes back is the ABI's.
    unsafe { answer(kebitx_ffi::kebitx_repair_apply(p.as_ptr(), f.as_ptr())) }
}

fn undo(backup: &str) -> Json {
    let c = CString::new(backup).unwrap();
    // SAFETY: a valid string alive across the call, and what comes back is the ABI's.
    unsafe { answer(kebitx_ffi::kebitx_repair_undo(c.as_ptr())) }
}

fn string_of(j: &Json, key: &str) -> String {
    j.get(key)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A preview that writes nothing, a repair that only applies what it showed,
/// and an undo that gives back the very bytes.
#[test]
fn preview_apply_and_undo_a_bedrock_pack() {
    let pack = copy_of("atlas_pack", "bedrock");
    let path = pack.to_string_lossy().to_string();
    let file = pack.join("textures/terrain_texture.json");
    let original = std::fs::read(&file).unwrap();

    let preview = plan(&path);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        original,
        "a preview writes nothing"
    );
    let changes = preview.get("problems").and_then(Json::as_array).unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(changes[0].as_str().unwrap().contains("Edit"), "{changes:?}");
    let fingerprint = string_of(&preview, "fingerprint");
    assert!(!fingerprint.is_empty());

    // another preview's changes are not applied
    let refused = apply(&path, "00000000-1");
    assert_eq!(string_of(&refused, "kind"), "Nothing was changed");
    assert_eq!(std::fs::read(&file).unwrap(), original);

    let done = apply(&path, &fingerprint);
    assert_eq!(string_of(&done, "kind"), "Repair kept", "{done:?}");
    assert_eq!(done.get("kept"), Some(&Json::Bool(true)));
    assert_ne!(std::fs::read(&file).unwrap(), original);

    // applying it again: the folder is not what that preview showed
    let again = apply(&path, &fingerprint);
    assert_eq!(string_of(&again, "kind"), "Nothing was changed");

    let backup = string_of(&done, "backup");
    let undone = undo(&backup);
    assert_eq!(string_of(&undone, "kind"), "Repair undone", "{undone:?}");
    assert_eq!(std::fs::read(&file).unwrap(), original, "the very bytes");

    let failed = undo("no/such/backup");
    assert_eq!(string_of(&failed, "kind"), "Undo failed");
    std::fs::remove_dir_all(pack.parent().unwrap()).unwrap();
}

#[test]
fn a_mods_folder_loses_its_duplicate_and_gets_it_back() {
    let base = std::env::temp_dir().join(format!("kebitx-ffi-mods-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (mods, stage) = (base.join("mods"), base.join("stage"));
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::create_dir_all(&stage).unwrap();
    for version in ["1.0.0", "2.0.0"] {
        let meta = stage.join("fabric.mod.json");
        std::fs::write(
            &meta,
            format!(r#"{{"schemaVersion":1,"id":"demo","version":"{version}"}}"#),
        )
        .unwrap();
        kebitx_build::zip::write_zip(
            &mods.join(format!("demo-{version}.jar")),
            &[("fabric.mod.json".to_string(), meta)],
        )
        .unwrap();
    }
    let path = mods.to_string_lossy().to_string();
    let preview = plan(&path);
    let changes = preview.get("problems").and_then(Json::as_array).unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(changes[0].as_str().unwrap().contains("demo-1.0.0.jar"));

    let done = apply(&path, &string_of(&preview, "fingerprint"));
    assert_eq!(string_of(&done, "kind"), "Repair kept", "{done:?}");
    assert!(!mods.join("demo-1.0.0.jar").exists());
    assert!(mods.join("demo-2.0.0.jar").exists());

    undo(&string_of(&done, "backup"));
    assert!(mods.join("demo-1.0.0.jar").exists());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn what_cannot_be_repaired_says_so_and_nothing_applies() {
    let preview = plan("no/such/folder");
    assert_eq!(
        preview
            .get("problems")
            .and_then(Json::as_array)
            .unwrap()
            .len(),
        0
    );
    let refused = apply("no/such/folder", &string_of(&preview, "fingerprint"));
    assert_eq!(string_of(&refused, "kind"), "Nothing was changed");
}

/// Answers "go on" until it has been called `limit` times.
unsafe extern "C" fn stop_after(
    _stage: *const std::ffi::c_char,
    _done: std::ffi::c_uint,
    _total: std::ffi::c_uint,
    user: *mut std::ffi::c_void,
) -> std::ffi::c_int {
    // SAFETY: the tests below pass a pointer to an `(u32, u32)` alive across the call.
    let (calls, limit) = unsafe { &mut *(user as *mut (u32, u32)) };
    *calls += 1;
    (*calls > *limit) as std::ffi::c_int
}

#[test]
fn an_analysis_tells_its_steps_and_can_be_cancelled() {
    let c = CString::new(fixture("atlas_pack")).unwrap();
    let mut state = (0u32, u32::MAX);
    let run = |state: &mut (u32, u32)| {
        // SAFETY: valid string and callback; `state` outlives the call.
        unsafe {
            answer(kebitx_ffi::kebitx_report_with_progress(
                c.as_ptr(),
                Some(stop_after),
                state as *mut (u32, u32) as *mut std::ffi::c_void,
            ))
        }
    };
    let whole = run(&mut state);
    assert_eq!(state.0, 4, "four stages");
    assert_eq!(string_of(&whole, "kind"), "Bedrock pack");
    assert_eq!(whole.get("cancelled"), None);

    for limit in 0..4 {
        let mut state = (0, limit);
        let stopped = run(&mut state);
        assert_eq!(stopped.get("cancelled"), Some(&Json::Bool(true)));
        assert_eq!(state.0, limit + 1, "it stops at the first refusal");
    }

    // no callback at all
    // SAFETY: a valid string; NULL is allowed for the callback.
    let plain = unsafe {
        answer(kebitx_ffi::kebitx_report_with_progress(
            c.as_ptr(),
            None,
            std::ptr::null_mut(),
        ))
    };
    assert_eq!(string_of(&plain, "kind"), "Bedrock pack");
}

/// A world in the game's folders uses installed packs: it reports on them, and
/// never repairs them, since other worlds share them.
#[test]
fn a_world_reads_installed_packs_and_repairs_only_its_own() {
    let tmp = std::env::temp_dir().join(format!("kebitx-abi-listed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let shared = tmp.join("Users/Shared/games/com.mojang");
    let world = tmp.join("Users/123/games/com.mojang/minecraftWorlds/w");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::create_dir_all(shared.join("resource_packs")).unwrap();
    let installed = shared.join("resource_packs/atlas");
    let copy = copy_of("atlas_pack", "listed");
    for rel in ["manifest.json", "textures"] {
        std::fs::rename(copy.join(rel), {
            std::fs::create_dir_all(&installed).unwrap();
            installed.join(rel)
        })
        .unwrap();
    }
    let manifest = std::fs::read_to_string(installed.join("manifest.json")).unwrap();
    let id = json::parse(&manifest)
        .unwrap()
        .get("header")
        .and_then(|h| h.get("uuid"))
        .and_then(Json::as_str)
        .unwrap()
        .to_string();
    std::fs::write(world.join("level.dat"), "").unwrap();
    std::fs::write(
        world.join("world_resource_packs.json"),
        format!(r#"[{{"pack_id":"{id}","version":[1,0,0]}}]"#),
    )
    .unwrap();

    let r = report(world.to_str().unwrap()).expect("a report");
    assert_eq!(r.get("kind").and_then(Json::as_str), Some("Bedrock world"));
    // the same 2 problems the pack has on its own
    assert_eq!(r.get("broken"), Some(&Json::Num(2.0)), "{r:?}");

    let p = plan(world.to_str().unwrap());
    let changes = p.get("problems").and_then(Json::as_array).unwrap();
    assert!(
        changes.is_empty(),
        "a repair must not reach a shared pack: {p:?}"
    );
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// Twenty places naming one missing thing are one thing to fix.
#[test]
fn findings_come_grouped_by_what_to_fix() {
    let r = report(&fixture("behavior_broken")).expect("a report");
    let lines = |key: &str| -> Vec<String> {
        r.get(key)
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect()
    };
    let count_of = |line: &String| line.split('\t').next().unwrap().parse::<usize>().unwrap();
    for line in lines("problem_causes").iter().chain(&lines("dead_causes")) {
        assert_eq!(line.split('\t').count(), 4, "{line}");
        let places = line.split('\t').next_back().unwrap().lines().count();
        assert_eq!(places, count_of(line), "{line}");
    }
    let dead: usize = lines("dead_causes").iter().map(count_of).sum();
    assert_eq!(dead, lines("dead").len());
}

fn package(path: &Path, out: &Path) -> Json {
    let (p, o) = (
        CString::new(path.to_str().unwrap()).unwrap(),
        CString::new(out.to_str().unwrap()).unwrap(),
    );
    // SAFETY: both are valid NUL-terminated strings alive across the call.
    unsafe { answer(kebitx_ffi::kebitx_package(p.as_ptr(), o.as_ptr())) }
}

#[test]
fn a_pack_becomes_one_file_the_game_opens() {
    use kebitx_core::codec::zip::ZipArchive;
    let dir = std::env::temp_dir().join(format!("kebitx-abi-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pack = Path::new(&fixture("atlas_pack")).to_path_buf();
    assert_eq!(
        report(pack.to_str().unwrap())
            .unwrap()
            .get("package")
            .and_then(Json::as_str),
        Some("mcpack")
    );

    let file = dir.join("atlas.mcpack");
    let done = package(&pack, &file);
    assert_eq!(
        done.get("package_file").and_then(Json::as_str),
        file.to_str()
    );
    let zip = ZipArchive::open(&file).unwrap();
    // at its root, where the game looks, with the very bytes
    assert_eq!(
        zip.read("manifest.json", 1 << 20).unwrap(),
        std::fs::read(pack.join("manifest.json")).unwrap()
    );
    assert!(!dir.join("atlas.mcpack.partial").exists());

    // the same folder, the same bytes
    let again = dir.join("again.mcpack");
    package(&pack, &again);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        std::fs::read(&again).unwrap()
    );

    // a folder that is neither, and a file inside what it packs
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let no = package(&empty, &dir.join("no.mcpack"));
    assert!(no.get("package_file").is_none());
    assert!(!dir.join("no.mcpack").exists());
    let copy = copy_of("atlas_pack", "inside");
    let inner = copy.join("itself.mcpack");
    assert!(package(&copy, &inner).get("package_file").is_none());
    assert!(!inner.exists());
    let _ = std::fs::remove_dir_all(&copy);

    // a world
    let world = dir.join("world");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::write(world.join("level.dat"), "data").unwrap();
    assert_eq!(kebitx_ffi::package::extension_for(&world), Some("mcworld"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A folder with no saved data is said to be one, and never reads as an empty world.
#[test]
fn a_world_with_no_database_is_not_an_empty_world() {
    let dir = std::env::temp_dir().join(format!("kebitx-abi-nodb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("level.dat"), "").unwrap();
    let c = CString::new(dir.to_str().unwrap()).unwrap();
    // SAFETY: `c` is valid across the call, and no callback is given.
    let r = unsafe {
        answer(kebitx_ffi::kebitx_world(
            c.as_ptr(),
            None,
            std::ptr::null_mut(),
        ))
    };
    assert_eq!(r.get("nothing"), Some(&Json::Bool(true)), "{r:?}");
    assert!(r.get("world_view").is_none());
    // and analysing it does not offer what cannot be done
    let analysed = report(dir.to_str().unwrap()).unwrap();
    assert!(analysed.get("inspect").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}
