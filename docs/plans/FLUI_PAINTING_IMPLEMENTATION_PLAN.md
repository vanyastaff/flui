# flui_painting Implementation Plan

**Crate:** `flui_painting`  
**Purpose:** 2D graphics primitives, text rendering, image loading, visual effects  
**Priority:** ⭐⭐⭐⭐⭐ CRITICAL (Core painting abstraction)

---

## Overview

`flui_painting` provides high-level painting abstractions on top of `flui_engine`:
- Canvas API (similar to HTML Canvas 2D / Flutter Canvas)
- Path rendering (lines, bezier curves, arcs)
- Text rendering and shaping
- Image loading and caching
- Visual effects (gradients, patterns, filters)
- Display list generation

**Architecture:**
```
flui_painting/
├── src/
│   ├── lib.rs              # Public API
│   ├── canvas.rs           # Canvas 2D API
│   ├── path.rs             # Path primitives
│   ├── paint.rs            # Paint (fill/stroke)
│   ├── text/
│   │   ├── font.rs         # Font loading
│   │   ├── shaping.rs      # Text shaping (rustybuzz/cosmic-text)
│   │   └── layout.rs       # Text layout
│   ├── image/
│   │   ├── loader.rs       # Image decoding
│   │   └── cache.rs        # Image cache
│   ├── effects/
│   │   ├── gradient.rs     # Linear/radial gradients
│   │   ├── blur.rs         # Blur filter
│   │   └── shadow.rs       # Drop shadow
│   ├── display_list.rs     # Display list builder
│   └── platforms/
│       ├── macos/
│       │   └── liquid_glass.rs  # macOS Liquid Glass materials
│       └── windows/
│           └── acrylic.rs       # Windows Acrylic materials
```

---

## Q1 2026: Foundation (Weeks 1-12)

### 1. Canvas API ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Priority:** CRITICAL

**Tasks:**

#### 1.1 Core Canvas Implementation
```rust
// src/canvas.rs

use crate::{Path, Paint, DisplayList};

pub struct Canvas {
    display_list: DisplayList,
    transform_stack: Vec<Affine2D>,
    clip_stack: Vec<Path>,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            display_list: DisplayList::new(),
            transform_stack: vec![Affine2D::identity()],
            clip_stack: vec![],
        }
    }

    // Transform operations
    pub fn save(&mut self) {
        let current_transform = self.current_transform();
        self.transform_stack.push(current_transform);
    }

    pub fn restore(&mut self) {
        self.transform_stack.pop();
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        let current = self.current_transform();
        self.transform_stack.push(current.pre_translate(dx, dy));
    }

    pub fn rotate(&mut self, angle: f32) {
        let current = self.current_transform();
        self.transform_stack.push(current.pre_rotate(angle));
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        let current = self.current_transform();
        self.transform_stack.push(current.pre_scale(sx, sy));
    }

    fn current_transform(&self) -> Affine2D {
        *self.transform_stack.last().unwrap()
    }

    // Drawing operations
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.display_list.add_rect(
            rect,
            paint.clone(),
            self.current_transform(),
        );
    }

    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        self.display_list.add_circle(
            center,
            radius,
            paint.clone(),
            self.current_transform(),
        );
    }

    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.display_list.add_path(
            path.clone(),
            paint.clone(),
            self.current_transform(),
        );
    }

    pub fn draw_text(&mut self, text: &str, position: Point, font: &Font, paint: &Paint) {
        // Shape text
        let shaped = self.shape_text(text, font);
        
        self.display_list.add_text(
            shaped,
            position,
            paint.clone(),
            self.current_transform(),
        );
    }

    pub fn draw_image(&mut self, image: &Image, rect: Rect) {
        self.display_list.add_image(
            image.id(),
            rect,
            self.current_transform(),
        );
    }

    // Clipping
    pub fn clip_rect(&mut self, rect: Rect) {
        let path = Path::from_rect(rect);
        self.clip_stack.push(path);
    }

    pub fn clip_path(&mut self, path: &Path) {
        self.clip_stack.push(path.clone());
    }

    // Finalization
    pub fn finish(self) -> DisplayList {
        self.display_list
    }
}
```

