//! Java Edition as a target of its own (Phase 7).
//!
//! `game`: the installed Java game (its launcher folder, asset index and
//! version `.jar`s), the reference every verdict is proven against.
//! `source`: what is analysed — a folder, or a `.jar` / `.zip` read in place.
//! `analyze`: static analysis of resource packs and mods.

pub mod analyze;
pub mod class;
pub mod code;
pub mod game;
pub mod mixin;
pub mod modset;
pub mod repair;
pub mod source;
pub mod version;

pub use analyze::{analyze, Finding, JavaReport, Kind};
pub use game::{JavaAssets, JavaJar};
pub use source::Source;

use std::path::Path;

/// A `mods/` folder: `.jar` files and no pack or mod metadata of its own.
pub fn is_mod_folder(path: &Path) -> bool {
    path.is_dir() && !is_java(path) && !modset::jars_in(path).is_empty()
}

/// Is this a Java resource pack or mod (and not a Bedrock pack or world)?
/// A `.jar` / `.zip`, or a folder with `pack.mcmeta` or a mod loader's
/// metadata.
pub fn is_java(path: &Path) -> bool {
    if path.is_file() {
        return path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("jar") || e.eq_ignore_ascii_case("zip"));
    }
    [
        "pack.mcmeta",
        "fabric.mod.json",
        "quilt.mod.json",
        "META-INF/mods.toml",
        "META-INF/neoforge.mods.toml",
    ]
    .iter()
    .any(|f| path.join(f).is_file())
}
