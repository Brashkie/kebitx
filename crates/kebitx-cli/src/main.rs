#![forbid(unsafe_code)]

use kebitx_build::watch::Outcome;
use kebitx_build::{BuildOptions, Project, Target};
use kebitx_core::analyze::{analyze_with, analyze_world_with, is_resource_pack, resource_packs_in};
use kebitx_core::compat::{check_world, module_inventory, CompatIssue, CompatReport, Severity};
use kebitx_core::explain;
use kebitx_core::graphics::{self, GraphicsIssue};
use kebitx_core::manifest::{pack_at, packs_in, parse_version, Pack};
use kebitx_core::optimize;
use kebitx_core::passes::unused_definition::{self, DefKind};
use kebitx_core::passes::{duplicate_resource, unused_asset};
use kebitx_core::report::{AnalysisReport, Issue};
use kebitx_core::vanilla::VanillaCatalog;
use std::env;
use std::path::Path;
use std::process::exit;

const USAGE: &str = "usage:
  kebitx analyze <pack_or_world_path> [--all] [--timings]
  kebitx analyze <java_pack_or_mod: folder, .jar or .zip> [--all] [--game <version>]
  kebitx build [project_dir] [--target local|development|mcaddon|mcpack|mcworld] [--no-check] [--watch]
  kebitx repair <pack_or_world_path> [--fix case,unused] [--apply]
  kebitx repair <mods_folder> [--fix duplicates,incompatible] [--apply] [--game <version>]
  kebitx repair --undo <backup_folder>
  kebitx compare <before> <after> [--all]
  kebitx migrate <java_pack_folder_or_jar> [--out <new_folder>] [--all]
  kebitx catalog [--list stems|flipbooks|tiles|sounds]
  kebitx --version | --help";

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version" | "-V") => {
            println!("kebitx {}", env!("CARGO_PKG_VERSION"));
            exit(0);
        }
        Some("--help" | "-h" | "help") => {
            println!("kebitx {}\n\n{USAGE}", env!("CARGO_PKG_VERSION"));
            exit(0);
        }
        Some("build") => exit(run_build(&args[2..])),
        Some("repair") => exit(run_repair(&args[2..])),
        Some("compare") => exit(run_compare(&args[2..])),
        Some("migrate") => exit(run_migrate(&args[2..])),
        Some("catalog") => exit(run_catalog(&args[2..])),
        Some("analyze") => {
            let Some(path) = args.get(2) else {
                eprintln!("{USAGE}");
                exit(2);
            };
            let show_all = args.iter().any(|a| a == "--all");
            let root = Path::new(path);
            if kebitx_java::is_mod_folder(root) {
                let game = args
                    .iter()
                    .position(|a| a == "--game")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str);
                exit(run_java_set(root, show_all, game));
            }
            if kebitx_java::is_java(root) {
                let game = args
                    .iter()
                    .position(|a| a == "--game")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str);
                exit(run_java_analyze(root, show_all, game));
            }
            if !root.is_dir() {
                eprintln!("not a folder: {path}");
                exit(2);
            }

            let show_timings = args.iter().any(|a| a == "--timings");
            let wall = std::time::Instant::now();
            // Built once and reported, so the reader can tell a clean pack from one
            // analysed without knowing what the game provides.
            let catalog = timed("game catalog", || {
                VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty)
            });

            println!("Kebitx Analysis — {path}");
            println!();
            print_catalog(&catalog);

            // A folder with a resource manifest is one pack. A folder that holds
            // resource packs is a world: every pack inside is resolved against
            // the union of all of them. `level.dat` or pack lists make it a world
            // even with no resource pack inside: its lists can still point at
            // packs that are missing. Anything else (a loose pack without a
            // manifest, as an author's work folder often is) is analysed as a
            // single pack rather than refused.
            let is_world = [
                "level.dat",
                "world_behavior_packs.json",
                "world_resource_packs.json",
            ]
            .iter()
            .any(|f| root.join(f).is_file());
            let is_pack = is_resource_pack(root);
            let single = !is_world && (is_pack || resource_packs_in(root).is_empty());
            let game = if catalog.is_empty() {
                None
            } else {
                parse_version(&catalog.version)
            };
            let extra_packs = if single {
                vec![pack_at(root)]
            } else {
                packs_in(root)
            };

            // Every stage only reads the catalog and the files: they run at the
            // same time, and are printed afterwards in the usual order. Nothing
            // changes the files during an analysis, so each is read only once.
            kebitx_core::snapshot::enable();
            let (packs, compat, modules, extras) = std::thread::scope(|s| {
                let refs = s.spawn(|| {
                    timed("references", || {
                        if single {
                            vec![(String::new(), analyze_with(root, &catalog))]
                        } else if is_pack {
                            Vec::new()
                        } else {
                            analyze_world_with(root, &catalog)
                        }
                    })
                });
                let compat = s.spawn(|| {
                    (!single).then(|| timed("compatibility", || check_world(root, game)))
                });
                let modules = s.spawn(|| {
                    if single {
                        Default::default()
                    } else {
                        timed("script modules", || module_inventory(root))
                    }
                });
                let extras = compute_extras(&extra_packs, &catalog);
                (join(refs), join(compat), join(modules), extras)
            });
            kebitx_core::snapshot::disable();

            let broken = if single {
                let r = &packs[0].1;
                print_report(r);
                print_issues(r, show_all);
                print_graphics(&extras.graphics, show_all);
                print_assets(&extras, catalog.is_empty(), show_all);
                print_optimize(&extras.opt, show_all);
                summary(r)
            } else {
                let mut total = AnalysisReport::default();
                println!(
                    "  World: {} resource packs, resolved against their union",
                    packs.len()
                );
                println!();
                println!(
                    "  {:24} {:>8} {:>9} {:>7} {:>6} {:>6}",
                    "pack", "entities", "atlas", "broken", "case", "empty"
                );
                for (name, r) in &packs {
                    println!(
                        "  {:24} {:>8} {:>9} {:>7} {:>6} {:>6}",
                        name,
                        r.entities,
                        r.atlas_entries,
                        r.broken(),
                        r.case_mismatches,
                        r.atlas_empty
                    );
                    total.absorb(r);
                }
                println!();
                print_report(&total);
                print_issues(&total, show_all);
                let compat = compat.unwrap_or_default();
                print_compat(&compat, show_all);
                print_modules(&modules);
                print_graphics(&extras.graphics, show_all);
                print_assets(&extras, catalog.is_empty(), show_all);
                print_optimize(&extras.opt, show_all);
                summary(&total) + compat.errors()
            };
            if show_timings {
                print_timings(wall.elapsed());
            }
            exit(if broken == 0 { 0 } else { 1 });
        }
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    }
}

