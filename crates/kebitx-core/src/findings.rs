use crate::graph::{Edge, EdgeState, Node};

#[derive(Debug, Clone)]
pub enum Finding {
    BrokenReference {
        at: Node,
    },
    DeclaredButUnused {
        shortname: String,
    },
    /// Informational. NEVER auto-merge: shortnames are override points (villager base*).
    SharedBytes {
        shortnames: Vec<String>,
    },
}

impl Finding {
    /// The project's invariant, made a type: "broken" is born ONLY from `KnownMissing`.
    pub fn broken_reference(edge: &Edge) -> Option<Finding> {
        match edge.state {
            EdgeState::KnownMissing => Some(Finding::BrokenReference {
                at: edge.from.clone(),
            }),
            _ => None,
        }
    }
}
