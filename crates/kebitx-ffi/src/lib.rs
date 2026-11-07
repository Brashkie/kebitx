//! kebitx-ffi — the engine behind a C ABI, for C, Zig and any other language.
//!
//! The ONLY crate of the workspace where `unsafe` exists. It is kept to the
//! frontier itself: turning a C string into a Rust one, handing a C string
//! out, and taking it back to free it. Every `unsafe` block states the
//! condition that makes it sound. Everything else is the same safe engine.
//!
//! ```c
//! const char* kebitx_version(void);             // static: never free it
//! char*       kebitx_analyze(const char* path); // a Bedrock pack or world: JSON; NULL on bad input
//! char*       kebitx_report(const char* path);  // whatever it is, as people read it: JSON
//! char*       kebitx_report_with_progress(path, progress, user); // the same, and it can be cancelled
//! char*       kebitx_repair_plan(const char* path);                  // the preview: writes nothing
//! char*       kebitx_repair_apply(const char* path, const char* fp); // only what that preview showed
//! char*       kebitx_repair_undo(const char* backup);                // puts it back
//! char*       kebitx_places(void);              // the packs, worlds and mods on this computer
//! char*       kebitx_world(path, progress, user);   // what a world has saved; it can be cancelled
//! char*       kebitx_package(const char* path, const char* out); // one file for the game
//! int         kebitx_set_language(const char* code); // what reports say from now on
//! const char* kebitx_languages(void);           // static: every code and name
//! const char* kebitx_tr(const char* english);   // a text of the list, translated
//! void       kebitx_string_free(char* s);      // frees what any of them gave
//! ```
//!
//! A Rust panic must never cross into C (that is undefined behaviour): every
//! entry point catches it and answers `NULL` instead.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod package;
pub mod places;
pub mod repair;
pub mod world;

use kebitx_core::analyze::{
    analyze_with, analyze_world_all, holds_nothing, is_resource_pack, is_world, resource_packs_in,
};
use kebitx_core::compat::check_world;
use kebitx_core::explain;
use kebitx_core::i18n::{self, tf, tr};
use kebitx_core::json::{self, Json, JsonMap};
use kebitx_core::manifest::{pack_at, packs_in, parse_version, world_packs, PackKind};
use kebitx_core::passes::unused_asset;
use kebitx_core::report::Issue;
use kebitx_core::vanilla::VanillaCatalog;
use kebitx_java::modset::Progress;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
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
    // SAFETY: passed on with the caller's guarantee.
    unsafe { call(path, analyze_to_json) }
}

/// Analyse whatever `path` is, the way `kebitx analyze` decides: a Java mods
/// folder, a Java pack or mod (folder, `.jar` or `.zip`), or a Bedrock pack or
/// world. Returns a JSON report with the lines people read, or NULL as
/// `kebitx_analyze` does. The result must be released with
/// `kebitx_string_free`.
///
/// # Safety
/// `path` must be NULL or point to a NUL-terminated string that stays valid
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn kebitx_report(path: *const c_char) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    unsafe { call(path, report_to_json) }
}

/// Told what comes next, how many are done and of how many; a value other than
/// 0 cancels. Called on the thread that called `kebitx_report_with_progress`.
pub type ProgressFn =
    Option<unsafe extern "C" fn(*const c_char, c_uint, c_uint, *mut c_void) -> c_int>;

/// As `kebitx_report`, telling `progress` what it is doing (NULL: nothing is
/// told). A cancelled analysis returns a report with `"cancelled": true`. It
/// stops between steps: a mod in a mods folder, or a stage elsewhere.
///
/// # Safety
/// `path` as `kebitx_report`. `progress` must be NULL or callable from this
/// thread for the duration of the call, with `user` as its last argument.
#[no_mangle]
pub unsafe extern "C" fn kebitx_report_with_progress(
    path: *const c_char,
    progress: ProgressFn,
    user: *mut c_void,
) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    let Some(path) = (unsafe { text_of(path) }) else {
        return std::ptr::null_mut();
    };
    let mut go = |stage: &str, done: usize, total: usize| {
        let Some(f) = progress else { return true };
        let stage = CString::new(stage.replace('\0', "")).unwrap_or_default();
        // SAFETY: the caller guarantees `f` and `user`; `stage` outlives the call.
        unsafe { f(stage.as_ptr(), done as c_uint, total as c_uint, user) == 0 }
    };
    out(catch_unwind(AssertUnwindSafe(|| {
        report_with(Path::new(&path), &mut go)
    })))
}

