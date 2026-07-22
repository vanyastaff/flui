//! Scene compositor: stack-based builder + retained-layer tracking.
//!
//! The original 967 LOC `compositor.rs` was split into:
//!
//! - [`builder`] -- `SceneBuilder<'a>` and its push/add/pop/build surface.
//! - [`retained`] -- `SceneCompositor` retained-layer registry.
//!
//! Integration-style tests live in `crates/flui-layer/tests/scene_builder.rs`.

mod builder;
mod retained;

pub use builder::SceneBuilder;
pub use retained::{CompositorStats, SceneCompositor};
