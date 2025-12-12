//! Trait definitions for render objects

mod render_object;
pub mod r#box;
pub mod sliver;

pub use render_object::{RenderObject, RenderObjectExt};
pub use r#box::*;
pub use sliver::*;
