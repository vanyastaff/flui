//! PaintingContext for recording paint commands.
//!
//! This module provides the `PaintingContext` type which manages the painting
//! of render objects into a layer tree. It uses `flui-layer`'s `LayerTree` and
//! `SceneBuilder` for constructing the compositor layer hierarchy.
//!
//! # Architecture
//!
//! ```text
//! RenderObject.paint()
//!     │
//!     ▼
//! PaintingContext
//!     │
//!     │ Uses Canvas for drawing, SceneBuilder for layers
//!     ▼
//! LayerTree + Picture layers
//! ```
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `PaintingContext` class in
//! `rendering/object.dart`.

use flui_foundation::LayerId;
use flui_layer::{
    ClipPathLayer, ClipRRectLayer, ClipRectLayer, ColorFilterLayer, Layer, LayerTree, OffsetLayer,
    OpacityLayer, PictureLayer, TransformLayer,
};
use flui_painting::DisplayListCore;
use flui_types::painting::{effects::ColorMatrix, Clip, Path};
use flui_types::{Matrix4, Offset, RRect, Rect};

use crate::traits::{RenderBox, RenderSliver};

// ============================================================================
// PaintingContext
// ============================================================================

/// A context for painting render objects.
///
/// Provides a canvas for recording paint commands and methods for
/// painting child render objects with proper layer management.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `PaintingContext` class in
/// `rendering/object.dart`.
///
/// # Usage
///
/// ```ignore
/// fn paint(&self, context: &mut PaintingContext, offset: Offset) {
///     // Paint background
///     context.canvas().draw_rect(Rect::from_size(self.size()), paint);
///
///     // Paint child
///     if let Some(child) = self.child() {
///         context.paint_child(child, offset + child_offset);
///     }
/// }
/// ```
#[derive(Debug)]
pub struct PaintingContext {
    /// The layer tree being built.
    layer_tree: LayerTree,

    /// Root layer ID for this context.
    root_layer: Option<LayerId>,

    /// Current parent layer for new layers.
    current_layer: Option<LayerId>,

    /// Estimated bounds for painting.
    estimated_bounds: Rect,

    /// Current recording canvas, if any.
    current_canvas: Option<Canvas>,

    /// Whether recording has started.
    is_recording: bool,

    /// Stack of layer IDs for push/pop operations.
    layer_stack: Vec<LayerId>,
}

impl PaintingContext {
    /// Creates a new painting context with the given bounds.
    pub fn new(estimated_bounds: Rect) -> Self {
        let mut layer_tree = LayerTree::new();

        // Create root offset layer
        let root_id = layer_tree.insert(Layer::Offset(OffsetLayer::zero()));
        layer_tree.set_root(Some(root_id));

        Self {
            layer_tree,
            root_layer: Some(root_id),
            current_layer: Some(root_id),
            estimated_bounds,
            current_canvas: None,
            is_recording: false,
            layer_stack: vec![root_id],
        }
    }

    /// Creates a painting context from an estimated bounds.
    ///
    /// Alias for `new()` for API compatibility.
    pub fn from_bounds(estimated_bounds: Rect) -> Self {
        Self::new(estimated_bounds)
    }

    /// Returns the estimated bounds for this context.
    pub fn estimated_bounds(&self) -> Rect {
        self.estimated_bounds
    }

    /// Returns a reference to the layer tree.
    pub fn layer_tree(&self) -> &LayerTree {
        &self.layer_tree
    }

    /// Returns a mutable reference to the layer tree.
    pub fn layer_tree_mut(&mut self) -> &mut LayerTree {
        &mut self.layer_tree
    }

    /// Takes ownership of the layer tree.
    pub fn into_layer_tree(mut self) -> LayerTree {
        self.stop_recording_if_needed();
        self.layer_tree
    }

    /// Returns the root layer ID.
    pub fn root_layer(&self) -> Option<LayerId> {
        self.root_layer
    }

    // ========================================================================
    // Static Repaint Methods
    // ========================================================================

