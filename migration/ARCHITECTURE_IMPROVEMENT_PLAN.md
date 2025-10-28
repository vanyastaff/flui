# flui_engine Architecture Improvement Plan

> Comprehensive architectural improvements for flui_engine based on Flutter's rendering patterns
>
> **Created**: 2025-10-28
> **Status**: Planning Phase

---

## Executive Summary

This document outlines a comprehensive plan to improve the architecture and API design of `flui_engine` to better align with Flutter's proven rendering patterns while maintaining Rust idioms and improving ergonomics.

**Key Goals**:
- Унифицировать Layer API (Unify Layer API)
- Добавить SceneBuilder pattern (Add SceneBuilder pattern)
- Расширить возможности рендеринга (Extend rendering capabilities)
- Улучшить эргономику API (Improve API ergonomics)

---

## 1. Layer System Architecture

### Current Problems

1. **Dual Layer Traits**: Conflicting trait definitions
   - `layer/mod.rs:103` - Legacy: `paint(&self, painter: &mut dyn Painter)`
   - `layer/base.rs:79` - New: `add_to_scene(&self, painter: &mut dyn Painter, offset: Offset)`

2. **Inconsistent API**: Different implementations use different traits

3. **Missing Flutter Pattern**: Doesn't fully match Flutter's `Layer.addToScene()` pattern

### Solution: Unified Layer Trait

**File**: `crates/flui_engine/src/layer/base.rs`

```rust
/// Abstract base for all composited layers
///
/// Matches Flutter's Layer API pattern
pub trait Layer: Send + Sync {
    /// Add this layer to the scene being built
    ///
    /// This is the primary method for layer composition.
    /// Layers add themselves to the SceneBuilder, which constructs
    /// the final scene graph.
    ///
    /// # Arguments
    /// * `builder` - The scene builder to add this layer to
    /// * `offset` - Offset to apply to this layer's content
    fn add_to_scene(&self, builder: &mut SceneBuilder, offset: Offset);

    /// Get the bounding rectangle of this layer
    ///
    /// Used for culling and optimization. Layers outside the viewport
    /// don't need to be painted.
    fn bounds(&self) -> Rect;

    /// Check if this layer is visible
    ///
    /// Invisible layers can be skipped during painting.
    fn is_visible(&self) -> bool {
        true
    }

    /// Mark this layer as needing to be repainted
    ///
    /// This is typically called when the layer's visual appearance has changed.
    fn mark_needs_paint(&mut self) {
        // Default: no-op
        // Subclasses override to implement dirty tracking
    }

    /// Dispose of this layer and release its resources
    ///
    /// After calling dispose, the layer must not be used.
    fn dispose(&mut self) {
        // Default: no-op
        // Subclasses override to clean up resources
    }

    /// Check if this layer has been disposed
    fn is_disposed(&self) -> bool {
        false // Default: not disposed
    }

    /// Attach this layer to a parent
    fn attach(&mut self, _parent: Option<Arc<RwLock<dyn Layer>>>) {
        // Default: no-op
    }

    /// Detach this layer from its parent
    fn detach(&mut self) {
        // Default: no-op
    }

    /// Get a debug description of this layer
    fn debug_description(&self) -> String {
        format!("Layer({:?})", self.bounds())
    }
}
```

**Migration Steps**:
1. Mark old `Layer` trait in `layer/mod.rs` as `#[deprecated]`
2. Update all layer implementations to use new trait
3. Update Scene and Compositor to work with new trait
4. Remove old trait in next major version

---

## 2. SceneBuilder Pattern

### Current Problem

The current `Scene` is just a container with a `ContainerLayer` root. Flutter uses a **SceneBuilder** pattern that constructs scenes incrementally with a stack-based API.

### Solution: Add SceneBuilder

**File**: `crates/flui_engine/src/scene_builder.rs` (new file)

```rust
use crate::layer::{Layer, BoxedLayer, TransformLayer, ClipLayer, OpacityLayer, PictureLayer};
use flui_types::{Rect, Offset};

/// Builder for constructing scenes incrementally
///
/// SceneBuilder provides a stack-based API for building complex
/// layer trees. This matches Flutter's SceneBuilder pattern.
///
/// # Example
///
/// ```rust,ignore
/// let mut builder = SceneBuilder::new();
///
/// // Push a transform
/// builder.push_transform(Transform::translate(10.0, 20.0));
///
/// // Push opacity
/// builder.push_opacity(0.8);
///
/// // Add a picture layer
/// let mut picture = PictureLayer::new();
/// picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
/// builder.add_picture(picture);
///
/// builder.pop(); // Pop opacity
/// builder.pop(); // Pop transform
///
/// let scene = builder.build();
/// ```
pub struct SceneBuilder {
    /// Stack of layer containers
    layer_stack: Vec<LayerContainer>,