/// Stage timings (`--timings`). A mutex, not a thread-local: the stages run on
/// several threads at once.
static TIMINGS: std::sync::Mutex<Vec<(&'static str, std::time::Duration)>> =
    std::sync::Mutex::new(Vec::new());

/// Run `f`, remembering how long it took under `label`.
fn timed<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let out = f();
    if let Ok(mut t) = TIMINGS.lock() {
        t.push((label, start.elapsed()));
    }
    out
}

/// Each stage's own time, then the wall-clock time of the whole analysis.
/// Stages overlap, so the wall clock is less than their sum.
fn print_timings(wall: std::time::Duration) {
    let Ok(t) = TIMINGS.lock() else { return };
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    println!();
    println!("  Timings (stages run concurrently):");
    for (label, d) in t.iter() {
        println!("    {label:22} {:>8.1} ms", ms(*d));
    }
    println!("    {:22} {:>8.1} ms", "wall clock", ms(wall));
}

fn join<T>(h: std::thread::ScopedJoinHandle<'_, T>) -> T {
    h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))
}

/// The graphics and optimisation passes, at the same time.
struct Extras {
    graphics: graphics::GraphicsReport,
    unused: unused_asset::UnusedReport,
    defs: unused_definition::UnusedDefinitions,
    dups: Vec<duplicate_resource::DuplicateGroup>,
    opt: optimize::OptReport,
}

fn compute_extras(packs: &[Pack], catalog: &VanillaCatalog) -> Extras {
    std::thread::scope(|s| {
        let g = s.spawn(|| timed("graphics", || graphics::check(packs, catalog)));
        let u = s.spawn(|| timed("unused assets", || unused_asset::run(packs, catalog)));
        let d = s.spawn(|| {
            timed("unused definitions", || {
                unused_definition::run(packs, catalog)
            })
        });
        let o = s.spawn(|| timed("optimization", || optimize::run(packs)));
        let dups = timed("identical files", || duplicate_resource::run(packs));
        Extras {
            graphics: join(g),
            unused: join(u),
            defs: join(d),
            dups,
            opt: join(o),
        }
    })
}

/// What costs memory, space or frame time on a phone. Information.
fn print_optimize(o: &optimize::OptReport, show_all: bool) {
    use optimize::OptIssue;
    let limit = if show_all { usize::MAX } else { 10 };
    let mb = |b: u64| b as f64 / 1048576.0;
    println!("  Optimization (phones as the reference):");
    println!(
        "    {} textures ({} unreadable), {:.0} MB decoded if all were loaded; {} models, {} cubes",
        o.images,
        o.unreadable_images,
        mb(o.video_bytes),
        o.models,
        o.cubes
    );
    let count = |f: fn(&OptIssue) -> bool| o.issues.iter().filter(|i| f(i)).count();
    println!("    oversized icons: {};  textures of {}x{} or more: {};  images mislabelled: {};  models of {}+ cubes: {}",
             count(|i| matches!(i, OptIssue::BigIcon { .. })),
             2048, 2048, count(|i| matches!(i, OptIssue::BigTexture { .. })),
             count(|i| matches!(i, OptIssue::MislabelledImage { .. })),
             optimize::HEAVY_MODEL_CUBES, count(|i| matches!(i, OptIssue::HeavyModel { .. })));
    for i in o.issues.iter().take(limit) {
        match i {
            OptIssue::BigIcon {
                pack,
                width,
                height,
                bytes,
            } => println!(
                "    ICON     {pack}/pack_icon.png: {width}x{height}, {:.0} KB",
                *bytes as f64 / 1024.0
            ),
            OptIssue::BigTexture {
                pack,
                path,
                width,
                height,
                video_bytes,
            } => println!(
                "    TEXTURE  {pack}/{path}: {width}x{height}, {:.0} MB decoded",
                mb(*video_bytes)
            ),
            OptIssue::MislabelledImage {
                pack,
                path,
                content,
            } => println!("    FORMAT   {pack}/{path}: the content is {content:?}"),
            OptIssue::HeavyModel {
                geometry,
                file,
                cubes,
                bones,
            } => println!("    MODEL    {geometry}: {cubes} cubes, {bones} bones  ({file})"),
        }
        print_explanation(&explain::optimize(i));
    }
    if o.issues.len() > limit {
        println!("    ... and {} more (use --all)", o.issues.len() - limit);
    }
    println!();
}

/// `kebitx build`: returns the exit code (0 built, 1 build failed, 2 usage).
fn run_build(args: &[String]) -> i32 {
    let mut dir = ".".to_string();
    let mut opts = BuildOptions::default();
    let mut watch = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-check" => opts.check = Some(false),
            "--watch" => watch = true,
            "--target" => {
                i += 1;
                opts.target = match args.get(i).and_then(|t| Target::parse(t)) {
                    Some(t) => Some(t),
                    None => {
                        eprintln!("--target needs one of: {}\n{USAGE}", Target::NAMES);
                        return 2;
                    }
                };
            }
            other if !other.starts_with('-') => dir = other.to_string(),
            other => {
                eprintln!("unknown option {other}\n{USAGE}");
                return 2;
            }
        }
        i += 1;
    }
    let project = match Project::load(Path::new(&dir)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    println!(
        "Kebitx Build — {} ({})",
        project.name,
        project.root.display()
    );
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    if watch {
        println!("  watching for changes (Ctrl+C to stop)");
        kebitx_build::watch::watch(
            &project.root,
            &opts,
            &catalog,
            std::time::Duration::from_millis(500),
            None,
            |o| {
                println!();
                match o {
                    Outcome::Built(r) => print_built(&r),
                    Outcome::Failed(e) => print_failed(&e),
                    Outcome::BadProject(e) => eprintln!("  {e}"),
                }
            },
        );
        return 0;
    }
    match kebitx_build::build(&project, &opts, &catalog) {
        Ok(r) => {
            print_built(&r);
            0
        }
        Err(e) => {
            print_failed(&e);
            1
        }
    }
}

