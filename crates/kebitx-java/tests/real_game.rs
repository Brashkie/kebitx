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

/// The lazy index answers from names: every class file of the game must be
/// the class its entry says, or a lookup would ask for the wrong one. Every
/// class name the `.jar` holds must also be a readable class.
#[test]
fn every_class_of_the_game_is_found_by_its_entry_name() {
    use kebitx_java::code::GameIndex;
    let Some(java) = JavaAssets::discover() else {
        return;
    };
    for (id, _) in java.installed() {
        let Some(jar) = java.jar_by_id(&id, true) else {
            continue;
        };
        let index = GameIndex::from_jar(&jar);
        let names: Vec<&String> = jar
            .zip
            .names()
            .filter(|n| n.ends_with(".class") && !n.starts_with("META-INF/"))
            .collect();
        assert!(names.len() > 1000, "{id}: {} classes", names.len());
        for n in names {
            let b = jar.zip.read(n, 16 * 1024 * 1024).unwrap();
            let c = kebitx_java::class::parse(&b).unwrap_or_else(|e| panic!("{id} {n}: {e}"));
            assert_eq!(format!("{}.class", c.name), *n, "{id}");
            assert!(index.get(&c.name).is_some(), "{id}: {}", c.name);
        }
        let multi = jar
            .zip
            .names()
            .filter(|n| n.ends_with(".class") && n.starts_with("META-INF/"))
            .count();
        assert_eq!(multi, 0, "{id}: multi-release classes are left out");
    }
}