    /// Root layer
    root: Option<ContainerLayer>,

    /// Current transform matrix (accumulated)
    current_transform: Matrix3,

    /// Current clip rect
    current_clip: Option<Rect>,

    /// Current opacity (accumulated)
    current_opacity: f32,
}

enum LayerContainer {
    Transform(TransformLayer),
    Opacity(OpacityLayer),
    Clip(ClipLayer),
    Container(ContainerLayer),
}

impl SceneBuilder {
    /// Create a new scene builder
    pub fn new() -> Self {
        Self {
            layer_stack: Vec::new(),
            root: Some(ContainerLayer::new()),
            current_transform: Matrix3::identity(),
            current_clip: None,
            current_opacity: 1.0,
        }
    }

    /// Push a transform layer onto the stack
    ///
    /// All subsequent layers will be affected by this transform
    /// until `pop()` is called.
    pub fn push_transform(&mut self, transform: Transform) {
        let layer = TransformLayer::new(transform);
        self.layer_stack.push(LayerContainer::Transform(layer));

        // Update accumulated transform
        self.current_transform = self.current_transform * transform.to_matrix();
    }

    /// Push a clip rect layer onto the stack
    ///
    /// All subsequent layers will be clipped to this rectangle
    /// until `pop()` is called.
    pub fn push_clip_rect(&mut self, rect: Rect) {
        let layer = ClipLayer::rect(rect);
        self.layer_stack.push(LayerContainer::Clip(layer));
        self.current_clip = Some(rect);
    }

    /// Push a clip rounded rect layer onto the stack
    pub fn push_clip_rrect(&mut self, rrect: RRect) {
        let layer = ClipLayer::rrect(rrect);
        self.layer_stack.push(LayerContainer::Clip(layer));
        self.current_clip = Some(rrect.rect);
    }

    /// Push an opacity layer onto the stack
    ///
    /// All subsequent layers will have their opacity multiplied by this value
    /// until `pop()` is called.
    ///
    /// # Arguments
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque)
    pub fn push_opacity(&mut self, opacity: f32) {
        let layer = OpacityLayer::new(opacity);
        self.layer_stack.push(LayerContainer::Opacity(layer));

        // Update accumulated opacity
        self.current_opacity *= opacity;
    }

    /// Add a picture layer (leaf node)
    ///
    /// Picture layers contain the actual drawing commands.
    pub fn add_picture(&mut self, picture: PictureLayer) {
        let boxed: BoxedLayer = Box::new(picture);

        if let Some(container) = self.current_container_mut() {
            container.add_child(boxed);
        }
    }

    /// Add any layer
    pub fn add_layer(&mut self, layer: BoxedLayer) {
        if let Some(container) = self.current_container_mut() {
            container.add_child(layer);
        }
    }

    /// Pop the current layer off the stack
    ///
    /// This must be called for each `push_*` call to maintain
    /// the layer hierarchy.
    pub fn pop(&mut self) {
        if let Some(layer_container) = self.layer_stack.pop() {
            // Add the completed layer to its parent
            let layer = match layer_container {
                LayerContainer::Transform(l) => Box::new(l) as BoxedLayer,
                LayerContainer::Opacity(l) => Box::new(l) as BoxedLayer,
                LayerContainer::Clip(l) => Box::new(l) as BoxedLayer,
                LayerContainer::Container(l) => Box::new(l) as BoxedLayer,
            };

            if let Some(parent) = self.current_container_mut() {
                parent.add_child(layer);
            }
        }
    }

    /// Build the final scene
    ///
    /// Consumes the builder and returns the constructed scene.
    pub fn build(mut self, viewport_size: Size) -> Scene {
        // Pop any remaining layers
        while !self.layer_stack.is_empty() {
            self.pop();
        }

        Scene::from_root(self.root.take().unwrap(), viewport_size)
    }

    /// Get the current container layer
    fn current_container_mut(&mut self) -> Option<&mut ContainerLayer> {
        if let Some(last) = self.layer_stack.last_mut() {
            match last {
                LayerContainer::Transform(ref mut l) => l.container_mut(),
                LayerContainer::Opacity(ref mut l) => l.container_mut(),
                LayerContainer::Clip(ref mut l) => l.container_mut(),
                LayerContainer::Container(ref mut l) => Some(l),
            }
        } else {
            self.root.as_mut()
        }
    }
}
```

**Add to**: `crates/flui_engine/src/lib.rs`
```rust
pub mod scene_builder;
pub use scene_builder::SceneBuilder;
```

---

## 3. PaintContext Enhancement

### Current Problem

`PaintContext` only provides a painter reference. Flutter's `PaintingContext` handles layer creation and is more sophisticated.

### Solution: Extend PaintContext

**File**: `crates/flui_engine/src/paint_context.rs`

Add to existing struct:

```rust
pub struct PaintContext<'a> {
    painter: &'a mut dyn Painter,
    canvas_bounds: Rect,

    // NEW: Scene builder for layer creation
    scene_builder: Option<&'a mut SceneBuilder>,

    debug_paint: bool,
}

