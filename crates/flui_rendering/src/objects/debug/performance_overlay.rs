//! RenderPerformanceOverlay - displays performance metrics
//!
//! This module provides [`RenderPerformanceOverlay`], a debug-only render object
//! that displays performance metrics like FPS, frame time, and GPU usage.
//!
//! # Flutter Equivalence
//!
//! This implementation matches Flutter's performance overlay from
//! `package:flutter/src/widgets/performance_overlay.dart`.
//!
//! **Flutter Widget:**
//! ```dart
//! PerformanceOverlay({
//!   int checkerboardRasterCacheImages = 0,
//!   int checkerboardOffscreenLayers = 0,
//! });
//! ```
//!
//! # Usage
//!
//! The performance overlay is typically enabled in debug builds to monitor
//! app performance during development.

use crate::core::{BoxLayoutCtx, BoxPaintCtx, Leaf, RenderBox};
use crate::{RenderObject, RenderResult};
use flui_types::{Color, Point, Rect, Size};

/// RenderObject that displays performance metrics overlay.
///
/// This is a debug-only render object that shows FPS, frame time,
/// and other performance indicators.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderPerformanceOverlay;
///
/// // Create performance overlay
/// let overlay = RenderPerformanceOverlay::new();
/// ```
///
/// # Implementation Status
///
/// **TODO**: This is currently a stub implementation.
/// Full implementation will include:
/// - FPS counter
/// - Frame time graph
/// - GPU usage metrics
/// - Memory usage display
#[derive(Debug)]
pub struct RenderPerformanceOverlay {
    /// Cached size from layout
    size: Size,
}

impl RenderPerformanceOverlay {
    /// Create new RenderPerformanceOverlay
    pub fn new() -> Self {
        Self { size: Size::ZERO }
    }
}

impl Default for RenderPerformanceOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderPerformanceOverlay {}

impl RenderBox<Leaf> for RenderPerformanceOverlay {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Leaf>) -> RenderResult<Size> {
        // TODO: Use intrinsic size for overlay (e.g., 200x100)
        // For now, take a small corner of the screen
        let size = Size::new(
            ctx.constraints.max_width.min(200.0),
            ctx.constraints.max_height.min(100.0),
        );
        self.size = size;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Leaf>) {
        // TODO: Implement actual performance overlay painting
        // For now, draw a simple placeholder rectangle

        let rect = Rect::from_min_size(Point::ZERO, self.size);

        // Semi-transparent black background
        let bg_paint = flui_painting::Paint {
            color: Color::rgba(0, 0, 0, 180),
            style: flui_painting::PaintStyle::Fill,
            ..Default::default()
        };

        ctx.canvas_mut().draw_rect(rect, &bg_paint);

        // TODO: Add text rendering for FPS/metrics
        // TODO: Add graph rendering for frame times
        // TODO: Add GPU/memory metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_performance_overlay_new() {
        let overlay = RenderPerformanceOverlay::new();
        assert_eq!(overlay.size, Size::ZERO);
    }

    #[test]
    fn test_render_performance_overlay_default() {
        let overlay = RenderPerformanceOverlay::default();
        assert_eq!(overlay.size, Size::ZERO);
    }
}
