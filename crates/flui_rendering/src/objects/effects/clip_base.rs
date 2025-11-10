//! Generic base for all clip RenderObjects
//!
//! This module provides a generic `RenderClip<S>` implementation that eliminates
//! code duplication across RenderClipRect, RenderClipRRect, RenderClipOval, and RenderClipPath.
//!
//! # Design
//!
//! - `ClipShape` trait: defines how to apply clipping to a canvas
//! - `RenderClip<S>`: generic RenderObject that uses any ClipShape
//! - Each clip type becomes a thin wrapper with a ClipShape implementation
//!
//! # Example
//!
//! ```rust,ignore
//! // Define a shape
//! #[derive(Debug)]
//! pub struct RectShape;
//!
//! impl ClipShape for RectShape {
//!     fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
//!         let rect = Rect::from_origin_size(Offset::ZERO, size);
//!         canvas.clip_rect(rect);
//!     }
//! }
//!
//! // Use it with generic base
//! pub type RenderClipRect = RenderClip<RectShape>;
//! ```

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::{painting::Clip, Size};

/// Trait for defining clip shapes
///
/// Implement this trait to define how a specific shape applies clipping to a canvas.
/// The generic `RenderClip<S>` handles all the common clipping logic.
pub trait ClipShape: std::fmt::Debug + Send + Sync {
    /// Apply clipping to the canvas for this shape
    ///
    /// # Parameters
    ///
    /// - `canvas`: The canvas to apply clipping to
    /// - `size`: The size of the render object (from layout)
    fn apply_clip(&self, canvas: &mut Canvas, size: Size);
}

/// Generic clip RenderObject
///
/// This eliminates ~410 lines of duplication across the 4 clip types.
/// Each clip type (Rect, RRect, Oval, Path) is now just a thin wrapper
/// that implements `ClipShape` and uses this generic base.
///
/// # Type Parameters
///
/// - `S`: The clip shape implementation (RectShape, RRectShape, etc.)
///
/// # Common Behavior
///
/// - Layout: Pass-through to child_id with same constraints
/// - Paint:
///   - If `!clip_behavior.clips()`, return child_id layer directly
///   - Otherwise, get child_id layer and wrap it with shape's clip layer
///
/// # Example
///
/// ```rust,ignore
/// // RenderClipRect is now just:
/// pub type RenderClipRect = RenderClip<RectShape>;
///
/// let clip = RenderClip::new(RectShape, Clip::AntiAlias);
/// ```
#[derive(Debug)]
pub struct RenderClip<S: ClipShape> {
    /// The clipping behavior (None, HardEdge, AntiAlias, etc.)
    pub clip_behavior: Clip,

    /// The shape to clip to
    pub shape: S,

    /// Cached size from layout
    size: Size,
}

impl<S: ClipShape> RenderClip<S> {
    /// Create new RenderClip with specified shape and clip behavior
    pub fn new(shape: S, clip_behavior: Clip) -> Self {
        Self {
            shape,
            clip_behavior,
            size: Size::ZERO,
        }
    }

    /// Set new clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        self.clip_behavior = clip_behavior;
    }

    /// Get the clip behavior
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Get a reference to the shape
    pub fn shape(&self) -> &S {
        &self.shape
    }

    /// Get a mutable reference to the shape
    pub fn shape_mut(&mut self) -> &mut S {
        &mut self.shape
    }
}

impl<S: ClipShape + 'static> Render for RenderClip<S> {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child_id with same constraints (pass-through)
        let size = tree.layout_child(child_id, constraints);
        // Cache size for paint
        self.size = size;
        size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // If no clipping needed, just return child canvas directly
        if !self.clip_behavior.clips() {
            return tree.paint_child(child_id, offset);
        }

        // Create canvas and apply clipping
        let mut canvas = Canvas::new();

        // Save canvas state before clipping
        canvas.save();

        // Let the shape apply its specific clipping
        self.shape.apply_clip(&mut canvas, self.size);

        // Paint child with clipping applied
        let child_canvas = tree.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);

        // Restore canvas state
        canvas.restore();

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Rect;

    // Test shape implementation
    #[derive(Debug)]
    struct TestShape;

    impl ClipShape for TestShape {
        fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
            // Apply a simple rectangular clip for testing
            let rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
            canvas.clip_rect(rect);
        }
    }

    #[test]
    fn test_render_clip_new() {
        let clip = RenderClip::new(TestShape, Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_set_clip_behavior() {
        let mut clip = RenderClip::new(TestShape, Clip::HardEdge);
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);

        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_shape_access() {
        let clip = RenderClip::new(TestShape, Clip::AntiAlias);
        let _shape = clip.shape();
        // Just verify we can access the shape
    }
}
