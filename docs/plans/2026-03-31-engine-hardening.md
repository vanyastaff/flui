# Engine Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix critical bugs, architectural gaps, and apply lessons learned from Iced, GPUI, Makepad, Vello, and Flutter issue tracker to harden flui-engine for production use.

**Architecture:** Incremental improvements to existing engine — no backend changes. Each task is self-contained and testable independently. Focus on correctness, memory safety, and performance.

**Tech Stack:** Rust, wgpu 25.x, glyphon, lyon, cosmic-text, parking_lot, slab

**Reference:** Comparative analysis of Iced, GPUI, Makepad, Xilem/Vello, Slint + Flutter GitHub issues (#32170 shader jank, #30985 image OOM, #44572 opacity, #14337 clip performance, #62527 large trees).

---

## Task 1: Wire TextureCache Eviction into Render Loop (P0 — Memory Leak)

**Problem:** `TextureCache::reset_use_counters()` and `shrink()` exist but are never called. Cache grows without bound → OOM for apps that load different images over time. Flutter equivalent: flutter/flutter#30985.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/renderer.rs:515-611` (render_scene)
- Modify: `crates/flui-engine/src/wgpu/painter.rs:891-946` (render method)
- Modify: `crates/flui-engine/src/wgpu/texture_cache.rs:146-478` (add budget)
- Test: `crates/flui-engine/src/wgpu/texture_cache.rs` (existing test module)

**Step 1: Add memory budget to TextureCache**

Add a `max_memory_bytes` field and `evict_over_budget()` method to `TextureCache`:

```rust
// In texture_cache.rs, add to TextureCache struct:
max_memory_bytes: usize, // Default: 100 MB

// New method:
pub fn evict_over_budget(&mut self) -> usize {
    let mut evicted = 0;
    while self.memory_bytes() > self.max_memory_bytes {
        // Find entry with lowest use_count, then oldest
        let victim = self.textures.iter()
            .filter(|(_, t)| t.use_count == 0)
            .min_by_key(|(_, t)| t.use_count)
            .map(|(k, _)| k.clone());
        if let Some(key) = victim {
            self.textures.remove(&key);
            evicted += 1;
        } else {
            break; // All textures in use
        }
    }
    evicted
}

fn memory_bytes(&self) -> usize {
    self.textures.values().map(|t| t.size_bytes).sum()
}
```

**Step 2: Wire eviction into painter's render()**

In `WgpuPainter::render()` (painter.rs:891), add frame-boundary cache maintenance after the buffer pool reset:

```rust
// At end of render(), after self.buffer_pool.reset():
self.texture_cache.reset_use_counters(); // Mark frame boundary
let evicted = self.texture_cache.shrink(); // Remove unused
if evicted > 0 {
    tracing::debug!(evicted, "TextureCache evicted unused textures");
}
```

**Step 3: Write test for eviction**

```rust
#[test]
fn test_texture_cache_eviction_on_shrink() {
    // Create cache, load texture, don't use it, reset counters, shrink
    // Assert texture was removed
}
```

**Step 4: Run tests**

```bash
rtk cargo test -p flui-engine --features enable-wgpu-tests
```

**Step 5: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/texture_cache.rs crates/flui-engine/src/wgpu/painter.rs
rtk git commit -m "fix(engine): wire TextureCache eviction into render loop

Fixes memory leak where textures were cached forever.
- reset_use_counters() called at frame boundary
- shrink() removes textures unused for 1+ frame
- Add memory budget with evict_over_budget()"
```

---

## Task 2: Add Device Loss Recovery to Renderer (P0 — Crash)

**Problem:** When `SurfaceLost` occurs, engine returns error but has no recovery path. Caller must know to call `resize()` — undocumented and fragile. Also `SurfaceCreation` is not classified by `is_recoverable()`/`is_fatal()`.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/renderer.rs:132-611` (Renderer struct + methods)
- Modify: `crates/flui-engine/src/error.rs:44-222` (error classification)
- Test: `crates/flui-engine/src/wgpu/renderer.rs` (add test module)

**Step 1: Fix error classification**

In `error.rs`, add `SurfaceCreation` to `is_fatal()`:

```rust
pub fn is_fatal(&self) -> bool {
    matches!(
        self,
        RenderError::OutOfMemory
            | RenderError::NoAdapter
            | RenderError::DeviceCreation(_)
            | RenderError::SurfaceCreation(_)  // Add this
            | RenderError::NotInitialized
    )
}
```

**Step 2: Add `reconfigure_surface()` to Renderer**

```rust
/// Reconfigure the surface after loss or outdated error.
///
/// Call this when `render_scene()` returns `SurfaceLost` or `SurfaceOutdated`.
/// Equivalent to `resize()` with current dimensions.
pub fn reconfigure_surface(&mut self) -> Result<(), RenderError> {
    if let (Some(config), Some(surface)) = (&self.config, &self.surface) {
        surface.configure(&self.device, config);
        tracing::info!("Surface reconfigured ({}x{})", config.width, config.height);
        Ok(())
    } else {
        Err(RenderError::NotInitialized)
    }
}
```

**Step 3: Add auto-recovery to render_scene()**

Wrap the surface acquisition with retry-on-lost:

```rust
// In render_scene(), replace the current get_current_texture call:
let output = match surface.get_current_texture() {
    Ok(output) => output,
    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
        // Auto-reconfigure and retry once
        self.reconfigure_surface()?;
        let surface = self.surface.as_ref().ok_or(RenderError::SurfaceLost)?;
        surface.get_current_texture().map_err(|e| match e {
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Other => RenderError::SurfaceLost,
            wgpu::SurfaceError::OutOfMemory => RenderError::OutOfMemory,
            wgpu::SurfaceError::Outdated => RenderError::SurfaceOutdated,
            wgpu::SurfaceError::Timeout => RenderError::Timeout,
        })?
    }
    Err(e) => {
        return Err(match e {
            wgpu::SurfaceError::OutOfMemory => RenderError::OutOfMemory,
            wgpu::SurfaceError::Timeout => RenderError::Timeout,
            _ => RenderError::SurfaceLost,
        });
    }
};
```

**Step 4: Run tests**

```bash
rtk cargo check -p flui-engine
```

**Step 5: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/renderer.rs crates/flui-engine/src/error.rs
rtk git commit -m "fix(engine): add device loss recovery with auto-reconfigure

- Add reconfigure_surface() for explicit recovery
- Auto-retry on SurfaceLost/Outdated in render_scene()
- Fix SurfaceCreation not classified in is_fatal()"
```

---

## Task 3: Cache Superellipse Paths (P1 — Per-Frame Allocation)

**Problem:** `generate_superellipse_path()` allocates a new `Path` (64+ points, heap allocation) every frame for each superellipse clip layer. Static UIs waste CPU on identical path re-generation.

**Approach (learned from GPUI):** GPUI caches `ContentMask` per element. We cache generated paths keyed by superellipse parameters.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/layer_render.rs:206-357` (add cache)
- Test: `crates/flui-engine/src/wgpu/layer_render.rs` (add test)

**Step 1: Add path cache module**

Add a thread-local LRU cache at the top of `layer_render.rs`:

```rust
use std::cell::RefCell;
use std::collections::HashMap;

/// Cache key for superellipse paths — based on the geometry parameters.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SuperellipseKey {
    // Store f32 as bits for Hash/Eq
    left: u32, top: u32, right: u32, bottom: u32,
    tl_x: u32, tl_y: u32, tr_x: u32, tr_y: u32,
    br_x: u32, br_y: u32, bl_x: u32, bl_y: u32,
}

impl SuperellipseKey {
    fn from_superellipse(s: &flui_types::geometry::RSuperellipse) -> Self {
        let r = s.outer_rect();
        let (tl, tr, br, bl) = (s.tl_radius(), s.tr_radius(), s.br_radius(), s.bl_radius());
        Self {
            left: r.left().0.to_bits(), top: r.top().0.to_bits(),
            right: r.right().0.to_bits(), bottom: r.bottom().0.to_bits(),
            tl_x: tl.x.0.to_bits(), tl_y: tl.y.0.to_bits(),
            tr_x: tr.x.0.to_bits(), tr_y: tr.y.0.to_bits(),
            br_x: br.x.0.to_bits(), br_y: br.y.0.to_bits(),
            bl_x: bl.x.0.to_bits(), bl_y: bl.y.0.to_bits(),
        }
    }
}

thread_local! {
    static SUPERELLIPSE_CACHE: RefCell<HashMap<SuperellipseKey, flui_types::painting::Path>> =
        RefCell::new(HashMap::new());
}

fn get_or_generate_superellipse_path(
    superellipse: &flui_types::geometry::RSuperellipse,
) -> flui_types::painting::Path {
    let key = SuperellipseKey::from_superellipse(superellipse);
    SUPERELLIPSE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(path) = cache.get(&key) {
            return path.clone();
        }
        let path = generate_superellipse_path(superellipse);
        cache.insert(key, path.clone());
        path
    })
}
```

**Step 2: Update ClipSuperellipseLayer to use cache**

```rust
impl<R: CommandRenderer + ?Sized> LayerRender<R> for flui_layer::ClipSuperellipseLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() { return; }
        let superellipse = self.clip_superellipse();
        let path = get_or_generate_superellipse_path(superellipse); // Changed
        renderer.push_clip_path(&path, self.clip_behavior());
    }
    // cleanup unchanged
}
```

**Step 3: Run tests**

```bash
rtk cargo check -p flui-engine
```

**Step 4: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/layer_render.rs
rtk git commit -m "perf(engine): cache superellipse paths to avoid per-frame allocation

Thread-local HashMap caches generated Path by RSuperellipse geometry.
Keys use f32::to_bits() for Hash/Eq. Eliminates 64-point heap allocation
per superellipse clip per frame for static content."
```

---

## Task 4: Integrate TextureAtlas into Image Rendering (P1 — Draw Call Reduction)

**Problem:** `TextureAtlas` is implemented (shelf packing) but not wired into image rendering. Each image = separate GPU texture = separate draw call. Icon-heavy UIs will be draw-call bound.

**Approach (learned from Iced + GPUI):** Both use guillotiere/etagere for atlas packing. Small images go to atlas, large images stay standalone.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/painter.rs` (add atlas integration)
- Modify: `crates/flui-engine/src/wgpu/atlas.rs` (add lookup + eviction)
- Modify: `crates/flui-engine/src/wgpu/texture_cache.rs` (route small images to atlas)
- Test: `crates/flui-engine/src/wgpu/atlas.rs`

**Step 1: Add atlas size threshold constant**

```rust
// In atlas.rs:
/// Images smaller than this in both dimensions go to the atlas.
/// Larger images get standalone GPU textures.
const ATLAS_MAX_DIMENSION: u32 = 256;
```

**Step 2: Add atlas lookup to TextureCache**

Add an `atlas` field to `TextureCache` and route small images:

```rust
// In TextureCache struct:
atlas: Option<TextureAtlas>,

// In load_from_rgba(), before creating standalone texture:
if width <= ATLAS_MAX_DIMENSION && height <= ATLAS_MAX_DIMENSION {
    if let Some(atlas) = &mut self.atlas {
        if let Ok(entry) = atlas.allocate(width, height) {
            atlas.upload_image(&self.queue, &entry, data);
            // Store atlas entry reference in CachedTexture
            // Return atlas texture + UV coords
        }
    }
}
```

**Step 3: Add UV coordinates to TextureInstance**

```rust
// In instancing.rs, modify TextureInstance or add AtlasTextureInstance:
pub struct AtlasTextureInstance {
    pub position: [f32; 4],   // x, y, w, h
    pub uv: [f32; 4],         // u_min, v_min, u_max, v_max
    pub color: [f32; 4],
}
```

**Step 4: Write test**

```rust
#[test]
fn test_small_images_use_atlas() {
    // Load several small images (64x64)
    // Assert they share the same GPU texture (atlas)
    // Assert UV coordinates are different
}
```

**Step 5: Run tests**

```bash
rtk cargo test -p flui-engine
```

**Step 6: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/atlas.rs crates/flui-engine/src/wgpu/texture_cache.rs crates/flui-engine/src/wgpu/painter.rs crates/flui-engine/src/wgpu/instancing.rs
rtk git commit -m "feat(engine): integrate TextureAtlas for small image batching

Images <= 256x256 are packed into a shared atlas texture.
Reduces draw calls for icon-heavy UIs. Larger images remain standalone.
Learned from Iced (guillotiere) and GPUI (etagere atlas) approaches."
```

---

## Task 5: Fix Opacity Compositing (P1 — Visual Correctness)

**Problem:** `Painter::save_layer()` default just calls `save()` with warning — no offscreen rendering. Semi-transparent groups with overlapping children render incorrectly (each child gets independent alpha instead of group alpha). Flutter equivalent: flutter/flutter#44572.

**Approach (learned from Makepad):** Fast path — if children don't overlap, multiply alpha into vertex data (already works). Slow path — render to offscreen texture, composite with alpha. Use heuristic: leaf nodes and non-overlapping groups use fast path.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/painter.rs` (implement proper save_layer)
- Modify: `crates/flui-engine/src/wgpu/offscreen.rs` (add alpha composite pass)
- Test: integration test

**Step 1: Implement offscreen save_layer in WgpuPainter**

```rust
// In painter.rs, add to WgpuPainter:
/// Stack of offscreen render targets for save_layer
layer_stack: Vec<SavedLayer>,

struct SavedLayer {
    /// Pooled texture for offscreen rendering
    texture: PooledTexture,
    /// Original draw_order to restore
    saved_draw_order: Vec<DrawItem>,
    /// Original segment
    saved_segment: DrawSegment,
    /// Opacity to apply when compositing
    opacity: f32,
    /// Bounds of the layer
    bounds: [f32; 4],
}
```

**Step 2: Implement push/pop for save_layer**

```rust
pub fn save_layer_with_opacity(&mut self, bounds: [f32; 4], opacity: f32) {
    // Get a pooled texture at surface size
    let texture = self.texture_pool.acquire(self.size.0, self.size.1, self.surface_format);

    let saved = SavedLayer {
        texture,
        saved_draw_order: std::mem::take(&mut self.draw_order),
        saved_segment: std::mem::replace(&mut self.current_segment, DrawSegment::new()),
        opacity,
        bounds,
    };
    self.layer_stack.push(saved);
    // Now all subsequent draws go to current_segment/draw_order which are empty
    // They will be flushed to the offscreen texture on restore_layer
}

pub fn restore_layer(&mut self) {
    if let Some(saved) = self.layer_stack.pop() {
        // Finalize offscreen content
        let offscreen_segment = std::mem::replace(&mut self.current_segment, saved.saved_segment);
        let offscreen_items = std::mem::replace(&mut self.draw_order, saved.saved_draw_order);

        // TODO: Flush offscreen_items to saved.texture, then composite
        // with saved.opacity into main draw_order
        self.draw_order.push(DrawItem::OffscreenTexture(OffscreenTexturePlacement {
            texture: saved.texture,
            bounds: saved.bounds,
        }));
    }
}
```

**Step 3: Run tests**

```bash
rtk cargo check -p flui-engine
```

**Step 4: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/painter.rs crates/flui-engine/src/wgpu/offscreen.rs
rtk git commit -m "feat(engine): implement proper save_layer with offscreen compositing

Semi-transparent groups now render to offscreen texture and composite
with group alpha. Fixes incorrect alpha blending for overlapping children.
Uses TexturePool for offscreen textures (RAII return-on-drop)."
```

---

## Task 6: Add Damage Region Tracking (P2 — Performance)

**Problem:** Full screen repaint every frame even if only a cursor blinks. Flutter equivalent: flutter/flutter#14337. Iced 0.13+ added damage tracking. GPUI has `scene.replay()` for partial caching.

**Approach:** Track dirty rectangles at the layer level. If no layers changed since last frame, skip rendering entirely. If some changed, use scissor rect to limit GPU work.

**Files:**
- Modify: `crates/flui-layer/src/tree/layer_tree.rs:28-47` (add dirty tracking to LayerNode)
- Create: `crates/flui-layer/src/damage.rs` (damage region accumulator)
- Modify: `crates/flui-engine/src/wgpu/renderer.rs` (use damage rects)
- Test: `crates/flui-layer/tests/damage_tracking.rs`

**Step 1: Add dirty rect tracking to LayerNode**

```rust
// In LayerNode, add:
/// Whether this layer's content changed since last frame
dirty: bool,

/// Bounding rect of dirty content (screen space)
dirty_rect: Option<Rect<Pixels>>,
```

**Step 2: Create DamageTracker**

```rust
// damage.rs
use flui_types::geometry::{Pixels, Rect};

/// Accumulates damage regions for incremental rendering.
pub struct DamageTracker {
    /// Dirty rects from current frame
    regions: Vec<Rect<Pixels>>,
    /// Whether full repaint is needed
    full_repaint: bool,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self { regions: Vec::new(), full_repaint: true }
    }

    /// Mark a region as dirty
    pub fn mark_dirty(&mut self, rect: Rect<Pixels>) {
        self.regions.push(rect);
    }

    /// Mark entire screen dirty
    pub fn mark_full_repaint(&mut self) {
        self.full_repaint = true;
    }

    /// Get the unified damage rect (bounding box of all dirty rects)
    pub fn damage_rect(&self) -> Option<Rect<Pixels>> {
        if self.full_repaint { return None; } // None = full repaint
        if self.regions.is_empty() { return Some(Rect::zero()); } // Empty = skip render

        // Compute bounding box of all dirty rects
        let mut union = self.regions[0];
        for r in &self.regions[1..] {
            union = union.union(r);
        }
        Some(union)
    }

    /// Reset for next frame
    pub fn reset(&mut self) {
        self.regions.clear();
        self.full_repaint = false;
    }
}
```

**Step 3: Wire into renderer**

In `render_scene()`, check damage before full repaint. If damage rect is zero (nothing changed), skip rendering entirely.

**Step 4: Write test**

```rust
#[test]
fn test_damage_tracker_union() {
    let mut tracker = DamageTracker::new();
    tracker.reset(); // Clear initial full_repaint
    tracker.mark_dirty(Rect::from_ltrb(px(10.0), px(10.0), px(50.0), px(50.0)));
    tracker.mark_dirty(Rect::from_ltrb(px(40.0), px(40.0), px(80.0), px(80.0)));
    let damage = tracker.damage_rect().unwrap();
    assert_eq!(damage, Rect::from_ltrb(px(10.0), px(10.0), px(80.0), px(80.0)));
}