/// The preview of a repair: a JSON report whose `problems` are the changes, and
/// with the `fingerprint` to apply them. Writes nothing. NULL as `kebitx_analyze`.
///
/// # Safety
/// As `kebitx_report`.
#[no_mangle]
pub unsafe extern "C" fn kebitx_repair_plan(path: *const c_char) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    unsafe { call(path, repair::plan_json) }
}

/// Applies a repair if it still is what the preview with `fingerprint` showed:
/// a JSON report with `kept` and, when kept, `backup`.
///
/// # Safety
/// `path` and `fingerprint` as the `path` of `kebitx_report`.
#[no_mangle]
pub unsafe extern "C" fn kebitx_repair_apply(
    path: *const c_char,
    fingerprint: *const c_char,
) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    let (path, fingerprint) = unsafe { (text_of(path), text_of(fingerprint)) };
    let (Some(path), Some(fingerprint)) = (path, fingerprint) else {
        return std::ptr::null_mut();
    };
    out(catch_unwind(|| {
        repair::apply_json(Path::new(&path), &fingerprint)
    }))
}

/// Puts back what a kept repair changed, from the `backup` it reported.
///
/// # Safety
/// As `kebitx_report`.
#[no_mangle]
pub unsafe extern "C" fn kebitx_repair_undo(backup: *const c_char) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    unsafe { call(backup, repair::undo_json) }
}

/// The packs, worlds and mods folders on this computer: a JSON report with one
/// note per place, as `what\tname\tpath`. A `what` of `Shortcut` is a game folder
/// to browse from, not to analyse. NULL if the engine panicked.
#[no_mangle]
pub extern "C" fn kebitx_places() -> *mut c_char {
    out(catch_unwind(|| Some(places::places_json())))
}

/// What a Bedrock world has saved: a JSON report whose notes are `what\tname\thow
/// many`, with `world_view`. Reads, and never writes. `progress` as in
/// `kebitx_report_with_progress`. A report's `inspect` says a world can be read.
///
/// # Safety
/// As `kebitx_report_with_progress`.
#[no_mangle]
pub unsafe extern "C" fn kebitx_world(
    path: *const c_char,
    progress: ProgressFn,
    user: *mut c_void,
) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    let Some(path) = (unsafe { text_of(path) }) else {
        return std::ptr::null_mut();
    };
    let mut go = |stage: &str, done: usize, total: usize| {
        let Some(f) = progress else { return true };
        let stage = CString::new(stage.replace('\0', "")).unwrap_or_default();
        // SAFETY: the caller guarantees `f` and `user`; `stage` outlives the call.
        unsafe { f(stage.as_ptr(), done as c_uint, total as c_uint, user) == 0 }
    };
    out(catch_unwind(AssertUnwindSafe(|| {
        world::world_report(Path::new(&path), &mut go)
    })))
}

/// Puts a pack or a world in one file for the game, `out`: a JSON report, with
/// `package_file` when it worked. A report's `package` says the extension to use.
///
/// # Safety
/// `path` and `out` as the `path` of `kebitx_report`.
#[no_mangle]
pub unsafe extern "C" fn kebitx_package(
    path: *const c_char,
    out_file: *const c_char,
) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    let (path, out_file) = unsafe { (text_of(path), text_of(out_file)) };
    let (Some(path), Some(out_file)) = (path, out_file) else {
        return std::ptr::null_mut();
    };
    out(catch_unwind(|| {
        package::package_json(Path::new(&path), Path::new(&out_file))
    }))
}

/// Chooses the language of what is said from now on, for the whole process, by
/// its code: `es`, `pt-BR`, ... Returns 1, or 0 with nothing changed if there is no
/// such language.
///
/// # Safety
/// As `kebitx_report`.
#[no_mangle]
pub unsafe extern "C" fn kebitx_set_language(code: *const c_char) -> c_int {
    // SAFETY: passed on with the caller's guarantee.
    let Some(code) = (unsafe { text_of(code) }) else {
        return 0;
    };
    c_int::from(i18n::set_language(&code))
}

/// Every language, one per line: its code, a tab and its name for itself. Static:
/// never free it.
#[no_mangle]
pub extern "C" fn kebitx_languages() -> *const c_char {
    static LIST: OnceLock<CString> = OnceLock::new();
    LIST.get_or_init(|| {
        let lines: Vec<String> = i18n::LANGUAGES
            .iter()
            .map(|(code, name, _)| format!("{code}\t{name}"))
            .collect();
        CString::new(lines.join("\n")).unwrap_or_default()
    })
    .as_ptr()
}

