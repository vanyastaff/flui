# GPU Tessellation Research: Vello Compute Pipeline & Masonry Widget Layer

**Date:** 2026-03-31
**Scope:** Study Vello's GPU compute pipeline and Masonry's widget architecture for applicability to FLUI

---

## 1. Vello GPU Compute Pipeline

### 1.1 Overview

Vello (linebender/vello) is a 2D GPU compute-centric renderer written in Rust using wgpu. Its core innovation is performing **all rendering stages as GPU compute shaders**, eliminating the traditional raster pipeline entirely. This is fundamentally different from FLUI's current approach of CPU-side lyon tessellation followed by GPU rasterization via wgpu render passes.

Vello uses **parallel prefix-sum algorithms** to parallelize work that traditionally must happen sequentially (sorting, aggregation, command generation), allowing the entire pipeline to run on the GPU with minimal CPU involvement.

### 1.2 Pipeline Stages

The pipeline flows through six sequential stages:

```
Scene Encoding (CPU) -> Flatten -> Binning -> Tile Allocation -> Coarse Rasterization -> Fine Rasterization
                         (GPU)     (GPU)       (GPU)              (GPU)                    (GPU)
```

#### Stage 1: Scene Encoding (CPU)

The `Encoding` struct maintains parallel data streams that are packed into a single GPU buffer:

| Stream | Purpose |
|--------|---------|
| `path_tags` | Commands for path processing |
| `path_data` | Coordinate data as bitwise `u32` |
| `draw_tags` | Commands for drawing operations |
| `draw_data` | Draw parameters (colors, gradients) |
| `transforms` | Affine transformation matrices |
| `styles` | Fill/stroke style information |

When `Scene::fill()` or `Scene::stroke()` is called, operations are encoded into these streams. The scene API is immediate-mode: callers push drawing commands, and the encoding is optimized for GPU consumption.

#### Stage 2: Flatten (`flatten.wgsl`)

Converts Bezier curves (quadratic and cubic) to line segments suitable for rasterization.

- **Input:** `path_tags`, `path_data` from scene buffer
- **Output:** `lines_buf` (line soup buffer), `path_bbox_buf` (bounding boxes)
- **Algorithm:** Adaptive subdivision based on flatness criteria

This is equivalent to what lyon does on the CPU in FLUI, but executed entirely on the GPU.

#### Stage 3: Binning (`binning.wgsl`)

Spatially sorts geometry into screen-space tiles (typically 16x16 pixels).

- **Input:** Draw objects, bounding boxes
- **Output:** `bin_header_buf`, `info_bin_data_buf`
- **Supporting shaders:**
  - `draw_reduce` / `draw_leaf`: Prefix-sum over draw tags
  - `clip_reduce` / `clip_leaf`: Prefix-sum over clip tags

Uses a "sort-middle" architecture: geometry is sorted by tile location before rasterization, rather than sorted by draw order. Clipping is handled through the draw tag/monoid system integrated into this stage.

#### Stage 4: Tile Allocation (`tile_alloc.wgsl`)

Allocates per-tile resources using GPU bump allocation.

- **Input:** Scene data, draw bounding boxes
- **Output:** `path_buf` (path metadata), `tile_buf` (per-tile structures)
- **Key structure:** `BumpAllocators` for dynamic GPU memory allocation

#### Stage 5: Coarse Rasterization (`coarse.wgsl`)

Generates **Per-Tile Command Lists (PTCL)** -- a list of drawing commands for each tile.

- **Input:** Binned data, path information
- **Output:** `ptcl_buf` (per-tile command buffer)
- **Supporting stages:**
  - `path_count_setup` / `path_count`: Count path segments per tile
  - `path_tiling_setup` / `path_tiling`: Assign path segments to tiles

This is analogous to building a per-tile draw list, where each tile knows exactly which primitives affect it.

#### Stage 6: Fine Rasterization (`fine.wgsl`)

The final pixel-level rasterization stage. Three variants exist:

| Variant | Anti-aliasing Method |
|---------|---------------------|
| `fine_area` | Area-based AA (fastest) |
| `fine_msaa8` | 8x multi-sample AA |
| `fine_msaa16` | 16x multi-sample AA |

- **Input:** PTCL commands from `ptcl_buf`
- **Output:** Final RGBA pixels written to output texture
- **Algorithm:** Interprets tile commands, computes winding numbers using prefix-sum for correct fill rules

### 1.3 Prefix-Sum Algorithm

The prefix-sum (parallel scan) is fundamental to Vello's design. It enables parallel computation of what would traditionally be sequential operations:

```wgsl
// Workgroup prefix sum of counts (from fine.wgsl)
for (var i = 0u; i < lg_n; i++) {
    workgroupBarrier();
    if th_ix >= 1u << i {
        count += sh_count[th_ix - (1u << i)];
    }
    workgroupBarrier();
    sh_count[th_ix] = count;
}
```

