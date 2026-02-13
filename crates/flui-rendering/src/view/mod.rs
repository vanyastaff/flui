//! View rendering - root of the render tree and viewport abstractions.
//!
//! This module provides the root render object for the render tree and
//! viewport-related types.
//!
//! # Architecture
//!
//! The view is the root of the render tree and is responsible for:
//! - Converting logical constraints to the render tree
//! - Managing the root transformation (device pixel ratio)
//! - Compositing the final frame to the screen
//! - Hit testing at the root level
//!
//! Viewports are render objects that are "bigger on the inside" - they
//! display a portion of their content controlled by a scroll offset.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's:
//! - `rendering/view.dart`
//! - `rendering/viewport_offset.dart`
//! - `rendering/viewport.dart` (partial)

mod configuration;
mod render_view;
mod viewport;
mod viewport_offset;

pub use configuration::ViewConfiguration;
pub use render_view::{CompositeResult, RenderView, RenderViewAdapter};
pub use viewport::{CacheExtentStyle, RenderAbstractViewport, RevealedOffset, SliverPaintOrder};
pub use viewport_offset::{
    FixedViewportOffset, ScrollDirection, ScrollableViewportOffset, ViewportOffset,
};
