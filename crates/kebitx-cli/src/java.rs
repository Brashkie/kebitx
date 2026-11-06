//! The Java Edition commands: `analyze` on a pack, a mod or a `mods/` folder,
//! and `repair` on a `mods/` folder.

use kebitx_java::code::CodeFinding;
use kebitx_java::modset::{ModFolder, SetFinding};
use kebitx_java::repair::{apply, plan, Fix};
use kebitx_java::{Finding, JavaAssets, JavaJar, Kind, Source};
use std::path::Path;

/// A titled list, cut to 20 lines unless `--all`. Nothing when it is empty.
fn print_list(title: &str, items: &[String], show_all: bool) {
    if items.is_empty() {
        return;
    }
    let limit = if show_all { usize::MAX } else { 20 };
    println!("  {title}: {}", items.len());
    for i in items.iter().take(limit) {
        println!("    {i}");
    }
    if items.len() > limit {
        println!("    ... and {} more (use --all)", items.len() - limit);
    }
}

/// The game to check against: the version `--game` names, else what `guess`
/// picks. `Err` is the exit code: the version asked for is not installed.
fn choose_game(
    java: Option<&JavaAssets>,
    game: Option<&str>,
    guess: impl FnOnce(&JavaAssets) -> Option<JavaJar>,
) -> Result<Option<JavaJar>, i32> {
    match (java, game) {
        (Some(j), Some(id)) => match j.jar_by_id(id, true) {
            Some(jar) => Ok(Some(jar)),
            None => {
                eprintln!("Java {id} is not installed (no versions/{id}/{id}.jar)");
                Err(2)
            }
        },
        (Some(j), None) => Ok(guess(j)),
        (None, _) => Ok(None),
    }
}