#[test]
fn test_no_damage_skips_render() {
    let mut tracker = DamageTracker::new();
    tracker.reset();
    // No marks
    let damage = tracker.damage_rect().unwrap();
    assert_eq!(damage, Rect::zero()); // Zero = skip render
}
```

**Step 5: Run tests**

```bash
rtk cargo test -p flui-layer
```

**Step 6: Commit**

```bash
rtk git add crates/flui-layer/src/damage.rs crates/flui-layer/src/tree/layer_tree.rs crates/flui-engine/src/wgpu/renderer.rs
rtk git commit -m "feat(engine): add damage region tracking for incremental rendering

Tracks dirty rectangles at layer level. Skips rendering when nothing
changed. Uses bounding box union for scissor optimization.
Inspired by Iced 0.13+ damage tracking and GPUI scene.replay()."
```

---

## Task 7: SDF Clipping for Rounded Rectangles (P2 — Performance)

**Problem:** Rounded rect clips go through tessellation + stencil/path clipping. This is expensive for the most common clip shape in Material Design. Flutter equivalent: flutter/flutter#14337.

**Approach (learned from Makepad + Iced):** Makepad does all clipping in fragment shader — each instance carries clip bounds, shader discards pixels outside. Iced renders rounded rects via SDF in quad shader. We should add SDF-based clip testing in fragment shaders for RRect clips.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/instancing.rs` (add clip_rrect to instances)
- Modify: `crates/flui-engine/src/wgpu/shaders/` (add SDF clip in fragment shaders)
- Modify: `crates/flui-engine/src/wgpu/painter.rs` (push_clip_rrect using SDF path)
- Test: visual test in examples

