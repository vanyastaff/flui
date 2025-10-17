# Three Syntax Styles for Container

**Date**: 2025-10-16
**Status**: 🎯 Design Document

## Goal

Support **three different syntax styles** for creating Container widgets, giving users maximum flexibility:

1. **Struct Literal** (Flutter-like, most concise)
2. **Builder Pattern** (Rust idiomatic, chainable)
3. **Constructor with builder** (Hybrid approach)

## Syntax Style 1: Struct Literal (Flutter-like)

**Most concise, closest to Flutter!**

```rust
use egui::Widget;

Container {
    width: Some(300.0),
    height: Some(200.0),
    padding: Some(EdgeInsets::all(20.0)),
    decoration: Some(BoxDecoration::new()
        .color(Color::from_rgb(100, 150, 255))
    ),
    child: Some(Box::new(|ui| {
        ui.label("Hello World")
    })),
    ..Default::default()
}
.ui(ui);
```

**Pros:**
- ✅ Closest to Flutter syntax
- ✅ Most concise
- ✅ Named parameters (fields)
- ✅ IDE autocomplete for fields

**Cons:**
- ❌ Must specify `Some(...)` for each field
- ❌ Must use `..Default::default()`
- ❌ Less discoverable than methods

## Syntax Style 2: Builder Pattern (Current)

**Rust idiomatic, chainable**

```rust
Container::new()
    .width(300.0)           // No .with_* prefix (cleaned up)
    .height(200.0)
    .padding(EdgeInsets::all(20.0))
    .decoration(BoxDecoration::new()
        .color(Color::from_rgb(100, 150, 255))
    )
    .child(|ui| {
        ui.label("Hello World")
    })
    .ui(ui);
```

**Pros:**
- ✅ Rust idiomatic
- ✅ Chainable/fluent API
- ✅ No `Some(...)` wrapper needed
- ✅ Method autocomplete
- ✅ `.child()` takes closure directly (no Box)

**Cons:**
- ❌ Longer than struct literal
- ❌ Requires `.new()` call

## Syntax Style 3: Typed Builder (New!)

**Combines benefits of both**

```rust
Container::build()      // or Container::builder()
    .width(300.0)
    .height(200.0)
    .padding(EdgeInsets::all(20.0))
    .decoration(BoxDecoration::new()
        .color(Color::from_rgb(100, 150, 255))
    )
    .child(|ui| {
        ui.label("Hello World")
    })
    .finish()           // Returns Container, then call .ui(ui)
    .ui(ui);
```

**Pros:**
- ✅ Clear separation: build phase vs use phase
- ✅ Can implement `Into<Container>` for auto-conversion
- ✅ Type-safe (using typestate pattern without proc-macros)

**Cons:**
- ❌ Requires `.finish()` call
- ❌ More complex implementation

## Implementation Strategy

### Make Container `pub` with Public Fields

```rust
/// Container widget - supports multiple creation styles
pub struct Container {
    // All fields public for struct literal syntax
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub padding: Option<EdgeInsets>,
    pub margin: Option<EdgeInsets>,
    pub decoration: Option<BoxDecoration>,
    pub foreground_decoration: Option<BoxDecoration>,
    pub alignment: Option<Alignment>,
    pub transform: Option<Transform>,
    pub transform_alignment: Option<Alignment>,
    pub clip_behavior: Option<Clip>,
    pub child: Option<Box<dyn FnOnce(&mut egui::Ui) -> egui::Response>>,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            padding: None,
            margin: None,
            decoration: None,
            foreground_decoration: None,
            alignment: None,
            transform: None,
            transform_alignment: None,
            clip_behavior: None,
            child: None,
        }
    }
}

// Builder methods (Style 2)
impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    // Remove .with_* prefix for Flutter-like API
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn padding(mut self, padding: impl Into<EdgeInsets>) -> Self {
        self.padding = Some(padding.into());
        self
    }

    pub fn decoration(mut self, decoration: impl Into<BoxDecoration>) -> Self {
        self.decoration = Some(decoration.into());
        self
    }

    // ... rest of builder methods

    pub fn child(mut self, child: impl FnOnce(&mut egui::Ui) -> egui::Response + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }
}

// Widget implementation (works with all styles)
impl egui::Widget for Container {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Same implementation as before
    }
}
```

