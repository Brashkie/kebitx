//! Analysis passes: assert ONLY what the evidence permits. `unused_asset` and
//! `duplicate_resource` are informational: they never make a pack "broken".
pub mod broken_reference;
pub mod duplicate_resource;
pub mod unused_asset;
pub mod unused_definition;