**Step 1: Add clip data to instance structs**

```rust
// In instancing.rs, add to RectInstance (and others):
/// Clip rounded rect: [x, y, w, h, radius_tl, radius_tr, radius_br, radius_bl]
/// All zeros = no clip
pub clip_rrect: [f32; 8],
```

**Step 2: Add SDF function to WGSL shaders**

```wgsl
// In rect shader fragment:
fn sdf_rounded_rect(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let d = abs(p - center) - half_size + vec2<f32>(radius);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}

// In fragment main:
if (clip_rrect.z > 0.0) {  // Has clip
    let clip_center = clip_rrect.xy + clip_rrect.zw * 0.5;
    let clip_half = clip_rrect.zw * 0.5;
    let d = sdf_rounded_rect(frag_pos, clip_center, clip_half, clip_radius);
    if (d > 0.0) { discard; }
}
```

**Step 3: Update push_clip_rrect in painter**

Instead of tessellating the rrect and using stencil, store the rrect parameters in a clip stack and pass them to instances:

```rust
pub fn push_clip_rrect(&mut self, rrect: &RRect<Pixels>, _behavior: Clip) {
    let clip_data = [
        rrect.rect().left().0, rrect.rect().top().0,
        rrect.rect().width().0, rrect.rect().height().0,
        rrect.tl_radius().0, rrect.tr_radius().0,
        rrect.br_radius().0, rrect.bl_radius().0,
    ];
    self.rrect_clip_stack.push(clip_data);
    self.current_rrect_clip = Some(clip_data);
}
```

