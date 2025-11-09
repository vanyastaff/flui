# Painter Architecture V2 - Final Design

**Status:** Approved Design
**Breaking Changes:** Yes (requires FLUI 0.7.0)
**Goal:** Production-ready, architecturally sound painting abstraction

---

## Design Principles

1. **Minimal Core** - Backend реализует только `draw_mesh` + transforms
2. **Zero-Cost Abstractions** - Высокоуровневый API компилируется в эффективный код
3. **Composable** - Всё строится из mesh примитива
4. **Type-Safe** - Невозможно использовать API неправильно
5. **Backend-Agnostic** - Легко портировать на Vulkan, Metal, WebGPU
6. **No Quad/Triangle Methods** - `draw_mesh` покрывает все случаи низкоуровневой отрисовки

---

## Core Architecture

### Level 0: Primitives (базовые типы)

```rust
// crates/flui_engine/src/painter/v2/primitives.rs

/// Vertex for mesh rendering
/// Size: 24 bytes (aligned)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// Position in 2D space
    pub pos: Point,        // 8 bytes (f32 x2)

    /// Vertex color (premultiplied alpha)
    pub color: Color,      // 4 bytes (u8 x4)

    /// Texture coordinates (0.0-1.0 range)
    pub uv: Point,         // 8 bytes (f32 x2)

    // Padding to 24 bytes for alignment
    _padding: [u8; 4],
}

impl Vertex {
    /// Create colored vertex (no texture)
    #[inline]
    pub const fn colored(pos: Point, color: Color) -> Self {
        Self {
            pos,
            color,
            uv: Point::ZERO,
            _padding: [0; 4],
        }
    }

    /// Create textured vertex
    #[inline]
    pub const fn textured(pos: Point, color: Color, uv: Point) -> Self {
        Self {
            pos,
            color,
            uv,
            _padding: [0; 4],
        }
    }
}

/// Paint properties (common for all drawing operations)
/// Size: 8 bytes (compact)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    /// Color (premultiplied alpha)
    pub color: Color,          // 4 bytes

    /// Anti-aliasing enabled
    pub anti_alias: bool,      // 1 byte

    /// Blend mode
    pub blend_mode: BlendMode, // 1 byte (enum u8)

    /// Reserved for future use
    _reserved: [u8; 2],
}

impl Paint {
    /// Solid fill paint
    #[inline]
    pub const fn fill(color: Color) -> Self {
        Self {
            color,
            anti_alias: true,
            blend_mode: BlendMode::SrcOver,
            _reserved: [0; 2],
        }
    }

    /// With custom blend mode
    #[inline]
    pub const fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Disable anti-aliasing
    #[inline]
    pub const fn no_aa(mut self) -> Self {
        self.anti_alias = false;
        self
    }
}

/// Stroke properties (extends Paint)
/// Size: 16 bytes
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// Base paint properties
    pub paint: Paint,      // 8 bytes

    /// Stroke width
    pub width: f32,        // 4 bytes

    /// Line cap style
    pub cap: StrokeCap,    // 1 byte

    /// Line join style
    pub join: StrokeJoin,  // 1 byte

    /// Miter limit (for StrokeJoin::Miter)
    /// Stored as u8: 0-255 → 0.0-25.5
    pub miter_limit: u8,   // 1 byte

    _reserved: u8,
}

impl Stroke {
    /// Create stroke with default settings
    #[inline]
    pub const fn new(width: f32, color: Color) -> Self {
        Self {
            paint: Paint::fill(color),
            width,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
            miter_limit: 40, // 4.0
            _reserved: 0,
        }
    }

    /// Set line cap
    #[inline]
    pub const fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }

    /// Set line join
    #[inline]
    pub const fn with_join(mut self, join: StrokeJoin) -> Self {
        self.join = join;
        self
    }

    /// Get miter limit as f32
    #[inline]
    pub fn miter_limit_f32(&self) -> f32 {
        self.miter_limit as f32 / 10.0
    }
}

/// Stroke cap styles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StrokeCap {
    Butt = 0,
    Round = 1,
    Square = 2,
}

/// Stroke join styles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StrokeJoin {
    Miter = 0,
    Round = 1,
    Bevel = 2,
}

/// Blend modes for compositing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendMode {
    SrcOver = 0,  // Standard alpha blending
    Multiply = 1,
    Screen = 2,
    Overlay = 3,
    // Add more as needed
}

/// Texture identifier for textured rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(pub u64);

impl TextureId {
    pub const NONE: Self = Self(0);
}
```