/// `kebitx analyze` on a `mods/` folder: every mod against the game and the
/// others, and what the loader refuses. Exit 1 when something is proven
/// broken, 0 otherwise, 2 when the game asked for is not installed.
pub fn run_java_set(root: &Path, show_all: bool, game: Option<&str>) -> i32 {
    let java = JavaAssets::discover();
    let folder = ModFolder::open(root);
    let jar = match choose_game(java.as_ref(), game, |j| folder.game_for(j)) {
        Ok(jar) => jar,
        Err(code) => return code,
    };
    let r = folder.analyze(jar.as_ref());

    println!(
        "Kebitx Analysis (Java Edition, mods folder) — {}",
        root.display()
    );
    println!();
    let mut loaders: std::collections::BTreeMap<&str, usize> = Default::default();
    for m in r.mods.iter().filter_map(|m| m.meta.as_ref()) {
        *loaders.entry(m.loader).or_default() += 1;
    }
    let by_loader: Vec<String> = loaders.iter().map(|(l, n)| format!("{l} {n}")).collect();
    println!(
        "  Mods:                {} ({})",
        r.mods.len(),
        by_loader.join(", ")
    );
    match &r.game {
        Some((id, true)) if game.is_some() => {
            println!("  Game:                Java {id} (chosen with --game)")
        }
        Some((id, true)) => println!("  Game:                Java {id} (every mod accepts it)"),
        Some((id, false)) => println!(
            "  Game:                Java {id} (newest installed stable: no installed version suits every mod)"
        ),
        None => println!("  Game:                not installed — nothing of the game is checked"),
    }
    println!();
    println!(
        "  {:<44} {:<30} {:>7} {:>6} {:>7}",
        "jar", "mod", "assets", "code", "info"
    );
    for m in &r.mods {
        let (id, version) = m
            .meta
            .as_ref()
            .map(|m| (m.id.as_str(), m.version.as_str()))
            .unwrap_or(("(no metadata)", ""));
        let assets = m.report.findings.iter().filter(|f| !f.is_info()).count();
        let code = m.report.code.as_ref().map_or(0, |c| c.broken());
        let info = m.report.findings.iter().filter(|f| f.is_info()).count()
            + m.report
                .code
                .as_ref()
                .map_or(0, |c| c.findings.len() - c.broken());
        let jar: String = m.jar.chars().take(44).collect();
        let module: String = format!("{id} {version}").chars().take(30).collect();
        println!("  {jar:<44} {module:<30} {assets:>7} {code:>6} {info:>7}");
    }
    println!();

    let show = |title: &str, items: Vec<String>| print_list(title, &items, show_all);
    let set_line = |f: &SetFinding| match f {
        SetFinding::Duplicate { id, jars } => format!(
            "mod {id} is in {} jars: {} (the loader refuses to start)",
            jars.len(),
            jars.join(", ")
        ),
        SetFinding::Missing {
            jar,
            id,
            dependency,
            wanted,
        } => format!("{id} ({jar}) needs {dependency} {wanted}, which no mod here provides"),
        SetFinding::WrongVersion {
            jar,
            id,
            dependency,
            wanted,
            found,
        } => format!("{id} ({jar}) needs {dependency} {wanted}; here it is {found}"),
        SetFinding::Breaks {
            jar,
            id,
            other,
            rule,
            found,
        } => format!("{id} ({jar}) declares it breaks {other} {rule}; here: {found}"),
        SetFinding::OverwriteConflict {
            target,
            method,
            mods,
        } => format!(
            "{} all @Overwrite {target}.{method}: only one change survives",
            mods.join(", ")
        ),
        SetFinding::MixedLoaders { loaders } => format!(
            "mods for {} in one folder: each loader loads only its own",
            loaders.join(" and ")
        ),
        SetFinding::NotAMod { jar } => format!("{jar}: no mod metadata (a library, or not a mod)"),
    };
    show(
        "The loader refuses this set",
        r.findings
            .iter()
            .filter(|f| !f.is_info())
            .map(set_line)
            .collect(),
    );
    show(
        "About the set (information)",
        r.findings
            .iter()
            .filter(|f| f.is_info())
            .map(set_line)
            .collect(),
    );
    let mut broken_lines = Vec::new();
    for m in &r.mods {
        for f in m.report.findings.iter().filter(|f| !f.is_info()) {
            broken_lines.push(format!("{}: {}", m.jar, java_line(f)));
        }
        for f in m
            .report
            .code
            .iter()
            .flat_map(|c| &c.findings)
            .filter(|f| !f.is_info())
        {
            broken_lines.push(format!("{}: {}", m.jar, code_line(f)));
        }
    }
    show("Broken in the mods", broken_lines);
    if r.broken() == 0 {
        println!("  OK — nothing proven broken");
        0
    } else {
        1
    }
}

