use super::ids::{MolangVarId, QueryId};

/// Horse discovery: `MolangVariable` was a special case. The real concept is an opaque index
/// whose SOURCE may be internal (villager) or engine-provided (horse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexSource {
    ComputedVariable { variable: MolangVarId },
    EngineQuery { query: QueryId },
}

/// State of EVERY edge. Engine core. See the invariant in `crate::findings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeState {
    Resolved,
    KnownMissing,
    External,
    BoundedOpaque { index: IndexSource },
    UnboundedOpaque,
}
