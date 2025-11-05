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

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints, painting::Clip};

/// Trait for defining clip shapes
///
/// Implement this trait to define how a specific shape creates its clip layer.
/// The generic `RenderClip<S>` handles all the common clipping logic.
pub trait ClipShape: std::fmt::Debug + Send + Sync {
    /// Create a clip layer for this shape
    ///
    /// # Parameters
    ///
    /// - `child_layer`: The child_id layer to be clipped
    /// - `size`: The size of the render object (from layout)
    ///
    /// # Returns
    ///
    /// A boxed layer that clips the child_id to this shape
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

impl<S: ClipShape + 'static> SingleRender for RenderClip<S> {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child_id with same constraints (pass-through)
        let size = tree.layout_child(child_id, constraints);
        // Cache size for paint
        self.size = size;
        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // If no clipping needed, just return child_id layer
        if !self.clip_behavior.clips() {
            return tree.paint_child(child_id, offset);
        }

        // Get child_id layer (already painted at correct offset)
        let child_layer = tree.paint_child(child_id, offset);

        // Use cached size from layout phase
        // Note: Clip rect is at (0,0) in child's local coordinate space,
        // since child_layer has already been painted at the correct offset
        let size = self.size;

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