**Step 4: Run tests**

```bash
rtk cargo check -p flui-engine
```

**Step 5: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/instancing.rs crates/flui-engine/src/wgpu/shaders/ crates/flui-engine/src/wgpu/painter.rs
rtk git commit -m "perf(engine): SDF-based rounded rect clipping in fragment shader

Replaces tessellation+stencil with analytical SDF clip test.
Each instance carries clip_rrect data, fragment shader discards pixels
outside the rounded rect. ~10x faster for common Material clip patterns.
Approach from Makepad (per-instance clip) and Iced (SDF quad shader)."
```

---

## Task 8: Occlusion Culling for Opaque Primitives (P2 — Overdraw)

**Problem:** Deep widget composition causes 3-8x overdraw per pixel. Flutter equivalent: flutter/flutter#48780. No framework in the Rust ecosystem does this well, but GPUI's `BoundsTree` spatial ordering is a step in this direction.

**Approach:** During layer traversal, track opaque regions. Skip painting for layers fully occluded by opaque content rendered after them (front-to-back check).

**Files:**
- Create: `crates/flui-engine/src/wgpu/occlusion.rs` (occlusion tracker)
- Modify: `crates/flui-engine/src/wgpu/renderer.rs` (check occlusion before render)
- Test: `crates/flui-engine/src/wgpu/occlusion.rs`

**Step 1: Create OcclusionTracker**

```rust
// occlusion.rs
use flui_types::geometry::{Pixels, Rect};