**Deliverables:**
- ✅ Canvas 2D API with transforms, clipping
- ✅ Drawing primitives (rect, circle, path, text, image)
- ✅ Display list generation

---

### 2. Path Rendering ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Priority:** CRITICAL

**Tasks:**

#### 2.1 Path Primitives
```rust
// src/path.rs

use lyon::path::Path as LyonPath;
use lyon::path::builder::PathBuilder;

pub struct Path {
    lyon_path: LyonPath,
}

impl Path {
    pub fn new() -> Self {
        Self {
            lyon_path: LyonPath::new(),
        }
    }

    pub fn from_rect(rect: Rect) -> Self {
        let mut builder = LyonPath::builder();
        builder.add_rectangle(&lyon::math::Box2D::new(
            lyon::math::Point::new(rect.x, rect.y),
            lyon::math::Point::new(rect.x + rect.width, rect.y + rect.height),
        ), lyon::path::Winding::Positive);
        
        Self {
            lyon_path: builder.build(),
        }
    }

    pub fn from_circle(center: Point, radius: f32) -> Self {
        let mut builder = LyonPath::builder();
        builder.add_circle(
            lyon::math::Point::new(center.x, center.y),
            radius,
            lyon::path::Winding::Positive,
        );
        
        Self {
            lyon_path: builder.build(),
        }
    }

    // Path building
    pub fn move_to(&mut self, point: Point) {
        // Use PathBuilder to add move_to command
    }

    pub fn line_to(&mut self, point: Point) {
        // Add line segment
    }

    pub fn quad_to(&mut self, control: Point, to: Point) {
        // Quadratic bezier curve
    }

    pub fn cubic_to(&mut self, control1: Point, control2: Point, to: Point) {
        // Cubic bezier curve
    }

    pub fn arc_to(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32) {
        // Elliptical arc
    }

    pub fn close(&mut self) {
        // Close current subpath
    }

    // Tessellation for GPU rendering
    pub fn tessellate(&self, tolerance: f32) -> Tessellation {
        use lyon::tessellation::{FillTessellator, FillOptions};

        let mut tessellator = FillTessellator::new();
        let mut geometry = VertexBuffers::new();

        tessellator.tessellate_path(
            &self.lyon_path,
            &FillOptions::default().with_tolerance(tolerance),
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                Vertex {
                    position: [vertex.position().x, vertex.position().y],
                }
            }),
        ).unwrap();

        Tessellation { geometry }
    }
}

pub struct Tessellation {
    pub geometry: VertexBuffers<Vertex, u32>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
}
```

**Deliverables:**
- ✅ Path API with lines, curves, arcs
- ✅ Lyon tessellation integration
- ✅ GPU-ready vertex buffers

---

### 3. Text Rendering ⭐⭐⭐⭐⭐
- **Effort:** 4 weeks
- **Priority:** CRITICAL

**Tasks:**

#### 3.1 Font Loading
```rust
// src/text/font.rs

use ab_glyph::{FontArc, GlyphId, ScaleFont};

pub struct Font {
    font_arc: FontArc,
    size: f32,
}

impl Font {
    pub fn from_bytes(data: &[u8], size: f32) -> Result<Self> {
        let font_arc = FontArc::try_from_vec(data.to_vec())?;
        Ok(Self { font_arc, size })
    }

    pub fn from_file(path: &Path, size: f32) -> Result<Self> {
        let data = std::fs::read(path)?;
        Self::from_bytes(&data, size)
    }

    pub fn glyph_id(&self, ch: char) -> GlyphId {
        self.font_arc.glyph_id(ch)
    }

    pub fn scale(&self) -> f32 {
        self.size
    }

    pub fn scaled_font(&self) -> ScaleFont<FontArc> {
        self.font_arc.as_scaled(self.size)
    }
}
```

