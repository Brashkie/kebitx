use crate::findings::Finding;
use crate::graph::Edge;

/// Emit one finding per `KnownMissing` edge. Emitting "broken" from any other state is impossible
/// because `Finding::broken_reference` forbids it by type.
pub fn run(edges: &[Edge]) -> Vec<Finding> {
    edges.iter().filter_map(Finding::broken_reference).collect()
}