    /// Repaint the given render object.
    ///
    /// The render object must be attached to a [`PipelineOwner`], must have a
    /// composited layer, and must be in need of painting. The render object's
    /// layer, if any, is re-used, along with any layers in the subtree that don't
    /// need to be repainted.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `PaintingContext.repaintCompositedChild`.
    ///
    /// # Arguments
    ///
    /// * `paint_bounds` - The bounds to use for the painting context
    /// * `painter` - A closure that performs the actual painting
    ///
    /// # Returns
    ///
    /// The [`LayerTree`] containing the painted content.
    pub fn repaint_composited_child<F>(paint_bounds: Rect, painter: F) -> LayerTree
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        tracing::debug!("repaint_composited_child bounds={:?}", paint_bounds);

        // Create painting context
        let mut context = PaintingContext::new(paint_bounds);

        // Paint the child
        painter(&mut context, Offset::ZERO);

        // Stop recording and finalize
        context.stop_recording_if_needed();

        context.layer_tree
    }

    /// Update the layer properties of a composited child without repainting.
    ///
    /// This is used when a render object's layer properties (like offset or opacity)
    /// have changed but the content itself doesn't need to be repainted.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `PaintingContext.updateLayerProperties`.
    ///
    /// # Arguments
    ///
    /// * `tree` - The layer tree containing the layer
    /// * `layer_id` - The layer ID to update
    /// * `offset` - The new offset for the layer
    pub fn update_layer_properties(tree: &mut LayerTree, layer_id: LayerId, offset: Offset) {
        tracing::debug!("update_layer_properties offset={:?}", offset);
        if let Some(layer) = tree.get_layer_mut(layer_id) {
            if let Some(offset_layer) = layer.as_offset_mut() {
                offset_layer.set_offset(offset);
            }
        }
    }

    // ========================================================================
    // Recording Management
    // ========================================================================

    /// Starts recording to a new canvas if not already recording.
    fn start_recording(&mut self) {
        if !self.is_recording {
            self.current_canvas = Some(Canvas::new());
            self.is_recording = true;
        }
    }

    /// Stops recording if currently recording and adds the picture to the layer tree.
    pub fn stop_recording_if_needed(&mut self) {
        if self.is_recording {
            self.is_recording = false;
            if let Some(canvas) = self.current_canvas.take() {
                // Create picture from canvas
                let picture = canvas.finish();
                if !picture.is_empty() {
                    let picture_layer = PictureLayer::new(picture);
                    let layer_id = self.layer_tree.insert(Layer::Picture(picture_layer));

                    // Add to current parent
                    if let Some(parent_id) = self.current_layer {
                        self.layer_tree.add_child(parent_id, layer_id);
                    }
                }
            }
        }
    }

    // ========================================================================
    // Child Painting
    // ========================================================================

    /// Paints a child box render object.
    ///
    /// This handles layer management - if the child is a repaint boundary,
    /// it will be painted into its own layer.
    pub fn paint_child(&mut self, child: &dyn RenderBox, offset: Offset) {
        // In full implementation, check if child is a repaint boundary
        // and create a new layer if needed
        let _ = (child, offset);

        // For now, just paint directly
        // child.paint(self, offset);
    }

    /// Paints a child sliver render object.
    ///
    /// Similar to `paint_child` but for sliver protocol.
    pub fn paint_sliver_child(&mut self, child: &dyn RenderSliver, offset: Offset) {
        // In full implementation, check if child is a repaint boundary
        let _ = (child, offset);

        // For now, just paint directly
        // child.paint(self, offset);
    }

    /// Paints a child into a new layer with the given offset.
    pub fn paint_child_with_offset<F>(&mut self, offset: Offset, painter: F)
    where
        F: FnOnce(&mut PaintingContext),
    {
        self.stop_recording_if_needed();

        // Create a new offset layer for the child
        let offset_layer = OffsetLayer::new(offset);
        let layer_id = self.layer_tree.insert(Layer::Offset(offset_layer));

        // Add to current parent
        if let Some(parent_id) = self.current_layer {
            self.layer_tree.add_child(parent_id, layer_id);
        }

        // Push onto stack
        let prev_layer = self.current_layer;
        self.current_layer = Some(layer_id);
        self.layer_stack.push(layer_id);

        // Paint into child context
        painter(self);

        // Stop recording in child
        self.stop_recording_if_needed();

        // Pop stack
        self.layer_stack.pop();
        self.current_layer = prev_layer;
    }

    // ========================================================================
    // Layer Operations
    // ========================================================================

    /// Pushes an opacity layer.
    ///
    /// All painting within the callback will be rendered with the given opacity.
    pub fn push_opacity<F>(&mut self, offset: Offset, alpha: u8, painter: F)
    where
        F: FnOnce(&mut PaintingContext),
    {
        self.stop_recording_if_needed();

        // Create opacity layer (alpha as f32 0.0-1.0)
        let alpha_f32 = alpha as f32 / 255.0;
        let opacity_layer = OpacityLayer::with_offset(alpha_f32, offset);
        let layer_id = self.layer_tree.insert(Layer::Opacity(opacity_layer));

        // Add to current parent
        if let Some(parent_id) = self.current_layer {
            self.layer_tree.add_child(parent_id, layer_id);
        }

        // Push onto stack
        let prev_layer = self.current_layer;
        self.current_layer = Some(layer_id);
        self.layer_stack.push(layer_id);

        // Paint within opacity
        painter(self);
        self.stop_recording_if_needed();

        // Pop stack
        self.layer_stack.pop();
        self.current_layer = prev_layer;
    }

    /// Pushes a clip rect layer.
    ///
    /// All painting within the callback will be clipped to the given rect.
    pub fn push_clip_rect<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        clip_rect: Rect,
        painter: F,
    ) where
        F: FnOnce(&mut PaintingContext),
    {
        if needs_compositing {
            self.stop_recording_if_needed();

            // Create clip rect layer
            let offset_clip = clip_rect.translate_offset(offset);
            let clip_layer = ClipRectLayer::new(offset_clip, Clip::HardEdge);
            let layer_id = self.layer_tree.insert(Layer::ClipRect(clip_layer));

            // Add to current parent
            if let Some(parent_id) = self.current_layer {
                self.layer_tree.add_child(parent_id, layer_id);
            }

            // Push onto stack
            let prev_layer = self.current_layer;
            self.current_layer = Some(layer_id);
            self.layer_stack.push(layer_id);

            painter(self);
            self.stop_recording_if_needed();

            // Pop stack
            self.layer_stack.pop();
            self.current_layer = prev_layer;
        } else {
            // Use canvas clipping
            self.canvas().save();
            self.canvas().clip_rect(clip_rect.translate_offset(offset));
            painter(self);
            self.canvas().restore();
        }
    }

    /// Pushes a clip rounded rect layer.
    ///
    /// All painting within the callback will be clipped to the given rounded rect.
    pub fn push_clip_rrect<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        clip_rrect: RRect,
        painter: F,
    ) where
        F: FnOnce(&mut PaintingContext),
    {
        if needs_compositing {
            self.stop_recording_if_needed();

            let offset_clip = clip_rrect.translate_offset(offset);
            let clip_layer = ClipRRectLayer::new(offset_clip, Clip::AntiAlias);
            let layer_id = self.layer_tree.insert(Layer::ClipRRect(clip_layer));

            // Add to current parent
            if let Some(parent_id) = self.current_layer {
                self.layer_tree.add_child(parent_id, layer_id);
            }

            // Push onto stack
            let prev_layer = self.current_layer;
            self.current_layer = Some(layer_id);
            self.layer_stack.push(layer_id);

            painter(self);
            self.stop_recording_if_needed();

            // Pop stack
            self.layer_stack.pop();
            self.current_layer = prev_layer;
        } else {
            self.canvas().save();
            self.canvas()
                .clip_rrect(clip_rrect.translate_offset(offset));
            painter(self);
            self.canvas().restore();
        }
    }

    /// Pushes a clip path layer.
    ///
    /// All painting within the callback will be clipped to the given path.
    pub fn push_clip_path<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        clip_path: Path,
        painter: F,
    ) where
        F: FnOnce(&mut PaintingContext),
    {
        if needs_compositing {
            self.stop_recording_if_needed();

            let offset_path = clip_path.translate(offset);
            let clip_layer = ClipPathLayer::new(offset_path, Clip::AntiAlias);
            let layer_id = self.layer_tree.insert(Layer::ClipPath(clip_layer));

            // Add to current parent
            if let Some(parent_id) = self.current_layer {
                self.layer_tree.add_child(parent_id, layer_id);
            }

            // Push onto stack
            let prev_layer = self.current_layer;
            self.current_layer = Some(layer_id);
            self.layer_stack.push(layer_id);

            painter(self);
            self.stop_recording_if_needed();

            // Pop stack
            self.layer_stack.pop();
            self.current_layer = prev_layer;
        } else {
            self.canvas().save();
            let translated_path = clip_path.translate(offset);
            self.canvas().clip_path(&translated_path);
            painter(self);
            self.canvas().restore();
        }
    }

    /// Pushes a transform layer.
    ///
    /// All painting within the callback will have the transform applied.
    ///
    /// Returns the layer ID if `needs_compositing` is true, for reuse in subsequent frames.
    pub fn push_transform<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        transform: &Matrix4,
        painter: F,
        _old_layer: Option<LayerId>,
    ) -> Option<LayerId>
    where
        F: FnOnce(&mut PaintingContext),
    {
        if needs_compositing {
            self.stop_recording_if_needed();

            // Create effective transform with offset
            let effective_transform = Self::compute_effective_transform(offset, transform);
            let transform_layer = TransformLayer::new(effective_transform);
            let layer_id = self.layer_tree.insert(Layer::Transform(transform_layer));

            // Add to current parent
            if let Some(parent_id) = self.current_layer {
                self.layer_tree.add_child(parent_id, layer_id);
            }

            // Push onto stack
            let prev_layer = self.current_layer;
            self.current_layer = Some(layer_id);
            self.layer_stack.push(layer_id);

            painter(self);
            self.stop_recording_if_needed();

            // Pop stack
            self.layer_stack.pop();
            self.current_layer = prev_layer;

            Some(layer_id)
        } else {
            self.canvas().save();
            self.canvas().translate(offset.dx, offset.dy);
            self.canvas().transform(*transform);
            self.canvas().translate(-offset.dx, -offset.dy);
            painter(self);
            self.canvas().restore();
            None
        }
    }

    /// Computes the effective transform matrix with offset applied.
    fn compute_effective_transform(offset: Offset, transform: &Matrix4) -> Matrix4 {
        // Translation to offset, apply transform, translate back
        let translate = Matrix4::translation(offset.dx, offset.dy, 0.0);
        let translate_back = Matrix4::translation(-offset.dx, -offset.dy, 0.0);
        translate * *transform * translate_back
    }

    /// Pushes a color filter layer.
    ///
    /// All painting within the callback will have the color filter applied.
    ///
    /// Returns the layer ID for reuse in subsequent frames.
    pub fn push_color_filter<F>(
        &mut self,
        _offset: Offset,
        color_matrix: ColorMatrix,
        painter: F,
        _old_layer: Option<LayerId>,
    ) -> LayerId
    where
        F: FnOnce(&mut PaintingContext),
    {
        self.stop_recording_if_needed();

        // Create color filter layer
        let filter_layer = ColorFilterLayer::new(color_matrix);
        let layer_id = self.layer_tree.insert(Layer::ColorFilter(filter_layer));

        // Add to current parent
        if let Some(parent_id) = self.current_layer {
            self.layer_tree.add_child(parent_id, layer_id);
        }

        // Push onto stack
        let prev_layer = self.current_layer;
        self.current_layer = Some(layer_id);
        self.layer_stack.push(layer_id);

        painter(self);
        self.stop_recording_if_needed();

        // Pop stack
        self.layer_stack.pop();
        self.current_layer = prev_layer;

        layer_id
    }

    /// Pushes a generic layer.
    ///
    /// This is the most flexible layer pushing method, allowing any Layer
    /// to be used.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `pushLayer` method.
    pub fn push_layer<F>(
        &mut self,
        layer: Layer,
        painter: F,
        _offset: Offset,
        _child_paint_bounds: Option<Rect>,
    ) where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        self.stop_recording_if_needed();

        let layer_id = self.layer_tree.insert(layer);

        // Add to current parent
        if let Some(parent_id) = self.current_layer {
            self.layer_tree.add_child(parent_id, layer_id);
        }

        // Push onto stack
        let prev_layer = self.current_layer;
        self.current_layer = Some(layer_id);
        self.layer_stack.push(layer_id);

        painter(self, Offset::ZERO);
        self.stop_recording_if_needed();

        // Pop stack
        self.layer_stack.pop();
        self.current_layer = prev_layer;
    }

    /// Adds a composited layer to the layer tree.
    pub fn add_layer(&mut self, layer: Layer) {
        self.stop_recording_if_needed();
        let layer_id = self.layer_tree.insert(layer);

        // Add to current parent
        if let Some(parent_id) = self.current_layer {
            self.layer_tree.add_child(parent_id, layer_id);
        }
    }

    // ========================================================================
    // Hints
    // ========================================================================

    /// Hints that the painting in the current layer is complex and would benefit
    /// from caching.
    ///
    /// If this hint is not set, the compositor will apply its own heuristics to
    /// decide whether the current layer is complex enough to benefit from caching.
    pub fn set_is_complex_hint(&mut self) {
        self.start_recording();
        // In full implementation, this would set a flag on the current layer
    }

    /// Hints that the painting in the current layer is likely to change next frame.
    ///
    /// This hint tells the compositor not to cache the current layer because the
    /// cache will not be used in the future.
    pub fn set_will_change_hint(&mut self) {
        self.start_recording();
        // In full implementation, this would set a flag on the current layer
    }

    // ========================================================================
    // Child Context Creation
    // ========================================================================

    /// Creates a child painting context with the given bounds.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `createChildContext` method.
    pub fn create_child_context(bounds: Rect) -> PaintingContext {
        PaintingContext::new(bounds)
    }

    // ========================================================================
    // Canvas Access
    // ========================================================================

    /// Returns a canvas for direct drawing.
    ///
    /// # Warning
    ///
    /// The canvas may change after painting children (due to layer creation).
    /// Do not cache the canvas reference across child paint calls.
    pub fn canvas(&mut self) -> &mut Canvas {
        self.start_recording();
        self.current_canvas
            .as_mut()
            .expect("Canvas should exist after start_recording")
    }

    /// Returns whether this context is currently recording to a canvas.
    #[inline]
    pub fn is_recording(&self) -> bool {
        self.is_recording
    }

    /// Returns the current canvas if recording, without starting recording.
    pub fn current_canvas(&self) -> Option<&Canvas> {
        self.current_canvas.as_ref()
    }
}

