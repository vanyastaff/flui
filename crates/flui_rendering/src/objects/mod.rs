//! Render objects implementing Flutter's layout system.
//!
//! This module provides concrete render object implementations organized
//! by protocol (Box vs Sliver) and functional category.
//!
//! # Module Structure
//!
//! - [`box`]: Box protocol render objects (2D cartesian layout)
//!   - `basic`: Simple single-child modifications (Padding, Align, etc.)
//!   - `effects`: Visual effects (Opacity, Transform, Clip, etc.)
//!   - `layout`: Multi-child layouts (Flex, Stack, Wrap, etc.)
//!
//! - [`sliver`]: Sliver protocol render objects (scrollable content)
//!   - `basic`: Simple sliver modifications
//!   - `layout`: Multi-child sliver layouts (List, Grid, etc.)
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::prelude::*;
//! use flui_types::EdgeInsets;
//!
//! let padding = RenderPadding::new(EdgeInsets::all(16.0));
//! ```

pub mod r#box;
pub mod sliver;

// Re-export all box render objects for convenience
pub use r#box::basic::*;
pub use r#box::effects::*;
pub use r#box::layout::*;
pub use r#box::scroll::*;
pub use r#box::text::*;

// Re-export sliver render objects
pub use sliver::*;