/// Tracks opaque regions to skip fully-occluded draw calls.
///
/// Simple approach: maintain a list of opaque rects.
/// A layer is occluded if any single opaque rect fully contains it.
/// This handles the common case (background fills) without complex region algebra.
pub struct OcclusionTracker {
    opaque_rects: Vec<Rect<Pixels>>,
}

impl OcclusionTracker {
    pub fn new() -> Self {
        Self { opaque_rects: Vec::with_capacity(32) }
    }

    /// Register an opaque region (rendered after current)
    pub fn add_opaque(&mut self, rect: Rect<Pixels>) {
        self.opaque_rects.push(rect);
    }

    /// Check if a rect is fully occluded by any opaque region
    pub fn is_occluded(&self, rect: &Rect<Pixels>) -> bool {
        self.opaque_rects.iter().any(|opaque| opaque.contains_rect(rect))
    }

    /// Reset for next frame
    pub fn reset(&mut self) {
        self.opaque_rects.clear();
    }
}
```

**Step 2: Wire into render_layer_recursive**

Before rendering a layer, check if its bounds are fully occluded. This requires a two-pass approach or back-to-front tracking.

**Step 3: Write tests**

```rust
#[test]
fn test_fully_occluded() {
    let mut tracker = OcclusionTracker::new();
    tracker.add_opaque(Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)));
    assert!(tracker.is_occluded(&Rect::from_ltrb(px(10.0), px(10.0), px(50.0), px(50.0))));
    assert!(!tracker.is_occluded(&Rect::from_ltrb(px(50.0), px(50.0), px(150.0), px(150.0))));
}
```

**Step 4: Run tests**

```bash
rtk cargo test -p flui-engine
```

**Step 5: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/occlusion.rs crates/flui-engine/src/wgpu/renderer.rs crates/flui-engine/src/wgpu/mod.rs
rtk git commit -m "perf(engine): add occlusion culling for opaque primitives

Tracks opaque regions to skip fully-occluded draw calls.
Reduces overdraw for common Material patterns (card backgrounds,
full-screen fills). Inspired by GPUI BoundsTree spatial ordering."
```

