//! Trait definitions for render objects.
//!
//! This module defines the core render object traits:
//! - `RenderObject` - Base trait for all render objects (dynamic, stored in RenderTree)
//! - `RenderBox` - 2D box layout with Arity-based child management
//! - `RenderSliver` - Scrollable content layout

mod render_box;
mod render_object;
mod render_sliver;

pub use render_box::*;
pub use render_object::*;
pub use render_sliver::*;
