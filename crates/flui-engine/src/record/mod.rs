//! The record side of the painter: the command IR a frame is recorded into,
//! the batchers and instance types that fill it, the state stacks and clips
//! that stamp it, and the caches that let an unchanged chunk keep its IR.

pub(crate) mod batches;
pub(crate) mod budget;
pub(crate) mod cache;
pub(crate) mod clip_chain;
pub(crate) mod clip_geometry;
pub(crate) mod command_ir;
pub(crate) mod effects;
pub(crate) mod instancing;
pub(crate) mod path_cache;
pub(crate) mod pixel_shift;
mod recency;
pub(crate) mod state_stack;
mod tallied_map;
mod tessellator;
pub(crate) mod vertex;
