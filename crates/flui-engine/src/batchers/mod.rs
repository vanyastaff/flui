//! Draw call batching and instance collection
//!
//! Collects primitives into batched draw calls for efficient GPU submission.
//! Each batcher handles one category of drawable primitive.

pub mod shapes;
pub mod paths;
pub mod text;
pub mod images;
pub mod effects;
pub mod compositing;
