# Migration Guide: `Any*` → `Dyn*` Renaming

## Overview

This guide covers the breaking changes from the `Any*` → `Dyn*` trait renaming in `flui_core`.

**Rationale:** The `Any*` prefix was confusing because it suggested a relationship with `std::any::Any`. The new `Dyn*` prefix clearly indicates these are object-safe traits for dynamic dispatch, following Rust API Guidelines.

## Breaking Changes

### 1. `AnyElement` → `DynElement`

All references to `AnyElement` must be updated to `DynElement`.

**Before:**
```rust
use flui_core::AnyElement;

let elements: Vec<Box<dyn AnyElement>> = vec![];

fn process_element(element: &dyn AnyElement) {
    // ...
}
```

**After:**
```rust
use flui_core::DynElement;

let elements: Vec<Box<dyn DynElement>> = vec![];

fn process_element(element: &dyn DynElement) {
    // ...
}
```

### 2. `AnyWidget` → `DynWidget`

All references to `AnyWidget` must be updated to `DynWidget`.

**Before:**
```rust
use flui_core::AnyWidget;

fn build(&self) -> Box<dyn AnyWidget> {
    Box::new(MyWidget)
}

let widgets: Vec<Box<dyn AnyWidget>> = vec![];
```

**After:**
```rust
use flui_core::DynWidget;

fn build(&self) -> Box<dyn DynWidget> {
    Box::new(MyWidget)
}

let widgets: Vec<Box<dyn DynWidget>> = vec![];
```

### 3. `AnyRenderObject` → `DynRenderObject`

All references to `AnyRenderObject` must be updated to `DynRenderObject`.

**Before:**
```rust
use flui_core::AnyRenderObject;

fn render_object(&self) -> Option<&dyn AnyRenderObject> {
    self.render.as_ref().map(|r| r.as_ref())
}

let render_objects: Vec<Box<dyn AnyRenderObject>> = vec![];
```

**After:**
```rust
use flui_core::DynRenderObject;

fn render_object(&self) -> Option<&dyn DynRenderObject> {
    self.render.as_ref().map(|r| r.as_ref())
}

let render_objects: Vec<Box<dyn DynRenderObject>> = vec![];
```

## Automated Migration

You can use `sed` or `find-replace` to automate most of the migration:

```bash
# Unix/Linux/macOS
find . -name "*.rs" -type f -exec sed -i 's/AnyElement/DynElement/g' {} +
find . -name "*.rs" -type f -exec sed -i 's/AnyWidget/DynWidget/g' {} +
find . -name "*.rs" -type f -exec sed -i 's/AnyRenderObject/DynRenderObject/g' {} +

# Windows (PowerShell)
Get-ChildItem -Recurse -Filter *.rs | ForEach-Object {
    (Get-Content $_.FullName) -replace 'AnyElement', 'DynElement' |
    Set-Content $_.FullName
}
Get-ChildItem -Recurse -Filter *.rs | ForEach-Object {
    (Get-Content $_.FullName) -replace 'AnyWidget', 'DynWidget' |
    Set-Content $_.FullName
}
Get-ChildItem -Recurse -Filter *.rs | ForEach-Object {
    (Get-Content $_.FullName) -replace 'AnyRenderObject', 'DynRenderObject' |
    Set-Content $_.FullName
}
```

## Import Changes

Update your imports:

**Before:**
```rust
use flui_core::{AnyElement, AnyWidget, AnyRenderObject};
use flui_core::element::AnyElement;
use flui_core::widget::AnyWidget;
use flui_core::render::AnyRenderObject;
```

**After:**
```rust
use flui_core::{DynElement, DynWidget, DynRenderObject};
use flui_core::element::DynElement;
use flui_core::widget::DynWidget;
use flui_core::render::DynRenderObject;
```

## Prelude Changes

The prelude has been updated:

**Before:**
```rust
use flui_core::prelude::*;
// Had: AnyElement, AnyWidget
```

**After:**
```rust
use flui_core::prelude::*;
// Now has: DynElement, DynWidget
```

## Common Patterns

### Pattern 1: Trait Objects in Structs

**Before:**
```rust
struct ElementTree {
    elements: HashMap<ElementId, Box<dyn AnyElement>>,
}
```

**After:**
```rust
struct ElementTree {
    elements: HashMap<ElementId, Box<dyn DynElement>>,
}
```

### Pattern 2: Downcasting

**Before:**
```rust
if let Some(component) = element.downcast_ref::<ComponentElement<MyWidget>>() {
    // Still works - DynElement supports downcasting via downcast_rs
}
```

**After:**
```rust
// No changes needed - downcasting still works the same way
if let Some(component) = element.downcast_ref::<ComponentElement<MyWidget>>() {
    // ...
}
```

### Pattern 3: Method Returns

**Before:**
```rust
impl Widget for MyWidget {
    fn into_element(self) -> Box<dyn AnyElement> {
        Box::new(ComponentElement::new(self))
    }
}
```

**After:**
```rust
impl Widget for MyWidget {
    fn into_element(self) -> Box<dyn DynElement> {
        Box::new(ComponentElement::new(self))
    }
}
```

## Trait Implementations

No changes needed - if you implemented the old traits, they automatically work with the new names after renaming:

```rust
// Your code automatically works after find-replace
impl DynElement for MyCustomElement {
    fn id(&self) -> ElementId { self.id }
    // ... other methods
}
```

## Testing

After migration, verify your code:

1. Run `cargo check` to find any remaining references
2. Run `cargo test` to ensure behavior is unchanged
3. Search for `Any` in your codebase to catch any missed occurrences:
   ```bash
   rg "AnyElement|AnyWidget|AnyRenderObject"
   ```

## Timeline

- **Deprecated:** None (hard breaking change, no deprecation period)
- **Removed:** Immediately (variant C - hard refactoring)

## Need Help?

If you encounter issues during migration:
1. Check that all imports are updated
2. Verify trait bounds use `DynElement` not `AnyElement`
3. Look for string literals that might contain "Any" (comments, error messages)

## Rationale

The `Dyn*` prefix:
- ✅ Clearly indicates dynamic dispatch (trait objects)
- ✅ Avoids confusion with `std::any::Any`
- ✅ Follows Rust API Guidelines (C-CONV)
- ✅ Matches naming conventions in other Rust projects

The `Any*` prefix:
- ❌ Suggested relationship with `std::any::Any`
- ❌ Was misleading about the trait's purpose
- ❌ Did not follow common Rust conventions
