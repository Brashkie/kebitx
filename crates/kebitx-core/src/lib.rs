//! kebitx-core — semantic resolution engine for Bedrock.
//!
//! Derived from real analyses of `villager_v2` and `horse`. Std-only, no dependencies.
//!
//! Philosophy (one line): "Precision is the product's only currency."
//! As an invariant: whatever the resolver does not understand SUPPRESSES findings; it never
//! downgrades them to a percentage. Only `EdgeState::KnownMissing` may say "broken".
//!
//! Every file kebitx reads is untrusted (downloaded add-ons, game bundles), so
//! the crate forbids `unsafe` outright: a malformed input can at worst produce
//! an `Err`, never memory corruption. The C ABI lives in its own crate, `kebitx-ffi`.
#![forbid(unsafe_code)]

pub mod analyze;
pub mod codec;
pub mod compat;
pub mod explain;
pub mod filesystem;
pub mod findings;
pub mod graph;
pub mod graphics;
pub mod image;
pub mod json;
pub mod manifest;
pub mod nbt;
pub mod optimize;
pub mod par;
pub mod parser;
pub mod passes;
pub mod registry;
pub mod regression;
pub mod report;
pub mod resolver;
pub mod snapshot;
pub mod vanilla;