impl<'a> PaintContext<'a> {
    /// Create a new paint context with scene builder support
    pub fn with_scene_builder(
        painter: &'a mut dyn Painter,
        canvas_bounds: Rect,
        scene_builder: &'a mut SceneBuilder,
    ) -> Self {
        Self {
            painter,
            canvas_bounds,
            scene_builder: Some(scene_builder),
            debug_paint: false,
        }
    }

    /// Push a layer onto the scene
    ///
    /// Matches Flutter's PaintingContext.pushLayer()
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// context.push_layer(ClipLayer::rect(rect), |ctx| {
    ///     // Paint children with clipping applied
    ///     child.paint(ctx, offset);
    /// });
    /// ```
    pub fn push_layer<L, F>(
        &mut self,
        layer: L,
        child_painter: F,
    ) where
        L: Layer + 'static,
        F: FnOnce(&mut PaintContext),
    {
        if let Some(builder) = &mut self.scene_builder {
            builder.add_layer(Box::new(layer));
            child_painter(self);
            builder.pop();
        } else {
            // Fallback: just call child painter without layer
            child_painter(self);
        }
    }

    /// Push a clip rect layer
    ///
    /// Convenience method for common case of pushing a clip rect.
    pub fn push_clip_rect<F>(
        &mut self,
        rect: Rect,
        child_painter: F,
    ) where
        F: FnOnce(&mut PaintContext),
    {
        if let Some(builder) = &mut self.scene_builder {
            builder.push_clip_rect(rect);
            child_painter(self);
            builder.pop();
        } else {
            self.painter.save();
            self.painter.clip_rect(rect);
            child_painter(self);
            self.painter.restore();
        }
    }

    /// Push an opacity layer
    pub fn push_opacity<F>(
        &mut self,
        opacity: f32,
        child_painter: F,
    ) where
        F: FnOnce(&mut PaintContext),
    {
        if let Some(builder) = &mut self.scene_builder {
            builder.push_opacity(opacity);
            child_painter(self);
            builder.pop();
        } else {
            self.painter.save();
            self.painter.set_opacity(opacity);
            child_painter(self);
            self.painter.restore();
        }
    }

    /// Push a transform layer
    pub fn push_transform<F>(
        &mut self,
        transform: Transform,
        child_painter: F,
    ) where
        F: FnOnce(&mut PaintContext),
    {
        if let Some(builder) = &mut self.scene_builder {
            builder.push_transform(transform);
            child_painter(self);
            builder.pop();
        } else {
            self.painter.save();
            match transform {
                Transform::Translate(offset) => self.painter.translate(offset),
                Transform::Rotate(angle) => self.painter.rotate(angle),
                Transform::Scale(sx, sy) => self.painter.scale(sx, sy),
                Transform::Matrix(matrix) => self.painter.set_transform(matrix),
            }
            child_painter(self);
            self.painter.restore();
        }
    }
}
```

---

## 4. Painter Trait Improvements

### Current Problem

The `Painter` trait is missing several important methods needed by `flui_painting` painters (Text, Image, Path, Polygon, Arc).

### Solution: Extend Painter Trait

**File**: `crates/flui_engine/src/painter/mod.rs`

```rust
use flui_types::{Point, Rect, Offset};
use flui_types::painting::{Path, Image, TextStyle};

