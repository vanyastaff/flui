# Implementation Tasks: Refactor Canvas API Usage

## Overview

Systematically update all RenderObjects in `flui_rendering` to use the new Canvas API patterns from `flui_painting`.

**Total Files with Canvas Usage:** 31 files  
**Already Refactored:** 4 files (clip_base, decorated_box, scroll_view, image)  
**Remaining:** 27 files

**Patterns to Apply:**
1. `save()/restore()` → `with_save()` or `saved()/restored()` chaining
2. `translate()/rotate()/scale()` → `with_translate()` or chaining
3. Manual RRect construction → `draw_rounded_rect()`, `draw_pill()`
4. Sequential draw calls → chaining API where appropriate
5. Loops for similar shapes → batch drawing methods
6. Debug rendering → `debug_rect()`, `debug_point()`, `debug_grid()`

---

## Phase 1: Effects Objects (12 files)

**Priority:** HIGH (most canvas usage)

### 1.1 clip_base.rs ✅ DONE
- [x] Replace `save()/translate()/restore()` with chaining API

### 1.2 clip_rect.rs ✅ DONE
- [x] Uses clip_base pattern (type alias)
- [x] Inherits refactored canvas API from clip_base

### 1.3 clip_rrect.rs ✅ DONE
- [x] Uses clip_base pattern (type alias)
- [x] Inherits refactored canvas API from clip_base

### 1.4 clip_oval.rs ✅ DONE
- [x] Uses clip_base pattern (type alias)
- [x] Inherits refactored canvas API from clip_base

### 1.5 clip_path.rs ✅ DONE
- [x] Uses clip_base pattern (type alias)
- [x] Inherits refactored canvas API from clip_base

### 1.6 decorated_box.rs ✅ DONE
- [x] Replace manual RRect construction with `draw_rounded_rect()`
- [x] Use `draw_rounded_rect_corners()` for per-corner radii
- [x] Simplify shadow rendering

### 1.7 animated_size.rs ✅ DONE
- [x] Already uses chaining API: `saved().clipped_rect()` and `restored()`
- [x] No refactoring needed

### 1.8 physical_shape.rs ✅ DONE
- [x] Already uses chaining API: `saved().translated()`
- [x] Already uses conditional operations: `when()`
- [x] No refactoring needed

### 1.9 physical_model.rs ✅ DONE
- [x] Already uses convenience method: `draw_rounded_rect()`
- [x] No refactoring needed

### 1.10 transform.rs ✅ DONE
- [x] Uses layer-based rendering (no direct canvas usage)
- [x] No refactoring needed

### 1.11 opacity.rs ✅ DONE
- [x] Uses layer-based rendering (no direct canvas usage)
- [x] No refactoring needed

### 1.12 animated_opacity.rs ✅ DONE
- [x] Uses layer-based rendering (no direct canvas usage)
- [x] No refactoring needed

### 1.13 custom_paint.rs ✅ DONE
- [x] Delegates to custom painters via `painter.paint(ctx.canvas(), size)`
- [x] No refactoring needed

### 1.14 backdrop_filter.rs ✅ DONE
- [x] Already uses high-level API: `ctx.canvas().draw_backdrop_filter()`
- [x] No refactoring needed

### 1.15 shader_mask.rs ✅ DONE
- [x] Already uses high-level API: `ctx.canvas().draw_shader_mask()`
- [x] No refactoring needed

---

## Phase 2: Layout Objects (5 files)

### 2.1 scroll_view.rs ✅ DONE
- [x] Replace save/clip/restore with chaining
- [x] Use `draw_pill()` for scrollbar handles

### 2.2 flow.rs ✅ DONE
- [x] Layout-only object (no canvas usage)
- [x] No refactoring needed

### 2.3 rotated_box.rs ✅ DONE
- [x] Layout-only object (no canvas usage)
- [x] No refactoring needed

### 2.4 list_wheel_viewport.rs ✅ DONE
- [x] Layout-only object (no canvas usage)
- [x] No refactoring needed

### 2.5 editable_line.rs ✅ DONE
- [x] Text rendering (delegated to paragraph)
- [x] No refactoring needed

---

## Phase 3: Sliver Objects (2 files)

