# Chapter 5: Layers & Compositing

## 📋 Overview

Layer система - это выходной формат paint фазы. Вместо рисования напрямую на screen, RenderObjects создают **Layer tree** - hierarchical structure оптимизированную для GPU compositing. Painter API предоставляет drawing primitives для создания PictureLayers.

## 🎨 Layer System

### Layer Hierarchy

```
Layer (trait)
  ├── ContainerLayer          - holds children, no content
  │   ├── OffsetLayer         - positions child with offset
  │   ├── TransformLayer      - 2D/3D transforms
  │   ├── OpacityLayer        - alpha blending
  │   └── ClipLayer           - clipping region
  │       ├── ClipRectLayer
  │       ├── ClipRRectLayer
  │       └── ClipPathLayer
  │
  └── PictureLayer            - actual drawing commands
```

### Base Layer Trait

```rust
/// Base Layer trait
pub trait Layer: Debug + Send + Sync {
    /// Get layer type (for debug/inspection)
    fn layer_type(&self) -> &'static str;
    
    /// Get bounding rect
    fn bounds(&self) -> Rect;
    
    /// Visit children (for compositing)
    fn visit_children(&self, visitor: &mut dyn FnMut(&BoxedLayer));
    
    /// Add child (for ContainerLayer)
    fn add_child(&mut self, child: BoxedLayer) {
        panic!("Cannot add child to {}", self.layer_type());
    }
    
    /// Convert to trait object
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub type BoxedLayer = Box<dyn Layer>;
```

---

## 📦 Container Layers

### 1. ContainerLayer (Base)

```rust
/// ContainerLayer - holds children, no content
#[derive(Debug)]
pub struct ContainerLayer {
    children: Vec<BoxedLayer>,
    bounds: Rect,
}

impl ContainerLayer {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            bounds: Rect::ZERO,
        }
    }
    
    pub fn with_children(children: Vec<BoxedLayer>) -> Self {
        let mut layer = Self::new();
        layer.children = children;
        layer.update_bounds();
        layer
    }
    
    fn update_bounds(&mut self) {
        if self.children.is_empty() {
            self.bounds = Rect::ZERO;
            return;
        }
        
        let mut bounds = self.children[0].bounds();
        for child in &self.children[1..] {
            bounds = bounds.union(child.bounds());
        }
        self.bounds = bounds;
    }
}

impl Layer for ContainerLayer {
    fn layer_type(&self) -> &'static str {
        "Container"
    }
    
    fn bounds(&self) -> Rect {
        self.bounds
    }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&BoxedLayer)) {
        for child in &self.children {
            visitor(child);
        }
    }
    
    fn add_child(&mut self, child: BoxedLayer) {
        self.children.push(child);
        self.update_bounds();
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

### 2. OffsetLayer

```rust
/// OffsetLayer - positions child with offset
#[derive(Debug)]
pub struct OffsetLayer {
    offset: Offset,
    child: Option<BoxedLayer>,
}

impl OffsetLayer {
    pub fn new(offset: Offset) -> Self {
        Self {
            offset,
            child: None,
        }
    }
    
    pub fn with_child(offset: Offset, child: BoxedLayer) -> Self {
        Self {
            offset,
            child: Some(child),
        }
    }
}

impl Layer for OffsetLayer {
    fn layer_type(&self) -> &'static str {
        "Offset"
    }
    
    fn bounds(&self) -> Rect {
        self.child.as_ref()
            .map(|c| c.bounds().translate(self.offset))
            .unwrap_or(Rect::ZERO)
    }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&BoxedLayer)) {
        if let Some(child) = &self.child {
            visitor(child);
        }
    }
    
    fn add_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

### 3. TransformLayer