pub trait Painter {
    // ===== Existing methods =====
    fn rect(&mut self, rect: Rect, paint: &Paint);
    fn rrect(&mut self, rrect: RRect, paint: &Paint);
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint);
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint);

    fn save(&mut self);
    fn restore(&mut self);
    fn translate(&mut self, offset: Offset);
    fn rotate(&mut self, angle: f32);
    fn scale(&mut self, sx: f32, sy: f32);
    fn clip_rect(&mut self, rect: Rect);
    fn clip_rrect(&mut self, rrect: RRect);
    fn set_opacity(&mut self, opacity: f32);

    // ===== NEW methods =====

    /// Render text at the given position
    ///
    /// # Arguments
    /// * `text` - The text to render
    /// * `position` - Top-left position of the text
    /// * `style` - Text style (font, size, color, etc.)
    fn text(
        &mut self,
        text: &str,
        position: Point,
        style: &TextStyle,
    ) {
        // Default implementation: no-op
        // Backends that support text override this
        let _ = (text, position, style);
    }

    /// Render an image
    ///
    /// # Arguments
    /// * `image` - The image to render
    /// * `src_rect` - Source rectangle in image coordinates
    /// * `dst_rect` - Destination rectangle on canvas
    /// * `paint` - Paint settings (opacity, blend mode, etc.)
    fn image(
        &mut self,
        image: &Image,
        src_rect: Rect,
        dst_rect: Rect,
        paint: &Paint,
    ) {
        // Default implementation: no-op
        let _ = (image, src_rect, dst_rect, paint);
    }

    /// Render a path
    ///
    /// The path can contain multiple contours with various commands
    /// (move, line, curve, close, etc.)
    fn path(&mut self, path: &Path, paint: &Paint) {
        // Default implementation: decompose path into primitives
        use flui_types::painting::PathCommand;

        let commands = path.commands();
        let mut current_pos = Point::ZERO;
        let mut subpath_start = Point::ZERO;

        for cmd in commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    current_pos = *p;
                    subpath_start = *p;
                }
                PathCommand::LineTo(p) => {
                    self.line(current_pos, *p, paint);
                    current_pos = *p;
                }
                PathCommand::QuadraticTo(c, p) => {
                    // Approximate quadratic bezier with lines
                    let steps = 20;
                    for i in 1..=steps {
                        let t = i as f32 / steps as f32;
                        let t1 = 1.0 - t;
                        let next = Point::new(
                            t1 * t1 * current_pos.x + 2.0 * t1 * t * c.x + t * t * p.x,
                            t1 * t1 * current_pos.y + 2.0 * t1 * t * c.y + t * t * p.y,
                        );
                        self.line(current_pos, next, paint);
                        current_pos = next;
                    }
                    current_pos = *p;
                }
                PathCommand::CubicTo(c1, c2, p) => {
                    // Approximate cubic bezier with lines
                    let steps = 20;
                    for i in 1..=steps {
                        let t = i as f32 / steps as f32;
                        let t1 = 1.0 - t;
                        let next = Point::new(
                            t1.powi(3) * current_pos.x
                                + 3.0 * t1.powi(2) * t * c1.x
                                + 3.0 * t1 * t.powi(2) * c2.x
                                + t.powi(3) * p.x,
                            t1.powi(3) * current_pos.y
                                + 3.0 * t1.powi(2) * t * c1.y
                                + 3.0 * t1 * t.powi(2) * c2.y
                                + t.powi(3) * p.y,
                        );
                        self.line(current_pos, next, paint);
                        current_pos = next;
                    }
                    current_pos = *p;
                }
                PathCommand::Close => {
                    if current_pos != subpath_start {
                        self.line(current_pos, subpath_start, paint);
                    }
                    current_pos = subpath_start;
                }
                PathCommand::AddRect(rect) => {
                    self.rect(*rect, paint);
                }
                PathCommand::AddCircle(center, radius) => {
                    self.circle(*center, *radius, paint);
                }
                PathCommand::AddOval(rect) => {
                    // Draw oval as circle with scale transform
                    self.save();
                    let center = rect.center();
                    let rx = rect.width() / 2.0;
                    let ry = rect.height() / 2.0;
                    self.translate(Offset::new(center.x, center.y));
                    self.scale(rx, ry);
                    self.circle(Point::ZERO, 1.0, paint);
                    self.restore();
                }
                PathCommand::AddArc(rect, start_angle, sweep_angle) => {
                    self.arc(*rect, *start_angle, *sweep_angle, paint);
                }
            }
        }
    }

    /// Render a polygon (closed path through points)
    ///
    /// # Arguments
    /// * `points` - The vertices of the polygon
    /// * `paint` - Paint settings
    fn polygon(&mut self, points: &[Point], paint: &Paint) {
        // Default implementation: draw lines between points
        if points.len() < 2 {
            return;
        }

        for i in 0..points.len() {
            let p1 = points[i];
            let p2 = points[(i + 1) % points.len()];
            self.line(p1, p2, paint);
        }
    }

    /// Render an arc or pie slice
    ///
    /// # Arguments
    /// * `rect` - Bounding rectangle of the ellipse
    /// * `start_angle` - Starting angle in radians
    /// * `sweep_angle` - Angle to sweep in radians
    /// * `paint` - Paint settings
    fn arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        paint: &Paint,
    ) {
        // Default implementation: approximate with line segments
        let steps = ((sweep_angle.abs() * 20.0) as usize).max(4);
        let center = rect.center();
        let rx = rect.width() / 2.0;
        let ry = rect.height() / 2.0;

        let mut points = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let angle = start_angle + sweep_angle * (i as f32 / steps as f32);
            points.push(Point::new(
                center.x + rx * angle.cos(),
                center.y + ry * angle.sin(),
            ));
        }

        for i in 0..points.len() - 1 {
            self.line(points[i], points[i + 1], paint);
        }
    }

    /// Get the current transform matrix
    ///
    /// This is useful for advanced rendering that needs to know
    /// the current coordinate system transformation.
    fn get_transform(&self) -> Matrix3 {
        // Default: identity transform
        Matrix3::identity()
    }

    /// Set an arbitrary transform matrix
    ///
    /// This provides low-level control over the coordinate system.
    fn set_transform(&mut self, matrix: Matrix3) {
        // Default: decompose into translate/rotate/scale if possible
        // Backends can override for native matrix support
        let _ = matrix;
    }
}
```

---

## 5. DrawCommand Enhancement

### Current Problem

`PictureLayer::DrawCommand` only supports 4 basic primitives (Rect, RRect, Circle, Line). Missing Text, Image, Path, Arc as noted in TODO comments.

### Solution: Extend DrawCommand Enum

**File**: `crates/flui_engine/src/layer/picture.rs`

```rust
use std::sync::Arc;
use flui_types::painting::{Path, Image, TextStyle};

