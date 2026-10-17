//! kebitx-ffi — the engine behind a C ABI, for C, Zig and any other language.
//!
//! The ONLY crate of the workspace where `unsafe` exists. It is kept to the
//! frontier itself: turning a C string into a Rust one, handing a C string
//! out, and taking it back to free it. Every `unsafe` block states the
//! condition that makes it sound. Everything else is the same safe engine.
//!
//! ```c
//! const char* kebitx_version(void);             // static: never free it
//! char*       kebitx_analyze(const char* path); // JSON; NULL on bad input
//! void        kebitx_string_free(char* s);      // frees what kebitx_analyze gave
//! ```
//!
//! A Rust panic must never cross into C (that is undefined behaviour): every
//! entry point catches it and answers `NULL` instead.
#![deny(unsafe_op_in_unsafe_fn)]

use kebitx_core::analyze::{analyze_with, analyze_world_with, is_resource_pack, resource_packs_in};
use kebitx_core::compat::check_world;
use kebitx_core::explain;
use kebitx_core::json::{self, Json, JsonMap};
use kebitx_core::manifest::{pack_at, packs_in, parse_version};
use kebitx_core::passes::unused_asset;
use kebitx_core::report::Issue;
use kebitx_core::vanilla::VanillaCatalog;
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::sync::OnceLock;

/// The engine version, as a NUL-terminated string that lives as long as the
/// program. Never free it.
#[no_mangle]
pub extern "C" fn kebitx_version() -> *const c_char {
    static V: OnceLock<CString> = OnceLock::new();
    V.get_or_init(|| CString::new(env!("CARGO_PKG_VERSION")).unwrap_or_default())
        .as_ptr()
}

/// Analyse a pack or a world folder and return a JSON report, or NULL when
/// `path` is NULL, not UTF-8, not a folder, or the analysis panicked. The
/// result must be released with `kebitx_string_free`.
///
/// # Safety
/// `path` must be NULL or point to a NUL-terminated string that stays valid
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn kebitx_analyze(path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: not NULL (checked above), and the caller guarantees a valid
    // NUL-terminated string for the duration of this call.
    let path = unsafe { CStr::from_ptr(path) };
    let Ok(path) = path.to_str() else {
        return std::ptr::null_mut();
    };
    let path = path.to_string();
    match std::panic::catch_unwind(|| analyze_to_json(Path::new(&path))) {
        Ok(Some(report)) => CString::new(report)
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
        _ => std::ptr::null_mut(),
    }
}

/// Free a string returned by `kebitx_analyze`. NULL is accepted and ignored.
///
/// # Safety
/// `s` must be NULL or a pointer returned by `kebitx_analyze` that has not
/// been freed yet.
#[no_mangle]
pub unsafe extern "C" fn kebitx_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: the caller guarantees `s` came from `CString::into_raw` in
    // `kebitx_analyze` and was not freed before: taking it back is sound.
    drop(unsafe { CString::from_raw(s) });
}

/// The report, built with the safe engine. `None` if `root` is not a folder.
pub fn analyze_to_json(root: &Path) -> Option<String> {
    if !root.is_dir() {
        return None;
    }
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    let is_world = [
        "level.dat",
        "world_behavior_packs.json",
        "world_resource_packs.json",
    ]
    .iter()
    .any(|f| root.join(f).is_file());
    let single = !is_world && (is_resource_pack(root) || resource_packs_in(root).is_empty());
    let reports = if single {
        vec![(String::new(), analyze_with(root, &catalog))]
    } else {
        analyze_world_with(root, &catalog)
    };
    let packs = if single {
        vec![pack_at(root)]
    } else {
        packs_in(root)
    };
    let game = if catalog.is_empty() {
        None
    } else {
        parse_version(&catalog.version)
    };
    let compat_errors = if single {
        0
    } else {
        check_world(root, game).errors()
    };

    let num = |n: usize| Json::Num(n as f64);
    let s = |v: &str| Json::Str(v.to_string());
    let obj = |pairs: Vec<(&str, Json)>| {
        Json::Obj(JsonMap::from_entries(
            pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        ))
    };
    let mut issues = Vec::new();
    let (mut entities, mut broken, mut case) = (0, 0, 0);
    for (pack, r) in &reports {
        entities += r.entities;
        broken += r.broken();
        case += r.case_mismatches;
        for i in &r.issues {
            let (kind, owner, reference) = match i {
                Issue::Missing {
                    kind,
                    owner,
                    reference,
                } => (kind.to_string(), owner.clone(), reference.clone()),
                Issue::CaseMismatch {
                    owner, reference, ..
                } => ("case".to_string(), owner.clone(), reference.clone()),
                Issue::EmptyTextureSet { file, shortname } => (
                    "empty texture set".to_string(),
                    file.clone(),
                    shortname.clone(),
                ),
                Issue::Unreadable { file, why } => {
                    ("unreadable".to_string(), file.clone(), why.clone())
                }
            };
            let e = explain::issue(i);
            issues.push(obj(vec![
                ("pack", s(pack)),
                ("kind", Json::Str(kind)),
                ("owner", Json::Str(owner)),
                ("reference", Json::Str(reference)),
                ("confidence", s(e.confidence.name())),
                ("fix", Json::Str(e.fix)),
            ]));
        }
    }
    let report = obj(vec![
        ("path", Json::Str(root.to_string_lossy().to_string())),
        ("engine", s(env!("CARGO_PKG_VERSION"))),
        (
            "game_version",
            if catalog.is_empty() {
                Json::Null
            } else {
                s(&catalog.version)
            },
        ),
        ("mode", s(if single { "pack" } else { "world" })),
        ("packs", num(packs.len())),
        ("entities", num(entities)),
        ("broken", num(broken)),
        ("case_mismatches", num(case)),
        ("compat_errors", num(compat_errors)),
        (
            "unused_assets",
            num(unused_asset::run(&packs, &catalog).unused.len()),
        ),
        ("issues", Json::Arr(issues)),
    ]);
    Some(json::to_string(&report))
}