/// `kebitx catalog [--list ...]`: what the installed game provides, as kebitx
/// sees it — the evidence every "the game ships it" verdict rests on.
fn run_catalog(args: &[String]) -> i32 {
    let Some(c) = VanillaCatalog::discover() else {
        eprintln!("  Minecraft Bedrock is not installed (or not found)");
        return 2;
    };
    let list = args
        .iter()
        .position(|a| a == "--list")
        .and_then(|i| args.get(i + 1));
    let Some(list) = list else {
        println!(
            "Kebitx Catalog — Minecraft {}{}",
            c.version,
            if c.from_cache { " (cached)" } else { "" }
        );
        println!(
            "  {} bundles read, {} unreadable",
            c.bundles_read, c.bundles_bad
        );
        println!(
            "  {} textures, {} texture and sound paths",
            c.texture_count(),
            c.asset_stems().count()
        );
        println!(
            "  {} geometries, {} animations, {} animation controllers, {} render controllers, {} materials",
            c.geometry_count(),
            c.animation_count(),
            c.animation_controller_count(),
            c.render_controller_count(),
            c.material_count()
        );
        println!(
            "  {} flipbooks, {} textures on the terrain atlas, {} on the item atlas, {} sound events",
            c.flipbook_count(),
            c.terrain_texture_count(),
            c.item_atlas_count(),
            c.sound_events().count()
        );
        return 0;
    };
    let mut lines: Vec<String> = match list.as_str() {
        "stems" => c.asset_stems().cloned().collect(),
        "flipbooks" => c
            .flipbooks()
            .map(|(s, t)| {
                format!(
                    "{s} -> {}{}",
                    t.tile,
                    t.index.map(|i| format!("[{i}]")).unwrap_or_default()
                )
            })
            .collect(),
        "tiles" => c
            .terrain_textures()
            .map(|(s, ts)| {
                let tiles: Vec<String> = ts
                    .iter()
                    .map(|t| {
                        format!(
                            "{}{}",
                            t.tile,
                            t.index.map(|i| format!("[{i}]")).unwrap_or_default()
                        )
                    })
                    .collect();
                format!("{s} -> {}", tiles.join(", "))
            })
            .collect(),
        "sounds" => c.sound_events().map(|(e, d)| format!("{e} {d}")).collect(),
        other => {
            eprintln!("  unknown list: {other} (stems, flipbooks, tiles, sounds)");
            return 2;
        }
    };
    lines.sort();
    // a list is meant for pipes: a reader that stops early (`head`) ends the
    // output quietly instead of a panic
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    for l in lines {
        if writeln!(out, "{l}").is_err() {
            break;
        }
    }
    0
}

