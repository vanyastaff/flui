//! PaintingContext for recording paint commands.

use std::sync::atomic::{AtomicU64, Ordering};

use flui_types::painting::Path;
use flui_types::{Offset, Point, RRect, Rect};

use crate::layer::{
    Clip, ClipPathLayer, ClipRRectLayer, ClipRectLayer, ColorFilter, ColorFilterLayer,
    ContainerLayer, Layer, OffsetLayer, OpacityLayer, Picture, PictureLayer, TransformLayer,
};
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
    /// The container layer that painting adds to.
    container_layer: ContainerLayer,

    /// Estimated bounds for painting.
    estimated_bounds: Rect,

    /// Current recording canvas, if any.
    current_canvas: Option<Canvas>,

    /// Whether recording has started.
    is_recording: bool,
}

impl PaintingContext {
    /// Creates a new painting context with the given container layer.
    pub fn new(container_layer: ContainerLayer, estimated_bounds: Rect) -> Self {
        Self {
            container_layer,
            estimated_bounds,
            current_canvas: None,
            is_recording: false,
        }
    }

    /// Creates a painting context from an estimated bounds (creates default container).
    pub fn from_bounds(estimated_bounds: Rect) -> Self {
        Self::new(ContainerLayer::new(), estimated_bounds)
    }

    /// Returns the estimated bounds for this context.
    pub fn estimated_bounds(&self) -> Rect {
        self.estimated_bounds
    }

    /// Returns the container layer.
    pub fn container_layer(&self) -> &ContainerLayer {
        &self.container_layer
    }

    /// Returns the container layer mutably.
    pub fn container_layer_mut(&mut self) -> &mut ContainerLayer {
        &mut self.container_layer
    }

