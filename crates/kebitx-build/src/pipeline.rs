//! copy → filters → check → export.
//!
//! The filter contract, for programs in any language:
//! - working directory: the scratch folder, which holds `BP/` and/or `RP/`;
//! - environment: `KEBITX_WORK`, `KEBITX_BP`, `KEBITX_RP` (only for packs the
//!   project has), `KEBITX_PROJECT` (the project root, read-only by
//!   convention), `KEBITX_NAME`;
//! - success is exit code 0; anything else stops the build.

use crate::config::{Project, Target};
use crate::fsutil::{copy_dir, fresh_owned_dir, replace_owned_dir};
use crate::mojang;
use crate::zip::write_zip;
use kebitx_core::analyze::analyze_world_with;
use kebitx_core::compat::check_world;
use kebitx_core::filesystem::all_files;
use kebitx_core::json::{self, Json, JsonMap};
use kebitx_core::manifest::{parse_version, version};
use kebitx_core::vanilla::VanillaCatalog;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Per-run overrides of the project file.
#[derive(Debug, Default, Clone)]
pub struct BuildOptions {
    pub target: Option<Target>,
    pub check: Option<bool>,
}

#[derive(Debug, Default)]
pub struct BuildReport {
    pub work: PathBuf,
    pub files: u64,
    pub filters: Vec<(String, Duration)>,
    pub checked: bool,
    pub broken: usize,
    pub compat_errors: usize,
    pub exported: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct BuildError {
    pub stage: &'static str,
    pub message: String,
    /// What had been done when it stopped; the scratch folder is kept.
    pub report: Box<BuildReport>,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.message)
    }
}

pub fn build(
    project: &Project,
    opts: &BuildOptions,
    catalog: &VanillaCatalog,
) -> Result<BuildReport, BuildError> {
    let mut report = BuildReport {
        work: project.root.join(".kebitx").join("tmp"),
        ..Default::default()
    };
    macro_rules! fail {
        ($stage:expr, $msg:expr) => {
            return Err(BuildError {
                stage: $stage,
                message: $msg,
                report: Box::new(report),
            })
        };
    }

    // ---- 1. copy: the source is never touched
    if let Err(e) = fresh_owned_dir(&report.work) {
        fail!("prepare", e);
    }
    for (src, sub) in [(&project.behavior, "BP"), (&project.resource, "RP")] {
        let Some(src) = src else { continue };
        match copy_dir(src, &report.work.join(sub)) {
            Ok(n) => report.files += n,
            Err(e) => fail!("copy", format!("{}: {e}", src.display())),
        }
    }

    // ---- 2. filters, in order
    for f in &project.filters {
        let started = Instant::now();
        let mut cmd = Command::new(program(&project.root, &f.run[0]));
        cmd.args(&f.run[1..])
            .current_dir(&report.work)
            .env("KEBITX_WORK", &report.work)
            .env("KEBITX_PROJECT", &project.root)
            .env("KEBITX_NAME", &project.name);
        for (var, sub) in [("KEBITX_BP", "BP"), ("KEBITX_RP", "RP")] {
            let dir = report.work.join(sub);
            if dir.is_dir() {
                cmd.env(var, dir);
            }
        }
        match cmd.status() {
            Ok(s) if s.success() => report.filters.push((f.name.clone(), started.elapsed())),
            Ok(s) => fail!("filter", format!("{} exited with {s}", f.name)),
            Err(e) => fail!(
                "filter",
                format!("{} could not start ({}): {e}", f.name, f.run[0])
            ),
        }
    }

    // ---- 3. the analysis as a gate
    if opts.check.unwrap_or(project.check) {
        report.checked = true;
        report.broken = analyze_world_with(&report.work, catalog)
            .iter()
            .map(|(_, r)| r.broken() + r.case_mismatches)
            .sum();
        let game = if catalog.is_empty() {
            None
        } else {
            parse_version(&catalog.version)
        };
        report.compat_errors = check_world(&report.work, game).errors();
        if report.broken + report.compat_errors > 0 {
            let msg = format!(
                "{} broken reference(s), {} compatibility error(s); nothing exported. \
                 Details: kebitx analyze \"{}\" --all",
                report.broken,
                report.compat_errors,
                report.work.display()
            );
            fail!("check", msg);
        }
    }

    // ---- 4. export
    let target = opts.target.unwrap_or(project.target);
    let packs: Vec<(&str, &str)> = [("BP", "behavior"), ("RP", "resource")]
        .into_iter()
        .filter(|(sub, _)| report.work.join(sub).is_dir())
        .collect();
    match target {
        Target::Local | Target::McAddon | Target::McPack | Target::McWorld => {
            let out = project.root.join("build");
            if let Err(e) = fs::create_dir_all(&out) {
                fail!("export", format!("{}: {e}", out.display()));
            }
            match target {
                Target::Local => {
                    for (sub, _) in &packs {
                        let dst = out.join(format!("{}_{sub}", project.name));
                        if let Err(e) = replace_owned_dir(&report.work.join(sub), &dst) {
                            fail!("export", e);
                        }
                        report.exported.push(dst);
                    }
                }
                Target::McAddon => {
                    let file = out.join(format!("{}.mcaddon", project.name));
                    let mut entries = Vec::new();
                    for (sub, _) in &packs {
                        entries.extend(tree(
                            &report.work.join(sub),
                            &format!("{}_{sub}/", project.name),
                        ));
                    }
                    entries.sort();
                    if let Err(e) = write_zip(&file, &entries) {
                        fail!("export", e);
                    }
                    report.exported.push(file);
                }
                Target::McPack => {
                    // one archive per pack, its files at the archive's root
                    for (sub, _) in &packs {
                        let file = out.join(format!("{}_{sub}.mcpack", project.name));
                        if let Err(e) = write_zip(&file, &tree(&report.work.join(sub), "")) {
                            fail!("export", e);
                        }
                        report.exported.push(file);
                    }
                }
                _ => match mcworld(project, &report.work, &packs, &out) {
                    Ok(file) => report.exported.push(file),
                    Err(e) => fail!("export", e),
                },
            }
        }
        Target::Development => {
            let Some(com) = project.com_mojang.clone().or_else(mojang::com_mojang) else {
                fail!(
                    "export",
                    "the game's com.mojang folder was not found; set export.com_mojang".into()
                );
            };
            for (sub, kind) in &packs {
                let dst = com
                    .join(format!("development_{kind}_packs"))
                    .join(format!("{}_{sub}", project.name));
                if let Err(e) = replace_owned_dir(&report.work.join(sub), &dst) {
                    fail!("export", e);
                }
                report.exported.push(dst);
            }
        }
    }
    Ok(report)
}