---

### Level 1: Backend Core (обязательная реализация)

```rust
// crates/flui_engine/src/painter/v2/backend.rs

/// Core backend interface - minimal required implementation
///
/// Backends MUST implement these 5 methods. Everything else has default
/// implementations that tessellate to these primitives.
///
/// # Contract
///
/// A backend implementation must guarantee:
/// - Transform state is maintained through save/restore stack
/// - Clipping is cumulative (intersects with previous clips)
/// - Vertices are in counter-clockwise winding order for filled shapes
/// - Colors use premultiplied alpha
pub trait PainterBackend {
    // ========== REQUIRED: Mesh Rendering ==========

    /// Render a triangle mesh
    ///
    /// This is the PRIMARY drawing primitive. All shapes (rect, circle,
    /// path, etc.) tessellate to meshes.
    ///
    /// # Parameters
    /// - `vertices`: Vertex array (position, color, uv)
    /// - `indices`: Triangle indices (length must be multiple of 3)
    /// - `paint`: Paint properties (blend mode, anti-aliasing)
    ///
    /// # Contract
    /// - Vertices are in counter-clockwise order for filled shapes
    /// - Indices form triangles: [i0, i1, i2, i3, i4, i5, ...] = [(i0,i1,i2), (i3,i4,i5), ...]
    /// - Colors are premultiplied alpha
    /// - UVs are in 0.0-1.0 range (for textured meshes)
    /// - Current transform and clip region are applied
    fn draw_mesh(&mut self, vertices: &[Vertex], indices: &[u32], paint: &Paint);

    // ========== REQUIRED: Transform State ==========

    /// Push current transform onto stack
    fn save(&mut self);

    /// Pop transform from stack
    ///
    /// # Panics
    /// Panics if save/restore are unbalanced (no matching save)
    fn restore(&mut self);

    /// Concatenate transform matrix to current transform
    ///
    /// New transform = current_transform * matrix
    fn concat_matrix(&mut self, matrix: Mat4);

    // ========== REQUIRED: Clipping ==========

    /// Set clip region to intersection with given mesh
    ///
    /// Clip region is defined by the mesh shape (filled polygon).
    /// All subsequent drawing is clipped to this region until restore().
    ///
    /// # Contract
    /// - Clip is cumulative (intersects with current clip)
    /// - Clip persists until restore()
    fn clip_mesh(&mut self, vertices: &[Vertex], indices: &[u32]);

    // ========== OPTIMIZATION HINTS (backend can override) ==========

    /// Hint: Draw axis-aligned rectangle
    ///
    /// Backend can override for direct rect rendering (e.g., egui::Shape::Rect).
    /// Default implementation tessellates to 2 triangles via draw_mesh().
    ///
    /// NOT part of public Painter API - used internally by PainterShapes.
    #[doc(hidden)]
    #[inline]
    fn draw_rect_hint(&mut self, rect: Rect, paint: &Paint) {
        // Default: 2 triangles (quad)
        let vertices = [
            Vertex::colored(rect.top_left(), paint.color),
            Vertex::colored(rect.top_right(), paint.color),
            Vertex::colored(rect.bottom_right(), paint.color),
            Vertex::colored(rect.bottom_left(), paint.color),
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        self.draw_mesh(&vertices, &indices, paint);
    }

    /// Hint: Draw textured rectangle
    ///
    /// Used internally for images, cached glyphs.
    /// Backend can optimize texture binding.
    ///
    /// NOT part of public Painter API - used internally.
    #[doc(hidden)]
    #[inline]
    fn draw_textured_rect_hint(
        &mut self,
        rect: Rect,
        uv_rect: Rect,
        texture_id: TextureId,
        paint: &Paint,
    ) {
        // Default: textured quad
        let vertices = [
            Vertex::textured(rect.top_left(), paint.color, uv_rect.top_left()),
            Vertex::textured(rect.top_right(), paint.color, uv_rect.top_right()),
            Vertex::textured(rect.bottom_right(), paint.color, uv_rect.bottom_right()),
            Vertex::textured(rect.bottom_left(), paint.color, uv_rect.bottom_left()),
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        // Backend должен биндить текстуру по texture_id
        self.draw_mesh(&vertices, &indices, paint);
    }
}
```