```rust
/// TransformLayer - 2D/3D transforms
#[derive(Debug)]
pub struct TransformLayer {
    transform: Mat4,
    child: Option<BoxedLayer>,
}

impl TransformLayer {
    pub fn new(transform: Mat4) -> Self {
        Self {
            transform,
            child: None,
        }
    }
    
    /// Create translation transform
    pub fn translate(offset: Offset) -> Self {
        Self::new(Mat4::translate(offset.x, offset.y, 0.0))
    }
    
    /// Create rotation transform
    pub fn rotate(angle: f32) -> Self {
        Self::new(Mat4::rotate_z(angle))
    }
    
    /// Create scale transform
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self::new(Mat4::scale(sx, sy, 1.0))
    }
}

impl Layer for TransformLayer {
    fn layer_type(&self) -> &'static str {
        "Transform"
    }
    
    fn bounds(&self) -> Rect {
        self.child.as_ref()
            .map(|c| c.bounds().transform(&self.transform))
            .unwrap_or(Rect::ZERO)
    }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&BoxedLayer)) {
        if let Some(child) = &self.child {
            visitor(child);
        }
    }
    
    fn add_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

### 4. OpacityLayer

```rust
/// OpacityLayer - alpha blending
#[derive(Debug)]
pub struct OpacityLayer {
    opacity: f32,  // 0.0 - 1.0
    child: Option<BoxedLayer>,
}

impl OpacityLayer {
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            child: None,
        }
    }
    
    pub fn with_child(opacity: f32, child: BoxedLayer) -> Self {
        let mut layer = Self::new(opacity);
        layer.child = Some(child);
        layer
    }
}

impl Layer for OpacityLayer {
    fn layer_type(&self) -> &'static str {
        "Opacity"
    }
    
    fn bounds(&self) -> Rect {
        self.child.as_ref()
            .map(|c| c.bounds())
            .unwrap_or(Rect::ZERO)
    }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&BoxedLayer)) {
        if let Some(child) = &self.child {
            visitor(child);
        }
    }
    
    fn add_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

### 5. ClipLayer

```rust
/// ClipLayer - clipping region
#[derive(Debug)]
pub struct ClipLayer {
    clip_behavior: ClipBehavior,
    clip_shape: ClipShape,
    child: Option<BoxedLayer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBehavior {
    /// No clipping
    None,
    
    /// Hard clip (no anti-aliasing)
    HardEdge,
    
    /// Anti-aliased clip
    AntiAlias,
    
    /// Anti-aliased with saveLayer (expensive)
    AntiAliasWithSaveLayer,
}

#[derive(Debug, Clone)]
pub enum ClipShape {
    /// Rectangular clip
    Rect(Rect),
    
    /// Rounded rect clip
    RRect(RRect),
    
    /// Arbitrary path clip
    Path(Path),
}

impl ClipLayer {
    pub fn rect(rect: Rect) -> Self {
        Self {
            clip_behavior: ClipBehavior::AntiAlias,
            clip_shape: ClipShape::Rect(rect),
            child: None,
        }
    }
    
    pub fn rrect(rrect: RRect) -> Self {
        Self {
            clip_behavior: ClipBehavior::AntiAlias,
            clip_shape: ClipShape::RRect(rrect),
            child: None,
        }
    }
    
    pub fn path(path: Path) -> Self {
        Self {
            clip_behavior: ClipBehavior::AntiAlias,
            clip_shape: ClipShape::Path(path),
            child: None,
        }
    }
}

impl Layer for ClipLayer {
    fn layer_type(&self) -> &'static str {
        match self.clip_shape {
            ClipShape::Rect(_) => "ClipRect",
            ClipShape::RRect(_) => "ClipRRect",
            ClipShape::Path(_) => "ClipPath",
        }
    }
    
    fn bounds(&self) -> Rect {
        let clip_bounds = match &self.clip_shape {
            ClipShape::Rect(rect) => *rect,
            ClipShape::RRect(rrect) => rrect.bounding_rect(),
            ClipShape::Path(path) => path.bounding_rect(),
        };
        
        if let Some(child) = &self.child {
            child.bounds().intersect(clip_bounds)
        } else {
            clip_bounds
        }
    }
    
    fn visit_children(&self, visitor: &mut dyn FnMut(&BoxedLayer)) {
        if let Some(child) = &self.child {
            visitor(child);
        }
    }
    
    fn add_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

---

## 🖼️ PictureLayer - Drawing Commands

### Definition

```rust
/// PictureLayer - contains actual drawing commands
#[derive(Debug)]
pub struct PictureLayer {
    /// Recorded picture
    picture: Picture,