#### 3.2 Text Shaping (Option A: rustybuzz)
```rust
// src/text/shaping.rs

use rustybuzz::{Face, UnicodeBuffer, shape};

pub struct TextShaper {
    face: Face<'static>,
}

impl TextShaper {
    pub fn new(font_data: &[u8]) -> Result<Self> {
        let face = Face::from_slice(font_data, 0)
            .ok_or_else(|| anyhow::anyhow!("Failed to load font"))?;
        Ok(Self { face })
    }

    pub fn shape(&self, text: &str, font_size: f32) -> ShapedText {
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);

        let output = shape(&self.face, &[], buffer);

        let glyphs: Vec<ShapedGlyph> = output.glyph_infos()
            .iter()
            .zip(output.glyph_positions())
            .map(|(info, pos)| ShapedGlyph {
                glyph_id: info.glyph_id,
                cluster: info.cluster,
                x_advance: pos.x_advance as f32 * font_size / 1000.0,
                y_advance: pos.y_advance as f32 * font_size / 1000.0,
                x_offset: pos.x_offset as f32 * font_size / 1000.0,
                y_offset: pos.y_offset as f32 * font_size / 1000.0,
            })
            .collect();

        ShapedText { glyphs }
    }
}

pub struct ShapedText {
    pub glyphs: Vec<ShapedGlyph>,
}

pub struct ShapedGlyph {
    pub glyph_id: u32,
    pub cluster: u32,
    pub x_advance: f32,
    pub y_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}
```

#### 3.3 Text Shaping (Option B: cosmic-text) ⭐⭐⭐⭐⭐
**RECOMMENDED for Linux compatibility (used by COSMIC Desktop)**

```rust
// src/text/cosmic_shaping.rs

use cosmic_text::{Buffer, FontSystem, Metrics, Shaping, SwashCache};

pub struct CosmicTextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl CosmicTextRenderer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    pub fn shape_text(&mut self, text: &str, font_size: f32) -> ShapedText {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(&mut self.font_system, text, Shaping::Advanced);

        // Layout text
        buffer.shape_until_scroll(&mut self.font_system);

        // Extract glyph positions
        let mut glyphs = Vec::new();
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                glyphs.push(ShapedGlyph {
                    glyph_id: glyph.cache_key.glyph_id,
                    x: glyph.x,
                    y: glyph.y,
                    width: glyph.w,
                });
            }
        }

        ShapedText { glyphs }
    }

    pub fn rasterize_glyph(
        &mut self,
        glyph_id: u16,
        font_size: f32,
    ) -> Option<Vec<u8>> {
        let image = self.swash_cache.get_image(
            &mut self.font_system,
            cache_key,
        )?;

        Some(image.data.to_vec())
    }
}
```

**Deliverables:**
- ✅ Font loading (TrueType, OpenType)
- ✅ Text shaping (rustybuzz OR cosmic-text)
- ✅ Glyph rasterization
- ✅ Text layout (line breaking, word wrapping)

---

### 4. Image Loading & Caching ⭐⭐⭐⭐
- **Effort:** 2 weeks

**Tasks:**

#### 4.1 Image Decoder
```rust
// src/image/loader.rs

use image::{DynamicImage, ImageFormat};

pub struct ImageLoader;

impl ImageLoader {
    pub fn load_from_bytes(data: &[u8]) -> Result<Image> {
        let dynamic_image = image::load_from_memory(data)?;
        Self::from_dynamic_image(dynamic_image)
    }

    pub fn load_from_file(path: &Path) -> Result<Image> {
        let dynamic_image = image::open(path)?;
        Self::from_dynamic_image(dynamic_image)
    }

    fn from_dynamic_image(dynamic_image: DynamicImage) -> Result<Image> {
        let rgba = dynamic_image.to_rgba8();
        let (width, height) = rgba.dimensions();

        Ok(Image {
            width,
            height,
            data: rgba.into_raw(),
            format: ImageFormat::Rgba8,
        })
    }

    pub fn decode_png(data: &[u8]) -> Result<Image> {
        let decoder = image::codecs::png::PngDecoder::new(data)?;
        // ...
    }

    pub fn decode_jpeg(data: &[u8]) -> Result<Image> {
        let decoder = image::codecs::jpeg::JpegDecoder::new(data)?;
        // ...
    }
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub format: ImageFormat,
}

pub enum ImageFormat {
    Rgba8,
    Rgb8,
    Gray8,
}
```