/// `kebitx migrate <java pack> [--out <folder>] [--all]`: the compatibility
/// report; with `--out`, the conversion of what provably converts, analysed
/// again before it is called done.
fn run_migrate(args: &[String]) -> i32 {
    use kebitx_migrate::Verdict;
    let show_all = args.iter().any(|a| a == "--all");
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1));
    let Some(src) = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with('-') && (*i == 0 || args[i - 1] != "--out"))
        .map(|(_, a)| a)
    else {
        eprintln!("{USAGE}");
        return 2;
    };
    // a folder, or a .jar / .zip (a mod, a zipped pack) extracted first
    let input = match kebitx_migrate::input::open(Path::new(src)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("  {e}");
            return 2;
        }
    };
    let root = input.root.as_path();
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    if catalog.is_empty() {
        eprintln!(
            "  the game catalog is needed to know what converts (Minecraft Bedrock installed)"
        );
        return 2;
    }
    let (_, _, pack_format) = kebitx_migrate::pack_meta(root);
    let sources = kebitx_migrate::Sources::discover_for(Some(root), pack_format);
    let mut r = kebitx_migrate::inspect(root, sources.refs(&catalog));
    if let Some(stem) = Path::new(src)
        .file_stem()
        .filter(|_| !Path::new(src).is_dir())
    {
        r.name = stem.to_string_lossy().to_string();
    }
    println!("Kebitx Migrate — {src}");
    match (&sources.java, &sources.java_jar) {
        (Some(j), Some(jar)) => println!(
            "  Java reference: {} (assets index {}, game {} for format {}{})",
            j.dir.display(),
            j.index,
            jar.id,
            jar.format,
            if jar.exact { "" } else { ": not the pack's format, the newest stable stands in" }
        ),
        (Some(j), None) => println!(
            "  Java reference: {} (assets index {}; no game .jar: pixels and strings cannot be proven)",
            j.dir.display(),
            j.index
        ),
        _ => println!(
            "  Java reference: not found (Java Edition installed?): sounds, pixels and strings cannot be proven"
        ),
    }
    if !r.is_java_pack {
        println!(
            "  no pack.mcmeta and no mod metadata: not a Java pack or mod (or not its root folder)"
        );
        return 2;
    }
    println!(
        "  Java pack: {} (pack_format {})",
        r.description,
        r.pack_format
            .map(|f| f.to_string())
            .unwrap_or_else(|| "?".into())
    );
    let total = r.entries.len().max(1);
    for v in [Verdict::Converts, Verdict::Adapts, Verdict::Rebuild] {
        let n = r.count(v);
        println!(
            "  {:9} {:6} files ({:.0}%)",
            v.name(),
            n,
            n as f64 * 100.0 / total as f64
        );
    }
    println!();
    for ((v, reason), n) in r.by_reason() {
        println!("    {:9} {n:6}  {reason}", v.name());
    }
    if !r.flipbooks.is_empty() {
        println!();
        println!(
            "  {} animated texture(s) become flipbooks",
            r.flipbooks.len()
        );
    }
    if !r.images.is_empty() {
        println!();
        println!(
            "  {} picture(s) cut or composed from pack textures (shared sheets, other layouts)",
            r.images.len()
        );
    }
    if let Some((loader, name)) = &r.mod_info {
        let items = r
            .behavior
            .keys()
            .filter(|p| p.starts_with("items/"))
            .count();
        let blocks = r
            .behavior
            .keys()
            .filter(|p| p.starts_with("blocks/"))
            .count();
        println!();
        println!(
            "  {name} ({loader} mod): {items} item(s) and {blocks} block(s) as Bedrock custom ones; {} task(s) for a person in kebitx_drafts/mod_tasks.txt",
            r.tasks.len()
        );
    }
    if !r.cem.is_empty() {
        println!();
        for c in &r.cem {
            let proven = c.parts.values().filter(|b| b.is_some()).count();
            println!(
                "  OptiFine model {}: {proven} of {} part(s) proven to be bones of {}; {} animation expression(s) not converted",
                c.entity,
                c.parts.len(),
                c.geometry.as_deref().unwrap_or("no Bedrock geometry"),
                c.animations
            );
        }
    }
    if !r.models.is_empty() {
        println!();
        let unverified = r.models.iter().filter(|m| m.unverified > 0).count();
        let unmapped = r
            .models
            .iter()
            .filter(|m| m.textures.values().any(Option::is_none))
            .count();
        println!(
            "  {} changed model(s) converted to Bedrock geometry ({} with rotations to check in game, {} with textures not proven); custom blocks and attachables drafted in kebitx_drafts/",
            r.models.len(),
            unverified,
            unmapped
        );
    }
    if !r.languages.is_empty() {
        println!();
        for (file, l) in &r.languages {
            println!(
                "  {file} -> {}: {} string(s) proven ({} to write), {} without proof{}",
                l.bedrock.as_deref().unwrap_or("no Bedrock language"),
                l.mapped,
                l.lines.len(),
                l.unmapped,
                if l.unwritable > 0 {
                    format!(", {} with line breaks", l.unwritable)
                } else {
                    String::new()
                }
            );
        }
    }
    if !r.sound_events.is_empty() {
        println!();
        println!(
            "  sound events: {} in sounds.json, {} rewritten in sound_definitions.json",
            r.sound_events.len(),
            r.sound_definitions.len()
        );
        let mut by: std::collections::BTreeMap<(Verdict, &str), usize> =
            std::collections::BTreeMap::new();
        for e in &r.sound_events {
            *by.entry((e.verdict, e.reason)).or_insert(0) += 1;
        }
        for ((v, reason), n) in by {
            println!("    {:9} {n:6}  {reason}", v.name());
        }
    }
    if show_all {
        println!();
        for e in &r.entries {
            match &e.bedrock {
                Some(b) => println!("    {:9} {} -> {b}", e.verdict.name(), e.java),
                None => println!("    {:9} {}  ({})", e.verdict.name(), e.java, e.reason),
            }
        }
        for e in &r.sound_events {
            match &e.bedrock {
                Some(b) => println!(
                    "    {:9} sound event {} -> {b}  ({})",
                    e.verdict.name(),
                    e.java,
                    e.reason
                ),
                None => println!(
                    "    {:9} sound event {}  ({})",
                    e.verdict.name(),
                    e.java,
                    e.reason
                ),
            }
        }
    }
    let Some(out) = out else {
        println!();
        println!("  report only: add --out <new folder> to write the Bedrock pack");
        return 0;
    };
    match kebitx_migrate::convert(root, &r, Path::new(out)) {
        Ok(c) => {
            println!();
            println!(
                "  wrote {} file(s) to {}{}",
                c.files,
                c.out.display(),
                if c.skipped_duplicates > 0 {
                    format!(
                        " ({} sharing a target with another, skipped)",
                        c.skipped_duplicates
                    )
                } else {
                    String::new()
                }
            );
            if let Some(bp) = &c.behavior_out {
                println!(
                    "  behavior pack (recipes, loot tables) written to {}",
                    bp.display()
                );
            }
            // the converted pack goes through the same analysis as any other
            let a = analyze_with(&c.out, &catalog);
            println!(
                "  analysis of the result: {} broken reference(s), {} case mismatch(es)",
                a.broken(),
                a.case_mismatches
            );
            if a.broken() + a.case_mismatches == 0 {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("  {e}");
            1
        }
    }
}

