//! Clip utilities for painting contexts.
//!
//! This module provides the [`ClipContext`] trait which abstracts clipping operations
//! for painting. It mirrors Flutter's `ClipContext` from `painting/clip.dart`.

use flui_types::painting::Path;
use flui_types::{RRect, Rect};

use flui_painting::Canvas;
use flui_types::painting::{Clip, ClipOp, Paint};

/// Clip utilities used by [`CanvasContext`](super::CanvasContext).
///
/// This trait provides convenience methods for clipping and painting in one operation.
/// After painting, the canvas is restored to its pre-clip state.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::pipeline::{ClipContext, Clip};
///
/// fn paint_clipped(ctx: &mut impl ClipContext) {
///     let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
///     ctx.clip_rect_and_paint(rect, Clip::AntiAlias, rect, || {
///         // Paint within the clipped area
///     });
/// }
/// ```
pub trait ClipContext {
    /// The canvas on which to paint.
    fn canvas(&mut self) -> &mut Canvas;

    /// Internal helper for clip-and-paint operations.
    ///
    /// This handles the save/restore logic and optional save layer for
    /// `Clip::AntiAliasWithSaveLayer`.
    fn clip_and_paint<F>(
        &mut self,
        clip_fn: impl FnOnce(&mut Canvas, bool),
        clip_behavior: Clip,
        bounds: Rect,
        painter: F,
    ) where
        F: FnOnce(&mut Canvas),
    {
        let canvas = self.canvas();
        canvas.save();

        match clip_behavior {
            Clip::None => {
                // No clipping
            }
            Clip::HardEdge => {
                clip_fn(canvas, false);
            }
            Clip::AntiAlias => {
                clip_fn(canvas, true);
            }
            Clip::AntiAliasWithSaveLayer => {
                clip_fn(canvas, true);
                canvas.save_layer(Some(bounds), &Paint::default());
            }
        }

        painter(canvas);

        if clip_behavior == Clip::AntiAliasWithSaveLayer {
            canvas.restore();
        }
        canvas.restore();
    }

    /// Clip canvas with a [`Path`] according to [`Clip`] behavior and then paint.
    ///
    /// The canvas is restored to its pre-clip status afterwards.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to clip to
    /// * `clip_behavior` - How to perform the clipping
    /// * `bounds` - The saveLayer bounds used for [`Clip::AntiAliasWithSaveLayer`]
    /// * `painter` - The painting callback
    fn clip_path_and_paint<F>(&mut self, path: &Path, clip_behavior: Clip, bounds: Rect, painter: F)
    where
        F: FnOnce(&mut Canvas),
    {
        let path_clone = path.clone();
        self.clip_and_paint(
            move |canvas, do_anti_alias| {
                let clip_behavior = if do_anti_alias {
                    Clip::AntiAlias
                } else {
                    Clip::HardEdge
                };
                canvas.clip_path_ext(&path_clone, ClipOp::Intersect, clip_behavior);
            },
            clip_behavior,
            bounds,
            painter,
        );
    }

    /// Clip canvas with an [`RRect`] according to [`Clip`] behavior and then paint.
    ///
    /// The canvas is restored to its pre-clip status afterwards.
    ///
    /// # Arguments
    ///
    /// * `rrect` - The rounded rectangle to clip to
    /// * `clip_behavior` - How to perform the clipping
    /// * `bounds` - The saveLayer bounds used for [`Clip::AntiAliasWithSaveLayer`]
    /// * `painter` - The painting callback
    fn clip_rrect_and_paint<F>(
        &mut self,
        rrect: RRect,
        clip_behavior: Clip,
        bounds: Rect,
        painter: F,
    ) where
        F: FnOnce(&mut Canvas),
    {
        self.clip_and_paint(
            move |canvas, do_anti_alias| {
                let clip_behavior = if do_anti_alias {
                    Clip::AntiAlias
                } else {
                    Clip::HardEdge
                };
                canvas.clip_rrect_ext(rrect, ClipOp::Intersect, clip_behavior);
            },
            clip_behavior,
            bounds,
            painter,
        );
    }