#### 4.2 Image Cache
```rust
// src/image/cache.rs

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

pub struct ImageCache {
    cache: Arc<RwLock<HashMap<ImageId, CachedImage>>>,
    max_memory: usize,
    current_memory: usize,
}

impl ImageCache {
    pub fn new(max_memory_mb: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            max_memory: max_memory_mb * 1024 * 1024,
            current_memory: 0,
        }
    }

    pub fn insert(&mut self, id: ImageId, image: Image) {
        let memory_size = image.width * image.height * 4;  // RGBA
        
        // Evict if over limit
        while self.current_memory + memory_size as usize > self.max_memory {
            self.evict_lru();
        }

        let cached = CachedImage {
            image,
            last_used: std::time::Instant::now(),
        };

        self.cache.write().insert(id, cached);
        self.current_memory += memory_size as usize;
    }

    pub fn get(&self, id: &ImageId) -> Option<Image> {
        let mut cache = self.cache.write();
        cache.get_mut(id).map(|cached| {
            cached.last_used = std::time::Instant::now();
            cached.image.clone()
        })
    }

    fn evict_lru(&mut self) {
        // Find least recently used image
        let mut cache = self.cache.write();
        if let Some((id, _)) = cache.iter()
            .min_by_key(|(_, cached)| cached.last_used)
            .map(|(id, cached)| (*id, cached.clone()))
        {
            cache.remove(&id);
        }
    }
}

struct CachedImage {
    image: Image,
    last_used: std::time::Instant,
}

pub type ImageId = u64;
```

**Deliverables:**
- ✅ Image decoding (PNG, JPEG, WebP)
- ✅ LRU image cache
- ✅ GPU texture upload integration

---

## Q2 2026: Visual Effects (Weeks 13-24)

### 5. Gradients ⭐⭐⭐⭐
- **Effort:** 2 weeks

**Tasks:**

#### 5.1 Linear Gradient
```rust
// src/effects/gradient.rs

pub struct LinearGradient {
    pub start: Point,
    pub end: Point,
    pub stops: Vec<ColorStop>,
}

pub struct ColorStop {
    pub offset: f32,  // 0.0 - 1.0
    pub color: Color,
}

impl LinearGradient {
    pub fn new(start: Point, end: Point) -> Self {
        Self {
            start,
            end,
            stops: vec![],
        }
    }

    pub fn add_stop(&mut self, offset: f32, color: Color) {
        self.stops.push(ColorStop { offset, color });
        self.stops.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap());
    }

    /// Generate GPU shader code for this gradient
    pub fn to_shader(&self) -> String {
        // Generate WGSL fragment shader for linear gradient
        format!(r#"
            @fragment
            fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{
                let t = dot(uv - vec2<f32>({}, {}), normalize(vec2<f32>({}, {}) - vec2<f32>({}, {})));
                // Interpolate between color stops based on t
                // ...
                return color;
            }}
        "#, self.start.x, self.start.y, self.end.x, self.end.y, self.start.x, self.start.y)
    }
}
```

#### 5.2 Radial Gradient
```rust
// src/effects/radial_gradient.rs

pub struct RadialGradient {
    pub center: Point,
    pub radius: f32,
    pub stops: Vec<ColorStop>,
}

impl RadialGradient {
    pub fn to_shader(&self) -> String {
        // Generate WGSL shader for radial gradient
        // Distance from center determines color
        todo!()
    }
}
```

**Deliverables:**
- ✅ Linear gradients
- ✅ Radial gradients
- ✅ Conic gradients
- ✅ GPU shader generation

---

### 6. Platform-Specific Materials ⭐⭐⭐⭐

#### 6.1 macOS Liquid Glass ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Platform:** macOS Tahoe 26+