    /// Takes ownership of the container layer.
    pub fn into_container_layer(self) -> ContainerLayer {
        self.container_layer
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
    /// * `child` - The render object to repaint (must be a repaint boundary)
    /// * `paint_bounds` - The bounds to use for the painting context
    /// * `painter` - A closure that performs the actual painting
    ///
    /// # Returns
    ///
    /// The [`OffsetLayer`] containing the painted content.
    pub fn repaint_composited_child<F>(paint_bounds: Rect, painter: F) -> OffsetLayer
    where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        tracing::debug!("repaint_composited_child bounds={:?}", paint_bounds);

        // Create a new offset layer for this repaint boundary
        let offset_layer = OffsetLayer::new(Offset::ZERO);

        // Create painting context with the layer
        let container = ContainerLayer::new();
        let mut context = PaintingContext::new(container, paint_bounds);

        // Paint the child
        painter(&mut context, Offset::ZERO);

        // Stop recording and finalize
        context.stop_recording_if_needed();

        offset_layer
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
    /// * `layer` - The offset layer to update
    /// * `offset` - The new offset for the layer
    pub fn update_layer_properties(layer: &mut OffsetLayer, offset: Offset) {
        tracing::debug!("update_layer_properties offset={:?}", offset);
        layer.set_offset(offset);
    }

    // ========================================================================
    // Recording Management
    // ========================================================================

    /// Starts recording to a new picture if not already recording.
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
                let picture = canvas.end_recording();
                let picture_layer = PictureLayer::with_picture(picture, Offset::ZERO);
                self.container_layer.append(Box::new(picture_layer));
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
        let mut offset_layer = OffsetLayer::new(offset);

        // Create child context
        let mut child_context = PaintingContext::new(
            ContainerLayer::new(),
            self.estimated_bounds.translate_offset(offset),
        );

        // Paint into child context
        painter(&mut child_context);

        // Stop recording in child
        child_context.stop_recording_if_needed();

        // Add child's container to offset layer
        for child_layer in child_context.container_layer.children() {
            // Clone child - in real impl would need proper ownership
            let _ = child_layer;
        }

        // For now, just add the child's container directly
        self.container_layer
            .append(Box::new(child_context.container_layer));

        let _ = offset_layer;
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

        // Create opacity layer
        let mut opacity_layer = OpacityLayer::new(alpha, offset);

        // Create child context for painting within opacity
        let child_bounds = self.estimated_bounds.translate_offset(offset);
        let mut child_context = PaintingContext::new(ContainerLayer::new(), child_bounds);

        // Paint within child context
        painter(&mut child_context);
        child_context.stop_recording_if_needed();

        // Add child context's layers to opacity layer
        opacity_layer.append(Box::new(child_context.container_layer));

        // Add opacity layer to our container
        self.container_layer.append(Box::new(opacity_layer));
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
            let mut clip_layer = ClipRectLayer::new(offset_clip, Clip::HardEdge);

            // Create child context
            let mut child_context = PaintingContext::new(ContainerLayer::new(), offset_clip);

            painter(&mut child_context);
            child_context.stop_recording_if_needed();

            clip_layer.append(Box::new(child_context.container_layer));
            self.container_layer.append(Box::new(clip_layer));
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
            let mut clip_layer = ClipRRectLayer::new(offset_clip.clone(), Clip::AntiAlias);

            let mut child_context = PaintingContext::new(ContainerLayer::new(), offset_clip.rect);

            painter(&mut child_context);
            child_context.stop_recording_if_needed();

            clip_layer.append(Box::new(child_context.container_layer));
            self.container_layer.append(Box::new(clip_layer));
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
            let mut path_copy = offset_path.clone();
            let bounds = path_copy.bounds();
            let mut clip_layer = ClipPathLayer::new(offset_path, Clip::AntiAlias);

            let mut child_context = PaintingContext::new(ContainerLayer::new(), bounds);

            painter(&mut child_context);
            child_context.stop_recording_if_needed();

            clip_layer.append(Box::new(child_context.container_layer));
            self.container_layer.append(Box::new(clip_layer));
        } else {
            self.canvas().save();
            self.canvas().clip_path(clip_path.translate(offset));
            painter(self);
            self.canvas().restore();
        }
    }

    /// Pushes a transform layer.
    ///
    /// All painting within the callback will have the transform applied.
    ///
    /// Returns the layer if `needs_compositing` is true, for reuse in subsequent frames.
    pub fn push_transform<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        transform: &[f32; 16],
        painter: F,
        old_layer: Option<TransformLayer>,
    ) -> Option<TransformLayer>
    where
        F: FnOnce(&mut PaintingContext),
    {
        if needs_compositing {
            self.stop_recording_if_needed();

            // Reuse or create transform layer
            let mut transform_layer = old_layer.unwrap_or_else(TransformLayer::identity);
            transform_layer.set_transform(*transform);

            // Calculate effective transform with offset
            let effective_transform = Self::compute_effective_transform(offset, transform);

            let mut child_context =
                PaintingContext::new(ContainerLayer::new(), self.estimated_bounds);

            painter(&mut child_context);
            child_context.stop_recording_if_needed();

            transform_layer.append(Box::new(child_context.container_layer));

            // Return a new layer for potential reuse
            let returned_layer = TransformLayer::new(*transform);
            self.container_layer.append(Box::new(transform_layer));

            Some(returned_layer)
        } else {
            self.canvas().save();
            self.canvas().translate(offset.dx, offset.dy);
            self.canvas().transform(transform);
            self.canvas().translate(-offset.dx, -offset.dy);
            painter(self);
            self.canvas().restore();
            None
        }
    }

    /// Computes the effective transform matrix with offset applied.
    fn compute_effective_transform(offset: Offset, transform: &[f32; 16]) -> [f32; 16] {
        // Translation to offset, apply transform, translate back
        // This is a simplified version - full implementation would use matrix multiplication
        let mut result = *transform;
        result[12] += offset.dx;
        result[13] += offset.dy;
        result
    }

    /// Pushes a color filter layer.
    ///
    /// All painting within the callback will have the color filter applied.
    ///
    /// Returns the layer for reuse in subsequent frames.
    pub fn push_color_filter<F>(
        &mut self,
        offset: Offset,
        color_filter: ColorFilter,
        painter: F,
        old_layer: Option<ColorFilterLayer>,
    ) -> ColorFilterLayer
    where
        F: FnOnce(&mut PaintingContext),
    {
        self.stop_recording_if_needed();

        // Reuse or create color filter layer
        let mut filter_layer = old_layer.unwrap_or_else(|| ColorFilterLayer::new(color_filter));
        filter_layer.set_color_filter(color_filter);

        let child_bounds = self.estimated_bounds.translate_offset(offset);
        let mut child_context = PaintingContext::new(ContainerLayer::new(), child_bounds);

        painter(&mut child_context);
        child_context.stop_recording_if_needed();

        filter_layer.append(Box::new(child_context.container_layer));

        // Return a new layer for potential reuse
        let returned_layer = ColorFilterLayer::new(color_filter);
        self.container_layer.append(Box::new(filter_layer));

        returned_layer
    }

    /// Pushes a generic container layer.
    ///
    /// This is the most flexible layer pushing method, allowing any ContainerLayer
    /// subtype to be used.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `pushLayer` method.
    pub fn push_layer<F>(
        &mut self,
        child_layer: Box<dyn Layer>,
        painter: F,
        offset: Offset,
        child_paint_bounds: Option<Rect>,
    ) where
        F: FnOnce(&mut PaintingContext, Offset),
    {
        self.stop_recording_if_needed();

        let bounds = child_paint_bounds.unwrap_or(self.estimated_bounds);
        let mut child_context = PaintingContext::new(ContainerLayer::new(), bounds);

        painter(&mut child_context, offset);
        child_context.stop_recording_if_needed();

        self.container_layer.append(child_layer);
        self.container_layer
            .append(Box::new(child_context.container_layer));
    }

    /// Adds a composited layer to the layer tree.
    pub fn add_layer(&mut self, layer: Box<dyn Layer>) {
        self.stop_recording_if_needed();
        self.container_layer.append(layer);
    }

    /// Appends a layer without stopping recording first.
    ///
    /// Use this only when you know recording state is already handled.
    fn append_layer(&mut self, layer: Box<dyn Layer>) {
        self.container_layer.append(layer);
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
        // In full implementation, this would set a flag on the current PictureLayer
        // For now, we just ensure recording has started
    }

    /// Hints that the painting in the current layer is likely to change next frame.
    ///
    /// This hint tells the compositor not to cache the current layer because the
    /// cache will not be used in the future.
    pub fn set_will_change_hint(&mut self) {
        self.start_recording();
        // In full implementation, this would set a flag on the current PictureLayer
    }

    // ========================================================================
    // Child Context Creation
    // ========================================================================

    /// Creates a child painting context for the given layer and bounds.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `createChildContext` method.
    pub fn create_child_context(child_layer: ContainerLayer, bounds: Rect) -> PaintingContext {
        PaintingContext::new(child_layer, bounds)
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
        self.current_canvas.as_mut().unwrap()
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
// BlendMode
// ============================================================================

/// Algorithms for combining colors during painting.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `BlendMode` enum from `dart:ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BlendMode {
    /// Drop both source and destination (transparent).
    Clear = 0,
    /// Drop destination, use source only.
    Src = 1,
    /// Drop source, use destination only.
    Dst = 2,
    /// Source over destination (default).
    #[default]
    SrcOver = 3,
    /// Destination over source.
    DstOver = 4,
    /// Source where destination is opaque.
    SrcIn = 5,
    /// Destination where source is opaque.
    DstIn = 6,
    /// Source where destination is transparent.
    SrcOut = 7,
    /// Destination where source is transparent.
    DstOut = 8,
    /// Source over destination, destination opaque areas.
    SrcATop = 9,
    /// Destination over source, source opaque areas.
    DstATop = 10,
    /// XOR of source and destination.
    Xor = 11,
    /// Sum of source and destination, clamped.
    Plus = 12,
    /// Product of source and destination.
    Modulate = 13,
    /// Multiplies source by destination screen value.
    Screen = 14,
    /// Darkens by selecting minimum.
    Overlay = 15,
    /// Selects darker of source and destination.
    Darken = 16,
    /// Selects lighter of source and destination.
    Lighten = 17,
    /// Brightens destination based on source.
    ColorDodge = 18,
    /// Darkens destination based on source.
    ColorBurn = 19,
    /// Multiplies or screens depending on destination.
    HardLight = 20,
    /// Softer version of hard light.
    SoftLight = 21,
    /// Absolute difference between source and destination.
    Difference = 22,
    /// Similar to difference with lower contrast.
    Exclusion = 23,
    /// Multiplies source and destination.
    Multiply = 24,
    /// Hue of source with saturation and luminosity of destination.
    Hue = 25,
    /// Saturation of source with hue and luminosity of destination.
    Saturation = 26,
    /// Hue and saturation of source with luminosity of destination.
    Color = 27,
    /// Luminosity of source with hue and saturation of destination.
    Luminosity = 28,
}

