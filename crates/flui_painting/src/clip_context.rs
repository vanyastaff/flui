//! ClipContext - Base trait for clip operations.
//!
//! # Flutter Reference
//!
//! **Source:** `flutter/packages/flutter/lib/src/painting/clip.dart`
//! **Lines:** 1-95 (Flutter 3.24)
//!
//! This module provides the [`ClipContext`] trait, which is the base abstraction
//! for clip operations. In Flutter, `ClipContext` is an abstract class that
//! `PaintingContext` extends.
//!
//! # Architecture
//!
//! ```text
//! ClipContext (trait)  [flui_painting]
//!     └── PaintingContext (implements)  [flui_rendering]
//!             └── TestRecordingPaintingContext (for testing)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_painting::ClipContext;
//! use flui_rendering::PaintingContext;
//!
//! fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!     // ClipContext methods available directly:
//!     ctx.clip_rect_and_paint(rect, Clip::AntiAlias, bounds, |ctx| {
//!         ctx.paint_child(child_id, offset);
//!     });
//! }
//! ```

use crate::Canvas;
use flui_types::{
    geometry::{RRect, Rect},
    painting::{Clip, Path},
};

// ============================================================================
// CLIP CONTEXT TRAIT
// ============================================================================

/// Base trait for clip operations on a canvas.
///
/// This trait provides methods for clipping content to rectangular, rounded
/// rectangular, and arbitrary path shapes.
///
/// # Flutter Equivalence
///
/// ```dart
/// // Flutter (painting/clip.dart)
/// abstract class ClipContext {
///   Canvas get canvas;
///   void clipPathAndPaint(Path path, Clip clipBehavior, Rect bounds, VoidCallback painter);
///   void clipRRectAndPaint(RRect rrect, Clip clipBehavior, Rect bounds, VoidCallback painter);
///   void clipRectAndPaint(Rect rect, Clip clipBehavior, Rect bounds, VoidCallback painter);
/// }
/// ```
///
/// # Design Notes
///
/// Unlike Flutter where `PaintingContext extends ClipContext`, in Rust we use
/// a trait with default implementations. This provides:
///
/// - **Composition over inheritance**: Any type with a canvas can implement this
/// - **Testability**: Easy to create mock contexts for testing
/// - **Zero runtime cost**: Default implementations are inlined
pub trait ClipContext {
    /// Returns a mutable reference to the canvas.
    ///
    /// This is the only method that implementors must provide.
    fn canvas_mut(&mut self) -> &mut Canvas;

    /// Clips to a rectangle and paints content within.
    ///
    /// The canvas is saved before clipping and restored after painting,
    /// ensuring the clip doesn't affect subsequent drawing operations.
    ///
    /// # Arguments
    ///
    /// * `rect` - Rectangle to clip to
    /// * `clip_behavior` - How to perform the clip (anti-aliasing, save layer, etc.)
    /// * `bounds` - Bounds hint for the painting operation (used for optimization)
    /// * `painter` - Closure that performs the painting within the clipped region
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// // Flutter
    /// context.clipRectAndPaint(rect, Clip.antiAlias, bounds, () {
    ///   // paint content
    /// });
    /// ```
    #[inline]
    fn clip_rect_and_paint<F>(&mut self, rect: Rect, clip_behavior: Clip, bounds: Rect, painter: F)
    where
        F: FnOnce(&mut Self),
    {
        self._clip_and_paint(
            |canvas, do_anti_alias| {
                canvas.clip_rect_ext(
                    rect,
                    flui_types::painting::ClipOp::Intersect,
                    if do_anti_alias {
                        Clip::AntiAlias
                    } else {
                        Clip::HardEdge
                    },
                );
            },
            clip_behavior,
            bounds,
            painter,
        );
    }

    /// Clips to a rounded rectangle and paints content within.
    ///
    /// # Arguments
    ///
    /// * `rrect` - Rounded rectangle to clip to
    /// * `clip_behavior` - How to perform the clip
    /// * `bounds` - Bounds hint for the painting operation
    /// * `painter` - Closure that performs the painting
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// // Flutter
    /// context.clipRRectAndPaint(rrect, Clip.antiAlias, bounds, () {
    ///   // paint content
    /// });
    /// ```
    #[inline]
    fn clip_rrect_and_paint<F>(
        &mut self,
        rrect: RRect,
        clip_behavior: Clip,
        bounds: Rect,
        painter: F,
    ) where
        F: FnOnce(&mut Self),
    {
        self._clip_and_paint(
            |canvas, do_anti_alias| {
                canvas.clip_rrect_ext(
                    rrect,
                    flui_types::painting::ClipOp::Intersect,
                    if do_anti_alias {
                        Clip::AntiAlias
                    } else {
                        Clip::HardEdge
                    },
                );
            },
            clip_behavior,
            bounds,
            painter,
        );
    }

