//! kebitx-build — builds add-ons.
//!
//! A project file (`kebitx.json`) names the packs and a list of filters. A
//! build:
//!
//! 1. copies the packs to a scratch folder (`.kebitx/tmp`) — the source is
//!    never modified;
//! 2. runs the filters on that copy, in order; a failing filter stops it;
//! 3. checks the result with the analysis engine (`kebitx-core`); errors stop
//!    it before anything is exported;
//! 4. exports: a local `build/` folder, the game's development pack folders,
//!    or a `.mcaddon` archive.
//!
//! Exporting only ever replaces a folder that a build created (it carries a
//! marker file). Anything else at the target is refused, never deleted.
#![forbid(unsafe_code)]

pub mod config;
pub mod fsutil;
pub mod mojang;
pub mod pipeline;
pub mod watch;
pub mod zip;

pub use config::{Filter, Project, Target};
pub use pipeline::{build, BuildError, BuildOptions, BuildReport};