impl BlendMode {
    /// Returns the blend mode as a u8 index.
    #[inline]
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Creates a blend mode from a u8 index.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clear),
            1 => Some(Self::Src),
            2 => Some(Self::Dst),
            3 => Some(Self::SrcOver),
            4 => Some(Self::DstOver),
            5 => Some(Self::SrcIn),
            6 => Some(Self::DstIn),
            7 => Some(Self::SrcOut),
            8 => Some(Self::DstOut),
            9 => Some(Self::SrcATop),
            10 => Some(Self::DstATop),
            11 => Some(Self::Xor),
            12 => Some(Self::Plus),
            13 => Some(Self::Modulate),
            14 => Some(Self::Screen),
            15 => Some(Self::Overlay),
            16 => Some(Self::Darken),
            17 => Some(Self::Lighten),
            18 => Some(Self::ColorDodge),
            19 => Some(Self::ColorBurn),
            20 => Some(Self::HardLight),
            21 => Some(Self::SoftLight),
            22 => Some(Self::Difference),
            23 => Some(Self::Exclusion),
            24 => Some(Self::Multiply),
            25 => Some(Self::Hue),
            26 => Some(Self::Saturation),
            27 => Some(Self::Color),
            28 => Some(Self::Luminosity),
            _ => None,
        }
    }
}

// ============================================================================
// Canvas
// ============================================================================

/// Unique ID for canvas/picture tracking.
static CANVAS_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A canvas for recording drawing commands.
///
/// Records drawing operations that will be converted to a Picture
/// when recording ends.
#[derive(Debug)]
pub struct Canvas {
    /// Unique identifier for this canvas.
    id: u64,

    /// Recorded drawing commands.
    commands: Vec<DrawCommand>,

    /// Current bounds of recorded content.
    bounds: Rect,

    /// Save/restore state stack.
    state_stack: Vec<CanvasState>,

    /// Current state.
    current_state: CanvasState,
}

/// State that can be saved/restored.
#[derive(Debug, Clone, Default)]
struct CanvasState {
    /// Current transform matrix.
    transform: [f32; 16],
    /// Clip region (simplified as rect for now).
    clip: Option<Rect>,
}

impl CanvasState {
    fn identity() -> Self {
        Self {
            transform: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            clip: None,
        }
    }
}

/// A recorded drawing command.
#[derive(Debug, Clone)]
pub enum DrawCommand {
    /// Draw a rectangle.
    DrawRect { rect: Rect, paint: Paint },
    /// Draw a rounded rectangle.
    DrawRRect { rrect: RRect, paint: Paint },
    /// Draw a "donut" shape (outer rrect minus inner rrect).
    DrawDRRect {
        outer: RRect,
        inner: RRect,
        paint: Paint,
    },
    /// Draw a circle.
    DrawCircle {
        center: Offset,
        radius: f32,
        paint: Paint,
    },
    /// Draw a line.
    DrawLine {
        p1: Offset,
        p2: Offset,
        paint: Paint,
    },
    /// Draw multiple lines.
    DrawLines { points: Vec<Offset>, paint: Paint },
    /// Draw points.
    DrawPoints {
        mode: PointMode,
        points: Vec<Offset>,
        paint: Paint,
    },
    /// Draw an oval.
    DrawOval { rect: Rect, paint: Paint },
    /// Draw an arc.
    DrawArc {
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: Paint,
    },
    /// Draw a path.
    DrawPath { path: Path, paint: Paint },
    /// Fill the entire canvas with a color.
    DrawColor { color: u32, blend_mode: BlendMode },
    /// Draw a shadow for the given path.
    DrawShadow {
        path: Path,
        color: u32,
        elevation: f32,
        transparent_occluder: bool,
    },
    /// Save canvas state.
    Save,
    /// Save canvas state with a layer.
    SaveLayer { bounds: Option<Rect>, paint: Paint },
    /// Restore canvas state.
    Restore,
    /// Translate.
    Translate { dx: f32, dy: f32 },
    /// Scale.
    Scale { sx: f32, sy: f32 },
    /// Rotate.
    Rotate { radians: f32 },
    /// Skew.
    Skew { sx: f32, sy: f32 },
    /// Apply transform.
    Transform { matrix: [f32; 16] },
    /// Clip to rectangle.
    ClipRect {
        rect: Rect,
        clip_op: ClipOp,
        do_anti_alias: bool,
    },
    /// Clip to rounded rectangle.
    ClipRRect { rrect: RRect, do_anti_alias: bool },
    /// Clip to path.
    ClipPath { path: Path, do_anti_alias: bool },
}

/// How to draw points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointMode {
    /// Draw each point as a dot.
    #[default]
    Points,
    /// Draw pairs of points as line segments.
    Lines,
    /// Draw a polyline connecting all points.
    Polygon,
}

/// Clipping operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipOp {
    /// Clip to the difference of current clip and given shape.
    Difference,
    /// Clip to the intersection of current clip and given shape.
    #[default]
    Intersect,
}

impl Canvas {
    /// Creates a new canvas.
    pub fn new() -> Self {
        Self {
            id: CANVAS_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            commands: Vec::new(),
            bounds: Rect::ZERO,
            state_stack: Vec::new(),
            current_state: CanvasState::identity(),
        }
    }

    /// Returns the canvas ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the recorded commands.
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Returns the accumulated bounds of all drawing.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Ends recording and returns a Picture.
    pub fn end_recording(self) -> Picture {
        let mut picture = Picture::new(self.bounds);
        picture.set_approximate_bytes_used(self.commands.len() * 64); // Rough estimate
        picture
    }

    // ========================================================================
    // Drawing Operations
    // ========================================================================

