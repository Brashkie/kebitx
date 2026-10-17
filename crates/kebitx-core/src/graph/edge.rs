use super::edge_state::EdgeState;
use super::node::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    PairsWith,
    DeclaresShortname,
    ResolvesTo,
    Uses,
    ReferencesShortname,
    MemberOf,
    Defines,
    Reads,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: Node,
    pub to: Node,
    pub kind: EdgeKind,
    pub state: EdgeState,
}
