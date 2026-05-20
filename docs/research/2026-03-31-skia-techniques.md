# Skia Rendering Optimizations & Impeller Architecture

**Date:** 2026-03-31
**Scope:** Research Skia's key rendering optimizations applicable to wgpu-based rendering, and Flutter's Impeller renderer

---

## 1. Skia Optimizations

### 1.1 GPU Path Caching

**How Skia does it:**

Skia caches tessellated path geometry on the GPU, keyed by a hash of the path data. When the same path is drawn again (even with different transforms or colors), Skia reuses the cached tessellated geometry rather than re-tessellating from scratch.

In the legacy Ganesh backend, this works through `GrStyledShape` which computes a shape key. The key includes the path data hash, fill type, and stroke parameters. Cached vertex buffers are stored in a GPU resource cache (`GrResourceCache`) with LRU eviction.

In the newer Graphite backend, Skia takes this further: **simple shapes (rects, rrects, circles) bypass tessellation entirely** and are rendered as quads with specialized fragment shaders. The shader computes the shape analytically, using the shape's metadata. Only complex paths require actual tessellation.

**Applicability to FLUI:**

FLUI uses lyon for CPU-side tessellation, producing `VertexBuffers` that are uploaded to the GPU. FLUI could implement path caching at two levels:

1. **CPU tessellation cache:** Hash the path data + stroke parameters, cache the resulting `VertexBuffers`. On cache hit, skip lyon tessellation entirely and reuse the vertex/index buffers. This is straightforward to implement.

2. **GPU vertex buffer cache:** Keep tessellated geometry in GPU buffers (`wgpu::Buffer`) across frames. On cache hit, skip both tessellation AND buffer upload. Only update the uniform (transform, color).

**Estimated implementation:**
```rust
struct PathCacheKey {
    path_hash: u64,        // Hash of path commands + coordinates
    fill_type: FillType,
    stroke_width: OrderedFloat<f32>,
    stroke_cap: StrokeCap,
    stroke_join: StrokeJoin,
}

struct CachedPath {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    last_used_frame: u64,
}

struct PathCache {
    entries: HashMap<PathCacheKey, CachedPath>,
    max_entries: usize,
    current_frame: u64,
}
```

**Impact:** HIGH for scenes with repeated path shapes (icons, UI decorations, charts). Eliminates the primary CPU bottleneck in complex scenes.

### 1.2 Distance Field Text Rendering

**How Skia does it:**

Skia uses Signed Distance Field (SDF) text rendering for medium-sized text (roughly 24-256px). The technique works as follows:

1. **Generation:** For each glyph, Skia computes an SDF texture -- a grayscale image where each pixel stores the distance to the nearest glyph edge. Inside the glyph is positive, outside is negative.
2. **Rendering:** The SDF texture is sampled in a fragment shader. The alpha value is computed using `smoothstep` or a similar function on the distance value.
3. **Resolution Independence:** Because the distance field is smooth, SDF glyphs look crisp at any scale without re-rasterization. A single SDF texture can serve text from ~50% to ~200% of its native size.

Skia uses an **analytical method** that computes the SDF directly from the font's vector outlines (Bezier curves), rather than first rasterizing to a bitmap and then computing distances. This produces higher-quality SDFs and is ~70% faster than bitmap-based methods.

For very small text, Skia falls back to bitmap glyph atlases (more accurate). For very large text, it renders paths directly.

**Comparison with FLUI's current approach:**

FLUI currently uses glyphon/cosmic-text with a glyph atlas approach:
- Glyphs are rasterized at specific sizes into atlas textures
- When text size changes significantly, glyphs must be re-rasterized
- Atlas management (packing, eviction) adds complexity

SDF advantages over atlas:
- Single texture serves multiple sizes (fewer atlas rebuilds)
- Better GPU memory efficiency (one SDF per glyph vs multiple rasterized sizes)
- Smooth scaling and rotation without artifacts

