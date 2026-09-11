//! Trait definitions for render objects.
//!
//! This module defines the core render object traits:
//! - `RenderObject` - Base trait for all render objects (dynamic, stored in
//!   RenderTree)
//! - `RenderBox` - 2D box layout with Arity-based child management
//! - `RenderSliver` - Scrollable content layout

mod paint_effects;
mod render_box;
mod render_object;
mod render_sliver;

pub use paint_effects::{PaintClip, PaintEffects, PaintOpacity, resolve_path_clip};
pub use render_box::*;
pub use render_object::*;
pub use render_sliver::*;