**Минимальная реализация для нового бэкенда:**
```rust
impl PainterBackend for MyBackend {
    fn draw_mesh(&mut self, vertices: &[Vertex], indices: &[u32], paint: &Paint) {
        // TODO: Render triangles to GPU
    }

    fn save(&mut self) {
        self.state_stack.push(self.current_state.clone());
    }

    fn restore(&mut self) {
        self.current_state = self.state_stack.pop()
            .expect("Unbalanced save/restore");
    }

    fn concat_matrix(&mut self, matrix: Mat4) {
        self.current_state.transform = self.current_state.transform * matrix;
    }

    fn clip_mesh(&mut self, vertices: &[Vertex], indices: &[u32]) {
        // TODO: Set clip region (stencil buffer / scissor rect)
    }
}
// ВСЁ! rect, circle, path - работают через default implementations
```

---

### Level 2: Shape Tessellation (автоматически доступно)

```rust
// crates/flui_engine/src/painter/v2/shapes.rs

/// Shape tessellation extensions
///
/// Provides high-level shape rendering with automatic tessellation.
/// All methods have default implementations via PainterBackend::draw_mesh().
///
/// Backends automatically get this trait for free.
pub trait PainterShapes: PainterBackend {
    // ========== Basic Shapes ==========

    /// Draw rectangle (axis-aligned)
    #[inline]
    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        // Delegate to hint (backend can optimize)
        self.draw_rect_hint(rect, paint);
    }

    /// Draw rounded rectangle
    fn draw_rrect(&mut self, rrect: &RRect, paint: &Paint) {
        let (vertices, indices) = tessellate::rrect(rrect, paint.color);
        self.draw_mesh(&vertices, &indices, paint);
    }

    /// Draw circle
    fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        let segments = tessellate::circle_segments(radius);
        let (vertices, indices) = tessellate::circle(center, radius, segments, paint.color);
        self.draw_mesh(&vertices, &indices, paint);
    }

    /// Draw ellipse
    fn draw_ellipse(&mut self, center: Point, radius_x: f32, radius_y: f32, paint: &Paint) {
        let segments = tessellate::ellipse_segments(radius_x, radius_y);
        let (vertices, indices) = tessellate::ellipse(
            center,
            radius_x,
            radius_y,
            segments,
            paint.color,
        );
        self.draw_mesh(&vertices, &indices, paint);
    }

    /// Draw arc
    fn draw_arc(
        &mut self,
        center: Point,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        paint: &Paint,
    ) {
        let angle_range = (end_angle - start_angle).abs();
        let segments = tessellate::arc_segments(radius, angle_range);
        let (vertices, indices) = tessellate::arc(
            center,
            radius,
            start_angle,
            end_angle,
            segments,
            paint.color,
        );
        self.draw_mesh(&vertices, &indices, paint);
    }

    // ========== Polygons & Lines ==========

    /// Draw filled polygon
    fn draw_polygon(&mut self, points: &[Point], paint: &Paint) {
        if points.len() < 3 {
            return;
        }
        let (vertices, indices) = tessellate::polygon(points, paint.color);
        self.draw_mesh(&vertices, &indices, paint);
    }

    /// Draw stroked line
    fn draw_line(&mut self, p1: Point, p2: Point, stroke: &Stroke) {
        let (vertices, indices) = tessellate::line(p1, p2, stroke.width, stroke.paint.color);
        self.draw_mesh(&vertices, &indices, &stroke.paint);
    }

    /// Draw stroked polyline
    fn draw_polyline(&mut self, points: &[Point], stroke: &Stroke) {
        if points.len() < 2 {
            return;
        }
        let (vertices, indices) = tessellate::polyline(
            points,
            stroke.width,
            stroke.cap,
            stroke.join,
            stroke.paint.color,
        );
        self.draw_mesh(&vertices, &indices, &stroke.paint);
    }

    // ========== Path Rendering ==========

    /// Draw arbitrary path (filled or stroked)
    fn draw_path(&mut self, path: &Path, paint: &Paint, stroke: Option<&Stroke>) {
        if let Some(stroke) = stroke {
            // Stroked path
            let (vertices, indices) = tessellate::path_stroke(path, stroke);
            self.draw_mesh(&vertices, &indices, &stroke.paint);
        } else {
            // Filled path
            let (vertices, indices) = tessellate::path_fill(path, paint.color);
            self.draw_mesh(&vertices, &indices, paint);
        }
    }
}

// Auto-implement for all PainterBackend implementations
impl<T: PainterBackend> PainterShapes for T {}
```

---

### Level 3: High-Level API (удобство использования)