/// A text of the list, in the language in use; `text` itself when it has none. What
/// it returns lives as long as the program: never free it.
///
/// # Safety
/// As `kebitx_report`. `text` must stay valid for as long as the answer is used.
#[no_mangle]
pub unsafe extern "C" fn kebitx_tr(text: *const c_char) -> *const c_char {
    // SAFETY: passed on with the caller's guarantee.
    let Some(english) = (unsafe { text_of(text) }) else {
        return text;
    };
    if tr(&english) == english {
        return text;
    }
    // each answer is kept: the same text asks once per language
    static KEPT: OnceLock<
        std::sync::Mutex<std::collections::HashMap<(&'static str, String), CString>>,
    > = OnceLock::new();
    let Ok(mut kept) = KEPT.get_or_init(Default::default).lock() else {
        return text;
    };
    let translated = tr(&english).to_string();
    kept.entry((i18n::language(), english))
        .or_insert_with(|| CString::new(translated).unwrap_or_default())
        .as_ptr()
}

/// A C string as text; `None` when it is NULL or not UTF-8.
///
/// # Safety
/// `s` must be NULL or point to a NUL-terminated string that stays valid for
/// the duration of the call.
unsafe fn text_of(s: *const c_char) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: not NULL (checked above), and the caller guarantees the rest.
    unsafe { CStr::from_ptr(s) }
        .to_str()
        .ok()
        .map(str::to_string)
}

/// The answer as a C string; NULL when there is none, or the engine panicked.
fn out(answer: std::thread::Result<Option<String>>) -> *mut c_char {
    match answer {
        Ok(Some(report)) => CString::new(report)
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
        _ => std::ptr::null_mut(),
    }
}

/// A path in, a string out, and no panic across the frontier.
///
/// # Safety
/// As `text_of`.
unsafe fn call(path: *const c_char, f: fn(&Path) -> Option<String>) -> *mut c_char {
    // SAFETY: passed on with the caller's guarantee.
    let Some(path) = (unsafe { text_of(path) }) else {
        return std::ptr::null_mut();
    };
    out(catch_unwind(|| f(Path::new(&path))))
}

/// Free a string returned by any function here. NULL is accepted and ignored.
///
/// # Safety
/// `s` must be NULL or a pointer one of them returned, not freed yet.
#[no_mangle]
pub unsafe extern "C" fn kebitx_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: the caller guarantees `s` came from `CString::into_raw` in `out`
    // and was not freed before: taking it back is sound.
    drop(unsafe { CString::from_raw(s) });
}

/// The report, built with the safe engine. `None` if `root` is not a folder.
pub fn analyze_to_json(root: &Path) -> Option<String> {
    bedrock_json(root, &mut |_, _, _| true).ok()
}

/// Why there is no report.
enum Stopped {
    NotAFolder,
    Cancelled,
}

/// What to say about a folder with nothing to analyse.
fn nothing_report(root: &Path) -> String {
    let help = match packs_in(root).len() {
        0 => tr(
            "A pack has manifest.json, a world has level.dat, and a mods folder has .jar files.",
        )
        .to_string(),
        1 => tr("This folder holds 1 pack: open that one.").to_string(),
        n => tf("This folder holds {0} packs: open one of them.", &[&n]),
    };
    Lines {
        kind: tr("No pack, world or mods here").to_string(),
        broken: 0,
        summary: vec![help],
        problems: Vec::new(),
        notes: Vec::new(),
        extra: vec![("nothing".to_string(), Json::Bool(true))],
    }
    .json(root)
}

