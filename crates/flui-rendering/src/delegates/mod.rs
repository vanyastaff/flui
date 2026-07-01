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
//! | `CustomClipper` | Custom clipping shapes | RenderClip* objects |
//! | `SingleChildLayoutDelegate` | Custom single-child layout | RenderCustomSingleChildLayoutBox |
//! | `MultiChildLayoutDelegate` | Custom multi-child layout | RenderCustomMultiChildLayoutBox |
//! | `FlowDelegate` | Flow layout algorithm | RenderFlow |
//!
//! # Feature gating
//!
//! `SliverGridDelegate` and `CustomPainter` (plus their concrete
//! implementations) are unconditionally available because `RenderSliverGrid`
//! and `RenderCustomPaint` ship in the default build. The remaining three
//! delegates (`CustomClipper`, `SingleChildLayoutDelegate`,
//! `MultiChildLayoutDelegate`/`FlowDelegate`) are still gated behind
//! `experimental-delegates` until their companion render objects land.

// Grid delegate — always available because RenderSliverGrid ships unconditionally.
mod sliver_grid_delegate;
pub use sliver_grid_delegate::*;

// Custom-painting delegate — always available because RenderCustomPaint ships
// unconditionally (flui-objects `proxy::custom_paint`).
mod custom_painter;
pub use custom_painter::*;

// Companion-less delegates — gated until their render objects land.
#[cfg(feature = "experimental-delegates")]
mod custom_clipper;
#[cfg(feature = "experimental-delegates")]
mod flow_delegate;
#[cfg(feature = "experimental-delegates")]
mod multi_child_layout_delegate;
#[cfg(feature = "experimental-delegates")]
mod single_child_layout_delegate;

#[cfg(feature = "experimental-delegates")]
pub use custom_clipper::*;
#[cfg(feature = "experimental-delegates")]
pub use flow_delegate::*;
#[cfg(feature = "experimental-delegates")]
pub use multi_child_layout_delegate::*;
#[cfg(feature = "experimental-delegates")]
pub use single_child_layout_delegate::*;
