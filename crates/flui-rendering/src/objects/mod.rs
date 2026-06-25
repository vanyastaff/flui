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
//!   child via [`crate::traits::RenderSliver::paint_alpha`]
//!   (Core.2 Wave 5a)
//! - [`RenderSliverIgnorePointer`] - Pointers pass through the sliver
//!   subtree to siblings beneath in the viewport (Core.2 Wave 5a)
//! - [`RenderSliverOffstage`] - Hides a sliver subtree (zero geometry,
//!   skipped paint, no hit-test) (Core.2 Wave 5a)
//! - [`RenderSliverToBoxAdapter`] - Wraps one Box child in Sliver
//!   protocol geometry (Core.2 W3.3)
//! - [`RenderSliverFillRemainingWithScrollable`] - Sizes one Box child to
//!   the remaining viewport paint extent (Core.2 W3.8)
//! - [`RenderSliverFillRemaining`] /
//!   [`RenderSliverFillRemainingAndOverscroll`] - Non-scroll fill remaining
//!   variants that use Box child intrinsics (Core.2 W3.9)
//! - [`RenderSliverFillViewport`] - Sizes each Box child to a viewport
//!   fraction in the sliver main axis (Core.2 W3.10)
//! - [`RenderSliverFixedExtentList`] - Sizes each Box child to a fixed
//!   main-axis extent (Core.2 W3.17)
//! - [`RenderViewport`] - Box viewport that drives Sliver children
//!   (Core.2 W3.4a)
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
mod baseline;
mod center;
mod clip;
mod colored_box;
mod constrained_box;
mod decorated_box;
mod fitted_box;
mod flex;
mod fractional_translation;
mod fractionally_sized_box;
mod ignore_pointer;
mod image;
mod limited_box;
mod meta_data;
mod offstage;
mod opacity;
mod padding;
mod paragraph;
mod repaint_boundary;
mod sized_box;
mod sliver_fill_remaining;
mod sliver_fill_viewport;
mod sliver_fixed_extent_list;
mod sliver_ignore_pointer;
mod sliver_list_lazy;
mod sliver_offstage;
mod sliver_opacity;
mod sliver_padding;
mod sliver_to_box_adapter;
mod stack;
mod transform;
mod viewport;

pub use absorb_pointer::RenderAbsorbPointer;
pub use aspect_ratio::{AspectRatio, RenderAspectRatio};
pub use baseline::RenderBaseline;
pub use center::RenderCenter;
pub use clip::{
    ClipGeometry, CustomClipper, Oval, RenderClip, RenderClipOval, RenderClipPath, RenderClipRRect,
    RenderClipRect,
};
pub use colored_box::RenderColoredBox;
pub use constrained_box::RenderConstrainedBox;
pub use decorated_box::{DecorationPosition, RenderDecoratedBox};
pub use fitted_box::RenderFittedBox;
pub use flex::{CrossAxisAlignment, FlexDirection, MainAxisAlignment, MainAxisSize, RenderFlex};
pub use fractional_translation::{RenderFractionalTranslation, TranslationFraction};
pub use fractionally_sized_box::{FractionFactor, RenderFractionallySizedBox};
pub use ignore_pointer::RenderIgnorePointer;
pub use image::{ImageAlignment, ImageFit, RenderImage};
pub use limited_box::RenderLimitedBox;
pub use meta_data::{MetaDataPayload, RenderMetaData};
pub use offstage::RenderOffstage;
pub use opacity::RenderOpacity;
pub use padding::RenderPadding;
pub use paragraph::RenderParagraph;
pub use repaint_boundary::RenderRepaintBoundary;
pub use sized_box::RenderSizedBox;
pub use sliver_fill_remaining::{
    RenderSliverFillRemaining, RenderSliverFillRemainingAndOverscroll,
    RenderSliverFillRemainingWithScrollable,
};
pub use sliver_fill_viewport::RenderSliverFillViewport;
pub use sliver_fixed_extent_list::RenderSliverFixedExtentList;
pub use sliver_ignore_pointer::RenderSliverIgnorePointer;
pub use sliver_list_lazy::RenderSliverListLazy;
pub use sliver_offstage::RenderSliverOffstage;
pub use sliver_opacity::RenderSliverOpacity;
pub use sliver_padding::RenderSliverPadding;
pub use sliver_to_box_adapter::RenderSliverToBoxAdapter;
pub use stack::{PositionedSpec, RenderStack, StackFit};
pub use transform::RenderTransform;
pub use viewport::RenderViewport;
