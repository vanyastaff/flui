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
//! - [`RenderConstrainedBox`] - Applies extra constraints to child (Core.2)
//! - [`RenderLimitedBox`] - Caps unbounded constraints (Core.2)
//! - [`RenderAspectRatio`] - Sizes child by aspect ratio (Core.2)
//! - [`RenderFractionallySizedBox`] - Sizes child as fraction of parent (Core.2)
//! - [`RenderClipRect`] / [`RenderClipRRect`] / [`RenderClipOval`] /
//!   [`RenderClipPath`] — generic [`RenderClip<S>`] family (Core.2)
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

mod aspect_ratio;
mod center;
mod clip;
mod colored_box;
mod constrained_box;
mod flex;
mod fractionally_sized_box;
mod limited_box;
mod opacity;
mod padding;
mod sized_box;
mod transform;

pub use aspect_ratio::{AspectRatio, RenderAspectRatio};
pub use center::RenderCenter;
pub use clip::{
    ClipGeometry, CustomClipper, Oval, RenderClip, RenderClipOval, RenderClipPath, RenderClipRRect,
    RenderClipRect,
};
pub use colored_box::RenderColoredBox;
pub use constrained_box::RenderConstrainedBox;
pub use flex::{CrossAxisAlignment, FlexDirection, MainAxisAlignment, RenderFlex};
pub use fractionally_sized_box::{FractionFactor, RenderFractionallySizedBox};
pub use limited_box::RenderLimitedBox;
pub use opacity::RenderOpacity;
pub use padding::RenderPadding;
pub use sized_box::RenderSizedBox;
pub use transform::RenderTransform;