#[derive(Debug, Clone)]
pub enum DrawCommand {
    // ===== Existing =====
    Rect {
        rect: Rect,
        paint: Paint,
    },
    RRect {
        rrect: RRect,
        paint: Paint,
    },
    Circle {
        center: Point,
        radius: f32,
        paint: Paint,
    },
    Line {
        p1: Point,
        p2: Point,
        paint: Paint,
    },

    // ===== NEW =====

    /// Render text
    Text {
        text: String,
        position: Point,
        style: TextStyle,
    },

    /// Render an image
    Image {
        image: Arc<Image>, // Arc for cheap cloning
        src_rect: Rect,
        dst_rect: Rect,
        paint: Paint,
    },

    /// Render a path
    Path {
        path: Arc<Path>, // Arc because paths can be large
        paint: Paint,
    },

    /// Render an arc or pie slice
    Arc {
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        paint: Paint,
    },

    /// Render a polygon
    Polygon {
        points: Arc<Vec<Point>>, // Arc for cheap cloning
        paint: Paint,
    },
}

impl PictureLayer {
    // ===== Existing methods =====
    // draw_rect, draw_rrect, draw_circle, draw_line

    // ===== NEW methods =====

    /// Draw text
    pub fn draw_text(&mut self, text: impl Into<String>, position: Point, style: TextStyle) {
        self.add_command(DrawCommand::Text {
            text: text.into(),
            position,
            style,
        });
    }

    /// Draw an image
    pub fn draw_image(
        &mut self,
        image: Arc<Image>,
        src_rect: Rect,
        dst_rect: Rect,
        paint: Paint,
    ) {
        self.add_command(DrawCommand::Image {
            image,
            src_rect,
            dst_rect,
            paint,
        });
    }

    /// Draw a path
    pub fn draw_path(&mut self, path: Arc<Path>, paint: Paint) {
        self.add_command(DrawCommand::Path { path, paint });
    }

    /// Draw an arc
    pub fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        paint: Paint,
    ) {
        self.add_command(DrawCommand::Arc {
            rect,
            start_angle,
            sweep_angle,
            paint,
        });
    }

    /// Draw a polygon
    pub fn draw_polygon(&mut self, points: Arc<Vec<Point>>, paint: Paint) {
        self.add_command(DrawCommand::Polygon { points, paint });
    }

    /// Calculate bounds of a single drawing command
    fn command_bounds(command: &DrawCommand) -> Rect {
        match command {
            // Existing cases...

            DrawCommand::Text { text, position, style } => {
                // Approximate text bounds
                // TODO: Use proper text measurement
                let width = text.len() as f32 * style.font_size * 0.6;
                let height = style.font_size;
                Rect::from_xywh(position.x, position.y, width, height)
            }

            DrawCommand::Image { dst_rect, paint, .. } => {
                if paint.stroke_width > 0.0 {
                    dst_rect.expand(paint.stroke_width / 2.0)
                } else {
                    *dst_rect
                }
            }

            DrawCommand::Path { path, paint } => {
                let bounds = path.bounds();
                if paint.stroke_width > 0.0 {
                    bounds.expand(paint.stroke_width / 2.0)
                } else {
                    bounds
                }
            }

            DrawCommand::Arc { rect, paint, .. } => {
                if paint.stroke_width > 0.0 {
                    rect.expand(paint.stroke_width / 2.0)
                } else {
                    *rect
                }
            }

            DrawCommand::Polygon { points, paint } => {
                if points.is_empty() {
                    return Rect::ZERO;
                }

                let mut min_x = points[0].x;
                let mut min_y = points[0].y;
                let mut max_x = points[0].x;
                let mut max_y = points[0].y;

                for p in points.iter().skip(1) {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }

                let stroke = paint.stroke_width / 2.0;
                Rect::from_min_max(
                    Point::new(min_x - stroke, min_y - stroke),
                    Point::new(max_x + stroke, max_y + stroke),
                )
            }
        }
    }
}

