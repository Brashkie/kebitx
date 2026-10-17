use crate::graph::GeometryId;
use crate::registry::geometry::GeometryRegistry;
use crate::vanilla::VanillaCatalog;

#[derive(Debug, PartialEq, Eq)]
pub enum GeometryResolution {
    Resolved(GeometryId),
    /// Provided by the game. Never a finding.
    External,
    KnownMissing,
}

/// Resolve an entity's geometry IDENTIFIER against the global index.
pub fn resolve_geometry(
    registry: &GeometryRegistry,
    catalog: &VanillaCatalog,
    identifier: &str,
) -> GeometryResolution {
    match registry.resolve(identifier) {
        Some(id) => GeometryResolution::Resolved(id),
        None if catalog.has_geometry(identifier) => GeometryResolution::External,
        None => GeometryResolution::KnownMissing,
    }
}