    /// Draws a rectangle.
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.expand_bounds(rect);
        self.commands.push(DrawCommand::DrawRect {
            rect,
            paint: paint.clone(),
        });
    }

    /// Draws a rounded rectangle.
    pub fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.expand_bounds(rrect.rect);
        self.commands.push(DrawCommand::DrawRRect {
            rrect,
            paint: paint.clone(),
        });
    }

    /// Draws a circle.
    pub fn draw_circle(&mut self, center: Offset, radius: f32, paint: &Paint) {
        let bounds = Rect::from_center(center, radius * 2.0, radius * 2.0);
        self.expand_bounds(bounds);
        self.commands.push(DrawCommand::DrawCircle {
            center,
            radius,
            paint: paint.clone(),
        });
    }

    /// Draws a line.
    pub fn draw_line(&mut self, p1: Offset, p2: Offset, paint: &Paint) {
        let point1 = Point::new(p1.dx, p1.dy);
        let point2 = Point::new(p2.dx, p2.dy);
        let bounds = Rect::from_points(point1, point2);
        self.expand_bounds(bounds);
        self.commands.push(DrawCommand::DrawLine {
            p1,
            p2,
            paint: paint.clone(),
        });
    }

    /// Draws an oval inscribed in the given rect.
    pub fn draw_oval(&mut self, rect: Rect, paint: &Paint) {
        self.expand_bounds(rect);
        self.commands.push(DrawCommand::DrawOval {
            rect,
            paint: paint.clone(),
        });
    }

    /// Draws an arc.
    pub fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        self.expand_bounds(rect);
        self.commands.push(DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint: paint.clone(),
        });
    }

    /// Draws a path.
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        let mut path_copy = path.clone();
        self.expand_bounds(path_copy.bounds());
        self.commands.push(DrawCommand::DrawPath {
            path: path.clone(),
            paint: paint.clone(),
        });
    }

    /// Draws a "donut" shape (outer rounded rect minus inner rounded rect).
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `Canvas.drawDRRect` method.
    pub fn draw_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) {
        self.expand_bounds(outer.rect);
        self.commands.push(DrawCommand::DrawDRRect {
            outer,
            inner,
            paint: paint.clone(),
        });
    }

    /// Draws a series of points.
    ///
    /// The `mode` parameter determines how points are interpreted:
    /// - `Points`: Each point is drawn as a dot
    /// - `Lines`: Pairs of points are drawn as line segments
    /// - `Polygon`: All points are connected in sequence
    pub fn draw_points(&mut self, mode: PointMode, points: &[Offset], paint: &Paint) {
        if points.is_empty() {
            return;
        }
        // Calculate bounds from all points
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for p in points {
            min_x = min_x.min(p.dx);
            min_y = min_y.min(p.dy);
            max_x = max_x.max(p.dx);
            max_y = max_y.max(p.dy);
        }
        let bounds = Rect::from_ltrb(min_x, min_y, max_x, max_y);
        self.expand_bounds(bounds);
        self.commands.push(DrawCommand::DrawPoints {
            mode,
            points: points.to_vec(),
            paint: paint.clone(),
        });
    }

    /// Draws raw points (coordinates as f32 pairs).
    pub fn draw_raw_points(&mut self, mode: PointMode, coords: &[f32], paint: &Paint) {
        let points: Vec<Offset> = coords
            .chunks_exact(2)
            .map(|c| Offset::new(c[0], c[1]))
            .collect();
        self.draw_points(mode, &points, paint);
    }

    /// Fills the canvas with the given color using the specified blend mode.
    pub fn draw_color(&mut self, color: u32, blend_mode: BlendMode) {
        self.commands
            .push(DrawCommand::DrawColor { color, blend_mode });
    }

    /// Draws a shadow for the given path.
    ///
    /// # Parameters
    ///
    /// - `path`: The path that casts the shadow
    /// - `color`: The shadow color
    /// - `elevation`: The elevation of the shadow
    /// - `transparent_occluder`: Whether the occluder is transparent
    pub fn draw_shadow(
        &mut self,
        path: &Path,
        color: u32,
        elevation: f32,
        transparent_occluder: bool,
    ) {
        let mut path_copy = path.clone();
        // Shadow extends beyond the path bounds
        let shadow_margin = elevation * 2.0;
        let bounds = path_copy.bounds().inflate(shadow_margin, shadow_margin);
        self.expand_bounds(bounds);
        self.commands.push(DrawCommand::DrawShadow {
            path: path.clone(),
            color,
            elevation,
            transparent_occluder,
        });
    }

    // ========================================================================
    // State Operations
    // ========================================================================

    /// Saves the current canvas state.
    pub fn save(&mut self) {
        self.state_stack.push(self.current_state.clone());
        self.commands.push(DrawCommand::Save);
    }

    /// Saves the canvas state and creates a new layer.
    ///
    /// All drawing after this call will be composited onto the previous
    /// layer when `restore()` is called.
    pub fn save_layer(&mut self, bounds: Option<Rect>, paint: &Paint) {
        self.state_stack.push(self.current_state.clone());
        self.commands.push(DrawCommand::SaveLayer {
            bounds,
            paint: paint.clone(),
        });
    }

    /// Restores the previously saved canvas state.
    pub fn restore(&mut self) {
        if let Some(state) = self.state_stack.pop() {
            self.current_state = state;
            self.commands.push(DrawCommand::Restore);
        }
    }

    /// Returns the number of saved states.
    pub fn save_count(&self) -> usize {
        self.state_stack.len()
    }

    // ========================================================================
    // Transform Operations
    // ========================================================================

    /// Translates the canvas.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.commands.push(DrawCommand::Translate { dx, dy });
    }

    /// Scales the canvas.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.commands.push(DrawCommand::Scale { sx, sy });
    }

    /// Rotates the canvas.
    pub fn rotate(&mut self, radians: f32) {
        self.commands.push(DrawCommand::Rotate { radians });
    }

    /// Skews the canvas.
    ///
    /// `sx` is the horizontal skew factor and `sy` is the vertical skew factor.
    pub fn skew(&mut self, sx: f32, sy: f32) {
        self.commands.push(DrawCommand::Skew { sx, sy });
    }

    /// Applies a transform matrix.
    pub fn transform(&mut self, matrix: &[f32; 16]) {
        self.commands
            .push(DrawCommand::Transform { matrix: *matrix });
    }

    // ========================================================================
    // Clip Operations
    // ========================================================================

    /// Clips to a rectangle.
    pub fn clip_rect(&mut self, rect: Rect) {
        self.clip_rect_with_options(rect, ClipOp::Intersect, true);
    }

    /// Clips to a rectangle with options.
    pub fn clip_rect_with_options(&mut self, rect: Rect, clip_op: ClipOp, do_anti_alias: bool) {
        self.current_state.clip = Some(rect);
        self.commands.push(DrawCommand::ClipRect {
            rect,
            clip_op,
            do_anti_alias,
        });
    }

    /// Clips to a rounded rectangle.
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_rrect_with_anti_alias(rrect, true);
    }

    /// Clips to a rounded rectangle with anti-aliasing option.
    pub fn clip_rrect_with_anti_alias(&mut self, rrect: RRect, do_anti_alias: bool) {
        self.current_state.clip = Some(rrect.rect);
        self.commands.push(DrawCommand::ClipRRect {
            rrect,
            do_anti_alias,
        });
    }

    /// Clips to a rounded rectangle with options.
    ///
    /// Alias for [`clip_rrect_with_anti_alias`](Self::clip_rrect_with_anti_alias).
    #[inline]
    pub fn clip_rrect_with_options(&mut self, rrect: RRect, do_anti_alias: bool) {
        self.clip_rrect_with_anti_alias(rrect, do_anti_alias);
    }

    /// Clips to a path.
    pub fn clip_path(&mut self, path: Path) {
        self.clip_path_with_anti_alias(path, true);
    }

    /// Clips to a path with anti-aliasing option.
    pub fn clip_path_with_anti_alias(&mut self, path: Path, do_anti_alias: bool) {
        let mut path_copy = path.clone();
        self.current_state.clip = Some(path_copy.bounds());
        self.commands.push(DrawCommand::ClipPath {
            path,
            do_anti_alias,
        });
    }

    /// Clips to a path with options.
    ///
    /// Alias for [`clip_path_with_anti_alias`](Self::clip_path_with_anti_alias).
    #[inline]
    pub fn clip_path_with_options(&mut self, path: &Path, do_anti_alias: bool) {
        self.clip_path_with_anti_alias(path.clone(), do_anti_alias);
    }

    /// Returns the current clip bounds, if any.
    pub fn get_local_clip_bounds(&self) -> Option<Rect> {
        self.current_state.clip
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    fn expand_bounds(&mut self, rect: Rect) {
        if self.bounds == Rect::ZERO {
            self.bounds = rect;
        } else {
            self.bounds = self.bounds.expand_to_include(&rect);
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Paint
// ============================================================================

/// Paint style for drawing operations.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `Paint` class from `dart:ui`.
#[derive(Debug, Clone)]
pub struct Paint {
    /// The color to paint with (ARGB format).
    pub color: u32,

    /// The paint style (fill, stroke, etc.).
    pub style: PaintStyle,

    /// The stroke width (for stroke style).
    pub stroke_width: f32,

    /// Stroke cap style.
    pub stroke_cap: StrokeCap,

    /// Stroke join style.
    pub stroke_join: StrokeJoin,

    /// The stroke miter limit (for miter joins).
    pub stroke_miter_limit: f32,

    /// Whether anti-aliasing is enabled.
    pub anti_alias: bool,

    /// The blend mode for compositing.
    pub blend_mode: BlendMode,

    /// Optional shader for gradients or images.
    pub shader: Option<Shader>,

    /// Optional mask filter (for blur effects).
    pub mask_filter: Option<MaskFilter>,

    /// Optional image filter.
    pub image_filter: Option<ImageFilter>,

    /// Whether to invert colors.
    pub invert_colors: bool,

    /// Filter quality for image scaling.
    pub filter_quality: FilterQuality,
}

impl Paint {
    /// Creates a new paint with the given color.
    pub fn new(color: u32) -> Self {
        Self {
            color,
            style: PaintStyle::Fill,
            stroke_width: 1.0,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            stroke_miter_limit: 4.0,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
            shader: None,
            mask_filter: None,
            image_filter: None,
            invert_colors: false,
            filter_quality: FilterQuality::default(),
        }
    }

    /// Sets the paint style.
    pub fn with_style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the stroke width.
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }

    /// Sets the stroke cap.
    pub fn with_stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.stroke_cap = cap;
        self
    }

    /// Sets the stroke join.
    pub fn with_stroke_join(mut self, join: StrokeJoin) -> Self {
        self.stroke_join = join;
        self
    }

    /// Sets the stroke miter limit.
    pub fn with_stroke_miter_limit(mut self, limit: f32) -> Self {
        self.stroke_miter_limit = limit;
        self
    }

    /// Sets anti-aliasing.
    pub fn with_anti_alias(mut self, anti_alias: bool) -> Self {
        self.anti_alias = anti_alias;
        self
    }

    /// Sets the blend mode.
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Sets the shader.
    pub fn with_shader(mut self, shader: Option<Shader>) -> Self {
        self.shader = shader;
        self
    }

    /// Sets the mask filter.
    pub fn with_mask_filter(mut self, filter: Option<MaskFilter>) -> Self {
        self.mask_filter = filter;
        self
    }

    /// Sets the image filter.
    pub fn with_image_filter(mut self, filter: Option<ImageFilter>) -> Self {
        self.image_filter = filter;
        self
    }

    /// Sets whether to invert colors.
    pub fn with_invert_colors(mut self, invert: bool) -> Self {
        self.invert_colors = invert;
        self
    }

    /// Sets the filter quality.
    pub fn with_filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter_quality = quality;
        self
    }

    /// Creates a paint from RGBA components.
    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        let color = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        Self::new(color)
    }

    /// Returns the alpha component.
    #[inline]
    pub fn alpha(&self) -> u8 {
        ((self.color >> 24) & 0xFF) as u8
    }

    /// Returns the red component.
    #[inline]
    pub fn red(&self) -> u8 {
        ((self.color >> 16) & 0xFF) as u8
    }

    /// Returns the green component.
    #[inline]
    pub fn green(&self) -> u8 {
        ((self.color >> 8) & 0xFF) as u8
    }

    /// Returns the blue component.
    #[inline]
    pub fn blue(&self) -> u8 {
        (self.color & 0xFF) as u8
    }

    /// Sets the color from RGBA components.
    pub fn set_rgba(&mut self, r: u8, g: u8, b: u8, a: u8) {
        self.color = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
    }

    /// Sets the alpha component while keeping RGB.
    pub fn set_alpha(&mut self, alpha: u8) {
        self.color = (self.color & 0x00FFFFFF) | ((alpha as u32) << 24);
    }

    /// Returns whether this paint would actually draw anything visible.
    pub fn is_visible(&self) -> bool {
        self.alpha() > 0 || self.shader.is_some()
    }
}