This is a classic parallel prefix-sum using logarithmic steps. Each iteration doubles the stride, allowing all threads to compute cumulative sums in O(log n) steps. It is used for:

- Aggregating draw monoids (binning)
- Computing winding numbers (fine rasterization)
- Memory allocation (tile allocation)
- Clip stack evaluation

### 1.4 Text Rendering

Vello renders text as **GPU vector paths** rather than using a glyph atlas:

1. Font outlines are extracted via `skrifa` (previously `swash`)
2. Glyph outlines are assembled into scene fragments
3. Outlines go through the same flatten -> bin -> rasterize pipeline as any other path

**Advantages:**
- No atlas texture memory overhead
- Perfect quality at any scale/rotation
- No atlas management complexity

**Disadvantages:**
- More GPU compute per glyph
- Slower on low-spec GPUs where texture sampling would be faster
- A glyph cache is on the roadmap to address this

### 1.5 Performance Characteristics

**When compute approach is faster:**
- Complex scenes with many overlapping paths
- Scenes with extensive clipping and masking
- Large text at varied sizes/rotations
- Scenes where CPU-side tessellation is the bottleneck

**When traditional approach is faster:**
- Simple UI scenes (rectangles, text, simple shapes)
- Low-spec GPUs without strong compute shader support
- imgui-style UIs made of text and simple graphic elements
- Scenes where draw call mapping is straightforward

**Key limitation:** Vello needs a GPU with compute shader support. This requires WebGPU on the web (not WebGL). Not all mobile GPUs have adequate compute shader performance.

### 1.6 WebGPU Requirements

- All shaders are written in WGSL
- Requires WebGPU compute shader support (not available in WebGL)
- Dynamic memory allocation patterns may stress some GPU drivers
- Image sampling across all scene images in a single shader is limited in WebGPU 1.0
- Uses wgpu as the runtime, same as FLUI

---

## 2. Masonry Widget Layer

### 2.1 Overview

Masonry is the widget toolkit underlying Xilem (linebender/xilem). It provides a **retained widget tree** with centralized event handling, layout, painting, and accessibility. It is built on Vello (rendering), Parley (text), and AccessKit (accessibility).

Masonry is designed to be **framework-agnostic**: it is not opinionated about the user-facing abstraction (immediate-mode, Elm architecture, FRP, etc.) but is opinionated about internals like focus, pointer interactions, and accessibility.

### 2.2 Widget Trait Design

The `Widget` trait (defined in `masonry_core/src/core/widget.rs`) has these key methods:

```rust
pub trait Widget: AsDynWidget + Any {
    type Action: Any + Debug where Self: Sized;

    // Event handling
    fn on_pointer_event(&mut self, ctx: &mut EventCtx, props: &mut PropertiesMut, event: &PointerEvent);
    fn on_text_event(&mut self, ctx: &mut EventCtx, props: &mut PropertiesMut, event: &TextEvent);
    fn on_access_event(&mut self, ctx: &mut EventCtx, props: &mut PropertiesMut, event: &AccessEvent);
    fn on_anim_frame(&mut self, ctx: &mut UpdateCtx, props: &mut PropertiesMut, interval: u64);
    fn on_action(&mut self, ctx: &mut ActionCtx, props: &mut PropertiesMut, action: &ErasedAction, source: WidgetId);

    // Tree management
    fn register_children(&mut self, ctx: &mut RegisterCtx);
    fn update(&mut self, ctx: &mut UpdateCtx, props: &mut PropertiesMut, event: &Update);
    fn property_changed(&mut self, ctx: &mut UpdateCtx, property_type: TypeId);

    // Layout (two-phase: measure + layout)
    fn measure(&mut self, ctx: &mut MeasureCtx, props: &PropertiesRef, axis: Axis, len_req: LenReq, cross_length: Option<f64>) -> f64;
    fn layout(&mut self, ctx: &mut LayoutCtx, props: &PropertiesRef, size: Size);

    // Rendering
    fn compose(&mut self, ctx: &mut ComposeCtx);
    fn pre_paint(&mut self, ctx: &mut PaintCtx, props: &PropertiesRef, painter: &mut Painter);
    fn paint(&mut self, ctx: &mut PaintCtx, props: &PropertiesRef, painter: &mut Painter);
    fn post_paint(&mut self, ctx: &mut PaintCtx, props: &PropertiesRef, painter: &mut Painter);

    // Accessibility
    fn accessibility_role(&self) -> Role;
    fn accessibility(&mut self, ctx: &mut AccessCtx, props: &PropertiesRef, node: &mut Node);

    // Child management
    fn children_ids(&self) -> ChildrenIds;

    // Configuration
    fn accepts_pointer_interaction(&self) -> bool;
    fn propagates_pointer_interaction(&self) -> bool;
    fn accepts_focus(&self) -> bool;
    fn accepts_text_input(&self) -> bool;
}
```