fn bedrock_json(root: &Path, go: Progress) -> Result<String, Stopped> {
    if !root.is_dir() {
        return Err(Stopped::NotAFolder);
    }
    if !go(tr("Reading the game"), 0, 4) {
        return Err(Stopped::Cancelled);
    }
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    let is_world = is_world(root);
    let single = !is_world && (is_resource_pack(root) || resource_packs_in(root).is_empty());
    if !go(tr("Checking references"), 1, 4) {
        return Err(Stopped::Cancelled);
    }
    let reports = if single {
        vec![(String::new(), analyze_with(root, &catalog))]
    } else {
        analyze_world_all(root, &catalog)
    };
    let packs = if single {
        vec![pack_at(root)]
    } else {
        world_packs(root)
    };
    let game = if catalog.is_empty() {
        None
    } else {
        parse_version(&catalog.version)
    };
    if !go(tr("Checking compatibility"), 2, 4) {
        return Err(Stopped::Cancelled);
    }
    let compat_errors = if single {
        0
    } else {
        check_world(root, game).errors()
    };

    if !go(tr("Looking for unused files"), 3, 4) {
        return Err(Stopped::Cancelled);
    }
    let num = |n: usize| Json::Num(n as f64);
    let s = |v: &str| Json::Str(v.to_string());
    let obj = |pairs: Vec<(&str, Json)>| {
        Json::Obj(JsonMap::from_entries(
            pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        ))
    };
    let mut issues = Vec::new();
    let (mut entities, mut broken, mut case) = (0, 0, 0);
    let (mut behavior_files, mut behavior_refs, mut behavior_unverified) = (0, 0, 0);
    for (pack, r) in &reports {
        entities += r.entities;
        broken += r.broken();
        case += r.case_mismatches;
        behavior_files += r.behavior_files;
        behavior_refs += r.behavior_refs;
        behavior_unverified += r.behavior_unverified;
        for i in &r.issues {
            let (kind, owner, reference) = match i {
                Issue::Missing {
                    kind,
                    owner,
                    reference,
                } => (kind.to_string(), owner.clone(), reference.clone()),
                Issue::Dead {
                    kind,
                    owner,
                    reference,
                } => (format!("dead {kind}"), owner.clone(), reference.clone()),
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
        ("behavior_files", num(behavior_files)),
        ("behavior_refs", num(behavior_refs)),
        ("behavior_unverified", num(behavior_unverified)),
        ("compat_errors", num(compat_errors)),
        (
            "unused_assets",
            num(unused_asset::run(&packs, &catalog).unused.len()),
        ),
        ("issues", Json::Arr(issues)),
    ]);
    Ok(json::to_string(&report))
}

/// The report of `kebitx_report`, built with the safe engine.
pub fn report_to_json(root: &Path) -> Option<String> {
    report_with(root, &mut |_, _, _| true)
}

fn report_with(root: &Path, go: Progress) -> Option<String> {
    if kebitx_java::is_mod_folder(root) {
        return java_folder_report(root, go);
    }
    if kebitx_java::is_java(root) {
        return java_report(root, go);
    }
    bedrock_report(root, go)
}

/// What a cancelled analysis answers.
pub(crate) fn cancelled(root: &Path) -> String {
    Lines {
        kind: tr("Cancelled").to_string(),
        broken: 0,
        summary: Vec::new(),
        problems: Vec::new(),
        notes: Vec::new(),
        extra: vec![("cancelled".to_string(), Json::Bool(true))],
    }
    .json(root)
}

/// What every report has, whatever was analysed.
pub(crate) struct Lines {
    pub(crate) kind: String,
    pub(crate) broken: usize,
    pub(crate) summary: Vec<String>,
    pub(crate) problems: Vec<String>,
    pub(crate) notes: Vec<String>,
    /// What only some reports have.
    pub(crate) extra: Vec<(String, Json)>,
}

impl Lines {
    pub(crate) fn json(self, root: &Path) -> String {
        let list = |v: Vec<String>| Json::Arr(v.into_iter().map(Json::Str).collect());
        let mut entries = vec![
            (
                "path".to_string(),
                Json::Str(root.to_string_lossy().to_string()),
            ),
            (
                "engine".to_string(),
                Json::Str(env!("CARGO_PKG_VERSION").to_string()),
            ),
            ("kind".to_string(), Json::Str(self.kind)),
            ("broken".to_string(), Json::Num(self.broken as f64)),
            ("summary".to_string(), list(self.summary)),
            ("problems".to_string(), list(self.problems)),
            ("notes".to_string(), list(self.notes)),
        ];
        entries.extend(self.extra);
        json::to_string(&Json::Obj(JsonMap::from_entries(entries)))
    }
}

fn game_line(game: Option<&(String, bool)>) -> String {
    match game {
        Some((id, true)) => tf("Game: Java {0}", &[id]),
        Some((id, false)) => tf(
            "Game: Java {0} (newest installed: not the version it asks for)",
            &[id],
        ),
        None => tr("Game: not installed, so nothing of the game was checked").to_string(),
    }
}

fn java_folder_report(root: &Path, go: Progress) -> Option<String> {
    use kebitx_java::describe::{code_line, finding_line, set_line};
    let folder = kebitx_java::modset::ModFolder::open(root);
    let jar = kebitx_java::JavaAssets::discover().and_then(|j| folder.game_for(&j));
    let Some(r) = folder.analyze_with(jar.as_ref(), go) else {
        return Some(cancelled(root));
    };
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    for f in &r.findings {
        let to = if f.is_info() {
            &mut notes
        } else {
            &mut problems
        };
        to.push(set_line(f));
    }
    for m in &r.mods {
        for f in &m.report.findings {
            let to = if f.is_info() {
                &mut notes
            } else {
                &mut problems
            };
            to.push(format!("{}: {}", m.jar, finding_line(f)));
        }
        for f in m.report.code.iter().flat_map(|c| &c.findings) {
            let to = if f.is_info() {
                &mut notes
            } else {
                &mut problems
            };
            to.push(format!("{}: {}", m.jar, code_line(f)));
        }
    }
    let lines = Lines {
        kind: tr("Java mods folder").to_string(),
        broken: r.broken(),
        summary: vec![
            tf("Mods: {0}", &[&r.mods.len()]),
            game_line(r.game.as_ref()),
        ],
        problems,
        notes,
        extra: Vec::new(),
    };
    Some(lines.json(root))
}

fn java_report(root: &Path, go: Progress) -> Option<String> {
    use kebitx_java::describe::{code_line, finding_line};
    use kebitx_java::Kind;
    let src = kebitx_java::Source::open(root).ok()?;
    let kind = kebitx_java::analyze::kind_of(&src, &src.files());
    let jar =
        kebitx_java::JavaAssets::discover().and_then(|j| kebitx_java::analyze::game_for(&j, &kind));
    if !go(tr("Analysing"), 0, 1) {
        return Some(cancelled(root));
    }
    let r = kebitx_java::analyze(&src, jar.as_ref());
    let name = match &r.kind {
        Kind::ResourcePack { .. } => tr("Java resource pack").to_string(),
        Kind::Mod { loader, ids, .. } => tf("{0} mod: {1}", &[loader, &ids.join(", ")]),
        Kind::Assets => tr("Java assets").to_string(),
    };
    let mut summary = vec![
        game_line(r.game.as_ref()),
        tf("References checked: {0}", &[&r.checked]),
    ];
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    for f in &r.findings {
        let to = if f.is_info() {
            &mut notes
        } else {
            &mut problems
        };
        to.push(finding_line(f));
    }
    if let Some(c) = &r.code {
        summary.push(tf("Classes: {0}", &[&c.classes]));
        for f in &c.findings {
            let to = if f.is_info() {
                &mut notes
            } else {
                &mut problems
            };
            to.push(code_line(f));
        }
    }
    Some(
        Lines {
            kind: name,
            broken: r.broken(),
            summary,
            problems,
            notes,
            extra: Vec::new(),
        }
        .json(root),
    )
}

/// What a finding is about, as people read it.
fn kind_name(kind: &str) -> &str {
    let kind = kind.strip_prefix("dead ").unwrap_or(kind);
    tr(match kind {
        "animation_controller" => "animation controller",
        "render" => "render controller",
        "case" => "wrong case",
        other => other,
    })
}

fn bedrock_report(root: &Path, go: Progress) -> Option<String> {
    if holds_nothing(root) {
        return Some(nothing_report(root));
    }
    let report = match bedrock_json(root, go) {
        Ok(report) => json::parse(&report).ok()?,
        Err(Stopped::Cancelled) => return Some(cancelled(root)),
        Err(Stopped::NotAFolder) => return None,
    };
    let text = |j: &Json, key: &str| {
        j.get(key)
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let count = |key: &str| match report.get(key) {
        Some(Json::Num(n)) => *n as usize,
        _ => 0,
    };
    let line_of = |i: &Json| {
        let pack = text(i, "pack");
        let line = format!(
            "{}: {} {} ({})",
            text(i, "owner"),
            kind_name(&text(i, "kind")),
            text(i, "reference"),
            text(i, "fix")
        );
        if pack.is_empty() {
            line
        } else {
            format!("{pack}: {line}")
        }
    };
    let issues = report.get("issues").and_then(Json::as_array);
    let is_dead = |i: &&Json| text(i, "kind").starts_with("dead ");
    let mut problems: Vec<String> = issues
        .into_iter()
        .flatten()
        .filter(|i| !is_dead(i))
        .map(line_of)
        .collect();
    let dead: Vec<String> = issues
        .into_iter()
        .flatten()
        .filter(is_dead)
        .map(line_of)
        .collect();
    // the same missing thing named from many places is one thing to fix
    let cause_lines = |dead_ones: bool| -> Vec<Json> {
        let mut order: Vec<(String, String)> = Vec::new();
        let mut owners: std::collections::HashMap<(String, String), Vec<String>> =
            Default::default();
        for i in issues.into_iter().flatten() {
            if is_dead(&i) != dead_ones {
                continue;
            }
            let what = format!("{} {}", kind_name(&text(i, "kind")), text(i, "reference"));
            let key = (what, text(i, "fix"));
            let pack = text(i, "pack");
            let owner = text(i, "owner");
            let owner = if pack.is_empty() {
                owner
            } else {
                format!("{pack}: {owner}")
            };
            let list = owners.entry(key.clone()).or_default();
            if list.is_empty() {
                order.push(key);
            }
            list.push(owner);
        }
        // what touches most comes first
        order.sort_by_key(|k| std::cmp::Reverse(owners[k].len()));
        order
            .into_iter()
            .map(|k| {
                let list = &owners[&k];
                Json::Str(format!(
                    "{}\t{}\t{}\t{}",
                    list.len(),
                    k.0,
                    k.1,
                    list.join("\n")
                ))
            })
            .collect()
    };
    let mut problem_causes = cause_lines(false);
    let dead_causes = cause_lines(true);
    // a verdict of OK must never hide them: the details are in the CLI's report
    if count("compat_errors") > 0 {
        let line = tf(
            "{0} compatibility error(s) between the packs: run `kebitx analyze` for each one",
            &[&count("compat_errors")],
        );
        problem_causes.push(Json::Str(format!("1\t{line}\t\t")));
        problems.push(line);
    }
    let game = text(&report, "game_version");
    let mode = text(&report, "mode");
    // nothing was checked, and that is no verdict either
    let unchecked = if !problems.is_empty() || !dead.is_empty() {
        None
    } else if mode == "world" && count("packs") == 0 {
        Some((
            tr("World with no packs"),
            tr("It carries none, and its lists activate none that could be found, so there is nothing to check."),
        ))
    } else if mode == "pack"
        && count("behavior_refs") == 0
        && count("entities") == 0
        && pack_at(root).manifest.kind == PackKind::Data
    {
        Some((
            tr("Behavior pack with nothing to check"),
            tr("It names no file, item or event that could be followed."),
        ))
    } else {
        None
    };
    if let Some((kind, why)) = unchecked {
        let mut extra = vec![("nothing".to_string(), Json::Bool(true))];
        if world::has_database(root) {
            extra.push(("inspect".to_string(), Json::Bool(true)));
        }
        if let Some(extension) = package::extension_for(root) {
            extra.push(("package".to_string(), Json::Str(extension.to_string())));
        }
        let lines = Lines {
            kind: kind.to_string(),
            broken: 0,
            summary: vec![why.to_string()],
            problems: Vec::new(),
            notes: Vec::new(),
            extra,
        };
        return Some(lines.json(root));
    }
    let lines = Lines {
        kind: tr(if mode == "world" {
            "Bedrock world"
        } else {
            "Bedrock pack"
        })
        .to_string(),
        broken: problems.len(),
        summary: vec![
            if game.is_empty() {
                tr("Game: not installed, so references to it were not checked").to_string()
            } else {
                tf("Game: Bedrock {0}", &[&game])
            },
            tf("Packs: {0}", &[&count("packs")]),
            tf("Entities: {0}", &[&count("entities")]),
            tf("Behavior references: {0}", &[&count("behavior_refs")]),
            tf("Not verifiable here: {0}", &[&count("behavior_unverified")]),
            tf("Case mismatches: {0}", &[&count("case_mismatches")]),
            tf("Compatibility errors: {0}", &[&count("compat_errors")]),
            tf("Unused assets: {0}", &[&count("unused_assets")]),
        ],
        problems,
        notes: Vec::new(),
        extra: {
            let mut extra = vec![
                (
                    "dead".to_string(),
                    Json::Arr(dead.into_iter().map(Json::Str).collect()),
                ),
                ("problem_causes".to_string(), Json::Arr(problem_causes)),
                ("dead_causes".to_string(), Json::Arr(dead_causes)),
            ];
            if world::has_database(root) {
                extra.push(("inspect".to_string(), Json::Bool(true)));
            }
            if let Some(extension) = package::extension_for(root) {
                extra.push(("package".to_string(), Json::Str(extension.to_string())));
            }
            extra
        },
    };
    Some(lines.json(root))
}
