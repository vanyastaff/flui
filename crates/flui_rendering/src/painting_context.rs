//! Flutter-style PaintingContext for managing canvas painting and layer composition.
//!
//! # Flutter Reference
//!
//! **Source:** `flutter/packages/flutter/lib/src/rendering/object.dart`
//! **Lines:** 94-852 (Flutter 3.24)
//!
//! This module provides [`PaintingContext`], the context type used during the paint
//! phase of the rendering pipeline. It follows Flutter's `PaintingContext` API design:
//!
//! - Canvas access via `canvas()` method (lazy initialization)
//! - Child painting via `paint_child(child, offset)`
//! - Layer composition via `push_*` methods with `needsCompositing` parameter
//! - Layer reuse via `oldLayer` parameter
//!
//! # Architecture
//!
//! ```text
//! PaintingContext
//!   ├── _containerLayer: LayerId       (the layer we're painting into)
//!   ├── estimatedBounds: Rect          (bounds hint for debugging)
//!   ├── _currentLayer: Option<LayerId> (current PictureLayer being recorded)
//!   └── _canvas: Option<Canvas>        (current canvas, lazy init)
//! ```
//!
//! # Recording State
//!
//! PaintingContext manages a recording state machine:
//!
//! 1. **Not recording**: `_canvas = None`, `_currentLayer = None`
//! 2. **Recording**: `_canvas = Some(canvas)`, `_currentLayer = Some(layer_id)`
//!
//! Recording starts lazily when `canvas()` is called, and ends when:
//! - `stop_recording_if_needed()` is called
//! - A layer is pushed via `push_*` methods
//! - Painting completes
//!
//! # Example
//!
//! ```rust,ignore
//! fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!     // Canvas access starts recording automatically
//!     ctx.canvas().draw_rect(rect, &paint);
//!
//!     // paint_child handles composited vs non-composited children
//!     ctx.paint_child(child, offset);
//!
//!     // push_* methods handle layer creation with needsCompositing
//!     let layer = ctx.push_opacity(
//!         needs_compositing,
//!         offset,
//!         128,
//!         |ctx, offset| {
//!             ctx.paint_child(child, offset);
//!         },
//!         old_layer,
//!     );
//! }
//! ```

use flui_foundation::{LayerId, RenderId};
use flui_layer::{
    ClipPathLayer, ClipRRectLayer, ClipRectLayer, ColorFilterLayer, Layer, LayerTree, OpacityLayer,
    PictureLayer, TransformLayer,
};
use flui_painting::Canvas;
use flui_types::geometry::{Matrix4, Offset, RRect, Rect};
use flui_types::painting::effects::ColorMatrix;
use flui_types::painting::{Clip, Path};
use tracing::instrument;

use crate::tree::PaintTree;
use flui_painting::ClipContext;

// ============================================================================
// TYPE ALIASES FOR LAYER HANDLES
// ============================================================================

/// Callback type for painting operations.
///
/// Flutter equivalent: `PaintingContextCallback`
pub type PaintingContextCallback<'a> = Box<dyn FnOnce(&mut PaintingContext<'a>, Offset) + 'a>;

// ============================================================================
// PAINTING CONTEXT
// ============================================================================