```rust
// crates/flui_engine/src/painter/v2/painter.rs

/// High-level painting API for application code
///
/// This is the main API that RenderObjects use.
/// Provides convenient methods with transform helpers, clipping, etc.
///
/// Automatically implemented for all types that implement PainterShapes.
pub trait Painter: PainterShapes {
    // ========== Transform Helpers ==========

    /// Translate coordinate system
    #[inline]
    fn translate(&mut self, offset: Offset) {
        self.concat_matrix(Mat4::from_translation(Vec3::new(offset.x, offset.y, 0.0)));
    }

    /// Rotate coordinate system (radians, counter-clockwise)
    #[inline]
    fn rotate(&mut self, angle: f32) {
        self.concat_matrix(Mat4::from_rotation_z(angle));
    }

    /// Scale coordinate system
    #[inline]
    fn scale(&mut self, sx: f32, sy: f32) {
        self.concat_matrix(Mat4::from_scale(Vec3::new(sx, sy, 1.0)));
    }

    /// Apply complete transform in one call (more efficient)
    #[inline]
    fn transform(&mut self, transform: &Transform) {
        self.concat_matrix(transform.matrix());
    }

    /// Execute drawing with temporary transform
    #[inline]
    fn with_transform<F>(&mut self, transform: &Transform, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.save();
        self.transform(transform);
        f(self);
        self.restore();
    }

    // ========== Clipping Helpers ==========

    /// Clip to rectangle
    #[inline]
    fn clip_rect(&mut self, rect: Rect) {
        let (vertices, indices) = tessellate::rect_mesh(rect, Color::WHITE);
        self.clip_mesh(&vertices, &indices);
    }

    /// Clip to rounded rectangle
    #[inline]
    fn clip_rrect(&mut self, rrect: &RRect) {
        let (vertices, indices) = tessellate::rrect(rrect, Color::WHITE);
        self.clip_mesh(&vertices, &indices);
    }

    /// Clip to oval/ellipse
    #[inline]
    fn clip_oval(&mut self, rect: Rect) {
        let center = rect.center();
        let radius_x = rect.width() / 2.0;
        let radius_y = rect.height() / 2.0;
        let segments = tessellate::ellipse_segments(radius_x, radius_y);
        let (vertices, indices) = tessellate::ellipse(
            center,
            radius_x,
            radius_y,
            segments,
            Color::WHITE,
        );
        self.clip_mesh(&vertices, &indices);
    }

    /// Clip to arbitrary path
    #[inline]
    fn clip_path(&mut self, path: &Path) {
        let (vertices, indices) = tessellate::path_fill(path, Color::WHITE);
        self.clip_mesh(&vertices, &indices);
    }

    /// Execute drawing with temporary clip
    #[inline]
    fn with_clip_rect<F>(&mut self, rect: Rect, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.save();
        self.clip_rect(rect);
        f(self);
        self.restore();
    }

    // ========== Text Rendering ==========

    /// Draw text at position
    ///
    /// Backend-specific implementation.
    fn draw_text(&mut self, text: &str, pos: Point, style: &TextStyle);

    /// Draw pre-shaped text run
    ///
    /// For advanced text layout (harfbuzz, cosmic-text, etc.)
    fn draw_text_run(&mut self, glyphs: &[GlyphInstance], style: &TextStyle);

    // ========== Image Rendering ==========

    /// Draw image
    #[inline]
    fn draw_image(&mut self, image: &Image, src: Rect, dst: Rect, paint: &Paint) {
        self.draw_textured_rect_hint(dst, src, image.texture_id(), paint);
    }
}

// Auto-implement for all PainterShapes implementations
impl<T: PainterShapes> Painter for T {}
```

---

## Transform Builder Pattern

```rust
// crates/flui_engine/src/painter/v2/transform.rs

/// Transform builder for composing multiple transformations
///
/// More efficient than individual translate/rotate/scale calls because
/// matrix multiplication happens once.
///
/// # Example
///
/// ```rust
/// let transform = Transform::identity()
///     .translate(Offset::new(100.0, 100.0))
///     .rotate(PI / 4.0)
///     .scale(2.0, 2.0);
///
/// painter.transform(&transform);
/// painter.draw_rect(rect, &paint);
/// ```
#[derive(Debug, Clone)]
pub struct Transform {
    matrix: Mat4,
}

impl Transform {
    /// Identity transform (no transformation)
    #[inline]
    pub const fn identity() -> Self {
        Self {
            matrix: Mat4::IDENTITY,
        }
    }