    /// Bounds of picture
    bounds: Rect,
}

impl PictureLayer {
    pub fn new(picture: Picture) -> Self {
        let bounds = picture.bounds();
        Self { picture, bounds }
    }
}

impl Layer for PictureLayer {
    fn layer_type(&self) -> &'static str {
        "Picture"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(&BoxedLayer)) {
        // Picture layers have no children
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Picture - recorded drawing commands
#[derive(Debug, Clone)]
pub struct Picture {
    commands: Arc<Vec<DrawCommand>>,
    bounds: Rect,
}

impl Picture {
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }
}

/// Individual drawing command
#[derive(Debug, Clone)]
pub enum DrawCommand {
    /// Draw rectangle
    DrawRect {
        rect: Rect,
        paint: Paint,
    },
    
    /// Draw rounded rectangle
    DrawRRect {
        rrect: RRect,
        paint: Paint,
    },
    
    /// Draw circle
    DrawCircle {
        center: Offset,
        radius: f32,
        paint: Paint,
    },
    
    /// Draw path
    DrawPath {
        path: Path,
        paint: Paint,
    },
    
    /// Draw text/paragraph
    DrawParagraph {
        paragraph: Arc<Paragraph>,
        offset: Offset,
    },
    
    /// Draw image
    DrawImage {
        image: Arc<Image>,
        offset: Offset,
        paint: Paint,
    },
    
    /// Draw image rect
    DrawImageRect {
        image: Arc<Image>,
        src: Rect,
        dst: Rect,
        paint: Paint,
    },
    
    /// Save canvas state
    Save,
    
    /// Restore canvas state
    Restore,
    
    /// Translate
    Translate {
        dx: f32,
        dy: f32,
    },
    
    /// Rotate
    Rotate {
        angle: f32,
    },
    
    /// Scale
    Scale {
        sx: f32,
        sy: f32,
    },
    
    /// Clip rect
    ClipRect {
        rect: Rect,
        clip_op: ClipOp,
    },
    
    /// Clip path
    ClipPath {
        path: Path,
        clip_op: ClipOp,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOp {
    Difference,
    Intersect,
}
```

### PictureRecorder

```rust
/// Records drawing commands into a Picture
pub struct PictureRecorder {
    commands: Vec<DrawCommand>,
    bounds: Rect,
}

impl PictureRecorder {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
        }
    }
    
    /// Finish recording and return Picture
    pub fn finish(self) -> Picture {
        Picture {
            commands: Arc::new(self.commands),
            bounds: self.bounds,
        }
    }
    
    /// Get mutable canvas for drawing
    pub fn canvas(&mut self) -> Canvas {
        Canvas {
            recorder: self,
        }
    }
    
    fn record(&mut self, command: DrawCommand) {
        self.commands.push(command);
        self.update_bounds(&command);
    }
    
    fn update_bounds(&mut self, command: &DrawCommand) {
        let command_bounds = match command {
            DrawCommand::DrawRect { rect, .. } => *rect,
            DrawCommand::DrawCircle { center, radius, .. } => {
                Rect::from_circle(*center, *radius)
            }
            DrawCommand::DrawPath { path, .. } => path.bounding_rect(),
            DrawCommand::DrawParagraph { paragraph, offset } => {
                Rect::from_offset_size(
                    *offset,
                    Size::new(paragraph.width(), paragraph.height()),
                )
            }
            // ... other commands
            _ => return,
        };
        
        self.bounds = self.bounds.union(command_bounds);
    }
}
```

---

## 🎨 Painter API (Canvas)

### Canvas - Drawing Interface

```rust
/// Canvas - high-level drawing API
pub struct Canvas<'a> {
    recorder: &'a mut PictureRecorder,
}