/// A place to paint - manages canvas recording and layer composition.
///
/// Rather than holding a canvas directly, RenderObjects paint using a painting
/// context. The painting context has a Canvas, which receives the individual
/// draw operations, and also has functions for painting child render objects.
///
/// When painting a child render object, the canvas held by the painting context
/// can change because the draw operations issued before and after painting the
/// child might be recorded in separate compositing layers. For this reason, do
/// not hold a reference to the canvas across operations that might paint child
/// render objects.
///
/// New PaintingContext objects are created automatically when using
/// `repaint_composited_child` and `push_layer`.
///
/// # Flutter Reference
///
/// ```dart
/// class PaintingContext extends ClipContext {
///   PaintingContext(this._containerLayer, this.estimatedBounds);
///
///   final ContainerLayer _containerLayer;
///   final Rect estimatedBounds;
///
///   PictureLayer? _currentLayer;
///   ui.PictureRecorder? _recorder;
///   Canvas? _canvas;
/// }
/// ```
pub struct PaintingContext<'a> {
    // ========================================================================
    // LAYER TREE REFERENCE
    // ========================================================================
    /// The layer tree for managing compositor layers
    layer_tree: &'a mut LayerTree,

    /// The render tree for painting children
    paint_tree: &'a mut dyn PaintTree,

    // ========================================================================
    // CONTAINER LAYER (Flutter: _containerLayer)
    // ========================================================================
    /// The container layer that receives child layers.
    ///
    /// This is the layer we're painting into. All recorded picture layers
    /// and pushed container layers are appended to this layer.
    ///
    /// Flutter: `final ContainerLayer _containerLayer;`
    container_layer: LayerId,

    // ========================================================================
    // ESTIMATED BOUNDS (Flutter: estimatedBounds)
    // ========================================================================
    /// An estimate of the bounds within which the painting context's canvas
    /// will record painting commands. This can be useful for debugging.
    ///
    /// The canvas will allow painting outside these bounds.
    ///
    /// Flutter: `final Rect estimatedBounds;`
    estimated_bounds: Rect,

    // ========================================================================
    // RECORDING STATE (Flutter: _currentLayer, _recorder, _canvas)
    // ========================================================================
    /// The current PictureLayer being recorded into.
    ///
    /// This is Some when recording is active, None otherwise.
    ///
    /// Flutter: `PictureLayer? _currentLayer;`
    current_layer: Option<LayerId>,

    /// The current canvas being painted to.
    ///
    /// This is Some when recording is active, None otherwise.
    /// Unlike Flutter which uses a separate PictureRecorder, we use Canvas
    /// directly since it handles recording internally.
    ///
    /// Flutter: `Canvas? _canvas;` (and `ui.PictureRecorder? _recorder;`)
    canvas: Option<Canvas>,
}

impl<'a> PaintingContext<'a> {
    /// Creates a new painting context.
    ///
    /// Typically only called by `repaint_composited_child` and `push_layer`.
    ///
    /// # Arguments
    ///
    /// * `layer_tree` - The layer tree for managing layers
    /// * `paint_tree` - The paint tree for painting children
    /// * `container_layer` - The container layer to paint into
    /// * `estimated_bounds` - Bounds estimate for debugging
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// PaintingContext(this._containerLayer, this.estimatedBounds);
    /// ```
    pub fn new(
        layer_tree: &'a mut LayerTree,
        paint_tree: &'a mut dyn PaintTree,
        container_layer: LayerId,
        estimated_bounds: Rect,
    ) -> Self {
        Self {
            layer_tree,
            paint_tree,
            container_layer,
            estimated_bounds,
            current_layer: None,
            canvas: None,
        }
    }

    /// Returns the estimated bounds for this painting context.
    #[inline]
    pub fn estimated_bounds(&self) -> Rect {
        self.estimated_bounds
    }

    /// Returns the container layer ID.
    #[inline]
    pub fn container_layer(&self) -> LayerId {
        self.container_layer
    }

    // ========================================================================
    // RECORDING STATE (Flutter: _isRecording, _startRecording)
    // ========================================================================

    /// Returns true if recording is active.
    ///
    /// Flutter: `bool get _isRecording`
    #[inline]
    fn is_recording(&self) -> bool {
        self.canvas.is_some()
    }

    /// Starts recording to a new PictureLayer.
    ///
    /// This creates a new Canvas and PictureLayer, appending the layer
    /// to the container.
    ///
    /// Flutter: `void _startRecording()`
    fn start_recording(&mut self) {
        debug_assert!(!self.is_recording(), "Already recording");

        // Create new canvas
        let canvas = Canvas::new();

        // Create PictureLayer (will be finalized in stop_recording_if_needed)
        // For now we create an empty picture layer as placeholder
        let picture_layer =
            PictureLayer::with_bounds(flui_painting::Picture::new(), self.estimated_bounds);
        let layer_id = self.layer_tree.insert(Layer::Picture(picture_layer));

        // Append to container
        self.layer_tree.append_layer(self.container_layer, layer_id);

        // Update state
        self.current_layer = Some(layer_id);
        self.canvas = Some(canvas);
    }