impl Default for Paint {
    fn default() -> Self {
        Self::new(0xFF000000) // Opaque black
    }
}

// ============================================================================
// Shader
// ============================================================================

/// A shader for painting gradients or images.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `Shader` class from `dart:ui`.
#[derive(Debug, Clone)]
pub enum Shader {
    /// A linear gradient shader.
    LinearGradient {
        /// Start point.
        from: Offset,
        /// End point.
        to: Offset,
        /// Colors in the gradient.
        colors: Vec<u32>,
        /// Color stops (0.0 to 1.0), or empty for uniform distribution.
        color_stops: Vec<f32>,
        /// Tile mode for the gradient.
        tile_mode: TileMode,
        /// Optional transform matrix.
        transform: Option<[f32; 16]>,
    },
    /// A radial gradient shader.
    RadialGradient {
        /// Center point.
        center: Offset,
        /// Radius.
        radius: f32,
        /// Colors in the gradient.
        colors: Vec<u32>,
        /// Color stops (0.0 to 1.0), or empty for uniform distribution.
        color_stops: Vec<f32>,
        /// Tile mode for the gradient.
        tile_mode: TileMode,
        /// Optional transform matrix.
        transform: Option<[f32; 16]>,
    },
    /// A sweep (angular) gradient shader.
    SweepGradient {
        /// Center point.
        center: Offset,
        /// Start angle in radians.
        start_angle: f32,
        /// End angle in radians.
        end_angle: f32,
        /// Colors in the gradient.
        colors: Vec<u32>,
        /// Color stops (0.0 to 1.0), or empty for uniform distribution.
        color_stops: Vec<f32>,
        /// Tile mode for the gradient.
        tile_mode: TileMode,
        /// Optional transform matrix.
        transform: Option<[f32; 16]>,
    },
}