// ============================================================================
// ClipContext Implementation for PaintingContext
// ============================================================================

impl super::ClipContext for PaintingContext {
    fn canvas(&mut self) -> &mut Canvas {
        self.canvas()
    }
}

// ============================================================================
// Re-exports for convenience
// ============================================================================

// Re-export from flui_painting
pub use flui_painting::Canvas;
pub use flui_painting::Picture;

// Re-export from flui_types::painting
pub use flui_types::painting::{Paint, PaintStyle};

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Color;

    #[test]
    fn test_painting_context_new() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let context = PaintingContext::new(bounds);

        assert_eq!(context.estimated_bounds(), bounds);
        assert!(context.root_layer().is_some());
    }

    #[test]
    fn test_painting_context_canvas() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::new(bounds);

        // Should be able to get canvas
        let _canvas = context.canvas();
        assert!(context.is_recording());
    }

    #[test]
    fn test_painting_context_stop_recording() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::new(bounds);

        // Draw something
        context.canvas().draw_rect(bounds, &Paint::fill(Color::RED));

        // Stop recording
        context.stop_recording_if_needed();
        assert!(!context.is_recording());

        // Layer tree should have picture layer
        let tree = context.into_layer_tree();
        assert!(tree.len() >= 1);
    }

    #[test]
    fn test_painting_context_push_opacity() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::new(bounds);

        context.push_opacity(Offset::ZERO, 128, |ctx| {
            ctx.canvas().draw_rect(bounds, &Paint::fill(Color::BLUE));
        });

        let tree = context.into_layer_tree();
        // Should have root offset + opacity + picture
        assert!(tree.len() >= 2);
    }

    #[test]
    fn test_painting_context_push_clip_rect() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::new(bounds);

        context.push_clip_rect(true, Offset::ZERO, bounds, |ctx| {
            ctx.canvas().draw_rect(bounds, &Paint::fill(Color::GREEN));
        });

        let tree = context.into_layer_tree();
        assert!(tree.len() >= 2);
    }
}
