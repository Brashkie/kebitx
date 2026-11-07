//! Reports in another language. On its own: the language is the whole process's.

use kebitx_core::json::{self, Json};
use kebitx_ffi::{kebitx_report, kebitx_set_language, kebitx_string_free, kebitx_tr};
use std::ffi::{CStr, CString};
use std::path::Path;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn report(path: &str) -> Json {
    let c = CString::new(path).unwrap();
    // SAFETY: `c` is valid across the call; the answer is read, then freed once.
    unsafe {
        let out = kebitx_report(c.as_ptr());
        assert!(!out.is_null());
        let text = CStr::from_ptr(out).to_str().unwrap().to_string();
        kebitx_string_free(out);
        json::parse(&text).unwrap()
    }
}

fn set(code: &str) -> i32 {
    let c = CString::new(code).unwrap();
    // SAFETY: `c` is valid across the call.
    unsafe { kebitx_set_language(c.as_ptr()) }
}

fn say(text: &CStr) -> String {
    // SAFETY: `text` is valid, and lives through this read.
    unsafe { CStr::from_ptr(kebitx_tr(text.as_ptr())) }
        .to_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_report_speaks_the_language_chosen_and_its_verdict_does_not_change() {
    let path = fixture("atlas_pack");
    let english = report(&path);
    assert_eq!(
        english.get("kind").and_then(Json::as_str),
        Some("Bedrock pack")
    );

    assert_eq!(set("xx"), 0);
    assert_eq!(say(c"Home"), "Home");

    assert_eq!(set("es-PE"), 1);
    assert_eq!(say(c"Home"), "Inicio");
    assert_eq!(say(c"not in the list"), "not in the list");
    let spanish = report(&path);
    assert_eq!(
        spanish.get("kind").and_then(Json::as_str),
        Some("Pack de Bedrock")
    );
    // the same findings: only the words differ
    assert_eq!(spanish.get("broken"), english.get("broken"));
    let problems = spanish.get("problems").and_then(Json::as_array).unwrap();
    assert_eq!(problems.len(), 2);
    let all: String = problems.iter().filter_map(Json::as_str).collect();
    assert!(all.contains("siempre la textura faltante"), "{all}");

    for code in ["pt-BR", "fr", "de", "ru", "zh-CN"] {
        assert_eq!(set(code), 1, "{code}");
        let r = report(&path);
        assert_eq!(r.get("broken"), english.get("broken"));
        assert_ne!(r.get("kind"), english.get("kind"), "{code}");
    }

    set("en");
    assert_eq!(report(&path).get("kind"), english.get("kind"));
}
