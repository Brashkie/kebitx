//! `kebitx build --watch`: rebuild when the sources change.
//!
//! Std only, so it polls: a fingerprint (path, size, modification time) of
//! everything a build reads — the project file, the source packs, the world
//! template, filter programs inside the project — is taken every interval.
//! A rebuild starts once the fingerprint has CHANGED and then STAYED the same
//! for one more interval: an editor saving several files, or saving in
//! bursts, triggers one build, not five.
//!
//! What a build writes (`build/`, `.kebitx/`) is never fingerprinted, so a
//! build cannot trigger itself.

use crate::config::{Project, PROJECT_FILE};
use crate::pipeline::{build, BuildError, BuildOptions, BuildReport};
use kebitx_core::filesystem::all_files;
use kebitx_core::vanilla::VanillaCatalog;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

/// Everything a build of `root` reads, hashed: the project file and, if it
/// parses, the sources it names.
pub fn fingerprint(root: &Path) -> u64 {
    let mut files: Vec<PathBuf> = vec![root.join(PROJECT_FILE)];
    if let Ok(p) = Project::load(root) {
        for src in [&p.behavior, &p.resource, &p.world].into_iter().flatten() {
            files.extend(all_files(src));
        }
        for f in &p.filters {
            let prog = root.join(&f.run[0]);
            if prog.is_file() {
                files.push(prog);
            }
        }
    }
    let outputs = [root.join("build"), root.join(".kebitx")];
    files.retain(|f| !outputs.iter().any(|o| f.starts_with(o)));
    files.sort();
    files.dedup();

    let mut h = DefaultHasher::new();
    for f in &files {
        f.hash(&mut h);
        match fs::metadata(f) {
            Ok(m) => {
                m.len().hash(&mut h);
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .hash(&mut h);
            }
            Err(_) => 0u8.hash(&mut h), // gone: also a change
        }
    }
    h.finish()
}

/// What a build attempt produced: a project file that does not load is
/// reported too, and watching goes on.
pub enum Outcome {
    Built(BuildReport),
    Failed(BuildError),
    BadProject(String),
}

/// Build now, then again on every settled change. `on_build` gets each
/// outcome; watching stops after `max_builds` builds (`None`: never).
pub fn watch(
    root: &Path,
    opts: &BuildOptions,
    catalog: &VanillaCatalog,
    interval: Duration,
    max_builds: Option<usize>,
    mut on_build: impl FnMut(Outcome),
) {
    let mut built = 0usize;
    let mut last_built = fingerprint(root);
    let run = |on_build: &mut dyn FnMut(Outcome)| {
        on_build(match Project::load(root) {
            Ok(p) => match build(&p, opts, catalog) {
                Ok(r) => Outcome::Built(r),
                Err(e) => Outcome::Failed(e),
            },
            Err(e) => Outcome::BadProject(e),
        });
    };
    run(&mut on_build);
    built += 1;

    let mut pending: Option<u64> = None;
    while max_builds.is_none_or(|m| built < m) {
        thread::sleep(interval);
        let now = fingerprint(root);
        match pending {
            // changed since the last build, and unchanged since the last look
            Some(seen) if seen == now && now != last_built => {
                pending = None;
                last_built = now;
                run(&mut on_build);
                built += 1;
            }
            _ if now != last_built => pending = Some(now),
            _ => pending = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kebitx-watch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("packs/BP")).unwrap();
        fs::write(root.join("packs/BP/manifest.json"), r#"{"format_version":2,"header":{"uuid":"a","version":[1,0,0]},"modules":[{"type":"data","uuid":"b","version":[1,0,0]}]}"#).unwrap();
        fs::write(
            root.join(PROJECT_FILE),
            r#"{"name":"w","packs":{"behavior":"packs/BP"},"check":false}"#,
        )
        .unwrap();
        root
    }

    #[test]
    fn sources_change_the_fingerprint_outputs_do_not() {
        let root = project("fp");
        let a = fingerprint(&root);
        assert_eq!(a, fingerprint(&root), "stable when nothing changes");

        fs::create_dir_all(root.join("build/w_BP")).unwrap();
        fs::write(root.join("build/w_BP/x.json"), "{}").unwrap();
        fs::create_dir_all(root.join(".kebitx/tmp")).unwrap();
        fs::write(root.join(".kebitx/tmp/y.json"), "{}").unwrap();
        assert_eq!(a, fingerprint(&root), "outputs never trigger a rebuild");

        fs::write(root.join("packs/BP/new.json"), "{}").unwrap();
        let b = fingerprint(&root);
        assert_ne!(a, b, "a new source file");
        fs::write(root.join("packs/BP/new.json"), "{ }").unwrap();
        assert_ne!(b, fingerprint(&root), "an edited source file");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_change_triggers_exactly_one_rebuild() {
        let root = project("loop");
        let mut outcomes = 0;
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let editor = {
            let root = root.clone();
            thread::spawn(move || {
                rx.recv().unwrap(); // after the first build
                fs::write(root.join("packs/BP/a.json"), "{}").unwrap();
                fs::write(root.join("packs/BP/b.json"), "{}").unwrap();
            })
        };
        watch(
            &root,
            &BuildOptions::default(),
            &VanillaCatalog::empty(),
            Duration::from_millis(60),
            Some(2),
            |o| {
                outcomes += 1;
                assert!(matches!(o, Outcome::Built(_)));
                if outcomes == 1 {
                    tx.send(()).unwrap();
                }
            },
        );
        editor.join().unwrap();
        assert_eq!(
            outcomes, 2,
            "the initial build and one rebuild for both edits"
        );
        assert!(
            root.join("build/w_BP/b.json").is_file(),
            "the rebuild saw the edits"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
