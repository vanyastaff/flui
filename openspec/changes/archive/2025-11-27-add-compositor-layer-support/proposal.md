# Proposal: Add Compositor Layer Support for Advanced Effects

## Meta

- **ID:** add-compositor-layer-support
- **Status:** PROPOSED
- **Created:** 2025-01-26
- **Author:** AI Assistant (requested by user)
- **Type:** Feature / Architecture Enhancement
- **Related Changes:** validate-effects-against-flutter

## Problem Statement

Two effect RenderObjects (`RenderShaderMask` and `RenderBackdropFilter`) are currently non-functional because they require compositor-level layer support that doesn't exist in FLUI's current architecture.

**Current Architecture:**
- FLUI has only **one layer type**: `CanvasLayer` (picture.rs)
- All effects (Transform, Opacity, Clip, etc.) are implemented as **RenderObjects** that generate Canvas commands
- This works well for simple canvas operations but **cannot** support advanced effects that require:
  - Rendering child to intermediate texture (ShaderMask)
  - Access to backdrop/previous layers (BackdropFilter)

**What's Broken:**
```rust
// crates/flui_rendering/src/objects/effects/shader_mask.rs
impl RenderBox<Single> for RenderShaderMask {
    fn paint(&self, ctx: &mut PaintContext) {
        // TODO: Implement ShaderMaskLayer when compositor supports it
        ctx.paint_child(child_id, ctx.offset); // ❌ No masking applied!
    }
}

// crates/flui_rendering/src/objects/effects/backdrop_filter.rs
impl RenderBox<Single> for RenderBackdropFilter {
    fn paint(&self, ctx: &mut PaintContext) {
        // TODO: Implement BackdropFilterLayer when compositor supports it
        ctx.paint_child(child_id, ctx.offset); // ❌ No filtering applied!
    }
}
```

**Why This Matters:**
- **ShaderMask**: Used for gradient fades, vignettes, custom masking patterns
- **BackdropFilter**: Essential for frosted glass effects, backdrop blur (iOS-style UI)
- **Flutter Parity**: Both are standard in Flutter, users expect them to work

## Proposed Solution

### Overview

Add two new layer types to the compositor system and implement proper rendering support for ShaderMask and BackdropFilter effects.

**Architecture Change:**
```
BEFORE:
flui_engine/src/layer/
├── mod.rs              # Only exports CanvasLayer
└── picture.rs          # CanvasLayer (only layer type)

AFTER:
flui_engine/src/layer/
├── mod.rs              # Exports CanvasLayer, ShaderMaskLayer, BackdropFilterLayer
├── picture.rs          # CanvasLayer
├── shader_mask.rs      # NEW: ShaderMaskLayer
└── backdrop_filter.rs  # NEW: BackdropFilterLayer
```

### Key Components

#### 1. ShaderMaskLayer (NEW)

**Purpose:** Render child to offscreen texture, apply shader as mask, composite result

**API:**
```rust
pub struct ShaderMaskLayer {
    /// Child layer to mask
    pub child: Box<dyn Layer>,
    /// Shader specification
    pub shader: ShaderSpec,
    /// Blend mode
    pub blend_mode: BlendMode,
    /// Bounds for rendering
    pub bounds: Rect,
}

impl Layer for ShaderMaskLayer {
    fn render(&self, renderer: &mut dyn CommandRenderer);
    fn bounds(&self) -> Rect;
}
```

**Rendering Flow:**
1. Allocate offscreen texture
2. Render child layer to texture
3. Apply shader as mask (GPU shader operation)
4. Composite masked result to main framebuffer

#### 2. BackdropFilterLayer (NEW)

**Purpose:** Capture backdrop, apply image filter, render filtered backdrop + child

**API:**
```rust
pub struct BackdropFilterLayer {
    /// Child layer to render on top
    pub child: Option<Box<dyn Layer>>,
    /// Image filter to apply to backdrop
    pub filter: ImageFilter,
    /// Blend mode
    pub blend_mode: BlendMode,
    /// Bounds for filtering
    pub bounds: Rect,
}

impl Layer for BackdropFilterLayer {
    fn render(&self, renderer: &mut dyn CommandRenderer);
    fn bounds(&self) -> Rect;
}
```

**Rendering Flow:**
1. Capture current framebuffer content in bounds
2. Apply image filter (blur, etc.) to captured content
3. Render filtered backdrop
4. Render child layer on top

#### 3. PaintContext Extensions

**Purpose:** Allow RenderObjects to push specialized layers

**NEW API:**
```rust
impl PaintContext {
    /// Push a shader mask layer (for RenderShaderMask)
    pub fn push_shader_mask(
        &mut self,
        shader: ShaderSpec,
        blend_mode: BlendMode,
        paint_child: impl FnOnce(&mut PaintContext),
    );

    /// Push a backdrop filter layer (for RenderBackdropFilter)
    pub fn push_backdrop_filter(
        &mut self,
        filter: ImageFilter,
        blend_mode: BlendMode,
        paint_child: impl FnOnce(&mut PaintContext),
    );
}
```

#### 4. Updated RenderObjects