impl<'a> Canvas<'a> {
    // ═══════════════════════════════════════════════════════════════
    // Primitive Shapes
    // ═══════════════════════════════════════════════════════════════
    
    /// Draw rectangle
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.recorder.record(DrawCommand::DrawRect {
            rect,
            paint: paint.clone(),
        });
    }
    
    /// Draw rounded rectangle
    pub fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.recorder.record(DrawCommand::DrawRRect {
            rrect,
            paint: paint.clone(),
        });
    }
    
    /// Draw circle
    pub fn draw_circle(&mut self, center: Offset, radius: f32, paint: &Paint) {
        self.recorder.record(DrawCommand::DrawCircle {
            center,
            radius,
            paint: paint.clone(),
        });
    }
    
    /// Draw oval
    pub fn draw_oval(&mut self, rect: Rect, paint: &Paint) {
        let path = Path::oval(rect);
        self.draw_path(&path, paint);
    }
    
    /// Draw line
    pub fn draw_line(&mut self, p1: Offset, p2: Offset, paint: &Paint) {
        let mut path = Path::new();
        path.move_to(p1);
        path.line_to(p2);
        self.draw_path(&path, paint);
    }
    
    /// Draw path
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.recorder.record(DrawCommand::DrawPath {
            path: path.clone(),
            paint: paint.clone(),
        });
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Text
    // ═══════════════════════════════════════════════════════════════
    
    /// Draw paragraph (pre-laid-out text)
    pub fn draw_paragraph(&mut self, paragraph: &Arc<Paragraph>, offset: Offset) {
        self.recorder.record(DrawCommand::DrawParagraph {
            paragraph: paragraph.clone(),
            offset,
        });
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Images
    // ═══════════════════════════════════════════════════════════════
    
    /// Draw image at offset
    pub fn draw_image(&mut self, image: &Arc<Image>, offset: Offset, paint: &Paint) {
        self.recorder.record(DrawCommand::DrawImage {
            image: image.clone(),
            offset,
            paint: paint.clone(),
        });
    }
    
    /// Draw image rect (with src/dst rects)
    pub fn draw_image_rect(
        &mut self,
        image: &Arc<Image>,
        src: Rect,
        dst: Rect,
        paint: &Paint,
    ) {
        self.recorder.record(DrawCommand::DrawImageRect {
            image: image.clone(),
            src,
            dst,
            paint: paint.clone(),
        });
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Canvas State
    // ═══════════════════════════════════════════════════════════════
    
    /// Save canvas state
    pub fn save(&mut self) {
        self.recorder.record(DrawCommand::Save);
    }
    
    /// Restore canvas state
    pub fn restore(&mut self) {
        self.recorder.record(DrawCommand::Restore);
    }
    
    /// Save state, execute closure, restore
    pub fn save_layer<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.save();
        f(self);
        self.restore();
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Transforms
    // ═══════════════════════════════════════════════════════════════
    
    /// Translate canvas
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.recorder.record(DrawCommand::Translate { dx, dy });
    }
    
    /// Rotate canvas (radians)
    pub fn rotate(&mut self, angle: f32) {
        self.recorder.record(DrawCommand::Rotate { angle });
    }
    
    /// Scale canvas
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.recorder.record(DrawCommand::Scale { sx, sy });
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Clipping
    // ═══════════════════════════════════════════════════════════════
    
    /// Clip to rectangle
    pub fn clip_rect(&mut self, rect: Rect, clip_op: ClipOp) {
        self.recorder.record(DrawCommand::ClipRect { rect, clip_op });
    }
    
    /// Clip to path
    pub fn clip_path(&mut self, path: &Path, clip_op: ClipOp) {
        self.recorder.record(DrawCommand::ClipPath {
            path: path.clone(),
            clip_op,
        });
    }
}
```

### Paint - Drawing Style

```rust
/// Paint - defines how to draw
#[derive(Debug, Clone)]
pub struct Paint {
    /// Color (RGBA)
    pub color: Color,
    
