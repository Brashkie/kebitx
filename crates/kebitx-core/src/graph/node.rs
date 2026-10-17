use super::edge_state::IndexSource;
use super::ids::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    ClientEntity(EntityId),
    ServerEntity(EntityId),
    Texture(TextureId),
    Geometry(GeometryId),
    Material(MaterialId),
    Animation(AnimationId),
    AnimationController(ControllerId),
    RenderController(ControllerId),
    OpaqueIndex(IndexSource),
}
