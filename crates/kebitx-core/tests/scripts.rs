//! Script-module dependencies, on `world_scripts`: a `level.dat` with the Beta
//! APIs experiment off, a pack that uses a beta module, and a pack with an
//! invalid version and one module required twice.

mod common;
use common::fixture;
use kebitx_core::compat::{check_world, module_inventory, CompatIssue, Severity};

#[test]
fn what_the_files_prove_about_script_modules() {
    let r = check_world(&fixture("world_scripts"), None);
    let i = &r.issues;
    assert!(i.contains(&CompatIssue::BetaApisOff {
        pack: "bp_beta".into(),
        module: "@minecraft/server".into(),
        version: "1.12.0-beta".into(),
    }));
    assert!(i.contains(&CompatIssue::InvalidModuleVersion {
        pack: "bp_bad".into(),
        module: "@minecraft/server".into(),
        version: "1.2".into(),
    }));
    assert!(i.contains(&CompatIssue::ConflictingModuleVersions {
        pack: "bp_bad".into(),
        module: "@minecraft/server-ui".into(),
        versions: vec!["1.3.0".into(), "1.2.0".into()],
    }));
    // `[0, 1, 0]` is a valid legacy version
    assert_eq!(r.errors(), 3, "{i:#?}");
    assert_eq!(r.warnings(), 0, "{i:#?}");
}

#[test]
fn without_level_dat_a_beta_module_is_only_a_warning() {
    // the packs folder on its own: same packs, no level.dat
    let r = check_world(&fixture("world_scripts/behavior_packs"), None);
    let needs: Vec<&CompatIssue> = r
        .issues
        .iter()
        .filter(|x| x.severity() == Severity::Warning)
        .collect();
    assert_eq!(
        needs,
        [&CompatIssue::NeedsBetaApis {
            pack: "bp_beta".into(),
            module: "@minecraft/server".into(),
            version: "1.12.0-beta".into(),
        }]
    );
}

#[test]
fn inventory_lists_every_module_version() {
    let inv = module_inventory(&fixture("world_scripts"));
    assert_eq!(inv["@minecraft/server"]["1.12.0-beta"], ["bp_beta"]);
    assert_eq!(inv["@minecraft/server-gametest"]["0.1.0"], ["bp_bad"]);
    assert_eq!(inv["@minecraft/server-ui"].len(), 2);
}
