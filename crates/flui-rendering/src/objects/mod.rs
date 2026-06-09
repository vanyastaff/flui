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
//! - [`RenderRepaintBoundary`] - Pure proxy that creates a repaint boundary
//!   (Core.2)
//! - [`RenderOffstage`] - Hides subtree (zero-size layout, skip paint &
//!   hit-test) (Core.2 Wave 4)
//! - [`RenderAbsorbPointer`] - Catches pointers itself, blocks child
//!   (Core.2 Wave 4)
//! - [`RenderIgnorePointer`] - Pointers pass straight through subtree
//!   (Core.2 Wave 4)
//! - [`RenderMetaData`] - Attaches opaque metadata to hit-test entries
//!   (Core.2 Wave 4)
//! - [`RenderFractionalTranslation`] - Paint-time shift by fraction of
//!   child size (Core.2 Wave 4)
//! - [`RenderFittedBox`] - Scales child to fit via
//!   [`flui_types::layout::BoxFit`] + [`flui_types::Alignment`]
//!   (Core.2 Wave 4)
//!
//! ## Multi-Child Objects
//! - [`RenderFlex`] - Lays out children in a row or column
//! - [`RenderStack`] - Overlays children with positioned + non-positioned
//!   flows (Core.2 Wave 2a)
//!
//! ## Sliver Single-Child Objects
//! - [`RenderSliverPadding`] - Pads a single sliver child on all four
//!   sides, honouring the sliver layout protocol (Core.2 Wave 5a)
//! - [`RenderSliverOpacity`] - Applies transparency to a single sliver
//!   child via [`crate::traits::PaintEffectsCapability::paint_alpha`]
//!   (Core.2 Wave 5a)
//! - [`RenderSliverIgnorePointer`] - Pointers pass through the sliver
//!   subtree to siblings beneath in the viewport (Core.2 Wave 5a)
//! - [`RenderSliverOffstage`] - Hides a sliver subtree (zero geometry,
//!   skipped paint, no hit-test) (Core.2 Wave 5a)
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

mod absorb_pointer;
mod aspect_ratio;
mod center;
mod clip;
mod colored_box;
mod constrained_box;
mod fitted_box;
mod flex;
mod fractional_translation;
mod fractionally_sized_box;
mod ignore_pointer;
mod limited_box;
mod meta_data;
mod offstage;
mod opacity;
mod padding;
mod repaint_boundary;
mod sized_box;
mod sliver_ignore_pointer;
mod sliver_offstage;
mod sliver_opacity;
mod sliver_padding;
mod stack;
mod transform;

pub use absorb_pointer::RenderAbsorbPointer;
pub use aspect_ratio::{AspectRatio, RenderAspectRatio};
pub use center::RenderCenter;
pub use clip::{
    ClipGeometry, CustomClipper, Oval, RenderClip, RenderClipOval, RenderClipPath, RenderClipRRect,
    RenderClipRect,
};
pub use colored_box::RenderColoredBox;
pub use constrained_box::RenderConstrainedBox;
pub use fitted_box::RenderFittedBox;
pub use flex::{CrossAxisAlignment, FlexDirection, MainAxisAlignment, RenderFlex};
pub use fractional_translation::{RenderFractionalTranslation, TranslationFraction};
pub use fractionally_sized_box::{FractionFactor, RenderFractionallySizedBox};
pub use ignore_pointer::RenderIgnorePointer;
pub use limited_box::RenderLimitedBox;
pub use meta_data::{MetaDataPayload, RenderMetaData};
pub use offstage::RenderOffstage;
pub use opacity::RenderOpacity;
pub use padding::RenderPadding;
pub use repaint_boundary::RenderRepaintBoundary;
pub use sized_box::RenderSizedBox;
pub use sliver_ignore_pointer::RenderSliverIgnorePointer;
pub use sliver_offstage::RenderSliverOffstage;
pub use sliver_opacity::RenderSliverOpacity;
pub use sliver_padding::RenderSliverPadding;
pub use stack::{PositionedSpec, RenderStack, StackFit};
pub use transform::RenderTransform;