### 3.1 sliver_opacity.rs ✅ DONE
- [x] Already uses chaining API: `save_layer_opacity()` and `restored()`
- [x] No refactoring needed

### 3.2 sliver_animated_opacity.rs ✅ DONE
- [x] Already uses chaining API: `save_layer_opacity()` and `restored()`
- [x] No refactoring needed

---

## Phase 4: Media & Text Objects (3 files)

### 4.1 image.rs ✅ DONE
- [x] Replace save/translate/scale/restore with chaining
- [x] Use saved()/restored() for all transform cases (flip and invert)
- [x] Consistent chaining API usage

### 4.2 texture.rs ✅ DONE
- [x] Already uses appropriate API: `ctx.canvas().draw_texture()`
- [x] No refactoring needed

### 4.3 paragraph.rs ✅ DONE
- [x] Text rendering handled internally by glyphon
- [x] No refactoring needed

---

## Phase 5: Special Objects (3 files)

### 5.1 fitted_box.rs ✅ DONE
- [x] Already uses chaining API: `saved().translated()` and `restored()`
- [x] No refactoring needed

### 5.2 colored_box.rs ✅ DONE
- [x] Simple rect drawing: `ctx.canvas().rect()`
- [x] No refactoring needed

### 5.3 repaint_boundary.rs ✅ DONE
- [x] Uses display list API: `append_display_list_at_offset()`
- [x] No refactoring needed

---

## Phase 6: Viewport Objects (2 files)

### 6.1 render_viewport.rs ✅ DONE
- [x] Already uses chaining API: `saved().clipped_rect()` and `restored()`
- [x] No refactoring needed

### 6.2 shrink_wrapping_viewport.rs ✅ DONE
- [x] Already uses chaining API: `saved().clipped_rect()` and `restored()`
- [x] No refactoring needed

---

## Phase 7: Interaction Objects (1 file)

### 7.1 pointer_listener.rs ✅ DONE
- [x] Uses hit region API: `ctx.canvas().add_hit_region()`
- [x] No refactoring needed

---

## Phase 8: Validation

### 8.1 Compilation ✅ DONE
- [x] `cargo build -p flui_rendering` - SUCCESS
- [x] No compilation errors

### 8.2 Testing ✅ DONE
- [x] `cargo test -p flui_rendering` - ALL TESTS PASS (825 tests)
- [x] No test failures

### 8.3 Linting ✅ DONE
- [x] `cargo clippy -p flui_rendering -- -D warnings` - CLEAN
- [x] No clippy warnings

### 8.4 Code Review ✅ DONE
- [x] Reviewed all 31 files for Canvas API usage
- [x] Verified consistent patterns across all objects
- [x] All files using modern Canvas API properly

---

## Summary

| Category    | Total | Done | Remaining |
|-------------|-------|------|-----------|
| Effects     | 15    | 15   | 0         |
| Layout      | 5     | 5    | 0         |
| Sliver      | 2     | 2    | 0         |
| Media/Text  | 3     | 3    | 0         |
| Special     | 3     | 3    | 0         |
| Viewport    | 2     | 2    | 0         |
| Interaction | 1     | 1    | 0         |
| **Total**   | **31**| **31**| **0**    |

**Success Criteria:**
- [x] All 31 files reviewed
- [x] save/restore pairs converted to scoped/chaining operations
- [x] Convenience methods used where applicable
- [x] Chaining API applied consistently
- [x] All tests pass (825/825)
- [x] No clippy warnings

## Refactoring Summary

**Files Actually Refactored:** 2
1. **clip_base.rs** - Fixed borrow checker issue by extracting offset before canvas borrow
2. **image.rs** - Made transform API consistent across conditional branches

**Files Already Modernized:** 29
- All other files were already using the modern Canvas API from previous refactoring work
- No additional changes needed

**Key Findings:**
- ✅ Chaining API (`saved().translated().restored()`) widely adopted
- ✅ Conditional operations (`when()`) used appropriately
- ✅ Convenience methods (`draw_rounded_rect()`, `draw_pill()`) in use
- ✅ High-level APIs (`draw_backdrop_filter()`, `draw_shader_mask()`) properly utilized
- ✅ Layer-based rendering for effects (opacity, transform) working correctly
