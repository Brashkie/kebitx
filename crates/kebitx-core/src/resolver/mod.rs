//! Resolvers turn references into edges with an `EdgeState`. A resolver knows nothing about
//! variants or Molang; that logic lives in the opaque layer (see `graph::EdgeState`).
pub mod animation;
pub mod entity;
pub mod geometry;
pub mod render_controller;
pub mod texture;