impl Layer for PictureLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Execute all drawing commands
        for command in &self.commands {
            match command {
                // Existing cases...

                DrawCommand::Text { text, position, style } => {
                    painter.text(text, *position, style);
                }

                DrawCommand::Image { image, src_rect, dst_rect, paint } => {
                    painter.image(image, *src_rect, *dst_rect, paint);
                }

                DrawCommand::Path { path, paint } => {
                    painter.path(path, paint);
                }

                DrawCommand::Arc { rect, start_angle, sweep_angle, paint } => {
                    painter.arc(*rect, *start_angle, *sweep_angle, paint);
                }

                DrawCommand::Polygon { points, paint } => {
                    painter.polygon(points, paint);
                }
            }
        }
    }

    // ... rest of implementation
}
```

---

## 6. Layer Handle Improvements

### Current State

`LayerHandle<L>` exists but isn't well integrated.

### Solution: Better Integration

**File**: `crates/flui_engine/src/layer/handle.rs`

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use crate::layer::Layer;

/// Handle to a layer for resource management
///
/// LayerHandle provides a smart pointer to a layer that tracks
/// its lifecycle and ensures proper cleanup.
///
/// # Example
///
/// ```rust,ignore
/// struct MyRenderObject {
///     clip_layer: LayerHandle<ClipLayer>,
/// }
///
/// impl MyRenderObject {
///     fn paint(&mut self, context: &mut PaintContext, offset: Offset) {
///         let old_layer = self.clip_layer.take();
///         let new_layer = context.push_clip_rect(
///             self.clip_rect + offset,
///             old_layer,
///             |ctx| {
///                 self.paint_children(ctx, offset);
///             },
///         );
///         self.clip_layer.set(Some(new_layer));
///     }
///
///     fn dispose(&mut self) {
///         self.clip_layer.clear(); // Clean up
///     }
/// }
/// ```
pub struct LayerHandle<L: Layer> {
    inner: Option<Arc<RwLock<L>>>,
}

impl<L: Layer> LayerHandle<L> {
    /// Create a new empty handle
    pub fn new() -> Self {
        Self { inner: None }
    }

    /// Create a handle with a layer
    pub fn from_layer(layer: L) -> Self {
        Self {
            inner: Some(Arc::new(RwLock::new(layer))),
        }
    }

    /// Take the layer out of the handle, leaving None
    ///
    /// This is useful when you need to pass ownership of the layer
    /// to another API.
    pub fn take(&mut self) -> Option<Arc<RwLock<L>>> {
        self.inner.take()
    }

    /// Set a new layer in the handle
    ///
    /// If there was a previous layer, it is dropped (and disposed
    /// if this was the last reference).
    pub fn set(&mut self, layer: Option<Arc<RwLock<L>>>) {
        // Drop old layer (will dispose if last reference)
        self.inner = layer;
    }

    /// Get a reference to the layer (clones the Arc)
    ///
    /// Returns None if the handle is empty.
    pub fn get(&self) -> Option<Arc<RwLock<L>>> {
        self.inner.clone()
    }

    /// Clear the handle and dispose the layer
    ///
    /// This explicitly disposes the layer if this handle holds
    /// the last reference to it.
    pub fn clear(&mut self) {
        if let Some(layer) = self.inner.take() {
            // Try to dispose if we have exclusive access
            if let Ok(mut layer) = layer.try_write() {
                layer.dispose();
            }
        }
    }

    /// Check if the handle is empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_none()
    }

    /// Check if the layer has been disposed
    pub fn is_disposed(&self) -> bool {
        if let Some(layer) = &self.inner {
            if let Ok(layer) = layer.try_read() {
                return layer.is_disposed();
            }
        }
        false
    }

    /// Access the layer with a read lock
    pub fn with_read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&L) -> R,
    {
        self.inner.as_ref().map(|arc| {
            let layer = arc.read();
            f(&*layer)
        })
    }

    /// Access the layer with a write lock
    pub fn with_write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut L) -> R,
    {
        self.inner.as_ref().map(|arc| {
            let mut layer = arc.write();
            f(&mut *layer)
        })
    }
}

impl<L: Layer> Default for LayerHandle<L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: Layer> Clone for LayerHandle<L> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<L: Layer> Drop for LayerHandle<L> {
    fn drop(&mut self) {
        // Dispose layer if this is the last handle
        if let Some(layer) = self.inner.take() {
            if Arc::strong_count(&layer) == 1 {
                if let Ok(mut layer) = layer.try_write() {
                    layer.dispose();
                }
            }
        }
    }
}
```

---

## 7. API Consistency and Ergonomics

### Builder Patterns

Add builder patterns for complex types:

**File**: `crates/flui_engine/src/painter/mod.rs`

```rust
impl Paint {
    /// Create a new paint with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set color (builder pattern)
    pub fn with_color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }

    /// Set stroke width (builder pattern)
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }

    /// Enable anti-aliasing (builder pattern)
    pub fn with_anti_alias(mut self, enabled: bool) -> Self {
        self.anti_alias = enabled;
        self
    }
}
```

**File**: `crates/flui_types/src/painting/text_style.rs`

```rust
impl TextStyle {
    /// Builder: set font size
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Builder: set color
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Builder: set font weight
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }
}
```

---

## 8. Compositor Improvements

### Advanced Features

**File**: `crates/flui_engine/src/compositor.rs`

```rust
pub struct CompositorOptions {
    pub enable_culling: bool,
    pub viewport: Rect,
    pub debug_mode: bool,
    pub track_performance: bool,