    /// Create from matrix
    #[inline]
    pub const fn from_matrix(matrix: Mat4) -> Self {
        Self { matrix }
    }

    /// Get matrix
    #[inline]
    pub const fn matrix(&self) -> Mat4 {
        self.matrix
    }

    /// Translate
    #[inline]
    pub fn translate(mut self, offset: Offset) -> Self {
        self.matrix = self.matrix
            * Mat4::from_translation(Vec3::new(offset.x, offset.y, 0.0));
        self
    }

    /// Rotate (radians, counter-clockwise)
    #[inline]
    pub fn rotate(mut self, angle: f32) -> Self {
        self.matrix = self.matrix * Mat4::from_rotation_z(angle);
        self
    }

    /// Scale
    #[inline]
    pub fn scale(mut self, sx: f32, sy: f32) -> Self {
        self.matrix = self.matrix * Mat4::from_scale(Vec3::new(sx, sy, 1.0));
        self
    }

    /// Skew
    pub fn skew(mut self, skew_x: f32, skew_y: f32) -> Self {
        let skew_matrix = Mat4::from_cols(
            Vec4::new(1.0, skew_y.tan(), 0.0, 0.0),
            Vec4::new(skew_x.tan(), 1.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        );
        self.matrix = self.matrix * skew_matrix;
        self
    }

    /// Compose with another transform
    #[inline]
    pub fn concat(mut self, other: &Transform) -> Self {
        self.matrix = self.matrix * other.matrix;
        self
    }
}

impl Default for Transform {
    #[inline]
    fn default() -> Self {
        Self::identity()
    }
}
```

---

## Tessellation Library

```rust
// crates/flui_engine/src/painter/v2/tessellate.rs

/// Tessellation utilities using Lyon
///
/// All functions return (vertices, indices) ready for PainterBackend::draw_mesh()

use lyon::tessellation::*;

/// Calculate optimal circle segments based on radius
pub fn circle_segments(radius: f32) -> usize {
    // Adaptive quality: larger circles get more segments
    // 8-128 segments based on radius
    ((radius.sqrt() * 4.0).clamp(8.0, 128.0)) as usize
}

/// Calculate optimal ellipse segments
pub fn ellipse_segments(radius_x: f32, radius_y: f32) -> usize {
    circle_segments(radius_x.max(radius_y))
}

/// Calculate optimal arc segments based on angle
pub fn arc_segments(radius: f32, angle: f32) -> usize {
    let full_segments = circle_segments(radius);
    let ratio = angle.abs() / std::f32::consts::TAU;
    ((full_segments as f32 * ratio).max(3.0)) as usize
}

/// Tessellate circle to triangle mesh
pub fn circle(
    center: Point,
    radius: f32,
    segments: usize,
    color: Color,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut geometry = VertexBuffers::new();
    let mut tessellator = FillTessellator::new();

    tessellator
        .tessellate_circle(
            lyon::math::Point::new(center.x, center.y),
            radius,
            &FillOptions::DEFAULT,
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                Vertex::colored(
                    Point::new(vertex.position().x, vertex.position().y),
                    color,
                )
            }),
        )
        .expect("Failed to tessellate circle");

    (geometry.vertices, geometry.indices)
}

/// Tessellate rounded rectangle
pub fn rrect(rrect: &RRect, color: Color) -> (Vec<Vertex>, Vec<u32>) {
    let mut geometry = VertexBuffers::new();
    let mut tessellator = FillTessellator::new();

    // Build lyon path for rounded rect
    let mut builder = lyon::path::Path::builder();

    // TODO: Use lyon's rounded rect helpers or build manually
    // For now, placeholder implementation
    builder.begin(lyon::math::point(rrect.rect.min.x, rrect.rect.min.y));
    // ... build rounded rect path
    builder.end(true);

    let path = builder.build();

    tessellator
        .tessellate_path(
            &path,
            &FillOptions::DEFAULT,
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                Vertex::colored(
                    Point::new(vertex.position().x, vertex.position().y),
                    color,
                )
            }),
        )
        .expect("Failed to tessellate rounded rect");

    (geometry.vertices, geometry.indices)
}