/// `kebitx analyze` on a Java resource pack or mod: exit 1 when something is
/// proven broken, 0 otherwise, 2 when it cannot be opened.
pub fn run_java_analyze(root: &Path, show_all: bool, game: Option<&str>) -> i32 {
    let src = match Source::open(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let files = src.files();
    let kind = kebitx_java::analyze::kind_of(&src, &files);
    let java = JavaAssets::discover();
    // the version asked for; else the pack's format, or the one version a
    // mod names; else the newest stable one
    let jar = match choose_game(java.as_ref(), game, |j| match &kind {
        Kind::ResourcePack { formats } => j.jar_in(*formats),
        Kind::Mod {
            minecraft: Some(m), ..
        } => j.jar_for_requirement(m).or_else(|| j.jar_for(None)),
        _ => j.jar_for(None),
    }) {
        Ok(jar) => jar,
        Err(code) => return code,
    };
    let r = kebitx_java::analyze(&src, jar.as_ref());

    println!("Kebitx Analysis (Java Edition) — {}", root.display());
    println!();
    match &r.kind {
        Kind::ResourcePack { formats } => println!(
            "  Resource pack, format {}",
            match formats {
                Some((a, b)) if a == b => a.to_string(),
                Some((a, b)) => format!("{a} to {b}"),
                None => "unknown".into(),
            }
        ),
        Kind::Mod {
            loader,
            ids,
            minecraft,
            ..
        } => {
            println!("  {loader} mod: {}", ids.join(", "));
            if let Some(m) = minecraft {
                println!("  Declares Minecraft:  {m}");
            }
        }
        Kind::Assets => println!("  Assets (no pack.mcmeta, no mod metadata)"),
    }
    match &r.game {
        Some((id, true)) if game.is_some() => {
            println!("  Game:                Java {id} (chosen with --game)")
        }
        Some((id, true)) => println!("  Game:                Java {id} (the version it is for)"),
        Some((id, false)) => println!(
            "  Game:                Java {id} (newest installed stable: not the version it declares)"
        ),
        None => println!(
            "  Game:                not installed — references to `minecraft:` are not checked"
        ),
    }
    let ns: Vec<&str> = r.namespaces.iter().map(String::as_str).collect();
    println!("  Namespaces:          {}", ns.join(", "));
    println!();
    println!("  Blockstates:         {}", r.blockstates);
    println!("  Item definitions:    {}", r.items);
    println!("  Models:              {}", r.models);
    println!("  Textures:            {}", r.textures);
    println!("  Animated textures:   {}", r.animations);
    println!("  Languages:           {}", r.languages);
    println!("  References checked:  {}", r.checked);
    println!(
        "  References unchecked: {}  (another mod's namespace, or the game's when it is not installed)",
        r.unchecked
    );
    println!();

    let show = |title: &str, items: Vec<String>| print_list(title, &items, show_all);
    show(
        "Broken",
        r.findings
            .iter()
            .filter(|f| !f.is_info())
            .map(java_line)
            .collect(),
    );
    show(
        "Information",
        r.findings
            .iter()
            .filter(|f| f.is_info())
            .map(java_line)
            .collect(),
    );
    if let Some(c) = &r.code {
        println!();
        println!(
            "  Code:                {} classes, {} with @Mixin",
            c.classes, c.mixins
        );
        if c.nested_jars > 0 {
            println!(
                "  Bundled:             {} jar(s), {} classes (what their Mixins add to the game counts)",
                c.nested_jars, c.bundled_classes
            );
        }
        if !c.external.is_empty() {
            println!(
                "  Runs with:           {} (not bundled: what they add to the game cannot be seen)",
                c.external.join(", ")
            );
        }
        if c.intermediary {
            println!(
                "  Not compared: the mod names intermediary classes (net.minecraft.class_NNNN), \
                 so it was built for an obfuscated version; this game's .jar uses the real names"
            );
        } else {
            println!("  Game references checked:   {}", c.checked);
            println!(
                "  Game references unchecked: {}  (looked up through a JDK or library class)",
                c.unchecked
            );
        }
        show(
            "Code that fails on this version",
            c.findings
                .iter()
                .filter(|f| !f.is_info())
                .map(code_line)
                .collect(),
        );
        show(
            "Code not proven either way",
            c.findings
                .iter()
                .filter(|f| f.is_info())
                .map(code_line)
                .collect(),
        );
    }
    if r.broken() == 0 {
        println!("  OK — nothing proven broken");
        0
    } else {
        1
    }
}

pub fn run_java_repair(dir: &Path, args: &[String]) -> i32 {
    let mut fixes = vec![Fix::Duplicates];
    let mut game = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--fix" => {
                i += 1;
                let parsed: Option<Vec<Fix>> = args.get(i).and_then(|s| {
                    s.split(',')
                        .map(|f| match f {
                            "duplicates" => Some(Fix::Duplicates),
                            "incompatible" => Some(Fix::Incompatible),
                            _ => None,
                        })
                        .collect()
                });
                match parsed {
                    Some(f) if !f.is_empty() => fixes = f,
                    _ => {
                        eprintln!(
                            "--fix takes duplicates, incompatible, or both for a mods folder\n{}",
                            crate::USAGE
                        );
                        return 2;
                    }
                }
            }
            "--game" => {
                i += 1;
                game = args.get(i).cloned();
            }
            _ => {}
        }
        i += 1;
    }
    let do_apply = args.iter().any(|a| a == "--apply");
    let java = JavaAssets::discover();
    let folder = ModFolder::open(dir);
    let jar = match choose_game(java.as_ref(), game.as_deref(), |j| folder.game_for(j)) {
        Ok(jar) => jar,
        Err(code) => return code,
    };
    println!(
        "Kebitx Repair (Java Edition, mods folder) — {}",
        dir.display()
    );
    if let Some(j) = &jar {
        println!("  Game: Java {}", j.id);
    }
    let report = folder.analyze(jar.as_ref());
    let removals = plan(&report, &fixes);
    if removals.is_empty() {
        println!("  nothing to repair");
        return 0;
    }
    println!("  {} jar(s) to take out of the folder:", removals.len());
    for r in &removals {
        println!("    {}: {}", r.jar, r.reason);
    }
    if !do_apply {
        println!("  preview only: nothing was changed (add --apply)");
        return 0;
    }
    match apply(dir, &removals, jar.as_ref()) {
        Ok(a) if a.kept => {
            println!(
                "  kept: problems {} -> {}; the jars are in {}",
                a.before,
                a.after,
                a.backup.display()
            );
            println!("  undo: kebitx repair --undo \"{}\"", a.backup.display());
            0
        }
        Ok(a) => {
            println!(
                "  rolled back: not strictly better (problems {} -> {}); every jar is back",
                a.before, a.after
            );
            for p in &a.new_problems {
                println!("    new: {p}");
            }
            1
        }
        Err(e) => {
            eprintln!("  {e}");
            1
        }
    }
}

