# Widget Associated Types - Design Document

> **Goal:** Add associated types to Widget trait while maintaining Box<dyn Widget> for collections
> **Challenge:** Associated types make traits not object-safe
> **Solution:** Two-trait pattern (AnyWidget + Widget)

---

## Problem Statement

Current design uses `Box<dyn Widget>` everywhere:

```rust
pub trait Widget: DynClone + Downcast + Debug + Send + Sync {
    fn create_element(&self) -> Box<dyn Element>;  // ❌ Dynamic dispatch
}

// Used in collections
struct Row {
    children: Vec<Box<dyn Widget>>,  // Need heterogeneous list
}
```

**Problems:**
- ❌ Heap allocation for every element creation
- ❌ Dynamic dispatch overhead
- ❌ Cannot use associated types (would break object safety)

---

## Solution: Two-Trait Pattern

Split into two traits:

### 1. AnyWidget (Object-Safe)

```rust
/// Object-safe base trait for all widgets
/// Used for `Box<dyn AnyWidget>` in heterogeneous collections
pub trait AnyWidget: DynClone + Downcast + Debug + Send + Sync {
    /// Create element (returns boxed for object safety)
    fn create_element(&self) -> Box<dyn Element>;

    /// Optional key
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Type name
    fn type_name(&self) -> &'static str;

    /// Can update check
    fn can_update(&self, other: &dyn AnyWidget) -> bool;
}
```

### 2. Widget (With Associated Types)

```rust
/// Extended widget trait with associated types
/// Use this for concrete widget types
pub trait Widget: AnyWidget + Sized {
    /// Associated element type (zero-cost!)
    type Element: Element;

    /// Consume self and create element (zero-copy)
    fn into_element(self) -> Self::Element;
}
```

### 3. Automatic AnyWidget Implementation

```rust
impl<T: Widget> AnyWidget for T {
    fn create_element(&self) -> Box<dyn Element> {
        // Clone and convert to concrete element
        Box::new(self.clone().into_element())
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn can_update(&self, other: &dyn AnyWidget) -> bool {
        // Same implementation as before
        if self.type_id() != other.type_id() {
            return false;
        }
        match (self.key(), other.key()) {
            (Some(k1), Some(k2)) => k1.id() == k2.id(),
            (None, None) => true,
            _ => false,
        }
    }
}
```

---

## Usage Examples

### For Single Widgets (Zero-Cost)

```rust
// Define widget
#[derive(Debug, Clone)]
struct MyWidget {
    value: i32,
}

impl Widget for MyWidget {
    type Element = MyElement;  // ✅ Concrete type!

    fn into_element(self) -> MyElement {
        MyElement {
            id: ElementId::new(),
            value: self.value,
            dirty: true,
        }
    }
}

// Automatic WidgetBase impl is provided

// Use it
let widget = MyWidget { value: 42 };
let element = widget.into_element();  // ✅ Zero-cost! No Box!
```

### For Collections (Box<dyn AnyWidget>)

```rust
struct Row {
    children: Vec<Box<dyn AnyWidget>>,  // ✅ Object-safe, heterogeneous
}

impl Row {
    fn new(children: Vec<Box<dyn AnyWidget>>) -> Self {
        Self { children }
    }
}

// Usage
let row = Row::new(vec![
    Box::new(Text::new("Hello")),   // Works!
    Box::new(Button::new("Click")), // Different types OK!
]);
```

### For StatelessWidget

```rust
impl<T: StatelessWidget> Widget for T {
    type Element = ComponentElement<T>;

    fn into_element(self) -> ComponentElement<T> {
        ComponentElement::new(self)  // ✅ No Box!
    }
}

// AnyWidget is automatically implemented via blanket impl
```

---

## Migration Path

### Phase 1: Add New Traits (Backward Compatible)

1. Create `AnyWidget` trait (object-safe)
2. Create `Widget` trait with associated types
3. Provide blanket impl of `AnyWidget` for all `Widget` types
4. Keep existing code working with `Box<dyn AnyWidget>`

### Phase 2: Update Internal Code

1. Change single-widget uses to `into_element()`
2. Keep collections using `Box<dyn AnyWidget>`
3. Update `ElementTree` to accept both

### Phase 3: Update Public API

1. Deprecate direct `create_element()` usage
2. Encourage `into_element()` for new code
3. Provide migration guide

---

## Benefits

### Zero-Cost for Single Widgets