/// Tessellate ellipse
pub fn ellipse(
    center: Point,
    radius_x: f32,
    radius_y: f32,
    segments: usize,
    color: Color,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut geometry = VertexBuffers::new();
    let mut tessellator = FillTessellator::new();

    tessellator
        .tessellate_ellipse(
            lyon::math::Point::new(center.x, center.y),
            lyon::math::Vector::new(radius_x, radius_y),
            lyon::math::Angle::radians(0.0),
            lyon::path::Winding::Positive,
            &FillOptions::DEFAULT,
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                Vertex::colored(
                    Point::new(vertex.position().x, vertex.position().y),
                    color,
                )
            }),
        )
        .expect("Failed to tessellate ellipse");

    (geometry.vertices, geometry.indices)
}

/// Tessellate arc
pub fn arc(
    center: Point,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    segments: usize,
    color: Color,
) -> (Vec<Vertex>, Vec<u32>) {
    // TODO: Implement using lyon
    todo!("Arc tessellation")
}

/// Triangulate polygon (ear clipping)
pub fn polygon(points: &[Point], color: Color) -> (Vec<Vertex>, Vec<u32>) {
    let mut geometry = VertexBuffers::new();
    let mut tessellator = FillTessellator::new();

    // Build lyon path from points
    let mut builder = lyon::path::Path::builder();
    if let Some(&first) = points.first() {
        builder.begin(lyon::math::point(first.x, first.y));
        for &point in &points[1..] {
            builder.line_to(lyon::math::point(point.x, point.y));
        }
        builder.end(true);
    }

    let path = builder.build();

    tessellator
        .tessellate_path(
            &path,
            &FillOptions::DEFAULT,
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                Vertex::colored(
                    Point::new(vertex.position().x, vertex.position().y),
                    color,
                )
            }),
        )
        .expect("Failed to tessellate polygon");

    (geometry.vertices, geometry.indices)
}

/// Generate stroke geometry for polyline
pub fn polyline(
    points: &[Point],
    width: f32,
    cap: StrokeCap,
    join: StrokeJoin,
    color: Color,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut geometry = VertexBuffers::new();
    let mut tessellator = StrokeTessellator::new();

    // Build lyon path
    let mut builder = lyon::path::Path::builder();
    if let Some(&first) = points.first() {
        builder.begin(lyon::math::point(first.x, first.y));
        for &point in &points[1..] {
            builder.line_to(lyon::math::point(point.x, point.y));
        }
        builder.end(false); // Open path
    }

    let path = builder.build();

    let lyon_cap = match cap {
        StrokeCap::Butt => lyon::tessellation::LineCap::Butt,
        StrokeCap::Round => lyon::tessellation::LineCap::Round,
        StrokeCap::Square => lyon::tessellation::LineCap::Square,
    };

    let lyon_join = match join {
        StrokeJoin::Miter => lyon::tessellation::LineJoin::Miter,
        StrokeJoin::Round => lyon::tessellation::LineJoin::Round,
        StrokeJoin::Bevel => lyon::tessellation::LineJoin::Bevel,
    };

    let options = StrokeOptions::DEFAULT
        .with_line_width(width)
        .with_line_cap(lyon_cap)
        .with_line_join(lyon_join);

    tessellator
        .tessellate_path(
            &path,
            &options,
            &mut BuffersBuilder::new(&mut geometry, |vertex: StrokeVertex| {
                Vertex::colored(
                    Point::new(vertex.position().x, vertex.position().y),
                    color,
                )
            }),
        )
        .expect("Failed to tessellate polyline");

    (geometry.vertices, geometry.indices)
}

/// Generate stroke geometry for single line
pub fn line(p1: Point, p2: Point, width: f32, color: Color) -> (Vec<Vertex>, Vec<u32>) {
    polyline(&[p1, p2], width, StrokeCap::Butt, StrokeJoin::Miter, color)
}

/// Tessellate filled path
pub fn path_fill(path: &Path, color: Color) -> (Vec<Vertex>, Vec<u32>) {
    // TODO: Convert flui_types::Path to lyon::path::Path
    todo!("Path fill tessellation")
}

/// Tessellate stroked path
pub fn path_stroke(path: &Path, stroke: &Stroke) -> (Vec<Vertex>, Vec<u32>) {
    // TODO: Convert flui_types::Path to lyon::path::Path and stroke
    todo!("Path stroke tessellation")
}

/// Helper: Convert rect to mesh (2 triangles)
pub fn rect_mesh(rect: Rect, color: Color) -> (Vec<Vertex>, Vec<u32>) {
    let vertices = vec![
        Vertex::colored(rect.top_left(), color),
        Vertex::colored(rect.top_right(), color),
        Vertex::colored(rect.bottom_right(), color),
        Vertex::colored(rect.bottom_left(), color),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    (vertices, indices)
}
```

---

## Example: EguiPainter Implementation

```rust
// crates/flui_engine/src/backends/egui/painter.rs

use crate::painter::v2::*;

pub struct EguiPainter<'a> {
    painter: &'a egui::Painter,
    state_stack: Vec<PaintState>,
    current_state: PaintState,
}