```rust
// src/platforms/macos/liquid_glass.rs

#[cfg(target_os = "macos")]
pub enum LiquidGlassMaterial {
    Standard,        // Default translucent glass
    Prominent,       // More opaque, emphasized
    Sidebar,         // Optimized for sidebars
    Menu,            // Optimized for menus
    Popover,         // Optimized for popovers
    ControlCenter,   // Optimized for Control Center style
}

#[cfg(target_os = "macos")]
pub struct LiquidGlassEffect {
    material: LiquidGlassMaterial,
    blur_radius: f32,
    tint: Color,
}

impl LiquidGlassEffect {
    pub fn new(material: LiquidGlassMaterial) -> Self {
        Self {
            material,
            blur_radius: Self::default_blur_radius(&material),
            tint: Self::default_tint(&material),
        }
    }

    fn default_blur_radius(material: &LiquidGlassMaterial) -> f32 {
        match material {
            LiquidGlassMaterial::Standard => 30.0,
            LiquidGlassMaterial::Prominent => 20.0,
            LiquidGlassMaterial::Sidebar => 40.0,
            LiquidGlassMaterial::Menu => 25.0,
            LiquidGlassMaterial::Popover => 30.0,
            LiquidGlassMaterial::ControlCenter => 35.0,
        }
    }

    fn default_tint(material: &LiquidGlassMaterial) -> Color {
        match material {
            LiquidGlassMaterial::Standard => Color::from_rgba(255, 255, 255, 0.3),
            LiquidGlassMaterial::Prominent => Color::from_rgba(255, 255, 255, 0.5),
            // ...
        }
    }

    pub fn apply(&self, canvas: &mut Canvas, rect: Rect) {
        // 1. Capture background into texture
        // 2. Apply blur with GPU compute shader
        // 3. Apply tint
        // 4. Composite back
    }
}
```

#### 6.2 Windows Acrylic/Mica ⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Platform:** Windows 11

```rust
// src/platforms/windows/acrylic.rs

#[cfg(target_os = "windows")]
pub enum WindowsMaterial {
    Acrylic,  // Windows 10/11
    Mica,     // Windows 11
    MicaAlt,  // Windows 11 22H2+
}

#[cfg(target_os = "windows")]
pub struct AcrylicEffect {
    material: WindowsMaterial,
    tint_opacity: f32,
    luminosity_opacity: f32,
}

impl AcrylicEffect {
    pub fn new(material: WindowsMaterial) -> Self {
        Self {
            material,
            tint_opacity: 0.9,
            luminosity_opacity: 0.85,
        }
    }

    pub fn apply(&self, canvas: &mut Canvas, rect: Rect) {
        // 1. Sample desktop wallpaper (Acrylic) or app background (Mica)
        // 2. Apply noise texture
        // 3. Apply tint with luminosity
        // 4. Apply blur (30px for Acrylic, subtle for Mica)
    }
}
```

**Deliverables:**
- ✅ macOS Liquid Glass materials (6 variants)
- ✅ Windows Acrylic/Mica materials
- ✅ Platform-specific blur optimizations

---

## Q3 2026: Advanced Effects (Weeks 25-36)

### 7. Filters & Effects ⭐⭐⭐⭐
- **Effort:** 3 weeks

**Tasks:**

#### 7.1 Image Filters
```rust
// src/effects/filters.rs

pub enum ImageFilter {
    Blur { radius: f32 },
    Brightness { amount: f32 },  // -1.0 to 1.0
    Contrast { amount: f32 },    // -1.0 to 1.0
    Saturation { amount: f32 },  // 0.0 to 2.0
    HueRotate { degrees: f32 },
    Invert,
    Grayscale,
    Sepia,
}

impl ImageFilter {
    pub fn apply(&self, canvas: &mut Canvas, texture: &Texture) -> Texture {
        match self {
            ImageFilter::Blur { radius } => {
                // Use GPU gaussian blur from flui_engine
            }
            ImageFilter::Brightness { amount } => {
                // Fragment shader: color.rgb += amount
            }
            // ...
        }
    }

    pub fn to_shader(&self) -> String {
        match self {
            ImageFilter::Grayscale => r#"
                @fragment
                fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
                    let color = textureSample(inputTexture, inputSampler, uv);
                    let gray = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
                    return vec4<f32>(gray, gray, gray, color.a);
                }
            "#.to_string(),
            // Generate shaders for other filters
            _ => todo!(),
        }
    }
}
```