```rust
// BEFORE
let element = widget.create_element();  // Box<dyn Element>
// - Heap allocation
// - Dynamic dispatch
// - Clone widget

// AFTER
let element = widget.into_element();  // ConcreteElement
// - Stack allocation
// - Static dispatch
// - Move widget (zero-copy)
```

### Still Works for Collections

```rust
// Collections still work!
Vec<Box<dyn AnyWidget>>  // ✅ Heterogeneous widgets
```

### Type Safety

```rust
fn mount_widget<W: Widget>(widget: W) -> W::Element {
    widget.into_element()  // ✅ Compiler knows exact type!
}
```

---

## Implementation Plan

### Step 1: Create AnyWidget trait

```rust
// widget/any_widget.rs
pub trait AnyWidget: DynClone + Downcast + Debug + Send + Sync {
    fn create_element(&self) -> Box<dyn Element>;
    fn key(&self) -> Option<&dyn Key>;
    fn type_name(&self) -> &'static str;
    fn can_update(&self, other: &dyn AnyWidget) -> bool;
}

dyn_clone::clone_trait_object!(AnyWidget);
impl_downcast!(AnyWidget);
```

### Step 2: Create Widget trait with associated types

```rust
// widget/traits.rs
pub trait Widget: AnyWidget + Sized + Clone {
    type Element: Element;

    fn into_element(self) -> Self::Element;
}
```

### Step 3: Blanket impl AnyWidget for Widget

```rust
impl<T: Widget> AnyWidget for T {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(self.clone().into_element())
    }

    fn key(&self) -> Option<&dyn Key> {
        Widget::key(self)
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn can_update(&self, other: &dyn AnyWidget) -> bool {
        // Same logic as before
        if self.type_id() != other.type_id() {
            return false;
        }
        match (self.key(), other.key()) {
            (Some(k1), Some(k2)) => k1.id() == k2.id(),
            (None, None) => true,
            _ => false,
        }
    }
}
```

### Step 4: Update StatelessWidget

```rust
impl<T: StatelessWidget> Widget for T {
    type Element = ComponentElement<T>;

    fn into_element(self) -> ComponentElement<T> {
        ComponentElement::new(self)
    }
}

// AnyWidget auto-implemented via blanket impl
```

### Step 5: Update all uses

```rust
// Collections: Box<dyn Widget> → Box<dyn AnyWidget>
Vec<Box<dyn AnyWidget>>

// Single widgets: widget.create_element() → widget.into_element()
let element = widget.into_element();
```

---

## Breaking Changes

### What Breaks

1. **Type signatures with `dyn Widget`**
   ```rust
   // BEFORE
   fn foo(widget: &dyn Widget) { }
   Vec<Box<dyn Widget>>

   // AFTER
   fn foo(widget: &dyn AnyWidget) { }
   Vec<Box<dyn AnyWidget>>
   ```

2. **Manual Widget implementations**
   ```rust
   // BEFORE
   impl Widget for MyWidget {
       fn create_element(&self) -> Box<dyn Element> { }
   }

   // AFTER
   impl Widget for MyWidget {
       type Element = MyElement;
       fn into_element(self) -> MyElement { }
   }
   // AnyWidget is auto-implemented
   ```

### What Doesn't Break

1. **StatelessWidget/StatefulWidget** - Auto-updated via blanket impl
2. **Using widgets** - `widget.create_element()` still works (via AnyWidget)
3. **Collections** - Just change `dyn Widget` to `dyn AnyWidget`

---

## Alternatives Considered

### Alternative 1: Enum-Based (iced approach)

```rust
enum AnyWidget {
    Text(TextWidget),
    Row(RowWidget),
    // ... all widget types
}
```

**Pros:** True zero-cost, no trait objects
**Cons:**
- Can't extend with custom widgets
- Large enum size
- More intrusive refactoring

**Decision:** Rejected - too restrictive

### Alternative 2: Full Breaking Change

Remove `Box<dyn Widget>` entirely, require users to use enums.

**Pros:** Cleanest design
**Cons:**
- Massive breaking change
- Harder migration
- Less flexible for users

**Decision:** Rejected - too disruptive

### Alternative 3: Keep Current (No Change)

**Pros:** No breaking changes
**Cons:**
- Miss performance benefits
- Not Rust-idiomatic

**Decision:** Rejected - we want improvements!

---

## Success Criteria

✅ Zero-cost element creation for single widgets
✅ Collections still work (`Vec<Box<dyn AnyWidget>>`)
✅ All tests pass
✅ Clear migration guide
✅ Minimal breaking changes

---

**Decision:** Proceed with Two-Trait Pattern (AnyWidget + Widget)