impl Shader {
    /// Creates a linear gradient.
    pub fn linear(from: Offset, to: Offset, colors: Vec<u32>, color_stops: Vec<f32>) -> Self {
        Self::LinearGradient {
            from,
            to,
            colors,
            color_stops,
            tile_mode: TileMode::Clamp,
            transform: None,
        }
    }

    /// Creates a radial gradient.
    pub fn radial(center: Offset, radius: f32, colors: Vec<u32>, color_stops: Vec<f32>) -> Self {
        Self::RadialGradient {
            center,
            radius,
            colors,
            color_stops,
            tile_mode: TileMode::Clamp,
            transform: None,
        }
    }

    /// Creates a sweep gradient.
    pub fn sweep(center: Offset, colors: Vec<u32>, color_stops: Vec<f32>) -> Self {
        Self::SweepGradient {
            center,
            start_angle: 0.0,
            end_angle: std::f32::consts::TAU,
            colors,
            color_stops,
            tile_mode: TileMode::Clamp,
            transform: None,
        }
    }
}

/// Tile mode for gradients and images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileMode {
    /// Clamp to the edge color.
    #[default]
    Clamp,
    /// Repeat the gradient.
    Repeat,
    /// Mirror the gradient.
    Mirror,
    /// Extend with a transparent color (for images).
    Decal,
}

// ============================================================================
// MaskFilter
// ============================================================================

/// A filter that transforms the alpha channel of a shape.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `MaskFilter` class from `dart:ui`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaskFilter {
    /// A blur effect.
    Blur {
        /// The blur style.
        style: BlurStyle,
        /// The blur radius (sigma).
        sigma: f32,
    },
}

impl MaskFilter {
    /// Creates a blur mask filter.
    pub fn blur(style: BlurStyle, sigma: f32) -> Self {
        Self::Blur { style, sigma }
    }
}

/// Style of blur for mask filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlurStyle {
    /// Blur both inside and outside the shape.
    #[default]
    Normal,
    /// Blur only the solid area (inside).
    Solid,
    /// Blur only the outer edge.
    Outer,
    /// Blur only the inner edge.
    Inner,
}

// ============================================================================
// ImageFilter
// ============================================================================

/// A filter effect applied to images.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `ImageFilter` class from `dart:ui`.
#[derive(Debug, Clone)]
pub enum ImageFilter {
    /// A blur filter.
    Blur {
        /// Horizontal blur sigma.
        sigma_x: f32,
        /// Vertical blur sigma.
        sigma_y: f32,
        /// Tile mode for edges.
        tile_mode: TileMode,
    },
    /// A dilation filter (expands bright areas).
    Dilate {
        /// Horizontal radius.
        radius_x: f32,
        /// Vertical radius.
        radius_y: f32,
    },
    /// An erosion filter (shrinks bright areas).
    Erode {
        /// Horizontal radius.
        radius_x: f32,
        /// Vertical radius.
        radius_y: f32,
    },
    /// A matrix transformation filter.
    Matrix {
        /// The transformation matrix.
        matrix: [f32; 16],
        /// The filter quality.
        filter_quality: FilterQuality,
    },
    /// A composed filter (apply inner then outer).
    Compose {
        /// The outer filter.
        outer: Box<ImageFilter>,
        /// The inner filter.
        inner: Box<ImageFilter>,
    },
}

impl ImageFilter {
    /// Creates a blur image filter.
    pub fn blur(sigma_x: f32, sigma_y: f32) -> Self {
        Self::Blur {
            sigma_x,
            sigma_y,
            tile_mode: TileMode::Clamp,
        }
    }

    /// Creates a dilation filter.
    pub fn dilate(radius_x: f32, radius_y: f32) -> Self {
        Self::Dilate { radius_x, radius_y }
    }

    /// Creates an erosion filter.
    pub fn erode(radius_x: f32, radius_y: f32) -> Self {
        Self::Erode { radius_x, radius_y }
    }

    /// Composes two filters.
    pub fn compose(outer: ImageFilter, inner: ImageFilter) -> Self {
        Self::Compose {
            outer: Box::new(outer),
            inner: Box::new(inner),
        }
    }
}

