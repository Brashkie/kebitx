//! A mods folder with one problem of each kind the loader refuses, built as
//! real `.jar` files. No game is needed: `minecraft` is then not judged.

use kebitx_java::modset::{analyze_set, SetFinding};
use std::path::{Path, PathBuf};

fn jar(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let stage = dir.join(format!("{name}.stage"));
    let mut entries = Vec::new();
    for (rel, bytes) in files {
        let p = stage.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        entries.push((rel.to_string(), p));
    }
    let out = dir.join(name);
    kebitx_build::zip::write_zip(&out, &entries).unwrap();
    std::fs::remove_dir_all(&stage).unwrap();
    out
}

fn fabric(id: &str, version: &str, extra: &str) -> Vec<u8> {
    format!(r#"{{"schemaVersion":1,"id":"{id}","version":"{version}"{extra}}}"#).into_bytes()
}

#[test]
fn what_the_loader_refuses_and_nothing_else() {
    let root = std::env::temp_dir().join(format!("kebitx-modset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mods = root.join("mods");
    let work = root.join("work");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::create_dir_all(&work).unwrap();

    // `a` needs b >= 2.0 (here 1.5), c (nowhere), h (bundled in g: fine),
    // and breaks d < 1.5 (here 1.2)
    let a = fabric(
        "a",
        "1.0.0",
        r#","depends":{"b":">=2.0","c":"*","h":"*","minecraft":"~26.3","fabricloader":">=0.19"},"breaks":{"d":"<1.5"}"#,
    );
    jar(&mods, "a.jar", &[("fabric.mod.json", &a)]);
    jar(
        &mods,
        "b.jar",
        &[("fabric.mod.json", &fabric("b", "1.5.0", ""))],
    );
    jar(
        &mods,
        "d.jar",
        &[("fabric.mod.json", &fabric("d", "1.2.0", ""))],
    );
    // the same mod twice
    jar(
        &mods,
        "e-1.jar",
        &[("fabric.mod.json", &fabric("e", "1.0.0", ""))],
    );
    jar(
        &mods,
        "e-2.jar",
        &[("fabric.mod.json", &fabric("e", "2.0.0", ""))],
    );
    // g bundles h
    let h = jar(
        &work,
        "h.jar",
        &[("fabric.mod.json", &fabric("h", "3.0.0", ""))],
    );
    let h_bytes = std::fs::read(&h).unwrap();
    jar(
        &mods,
        "g.jar",
        &[
            ("fabric.mod.json", &fabric("g", "1.0.0", "")),
            ("META-INF/jars/h.jar", &h_bytes),
        ],
    );
    // a library: no mod metadata
    jar(&mods, "lib.jar", &[("lib/Thing.txt", b"x")]);

    let r = analyze_set(&mods, None);
    let _ = std::fs::remove_dir_all(&root);

    let s = String::from;
    let expected = vec![
        SetFinding::Duplicate {
            id: s("e"),
            jars: vec![s("e-1.jar"), s("e-2.jar")],
        },
        SetFinding::Missing {
            jar: s("a.jar"),
            id: s("a"),
            dependency: s("c"),
            wanted: s("*"),
        },
        SetFinding::WrongVersion {
            jar: s("a.jar"),
            id: s("a"),
            dependency: s("b"),
            wanted: s(">=2.0"),
            found: s("1.5.0"),
        },
        SetFinding::Breaks {
            jar: s("a.jar"),
            id: s("a"),
            other: s("d"),
            rule: s("<1.5"),
            found: s("1.2.0 in d.jar"),
        },
        SetFinding::NotAMod { jar: s("lib.jar") },
    ];
    let mut expected = expected;
    expected.sort();
    assert_eq!(r.findings, expected, "{:#?}", r.findings);
    assert_eq!(r.broken(), 4);
    assert_eq!(r.mods.len(), 7);
}