/// `kebitx analyze` on a `mods/` folder: every mod against the game and the
/// others, and what the loader refuses. Exit 1 when something is proven
/// broken, 0 otherwise, 2 when the game asked for is not installed.
fn run_java_set(root: &Path, show_all: bool, game: Option<&str>) -> i32 {
    use kebitx_java::modset::{self, SetFinding};
    let java = kebitx_java::JavaAssets::discover();
    let metas: Vec<modset::ModMeta> = modset::jars_in(root)
        .iter()
        .filter_map(|p| kebitx_java::Source::open(p).ok())
        .filter_map(|s| modset::meta_of(&s))
        .collect();
    let jar = match (java.as_ref(), game) {
        (Some(j), Some(id)) => match j.jar_by_id(id, true) {
            Some(jar) => Some(jar),
            None => {
                eprintln!("Java {id} is not installed (no versions/{id}/{id}.jar)");
                return 2;
            }
        },
        (Some(j), None) => modset::game_for(j, &metas.iter().collect::<Vec<_>>()),
        (None, _) => None,
    };
    let r = modset::analyze_set(root, jar.as_ref());

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

    let limit = if show_all { usize::MAX } else { 20 };
    let show = |title: &str, items: Vec<String>| {
        if items.is_empty() {
            return;
        }
        println!("  {title}: {}", items.len());
        for i in items.iter().take(limit) {
            println!("    {i}");
        }
        if items.len() > limit {
            println!("    ... and {} more (use --all)", items.len() - limit);
        }
    };
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
fn run_java_analyze(root: &Path, show_all: bool, game: Option<&str>) -> i32 {
    use kebitx_java::{JavaAssets, Kind, Source};
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
    let jar = match (java.as_ref(), game) {
        (Some(j), Some(id)) => match j.jar_by_id(id, true) {
            Some(jar) => Some(jar),
            None => {
                eprintln!("Java {id} is not installed (no versions/{id}/{id}.jar)");
                return 2;
            }
        },
        (Some(j), None) => match &kind {
            Kind::ResourcePack { formats } => j.jar_in(*formats),
            Kind::Mod {
                minecraft: Some(m), ..
            } => j.jar_for_requirement(m).or_else(|| j.jar_for(None)),
            _ => j.jar_for(None),
        },
        (None, _) => None,
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

    let limit = if show_all { usize::MAX } else { 20 };
    let show = |title: &str, items: Vec<String>| {
        if items.is_empty() {
            return;
        }
        println!("  {title}: {}", items.len());
        for i in items.iter().take(limit) {
            println!("    {i}");
        }
        if items.len() > limit {
            println!("    ... and {} more (use --all)", items.len() - limit);
        }
    };
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

/// `kebitx compare <before> <after>`: exit 1 when something that worked
/// before fails now, 0 otherwise, 2 on usage errors.
fn run_compare(args: &[String]) -> i32 {
    let show_all = args.iter().any(|a| a == "--all");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let [before, after] = paths.as_slice() else {
        eprintln!("{USAGE}");
        return 2;
    };
    for p in [before, after] {
        if !Path::new(p.as_str()).is_dir() {
            eprintln!("not a folder: {p}");
            return 2;
        }
    }
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    println!("Kebitx Compare — {before} -> {after}");
    let (b, a) = std::thread::scope(|s| {
        let b = s.spawn(|| kebitx_core::regression::snapshot(Path::new(before.as_str()), &catalog));
        let a = kebitx_core::regression::snapshot(Path::new(after.as_str()), &catalog);
        (join(b), a)
    });
    let r = kebitx_core::regression::compare(&b, &a);
    let limit = if show_all { usize::MAX } else { 15 };
    let list = |title: &str, items: Vec<String>| {
        println!("  {title}: {}", items.len());
        for i in items.iter().take(limit) {
            println!("    {i}");
        }
        if items.len() > limit {
            println!("    ... and {} more (use --all)", items.len() - limit);
        }
    };
    println!(
        "  problems: {} before, {} after",
        b.problems.len(),
        a.problems.len()
    );
    list(
        "NEW problems (worked before, fail now)",
        r.new_problems.clone(),
    );
    list("fixed problems", r.fixed_problems.clone());
    if !r.removed_packs.is_empty() || !r.added_packs.is_empty() {
        list(
            "packs GONE (there before, not now)",
            r.removed_packs.clone(),
        );
        list(
            "problems lost with their pack (not fixed: the pack is gone)",
            r.lost_problems.clone(),
        );
        println!("  packs added: {}", r.added_packs.len());
    }
    list(
        "removed (defined before, gone now: other packs or worlds may still use them)",
        r.removed
            .iter()
            .map(|(k, id)| format!("{k:20} {id}"))
            .collect(),
    );
    println!("  added: {}", r.added.len());
    if r.is_regression() {
        println!(
            "  REGRESSION: {} new problem(s), {} pack(s) gone",
            r.new_problems.len(),
            r.removed_packs.len()
        );
        1
    } else {
        println!("  OK — nothing that worked before fails now");
        0
    }
}

/// `kebitx repair`: preview by default, `--apply` to write, `--undo` to put a
/// backup back. Exit code: 0 done (or nothing to do), 1 rolled back or
/// failed, 2 usage.
/// `kebitx repair <mods folder>`: preview by default; `--apply` takes the
/// jars out, analyses again and keeps the result only if strictly better.
fn run_java_repair(dir: &Path, args: &[String]) -> i32 {
    use kebitx_java::modset;
    use kebitx_java::repair::{apply, plan, Fix};
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
                            "--fix takes duplicates, incompatible, or both for a mods folder\n{USAGE}"
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
    let java = kebitx_java::JavaAssets::discover();
    let metas: Vec<modset::ModMeta> = modset::jars_in(dir)
        .iter()
        .filter_map(|p| kebitx_java::Source::open(p).ok())
        .filter_map(|s| modset::meta_of(&s))
        .collect();
    let jar = match (java.as_ref(), game.as_deref()) {
        (Some(j), Some(id)) => match j.jar_by_id(id, true) {
            Some(jar) => Some(jar),
            None => {
                eprintln!("Java {id} is not installed");
                return 2;
            }
        },
        (Some(j), None) => modset::game_for(j, &metas.iter().collect::<Vec<_>>()),
        (None, _) => None,
    };
    println!(
        "Kebitx Repair (Java Edition, mods folder) — {}",
        dir.display()
    );
    if let Some(j) = &jar {
        println!("  Game: Java {}", j.id);
    }
    let report = modset::analyze_set(dir, jar.as_ref());
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

fn run_repair(args: &[String]) -> i32 {
    use kebitx_repair::{Change, Fix};
    if args.first().map(String::as_str) == Some("--undo") {
        let Some(backup) = args.get(1) else {
            eprintln!("{USAGE}");
            return 2;
        };
        return match kebitx_repair::restore(Path::new(backup)) {
            Ok(n) => {
                println!("  restored {n} file(s) from {backup}");
                0
            }
            Err(e) => {
                eprintln!("  {e}");
                1
            }
        };
    }
    if let Some(dir) = args
        .iter()
        .find(|a| !a.starts_with('-') && kebitx_java::is_mod_folder(Path::new(a.as_str())))
    {
        return run_java_repair(Path::new(dir), args);
    }
    let mut path = None;
    let mut fixes = vec![Fix::Case];
    let mut do_apply = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply" => do_apply = true,
            "--fix" => {
                i += 1;
                let parsed: Option<Vec<Fix>> = args
                    .get(i)
                    .map(|s| s.split(',').map(Fix::parse).collect())
                    .unwrap_or(None);
                match parsed {
                    Some(f) if !f.is_empty() => fixes = f,
                    _ => {
                        eprintln!("--fix takes case, unused, or both: --fix case,unused\n{USAGE}");
                        return 2;
                    }
                }
            }
            other if !other.starts_with('-') => path = Some(other.to_string()),
            other => {
                eprintln!("unknown option {other}\n{USAGE}");
                return 2;
            }
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("{USAGE}");
        return 2;
    };
    let root = Path::new(&path);
    if !root.is_dir() {
        eprintln!("not a folder: {path}");
        return 2;
    }
    let catalog = VanillaCatalog::discover().unwrap_or_else(VanillaCatalog::empty);
    if fixes.contains(&Fix::Unused) && catalog.is_empty() {
        // without the catalog, a pack's replacement of a game file looks unused
        eprintln!("  --fix unused needs the game catalog (Minecraft installed): refusing to delete without it");
        return 2;
    }
    println!("Kebitx Repair — {path}");
    let plan = kebitx_repair::plan(root, &catalog, &fixes);
    if plan.changes.is_empty() {
        println!("  nothing to repair");
        return 0;
    }
    let (mut edits, mut deletes, mut bytes) = (0, 0, 0u64);
    for c in &plan.changes {
        let rel = c
            .file()
            .strip_prefix(root)
            .unwrap_or(c.file())
            .display()
            .to_string();
        match c {
            Change::Edit {
                old,
                new,
                occurrences,
                reason,
                ..
            } => {
                edits += 1;
                println!("  EDIT    {rel}: {old} -> {new} ({occurrences}x)");
                println!("          {reason}");
            }
            Change::Delete {
                bytes: b, reason, ..
            } => {
                deletes += 1;
                bytes += b;
                println!("  DELETE  {rel} ({:.1} KB): {reason}", *b as f64 / 1024.0);
            }
        }
    }
    println!();
    println!(
        "  {edits} edit(s), {deletes} deletion(s) ({:.2} MB)",
        bytes as f64 / 1048576.0
    );
    if !do_apply {
        println!("  preview only: nothing was written. Run again with --apply to repair.");
        return 0;
    }
    match kebitx_repair::apply(root, &plan, &catalog) {
        Ok(r) => {
            let s = |s: &kebitx_repair::Score| {
                format!(
                    "{} broken, {} case, {} compat errors, {} unused",
                    s.broken, s.case_mismatches, s.compat_errors, s.unused_assets
                )
            };
            println!("  before: {}", s(&r.before));
            println!("  after:  {}", s(&r.after));
            if r.kept {
                println!("  KEPT: strictly better. Backup: {}", r.backup.display());
                println!(
                    "  undo with: kebitx repair --undo \"{}\"",
                    r.backup.display()
                );
                0
            } else {
                println!("  ROLLED BACK: not strictly better; every file is as it was");
                1
            }
        }
        Err(e) => {
            eprintln!("  {e}");
            1
        }
    }
}

fn print_steps(r: &kebitx_build::BuildReport) {
    println!("  copied {} files to {}", r.files, r.work.display());
    for (name, t) in &r.filters {
        println!("  filter {name}: ok ({} ms)", t.as_millis());
    }
    if r.checked {
        println!(
            "  check: {} broken reference(s), {} compatibility error(s)",
            r.broken, r.compat_errors
        );
    }
}

fn print_built(r: &kebitx_build::BuildReport) {
    print_steps(r);
    for p in &r.exported {
        println!("  exported {}", p.display());
    }
    println!("  OK");
}

fn print_failed(e: &kebitx_build::BuildError) {
    print_steps(&e.report);
    eprintln!("  BUILD FAILED at {}: {}", e.stage, e.message);
}

/// Why a model or texture looks wrong. Warnings: they explain a visual
/// defect, they do not stop the game from loading anything.
fn print_graphics(g: &graphics::GraphicsReport, show_all: bool) {
    let limit = if show_all { usize::MAX } else { 10 };
    println!("  Graphics:");
    println!(
        "    {} models ({} declare no canvas, skipped), {} cubes; {} texture/model pairs checked",
        g.models, g.models_without_canvas, g.cubes, g.pairs_checked
    );
    println!("    {} entity animations compared bone by bone ({} entities skipped: a model of theirs is not in the packs)",
             g.animations_checked, g.entities_unknown_bones);
    println!(
        "    {} move some bones the model lacks (normal for shared animations, not listed)",
        g.animations_partial
    );
    let count = |tag: &str| g.issues.iter().filter(|i| graphics_tag(i) == tag).count();
    println!(
        "    UV outside the canvas: {};  stretched textures: {};  animations that move nothing: {}",
        count("UV"),
        count("STRETCH"),
        count("BONES")
    );
    println!("    bones hanging from nothing: {};  repeated bone names: {};  inverted cubes: {};  compiled shaders: {}",
             count("PARENT"), count("DUPBONE"), count("INVERT"), count("SHADER"));
    println!(
        "    information: empty cubes {}, extreme animation scales {}, Vibrant Visuals packs {}",
        count("EMPTY"),
        count("SCALE"),
        count("VIBRANT")
    );
    // defects first, then what only explains a look
    let mut issues: Vec<&GraphicsIssue> = g.issues.iter().collect();
    issues.sort_by_key(|i| i.is_info());
    for i in issues.iter().take(limit) {
        let tag = graphics_tag(i);
        match i {
            GraphicsIssue::UvOutside { geometry, file, cubes, worst, bone } =>
                println!("    {tag:8} {geometry}: {cubes} cube(s) outside, worst {worst:.1} texels (bone '{bone}')  ({file})"),
            GraphicsIssue::Stretched { geometry, texture, declared, image, entities } =>
                println!("    {tag:8} {texture} is {}x{}, {geometry} expects {}:{}  (used by {})",
                         image.0, image.1, declared[0], declared[1], entities.join(", ")),
            GraphicsIssue::AnimationBones { entity, animation, missing } =>
                println!("    {tag:8} {entity}: {animation} does nothing — the model has none of: {}", missing.join(", ")),
            GraphicsIssue::BoneParentMissing { geometry, file, bone, parent } =>
                println!("    {tag:8} {geometry}: bone '{bone}' hangs from '{parent}', which the model does not have  ({file})"),
            GraphicsIssue::DuplicateBones { geometry, file, bones } =>
                println!("    {tag:8} {geometry}: more than one bone named {}  ({file})", bones.join(", ")),
            GraphicsIssue::InvertedCubes { geometry, file, cubes, bone } =>
                println!("    {tag:8} {geometry}: {cubes} cube(s) inside out (first in bone '{bone}')  ({file})"),
            GraphicsIssue::EmptyCubes { geometry, file, cubes } =>
                println!("    {tag:8} {geometry}: {cubes} cube(s) of size 0  ({file})"),
            GraphicsIssue::AnimationScale { animation, file, bone, value } =>
                println!("    {tag:8} {animation}: bone '{bone}' scaled to [{}, {}, {}]  ({file})", value[0], value[1], value[2]),
            GraphicsIssue::ShaderNeedsModifiedClient { pack, files } =>
                println!("    {tag:8} {pack}: {files} compiled shader file(s) (*.material.bin)"),
            GraphicsIssue::VibrantVisuals { pack, parts } =>
                println!("    {tag:8} {pack}: Vibrant Visuals content ({})", parts.join(", ")),
        }
        print_explanation(&explain::graphics(i));
    }
    if issues.len() > limit {
        println!("    ... and {} more (use --all)", issues.len() - limit);
    }
    println!();
}

fn graphics_tag(i: &GraphicsIssue) -> &'static str {
    match i {
        GraphicsIssue::UvOutside { .. } => "UV",
        GraphicsIssue::Stretched { .. } => "STRETCH",
        GraphicsIssue::AnimationBones { .. } => "BONES",
        GraphicsIssue::BoneParentMissing { .. } => "PARENT",
        GraphicsIssue::DuplicateBones { .. } => "DUPBONE",
        GraphicsIssue::InvertedCubes { .. } => "INVERT",
        GraphicsIssue::EmptyCubes { .. } => "EMPTY",
        GraphicsIssue::AnimationScale { .. } => "SCALE",
        GraphicsIssue::ShaderNeedsModifiedClient { .. } => "SHADER",
        GraphicsIssue::VibrantVisuals { .. } => "VIBRANT",
    }
}

/// The second line of every listed finding: confidence and a fix.
fn print_explanation(e: &explain::Explanation) {
    println!("               [{}] {}", e.confidence.name(), e.fix);
}

/// Unused and duplicated textures/sounds. Information for optimisation (size
/// matters on phones); never part of the exit code.
fn print_assets(e: &Extras, no_catalog: bool, show_all: bool) {
    let limit = if show_all { usize::MAX } else { 10 };
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);

    let u = &e.unused;
    println!("  Unused assets (textures and sounds nothing references):");
    println!(
        "    scanned {} files; {} used by the game itself (replacements, engine paths)",
        u.scanned, u.replace_game
    );
    println!(
        "    unused: {} files, {:.2} MB",
        u.unused.len(),
        mb(u.bytes())
    );
    if no_catalog {
        println!("    note: without the game catalog, replacements of game files look unused");
    }
    for a in u.unused.iter().take(limit) {
        println!(
            "    {:>9.1} KB  {}/{}",
            a.bytes as f64 / 1024.0,
            a.pack,
            a.path
        );
    }
    if u.unused.len() > limit {
        println!("    ... and {} more (use --all)", u.unused.len() - limit);
    }
    println!();

    let defs = &e.defs;
    println!("  Unused definitions (geometry, animations, controllers nothing names):");
    let count = |k: DefKind| defs.unused.iter().filter(|u| u.kind == k).count();
    println!("    {} defined; unused: {} geometry, {} animations, {} animation controllers, {} render controllers",
             defs.defined, count(DefKind::Geometry), count(DefKind::Animation),
             count(DefKind::AnimationController), count(DefKind::RenderController));
    for u in defs.unused.iter().take(limit) {
        println!("    {:20} {}  ({})", u.kind.name(), u.id, u.file);
    }
    if defs.unused.len() > limit {
        println!("    ... and {} more (use --all)", defs.unused.len() - limit);
    }
    println!();

    let d = &e.dups;
    let wasted: u64 = d.iter().map(|g| g.wasted()).sum();
    println!("  Identical files (same bytes; never merged automatically):");
    println!(
        "    {} groups, {:.2} MB in extra copies",
        d.len(),
        mb(wasted)
    );
    for g in d.iter().take(limit) {
        println!(
            "    {:>9.1} KB x{}  {}",
            g.bytes as f64 / 1024.0,
            g.files.len(),
            g.files.join(", ")
        );
    }
    if d.len() > limit {
        println!("    ... and {} more (use --all)", d.len() - limit);
    }
    println!();
}

fn print_compat(c: &CompatReport, show_all: bool) {
    println!(
        "  Compatibility ({} packs, behavior packs included):",
        c.packs
    );
    println!(
        "    errors:   {}  (the game refuses or drops something)",
        c.errors()
    );
    println!(
        "    warnings: {}  (two different versions, the game silently keeps one)",
        c.warnings()
    );
    // errors first: they are what stops a world from working
    let mut issues: Vec<&CompatIssue> = c.issues.iter().collect();
    issues.sort_by_key(|i| i.severity() != Severity::Error);
    let limit = if show_all { usize::MAX } else { 12 };
    let v = |v: &[u32; 3]| format!("{}.{}.{}", v[0], v[1], v[2]);
    for i in issues.iter().take(limit) {
        match i {
            CompatIssue::DuplicateUuid { uuid, places } =>
                println!("    ERROR  uuid {uuid} used by: {}", places.join(", ")),
            CompatIssue::EngineTooNew { pack, needs, game } =>
                println!("    ERROR  {pack} needs Minecraft {} (installed: {})", v(needs), v(game)),
            CompatIssue::MissingDependency { pack, uuid } =>
                println!("    ERROR  {pack} depends on pack {uuid}, not in the world"),
            CompatIssue::MissingListedPack { list, uuid } =>
                println!("    ERROR  {list} activates {uuid}, not in the world"),
            CompatIssue::NotActivated { pack } =>
                println!("    WARN   {pack} is in the world but not activated"),
            CompatIssue::ConflictingFile { path, packs } =>
                println!("    WARN   {path} differs in: {}", packs.join(", ")),
            CompatIssue::ConflictingDefinition { kind, id, places } =>
                println!("    WARN   {kind} {id} defined differently in: {}", places.join(", ")),
            CompatIssue::InvalidModuleVersion { pack, module, version } =>
                println!("    ERROR  {pack} requires {module} version '{version}', not a valid version"),
            CompatIssue::ConflictingModuleVersions { pack, module, versions } =>
                println!("    ERROR  {pack} requires {module} twice: {}", versions.join(" and ")),
            CompatIssue::BetaApisOff { pack, module, version } =>
                println!("    ERROR  {pack} uses {module} {version}; the world has Beta APIs off, its scripts will not run"),
            CompatIssue::NeedsBetaApis { pack, module, version } =>
                println!("    WARN   {pack} uses {module} {version}: needs the Beta APIs experiment on"),
        }
        print_explanation(&explain::compat(i));
    }
    if issues.len() > limit {
        println!("    ... and {} more (use --all)", issues.len() - limit);
    }
    println!();
}

/// Which script modules the packs use, and at which versions. Kebitx cannot
/// read which versions the installed game supports, so it shows them instead.
fn print_modules(inv: &ModuleInventory) {
    if inv.is_empty() {
        return;
    }
    println!("  Script modules used (compare with what your game version supports):");
    for (module, versions) in inv {
        for (version, packs) in versions {
            println!("    {module} {version}: {}", packs.join(", "));
        }
    }
    println!();
}

fn print_catalog(c: &VanillaCatalog) {
    if c.is_empty() {
        println!("  Vanilla catalog:     NOT FOUND — every vanilla reference will");
        println!("                       look missing. Install not detected.");
    } else {
        println!(
            "  Vanilla catalog:     Minecraft {} — {} textures, {} geometries",
            c.version,
            c.texture_count(),
            c.geometry_count()
        );
        println!("                       {} animations, {} anim controllers, {} render controllers, {} materials",
                 c.animation_count(), c.animation_controller_count(), c.render_controller_count(), c.material_count());
        println!(
            "                       from {} bundles ({} unreadable){}",
            c.bundles_read,
            c.bundles_bad,
            if c.from_cache {
                ", cached (KEBITX_NO_CACHE=1 to rebuild)"
            } else {
                ""
            }
        );
    }
    println!();
}

fn print_report(r: &AnalysisReport) {
    println!("  Entities:            {}", r.entities);
    println!();
    println!("  Textures declared:   {}", r.textures_declared);
    println!("  Textures resolved:   {}", r.textures_resolved);
    println!(
        "  Textures external:   {}  (the game provides them)",
        r.textures_external
    );
    println!("  Textures missing:    {}", r.textures_missing);
    println!();
    println!("  Geometry declared:   {}", r.geometry_declared);
    println!("  Geometry resolved:   {}", r.geometry_resolved);
    println!(
        "  Geometry external:   {}  (the game provides them)",
        r.geometry_external
    );
    println!("  Geometry missing:    {}", r.geometry_missing);
    println!();
    println!(
        "  Animations declared: {}  (animations + animation controllers)",
        r.animations_declared
    );
    println!("  Animations resolved: {}", r.animations_resolved);
    println!("  Animations external: {}", r.animations_external);
    println!("  Animations missing:  {}", r.animations_missing);
    println!();
    println!("  Render ctrl declared: {}", r.render_controllers_declared);
    println!("  Render ctrl resolved: {}", r.render_controllers_resolved);
    println!("  Render ctrl external: {}", r.render_controllers_external);
    println!("  Render ctrl missing:  {}", r.render_controllers_missing);
    println!();
    println!("  Materials declared:  {}", r.materials_declared);
    println!(
        "  Materials resolved:  {}  (a pack's materials/*.material)",
        r.materials_resolved
    );
    println!(
        "  Materials external:  {}  (the game provides them)",
        r.materials_external
    );
    println!("  Materials missing:   {}", r.materials_missing);
    if r.materials_unchecked > 0 {
        println!(
            "  Materials unchecked: {}  (no game catalog: not provable)",
            r.materials_unchecked
        );
    }
    println!();
    println!(
        "  Slots checked:       {}  (Texture./Geometry./Material. a controller asks for)",
        r.slots_checked
    );
    println!("  Slots resolved:      {}", r.slots_resolved);
    println!(
        "  Slots bounded:       {}  (array; the runtime picks, some candidates exist)",
        r.slots_bounded
    );
    println!(
        "  Slots opaque:        {}  (Molang expression; not evaluated)",
        r.slots_opaque
    );
    println!("  Slots missing:       {}", r.slots_missing);
    println!();
    println!(
        "  Atlas entries:       {}  ({} texture paths)",
        r.atlas_entries, r.atlas_paths
    );
    println!("  Atlas resolved:      {}", r.atlas_resolved);
    println!("  Atlas external:      {}", r.atlas_external);
    println!("  Atlas missing:       {}", r.atlas_missing);
    println!(
        "  Atlas EMPTY:         {}  (a shortname with no texture: pink, guaranteed)",
        r.atlas_empty
    );
    println!();
    println!(
        "  Case mismatches:     {}  (work on Windows, pink on Android/iOS)",
        r.case_mismatches
    );
    println!(
        "  Unreadable files:    {}  (not analysed, so not counted as broken)",
        r.unreadable
    );
    println!();
}

fn print_issues(r: &AnalysisReport, show_all: bool) {
    if r.issues.is_empty() {
        return;
    }
    let limit = if show_all { usize::MAX } else { 12 };
    println!("  Issues:");
    for i in r.issues.iter().take(limit) {
        match i {
            Issue::Missing {
                kind,
                owner,
                reference,
            } => println!("    MISSING  {kind:20} {owner} -> {reference}"),
            Issue::CaseMismatch {
                owner,
                reference,
                on_disk,
            } => println!("    CASE     {owner}: '{reference}' is on disk as '{on_disk}'"),
            Issue::EmptyTextureSet { file, shortname } => {
                println!("    EMPTY    {file}: '{shortname}' declares no texture")
            }
            Issue::Unreadable { file, why } => println!("    UNREAD   {file}: {why}"),
        }
        print_explanation(&explain::issue(i));
    }
    if r.issues.len() > limit {
        println!("    ... and {} more (use --all)", r.issues.len() - limit);
    }
    println!();
}

fn summary(r: &AnalysisReport) -> usize {
    let broken = r.broken();
    if broken == 0 && r.case_mismatches == 0 {
        println!("  OK — no broken references.");
    } else {
        if broken > 0 {
            println!("  {broken} broken reference(s) (KnownMissing or empty set).");
        }
        if r.case_mismatches > 0 {
            println!(
                "  {} case mismatch(es): fine on Windows, broken on Android/iOS.",
                r.case_mismatches
            );
        }
    }
    // Case mismatches fail the run too: the world is played from phones.
    broken + r.case_mismatches
}

/// module -> version -> packs, as `compat::module_inventory` returns it.
type ModuleInventory =
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>>;

/// One finding of a Java pack or mod, as a report line.
fn java_line(f: &kebitx_java::Finding) -> String {
    use kebitx_java::Finding;
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
fn code_line(f: &kebitx_java::code::CodeFinding) -> String {
    use kebitx_java::code::CodeFinding;
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