/// A filter program written as a path relative to the project
/// (`filters/x.exe`, `./x`) runs from the project; a bare name uses PATH.
fn program(root: &Path, prog: &str) -> PathBuf {
    let local = root.join(prog);
    let looks_like_path = prog.contains('/') || prog.contains('\\') || prog.starts_with('.');
    if looks_like_path && local.exists() {
        local
    } else {
        PathBuf::from(prog)
    }
}

/// Every file under `root` as an archive entry `prefix + relative path`,
/// sorted, `/` separators.
fn tree(root: &Path, prefix: &str) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = all_files(root)
        .into_iter()
        .filter_map(|f| {
            let rel = f
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            Some((format!("{prefix}{rel}"), f))
        })
        .collect();
    out.sort();
    out
}

/// `build/<name>.mcworld`: the world template, the packs under
/// `behavior_packs/` and `resource_packs/`, and the world's pack lists with
/// the packs activated. Entries of the template's lists for other packs are
/// kept; an older entry for the same pack is replaced.
fn mcworld(
    project: &Project,
    work: &Path,
    packs: &[(&str, &str)],
    out: &Path,
) -> Result<PathBuf, String> {
    let world = project
        .world
        .as_ref()
        .ok_or("the mcworld target needs export.world: a world folder with level.dat")?;
    if !world.join("level.dat").is_file() {
        return Err(format!(
            "{}: no level.dat; export.world must be a world folder",
            world.display()
        ));
    }
    let lists = [
        ("BP", "world_behavior_packs.json", "behavior_packs"),
        ("RP", "world_resource_packs.json", "resource_packs"),
    ];
    let ours: Vec<String> = packs
        .iter()
        .map(|(sub, _)| format!("{}_{sub}", project.name))
        .collect();

    // the template, minus what the build replaces: the lists, and any older
    // copy of these same packs
    let mut entries: Vec<(String, PathBuf)> = tree(world, "")
        .into_iter()
        .filter(|(name, _)| {
            !lists.iter().any(|(_, l, _)| name == l)
                && !ours.iter().any(|p| {
                    name.starts_with(&format!("behavior_packs/{p}/"))
                        || name.starts_with(&format!("resource_packs/{p}/"))
                })
        })
        .collect();

    for (sub, list, folder) in lists {
        if !packs.iter().any(|(s, _)| *s == sub) {
            // no such pack in the project: keep the template's list as it is
            if world.join(list).is_file() {
                entries.push((list.to_string(), world.join(list)));
            }
            continue;
        }
        let pack = work.join(sub);
        entries.extend(tree(&pack, &format!("{folder}/{}_{sub}/", project.name)));
        let (uuid, version) = pack_identity(&pack)?;
        let mut items: Vec<Json> = fs::read_to_string(world.join(list))
            .ok()
            .and_then(|s| json::parse(&s).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        items.retain(|e| {
            e.get("pack_id")
                .and_then(Json::as_str)
                .map(|id| !id.eq_ignore_ascii_case(&uuid))
                .unwrap_or(true)
        });
        items.push(Json::Obj(JsonMap::from_entries(vec![
            ("pack_id".into(), Json::Str(uuid)),
            (
                "version".into(),
                Json::Arr(version.iter().map(|&n| Json::Num(n as f64)).collect()),
            ),
        ])));
        // written into the scratch folder, archived from there
        let generated = work.join(format!(".{list}"));
        fs::write(&generated, json::to_string(&Json::Arr(items)))
            .map_err(|e| format!("{}: {e}", generated.display()))?;
        entries.push((list.to_string(), generated));
    }
    entries.sort();
    let file = out.join(format!("{}.mcworld", project.name));
    write_zip(&file, &entries)?;
    Ok(file)
}

/// A built pack's header UUID and version, as a world list needs them.
fn pack_identity(pack: &Path) -> Result<(String, [u32; 3]), String> {
    let file = pack.join("manifest.json");
    let src = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let root = json::parse(&src).map_err(|e| format!("{}: {}", file.display(), e.msg))?;
    let header = root
        .get("header")
        .ok_or_else(|| format!("{}: no header", file.display()))?;
    let uuid = header
        .get("uuid")
        .and_then(Json::as_str)
        .ok_or_else(|| format!("{}: no header.uuid", file.display()))?
        .trim()
        .to_ascii_lowercase();
    let version = header.get("version").and_then(version).unwrap_or([1, 0, 0]);
    Ok((uuid, version))
}