**Deliverables:**
- ✅ Image filters (brightness, contrast, saturation, hue, grayscale, sepia)
- ✅ Blur filter (gaussian)
- ✅ Drop shadow filter
- ✅ Filter composition (chain multiple filters)

---

### 8. Display List Optimization ⭐⭐⭐⭐
- **Effort:** 2 weeks

**Tasks:**

```rust
// src/display_list.rs

pub struct DisplayList {
    commands: Vec<DrawCommand>,
}

pub enum DrawCommand {
    SaveLayer {
        bounds: Rect,
        paint: Paint,
    },
    RestoreLayer,
    DrawRect {
        rect: Rect,
        paint: Paint,
    },
    DrawPath {
        path: Path,
        paint: Paint,
    },
    DrawText {
        glyphs: Vec<PositionedGlyph>,
        paint: Paint,
    },
    DrawImage {
        image_id: ImageId,
        src_rect: Rect,
        dst_rect: Rect,
    },
    SetTransform {
        transform: Affine2D,
    },
    ClipRect {
        rect: Rect,
    },
    ClipPath {
        path: Path,
    },
}

impl DisplayList {
    /// Optimize display list (merge adjacent rects, cull offscreen, etc.)
    pub fn optimize(&mut self) {
        self.merge_adjacent_rects();
        self.cull_offscreen_commands();
        self.batch_by_material();
    }

    fn merge_adjacent_rects(&mut self) {
        // Merge consecutive DrawRect with same paint
    }

    fn cull_offscreen_commands(&mut self) {
        // Remove commands outside clip bounds
    }

    fn batch_by_material(&mut self) {
        // Group commands by paint/shader for GPU batching
    }
}
```

**Deliverables:**
- ✅ Display list optimization
- ✅ Command merging
- ✅ Offscreen culling
- ✅ Material batching

---

## Testing Strategy

```rust
// tests/canvas_tests.rs

#[test]
fn test_canvas_rect() {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::new(10.0, 10.0, 100.0, 100.0),
        &Paint::fill(Color::RED),
    );
    
    let display_list = canvas.finish();
    assert_eq!(display_list.commands.len(), 1);
}

#[test]
fn test_text_shaping() {
    let mut shaper = TextShaper::new(FONT_DATA).unwrap();
    let shaped = shaper.shape("Hello, world!", 16.0);
    assert_eq!(shaped.glyphs.len(), 13);  // Including space and comma
}

#[test]
fn test_image_cache_eviction() {
    let mut cache = ImageCache::new(1);  // 1MB limit
    
    // Add 2MB of images
    cache.insert(ImageId(1), create_dummy_image(512, 512));  // ~1MB
    cache.insert(ImageId(2), create_dummy_image(512, 512));  // ~1MB
    
    // First image should be evicted
    assert!(cache.get(&ImageId(1)).is_none());
    assert!(cache.get(&ImageId(2)).is_some());
}
```

---

## Dependencies

```toml
[dependencies]
lyon = "1.0"  # Path tessellation
ab_glyph = "0.2"  # Font loading
rustybuzz = "0.17"  # Text shaping (option A)
cosmic-text = "0.12"  # Text shaping (option B, RECOMMENDED)
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
parking_lot = "0.12"

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-app-kit = "0.2"

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = ["UI_Composition"] }
```

---

## Success Metrics

- ✅ Text rendering at 60 FPS (100+ glyphs)
- ✅ Image cache hit rate > 90%
- ✅ Display list optimization reduces commands by 30%+
- ✅ Gradient rendering with zero CPU overhead (GPU shaders)
- ✅ Liquid Glass material on macOS looks identical to native
- ✅ Blur performance < 5ms @ 1080p

---

**Next Steps:**
1. Implement Canvas API
2. Integrate Lyon for path tessellation
3. Choose text shaping library (cosmic-text recommended for Linux)
4. Implement image loading and caching
5. Add platform-specific materials (Liquid Glass, Acrylic)
