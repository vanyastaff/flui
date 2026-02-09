//! Concrete RenderBox implementations.
//!
//! This module provides ready-to-use render objects for common layout patterns:
//!
//! ## Leaf Objects (no children)
//! - [`RenderColoredBox`] - Paints a colored rectangle
//! - [`RenderSizedBox`] - Forces specific size constraints
//!
//! ## Single Child Objects
//! - [`RenderPadding`] - Adds padding around child
//! - [`RenderCenter`] - Centers child within available space
//! - [`RenderOpacity`] - Applies transparency to child
//! - [`RenderTransform`] - Applies transformation matrix to child
//!
//! ## Multi-Child Objects
//! - [`RenderFlex`] - Lays out children in a row or column
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::objects::RenderColoredBox;
//! use flui_rendering::traits::RenderBox;
//! use flui_types::{Color, Size};
//!
//! // Create a colored box render object
//! let colored_box = RenderColoredBox::new(Color::RED);
//! // Use with PipelineOwner for actual rendering
//! ```

mod center;
mod colored_box;
mod flex;
mod opacity;
mod padding;
mod sized_box;
mod transform;

pub use center::RenderCenter;
pub use colored_box::RenderColoredBox;
pub use flex::{CrossAxisAlignment, FlexDirection, MainAxisAlignment, RenderFlex};
pub use opacity::RenderOpacity;
pub use padding::RenderPadding;
pub use sized_box::RenderSizedBox;
pub use transform::RenderTransform;