    // NEW: Layer caching
    pub enable_caching: bool,

    // NEW: Incremental rendering
    pub enable_incremental: bool,
}

pub struct CompositionStats {
    pub composition_time: Duration,
    pub layers_painted: usize,
    pub layers_culled: usize,
    pub painted_bounds: Rect,
    pub frame_number: u64,

    // NEW: Cache statistics
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub cache_memory_bytes: usize,
}

pub struct Compositor {
    options: CompositorOptions,
    stats: CompositionStats,

    // NEW: Layer cache
    layer_cache: HashMap<LayerId, CachedLayer>,

    // NEW: Dirty region tracking
    dirty_regions: Vec<Rect>,
}

impl Compositor {
    /// Composite only dirty regions (incremental rendering)
    pub fn composite_incremental(
        &mut self,
        scene: &Scene,
        painter: &mut dyn Painter,
        dirty_rect: Rect,
    ) {
        if !self.options.enable_incremental {
            return self.composite(scene, painter);
        }

        // Only paint layers that intersect dirty_rect
        self.paint_layer_incremental(scene.root(), painter, dirty_rect);
    }

    /// Enable or disable layer caching
    pub fn set_caching_enabled(&mut self, enabled: bool) {
        self.options.enable_caching = enabled;
        if !enabled {
            self.layer_cache.clear();
        }
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            hits: self.stats.cache_hits,
            misses: self.stats.cache_misses,
            memory_bytes: self.stats.cache_memory_bytes,
            entry_count: self.layer_cache.len(),
        }
    }

    /// Clear the layer cache
    pub fn clear_cache(&mut self) {
        self.layer_cache.clear();
        self.stats.cache_hits = 0;
        self.stats.cache_misses = 0;
        self.stats.cache_memory_bytes = 0;
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub memory_bytes: usize,
    pub entry_count: usize,
}
```

---

## 9. Documentation and Examples

### Complete Example

**File**: `crates/flui_engine/examples/layer_composition.rs`

```rust
//! Complete example showing layer composition with SceneBuilder

use flui_engine::*;
use flui_types::*;

fn main() {
    // Create a scene builder
    let mut builder = SceneBuilder::new();

    // === Background layer ===
    let mut background = PictureLayer::new();
    background.draw_rect(
        Rect::from_xywh(0.0, 0.0, 800.0, 600.0),
        Paint::new().with_color([0.1, 0.1, 0.1, 1.0]), // Dark gray
    );
    builder.add_picture(background);

    // === Transformed content ===
    builder.push_transform(Transform::translate(100.0, 100.0));
    builder.push_opacity(0.8);

    let mut content = PictureLayer::new();
    content.draw_rect(
        Rect::from_xywh(0.0, 0.0, 200.0, 150.0),
        Paint::new().with_color([0.2, 0.6, 0.9, 1.0]), // Blue
    );
    content.draw_circle(
        Point::new(100.0, 75.0),
        50.0,
        Paint::new().with_color([0.9, 0.3, 0.2, 1.0]), // Red
    );
    builder.add_picture(content);

    builder.pop(); // Pop opacity
    builder.pop(); // Pop transform

    // === Clipped overlay ===
    builder.push_clip_rect(Rect::from_xywh(400.0, 200.0, 300.0, 300.0));

    let mut overlay = PictureLayer::new();
    overlay.draw_circle(
        Point::new(550.0, 350.0),
        100.0,
        Paint::new()
            .with_color([0.3, 0.9, 0.3, 1.0]) // Green
            .with_stroke_width(5.0),
    );
    builder.add_picture(overlay);

    builder.pop(); // Pop clip

    // Build the scene
    let scene = builder.build(Size::new(800.0, 600.0));

    println!("Scene metadata:");
    println!("  Layer count: {}", scene.metadata().layer_count);
    println!("  Bounds: {:?}", scene.metadata().bounds);

    // Create compositor
    let mut compositor = Compositor::new();
    compositor.set_viewport(Rect::from_xywh(0.0, 0.0, 800.0, 600.0));

    // Composite (with a mock painter for this example)
    let mut painter = MockPainter::new();
    compositor.composite(&scene, &mut painter);

    println!("\nComposition stats:");
    println!("  Layers painted: {}", compositor.stats().layers_painted);
    println!("  Layers culled: {}", compositor.stats().layers_culled);
    println!("  Composition time: {:?}", compositor.stats().composition_time);
}

