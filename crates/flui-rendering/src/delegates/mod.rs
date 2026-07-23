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
//! | `CustomClipper` | Owner-local custom clipping shapes | experimental only |
//!
//! # Feature gating
//!
//! `SliverGridDelegate`, `CustomPainter`, `FlowDelegate`,
//! `SingleChildLayoutDelegate`, and `MultiChildLayoutDelegate` (plus their
//! concrete implementations) are unconditionally available because their
//! companion render objects ship in the default build. The remaining delegate
//! (`CustomClipper`) is still gated behind `experimental-delegates`; production
//! path-based render effects use ADR-0027 owner-lane targets instead of storing
//! delegate callbacks in render objects.

// Grid delegate — always available because RenderSliverGrid ships unconditionally.
mod sliver_grid_delegate;
pub use sliver_grid_delegate::*;

// Custom-painting delegate — always available because RenderCustomPaint ships
// unconditionally (flui-objects `proxy::custom_paint`).
mod custom_painter;
pub use custom_painter::*;

// Flow delegate — always available because RenderFlow ships unconditionally
// (flui-objects `layout::flow`, ADR-0007 amendment).
mod flow_delegate;
pub use flow_delegate::*;

// Single-child layout delegate — always available because
// RenderCustomSingleChildLayoutBox ships unconditionally (flui-objects
// `layout::custom_single_child_layout`, ADR-0007 amendment).
mod single_child_layout_delegate;
pub use single_child_layout_delegate::*;

// Multi-child layout delegate — always available because
// RenderCustomMultiChildLayoutBox ships unconditionally (flui-objects
// `layout::custom_multi_child_layout`, ADR-0007 amendment).
mod multi_child_layout_delegate;
pub use multi_child_layout_delegate::*;

// Companion-less delegates — gated until their render objects land.
#[cfg(feature = "experimental-delegates")]
mod custom_clipper;
#[cfg(feature = "experimental-delegates")]
pub use custom_clipper::*;
