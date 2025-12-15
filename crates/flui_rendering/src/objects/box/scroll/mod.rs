//! Scroll-related render objects.
//!
//! This module provides render objects for scrollable content:
//!
//! - [`RenderViewport`]: Standard viewport with fixed extent
//! - [`RenderShrinkWrappingViewport`]: Viewport that sizes to content
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `rendering/viewport.dart`.

mod shrink_wrapping_viewport;
mod viewport;

pub use shrink_wrapping_viewport::RenderShrinkWrappingViewport;
pub use viewport::RenderViewport;
