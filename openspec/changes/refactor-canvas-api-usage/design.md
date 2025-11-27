# Design: Refactor Canvas API Usage

## Overview

This change systematically refactors all RenderObjects in `flui_rendering` to use the modernized Canvas API from `flui_painting`. The refactoring improves code readability, safety, and maintainability without changing any external APIs or functionality.

## Architecture

### Old Pattern (Verbose)

```rust
// Manual save/restore - error prone
canvas.save();
canvas.translate(x, y);
canvas.clip_rect(rect);
// ... drawing ...
canvas.restore();  // Easy to forget, or skip on early return
```

### New Patterns

#### 1. Scoped Operations (`with_*` methods)

**Use Case:** When you need automatic cleanup

```rust
// Safe - restore is guaranteed
canvas.with_translate(x, y, |c| {
    c.clip_rect(rect);
    // ... drawing ...
});  // Automatic restore
```

**Benefits:**
- ✅ Automatic cleanup even on early returns or panics
- ✅ Clear scope boundaries
- ✅ No forgotten `restore()` calls
- ✅ Return values from closure

**Limitations:**
- Cannot use with multiple mutable borrows (ctx.canvas() and ctx methods)
- Slightly more verbose for simple transforms

#### 2. Chaining API

**Use Case:** When you need fluent, readable code

```rust
// Fluent and concise
canvas
    .saved()
    .translated(100.0, 50.0)
    .rotated(PI / 4.0)
    .clipped_rect(viewport)
    .rect(rect, &paint)
    .restored();
```

**Benefits:**
- ✅ Very concise and readable
- ✅ Method chaining reduces boilerplate
- ✅ Works with multiple borrows
- ✅ Easy to see transform pipeline

**Limitations:**
- Manual `restored()` call required
- Can be harder to debug middle of chain

#### 3. Conditional Operations

**Use Case:** Optional drawing based on runtime conditions

```rust
// Only draw if condition is true
canvas
    .when(show_border, |c| {
        c.rect(border_rect, &border_paint);
    })
    .when_else(is_selected,
        |c| c.rect(rect, &selected_paint),
        |c| c.rect(rect, &normal_paint),
    );
```

#### 4. Convenience Shapes

**Use Case:** Common shape patterns

```rust
// Old - manual RRect construction
let rrect = RRect::from_rect_circular(rect, radius);
canvas.draw_rrect(rrect, &paint);

// New - direct convenience method
canvas.draw_rounded_rect(rect, radius, &paint);
```

## Decision Matrix

### When to Use Scoped Operations

✅ **USE `with_*` when:**
- You need guaranteed cleanup (critical transforms)
- You want to return a value from the scoped operation
- Early returns are possible within the scope
- You need maximum safety

❌ **DON'T USE when:**
- You need to access both `ctx.canvas()` and `ctx` methods (borrow issues)
- Simple transforms where chaining is clearer

### When to Use Chaining API

✅ **USE chaining when:**
- You have multiple transforms to apply
- You need to access both canvas and ctx
- Code readability benefits from fluent style
- Transforms are simple and straightforward

❌ **DON'T USE when:**
- Risk of forgetting `restored()` is high
- Scope boundaries need to be very clear

### When to Use Conditional Operations

✅ **USE `when`/`when_else` when:**
- Drawing is conditional on runtime flags
- You want declarative conditional rendering
- Conditions are simple boolean expressions

❌ **DON'T USE when:**
- Complex conditional logic that would be clearer with `if`
- Multiple nested conditions (prefer explicit `if`)

## Refactoring Patterns by Use Case

### Pattern 1: Simple Transform

**Before:**
```rust
canvas.save();
canvas.translate(x, y);
canvas.draw_rect(rect, &paint);
canvas.restore();
```

**After (Chaining):**
```rust
canvas
    .saved()
    .translated(x, y)
    .rect(rect, &paint)
    .restored();
```

### Pattern 2: Transform + Clip

**Before:**
```rust
canvas.save();
canvas.translate(offset.dx, offset.dy);
canvas.clip_rect(bounds);
// ... drawing ...
canvas.restore();
```

**After (Chaining):**
```rust
canvas
    .saved()
    .translated(offset.dx, offset.dy)
    .clipped_rect(bounds);
// ... drawing ...
canvas.restored();
```

### Pattern 3: Conditional Transform