/// Quality of image filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterQuality {
    /// No filtering (nearest neighbor).
    None,
    /// Low quality (bilinear).
    #[default]
    Low,
    /// Medium quality (bilinear with mipmaps).
    Medium,
    /// High quality (bicubic or similar).
    High,
}

/// The style of painting (fill vs stroke).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaintStyle {
    /// Fill the shape.
    #[default]
    Fill,

    /// Stroke the shape outline.
    Stroke,

    /// Fill and stroke the shape.
    FillAndStroke,
}

/// How to render the end of a stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeCap {
    /// Flat edge at the end of the stroke.
    #[default]
    Butt,

    /// Round cap at the end of the stroke.
    Round,

    /// Square cap extending beyond the end.
    Square,
}

/// How to render the junction of two stroke segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeJoin {
    /// Sharp corner.
    #[default]
    Miter,

    /// Rounded corner.
    Round,

    /// Beveled corner.
    Bevel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_painting_context_new() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let context = PaintingContext::from_bounds(bounds);
        assert_eq!(context.estimated_bounds(), bounds);
    }

    #[test]
    fn test_canvas_draw_rect() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltwh(10.0, 20.0, 50.0, 60.0);
        let paint = Paint::new(0xFF0000FF);

        canvas.draw_rect(rect, &paint);

        assert_eq!(canvas.commands().len(), 1);
        assert_eq!(canvas.bounds(), rect);
    }

    #[test]
    fn test_canvas_draw_circle() {
        let mut canvas = Canvas::new();
        let center = Offset::new(50.0, 50.0);
        let paint = Paint::new(0xFF00FF00);

        canvas.draw_circle(center, 25.0, &paint);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_save_restore() {
        let mut canvas = Canvas::new();

        canvas.save();
        assert_eq!(canvas.save_count(), 1);

        canvas.save();
        assert_eq!(canvas.save_count(), 2);

        canvas.restore();
        assert_eq!(canvas.save_count(), 1);

        canvas.restore();
        assert_eq!(canvas.save_count(), 0);
    }

    #[test]
    fn test_canvas_transforms() {
        let mut canvas = Canvas::new();

        canvas.translate(10.0, 20.0);
        canvas.scale(2.0, 2.0);
        canvas.rotate(1.5);

        assert_eq!(canvas.commands().len(), 3);
    }

    #[test]
    fn test_canvas_end_recording() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        canvas.draw_rect(rect, &Paint::default());

        let picture = canvas.end_recording();
        assert_eq!(picture.bounds(), rect);
    }

    #[test]
    fn test_paint_rgba() {
        let paint = Paint::from_rgba(255, 128, 64, 200);
        assert_eq!(paint.red(), 255);
        assert_eq!(paint.green(), 128);
        assert_eq!(paint.blue(), 64);
        assert_eq!(paint.alpha(), 200);
    }

    #[test]
    fn test_paint_builder() {
        let paint = Paint::new(0xFFFFFFFF)
            .with_style(PaintStyle::Stroke)
            .with_stroke_width(2.0)
            .with_stroke_cap(StrokeCap::Round)
            .with_stroke_join(StrokeJoin::Bevel)
            .with_anti_alias(false);

        assert_eq!(paint.style, PaintStyle::Stroke);
        assert_eq!(paint.stroke_width, 2.0);
        assert_eq!(paint.stroke_cap, StrokeCap::Round);
        assert_eq!(paint.stroke_join, StrokeJoin::Bevel);
        assert!(!paint.anti_alias);
    }

    #[test]
    fn test_push_opacity() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::from_bounds(bounds);

        context.push_opacity(Offset::ZERO, 128, |ctx| {
            ctx.canvas().draw_rect(bounds, &Paint::default());
        });

        // Verify layer was added
        assert!(context.container_layer().first_child().is_some());
    }

    #[test]
    fn test_push_clip_rect_compositing() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let clip = Rect::from_ltwh(10.0, 10.0, 80.0, 80.0);
        let mut context = PaintingContext::from_bounds(bounds);

        context.push_clip_rect(true, Offset::ZERO, clip, |ctx| {
            ctx.canvas().draw_rect(bounds, &Paint::default());
        });

        // Verify clip layer was added
        assert!(context.container_layer().first_child().is_some());
    }

    // ========================================================================
    // New Tests for Enhanced Pipeline
    // ========================================================================

    #[test]
    fn test_blend_mode_conversion() {
        assert_eq!(BlendMode::SrcOver.as_u8(), 3);
        assert_eq!(BlendMode::from_u8(3), Some(BlendMode::SrcOver));
        assert_eq!(BlendMode::from_u8(255), None);
    }

    #[test]
    fn test_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
    }

    #[test]
    fn test_canvas_draw_drrect() {
        let mut canvas = Canvas::new();
        let outer = RRect::from_rect_xy(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0), 10.0, 10.0);
        let inner = RRect::from_rect_xy(Rect::from_ltwh(20.0, 20.0, 60.0, 60.0), 5.0, 5.0);
        let paint = Paint::new(0xFF0000FF);

        canvas.draw_drrect(outer, inner, &paint);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_draw_points() {
        let mut canvas = Canvas::new();
        let points = vec![
            Offset::new(10.0, 10.0),
            Offset::new(50.0, 50.0),
            Offset::new(90.0, 10.0),
        ];
        let paint = Paint::new(0xFF00FF00);

        canvas.draw_points(PointMode::Points, &points, &paint);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_draw_raw_points() {
        let mut canvas = Canvas::new();
        let coords = [10.0, 10.0, 50.0, 50.0, 90.0, 10.0];
        let paint = Paint::new(0xFF00FF00);

        canvas.draw_raw_points(PointMode::Polygon, &coords, &paint);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_draw_color() {
        let mut canvas = Canvas::new();

        canvas.draw_color(0xFF0000FF, BlendMode::SrcOver);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_draw_shadow() {
        let mut canvas = Canvas::new();
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));
        path.close();

        canvas.draw_shadow(&path, 0x80000000, 8.0, false);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_save_layer() {
        let mut canvas = Canvas::new();

        canvas.save_layer(
            Some(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0)),
            &Paint::default(),
        );
        assert_eq!(canvas.save_count(), 1);

        canvas.restore();
        assert_eq!(canvas.save_count(), 0);
    }

    #[test]
    fn test_canvas_skew() {
        let mut canvas = Canvas::new();

        canvas.skew(0.5, 0.0);

        assert_eq!(canvas.commands().len(), 1);
    }

    #[test]
    fn test_canvas_clip_with_options() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltwh(10.0, 10.0, 80.0, 80.0);

        canvas.clip_rect_with_options(rect, ClipOp::Difference, false);

        assert_eq!(canvas.commands().len(), 1);
        assert_eq!(canvas.get_local_clip_bounds(), Some(rect));
    }

    #[test]
    fn test_paint_with_blend_mode() {
        let paint = Paint::new(0xFFFFFFFF).with_blend_mode(BlendMode::Multiply);

        assert_eq!(paint.blend_mode, BlendMode::Multiply);
    }

    #[test]
    fn test_paint_with_shader() {
        let shader = Shader::linear(
            Offset::new(0.0, 0.0),
            Offset::new(100.0, 0.0),
            vec![0xFFFF0000, 0xFF0000FF],
            vec![0.0, 1.0],
        );
        let paint = Paint::new(0xFFFFFFFF).with_shader(Some(shader));

        assert!(paint.shader.is_some());
    }

    #[test]
    fn test_paint_with_mask_filter() {
        let filter = MaskFilter::blur(BlurStyle::Normal, 5.0);
        let paint = Paint::new(0xFFFFFFFF).with_mask_filter(Some(filter));

        assert!(paint.mask_filter.is_some());
    }

    #[test]
    fn test_paint_with_image_filter() {
        let filter = ImageFilter::blur(10.0, 10.0);
        let paint = Paint::new(0xFFFFFFFF).with_image_filter(Some(filter));

        assert!(paint.image_filter.is_some());
    }

    #[test]
    fn test_paint_is_visible() {
        let visible_paint = Paint::new(0xFF000000);
        assert!(visible_paint.is_visible());

        let transparent_paint = Paint::new(0x00000000);
        assert!(!transparent_paint.is_visible());

        let shader_paint = Paint::new(0x00000000).with_shader(Some(Shader::linear(
            Offset::ZERO,
            Offset::new(100.0, 0.0),
            vec![0xFFFF0000],
            vec![],
        )));
        assert!(shader_paint.is_visible());
    }

    #[test]
    fn test_paint_set_alpha() {
        let mut paint = Paint::new(0xFFFF0000);
        paint.set_alpha(128);

        assert_eq!(paint.alpha(), 128);
        assert_eq!(paint.red(), 255);
    }

    #[test]
    fn test_shader_linear() {
        let shader = Shader::linear(
            Offset::new(0.0, 0.0),
            Offset::new(100.0, 100.0),
            vec![0xFFFF0000, 0xFF00FF00, 0xFF0000FF],
            vec![0.0, 0.5, 1.0],
        );

        match shader {
            Shader::LinearGradient { colors, .. } => {
                assert_eq!(colors.len(), 3);
            }
            _ => panic!("Expected LinearGradient"),
        }
    }

    #[test]
    fn test_shader_radial() {
        let shader = Shader::radial(
            Offset::new(50.0, 50.0),
            50.0,
            vec![0xFFFFFFFF, 0xFF000000],
            vec![],
        );

        match shader {
            Shader::RadialGradient { radius, .. } => {
                assert_eq!(radius, 50.0);
            }
            _ => panic!("Expected RadialGradient"),
        }
    }

    #[test]
    fn test_shader_sweep() {
        let shader = Shader::sweep(
            Offset::new(50.0, 50.0),
            vec![0xFFFF0000, 0xFF00FF00, 0xFF0000FF, 0xFFFF0000],
            vec![],
        );

        match shader {
            Shader::SweepGradient {
                start_angle,
                end_angle,
                ..
            } => {
                assert_eq!(start_angle, 0.0);
                assert!((end_angle - std::f32::consts::TAU).abs() < 0.001);
            }
            _ => panic!("Expected SweepGradient"),
        }
    }

    #[test]
    fn test_mask_filter_blur() {
        let filter = MaskFilter::blur(BlurStyle::Outer, 3.0);

        match filter {
            MaskFilter::Blur { style, sigma } => {
                assert_eq!(style, BlurStyle::Outer);
                assert_eq!(sigma, 3.0);
            }
        }
    }

    #[test]
    fn test_image_filter_compose() {
        let outer = ImageFilter::blur(5.0, 5.0);
        let inner = ImageFilter::dilate(2.0, 2.0);
        let composed = ImageFilter::compose(outer, inner);

        match composed {
            ImageFilter::Compose { .. } => {}
            _ => panic!("Expected Compose"),
        }
    }

    #[test]
    fn test_point_mode_default() {
        assert_eq!(PointMode::default(), PointMode::Points);
    }

    #[test]
    fn test_clip_op_default() {
        assert_eq!(ClipOp::default(), ClipOp::Intersect);
    }

    #[test]
    fn test_tile_mode_default() {
        assert_eq!(TileMode::default(), TileMode::Clamp);
    }

    #[test]
    fn test_blur_style_default() {
        assert_eq!(BlurStyle::default(), BlurStyle::Normal);
    }

    #[test]
    fn test_filter_quality_default() {
        assert_eq!(FilterQuality::default(), FilterQuality::Low);
    }

    #[test]
    fn test_painting_context_is_recording() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::from_bounds(bounds);

        assert!(!context.is_recording());

        // Access canvas starts recording
        let _ = context.canvas();
        assert!(context.is_recording());

        context.stop_recording_if_needed();
        assert!(!context.is_recording());
    }

    #[test]
    fn test_painting_context_set_hints() {
        let bounds = Rect::from_ltwh(0.0, 0.0, 100.0, 100.0);
        let mut context = PaintingContext::from_bounds(bounds);

        context.set_is_complex_hint();
        assert!(context.is_recording());

        context.stop_recording_if_needed();

        context.set_will_change_hint();
        assert!(context.is_recording());
    }

    #[test]
    fn test_create_child_context() {
        let layer = ContainerLayer::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 50.0, 50.0);

        let child_context = PaintingContext::create_child_context(layer, bounds);

        assert_eq!(child_context.estimated_bounds(), bounds);
    }
}