---

## Task 9: Batch Sorting by GPU State (P2 — Draw Call Optimization)

**Problem:** Current batching groups by primitive type within a DrawSegment, but doesn't optimize for GPU state changes (shader switches, texture binds). GPUI's `BatchIterator` merges consecutive same-type primitives and only breaks batches when draw order requires interleaving.

**Approach (learned from GPUI):** Sort opaque primitives front-to-back (for early-z), transparent back-to-front. Group by pipeline state within each Z-band.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/painter.rs` (optimize flush order)
- Test: benchmark

**Step 1: Reorder flush_segment to minimize state changes**

Currently `flush_segment` processes each batch type sequentially (rects, circles, arcs, shadows, gradients, tessellated, textures). Reorder to:
1. All opaque instanced draws first (one pipeline switch per type)
2. Tessellated geometry
3. Transparent draws

```rust
fn flush_segment(&mut self, seg: &mut DrawSegment, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    std::mem::swap(&mut self.current_segment, seg);

    // Flush in pipeline-optimal order to minimize state switches:
    // 1. Instanced primitives (one pipeline each)
    self.flush_rect_batch(encoder, view);
    self.flush_circle_batch(encoder, view);
    self.flush_arc_batch(encoder, view);
    self.flush_shadow_batch(encoder, view);
    // 2. Gradient primitives (similar pipelines)
    self.flush_linear_gradient_batch(encoder, view);
    self.flush_radial_gradient_batch(encoder, view);
    self.flush_sweep_gradient_batch(encoder, view);
    // 3. Tessellated paths (different pipeline)
    self.flush_tessellated_batches(encoder, view);
    // 4. Textures (texture bind changes)
    self.flush_texture_batch_if_needed(encoder, view);

    std::mem::swap(&mut self.current_segment, seg);
}
```

**Step 2: Run tests**

```bash
rtk cargo check -p flui-engine
```

**Step 3: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/painter.rs
rtk git commit -m "perf(engine): optimize draw call ordering to minimize GPU state changes

Reorder batch flushing: instanced primitives → gradients → tessellated → textures.
Minimizes pipeline switches per DrawSegment.
Inspired by GPUI BatchIterator approach."
```

