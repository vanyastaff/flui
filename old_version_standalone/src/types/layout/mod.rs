//! Layout and spacing types.
//!
//! This module contains types for controlling layout, alignment, and spacing:
//! - [`Alignment`]: Horizontal and vertical alignment within a container
//! - [`Axis`]: Horizontal or vertical axis direction
//! - [`AspectRatio`]: Width-to-height ratio constraints
//! - [`EdgeInsets`]: Padding or margins for all four edges
//! - [`Margin`]: External spacing around elements
//! - [`Padding`]: Internal spacing within elements
//! - [`Spacing`]: Standardized spacing scale (XXS to XXXL)
//! - [`FlexFit`], [`FlexDirection`], [`FlexWrap`]: Flexbox layout properties
//! - [`MainAxisAlignment`], [`CrossAxisAlignment`]: Flex alignment

pub mod alignment;
pub mod aspect_ratio;
pub mod axis;
pub mod box_constraints;
pub mod edge_insets;
pub mod flex;
pub mod margin;
pub mod padding;
pub mod spacing;


// Re-export types for convenience
pub use alignment::{Alignment, CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
pub use aspect_ratio::AspectRatio;
pub use axis::Axis;
pub use box_constraints::BoxConstraints;
pub use edge_insets::EdgeInsets;
pub use flex::{FlexDirection, FlexFit, FlexWrap};
pub use margin::Margin;
pub use padding::Padding;
pub use spacing::Spacing;

/// Prelude module for convenient imports of commonly used layout types
pub mod prelude {
    pub use super::{
        Alignment, EdgeInsets, BoxConstraints,
        Padding, Margin, Spacing,
        CrossAxisAlignment, MainAxisAlignment,
        Axis, FlexDirection, FlexFit, FlexWrap,
    };
}


