    /// Clips to an arbitrary path and paints content within.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to clip to
    /// * `clip_behavior` - How to perform the clip
    /// * `bounds` - Bounds hint for the painting operation
    /// * `painter` - Closure that performs the painting
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// // Flutter
    /// context.clipPathAndPaint(path, Clip.antiAlias, bounds, () {
    ///   // paint content
    /// });
    /// ```
    #[inline]
    fn clip_path_and_paint<F>(&mut self, path: &Path, clip_behavior: Clip, bounds: Rect, painter: F)
    where
        F: FnOnce(&mut Self),
    {
        self._clip_and_paint(
            |canvas, do_anti_alias| {
                canvas.clip_path_ext(
                    path,
                    flui_types::painting::ClipOp::Intersect,
                    if do_anti_alias {
                        Clip::AntiAlias
                    } else {
                        Clip::HardEdge
                    },
                );
            },
            clip_behavior,
            bounds,
            painter,
        );
    }

    /// Internal helper for clip operations.
    ///
    /// This implements the common pattern of:
    /// 1. Save canvas state (optionally with layer for anti-aliased clips)
    /// 2. Apply clip
    /// 3. Execute painter
    /// 4. Restore canvas state
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// // Flutter (painting/clip.dart #15-45)
    /// void _clipAndPaint(
    ///   void Function(bool doAntiAlias) canvasClipCall,
    ///   Clip clipBehavior,
    ///   Rect bounds,
    ///   VoidCallback painter,
    /// ) { ... }
    /// ```
    #[doc(hidden)]
    fn _clip_and_paint<C, F>(
        &mut self,
        canvas_clip_call: C,
        clip_behavior: Clip,
        bounds: Rect,
        painter: F,
    ) where
        C: FnOnce(&mut Canvas, bool),
        F: FnOnce(&mut Self),
    {
        // Handle Clip::None - no clipping needed
        if clip_behavior == Clip::None {
            painter(self);
            return;
        }

        // Save canvas state
        // For AntiAliasWithSaveLayer, we need to save a layer to get proper
        // edge anti-aliasing when content overlaps the clip boundary
        if clip_behavior == Clip::AntiAliasWithSaveLayer {
            self.canvas_mut().save_layer_alpha(Some(bounds), 255);
        } else {
            self.canvas_mut().save();
        }

        // Apply clip with appropriate anti-aliasing
        let do_anti_alias = clip_behavior.is_anti_aliased();
        canvas_clip_call(self.canvas_mut(), do_anti_alias);

        // Execute painter within clipped region
        painter(self);

        // Restore canvas state (also restores layer if save_layer was used)
        self.canvas_mut().restore();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test implementation of ClipContext for unit testing
    struct TestClipContext {
        canvas: Canvas,
        #[allow(dead_code)]
        clip_calls: Vec<String>,
    }

    impl TestClipContext {
        fn new() -> Self {
            Self {
                canvas: Canvas::new(),
                clip_calls: Vec::new(),
            }
        }
    }

    impl ClipContext for TestClipContext {
        fn canvas_mut(&mut self) -> &mut Canvas {
            &mut self.canvas
        }
    }

    #[test]
    fn test_clip_rect_and_paint() {
        let mut ctx = TestClipContext::new();
        let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let bounds = rect;

        let mut painted = false;
        ctx.clip_rect_and_paint(rect, Clip::AntiAlias, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted);
    }

    #[test]
    fn test_clip_none_skips_clipping() {
        let mut ctx = TestClipContext::new();
        let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let bounds = rect;

        let initial_save_count = ctx.canvas.save_count();

        ctx.clip_rect_and_paint(rect, Clip::None, bounds, |_ctx| {
            // Painter executes
        });

        // Save count should be unchanged (no save/restore for Clip::None)
        assert_eq!(ctx.canvas.save_count(), initial_save_count);
    }

    #[test]
    fn test_clip_rrect_and_paint() {
        let mut ctx = TestClipContext::new();
        let rrect =
            RRect::from_rect_elliptical(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0), 10.0, 10.0);
        let bounds = rrect.bounding_rect();

        let mut painted = false;
        ctx.clip_rrect_and_paint(rrect, Clip::HardEdge, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted);
    }

    #[test]
    fn test_clip_path_and_paint() {
        let mut ctx = TestClipContext::new();
        let path = Path::new(); // Empty path for testing
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);

        let mut painted = false;
        ctx.clip_path_and_paint(&path, Clip::AntiAlias, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted);
    }
}
