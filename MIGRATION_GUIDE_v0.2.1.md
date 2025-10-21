# Migration Guide: v0.2.0 → v0.3.0

## MAJOR BREAKING CHANGES - Trait Object Renaming

This release includes **major breaking changes** to achieve full compliance with Rust API Guidelines (RFC 199).

### Summary of Changes

This release includes two categories of breaking changes:

1. **🔴 MAJOR: Trait object renaming** (`Any*` → `Dyn*`) - Affects all code using trait objects
2. **API naming improvements** - Removing `get_` prefix for better compliance

⚠️ **All changes require code updates** - See automated migration tools below.

---

## 🔴 PART 1: MAJOR BREAKING CHANGES - Trait Object Renaming

### Overview: `Any*` → `Dyn*` Migration

**Rationale:** The `Any*` prefix was confusing because it suggested a relationship with `std::any::Any`.
The new `Dyn*` prefix clearly indicates these are object-safe traits for dynamic dispatch, following Rust conventions.

**Impact:** 182+ occurrences renamed across 120+ files.

---

## 1. AnyElement → DynElement

**Location:** `flui_core::element::DynElement`

### Breaking Changes

| Old Name | New Name | Type | Usage |
|----------|----------|------|-------|
| `AnyElement` | `DynElement` | Trait | Object-safe element trait |
| `Box<dyn AnyElement>` | `Box<dyn DynElement>` | Type | Trait object |
| `any_element` module | `dyn_element` module | Module | Renamed file |

### Migration Example

```rust
// Before
use flui_core::AnyElement;

fn process_element(element: &dyn AnyElement) {
    // ...
}

let elements: Vec<Box<dyn AnyElement>> = vec![];

// After
use flui_core::DynElement;

fn process_element(element: &dyn DynElement) {
    // ...
}

let elements: Vec<Box<dyn DynElement>> = vec![];
```

**Search & Replace:**
- `AnyElement` → `DynElement` (all occurrences)
- `use flui_core::element::any_element` → `use flui_core::element::dyn_element`

---

## 2. AnyWidget → DynWidget

**Location:** `flui_core::widget::DynWidget`

### Breaking Changes

| Old Name | New Name | Type | Usage |
|----------|----------|------|-------|
| `AnyWidget` | `DynWidget` | Trait | Object-safe widget trait |
| `Box<dyn AnyWidget>` | `Box<dyn DynWidget>` | Type | Trait object |
| `any_widget` module | `dyn_widget` module | Module | Renamed file |

### Migration Example

```rust
// Before
use flui_core::AnyWidget;

struct Row {
    children: Vec<Box<dyn AnyWidget>>,
}

impl StatelessWidget for MyWidget {
    fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
        Box::new(Text::new("Hello"))
    }
}

// After
use flui_core::DynWidget;

struct Row {
    children: Vec<Box<dyn DynWidget>>,
}

impl StatelessWidget for MyWidget {
    fn build(&self, context: &Context) -> Box<dyn DynWidget> {
        Box::new(Text::new("Hello"))
    }
}
```

**Search & Replace:**
- `AnyWidget` → `DynWidget` (all occurrences)
- `use flui_core::widget::any_widget` → `use flui_core::widget::dyn_widget`

---

## 3. AnyRenderObject → DynRenderObject

**Location:** `flui_core::render::DynRenderObject`

### Breaking Changes

| Old Name | New Name | Type | Usage |
|----------|----------|------|-------|
| `AnyRenderObject` | `DynRenderObject` | Trait | Object-safe render object trait |
| `Box<dyn AnyRenderObject>` | `Box<dyn DynRenderObject>` | Type | Trait object |
| `any_render_object` module | `dyn_render_object` module | Module | Renamed file |

### Migration Example

