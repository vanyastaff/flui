# Migration Guide

This guide helps you migrate between different versions of `flui_painting`.

## Table of Contents

- [Migrating from 0.0.x to 0.1.x](#migrating-from-00x-to-01x)
- [API Changes](#api-changes)
- [Breaking Changes](#breaking-changes)

## Migrating from 0.0.x to 0.1.x

### Overview of Changes

Version 0.1.0 introduces several improvements to the API:

1. **Extension Traits** - DisplayList methods split into core and extension traits
2. **Sealed Traits** - DisplayListCore is now sealed for API stability
3. **Iterator Methods** - Added `iter()` and `iter_mut()` for IntoIterator convention
4. **Debug Trait** - Canvas now implements Debug
5. **Safe restore()** - No longer panics, now a no-op when called without save()

### Breaking Changes

#### 1. DisplayList Trait Split

**Before (0.0.x):**

```rust
// All methods on DisplayList directly
let display_list = canvas.finish();
for cmd in display_list.commands() {
    // ...
}
let stats = display_list.stats();
```

**After (0.1.x):**

```rust
use flui_painting::prelude::*; // Imports traits

let display_list = canvas.finish();
for cmd in display_list.commands() {
    // ...
}
let stats = display_list.stats();
```

**Action Required:**
- Import traits via prelude: `use flui_painting::prelude::*;`
- Or import specific traits: `use flui_painting::{DisplayListCore, DisplayListExt};`

#### 2. restore() No Longer Panics

**Before (0.0.x):**

```rust
canvas.restore(); // PANIC if no matching save()
```

**After (0.1.x):**

```rust
canvas.restore(); // No-op if no matching save()
```

**Action Required:**
- Remove defensive checks before restore()
- Tests expecting panic will fail - update them

**Migration Example:**

```rust
// Before - defensive code
if canvas.save_count() > 1 {
    canvas.restore();
}

// After - always safe
canvas.restore();
```

#### 3. Iteration Pattern

**Before (0.0.x):**

```rust
// Only via commands() method
for cmd in display_list.commands() {
    // ...
}
```

**After (0.1.x):**

```rust
// Both ways work
for cmd in display_list.commands() {
    // ...
}

// New: Direct iteration
for cmd in &display_list {
    // ...
}

// New: iter() method
for cmd in display_list.iter() {
    // ...
}
```

**Action Required:** None - old code still works.

### Non-Breaking Changes

These are additions that don't require code changes:

#### 1. New Iteration Methods

```rust
// New iter() method
let mut iter = display_list.iter();

// New iter_mut() method (for commands_mut)
for cmd in display_list.commands_mut() {
    cmd.apply_transform(matrix);
}
```

#### 2. Canvas Debug

```rust
// Can now debug print Canvas
let canvas = Canvas::new();
println!("{:?}", canvas); // Now works
```

#### 3. Improved Documentation

All types now have comprehensive documentation with examples.

## API Changes

### Added APIs

#### DisplayList

- `iter(&self) -> Iter<DrawCommand>` - Iterator over commands
- `iter_mut(&mut self) -> IterMut<DrawCommand>` - Mutable iterator

#### Canvas

- `impl Debug for Canvas` - Debug trait implementation

### Changed APIs

#### Canvas::restore()

- **Old behavior:** Panics if no saved state
- **New behavior:** No-op if no saved state
- **Reason:** Safer, matches common graphics API patterns

### Deprecated APIs

None in this release.

### Removed APIs

None in this release.

## Troubleshooting

### Trait Method Not Found

**Error:**

```
error[E0599]: no method named `draw_commands` found for struct `DisplayList`
```

**Solution:**

Import the extension trait:

```rust
use flui_painting::prelude::*;

// Or specifically:
use flui_painting::DisplayListExt;
```

### Test Panics on restore()

**Error:**

```
test test_canvas_restore_without_save ... FAILED
note: test did not panic as expected
```

**Solution:**

Update test to check for no-op behavior:

```rust
// Before
#[test]
#[should_panic(expected = "Canvas::restore() called without matching save()")]
fn test_canvas_restore_without_save() {
    let mut canvas = Canvas::new();
    canvas.restore();
}

// After
#[test]
fn test_canvas_restore_without_save() {
    let mut canvas = Canvas::new();
    canvas.restore(); // Should not panic

    // Verify canvas is still usable
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
    assert_eq!(canvas.len(), 1);
}
```

## Version Compatibility

| flui_painting | flui_types | Rust Version |
|---------------|------------|--------------|
| 0.1.x         | 0.1.x      | 1.70+        |
| 0.0.x         | 0.0.x      | 1.70+        |

## Need Help?

If you encounter issues during migration:

1. Check the [CHANGELOG](../CHANGELOG.md) for detailed changes
2. Review the [examples](../examples/) for updated patterns (when available)
3. Consult the [API documentation](https://docs.rs/flui_painting)
4. Open an issue on [GitHub](https://github.com/flui-org/flui/issues)