SDF disadvantages:
- Less crisp for very small text (< 16px)
- Slightly more GPU fragment shader work
- Multi-channel SDF (MSDF) needed for sharp corners (adds complexity)

**Recommendation for FLUI:**

Consider a **hybrid approach**:
- Keep glyph atlas for small text (< 20px) where bitmap accuracy matters
- Use SDF for medium text (20-200px) where scaling flexibility matters
- Use path rendering for very large text (> 200px)

This matches Skia's tiered strategy and would significantly reduce atlas pressure for typical UI text sizes.

### 1.3 Pipeline State Sorting

**How Skia does it:**

Skia's GPU backends organize draw operations to minimize GPU state switches, which are one of the most expensive GPU operations.

**Ganesh backend (GrOp system):**
- Drawing commands are converted to `GrOp` objects
- Ops are **merged** when possible (e.g., two adjacent rect fills with the same shader become one batched draw)
- Ops are **reordered** to group by pipeline state (shader program, blend mode, texture bindings)
- The reordering respects draw order constraints (overlapping primitives must maintain order)

**Graphite backend (SortKey system):**
- Each draw is assigned a `SortKey` with three components: **depth** (painter's order), **pipeline** (shader + state), **geometry** (vertex data)
- `DrawPass` groups compatible draws for efficient GPU submission
- Pipeline objects are cached by `GraphicsPipelineDesc` hash
- Opaque draws use **depth testing** with z-values, allowing out-of-order rendering that automatically eliminates overdraw

**Applicability to FLUI:**

FLUI's `DrawSegment` batching in `flui-engine` could adopt this approach:

1. **Sort by pipeline state:** Group DrawSegments by (shader, blend_mode, texture_bindings) before submitting to wgpu. This minimizes `set_pipeline` and `set_bind_group` calls.

2. **Merge adjacent draws:** When consecutive DrawSegments use the same pipeline state, merge their vertex/index buffers into a single draw call.

3. **Depth-based overdraw elimination:** For opaque primitives, assign z-values and enable depth testing. Opaque foreground objects automatically prevent unnecessary fragment shader execution on occluded background pixels.

```rust
struct DrawSortKey {
    // Primary: group by pipeline to minimize state switches
    pipeline_id: u32,
    // Secondary: group by texture to minimize bind group switches
    texture_id: u32,
    // Tertiary: maintain painter's order within same pipeline+texture
    depth: u32,
}
```

**Impact:** MEDIUM-HIGH. State switches are a significant source of GPU overhead, especially on mobile GPUs. Sorting by pipeline state can reduce draw call overhead by 30-50% in complex scenes.

### 1.4 Image Tiling

**How Skia does it:**

When an image is too large to upload as a single GPU texture (exceeding `maxTextureSize`), Skia automatically tiles it:

1. The image is split into tiles that fit within GPU texture limits
2. Each tile is uploaded as a separate texture
3. Tiles are drawn with appropriate texture coordinates
4. The tiling is transparent to the caller

Skia provides dedicated entry points (`SkCanvas::drawImageRect` variants) that break up large `SkBitmap`-backed images into tiles only when needed, falling through to single-texture rendering when tiling is not necessary.

**Applicability to FLUI:**

FLUI's `TextureAtlas` / `texture_cache` could implement similar auto-tiling:

```rust
fn upload_image(&mut self, image: &Image, device: &wgpu::Device, queue: &wgpu::Queue) -> TextureHandle {
    let max_size = device.limits().max_texture_dimension_2d;
    if image.width() <= max_size && image.height() <= max_size {
        // Single texture upload
        self.upload_single(image, device, queue)
    } else {
        // Tile the image
        self.upload_tiled(image, max_size, device, queue)
    }
}
```

This is most relevant for:
- User-provided images that may exceed GPU limits
- High-DPI screenshots or large background images
- Canvas-style drawing surfaces

**Impact:** LOW for typical UI rendering (most UI images are small), but important for robustness. Without tiling, large images simply fail to render.

### 1.5 Overdraw Visualization

**How Skia does it:**

Skia's debugger provides a "Display Overdraw Viz" mode that renders each pixel with a color indicating how many times it was drawn:

- 1x drawn: blue
- 2x drawn: green
- 3x drawn: yellow
- 4x drawn: orange
- 5x+ drawn: red

This is implemented by replacing the normal blend mode with an additive mode that increments a counter for each fragment, then mapping the counter to a heat-map color.

Flutter also exposes this as `debugOverdraw` through DevTools.

**Applicability to FLUI:**

FLUI could implement overdraw visualization as a debug tool in `flui-engine`:

1. **Stencil-based approach:** Use the stencil buffer to count draw operations per pixel, then render a full-screen quad that maps stencil values to heat-map colors.

2. **Additive blend approach:** Render all primitives with additive blending into a separate render target, where each primitive adds a fixed small value. Then map the accumulated values to a heat-map.

3. **Fragment shader approach:** Add a debug uniform to all shaders. When enabled, output a fixed alpha value instead of the real color. Post-process to generate the heat map.

```rust
// Debug mode in the renderer
pub enum DebugVisualization {
    None,
    Overdraw,       // Heat-map of overdraw
    WireFrame,      // Show tessellated triangles
    ClipRegions,    // Highlight clip boundaries
    LayerBounds,    // Show layer boundaries
    DirtyRegions,   // Show regions being repainted
}
```

**Impact:** LOW for production performance, but HIGH for development and optimization. Identifying overdraw hotspots is essential for rendering optimization.

---

## 2. Impeller (Flutter's New Renderer)

### 2.1 Key Architectural Decisions vs Skia

| Aspect | Skia | Impeller |
|--------|------|---------|
| Shader compilation | JIT (runtime) | AOT (build time) |
| Shader count | Hundreds, generated dynamically | < 50, manually authored |
| Rendering model | Immediate-mode rasterizer | Retain-mode, tile-based |
| GPU API | OpenGL, Vulkan, Metal, Dawn | Metal (iOS), Vulkan (Android) |
| Path rendering | Analytic AA | Stencil-then-cover tessellation |
| Caching | Implicit (GrResourceCache) | Explicit, engine-controlled |

### 2.2 AOT Shader Compilation

Impeller's most impactful decision is **ahead-of-time shader compilation**:

- All shaders are manually written (not generated from paint state)
- Shaders are compiled during Flutter engine build, not at app runtime
- Pipeline State Objects (PSOs) are built at startup from pre-compiled shaders
- **Result:** Zero shader compilation jank during animations

Skia generates shaders dynamically based on paint state combinations (color, gradient type, blend mode, clip mode, etc.), leading to hundreds of shader variants that may need compilation during the first frame they are used.

**Key insight:** Impeller's bounded set of shaders (< 50) is possible because rendering intent is **parameterized** through uniforms rather than baked into shader code. A single gradient shader handles all gradient types through uniforms, rather than generating specialized shaders per gradient type.

### 2.3 Tessellation-Based Rendering

Impeller uses **stencil-then-cover** tessellation for path rendering:

1. **Stencil pass:** Tessellate the path into triangles, render them to the stencil buffer with alternating winding
2. **Cover pass:** Draw a bounding rectangle that reads the stencil buffer to determine coverage

This approach:
- Is often faster on mobile GPUs than Skia's analytic anti-aliasing
- Produces consistent quality regardless of path complexity
- Works well with Metal and Vulkan's tile-based deferred rendering architectures

Impeller also performs aggressive **culling based on clips and texture sizes** by tracking a stencil coverage stack, enabling earlier rejection of invisible geometry.

### 2.4 Entity-Based Scene Model

Impeller uses an `Entity` as its primary rendering unit:

```
Entity {
    contents: Contents,           // What to draw
    transform: Matrix,            // Where to draw it
    blend_mode: BlendMode,        // How to blend
    stencil_clip_depth: u32,      // Clip stack depth
}
```

`EntityPass` collects entities and manages render targets:
- Render target textures are cached by `RenderTargetCache`
- Render targets are allocated from a pool and reused across frames
- Stencil coverage tracking enables early culling

### 2.5 What FLUI Already Does Right (Aligned with Impeller's Philosophy)

1. **Explicit caching:** FLUI's `ImageCache`, `TextureCache`, and `BufferPool` are explicitly managed, matching Impeller's principle of engine-controlled caching rather than implicit caches.

2. **Scene graph model:** FLUI's `Scene` with `Layer` and `Primitive` is structurally similar to Impeller's `EntityPass` with `Entity`. Both use a retained-mode scene graph rather than immediate-mode drawing.

3. **On-demand rendering:** FLUI's `ControlFlow::Wait` and dirty-flag system matches Impeller's philosophy of not rendering frames unnecessarily.

4. **Lyon tessellation:** FLUI's use of lyon for path tessellation aligns with Impeller's tessellation-based approach (vs Skia's analytic approach). Tessellation is generally more predictable in performance.

5. **Layer compositing:** FLUI's layer system with transforms, opacity, and blend modes maps well to Impeller's entity model.

6. **wgpu abstraction:** Using wgpu provides access to Vulkan, Metal, and DX12 -- the same modern GPU APIs that Impeller targets.

---

## 3. Concrete Recommendations for FLUI

### Priority 1: GPU Path Caching (HIGH IMPACT)

**What:** Cache tessellated lyon output in GPU buffers, keyed by path hash + style.

**Why:** Eliminates redundant CPU tessellation and GPU buffer uploads for repeated shapes. Most UI widgets redraw the same shapes (rounded rects, icons) across frames.

**Estimated impact:** 30-60% reduction in CPU frame time for static/mostly-static UIs.

**Implementation cost:** Low-Medium. Requires:
- Path hashing function
- LRU cache with frame-based eviction
- Integration with the renderer's draw path

### Priority 2: Pipeline State Sorting (HIGH IMPACT)

**What:** Sort draw calls by (pipeline, texture_bindings, blend_mode) before GPU submission.

**Why:** Minimizes GPU state switches, which are among the most expensive GPU operations. FLUI currently submits draws in painter's order, which may alternate between different pipeline states unnecessarily.

**Estimated impact:** 20-40% reduction in GPU overhead for complex scenes with mixed primitive types.

**Implementation cost:** Low. Requires:
- Sort key generation per DrawSegment
- Stable sort preserving order within same pipeline
- Potential depth buffer usage for opaque primitives

### Priority 3: Overdraw Visualization Debug Tool (MEDIUM IMPACT)

**What:** Implement a debug rendering mode that visualizes overdraw as a heat map.

**Why:** Essential for identifying performance bottlenecks. Without visibility into overdraw, optimization is guesswork.

**Estimated impact:** No runtime performance impact (debug-only), but enables identification of 10-30% GPU waste from overdraw in typical UI scenes.

**Implementation cost:** Low. Stencil-buffer based approach is straightforward with wgpu.

### Priority 4: AOT-Style Shader Management (MEDIUM IMPACT)

**What:** Pre-compile all shader variants at engine initialization, not on first use.

**Why:** Prevents shader compilation jank during animations. Following Impeller's lead, FLUI should have a bounded, known set of shaders.

**Estimated impact:** Eliminates first-frame jank for new primitive types. Reduces hitching during UI transitions.

**Implementation cost:** Low. FLUI already has a bounded shader set. The change is to compile all pipeline variants eagerly at startup rather than lazily.

### Priority 5: SDF Text for Medium Sizes (MEDIUM IMPACT)

**What:** Add SDF-based text rendering as an option alongside the current glyph atlas.

**Why:** Reduces atlas pressure, enables smooth text scaling, and reduces memory usage for multi-size text.

**Estimated impact:** 20-40% reduction in text-related GPU memory. Smoother text animations.

**Implementation cost:** Medium-High. Requires:
- SDF glyph generation (could use `msdfgen` or similar)
- SDF-specific fragment shader
- Integration with cosmic-text for layout
- Fallback logic for small/large text sizes

### Priority 6: Depth-Based Overdraw Elimination (MEDIUM IMPACT)

**What:** Use depth buffer for opaque primitives (solid rects, opaque images) to automatically skip fragment shader execution for occluded pixels.

**Why:** Skia Graphite's key innovation. Opaque UI elements (backgrounds, cards, containers) occlude everything behind them. Depth testing eliminates this wasted work without changing draw order.

**Estimated impact:** 10-30% reduction in fragment shader work for typical layered UIs.

**Implementation cost:** Medium. Requires:
- Z-value assignment per primitive (based on painter's order)
- Depth buffer configuration in wgpu render pass
- Separate opaque and transparent render passes

### Summary Table

| Priority | Technique | Source | Effort | Performance Impact |
|----------|-----------|--------|--------|--------------------|
| 1 | GPU Path Caching | Skia | Low-Med | 30-60% CPU reduction |
| 2 | Pipeline State Sorting | Skia/Graphite | Low | 20-40% GPU reduction |
| 3 | Overdraw Visualization | Skia/Flutter | Low | Enables optimization |
| 4 | AOT Shader Management | Impeller | Low | Eliminates jank |
| 5 | SDF Text Rendering | Skia | Med-High | 20-40% text memory |
| 6 | Depth-Based Occlusion | Graphite | Medium | 10-30% fragment reduction |

---

## Sources

- [Skia Graphite: Next-Generation GPU Backend (DeepWiki)](https://deepwiki.com/google/skia/4-graphite:-next-generation-gpu-backend)
- [Skia Core Graphics Primitives (DeepWiki)](https://deepwiki.com/google/skia/3-graphite:-skia's-next-generation-rendering-engine)
- [Skia Documentation](https://skia.org/docs/)
- [Skia Debugger](https://skia.org/docs/dev/tools/debugger/)
- [Introducing Skia Graphite: Chrome's Rasterization Backend (Chromium Blog)](https://blog.chromium.org/2025/07/introducing-skia-graphite-chromes.html)
- [Impeller Rendering Engine (Flutter Docs)](https://docs.flutter.dev/perf/impeller)
- [Impeller FAQ (flutter/engine)](https://github.com/flutter/engine/blob/main/impeller/docs/faq.md)
- [Understanding Impeller: A Deep-Dive into Flutter's Rendering Engine](https://tomicriedel.com/blog/posts/understanding-impeller-a-deep-dive-into-flutters-rendering-engine)
- [Impeller vs Skia: How Flutter's New Renderer Changes Everything](https://medium.com/@ayaanhaider.dev/impeller-vs-skia-how-flutters-new-renderer-changes-everything-189ee5102bef)
- [How Impeller Is Transforming Flutter UI Rendering in 2026](https://dev.to/eira-wexford/how-impeller-is-transforming-flutter-ui-rendering-in-2026-3dpd)
- [Skia vs Impeller: The Battle for 120 FPS](https://medium.com/@serikbay.a04/skia-vs-impeller-the-battle-for-120-fps-58cc23418c1d)
- [MSDF Font Generator (Chlumsky/msdfgen)](https://github.com/Chlumsky/msdfgen)
- [Practical Analytic 2D Signed Distance Field Generation (SIGGRAPH)](https://history.siggraph.org/learning/practical-analytic-2d-signed-distance-field-generation-by-abbas-doran-evans-and-mendez/)
- [Android GPU Overdraw Inspection](https://developer.android.com/topic/performance/rendering/inspect-gpu-rendering)
- [Skia Path Rendering Performance Discussion](https://groups.google.com/g/skia-discuss/c/Ko6JbkvN1fQ)