**Key design decisions:**

1. **Associated `Action` type** -- each widget declares what actions it can emit, providing type safety at the boundary.
2. **Properties system** -- `PropertiesMut`/`PropertiesRef` provide a CSS-like property inheritance mechanism separate from the widget itself (padding, dimensions, etc.).
3. **Three-phase painting** -- `pre_paint` (background/borders), `paint` (content, before children), `post_paint` (overlays, after children).
4. **Measure/Layout split** -- `measure` returns preferred length on an axis (called speculatively), `layout` receives final size (called definitively).

### 2.3 Arena-Based Widget Storage

Widgets are stored in a **global arena** rather than inline in parent widgets:

- Parent widgets hold `WidgetPod` handles (containing `WidgetId`)
- The arena maps `WidgetId` to the actual widget data
- This enables O(1) access to any widget by ID
- Makes adding features like "send accessibility event to widget X" trivial

**Comparison with FLUI:** FLUI uses a Slab-based arena with NonZeroUsize IDs (the ID offset pattern). The conceptual approach is identical -- Masonry's arena pattern validates FLUI's existing design choice.

### 2.4 Pass-Based Architecture

Masonry uses a **pass-based** update model. When user interaction occurs, passes run in sequence:

1. **Event pass** -- Route pointer/text/accessibility events to target widgets (bubbling up)
2. **Update pass** -- Notify widgets of state changes (Update enum: WidgetAdded, FocusChanged, etc.)
3. **Measure pass** -- Speculatively measure widgets that need sizing
4. **Layout pass** -- Compute final sizes and positions
5. **Compose pass** -- Compute final transforms
6. **Paint pass** -- Generate Vello scene fragments (pre_paint, paint, post_paint)
7. **Accessibility pass** -- Update AccessKit tree

Each pass uses dirty flags to skip unchanged widgets. Widgets request re-runs via context methods (`request_layout`, `request_paint_only`, `request_render`).

### 2.5 Taffy Integration

Taffy (a Rust flexbox/CSS Grid layout engine) is being integrated as an **optional layout mode**:

- Taffy provides CSS Flexbox and CSS Grid algorithms
- It integrates through the existing `measure`/`layout` split
- A `measure` function bridges Masonry's widget measurement to Taffy's sizing
- Taffy handles the constraint solving; Masonry handles the widget tree

The integration exposed limitations in the original layout API, leading to the current two-phase `measure` + `layout` design.

### 2.6 Dirty Tracking and Incremental Layout

Dirty tracking in Masonry uses per-widget state flags in `WidgetState`:

- `needs_layout`: Widget's size may have changed
- `needs_paint`: Widget's appearance changed (but not size)
- `needs_accessibility_update`: AccessKit node needs refresh
- `request_anim_frame`: Widget wants animation callbacks

When a widget calls `request_layout()`, the flag propagates up to ancestors. During the layout pass, only widgets with `needs_layout` (or whose constraints changed) are re-laid out. The measurement cache uses `(axis, len_req, cross_length)` as cache keys.

### 2.7 Composition Model

Masonry uses **composition over inheritance** (trait objects, not class hierarchies):

- `Widget` is a trait, not a base class
- `CollectionWidget<Params>` extends `Widget` for container widgets
- `AllowRawMut` marker trait for internal-only child access
- `WidgetMut<W>` provides type-safe mutable access with context
- `WidgetRef<W>` provides type-safe immutable access

Container widgets manage children through `WidgetPod` handles and arena storage, not by embedding child widgets directly.

---

## 3. Applicability to FLUI

### 3.1 What FLUI Could Adopt WITHOUT Changing the wgpu Rendering Backend

These improvements work with FLUI's existing lyon tessellation + wgpu render pipeline:

#### 3.1.1 Measurement Cache (from Masonry)
**Priority: HIGH**

Masonry's measurement caching with `(axis, len_req, cross_length)` keys is directly applicable to FLUI's `compute_intrinsic_size`. FLUI could cache intrinsic size computations per-axis, invalidating only when `mark_needs_layout()` is called.

#### 3.1.2 Three-Phase Painting (from Masonry)
**Priority: MEDIUM**

Masonry's `pre_paint` / `paint` / `post_paint` split cleanly separates background/border rendering from content rendering from overlay rendering. FLUI currently has a single `paint()` method. Adding `pre_paint` and `post_paint` would simplify widget implementations, especially for containers that need to draw on top of children.

#### 3.1.3 Properties System (from Masonry)
**Priority: MEDIUM**