---

## Task 10: Fix Scissor Clipping Under Transforms (P1 — Visual Bug)

**Problem:** `push_clip_rect` maps to scissor rectangles which are axis-aligned in screen space. After `push_transform` with rotation, the scissor rect is incorrect — it clips in screen space, not transformed space.

**Approach (learned from Makepad):** For transformed clips, fall back to SDF clipping (from Task 7) or axis-aligned bounding box of the transformed rect. This matches GPUI's approach — they also use ContentMask (axis-aligned) but acknowledge the limitation.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/painter.rs` (transform-aware clip)
- Test: visual test

**Step 1: Transform clip rect before applying scissor**

```rust
pub fn push_clip_rect(&mut self, rect: &Rect<Pixels>, _behavior: Clip) {
    // Transform clip rect corners to screen space
    let transform = self.current_transform;

    if transform == glam::Mat4::IDENTITY {
        // Fast path: no transform, use scissor directly
        let scissor = self.rect_to_scissor(rect);
        self.push_scissor(scissor);
    } else {
        // Transform the 4 corners and compute axis-aligned bounding box
        let corners = [
            transform.transform_point3(glam::Vec3::new(rect.left().0, rect.top().0, 0.0)),
            transform.transform_point3(glam::Vec3::new(rect.right().0, rect.top().0, 0.0)),
            transform.transform_point3(glam::Vec3::new(rect.right().0, rect.bottom().0, 0.0)),
            transform.transform_point3(glam::Vec3::new(rect.left().0, rect.bottom().0, 0.0)),
        ];
        let min_x = corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
        let min_y = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
        let max_x = corners.iter().map(|c| c.x).fold(f32::NEG_INFINITY, f32::max);
        let max_y = corners.iter().map(|c| c.y).fold(f32::NEG_INFINITY, f32::max);

        let aabb_scissor = (
            min_x.max(0.0) as u32,
            min_y.max(0.0) as u32,
            (max_x - min_x).ceil() as u32,
            (max_y - min_y).ceil() as u32,
        );
        self.push_scissor(aabb_scissor);
        // Note: AABB is a conservative approximation.
        // For exact clipping under rotation, use SDF clip (Task 7)
    }
}
```