    /// Style (fill or stroke)
    pub style: PaintStyle,
    
    /// Stroke width (for stroke style)
    pub stroke_width: f32,
    
    /// Blend mode
    pub blend_mode: BlendMode,
    
    /// Shader (gradient, image pattern, etc.)
    pub shader: Option<Arc<Shader>>,
    
    /// Anti-alias
    pub anti_alias: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintStyle {
    Fill,
    Stroke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    Clear,
    Src,
    Dst,
    SrcOver,
    DstOver,
    SrcIn,
    DstIn,
    SrcOut,
    DstOut,
    SrcATop,
    DstATop,
    Xor,
    Plus,
    Modulate,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Paint {
    pub fn new() -> Self {
        Self {
            color: Color::BLACK,
            style: PaintStyle::Fill,
            stroke_width: 1.0,
            blend_mode: BlendMode::SrcOver,
            shader: None,
            anti_alias: true,
        }
    }
    
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    
    pub fn style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }
    
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }
    
    pub fn blend_mode(mut self, mode: BlendMode) -> Self {
        self.blend_mode = mode;
        self
    }
    
    pub fn shader(mut self, shader: Arc<Shader>) -> Self {
        self.shader = Some(shader);
        self
    }
}

/// Shader - for gradients, patterns, etc.
#[derive(Debug, Clone)]
pub enum Shader {
    /// Linear gradient
    LinearGradient {
        start: Offset,
        end: Offset,
        colors: Vec<Color>,
        stops: Vec<f32>,
    },

    /// Radial gradient
    RadialGradient {
        center: Offset,
        radius: f32,
        colors: Vec<Color>,
        stops: Vec<f32>,
    },

