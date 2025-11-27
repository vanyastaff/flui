# Change: Refactor Canvas API Usage in flui_rendering

## Why

The `flui_painting` crate recently received significant Canvas API improvements:
- **Chaining API**: Methods like `saved()`, `translated()`, `rotated()`, `rect()` that return `&mut Self`
- **Convenience shapes**: `draw_rounded_rect()`, `draw_pill()`, `draw_ring()`
- **Scoped operations**: `with_save()`, `with_translate()`, `with_clip_rect()`
- **Batch drawing**: `draw_rects()`, `draw_circles()`, `draw_lines()`
- **Debug helpers**: `debug_rect()`, `debug_point()`, `debug_grid()`

Currently, `flui_rendering` RenderObjects use the old verbose patterns:
```rust
// Old pattern (verbose)
canvas.save();
canvas.translate(x, y);
canvas.clip_rect(rect);
// ... drawing ...
canvas.restore();
```

This can be modernized to:
```rust
// New pattern (concise)
canvas.with_translate(x, y, |c| {
    c.clip_rect(rect);
    // ... drawing ...
});

// Or chaining
canvas.saved().translated(x, y).clipped_rect(rect);
```

**Benefits:**
- **Readability**: More expressive, fluent code
- **Safety**: `with_*` methods guarantee restore even on early returns
- **Consistency**: Unified patterns across all RenderObjects
- **Maintainability**: Less boilerplate, easier to understand

## What Changes

Systematically review and update all RenderObjects in `flui_rendering` to use new Canvas API:

1. **Replace save/restore with scoped operations** - Use `with_save()`, `with_translate()` where possible
2. **Use chaining API** - Replace sequential calls with chained methods
3. **Use convenience shapes** - Replace manual RRect construction with `draw_rounded_rect()`, `draw_pill()`
4. **Use batch drawing** - Replace loops with `draw_rects()`, `draw_circles()` where applicable
5. **Add debug helpers** - Use `debug_rect()`, `debug_grid()` in debug rendering

**Affected Directories:**
- `crates/flui_rendering/src/objects/effects/` - All effect RenderObjects
- `crates/flui_rendering/src/objects/layout/` - Layout objects with custom painting
- `crates/flui_rendering/src/objects/media/` - Image, texture rendering
- `crates/flui_rendering/src/objects/sliver/` - Sliver painting
- `crates/flui_rendering/src/objects/debug/` - Debug overlays
- `crates/flui_rendering/src/objects/viewport/` - Viewport rendering

**Non-Goals:**
- No API changes to RenderObject traits
- No new functionality - purely code quality improvement
- No changes to flui_painting itself

## Impact

- **Affected specs**: flui-rendering (internal refactor only)
- **Affected code**: ~40 files in `crates/flui_rendering/src/objects/`
- **Risk**: LOW - internal implementation changes, no API changes
- **Testing**: Existing tests should pass unchanged

## Scenarios

### Scenario: Refactor save/restore to chaining API

**GIVEN** a RenderObject using manual save()/restore() pattern
**WHEN** the transform is simple (translate, rotate, scale)
**THEN** code SHALL use chaining API with saved()/restored()
**AND** transforms SHALL use translated(), rotated(), scaled_xy() methods
**AND** code SHALL be more concise and readable

**Example:**
```rust
// Before
canvas.save();
canvas.translate(x, y);
canvas.draw_rect(rect, &paint);
canvas.restore();

// After
canvas.saved().translated(x, y).rect(rect, &paint).restored();
```

### Scenario: Refactor conditional transforms consistently

**GIVEN** a RenderObject with conditional transforms (multiple branches)
**WHEN** some branches use chaining and others use old API
**THEN** all branches SHALL use consistent API (chaining)
**AND** saved() SHALL be used in all transform branches
**AND** restored() SHALL be called once at the end

**Example (RenderImage):**
```rust
// Before - inconsistent
if should_flip {
    canvas.saved().translated(...).scaled_xy(...);
} else {
    canvas.save();  // ❌ Inconsistent
}

// After - consistent
if should_flip {
    canvas.saved().translated(...).scaled_xy(...);
} else {
    canvas.saved();  // ✅ Consistent
}
```

### Scenario: Use convenience shape methods

**GIVEN** a RenderObject manually constructing RRect for rounded rectangles
**WHEN** the rectangle uses uniform corner radius
**THEN** code SHALL use draw_rounded_rect() convenience method
**AND** code SHALL be more concise

**Example:**
```rust
// Before
let rrect = RRect::from_rect_circular(rect, radius);
canvas.draw_rrect(rrect, &paint);

// After
canvas.draw_rounded_rect(rect, radius, &paint);
```

### Scenario: Replace batch drawing loops

**GIVEN** a RenderObject with loop drawing multiple similar shapes
**WHEN** shapes are rects, circles, or lines
**THEN** code SHALL use batch drawing methods
**AND** code SHALL use draw_rects(), draw_circles(), or draw_lines()
**AND** performance SHALL improve (single call vs N calls)

**Example:**
```rust
// Before
for rect in rects {
    canvas.draw_rect(rect, &paint);
}

// After
canvas.draw_rects(&rects, &paint);
```

### Scenario: Validation - all tests pass

**GIVEN** Canvas API refactoring is complete
**WHEN** running test suite
**THEN** all existing tests SHALL pass unchanged
**AND** no new test failures SHALL occur
**AND** visual output SHALL be identical to before refactoring

### Scenario: Validation - no clippy warnings

**GIVEN** Canvas API refactoring is complete
**WHEN** running clippy with -D warnings
**THEN** no clippy warnings SHALL be generated
**AND** code quality SHALL be maintained or improved

## Success Criteria

- [ ] All RenderObjects reviewed for Canvas API usage
- [ ] Old verbose patterns replaced with new concise patterns
- [ ] Consistent API usage across all objects
- [ ] All scenarios validated
- [ ] All tests pass
- [ ] No new clippy warnings
- [ ] Code is more readable and maintainable
