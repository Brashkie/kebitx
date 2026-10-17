//! Shared helpers for the integration tests.

use std::path::PathBuf;

/// A fixture from the workspace-level `fixtures/` folder. Anchored to this
/// crate's manifest, so the tests pass whatever directory cargo runs them from.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}
