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
//! - [`RenderAlign`] - Aligns child with configurable alignment
//!
//! ## Multi-Child Objects
//! - [`RenderFlex`] - Lays out children in a row or column
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::objects::{RenderColoredBox, RenderPadding};
//! use flui_rendering::wrapper::BoxWrapper;
//!
//! // Create a colored box
//! let box_obj = RenderColoredBox::new(Color::RED, Size::new(100.0, 50.0));
//! let wrapper = BoxWrapper::new(box_obj);
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
