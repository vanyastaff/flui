# Container Widget - Future Enhancements

## 3 Syntax Styles (TODO for future session)

### 1. ✅ Struct Literal Syntax (Already works)
```rust
Container {
    width: Some(100.0),
    height: Some(200.0),
    padding: Some(EdgeInsets::all(16.0)),
    color: Some(Color::rgb(255, 0, 0)),
    ..Default::default()
}
```

### 2. 🔄 Builder Pattern (with bon derive)
```rust
Container::builder()
    .width(100.0)
    .height(200.0)
    .padding(EdgeInsets::all(16.0))
    .color(Color::rgb(255, 0, 0))
    .child(child_widget)
    .build()
```

**Reference:** See `old_container.txt` lines 76-435 for bon implementation:
- `#[derive(Builder)]` with custom setters
- `.child()` smart setter
- `.ui()` finishing function
- `.try_build()` with validation

### 3. 🔄 Declarative Macro
```rust
container! {
    width: 100.0,
    height: 200.0,
    padding: EdgeInsets::all(16.0),
    color: Color::rgb(255, 0, 0),
    child: child_widget,
}
```

## Useful Methods from Old Code

Before implementing, check if already available in:
- `flui_types::EdgeInsets` - check for `horizontal_total()`, `vertical_total()`
- `flui_core::BoxConstraints` - check for constraint merging
- `flui_types::Alignment` - check for conversion methods

### From old_container.txt to consider:

1. **calculate_size()** (lines 242-277)
   - Smart size calculation with constraints
   - Check if BoxConstraints already has this

2. **validate()** (lines 293-341)
   - Validates conflicting constraints
   - Could be useful for debug builds

3. **Factory methods:**
   - `colored(color)` - line 192
   - `bordered(width, color)` - line 209
   - `rounded(color, radius)` - line 231
   - These are nice shortcuts!

4. **get_decoration()** (lines 280-290)
   - Merges `color` shorthand into `decoration`
   - Useful pattern

5. **Transform support** (lines 140-144, 472-482)
   - `transform: Option<Transform>`
   - `transform_alignment: Option<Alignment>`
   - Could add later when needed

6. **Validation helpers:**
   - Check min/max conflicts
   - Check NaN/infinite values
   - Good for development

## Next Steps

1. **Current session:** Focus on Row & Column widgets
2. **Future session:** Come back to Container enhancements
3. **Before adding methods:** Always check existing API in flui_types/flui_core first!

## References

- Old implementation: `old_container.txt`
- bon crate docs: https://docs.rs/bon/
- Current Container: `crates/flui_widgets/src/container.rs`