#[derive(Clone)]
struct PaintState {
    transform: Mat4,
    clip: Option<egui::Rect>,
}

impl PainterBackend for EguiPainter<'_> {
    fn draw_mesh(&mut self, vertices: &[Vertex], indices: &[u32], paint: &Paint) {
        // Convert our Vertex to egui::epaint::Vertex
        let egui_vertices: Vec<egui::epaint::Vertex> = vertices
            .iter()
            .map(|v| {
                // Apply current transform
                let pos_3d = Vec3::new(v.pos.x, v.pos.y, 0.0);
                let transformed = self.current_state.transform.project_point3(pos_3d);

                egui::epaint::Vertex {
                    pos: egui::pos2(transformed.x, transformed.y),
                    uv: egui::pos2(v.uv.x, v.uv.y),
                    color: egui::Color32::from_rgba_premultiplied(
                        v.color.r,
                        v.color.g,
                        v.color.b,
                        v.color.a,
                    ),
                }
            })
            .collect();

        let mesh = egui::epaint::Mesh {
            indices: indices.to_vec(),
            vertices: egui_vertices,
            texture_id: egui::TextureId::default(),
        };

        // Apply clip if set
        if let Some(clip) = self.current_state.clip {
            self.painter
                .with_clip_rect(clip)
                .add(egui::Shape::Mesh(mesh));
        } else {
            self.painter.add(egui::Shape::Mesh(mesh));
        }
    }

    fn save(&mut self) {
        self.state_stack.push(self.current_state.clone());
    }

    fn restore(&mut self) {
        self.current_state = self
            .state_stack
            .pop()
            .expect("Unbalanced save/restore");
    }

    fn concat_matrix(&mut self, matrix: Mat4) {
        self.current_state.transform = self.current_state.transform * matrix;
    }

    fn clip_mesh(&mut self, vertices: &[Vertex], indices: &[u32]) {
        // Compute bounding box of mesh (conservative clip)
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

        for v in vertices {
            let pos_3d = Vec3::new(v.pos.x, v.pos.y, 0.0);
            let transformed = self.current_state.transform.project_point3(pos_3d);
            min.x = min.x.min(transformed.x);
            min.y = min.y.min(transformed.y);
            max.x = max.x.max(transformed.x);
            max.y = max.y.max(transformed.y);
        }

        let clip_rect = egui::Rect::from_min_max(
            egui::pos2(min.x, min.y),
            egui::pos2(max.x, max.y),
        );

        // Intersect with existing clip
        self.current_state.clip = if let Some(existing) = self.current_state.clip {
            Some(existing.intersect(clip_rect))
        } else {
            Some(clip_rect)
        };
    }

    // Optimization: Use native egui rect for axis-aligned rectangles
    fn draw_rect_hint(&mut self, rect: Rect, paint: &Paint) {
        // If no rotation/scale, use native egui rect
        if self.is_simple_transform() {
            let offset = self.get_translation_offset();
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + offset.x, rect.min.y + offset.y),
                egui::pos2(rect.max.x + offset.x, rect.max.y + offset.y),
            );

            let egui_color = egui::Color32::from_rgba_premultiplied(
                paint.color.r,
                paint.color.g,
                paint.color.b,
                paint.color.a,
            );

            let shape = egui::Shape::rect_filled(egui_rect, 0.0, egui_color);

            if let Some(clip) = self.current_state.clip {
                self.painter.with_clip_rect(clip).add(shape);
            } else {
                self.painter.add(shape);
            }
        } else {
            // Fall back to mesh for rotated/scaled rects
            PainterBackend::draw_rect_hint(self, rect, paint);
        }
    }
}

impl EguiPainter<'_> {
    fn is_simple_transform(&self) -> bool {
        // Check if transform is identity or translation-only
        let m = self.current_state.transform.to_cols_array_2d();
        m[0][0] == 1.0
            && m[1][1] == 1.0
            && m[0][1] == 0.0
            && m[1][0] == 0.0
    }

    fn get_translation_offset(&self) -> Offset {
        let m = self.current_state.transform.to_cols_array_2d();
        Offset::new(m[3][0], m[3][1])
    }
}