## Usage Examples

### Example 1: All Three Styles Side-by-Side

```rust
use nebula_ui::prelude::*;

// Style 1: Struct Literal (Flutter-like)
Container {
    width: Some(300.0),
    height: Some(200.0),
    padding: Some(EdgeInsets::all(20.0)),
    decoration: Some(BoxDecoration::new().color(Color::BLUE)),
    child: Some(Box::new(|ui| { ui.label("Style 1") })),
    ..Default::default()
}
.ui(ui);

// Style 2: Builder Pattern (Rust idiomatic)
Container::new()
    .width(300.0)
    .height(200.0)
    .padding(EdgeInsets::all(20.0))
    .decoration(BoxDecoration::new().color(Color::BLUE))
    .child(|ui| { ui.label("Style 2") })
    .ui(ui);

// Style 3: Explicit Constructor (alternative)
let mut container = Container::new();
container.width = Some(300.0);
container.height = Some(200.0);
container.padding = Some(EdgeInsets::all(20.0));
container.ui(ui);
```

### Example 2: Partial Application

```rust
// Create base container
let base = Container {
    padding: Some(EdgeInsets::all(10.0)),
    decoration: Some(BoxDecoration::new().color(Color::GRAY)),
    ..Default::default()
};

// Extend with builder methods
base
    .width(300.0)
    .child(|ui| { ui.label("Extended!") })
    .ui(ui);
```

## Comparison Table

| Feature | Struct Literal | Builder Pattern | Winner |
|---------|---------------|----------------|--------|
| **Conciseness** | 9/10 | 7/10 | Literal |
| **Flutter-like** | 10/10 | 8/10 | Literal |
| **Type safety** | 7/10 (manual Some) | 10/10 (automatic) | Builder |
| **Discoverability** | 6/10 | 10/10 | Builder |
| **IDE support** | 8/10 | 10/10 | Builder |
| **Readability** | 8/10 | 9/10 | Builder |
| **Flexibility** | 10/10 | 10/10 | Tie |

## Recommendations

### For Users

**Use struct literal when:**
- You want Flutter-like syntax
- You're setting many fields at once
- Code conciseness is priority

**Use builder pattern when:**
- You want method chaining
- You prefer Rust idioms
- You want better IDE autocomplete

### For Library

**Implement both!** It costs almost nothing:
1. Make fields `pub`
2. Add `#[derive(Default)]` or manual impl
3. Keep existing builder methods
4. Remove `.with_*` prefix for cleaner API

## Implementation Checklist

- [ ] Make Container fields `pub`
- [ ] Implement `Default` trait
- [ ] Rename methods (remove `.with_*` prefix)
- [ ] Add documentation for both styles
- [ ] Create example showing all three styles
- [ ] Update tests
- [ ] Update README with syntax examples

## Code Changes Needed

### Current Container (private fields):
```rust
pub struct Container {
    width: Option<f32>,  // private
    // ...
}
```

### New Container (public fields):
```rust
pub struct Container {
    pub width: Option<f32>,  // public!
    // ...
}
```

This **single change** enables struct literal syntax!

## Migration Path

1. **Phase 1**: Make fields public (non-breaking change)
2. **Phase 2**: Remove `.with_*` prefix (breaking change, but simple find-replace)
3. **Phase 3**: Add examples and documentation

## Benefits

✅ **User choice** - pick the style they prefer
✅ **Flutter compatibility** - struct literal matches Flutter
✅ **Rust idioms** - builder pattern is idiomatic
✅ **Gradual adoption** - users can mix styles
✅ **No external dependencies** - pure Rust
✅ **Simple implementation** - just make fields public

---

**Status**: 🎯 Ready to implement
**Recommendation**: Support all three styles
**Next Step**: Make Container fields public and rename methods
