//! Delegate traits for extensible render objects.
//!
//! This module provides custom behavior delegates that allow users to customize
//! layout, painting, and clipping behavior without subclassing.
//!
//! # Delegate Types
//!
//! | Delegate | Purpose | Used By |
//! |----------|---------|---------|
//! | [`SliverGridDelegate`] | Grid layout in slivers | RenderSliverGrid |
//! | [`CustomPainter`] | Custom painting | RenderCustomPaint |
//! | [`FlowDelegate`] | Flow layout algorithm | RenderFlow |
//! | [`SingleChildLayoutDelegate`] | Custom single-child layout | RenderCustomSingleChildLayoutBox |
//! | [`MultiChildLayoutDelegate`] | Custom multi-child layout | RenderCustomMultiChildLayoutBox |

// Grid delegate — always available because RenderSliverGrid ships unconditionally.
mod sliver_grid_delegate;
pub use sliver_grid_delegate::*;

// Custom-painting delegate — always available because RenderCustomPaint ships
// unconditionally (flui-objects `proxy::custom_paint`).
mod custom_painter;
pub use custom_painter::*;

// Flow delegate — always available because RenderFlow ships unconditionally
// (flui-objects `layout::flow`).
mod flow_delegate;
pub use flow_delegate::*;

// Single-child layout delegate — always available because
// RenderCustomSingleChildLayoutBox ships unconditionally (flui-objects
// `layout::custom_single_child_layout`).
mod single_child_layout_delegate;
pub use single_child_layout_delegate::*;

// Multi-child layout delegate — always available because
// RenderCustomMultiChildLayoutBox ships unconditionally (flui-objects
// `layout::custom_multi_child_layout`).
mod multi_child_layout_delegate;
pub use multi_child_layout_delegate::*;