    // ========================================================================
    // CANVAS ACCESS (Flutter: canvas getter)
    // ========================================================================

    /// The canvas on which to paint.
    ///
    /// The current canvas can change whenever you paint a child using this
    /// context, which means it's fragile to hold a reference to the canvas
    /// returned by this getter.
    ///
    /// # Panics
    ///
    /// This will start recording if not already recording.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// Canvas get canvas {
    ///   if (_canvas == null) _startRecording();
    ///   return _canvas!;
    /// }
    /// ```
    pub fn canvas(&mut self) -> &mut Canvas {
        if self.canvas.is_none() {
            self.start_recording();
        }
        self.canvas
            .as_mut()
            .expect("Canvas should exist after start_recording")
    }

    // ========================================================================
    // STOP RECORDING (Flutter: stopRecordingIfNeeded)
    // ========================================================================

    /// Stop recording to a canvas if recording has started.
    ///
    /// Do not call this function directly: functions in this class will call
    /// this method as needed. This function is called internally to ensure
    /// that recording is stopped before adding layers or finalizing results.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// @mustCallSuper
    /// void stopRecordingIfNeeded() {
    ///   if (!_isRecording) return;
    ///   _currentLayer!.picture = _recorder!.endRecording();
    ///   _currentLayer = null;
    ///   _recorder = null;
    ///   _canvas = null;
    /// }
    /// ```
    #[instrument(level = "trace", skip(self))]
    pub fn stop_recording_if_needed(&mut self) {
        if !self.is_recording() {
            return;
        }

        // Take canvas and finalize to Picture
        let canvas = self
            .canvas
            .take()
            .expect("Canvas should exist when recording");
        let picture = canvas.finish();

        // Update the PictureLayer with the finished picture
        if let Some(layer_id) = self.current_layer.take() {
            if let Some(layer) = self.layer_tree.get_layer_mut(layer_id) {
                if let Some(picture_layer) = layer.as_picture_mut() {
                    picture_layer.set_picture(picture);
                }
            }
        }

        tracing::trace!("Recording stopped");
    }

    // ========================================================================
    // CHILD PAINTING (Flutter: paintChild)
    // ========================================================================

