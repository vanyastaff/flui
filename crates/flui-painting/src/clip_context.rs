//! ClipContext - Base trait for clip operations.
//!
//! # Flutter Reference
//!
//! **Source:** `flutter/packages/flutter/lib/src/painting/clip.dart`
//! **Lines:** 1-95 (Flutter 3.24)
//!
//! This module provides the [`ClipContext`] trait, which is the base
//! abstraction for clip operations. In Flutter, `ClipContext` is an abstract
//! class that `PaintingContext` extends; in FLUI the production implementer
//! is `flui_rendering::context::CanvasContext` (a `PaintingContext` type is
//! not present in the current workspace).
//!
//! # Architecture
//!
//! ```text
//! ClipContext (trait)  [flui_painting]
//!     └── CanvasContext (implements)  [flui_rendering]
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_painting::ClipContext;
//! use flui_rendering::context::CanvasContext;
//! use flui_types::geometry::Pixels;
//!
//! fn paint(&self, ctx: &mut CanvasContext, offset: Offset<Pixels>) {
//!     // ClipContext methods available directly:
//!     ctx.clip_rect_and_paint(rect, Clip::AntiAlias, bounds, |ctx| {
//!         ctx.canvas().draw_rect(rect, &paint);
//!     });
//! }
//! ```

use flui_types::{
    geometry::{Pixels, RRect, RSuperellipse, Rect},
    painting::{Clip, Path},
};

use crate::Canvas;

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
///   void clipRSuperellipseAndPaint(RSuperellipse rse, Clip clipBehavior, Rect bounds, VoidCallback painter);
///   void clipRectAndPaint(Rect rect, Clip clipBehavior, Rect bounds, VoidCallback painter);
/// }
/// ```
///
/// # Design Notes
///
/// Unlike Flutter where `PaintingContext extends ClipContext`, in Rust we use
/// a trait with default implementations. This provides:
///
/// - **Composition over inheritance**: Any type with a canvas can implement
///   this
/// - **Testability**: Easy to create mock contexts for testing
/// - **Zero runtime cost**: Default implementations are inlined
pub trait ClipContext {
    /// Returns a mutable reference to the canvas.
    ///
    /// This is the only method that implementors must provide. Named `canvas`
    /// (no `_mut` suffix) to match Flutter's `Canvas get canvas` getter; the
    /// trait is `&mut self -> &mut Canvas` so Rust's `wrong_self_convention`
    /// lint does not fire on the missing suffix.
    fn canvas(&mut self) -> &mut Canvas;