```rust
// Before
use flui_core::AnyRenderObject;

impl RenderObjectWidget for Padding {
    fn create_render_object(&self) -> Box<dyn AnyRenderObject> {
        Box::new(RenderPadding::new(self.padding))
    }

    fn update_render_object(&self, render_object: &mut dyn AnyRenderObject) {
        // ...
    }
}

// After
use flui_core::DynRenderObject;

impl RenderObjectWidget for Padding {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        Box::new(RenderPadding::new(self.padding))
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        // ...
    }
}
```

**Search & Replace:**
- `AnyRenderObject` → `DynRenderObject` (all occurrences)
- `use flui_core::render::any_render_object` → `use flui_core::render::dyn_render_object`

---

## 🔵 PART 2: API Naming Improvements

### Overview

Removing the `get_` prefix from methods per [Rust API Guidelines C-GETTER](https://rust-lang.github.io/api-guidelines/naming.html#c-getter).

---

## 4. Intrinsic Size Methods (DynRenderObject trait)

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

⚠️ **IMPORTANT:** Backup your code first! These are breaking changes.

```bash
git commit -am "backup before migration to flui v0.3.0"
```

### Using sed (Unix/Linux/macOS)

```bash
# Navigate to your project root
cd /path/to/your/project

# PART 1: Rename trait objects (Any* → Dyn*)
find . -name "*.rs" -type f -exec sed -i '' \
  -e 's/AnyElement/DynElement/g' \
  -e 's/AnyWidget/DynWidget/g' \
  -e 's/AnyRenderObject/DynRenderObject/g' \
  -e 's/any_element/dyn_element/g' \
  -e 's/any_widget/dyn_widget/g' \
  -e 's/any_render_object/dyn_render_object/g' \
  {} +

# PART 2: Remove get_ prefix from methods
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
git commit -am "migrate to flui v0.3.0 naming conventions"
```

### Using sed (Windows Git Bash)

```bash
# Navigate to your project root
cd /c/path/to/your/project

# PART 1: Rename trait objects (Any* → Dyn*)
find . -name "*.rs" -type f -exec sed -i \
  -e 's/AnyElement/DynElement/g' \
  -e 's/AnyWidget/DynWidget/g' \
  -e 's/AnyRenderObject/DynRenderObject/g' \
  -e 's/any_element/dyn_element/g' \
  -e 's/any_widget/dyn_widget/g' \
  -e 's/any_render_object/dyn_render_object/g' \
  {} +

# PART 2: Remove get_ prefix
find . -name "*.rs" -type f -exec sed -i \
  -e 's/get_min_intrinsic_width/min_intrinsic_width/g' \
  -e 's/get_max_intrinsic_width/max_intrinsic_width/g' \
  -e 's/get_min_intrinsic_height/min_intrinsic_height/g' \
  -e 's/get_max_intrinsic_height/max_intrinsic_height/g' \
  -e 's/get_layout_cache/layout_cache/g' \
  {} +

# Verify
cargo check
```

### Using ripgrep + sd (Cross-platform - Recommended)

```bash
# Install tools if needed
# cargo install sd ripgrep

# PART 1: Rename trait objects (Any* → Dyn*)
rg -l 'AnyElement' --type rust | xargs sd 'AnyElement' 'DynElement'
rg -l 'AnyWidget' --type rust | xargs sd 'AnyWidget' 'DynWidget'
rg -l 'AnyRenderObject' --type rust | xargs sd 'AnyRenderObject' 'DynRenderObject'

# Rename module paths
rg -l 'any_element' --type rust | xargs sd 'any_element' 'dyn_element'
rg -l 'any_widget' --type rust | xargs sd 'any_widget' 'dyn_widget'
rg -l 'any_render_object' --type rust | xargs sd 'any_render_object' 'dyn_render_object'

# PART 2: Remove get_ prefix from methods
rg -l 'get_min_intrinsic_width' --type rust | xargs sd 'get_min_intrinsic_width' 'min_intrinsic_width'
rg -l 'get_max_intrinsic_width' --type rust | xargs sd 'get_max_intrinsic_width' 'max_intrinsic_width'
rg -l 'get_min_intrinsic_height' --type rust | xargs sd 'get_min_intrinsic_height' 'min_intrinsic_height'
rg -l 'get_max_intrinsic_height' --type rust | xargs sd 'get_max_intrinsic_height' 'max_intrinsic_height'
rg -l 'get_layout_cache' --type rust | xargs sd 'get_layout_cache' 'layout_cache'

# Verify
cargo check

# If successful
git commit -am "migrate to flui v0.3.0"
```

---

## Impact Analysis

### Who is affected?

**🔴 EVERYONE using flui_core is affected by Any* → Dyn* changes**

1. **ALL users** - Any code using `Box<dyn AnyElement>`, `Box<dyn AnyWidget>`, or `Box<dyn AnyRenderObject>`
2. **Widget developers** - All `StatelessWidget::build()` return types
3. **RenderObject implementors** - All `create_render_object()` and `update_render_object()` methods
4. **Custom element creators** - All trait object usage
5. **Users calling `get_layout_cache()` directly** - Update to `layout_cache()`
6. **Users implementing intrinsic size methods** - Remove `get_` prefix

### Compilation Errors You Might See

#### Error 1: Cannot find type `AnyElement`

```
error[E0412]: cannot find type `AnyElement` in this scope
  --> src/my_widget.rs:15:32
   |
15 | fn process(element: &dyn AnyElement) {
   |                          ^^^^^^^^^^ not found in this scope
   |
help: consider importing this trait
   |
1  | use flui_core::DynElement;
   |
```

**Fix:** Replace `AnyElement` with `DynElement` and update imports.

#### Error 2: Cannot find type `AnyWidget`

```
error[E0412]: cannot find type `AnyWidget` in crate `flui_core`
  --> src/my_widget.rs:8:37
   |
8  |     fn build(&self) -> Box<dyn AnyWidget> {
   |                                 ^^^^^^^^^ not found in this scope
   |
help: consider importing this trait
   |
1  | use flui_core::DynWidget;
   |
```

**Fix:** Replace `AnyWidget` with `DynWidget`.

#### Error 3: Method renamed

```
error[E0599]: no function named `get_min_intrinsic_width` found
  --> src/my_render_object.rs:42:5
   |
42 |     fn get_min_intrinsic_width(&self, height: f32) -> f32 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = note: the trait `DynRenderObject` defines an item `min_intrinsic_width`
```

**Fix:** Rename to `min_intrinsic_width` (remove `get_` prefix).

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

### Breaking Changes Count

🔴 **MAJOR: 3 trait object renames** (Any* → Dyn*)
- `AnyElement` → `DynElement` (82+ occurrences)
- `AnyWidget` → `DynWidget` (60+ occurrences)
- `AnyRenderObject` → `DynRenderObject` (40+ occurrences)

🔵 **Minor: 5 method/function renames** (remove get_ prefix)
- 4 intrinsic size methods
- 1 layout cache function

### Migration Time

- **Automated migration:** < 5 minutes using sed/ripgrep
- **Manual verification:** 5-10 minutes
- **Total time:** ~10-15 minutes for most projects

### Benefits

✅ **100% Rust API Guidelines compliance** - No more confusing `Any*` prefix
✅ **Clearer code** - `Dyn*` explicitly indicates dynamic dispatch
✅ **Better IDE support** - Less confusion with `std::any::Any`
✅ **Improved maintainability** - Consistent with Rust ecosystem

### Migration Checklist

- [ ] Backup code (`git commit -am "backup"`)
- [ ] Run automated migration script
- [ ] Verify with `cargo check`
- [ ] Run tests (`cargo test`)
- [ ] Review changes (`git diff`)
- [ ] Commit (`git commit -am "migrate to flui v0.3.0"`)

---

*Last updated: 2025-10-21*
*Flui version: 0.3.0*
*Migration difficulty: ⭐⭐ (Easy with automation)*
