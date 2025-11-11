# Phase 11: Remove Widget-Level Layers from Engine

## Scope Definition

**flui-engine** should contain ONLY low-level compositor primitives:
- Picture (drawing commands)
- Container (multi-child composition)
- Transform (geometric transforms)
- Opacity (alpha blending)
- Clipping (rect/rrect/oval/path)
- Blur/Filter (compositor effects)
- Backdrop filter (compositor effects)

**Should NOT be in engine** (belong in widgets/rendering):
- Event handling (pointer, scroll, gestures)
- Layout logic (offset, positioning)
- Pooling/caching (optimization, not core primitive)

---

## Files to DELETE

### 1. scrollable.rs (145 lines) - WIDGET LEVEL
**Status:** 🔴 DELETE

**Why:**
- Event handling logic (scroll callbacks)
- Should be implemented as RenderScrollView in flui_rendering
- Already exists: `flui_rendering/src/objects/render_scroll_view.rs`

**Migration:** Use RenderScrollView widget instead

---

### 2. pointer_listener_layer.rs (253 lines) - WIDGET LEVEL  
**Status:** 🔴 DELETE

**Why:**
- Event handling logic (pointer callbacks)
- Already exists: `flui_rendering/src/objects/interaction/pointer_listener.rs`
- Not a compositor primitive

**Migration:** Use RenderPointerListener instead

---

### 3. offset.rs (191 lines) - LAYOUT LOGIC
**Status:** 🔴 DELETE

**Why:**
- Layout logic, not compositor primitive
- Just wraps child with offset
- Should use Transform layer or widget layout

**Migration:** Use TransformLayer::translate() or RenderPositioned

---

### 4. pooled.rs + pool.rs (292 + 395 = 687 lines) - OPTIMIZATION
**Status:** ⚠️ EVALUATE

**Why DELETE:**
- Premature optimization
- Adds complexity without proven benefit
- Layer reuse should be higher level

**Why KEEP:**
- Performance optimization for layer reuse
- Used in some production code paths

**Decision:** DELETE (user requested "ненужный код", pooling is optimization)

---

### 5. handle.rs (165 lines) - UNCLEAR PURPOSE
**Status:** ⚠️ EVALUATE

**Read file to determine if needed**

---

## Files to KEEP (Core Compositor)

✅ **picture.rs** - Drawing commands (core)
✅ **container.rs** - Multi-child composition (core)  
✅ **transform.rs** - Geometric transforms (core)
✅ **opacity.rs** - Alpha blending (core)
✅ **clip_generic.rs** - Clipping (core)
✅ **blur.rs** - Blur effect (compositor)
✅ **filter.rs** - Image filters (compositor)
✅ **backdrop_filter.rs** - Backdrop blur (compositor)
✅ **base.rs** - Layer trait (core)
✅ **base_multi_child.rs** - Multi-child base (core)
✅ **base_single_child.rs** - Single-child base (core)

---

## Summary

**Lines to delete:** ~1,436 lines
- scrollable.rs: 145
- pointer_listener_layer.rs: 253  
- offset.rs: 191
- pooled.rs: 292
- pool.rs: 395
- handle.rs: 165 (if not needed)

**Impact:** Clean engine scope, widget layers moved to proper location