    /// Clips to a rectangle and paints content within.
    ///
    /// The canvas is saved before clipping and restored after painting,
    /// ensuring the clip doesn't affect subsequent drawing operations.
    ///
    /// # Arguments
    ///
    /// * `rect` - Rectangle to clip to
    /// * `clip_behavior` - How to perform the clip (anti-aliasing, save layer,
    ///   etc.)
    /// * `bounds` - Bounds hint for the painting operation (used for
    ///   optimization)
    /// * `painter` - Closure that performs the painting within the clipped
    ///   region
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
    fn clip_rect_and_paint<F>(
        &mut self,
        rect: Rect<Pixels>,
        clip_behavior: Clip,
        bounds: Rect<Pixels>,
        painter: F,
    ) where
        F: FnOnce(&mut Self),
    {
        self.clip_and_paint_impl(
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
        bounds: Rect<Pixels>,
        painter: F,
    ) where
        F: FnOnce(&mut Self),
    {
        self.clip_and_paint_impl(
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

    /// Clips to a rounded superellipse and paints content within.
    ///
    /// The rounded-superellipse (Flutter `RSuperellipse`) uses a smoother
    /// corner falloff than the elliptical arcs of `RRect`, matching the iOS
    /// squircle shape.
    ///
    /// # Arguments
    ///
    /// * `rsuperellipse` - Rounded superellipse to clip to
    /// * `clip_behavior` - How to perform the clip
    /// * `bounds` - Bounds hint for the painting operation
    /// * `painter` - Closure that performs the painting
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// // Flutter
    /// context.clipRSuperellipseAndPaint(rse, Clip.antiAlias, bounds, () {
    ///   // paint content
    /// });
    /// ```
    #[inline]
    fn clip_rsuperellipse_and_paint<F>(
        &mut self,
        rsuperellipse: RSuperellipse,
        clip_behavior: Clip,
        bounds: Rect<Pixels>,
        painter: F,
    ) where
        F: FnOnce(&mut Self),
    {
        self.clip_and_paint_impl(
            |canvas, do_anti_alias| {
                canvas.clip_rsuperellipse_ext(
                    rsuperellipse,
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
    fn clip_path_and_paint<F>(
        &mut self,
        path: &Path,
        clip_behavior: Clip,
        bounds: Rect<Pixels>,
        painter: F,
    ) where
        F: FnOnce(&mut Self),
    {
        self.clip_and_paint_impl(
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
    fn clip_and_paint_impl<C, F>(
        &mut self,
        canvas_clip_call: C,
        clip_behavior: Clip,
        bounds: Rect<Pixels>,
        painter: F,
    ) where
        C: FnOnce(&mut Canvas, bool),
        F: FnOnce(&mut Self),
    {
        // Flutter parity: `save()` runs unconditionally, including for
        // `Clip::None`. The clip-call step is the only thing `Clip::None`
        // skips — save/restore around the painter still happens so nested
        // contexts observe consistent canvas state. Matches
        // `painting/clip.dart::_clipAndPaint` lines 21-37.
        self.canvas().save();

        let do_anti_alias = clip_behavior.is_anti_aliased();
        match clip_behavior {
            Clip::None => {
                // No clip-call step; save/restore still bracket the painter.
            }
            Clip::HardEdge | Clip::AntiAlias => {
                canvas_clip_call(self.canvas(), do_anti_alias);
            }
            Clip::AntiAliasWithSaveLayer => {
                canvas_clip_call(self.canvas(), true);
                self.canvas().save_layer_alpha(Some(bounds), 255);
            }
        }

        painter(self);

        if clip_behavior == Clip::AntiAliasWithSaveLayer {
            self.canvas().restore();
        }
        self.canvas().restore();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::{Radius, px};

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
        fn canvas(&mut self) -> &mut Canvas {
            &mut self.canvas
        }
    }

    #[test]
    fn test_clip_rect_and_paint() {
        let mut ctx = TestClipContext::new();
        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let bounds = rect;

        let mut painted = false;
        ctx.clip_rect_and_paint(rect, Clip::AntiAlias, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted);
    }

    /// Covers AE1 (R4): `Clip::None` no longer short-circuits the
    /// save/restore bracket. Flutter saves the canvas, runs the painter,
    /// then restores — even when no clip call is issued. This locks in
    /// that semantic against the prior FLUI short-circuit code path.
    #[test]
    fn clip_none_still_saves_and_restores_around_painter() {
        let mut ctx = TestClipContext::new();
        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let bounds = rect;

        let before = ctx.canvas.save_count();
        let mut observed_inside: Option<usize> = None;
        ctx.clip_rect_and_paint(rect, Clip::None, bounds, |ctx| {
            observed_inside = Some(ctx.canvas().save_count());
        });
        let after = ctx.canvas.save_count();

        assert_eq!(
            observed_inside,
            Some(before + 1),
            "save_count inside the painter must be one greater than before"
        );
        assert_eq!(
            after, before,
            "save_count after the call must return to the pre-call value"
        );
    }

    #[test]
    fn test_clip_rrect_and_paint() {
        let mut ctx = TestClipContext::new();
        let rrect = RRect::from_rect_elliptical(
            Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(10.0),
            px(10.0),
        );
        let bounds = rrect.bounding_rect();

        let mut painted = false;
        ctx.clip_rrect_and_paint(rrect, Clip::HardEdge, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted);
    }

    #[test]
    fn test_clip_rsuperellipse_and_paint() {
        let mut ctx = TestClipContext::new();
        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let rse = RSuperellipse::from_rect_and_radius(rect, Radius::circular(px(12.0)));
        let bounds = rse.outer_rect();

        let mut painted = false;
        ctx.clip_rsuperellipse_and_paint(rse, Clip::AntiAlias, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted, "rsuperellipse painter callback must execute");
    }

    #[test]
    fn test_clip_path_and_paint() {
        let mut ctx = TestClipContext::new();
        let path = Path::new(); // Empty path for testing
        let bounds = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));

        let mut painted = false;
        ctx.clip_path_and_paint(&path, Clip::AntiAlias, bounds, |_ctx| {
            painted = true;
        });

        assert!(painted);
    }
}
