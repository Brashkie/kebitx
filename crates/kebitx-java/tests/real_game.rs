//! The analysis against the installed Java game. Skips itself where there is
//! no Java Edition (CI machines): the fixture tests cover the logic there.

use kebitx_core::codec::zip::ZipArchive;
use kebitx_java::{analyze, JavaAssets, JavaJar, Kind, Source};

/// Every installed version's own assets, analysed as a pack against that
/// version, give no finding: whatever the analysis would report on them is a
/// rule of its own that is wrong. Snapshots included: they are where the
/// formats change first.
#[test]
fn every_installed_versions_own_assets_are_clean() {
    let Some(java) = JavaAssets::discover() else {
        eprintln!("no Java Edition installed: skipped");
        return;
    };
    let Ok(entries) = std::fs::read_dir(java.dir.join("versions")) else {
        eprintln!("no versions folder: skipped");
        return;
    };
    let mut checked = 0;
    for e in entries.flatten() {
        let id = e.file_name().to_string_lossy().to_string();
        let path = e.path().join(format!("{id}.jar"));
        let Ok(zip) = ZipArchive::open(&path) else {
            continue;
        };
        let jar = JavaJar {
            id: id.clone(),
            format: 0,
            exact: true,
            zip,
        };
        let r = analyze(&Source::open(&path).unwrap(), Some(&jar));
        eprintln!(
            "{id}: {} blockstates, {} item definitions, {} models, {} animations; {} references checked",
            r.blockstates, r.items, r.models, r.animations, r.checked
        );
        assert_eq!(r.kind, Kind::Assets);
        assert!(
            r.blockstates > 1000 && r.items > 1000 && r.models > 3000,
            "{id}: {r:?}"
        );
        assert!(r.animations > 50, "{id}: {}", r.animations);
        assert_eq!(
            r.unchecked, 0,
            "{id}: every reference of the game is the game's"
        );
        assert!(r.findings.is_empty(), "{id}: {:#?}", r.findings);

        // the game's code against itself: every class reads, and every
        // reference the game makes to its own classes resolves — a single
        // "missing" here would be the lookup's mistake, not the game's
        let code = r.code.as_ref().expect("the game's .jar has classes");
        eprintln!(
            "{id}: {} classes; {} code references checked, {} unchecked",
            code.classes, code.checked, code.unchecked
        );
        assert!(!code.intermediary);
        assert!(code.classes > 10_000, "{}", code.classes);
        assert!(code.checked > 100_000, "{}", code.checked);
        assert!(code.findings.is_empty(), "{id}: {:#?}", code.findings);
        checked += 1;
    }
    eprintln!("{checked} version(s) checked");
}
