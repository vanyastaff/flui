//! Delegate traits for extensible render objects.
//!
//! This module provides custom behavior delegates that allow users to customize
//! layout, painting, and clipping behavior without subclassing.
//!
//! # Delegate Types
//!
//! | Delegate | Purpose | Used By |
//! |----------|---------|---------|
//! | [`CustomPainter`] | Custom painting | RenderCustomPaint |
//! | [`CustomClipper`] | Custom clipping shapes | RenderClip* objects |
//! | [`SingleChildLayoutDelegate`] | Custom single-child layout | RenderCustomSingleChildLayoutBox |
//! | [`MultiChildLayoutDelegate`] | Custom multi-child layout | RenderCustomMultiChildLayoutBox |
//! | [`FlowDelegate`] | Flow layout algorithm | RenderFlow |
//! | [`SliverGridDelegate`] | Grid layout in slivers | RenderSliverGrid |

mod custom_clipper;
mod custom_painter;
mod flow_delegate;
mod multi_child_layout_delegate;
mod single_child_layout_delegate;
mod sliver_grid_delegate;

pub use custom_clipper::*;
pub use custom_painter::*;
pub use flow_delegate::*;
pub use multi_child_layout_delegate::*;
pub use single_child_layout_delegate::*;
pub use sliver_grid_delegate::*;