Masonry's `PropertiesMut`/`PropertiesRef` system separates styling properties (padding, dimensions, colors) from widget logic. This is similar to CSS property inheritance. FLUI could adopt this to provide a more flexible theming system without modifying widget implementations.

#### 3.1.4 Scene Encoding Optimization (from Vello)
**Priority: MEDIUM**

Vello's parallel data stream encoding (separate arrays for tags, data, transforms, styles) is more cache-friendly than FLUI's current `Vec<Primitive>` approach in the scene graph. FLUI could restructure its scene representation to use SoA (Structure of Arrays) rather than AoS (Array of Structures) for better CPU cache utilization during scene building.

#### 3.1.5 Tile-Based Dirty Tracking (inspired by Vello)
**Priority: LOW**

While FLUI cannot use GPU-side tiling, it could implement CPU-side tile-based damage tracking: divide the screen into tiles and only re-render tiles that contain dirty regions. This would reduce GPU work for partial repaints.

### 3.2 What Would Require a New Backend

#### 3.2.1 Full Vello Compute Pipeline
**Impact: Transformative, but high cost**

Adopting Vello's full compute pipeline would mean replacing:
- Lyon tessellation (CPU) with GPU flatten/tessellate
- wgpu render passes with compute shader pipeline
- Per-draw-call rendering with tile-based rasterization

This would require a complete rewrite of `flui-engine` and would only work on GPUs with compute shader support. However, FLUI could potentially use Vello as an alternative backend (like how Skia had both Ganesh and Graphite backends).

#### 3.2.2 GPU Text Path Rendering
**Impact: Eliminates glyph atlas complexity**

Rendering text as GPU paths (Vello's approach) would eliminate FLUI's glyphon/cosmic-text atlas management. However, this requires the compute pipeline for acceptable performance -- doing it through the traditional raster pipeline would be too slow.

#### 3.2.3 Prefix-Sum Based Sorting
**Impact: Enables GPU-side draw ordering**

Vello's prefix-sum approach to sorting and aggregation on the GPU would require compute shader support and a fundamentally different rendering architecture.

### 3.3 Concrete Recommendations

| Recommendation | Source | Effort | Impact |
|---------------|--------|--------|--------|
| Add measurement caching to RenderBox | Masonry | Low | High |
| Restructure scene to SoA layout | Vello | Medium | Medium |
| Add pre_paint/post_paint to RenderObject | Masonry | Low | Medium |
| Implement CPU-side tile damage tracking | Vello-inspired | Medium | Medium |
| Investigate Vello as alternative backend | Vello | High | High |
| Add properties/theming system | Masonry | Medium | Medium |

**Recommended order of implementation:**
1. Measurement caching (immediate win for layout performance)
2. Pre_paint/post_paint (cleaner widget painting API)
3. SoA scene encoding (better cache performance during scene building)
4. CPU-side tile damage tracking (reduced GPU work for partial updates)
5. Properties system (better theming support)
6. Vello backend investigation (long-term, high-impact)

---

## Sources

- [Vello GitHub Repository](https://github.com/linebender/vello)
- [Vello Architecture (DeepWiki)](https://deepwiki.com/linebender/vello/1.1-architecture)
- [Vello Vision Document](https://github.com/linebender/vello/blob/main/doc/vision.md)
- [Vello Roadmap 2023](https://github.com/linebender/vello/blob/main/doc/roadmap_2023.md)
- [Vello Architecture Documentation Issue](https://github.com/linebender/vello/issues/488)
- [Porting Vello's GPU Tile Rasterizer to Pure Go](https://dev.to/kolkov/porting-vellos-gpu-tile-rasterizer-to-pure-go-7i8)
- [Vello Glyph Rendering Plan (Issue #204)](https://github.com/linebender/vello/issues/204)
- [Vello Text Rendering Questions (Issue #452)](https://github.com/linebender/vello/issues/452)
- [Raph Levien - Vello: High Performance (2023 Presentation)](https://www.datocms-assets.com/98516/1707130683-levien_2023.pdf)
- [Xilem GitHub Repository](https://github.com/linebender/xilem)
- [Masonry Source (masonry_core/src/core/widget.rs)](https://github.com/linebender/xilem/blob/main/masonry_core/src/core/widget.rs)
- [Masonry Widgets-in-Arenas RFC](https://github.com/linebender/rfcs/blob/main/rfcs/0006-widgets-in-arenas.md)
- [Xilem Widget Trait Design (Issue #7)](https://github.com/linebender/xilem/issues/7)
- [Taffy Integration PR (#682)](https://github.com/linebender/xilem/pull/682)
- [Post-mortem for Xilem Work in 2024](https://poignardazur.github.io/2025/03/24/plan-for-linebender-post-mortem/)
- [Linebender Monthly Updates](https://linebender.org/blog/tmix-08/)
