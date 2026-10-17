use crate::report::AnalysisReport;
use std::path::Path;

/// Orchestrates full-entity resolution: shortnames -> arrays -> opaque indices.
/// It is the first CONSUMER of the indexes, not the starting point.
pub fn resolve_entity(_rp_root: &Path, _entity_file: &Path, _report: &mut AnalysisReport) {
    todo!("resolve identifier, animations, animation_controllers, render_controllers, spawn_egg")
}
