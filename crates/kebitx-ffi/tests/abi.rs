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
    assert_eq!(w.get("compat_errors"), Some(&Json::Num(4.0)));
}