// Mock painter for example
struct MockPainter;
impl MockPainter {
    fn new() -> Self { Self }
}
impl Painter for MockPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        println!("  Drawing rect: {:?} with color {:?}", rect, paint.color);
    }
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        println!("  Drawing circle: center={:?}, radius={}, color={:?}",
                 center, radius, paint.color);
    }
    // ... implement other methods ...
}
```

---

## 10. Implementation Priority

### Phase 1: High Priority (Core Architecture)

1. **Unify Layer Trait** → [layer/base.rs](crates/flui_engine/src/layer/base.rs)
   - Remove duplicate trait in `layer/mod.rs`
   - Mark old trait as deprecated
   - Update all implementations

2. **Add SceneBuilder** → [scene_builder.rs](crates/flui_engine/src/scene_builder.rs) (new file)
   - Implement stack-based scene construction
   - Add push/pop methods for layers
   - Integrate with Scene

3. **Extend DrawCommand** → [layer/picture.rs](crates/flui_engine/src/layer/picture.rs)
   - Add Text, Image, Path, Arc, Polygon variants
   - Add corresponding draw_* methods
   - Update command_bounds calculation

4. **Extend Painter Trait** → [painter/mod.rs](crates/flui_engine/src/painter/mod.rs)
   - Add text(), image(), path(), polygon(), arc() methods
   - Provide default implementations
   - Update EguiPainter to implement new methods

### Phase 2: Medium Priority (API Improvements)

5. **Enhance PaintContext** → [paint_context.rs](crates/flui_engine/src/paint_context.rs)
   - Add SceneBuilder integration
   - Add push_layer helpers
   - Add convenience methods for common layers

6. **Improve LayerHandle** → [layer/handle.rs](crates/flui_engine/src/layer/handle.rs)
   - Add with_read/with_write helpers
   - Better lifecycle management
   - Auto-dispose on drop

7. **Add Builder Patterns** → Various files
   - Paint builder methods
   - TextStyle builder methods
   - Other complex types

8. **Add Examples** → [examples/](crates/flui_engine/examples/)
   - layer_composition.rs
   - paint_context_usage.rs
   - custom_painter.rs

### Phase 3: Low Priority (Optimizations)

9. **Layer Caching** → [compositor.rs](crates/flui_engine/src/compositor.rs)
   - Implement RasterCache
   - Add cache hit/miss tracking
   - Memory management

10. **Dirty Region Tracking** → [scene.rs](crates/flui_engine/src/scene.rs)
    - Track changed regions
    - Incremental composition
    - Repaint optimization

---

## Migration Strategy

### Phase 1: Additive Changes (Non-Breaking)

- Add new APIs alongside existing ones
- Mark old APIs as `#[deprecated]`
- Ensure backward compatibility
- Update documentation with migration guide

### Phase 2: Transition Period

- Update all examples to use new APIs
- Update tests to use new APIs
- Provide migration assistance in deprecation warnings

### Phase 3: Cleanup (Breaking Changes)

- Remove deprecated APIs in next major version (0.2.0)
- Update CHANGELOG.md with migration guide
- Ensure all consumers have migrated

---

## Testing Strategy

For each change:

1. **Unit Tests**: Test individual components in isolation
2. **Integration Tests**: Test how components work together
3. **Example Tests**: Ensure examples compile and run
4. **Performance Tests**: Benchmark critical paths
5. **Documentation Tests**: Ensure doc examples compile

---

## Success Metrics

- [ ] All duplicate/conflicting APIs removed
- [ ] SceneBuilder pattern fully functional
- [ ] All planned DrawCommand types implemented
- [ ] All planned Painter methods implemented
- [ ] Full test coverage (>80%) for new code
- [ ] All examples updated and working
- [ ] Documentation complete for all new APIs
- [ ] No performance regressions
- [ ] Zero clippy warnings

---

## References

- **Flutter Rendering Architecture**: https://api.flutter.dev/flutter/rendering/rendering-library.html
- **Flutter Layers**: https://api.flutter.dev/flutter/rendering/Layer-class.html
- **Flutter SceneBuilder**: https://api.flutter.dev/flutter/dart-ui/SceneBuilder-class.html
- **Flutter PaintingContext**: https://api.flutter.dev/flutter/rendering/PaintingContext-class.html

---

## Notes

**Русский**: Этот план основан на анализе текущей архитектуры flui_engine и паттернах Flutter. Главная цель - сделать API более согласованным, мощным и эргономичным, следуя проверенным паттернам Flutter, но адаптированным под идиомы Rust.

**English**: This plan is based on analysis of the current flui_engine architecture and Flutter patterns. The main goal is to make the API more consistent, powerful, and ergonomic by following Flutter's proven patterns adapted to Rust idioms.