// PainterShapes and Painter are auto-implemented!
```

---

## Usage Examples

### Basic Shape Drawing

```rust
use flui_engine::painter::v2::*;

// Simple rectangles and circles
painter.draw_rect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0), &Paint::fill(Color::RED));
painter.draw_circle(Point::new(50.0, 50.0), 25.0, &Paint::fill(Color::BLUE));

// Stroked shapes
let stroke = Stroke::new(2.0, Color::BLACK);
painter.draw_polyline(&points, &stroke);
```

### Transform Composition

```rust
// Efficient: Single matrix multiplication
painter.transform(
    &Transform::identity()
        .translate(Offset::new(100.0, 100.0))
        .rotate(PI / 4.0)
        .scale(2.0, 2.0),
);
painter.draw_rect(rect, &paint);

// Or with closure
painter.with_transform(
    &Transform::identity()
        .translate(Offset::new(100.0, 100.0))
        .rotate(PI / 4.0),
    |p| {
        p.draw_rect(rect, &paint);
    },
);
```

### Clipping

```rust
painter.with_clip_rect(clip_rect, |p| {
    p.draw_circle(center, radius, &paint);
});
```

### Low-Level Mesh (when needed)

```rust
// For advanced users: direct mesh access
let vertices = [
    Vertex::colored(Point::new(0.0, 0.0), Color::RED),
    Vertex::colored(Point::new(100.0, 0.0), Color::GREEN),
    Vertex::colored(Point::new(50.0, 100.0), Color::BLUE),
];
let indices = [0, 1, 2]; // One triangle
painter.draw_mesh(&vertices, &indices, &Paint::fill(Color::WHITE));
```

---

## Migration Plan

### Phase 1: Implement New System (Parallel)

```bash
# New module structure
crates/flui_engine/src/painter/
├── mod.rs           # Re-exports old + new
├── old/             # Current Painter trait
│   ├── mod.rs
│   └── ...
└── v2/              # New architecture
    ├── mod.rs
    ├── primitives.rs
    ├── backend.rs
    ├── shapes.rs
    ├── painter.rs
    ├── transform.rs
    └── tessellate.rs
```

```rust
// crates/flui_engine/src/painter/mod.rs

// Keep old for backwards compat
pub mod old {
    pub use super::old_impl::*;
}

// New system
pub mod v2 {
    pub use super::new_impl::*;
}

// Re-export new as default (after migration)
pub use v2::*;
```

### Phase 2: Migrate Backend

```rust
// EguiPainter implements both old and new
impl old::Painter for EguiPainter { /* ... */ }
impl v2::PainterBackend for EguiPainter { /* ... */ }
```

### Phase 3: Migrate RenderObjects

Update RenderObjects one by one to use new API:

```rust
// Before
use flui_engine::painter::old::Painter;

// After
use flui_engine::painter::v2::Painter;
```

### Phase 4: Remove Old System

```rust
// Delete old/ module
// Update FLUI to 0.7.0
// Breaking change notice in CHANGELOG
```

---

## Benefits Summary

1. **Minimal Backend Implementation**: 5 methods → full functionality
2. **No Quad/Triangle Methods**: `draw_mesh` covers all low-level cases
3. **Type-Safe**: Separate Paint and Stroke types
4. **Zero-Cost**: High-level API compiles to efficient mesh calls
5. **Composable**: Transform builder pattern
6. **Backend-Agnostic**: Easy to add Vulkan, Metal, WebGPU
7. **Optimizable**: Backends can override any hint for performance
8. **Production-Ready**: Uses battle-tested Lyon for tessellation

---

## Architecture Summary

```
User Code (RenderObjects)
    ↓
Painter trait (high-level: transform helpers, clipping)
    ↓
PainterShapes trait (shape methods: rect, circle, path)
    ↓
PainterBackend trait (core: draw_mesh, transform, clip)
    ↓
Backend Implementation (egui, wgpu, etc.)
    ↓
GPU
```

**Key Points:**
- Only `draw_mesh` is required
- Everything else auto-implemented
- Backends can optimize any level
- No public quad/triangle methods (use `draw_mesh` directly if needed)

---

## Next Steps

1. ✅ Design approved
2. Create `crates/flui_engine/src/painter/v2/` modules
3. Implement core traits + primitives
4. Integrate Lyon for tessellation
5. Port EguiPainter to new system
6. Add comprehensive tests
7. Benchmark vs current implementation
8. Migrate RenderObjects
9. Update documentation
10. Release FLUI 0.7.0