**Before:**
```rust
if needs_transform {
    canvas.save();
    canvas.translate(x, y);
}
// ... drawing ...
if needs_transform {
    canvas.restore();
}
```

**After (Conditional + Chaining):**
```rust
canvas.when(needs_transform, |c| c.saved().translated(x, y));
// ... drawing ...
canvas.when(needs_transform, |c| c.restored());
```

Or better - combine both:
```rust
if needs_transform {
    canvas.saved().translated(x, y);
}
// ... drawing ...
if needs_transform {
    canvas.restored();
}
```

### Pattern 4: Multiple Transforms (image.rs case)

**Before:**
```rust
if should_flip {
    canvas.save();
    canvas.translate(center_x, 0.0);
    canvas.scale_xy(-1.0, 1.0);
    canvas.translate(-center_x, 0.0);
} else {
    canvas.save();  // Still need save for other transform
}
// ... drawing ...
canvas.restore();
```

**After (Chaining for consistency):**
```rust
if should_flip {
    canvas
        .saved()
        .translated(center_x, 0.0)
        .scaled_xy(-1.0, 1.0)
        .translated(-center_x, 0.0);
} else {
    canvas.saved();  // Consistent API
}
// ... drawing ...
canvas.restored();
```

### Pattern 5: Rounded Rectangles

**Before:**
```rust
let rrect = RRect::from_rect_circular(rect, radius);
canvas.draw_rrect(rrect, &paint);
```

**After:**
```rust
canvas.draw_rounded_rect(rect, radius, &paint);
```

### Pattern 6: Batch Drawing

**Before:**
```rust
for rect in rects {
    canvas.draw_rect(rect, &paint);
}
```

**After:**
```rust
canvas.draw_rects(&rects, &paint);
```

## Implementation Strategy

### Phase 1: Effects Objects
- Most canvas usage concentration
- Critical rendering paths
- Focus on save/restore patterns

### Phase 2-7: Remaining Objects
- Apply patterns learned in Phase 1
- Ensure consistency across all objects
- Document any special cases

### Phase 8: Validation
- Build and test all changes
- Verify no regressions
- Check code consistency

## Testing Strategy

### No New Tests Required

This is a pure refactoring - existing tests should pass unchanged:

✅ **Unit tests** - Verify behavior unchanged
✅ **Integration tests** - Ensure no visual regressions
✅ **Compilation** - All code compiles without warnings

### Validation Checklist

- [ ] `cargo build -p flui_rendering` succeeds
- [ ] `cargo test -p flui_rendering` passes
- [ ] `cargo clippy -p flui_rendering -- -D warnings` clean
- [ ] Code review for consistency
- [ ] All save/restore pairs converted
- [ ] All convenience methods applied

## Migration Examples

### Example 1: RenderImage (completed)

**Problem:** Inconsistent API usage - chaining for flip, but old save() for invert

**Solution:** Unified chaining API for all transform cases

```rust
// Consistent usage for both branches
if should_flip_horizontally() {
    canvas.saved().translated(...).scaled_xy(...).translated(...);
} else {
    canvas.saved();  // Consistent with flip branch
}
// ... drawing ...
canvas.restored();
```

### Example 2: RenderClipRect (already done via clip_base)

**Pattern:** All clip objects use consistent base implementation

### Example 3: RenderDecoratedBox (already done)

**Pattern:** Used `draw_rounded_rect()` convenience method

## Success Criteria

✅ All RenderObjects reviewed
✅ Consistent API patterns applied
✅ save/restore replaced with chaining or scoped operations
✅ Convenience methods used where applicable
✅ All tests pass
✅ No clippy warnings
✅ Code is more readable and maintainable

## Future Enhancements

### Potential Canvas API Additions

1. **Batch transform operations**
   ```rust
   canvas.with_transforms([
       Transform::translate(x, y),
       Transform::rotate(angle),
       Transform::scale(factor),
   ], |c| { /* ... */ });
   ```

2. **Transform builder**
   ```rust
   canvas.transform_builder()
       .translate(x, y)
       .rotate(angle)
       .apply(|c| { /* ... */ });
   ```

3. **Debug mode helpers**
   ```rust
   canvas.debug_when(cfg!(debug_assertions), |c| {
       c.debug_rect(bounds, Color::RED);
   });
   ```

These are **not** part of this change - just ideas for future canvas API evolution.