    /// Image pattern
    ImagePattern {
        image: Arc<Image>,
        tile_mode: TileMode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileMode {
    Clamp,
    Repeat,
    Mirror,
}

impl Shader {
    pub fn linear_gradient(
        start: Offset,
        end: Offset,
        colors: Vec<Color>,
        stops: Vec<f32>,
    ) -> Self {
        Self::LinearGradient { start, end, colors, stops }
    }

    pub fn radial_gradient(
        center: Offset,
        radius: f32,
        colors: Vec<Color>,
        stops: Vec<f32>,
    ) -> Self {
        Self::RadialGradient { center, radius, colors, stops }
    }
}
```

---

## 🔄 Compositing Algorithm

### Compositor

```rust
/// Compositor - combines layer tree into final image
pub struct Compositor {
    /// Rasterization cache
    raster_cache: RasterCache,
    
    /// GPU backend
    backend: Box<dyn RenderBackend>,
}

impl Compositor {
    pub fn new(backend: Box<dyn RenderBackend>) -> Self {
        Self {
            raster_cache: RasterCache::new(),
            backend,
        }
    }
    
    /// Composite layer tree to screen
    pub fn composite(&mut self, root_layer: &BoxedLayer, window_size: Size) {
        // 1. Flatten layer tree
        let mut flattened = Vec::new();
        self.flatten_layers(root_layer, Mat4::identity(), &mut flattened);
        
        // 2. Rasterize layers (with caching)
        let mut textures = Vec::new();
        for (layer, transform) in flattened {
            if let Some(picture_layer) = self.as_picture_layer(layer) {
                let texture = self.rasterize_picture(picture_layer, transform);
                textures.push((texture, transform));
            }
        }
        
        // 3. Composite on GPU
        self.backend.begin_frame(window_size);
        
        for (texture, transform) in textures {
            self.backend.draw_texture(texture, transform);
        }
        
        self.backend.end_frame();
    }
    
    /// Flatten layer tree with transforms
    fn flatten_layers(
        &self,
        layer: &BoxedLayer,
        transform: Mat4,
        output: &mut Vec<(&BoxedLayer, Mat4)>,
    ) {
        match layer.layer_type() {
            "Picture" => {
                // Leaf - add to output
                output.push((layer, transform));
            }
            
            "Offset" => {
                let offset_layer = layer.as_any()
                    .downcast_ref::<OffsetLayer>()
                    .unwrap();
                
                let child_transform = transform
                    * Mat4::translate(offset_layer.offset.x, offset_layer.offset.y, 0.0);
                
                offset_layer.visit_children(&mut |child| {
                    self.flatten_layers(child, child_transform, output);
                });
            }
            
            "Transform" => {
                let transform_layer = layer.as_any()
                    .downcast_ref::<TransformLayer>()
                    .unwrap();
                
                let child_transform = transform * transform_layer.transform;
                
                transform_layer.visit_children(&mut |child| {
                    self.flatten_layers(child, child_transform, output);
                });
            }
            
            "Opacity" => {
                let opacity_layer = layer.as_any()
                    .downcast_ref::<OpacityLayer>()
                    .unwrap();
                
                // TODO: Apply opacity in shader
                opacity_layer.visit_children(&mut |child| {
                    self.flatten_layers(child, transform, output);
                });
            }
            
            _ => {
                // Container - recurse to children
                layer.visit_children(&mut |child| {
                    self.flatten_layers(child, transform, output);
                });
            }
        }
    }
    
    /// Rasterize picture layer (with caching)
    fn rasterize_picture(
        &mut self,
        picture_layer: &PictureLayer,
        transform: Mat4,
    ) -> Arc<Texture> {
        // Get picture from layer
        let picture = &picture_layer.picture;

        // Check cache
        let cache_key = RasterCacheKey::new(picture, transform);

        if let Some(cached) = self.raster_cache.get(&cache_key) {
            return cached;  // Cache hit!
        }

        // Cache miss - rasterize
        let texture = self.backend.rasterize_picture(picture, transform);

        // Store in cache
        self.raster_cache.insert(cache_key, texture.clone());

        texture
    }

    fn as_picture_layer<'a>(&self, layer: &'a BoxedLayer) -> Option<&'a PictureLayer> {
        layer.as_any().downcast_ref::<PictureLayer>()
    }
}
```

### Raster Cache

```rust
use moka::sync::Cache;

/// Cache for rasterized pictures
pub struct RasterCache {
    cache: Cache<RasterCacheKey, Arc<Texture>>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct RasterCacheKey {
    picture_id: PictureId,
    transform: TransformKey,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct TransformKey {
    // Quantized transform for cache key
    a: i32, b: i32, c: i32, d: i32,
    tx: i32, ty: i32,
}

impl TransformKey {
    fn from_mat4(m: Mat4) -> Self {
        // Quantize to 0.01 precision
        Self {
            a: (m.a * 100.0) as i32,
            b: (m.b * 100.0) as i32,
            c: (m.c * 100.0) as i32,
            d: (m.d * 100.0) as i32,
            tx: (m.tx * 100.0) as i32,
            ty: (m.ty * 100.0) as i32,
        }
    }
}

impl RasterCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(1000)
                .time_to_live(Duration::from_secs(60))
                .build(),
        }
    }
    
    pub fn get(&self, key: &RasterCacheKey) -> Option<Arc<Texture>> {
        self.cache.get(key)
    }
    
    pub fn insert(&self, key: RasterCacheKey, texture: Arc<Texture>) {
        self.cache.insert(key, texture);
    }
}
```

---

## 🎯 Usage Example - Custom Painter

```rust
/// Custom painter widget
#[derive(Debug, Clone)]
pub struct CustomPaint<P: Painter> {
    painter: P,
    size: Size,
}

pub trait Painter: Debug + Clone + Send + Sync + 'static {
    fn paint(&self, canvas: &mut Canvas, size: Size);
}

impl<P: Painter> RenderObjectWidget for CustomPaint<P> {
    type Arity = LeafArity;
    type Render = RenderCustomPaint<P>;
    