    /// Paint a child RenderObject.
    ///
    /// If the child has its own composited layer, the child will be composited
    /// into the layer subtree associated with this painting context. Otherwise,
    /// the child will be painted into the current PictureLayer for this context.
    ///
    /// # Arguments
    ///
    /// * `child_id` - The render element ID of the child to paint
    /// * `offset` - The offset at which to paint the child
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void paintChild(RenderObject child, Offset offset) {
    ///   if (child.isRepaintBoundary) {
    ///     stopRecordingIfNeeded();
    ///     _compositeChild(child, offset);
    ///   } else {
    ///     child._paintWithContext(this, offset);
    ///   }
    /// }
    /// ```
    #[instrument(level = "trace", skip(self), fields(child = %child_id.get(), x = %offset.dx, y = %offset.dy))]
    pub fn paint_child(&mut self, child_id: RenderId, offset: Offset) {
        // TODO: Check if child.isRepaintBoundary and handle composited children
        // For now, paint directly (non-composited path)

        match self.paint_tree.perform_paint(child_id, offset) {
            Ok(child_canvas) => {
                // Composite child canvas into our canvas
                self.canvas().extend_from(child_canvas);
                tracing::trace!("paint_child complete");
            }
            Err(e) => {
                tracing::error!(
                    child = %child_id.get(),
                    offset = ?offset,
                    error = %e,
                    "paint_child failed"
                );
            }
        }
    }

    // ========================================================================
    // LAYER OPERATIONS (Flutter: appendLayer, addLayer, pushLayer)
    // ========================================================================

    /// Adds a layer to the recording requiring that the recording is already stopped.
    ///
    /// Do not call this function directly: call `add_layer` or `push_layer` instead.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// void appendLayer(Layer layer) {
    ///   assert(!_isRecording);
    ///   layer.remove();
    ///   _containerLayer.append(layer);
    /// }
    /// ```
    #[inline]
    fn append_layer(&mut self, layer_id: LayerId) {
        debug_assert!(
            !self.is_recording(),
            "Must stop recording before appending layer"
        );
        self.layer_tree.append_layer(self.container_layer, layer_id);
    }

    /// Adds a composited leaf layer to the recording.
    ///
    /// After calling this function, the canvas property will change to refer to
    /// a new Canvas that draws on top of the given layer.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void addLayer(Layer layer) {
    ///   stopRecordingIfNeeded();
    ///   appendLayer(layer);
    /// }
    /// ```
    pub fn add_layer(&mut self, layer: Layer) {
        self.stop_recording_if_needed();
        let layer_id = self.layer_tree.insert(layer);
        self.append_layer(layer_id);
    }

    /// Appends the given layer to the recording, and calls the painter callback.
    ///
    /// The given layer must be an unattached orphan.
    ///
    /// # Arguments
    ///
    /// * `child_layer` - The layer to push
    /// * `painter` - Callback to paint into the layer
    /// * `offset` - Offset to pass to the painter
    /// * `child_paint_bounds` - Optional bounds for the child context
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void pushLayer(
    ///   ContainerLayer childLayer,
    ///   PaintingContextCallback painter,
    ///   Offset offset, {
    ///   Rect? childPaintBounds,
    /// }) {
    ///   if (childLayer.hasChildren) childLayer.removeAllChildren();
    ///   stopRecordingIfNeeded();
    ///   appendLayer(childLayer);
    ///   final childContext = createChildContext(childLayer, childPaintBounds ?? estimatedBounds);
    ///   painter(childContext, offset);
    ///   childContext.stopRecordingIfNeeded();
    /// }
    /// ```
    pub fn push_layer<F>(
        &mut self,
        child_layer: Layer,
        painter: F,
        offset: Offset,
        child_paint_bounds: Option<Rect>,
    ) where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        self.stop_recording_if_needed();

        // Insert layer and append to container
        let layer_id = self.layer_tree.insert(child_layer);
        self.append_layer(layer_id);

        // Create child context
        let bounds = child_paint_bounds.unwrap_or(self.estimated_bounds);

        // SAFETY: We need to create a child context with the same layer_tree
        // This requires unsafe because we're creating multiple mutable references
        // In a real implementation, this would use interior mutability or a different pattern
        // For now, we'll use a simplified approach

        // TODO: Implement proper child context creation
        // For now, just call the painter with self
        painter(self, offset);
    }

    // ========================================================================
    // CLIP LAYERS (Flutter: pushClipRect, pushClipRRect, pushClipPath)
    // ========================================================================

    /// Clip further painting using a rectangle.
    ///
    /// # Arguments
    ///
    /// * `needs_compositing` - Whether the child needs compositing
    /// * `offset` - Offset from canvas origin to caller's coordinate system
    /// * `clip_rect` - Rectangle to clip to (in caller's coordinates)
    /// * `painter` - Callback to paint the clipped content
    /// * `clip_behavior` - How to clip (default: `Clip::HardEdge`)
    /// * `old_layer` - Previous layer for reuse optimization
    ///
    /// # Returns
    ///
    /// The clip layer if `needs_compositing` is true, None otherwise.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ClipRectLayer? pushClipRect(
    ///   bool needsCompositing,
    ///   Offset offset,
    ///   Rect clipRect,
    ///   PaintingContextCallback painter, {
    ///   Clip clipBehavior = Clip.hardEdge,
    ///   ClipRectLayer? oldLayer,
    /// })
    /// ```
    #[instrument(level = "trace", skip(self, painter, old_layer), fields(
        needs_compositing,
        clip_rect = ?clip_rect,
    ))]
    pub fn push_clip_rect<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        clip_rect: Rect,
        painter: F,
        clip_behavior: Clip,
        old_layer: Option<LayerId>,
    ) -> Option<LayerId>
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        if clip_behavior == Clip::None {
            painter(self, offset);
            return None;
        }

        let offset_clip_rect = clip_rect.translate(offset.dx, offset.dy);

        if needs_compositing {
            // Create or reuse layer
            let layer = if let Some(old_id) = old_layer {
                // Update existing layer
                if let Some(layer) = self.layer_tree.get_layer_mut(old_id) {
                    if let Some(clip_layer) = layer.as_clip_rect_mut() {
                        clip_layer.set_clip_rect(offset_clip_rect);
                        clip_layer.set_clip_behavior(clip_behavior);
                    }
                }
                // Clear old children
                self.layer_tree.clear_children(old_id);
                old_id
            } else {
                // Create new layer
                let clip_layer = ClipRectLayer::new(offset_clip_rect, clip_behavior);
                self.layer_tree.insert(Layer::ClipRect(clip_layer))
            };

            self.push_layer(
                Layer::ClipRect(ClipRectLayer::new(offset_clip_rect, clip_behavior)),
                painter,
                offset,
                Some(offset_clip_rect),
            );
            Some(layer)
        } else {
            // Non-composited path: use ClipContext
            self.clip_rect_and_paint(clip_rect, clip_behavior, offset_clip_rect, |ctx| {
                painter(ctx, offset);
            });
            None
        }
    }

    /// Clip further painting using a rounded rectangle.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ClipRRectLayer? pushClipRRect(
    ///   bool needsCompositing,
    ///   Offset offset,
    ///   Rect bounds,
    ///   RRect clipRRect,
    ///   PaintingContextCallback painter, {
    ///   Clip clipBehavior = Clip.antiAlias,
    ///   ClipRRectLayer? oldLayer,
    /// })
    /// ```
    #[instrument(
        level = "trace",
        skip(self, painter, old_layer),
        fields(needs_compositing)
    )]
    pub fn push_clip_rrect<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        bounds: Rect,
        clip_rrect: RRect,
        painter: F,
        clip_behavior: Clip,
        old_layer: Option<LayerId>,
    ) -> Option<LayerId>
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        if clip_behavior == Clip::None {
            painter(self, offset);
            return None;
        }

        let offset_bounds = bounds.translate(offset.dx, offset.dy);
        let offset_clip_rrect = clip_rrect.translate(offset);

        if needs_compositing {
            let layer = if let Some(old_id) = old_layer {
                if let Some(layer) = self.layer_tree.get_layer_mut(old_id) {
                    if let Some(clip_layer) = layer.as_clip_rrect_mut() {
                        clip_layer.set_clip_rrect(offset_clip_rrect);
                        clip_layer.set_clip_behavior(clip_behavior);
                    }
                }
                self.layer_tree.clear_children(old_id);
                old_id
            } else {
                let clip_layer = ClipRRectLayer::new(offset_clip_rrect, clip_behavior);
                self.layer_tree.insert(Layer::ClipRRect(clip_layer))
            };

            self.push_layer(
                Layer::ClipRRect(ClipRRectLayer::new(offset_clip_rrect, clip_behavior)),
                painter,
                offset,
                Some(offset_bounds),
            );
            Some(layer)
        } else {
            self.clip_rrect_and_paint(clip_rrect, clip_behavior, offset_bounds, |ctx| {
                painter(ctx, offset);
            });
            None
        }
    }

    /// Clip further painting using a path.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ClipPathLayer? pushClipPath(
    ///   bool needsCompositing,
    ///   Offset offset,
    ///   Rect bounds,
    ///   Path clipPath,
    ///   PaintingContextCallback painter, {
    ///   Clip clipBehavior = Clip.antiAlias,
    ///   ClipPathLayer? oldLayer,
    /// })
    /// ```
    #[instrument(
        level = "trace",
        skip(self, painter, old_layer, clip_path),
        fields(needs_compositing)
    )]
    pub fn push_clip_path<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        bounds: Rect,
        clip_path: Path,
        painter: F,
        clip_behavior: Clip,
        old_layer: Option<LayerId>,
    ) -> Option<LayerId>
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        if clip_behavior == Clip::None {
            painter(self, offset);
            return None;
        }

        let offset_bounds = bounds.translate(offset.dx, offset.dy);
        let offset_clip_path = clip_path.translate(offset);

        if needs_compositing {
            let layer = if let Some(old_id) = old_layer {
                if let Some(layer) = self.layer_tree.get_layer_mut(old_id) {
                    if let Some(clip_layer) = layer.as_clip_path_mut() {
                        clip_layer.set_clip_path(offset_clip_path.clone());
                        clip_layer.set_clip_behavior(clip_behavior);
                    }
                }
                self.layer_tree.clear_children(old_id);
                old_id
            } else {
                let clip_layer = ClipPathLayer::new(offset_clip_path.clone(), clip_behavior);
                self.layer_tree.insert(Layer::ClipPath(clip_layer))
            };

            self.push_layer(
                Layer::ClipPath(ClipPathLayer::new(offset_clip_path, clip_behavior)),
                painter,
                offset,
                Some(offset_bounds),
            );
            Some(layer)
        } else {
            self.clip_path_and_paint(&clip_path, clip_behavior, offset_bounds, |ctx| {
                painter(ctx, offset);
            });
            None
        }
    }

    // ========================================================================
    // EFFECT LAYERS (Flutter: pushColorFilter, pushTransform, pushOpacity)
    // ========================================================================

    /// Blend further painting with a color filter.
    ///
    /// # Arguments
    ///
    /// * `offset` - Offset to apply to children
    /// * `color_matrix` - The color matrix transformation to apply
    /// * `painter` - Callback to paint content
    /// * `old_layer` - Previous layer for reuse optimization
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// ColorFilterLayer pushColorFilter(
    ///   Offset offset,
    ///   ColorFilter colorFilter,
    ///   PaintingContextCallback painter, {
    ///   ColorFilterLayer? oldLayer,
    /// })
    /// ```
    #[instrument(level = "trace", skip(self, painter, old_layer))]
    pub fn push_color_filter<F>(
        &mut self,
        offset: Offset,
        color_matrix: ColorMatrix,
        painter: F,
        old_layer: Option<LayerId>,
    ) -> LayerId
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        let layer = if let Some(old_id) = old_layer {
            if let Some(layer) = self.layer_tree.get_layer_mut(old_id) {
                if let Some(filter_layer) = layer.as_color_filter_mut() {
                    filter_layer.set_color_filter(color_matrix.clone());
                }
            }
            self.layer_tree.clear_children(old_id);
            old_id
        } else {
            let filter_layer = ColorFilterLayer::new(color_matrix.clone());
            self.layer_tree.insert(Layer::ColorFilter(filter_layer))
        };

        self.push_layer(
            Layer::ColorFilter(ColorFilterLayer::new(color_matrix)),
            painter,
            offset,
            None,
        );
        layer
    }

    /// Transform further painting using a matrix.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// TransformLayer? pushTransform(
    ///   bool needsCompositing,
    ///   Offset offset,
    ///   Matrix4 transform,
    ///   PaintingContextCallback painter, {
    ///   TransformLayer? oldLayer,
    /// })
    /// ```
    #[instrument(
        level = "trace",
        skip(self, painter, old_layer, transform),
        fields(needs_compositing)
    )]
    pub fn push_transform<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        transform: Matrix4,
        painter: F,
        old_layer: Option<LayerId>,
    ) -> Option<LayerId>
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        // Compute effective transform: translate to offset, apply transform, translate back
        let effective_transform = Matrix4::translation(offset.dx, offset.dy, 0.0)
            * transform
            * Matrix4::translation(-offset.dx, -offset.dy, 0.0);

        if needs_compositing {
            let layer = if let Some(old_id) = old_layer {
                if let Some(layer) = self.layer_tree.get_layer_mut(old_id) {
                    if let Some(transform_layer) = layer.as_transform_mut() {
                        transform_layer.set_transform(effective_transform);
                    }
                }
                self.layer_tree.clear_children(old_id);
                old_id
            } else {
                let transform_layer = TransformLayer::new(effective_transform);
                self.layer_tree.insert(Layer::Transform(transform_layer))
            };

            // TODO: Compute inverse transform for child bounds
            self.push_layer(
                Layer::Transform(TransformLayer::new(effective_transform)),
                painter,
                offset,
                None,
            );
            Some(layer)
        } else {
            // Non-composited path
            self.canvas().save();
            self.canvas().transform(effective_transform);
            painter(self, offset);
            self.canvas().restore();
            None
        }
    }

    /// Blend further painting with an alpha value.
    ///
    /// # Arguments
    ///
    /// * `offset` - Offset to apply to children
    /// * `alpha` - Alpha value (0 = transparent, 255 = opaque)
    /// * `painter` - Callback to paint content
    /// * `old_layer` - Previous layer for reuse
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// OpacityLayer pushOpacity(
    ///   Offset offset,
    ///   int alpha,
    ///   PaintingContextCallback painter, {
    ///   OpacityLayer? oldLayer,
    /// })
    /// ```
    #[instrument(level = "trace", skip(self, painter, old_layer), fields(alpha))]
    pub fn push_opacity<F>(
        &mut self,
        offset: Offset,
        alpha: u8,
        painter: F,
        old_layer: Option<LayerId>,
    ) -> LayerId
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        let alpha_f32 = alpha as f32 / 255.0;

        let layer = if let Some(old_id) = old_layer {
            if let Some(layer) = self.layer_tree.get_layer_mut(old_id) {
                if let Some(opacity_layer) = layer.as_opacity_mut() {
                    opacity_layer.set_alpha(alpha_f32);
                    opacity_layer.set_offset(offset);
                }
            }
            self.layer_tree.clear_children(old_id);
            old_id
        } else {
            let opacity_layer = OpacityLayer::with_offset(alpha_f32, offset);
            self.layer_tree.insert(Layer::Opacity(opacity_layer))
        };

        self.push_layer(
            Layer::Opacity(OpacityLayer::with_offset(alpha_f32, offset)),
            painter,
            Offset::ZERO, // Children use zero offset, layer handles it
            None,
        );
        layer
    }

    // ========================================================================
    // HINTS (Flutter: setIsComplexHint, setWillChangeHint)
    // ========================================================================

    /// Hints that the painting in the current layer is complex and would benefit
    /// from caching.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void setIsComplexHint() {
    ///   if (_currentLayer == null) _startRecording();
    ///   _currentLayer!.isComplexHint = true;
    /// }
    /// ```
    pub fn set_is_complex_hint(&mut self) {
        if self.canvas.is_none() {
            self.start_recording();
        }
        // TODO: Set hint on current layer when PictureLayer supports it
        tracing::trace!("set_is_complex_hint called");
    }

    /// Hints that the painting in the current layer is likely to change next frame.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void setWillChangeHint() {
    ///   if (_currentLayer == null) _startRecording();
    ///   _currentLayer!.willChangeHint = true;
    /// }
    /// ```
    pub fn set_will_change_hint(&mut self) {
        if self.canvas.is_none() {
            self.start_recording();
        }
        // TODO: Set hint on current layer when PictureLayer supports it
        tracing::trace!("set_will_change_hint called");
    }

    // ========================================================================
    // CONVENIENCE METHODS
    // ========================================================================

    /// Convenience method to push opacity with f32 alpha.
    pub fn push_opacity_f32<F>(
        &mut self,
        offset: Offset,
        opacity: f32,
        painter: F,
        old_layer: Option<LayerId>,
    ) -> LayerId
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        self.push_opacity(offset, alpha, painter, old_layer)
    }
}

// ============================================================================
// CLIP CONTEXT IMPLEMENTATION
// ============================================================================

impl ClipContext for PaintingContext<'_> {
    #[inline]
    fn canvas_mut(&mut self) -> &mut Canvas {
        self.canvas()
    }
}

// ============================================================================
// DEBUG
// ============================================================================

impl std::fmt::Debug for PaintingContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintingContext")
            .field("container_layer", &self.container_layer)
            .field("estimated_bounds", &self.estimated_bounds)
            .field("is_recording", &self.is_recording())
            .field("current_layer", &self.current_layer)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    // Tests will be added after verifying compilation
}