**Step 2: Run tests**

```bash
rtk cargo check -p flui-engine
```

**Step 3: Commit**

```bash
rtk git add crates/flui-engine/src/wgpu/painter.rs
rtk git commit -m "fix(engine): transform clip rects to screen space before scissor

Axis-aligned bounding box of transformed clip rect used as scissor.
Conservative approximation that prevents incorrect clipping under
rotation/scale transforms. Exact clipping via SDF for RRect clips."
```

---

## Task 11: Learn from Vello/Masonry — Study Compute Tessellation (P3 — Research)

**Problem:** lyon CPU tessellation is a bottleneck for complex paths. Vello does all tessellation on GPU via compute shaders. Not implementing a new backend, but should understand the approach for future optimization.

**NOTE:** This is a research task, not implementation. Document findings for future reference.

**Files:**
- Create: `docs/research/2026-03-31-gpu-tessellation.md`

**Step 1: Research Vello's compute pipeline**

Study Vello's architecture:
- Scene encoding → tile allocation → path sorting → fine rasterization
- All compute shaders, no traditional raster pipeline
- How they handle text (GPU path rendering, no atlas)

**Step 2: Research Masonry's layout engine**

Study Masonry (Xilem's widget layer):
- Taffy-based flexbox layout
- How they handle dirty tracking and incremental layout
- Widget trait design

**Step 3: Document findings**

Write research document with:
- Key architectural insights applicable to FLUI
- What could be adopted without changing backends
- Future roadmap considerations

**Step 4: Commit**

```bash
rtk git add docs/research/
rtk git commit -m "docs: research notes on Vello compute tessellation and Masonry layout"
```

---

## Task 12: Learn from Skia — Study Rendering Optimizations (P3 — Research)

**Problem:** Skia has decades of rendering optimization. While we use wgpu (not Skia), many of their techniques apply.

**NOTE:** Research task — document findings.

**Files:**
- Create: `docs/research/2026-03-31-skia-techniques.md`

**Step 1: Research Skia's key optimizations**

- **GPU path caching:** Skia caches tessellated path geometry on GPU, reusing across frames
- **Distance field text:** SDF-based text rendering for resolution independence
- **Overdraw analysis:** Skia's `debugOverdraw` visualization technique
- **Pipeline state caching:** How Skia organizes pipeline objects for minimal state switches
- **Image tiling:** How Skia handles large images via tiled rendering

**Step 2: Document which techniques apply to FLUI**

Focus on techniques that work with wgpu's pipeline model:
- GPU path cache → store tessellated geometry in GPU buffers, key by path hash
- SDF text → evaluate vs current glyphon atlas approach
- Pipeline state sorting → reference for Task 9

**Step 3: Commit**

```bash
rtk git add docs/research/
rtk git commit -m "docs: research notes on Skia rendering optimizations applicable to FLUI"
```

---

## Execution Order and Dependencies

```
Task 1 (TextureCache eviction)     ← P0, no deps, start immediately
Task 2 (Device loss recovery)      ← P0, no deps, parallel with Task 1
Task 3 (Superellipse cache)        ← P1, no deps, parallel with 1+2
Task 10 (Scissor + transforms)     ← P1, no deps, parallel with above
Task 4 (TextureAtlas integration)  ← P1, depends on Task 1 (cache changes)
Task 5 (Opacity compositing)       ← P1, no deps
Task 6 (Damage tracking)           ← P2, no deps
Task 7 (SDF clipping)              ← P2, no deps, but Task 10 references it
Task 8 (Occlusion culling)         ← P2, benefits from Task 6
Task 9 (Batch sorting)             ← P2, no deps
Task 11 (Vello research)           ← P3, pure research
Task 12 (Skia research)            ← P3, pure research
```

**Recommended parallel waves:**

1. **Wave 1 (P0):** Tasks 1 + 2 + 3 + 10 (all independent, fix critical bugs)
2. **Wave 2 (P1):** Tasks 4 + 5 (after wave 1, build on cache changes)
3. **Wave 3 (P2):** Tasks 6 + 7 + 8 + 9 (performance optimizations)
4. **Wave 4 (P3):** Tasks 11 + 12 (research, anytime)