    /// Clip canvas with a [`Rect`] according to [`Clip`] behavior and then paint.
    ///
    /// The canvas is restored to its pre-clip status afterwards.
    ///
    /// # Arguments
    ///
    /// * `rect` - The rectangle to clip to
    /// * `clip_behavior` - How to perform the clipping
    /// * `bounds` - The saveLayer bounds used for [`Clip::AntiAliasWithSaveLayer`]
    /// * `painter` - The painting callback
    fn clip_rect_and_paint<F>(&mut self, rect: Rect, clip_behavior: Clip, bounds: Rect, painter: F)
    where
        F: FnOnce(&mut Canvas),
    {
        self.clip_and_paint(
            move |canvas, do_anti_alias| {
                let clip_behavior = if do_anti_alias {
                    Clip::AntiAlias
                } else {
                    Clip::HardEdge
                };
                canvas.clip_rect_ext(rect, ClipOp::Intersect, clip_behavior);
            },
            clip_behavior,
            bounds,
            painter,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_clip_none() {
        let clip = Clip::None;
        assert!(!clip.is_anti_aliased());
        assert!(!clip.saves_layer());
        assert!(!clip.clips());
    }

    #[test]
    fn test_clip_hard_edge() {
        let clip = Clip::HardEdge;
        assert!(!clip.is_anti_aliased());
        assert!(!clip.saves_layer());
        assert!(clip.clips());
    }

    #[test]
    fn test_clip_anti_alias() {
        let clip = Clip::AntiAlias;
        assert!(clip.is_anti_aliased());
        assert!(!clip.saves_layer());
        assert!(clip.clips());
    }

    #[test]
    fn test_clip_anti_alias_with_save_layer() {
        let clip = Clip::AntiAliasWithSaveLayer;
        assert!(clip.is_anti_aliased());
        assert!(clip.saves_layer());
        assert!(clip.clips());
    }

    #[test]
    fn test_clip_default() {
        let clip = Clip::default();
        assert_eq!(clip, Clip::HardEdge);
    }

    /// Test struct implementing ClipContext for testing
    struct TestClipContext {
        canvas: Canvas,
    }

    impl ClipContext for TestClipContext {
        fn canvas(&mut self) -> &mut Canvas {
            &mut self.canvas
        }
    }

    #[test]
    fn test_clip_rect_and_paint_none() {
        let mut ctx = TestClipContext {
            canvas: Canvas::new(),
        };

        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        ctx.clip_rect_and_paint(rect, Clip::None, rect, |canvas| {
            canvas.draw_rect(rect, &Paint::default());
        });

        // Canvas.len() counts DrawCommands in display_list, not save/restore
        // Clip::None: just DrawRect (save/restore don't add commands)
        assert_eq!(ctx.canvas.len(), 1);
    }

    #[test]
    fn test_clip_rect_and_paint_hard_edge() {
        let mut ctx = TestClipContext {
            canvas: Canvas::new(),
        };

        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        ctx.clip_rect_and_paint(rect, Clip::HardEdge, rect, |canvas| {
            canvas.draw_rect(rect, &Paint::default());
        });

        // Clip::HardEdge: ClipRect + DrawRect (save/restore don't add commands)
        assert_eq!(ctx.canvas.len(), 2);
    }

    #[test]
    fn test_clip_rect_and_paint_anti_alias() {
        let mut ctx = TestClipContext {
            canvas: Canvas::new(),
        };

        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        ctx.clip_rect_and_paint(rect, Clip::AntiAlias, rect, |canvas| {
            canvas.draw_rect(rect, &Paint::default());
        });

        // Clip::AntiAlias: ClipRect + DrawRect (save/restore don't add commands)
        assert_eq!(ctx.canvas.len(), 2);
    }

    #[test]
    fn test_clip_rect_and_paint_with_save_layer() {
        let mut ctx = TestClipContext {
            canvas: Canvas::new(),
        };

        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        ctx.clip_rect_and_paint(rect, Clip::AntiAliasWithSaveLayer, rect, |canvas| {
            canvas.draw_rect(rect, &Paint::default());
        });

        // Clip::AntiAliasWithSaveLayer: ClipRect + SaveLayer + DrawRect + RestoreLayer
        // save_layer adds SaveLayer command, restore from save_layer adds RestoreLayer
        assert_eq!(ctx.canvas.len(), 4);
    }

    #[test]
    fn test_clip_rrect_and_paint() {
        let mut ctx = TestClipContext {
            canvas: Canvas::new(),
        };

        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let rrect = RRect::from_rect_xy(rect, px(10.0), px(10.0));
        ctx.clip_rrect_and_paint(rrect, Clip::AntiAlias, rect, |canvas| {
            canvas.draw_rect(rect, &Paint::default());
        });

        // ClipRRect + DrawRect (save/restore don't add commands)
        assert_eq!(ctx.canvas.len(), 2);
    }

    #[test]
    fn test_clip_path_and_paint() {
        use flui_types::Point;

        let mut ctx = TestClipContext {
            canvas: Canvas::new(),
        };

        let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let mut path = Path::new();
        path.move_to(Point::new(px(0.0), px(0.0)));
        path.line_to(Point::new(px(100.0), px(0.0)));
        path.line_to(Point::new(px(100.0), px(100.0)));
        path.close();

        ctx.clip_path_and_paint(&path, Clip::AntiAlias, rect, |canvas| {
            canvas.draw_rect(rect, &Paint::default());
        });

        // ClipPath + DrawRect (save/restore don't add commands)
        assert_eq!(ctx.canvas.len(), 2);
    }
}
