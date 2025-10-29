//! Generic base for all clip RenderObjects
//!
//! This module provides a generic `RenderClip<S>` implementation that eliminates
//! code duplication across RenderClipRect, RenderClipRRect, RenderClipOval, and RenderClipPath.
//!
//! # Design
//!
//! - `ClipShape` trait: defines how to create a clip layer for a specific shape
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
//!     fn create_clip_layer(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer {
//!         let rect = Rect::from_origin_size(Offset::ZERO, size);
//!         let mut clip_layer = ClipRectLayer::new(rect);
//!         clip_layer.add_child(child_layer);
//!         Box::new(clip_layer)
//!     }
//! }
//!
//! // Use it with generic base
//! pub type RenderClipRect = RenderClip<RectShape>;
//! ```

use flui_core::render::{
    LayoutCx, PaintCx, RenderObject, SingleArity, SingleChild, SingleChildPaint,
};
use flui_engine::BoxedLayer;
use flui_types::{Size, painting::Clip};

/// Trait for defining clip shapes
///
/// Implement this trait to define how a specific shape creates its clip layer.
/// The generic `RenderClip<S>` handles all the common clipping logic.
pub trait ClipShape: std::fmt::Debug + Send + Sync {
    /// Create a clip layer for this shape
    ///
    /// # Parameters
    ///
    /// - `child_layer`: The child layer to be clipped
    /// - `size`: The size of the render object (from layout)
    ///
    /// # Returns
    ///
    /// A boxed layer that clips the child to this shape
    fn create_clip_layer(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer;
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
/// - Layout: Pass-through to child with same constraints
/// - Paint:
///   - If `!clip_behavior.clips()`, return child layer directly
///   - Otherwise, get child layer and wrap it with shape's clip layer
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
}

impl<S: ClipShape> RenderClip<S> {
    /// Create new RenderClip with specified shape and clip behavior
    pub fn new(shape: S, clip_behavior: Clip) -> Self {
        Self {
            shape,
            clip_behavior,
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

impl<S: ClipShape + 'static> RenderObject for RenderClip<S> {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints (pass-through)
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let child = cx.child();

        // If no clipping needed, just return child layer
        if !self.clip_behavior.clips() {
            return cx.capture_child_layer(child);
        }

        // Get child layer
        let child_layer = cx.capture_child_layer(child);

        // Get actual size from layout phase
        // Fall back to ZERO if size is not available (shouldn't happen during normal paint)
        let size = cx.size().unwrap_or(Size::ZERO);

        // Let the shape create its specific clip layer
        self.shape.create_clip_layer(child_layer, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    // Test shape implementation
    #[derive(Debug)]
    struct TestShape;

    impl ClipShape for TestShape {
        fn create_clip_layer(&self, child_layer: BoxedLayer, _size: Size) -> BoxedLayer {
            // Just wrap in a container for testing
            let mut container = ContainerLayer::new();
            container.add_child(child_layer);
            Box::new(container)
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
