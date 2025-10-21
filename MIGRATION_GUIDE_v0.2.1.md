# Migration Guide: v0.2.0 → v0.2.1

## API Naming Improvements (Rust API Guidelines Compliance)

This release improves naming consistency to fully comply with Rust API Guidelines (RFC 199).

### Summary of Changes

All changes are focused on removing the `get_` prefix from methods where it's not idiomatic per [Rust API Guidelines C-GETTER](https://rust-lang.github.io/api-guidelines/naming.html#c-getter).

---

## 1. Intrinsic Size Methods (DynRenderObject trait)

**Location:** `flui_core::render::DynRenderObject`

### Breaking Changes

| Old Method Name | New Method Name | Status |
|----------------|-----------------|--------|
| `get_min_intrinsic_width(height: f32)` | `min_intrinsic_width(height: f32)` | ✅ Renamed |
| `get_max_intrinsic_width(height: f32)` | `max_intrinsic_width(height: f32)` | ✅ Renamed |
| `get_min_intrinsic_height(width: f32)` | `min_intrinsic_height(width: f32)` | ✅ Renamed |
| `get_max_intrinsic_height(width: f32)` | `max_intrinsic_height(width: f32)` | ✅ Renamed |

### Migration Example

```rust
// Before
impl DynRenderObject for MyRenderObject {
    fn get_min_intrinsic_width(&self, height: f32) -> f32 {
        100.0
    }

    fn get_max_intrinsic_width(&self, height: f32) -> f32 {
        200.0
    }
}

// After
impl DynRenderObject for MyRenderObject {
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        100.0
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        200.0
    }
}
```

**Search & Replace:** Use your editor to find and replace:
- `get_min_intrinsic_width` → `min_intrinsic_width`
- `get_max_intrinsic_width` → `max_intrinsic_width`
- `get_min_intrinsic_height` → `min_intrinsic_height`
- `get_max_intrinsic_height` → `max_intrinsic_height`

---

## 2. Layout Cache Global Function

**Location:** `flui_core::cache`

### Breaking Changes

| Old Function Name | New Function Name | Status |
|------------------|-------------------|--------|
| `get_layout_cache()` | `layout_cache()` | ✅ Renamed |

Other cache functions remain unchanged:
- ✅ `invalidate_layout(element_id)` - No change
- ✅ `clear_layout_cache()` - No change

### Migration Example

```rust
// Before
use flui_core::cache::get_layout_cache;

let cache = get_layout_cache();
let stats = get_layout_cache().stats();

// After
use flui_core::cache::layout_cache;

let cache = layout_cache();
let stats = layout_cache().stats();
```

**Search & Replace:**
- `get_layout_cache` → `layout_cache`

---

## 3. Inherited Widget Methods (No Changes Required)

**Location:** `flui_core::context::Context`

### Status: No Breaking Changes

The following methods retain their names for backward compatibility:

- ✅ `get_inherited_widget<W>()` - **Kept** (legacy API for macro compatibility)
- ✅ `get_inherited_widget_of_exact_type<T>()` - Already deprecated since v0.2.0
- ✅ `get_element_for_inherited_widget_of_exact_type<W>()` - Already deprecated since v0.2.0

**Recommendation:** Use the modern API instead:
- `context.read::<Theme>()` - Read without dependency
- `context.inherit::<Theme>()` - Read with dependency
- `context.watch::<Theme>()` - React-style alias for inherit

```rust
// Legacy (still supported, but not recommended)
let theme = context.get_inherited_widget::<Theme>();

// Recommended (modern API)
let theme = context.read::<Theme>();
let theme = context.inherit::<Theme>();
```

---

## Automated Migration

### Using sed (Unix/Linux/macOS)

```bash
# Navigate to your project root
cd /path/to/your/project

# Backup your code first!
git commit -am "backup before migration"

# Apply automatic replacements
find . -name "*.rs" -type f -exec sed -i '' \
  -e 's/get_min_intrinsic_width/min_intrinsic_width/g' \
  -e 's/get_max_intrinsic_width/max_intrinsic_width/g' \
  -e 's/get_min_intrinsic_height/min_intrinsic_height/g' \
  -e 's/get_max_intrinsic_height/max_intrinsic_height/g' \
  -e 's/get_layout_cache/layout_cache/g' \
  {} +

# Verify changes
cargo check

# If successful
git commit -am "migrate to flui v0.2.1 naming conventions"
```

### Using ripgrep + sd (Cross-platform)

```bash
# Install tools if needed
# cargo install sd

# Intrinsic size methods
rg -l 'get_min_intrinsic_width' | xargs sd 'get_min_intrinsic_width' 'min_intrinsic_width'
rg -l 'get_max_intrinsic_width' | xargs sd 'get_max_intrinsic_width' 'max_intrinsic_width'
rg -l 'get_min_intrinsic_height' | xargs sd 'get_min_intrinsic_height' 'min_intrinsic_height'
rg -l 'get_max_intrinsic_height' | xargs sd 'get_max_intrinsic_height' 'max_intrinsic_height'

# Layout cache
rg -l 'get_layout_cache' | xargs sd 'get_layout_cache' 'layout_cache'

# Verify
cargo check
```

---

## Impact Analysis

### Who is affected?

1. **Users implementing custom `RenderObject` traits** - If you override intrinsic size methods
2. **Users calling `get_layout_cache()` directly** - Update to `layout_cache()`
3. **Most widget users** - ✅ No changes needed (high-level API unchanged)

### Compilation Errors You Might See

```
error[E0599]: no function or associated item named `get_min_intrinsic_width` found for type `MyRenderObject`
  --> src/my_render_object.rs:42:5
   |
42 |     fn get_min_intrinsic_width(&self, height: f32) -> f32 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: items from traits can only be used if the trait is implemented and in scope
   = note: the following trait defines an item `min_intrinsic_width`, perhaps you need to implement it:
           candidate #1: `DynRenderObject`
```

**Fix:** Rename the method to `min_intrinsic_width`.

---

## Rationale

### Why these changes?

These changes improve compliance with [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html#c-getter):

> **C-GETTER**: Methods that return the value of a field should not use the `get_` prefix.

**Examples from Rust stdlib:**
- ✅ `vec.len()` not `vec.get_len()`
- ✅ `string.capacity()` not `string.get_capacity()`
- ✅ `path.parent()` not `path.get_parent()`

**Our changes:**
- ✅ `min_intrinsic_width(h)` not `get_min_intrinsic_width(h)` - Computes a value
- ✅ `layout_cache()` not `get_layout_cache()` - Returns a reference

### Benefits

1. **Consistency** - Aligns with Rust ecosystem conventions
2. **Readability** - Shorter, clearer names
3. **Tooling** - Better clippy lint compliance
4. **Learning** - Easier for Rust developers to understand

---

## Testing Your Migration

After migration, verify everything works:

```bash
# Run tests
cargo test

# Check for warnings
cargo clippy -- -D warnings

# Format code
cargo fmt

# Build release
cargo build --release
```

---

## Need Help?

- **Documentation:** https://docs.rs/flui_core
- **Examples:** `flui/crates/flui_core/examples/`
- **Issues:** https://github.com/yourusername/flui/issues
- **API Guidelines:** https://rust-lang.github.io/api-guidelines/

---

## Summary

✅ **3 breaking changes** (method/function renames)
✅ **Simple migration** (find & replace)
✅ **Improved compliance** with Rust API Guidelines
✅ **Better developer experience**

Most projects can migrate in **< 5 minutes** using automated find & replace.

---

*Last updated: 2025-10-21*
*Flui version: 0.2.1*
