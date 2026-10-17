//! The Semantic Graph: nodes, edges, and each edge's state (the engine's core).
pub mod edge;
pub mod edge_state;
pub mod ids;
pub mod node;

pub use edge::{Edge, EdgeKind};
pub use edge_state::{EdgeState, IndexSource};
pub use ids::*;
pub use node::Node;
