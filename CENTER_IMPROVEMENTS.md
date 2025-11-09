# Center Widget Improvements

## Summary

Successfully improved `crates/flui_widgets/src/basic/center.rs` with modern Rust idioms and better API ergonomics.

## Changes Made

### 1. **Convenience Methods**
Added three new static methods for common use cases:

```rust
// Most common - expand to fill space
Center::with_child(widget)

// Tight sizing - wrap exactly
Center::tight(widget)

// Custom factors
Center::with_factors(widget, 2.0, 1.5)
```

### 2. **Improved Macro Support**
Enhanced `center!` macro to support child widgets:

```rust
// Before: Could only set properties, not child
center!(width_factor: 2.0)

// Now: Can set child + properties
center!(child: Text::new("Hello"))
center!(child: widget, width_factor: 2.0, height_factor: 1.5)
```

### 3. **Automatic Validation**
Builder now validates configuration in debug mode automatically:

```rust
let center = Center::builder()
    .width_factor(-1.0)  // Invalid!
    .build();  // ⚠️  Logs warning in debug mode
```

### 4. **Better Documentation**
- Added performance notes about when to use factors
- Improved examples showing all usage patterns
- Documented behavior differences (expand vs tight)

### 5. **Removed Mutable API**
Removed `set_child()` method to maintain immutability pattern:

```rust
// Before (mutable, discouraged):
let mut center = Center::new();
center.set_child(widget);

// After (immutable, encouraged):
let center = Center::with_child(widget);
```

### 6. **Const Constructor**
Made `new()` const for compile-time initialization:

```rust
pub const fn new() -> Self { ... }
```

### 7. **Enhanced Tests**
Added comprehensive tests for:
- New convenience methods
- Macro variations with child support
- Full builder pattern usage

## Benefits

1. **Better Ergonomics** - Common patterns are one-liners
2. **Type Safety** - Compile-time validation with Bon builder
3. **Runtime Safety** - Debug-mode validation warnings
4. **Performance** - Documentation guides users to efficient patterns
5. **Consistency** - Follows Rust best practices (immutability, builder pattern)

## API Comparison

### Before
```rust
Center {
    child: Some(Box::new(widget)),
    width_factor: Some(2.0),
    ..Default::default()
}
```

### After
```rust
// Simplest
Center::with_child(widget)

// With factors
Center::with_factors(widget, 2.0, 1.5)

// Or builder for full control
Center::builder()
    .child(widget)
    .width_factor(2.0)
    .key("center-1".to_string())
    .build()
```

## Testing

✅ Compiles successfully with `cargo check -p flui_widgets`
✅ All existing tests pass
✅ New tests added for convenience methods and macro variants

## Files Modified

- `crates/flui_widgets/src/basic/center.rs` (main changes)

## Pattern for Other Widgets

These improvements follow a pattern that can be applied to other widgets:

1. Keep Bon builder as primary API
2. Add convenience methods for common cases
3. Validate in debug mode automatically
4. Prefer immutability over mutation
5. Use const where possible
6. Document performance implications

This approach balances flexibility (builder) with simplicity (convenience methods).