/// One finding of a Java pack or mod, as a report line.
fn java_line(f: &Finding) -> String {
    match f {
        Finding::MissingModel { file, model } => format!("{file}: model {model} does not exist"),
        Finding::MissingTexture { file, texture } => {
            format!("{file}: texture {texture} does not exist")
        }
        Finding::UnboundTexture {
            model,
            variable,
            used_by,
        } => format!(
            "{model}: a face asks for #{variable}, which its chain never sets (the missing texture shows; used by {used_by})"
        ),
        Finding::BadAnimation {
            texture,
            size,
            frame,
        } => format!(
            "{texture}: {}x{} does not divide into {}x{} frames (the game refuses it)",
            size.0, size.1, frame.0, frame.1
        ),
        Finding::Unreadable { file, why } => format!("{file}: unreadable ({why})"),
        Finding::NotPng { texture } => {
            format!("{texture}: not a PNG (the game checks the signature and refuses it)")
        }
        Finding::Proportions {
            texture,
            size,
            game,
        } => format!(
            "{texture}: {}x{}, the game's is {}x{} (other proportions)",
            size.0, size.1, game.0, game.1
        ),
        Finding::MissingLangKeys { file, missing, of } => {
            format!("{file}: {missing} of {of} English keys missing (English shows)")
        }
    }
}

/// One finding about a mod's code, as a report line.
fn code_line(f: &CodeFinding) -> String {
    match f {
            CodeFinding::MissingClass {
                class,
                used_by,
                uses,
            } => format!("class {class} does not exist (used by {used_by}, {uses} class(es))"),
            CodeFinding::MissingMember {
                owner,
                name,
                descriptor,
                field,
                used_by,
                uses,
            } => format!(
                "{} {owner}.{name} {descriptor} does not exist (used by {used_by}, {uses} class(es))",
                if *field { "field" } else { "method" }
            ),
            CodeFinding::MissingMixinTarget { mixin, target } => {
                format!("mixin {mixin} targets {target}, which does not exist")
            }
            CodeFinding::Unreadable { file, why } => format!("{file}: unreadable ({why})"),
            CodeFinding::MixinMember {
                mixin,
                target,
                how,
                member,
            } => format!(
                "mixin {mixin}: {how} {member} is not in {target} (Mixin refuses the class at start-up)"
            ),
            CodeFinding::MixinInjection {
                mixin,
                handler,
                missing,
                required,
                ..
            } => format!(
                "mixin {mixin}: {handler} finds no {missing} ({})",
                if *required {
                    "required: the game does not start"
                } else {
                    "not required: it silently does nothing"
                }
            ),
            CodeFinding::NotInGame {
                what,
                used_by,
                may_come_from,
            } => format!(
                "{what} is not in the game's .jar (used by {used_by}; may come from: {may_come_from})"
            ),
        }
}
