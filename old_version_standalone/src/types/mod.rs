//! Type system for nebula-ui
//!
//! This module contains all type definitions organized by category:
//! - [`core`]: Geometric primitives, colors, transforms
//! - [`layout`]: Alignment, spacing, constraints
//! - [`styling`]: Decorations, borders, shadows, gradients
//! - [`typography`]: Font and text styling
//! - [`interaction`]: Animation curves and interpolation
//! - [`utility`]: Helper types

pub mod core;
pub mod interaction;
pub mod layout;
pub mod styling;
pub mod typography;
pub mod utility;

/// Prelude module for convenient imports of all commonly used types
///
/// This re-exports the preludes from all type category modules.
///
/// # Example
///
/// ```ignore
/// use nebula_ui::types::prelude::*;
///
/// // Now you have access to all commonly used types:
/// let color = Color::from_rgb(100, 150, 200);
/// let padding = EdgeInsets::all(10.0);
/// let decoration = BoxDecoration::new().with_color(color);
/// ```
pub mod prelude {
    // Re-export all type preludes
    pub use super::core::prelude::*;
    pub use super::layout::prelude::*;
    pub use super::styling::prelude::*;
}











































































































