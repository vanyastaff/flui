//! Trait definitions for render objects.
//!
//! This module defines the core render object traits:
//! - `Renderable` - Base trait for protocol-based render objects
//! - `RenderObject` - Base trait for all render objects (implemented by Wrapper)
//! - `RenderBox` - 2D box layout with Arity-based child management
//! - `RenderSliver` - Scrollable content layout

mod render_box;
mod render_object;
mod render_sliver;
mod renderable;

// Re-export at traits level
pub use render_box::*;
pub use render_object::*;
pub use render_sliver::*;
pub use renderable::*;