**Update existing implementations:**
```rust
// shader_mask.rs - UPDATED
impl RenderBox<Single> for RenderShaderMask {
    fn paint(&self, ctx: &mut PaintContext) {
        ctx.push_shader_mask(
            self.shader.clone(),
            self.blend_mode,
            |ctx| {
                let child_id = ctx.children.single();
                ctx.paint_child(child_id, ctx.offset);
            },
        );
    }
}

// backdrop_filter.rs - UPDATED
impl RenderBox<Single> for RenderBackdropFilter {
    fn paint(&self, ctx: &mut PaintContext) {
        ctx.push_backdrop_filter(
            self.filter.clone(),
            self.blend_mode,
            |ctx| {
                let child_id = ctx.children.single();
                ctx.paint_child(child_id, ctx.offset);
            },
        );
    }
}
```

### Out of Scope

- **Performance optimizations** beyond correctness (e.g., layer caching)
- **Additional layer types** (OpacityLayer, TransformLayer, ClipLayer) - FLUI's RenderObject approach works well for these
- **Software rendering fallback** - wgpu backend only (per project constraints)

## Relationship to Other Changes

- **Depends on:** `validate-effects-against-flutter` (validation identified these gaps)
- **Enables:** Full Flutter parity for visual effects
- **Related:** Future work on RepaintBoundary layer caching

## Impact

### Affected Systems

**flui_engine:**
- `src/layer/mod.rs` - Export new layer types
- `src/layer/shader_mask.rs` - NEW file
- `src/layer/backdrop_filter.rs` - NEW file
- `src/gpu_renderer.rs` - Add rendering support for new layers

**flui_rendering:**
- `src/objects/effects/shader_mask.rs` - Update paint() to use layer
- `src/objects/effects/backdrop_filter.rs` - Update paint() to use layer

**flui_painting:**
- `src/canvas.rs` or new `src/paint_context.rs` - Add push_shader_mask, push_backdrop_filter methods

### Breaking Changes

**None** - This is purely additive. Current code continues to work:
- ✅ Existing RenderObjects unchanged
- ✅ Existing layer system (CanvasLayer) unchanged
- ✅ Only RenderShaderMask and RenderBackdropFilter gain functionality

### Performance Impact

- **ShaderMask**: Requires offscreen texture allocation (~2-4 MB for 1080p)
- **BackdropFilter**: Requires framebuffer readback + filter pass (expensive)
- **Mitigation**: Use sparingly, document performance characteristics

### Testing Requirements

1. Unit tests for new layer types
2. Integration tests for shader masking (gradient fade, vignette)
3. Integration tests for backdrop filtering (blur, frosted glass)
4. Performance benchmarks for layer operations
5. Visual regression tests (compare against Flutter)

## Success Criteria

1. ✅ ShaderMaskLayer implemented and functional
2. ✅ BackdropFilterLayer implemented and functional
3. ✅ RenderShaderMask applies shader masks correctly
4. ✅ RenderBackdropFilter applies backdrop filters correctly
5. ✅ All tests passing: `cargo test -p flui_engine -p flui_rendering`
6. ✅ Visual output matches Flutter for equivalent widgets
7. ✅ Documentation updated with usage examples
8. ✅ Performance characteristics documented

## Risks & Mitigation

**Risk:** wgpu offscreen rendering complexity
- **Mitigation:** Reference wgpu examples for render-to-texture patterns
- **Mitigation:** Start with simple case (solid color mask) before gradients

**Risk:** Framebuffer readback performance (BackdropFilter)
- **Mitigation:** Document as expensive operation
- **Mitigation:** Consider async readback in future optimization

**Risk:** Shader compilation complexity
- **Mitigation:** Use wgsl shaders (wgpu native format)
- **Mitigation:** Pre-compile common shaders

## Alternatives Considered

### 1. Software Rendering (REJECTED)
- **Pros:** Simpler implementation, no GPU shader complexity
- **Cons:** Performance unacceptable, violates project constraint (GPU-only)

### 2. Canvas-Only Implementation (REJECTED)
- **Pros:** No new layer types needed
- **Cons:** Impossible - Canvas API cannot capture backdrop or render to texture

### 3. Add All Flutter Layers (REJECTED)
- **Pros:** Full Flutter parity
- **Cons:** Over-engineering - FLUI's RenderObject approach works well for most effects
- **Decision:** Only add layers when Canvas API is insufficient

### 4. Compositor Layer Support (CHOSEN)
- **Pros:** Enables advanced effects, minimal architecture change, Flutter-compatible
- **Cons:** Requires GPU programming, offscreen texture management
- **Decision:** Best balance of functionality and complexity

## Implementation Notes

See `tasks.md` for detailed implementation checklist and validation steps.

**Key Implementation Phases:**
1. **Phase 1:** ShaderMaskLayer (simpler - no backdrop access)
2. **Phase 2:** BackdropFilterLayer (complex - requires framebuffer capture)
3. **Phase 3:** RenderObject integration
4. **Phase 4:** Testing and documentation

**Estimated Effort:** 2-4 weeks
- Phase 1: 4-6 days
- Phase 2: 6-8 days
- Phase 3: 2-3 days
- Phase 4: 3-4 days (testing + docs)

---

## References

### Flutter Documentation
- [RenderShaderMask](https://api.flutter.dev/flutter/rendering/RenderShaderMask-class.html)
- [RenderBackdropFilter](https://api.flutter.dev/flutter/rendering/RenderBackdropFilter-class.html)
- [Layer Compositing](https://api.flutter.dev/flutter/rendering/Layer-class.html)

### Related Files
- Validation report: `openspec/changes/validate-effects-against-flutter/validation-report.md`
- Current implementations: `crates/flui_rendering/src/objects/effects/{shader_mask,backdrop_filter}.rs`
- Layer system: `crates/flui_engine/src/layer/`