    fn create_render_object(&self) -> Self::Render {
        RenderCustomPaint {
            painter: self.painter.clone(),
            size: self.size,
        }
    }
    
    fn update_render_object(&self, render: &mut Self::Render) {
        render.painter = self.painter.clone();
        render.size = self.size;
    }
}

#[derive(Debug)]
pub struct RenderCustomPaint<P: Painter> {
    painter: P,
    size: Size,
}

impl<P: Painter> RenderObject for RenderCustomPaint<P> {
    type Arity = LeafArity;
    
    fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
        cx.constraints().constrain(self.size)
    }
    
    fn paint(&self, cx: &PaintCx<LeafArity>) -> BoxedLayer {
        let mut recorder = PictureRecorder::new();
        let mut canvas = recorder.canvas();
        
        // Call custom painter
        self.painter.paint(&mut canvas, cx.size());
        
        let picture = recorder.finish();
        Box::new(PictureLayer::new(picture))
    }
}

// Example: Checkerboard painter
#[derive(Debug, Clone)]
struct CheckerboardPainter {
    cell_size: f32,
    color1: Color,
    color2: Color,
}

impl Painter for CheckerboardPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let cols = (size.width / self.cell_size).ceil() as usize;
        let rows = (size.height / self.cell_size).ceil() as usize;
        
        for row in 0..rows {
            for col in 0..cols {
                let color = if (row + col) % 2 == 0 {
                    self.color1
                } else {
                    self.color2
                };
                
                let rect = Rect::from_xywh(
                    col as f32 * self.cell_size,
                    row as f32 * self.cell_size,
                    self.cell_size,
                    self.cell_size,
                );
                
                canvas.draw_rect(rect, &Paint::new().color(color));
            }
        }
    }
}

// Usage example:
fn main() {
    let checkerboard = CustomPaint {
        painter: CheckerboardPainter {
            cell_size: 20.0,
            color1: Color::WHITE,
            color2: Color::from_hex("#333333"),
        },
        size: Size::new(400.0, 400.0),
    };

    run_app(checkerboard);
}
```

**More Practical Examples:**

```rust
// Gradient background painter
#[derive(Debug, Clone)]
struct GradientPainter {
    start_color: Color,
    end_color: Color,
    direction: GradientDirection,
}

#[derive(Debug, Clone, Copy)]
enum GradientDirection {
    Vertical,
    Horizontal,
    Diagonal,
}

impl Painter for GradientPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let gradient = match self.direction {
            GradientDirection::Vertical => {
                Shader::linear_gradient(
                    Offset::new(0.0, 0.0),
                    Offset::new(0.0, size.height),
                    vec![self.start_color, self.end_color],
                    vec![0.0, 1.0],
                )
            }
            GradientDirection::Horizontal => {
                Shader::linear_gradient(
                    Offset::new(0.0, 0.0),
                    Offset::new(size.width, 0.0),
                    vec![self.start_color, self.end_color],
                    vec![0.0, 1.0],
                )
            }
            GradientDirection::Diagonal => {
                Shader::linear_gradient(
                    Offset::new(0.0, 0.0),
                    Offset::new(size.width, size.height),
                    vec![self.start_color, self.end_color],
                    vec![0.0, 1.0],
                )
            }
        };

        let paint = Paint::new().shader(Arc::new(gradient));
        canvas.draw_rect(Rect::from_origin_size(Offset::ZERO, size), &paint);
    }
}
```

---

## 🔗 Cross-References

- **Previous:** [Chapter 4: Layout Engine](04_layout_engine.md)
- **Next:** [Chapter 6: Render Backend](06_render_backend.md)
- **Related:** [Chapter 3: RenderObject System](03_render_objects.md)

---

**Key Takeaway:** FLUI's layer system provides GPU-optimized compositing with rasterization caching, enabling smooth 60+ FPS rendering. The Painter API offers high-level drawing primitives while maintaining flexibility!
