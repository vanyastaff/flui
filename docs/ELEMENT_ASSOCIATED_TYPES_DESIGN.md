# Element Associated Types - Design Document

> **Goal:** Add associated types to Element trait while maintaining Box<dyn Element> for collections
> **Challenge:** Associated types make traits not object-safe
> **Solution:** Two-trait pattern (AnyElement + Element)

---

## Problem Statement

Current design uses `Box<dyn Element>` everywhere:

```rust
pub trait Element: DowncastSync + Debug {
    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>);  // ❌ Type-erased
    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)>;
}

// Used in element tree
struct ElementTree {
    elements: HashMap<ElementId, Box<dyn Element>>,  // Need heterogeneous storage
}
```

**Problems:**
- ❌ Type-erased widget updates via `Box<dyn Any>`
- ❌ No compile-time type safety for widget-element relationship
- ❌ Cannot express widget type constraint

---

## Solution: Two-Trait Pattern

Split into two traits:

### 1. AnyElement (Object-Safe)

```rust
/// Object-safe base trait for all elements
/// Used for `Box<dyn AnyElement>` in element tree storage
pub trait AnyElement: Downcast + Debug + Send + Sync {
    // Identity
    fn id(&self) -> ElementId;
    fn parent(&self) -> Option<ElementId>;
    fn key(&self) -> Option<&dyn Key>;

    // Lifecycle (object-safe versions)
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);
    fn unmount(&mut self);
    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>);  // Type-erased
    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)>;

    // State
    fn is_dirty(&self) -> bool;
    fn mark_dirty(&mut self);
    fn lifecycle(&self) -> ElementLifecycle;

    // Child traversal
    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_>;

    // RenderObject support (optional)
    fn render_object(&self) -> Option<&dyn RenderObject>;
    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject>;

    // Other methods...
}
```

### 2. Element (With Associated Types)

```rust
/// Extended element trait with associated types
/// Use this for concrete element types
pub trait Element: AnyElement + Sized {
    /// Associated widget type (zero-cost!)
    type Widget: crate::Widget;

    /// Update element with concrete widget type (zero-cost)
    fn update(&mut self, new_widget: Self::Widget);

    /// Get widget reference (zero-cost)
    fn widget(&self) -> &Self::Widget;
}
```

### 3. Automatic AnyElement Implementation

```rust
impl<T: Element> AnyElement for T {
    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>) {
        // Downcast and call type-safe update
        if let Some(widget) = new_widget.downcast_ref::<T::Widget>() {
            self.update(widget.clone());
        } else {
            panic!("Widget type mismatch");
        }
    }

    fn id(&self) -> ElementId {
        Element::id(self)
    }

    // ... forward other methods
}
```

---

## Usage Examples

### For Concrete Elements (Zero-Cost)

```rust
#[derive(Debug)]
struct ComponentElement<W: StatelessWidget> {
    id: ElementId,
    widget: W,
    child: Option<ElementId>,
    // ...
}

impl<W: StatelessWidget> Element for ComponentElement<W> {
    type Widget = W;  // ✅ Concrete type!

    fn update(&mut self, new_widget: W) {
        self.widget = new_widget;  // ✅ Zero-cost! No downcast!
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}

// AnyElement is automatically implemented

// Usage
let element = ComponentElement::new(my_widget);
element.update(new_widget);  // ✅ Type-safe! No Box!
```

### For Element Tree (Box<dyn AnyElement>)

```rust
struct ElementTree {
    elements: HashMap<ElementId, Box<dyn AnyElement>>,  // ✅ Heterogeneous
}

impl ElementTree {
    fn update_element(&mut self, id: ElementId, widget: Box<dyn AnyWidget>) {
        if let Some(element) = self.elements.get_mut(&id) {
            element.update_any(widget);  // Uses object-safe method
        }
    }
}
```

---

## Benefits

### Zero-Cost for Concrete Operations

```rust
// BEFORE
element.update(Box::new(new_widget) as Box<dyn Any>);
// - Heap allocation
// - Type erasure
// - Runtime downcast needed

// AFTER
element.update(new_widget);
// - Stack value
// - Compile-time type checking
// - Zero overhead
```

### Type Safety

```rust
fn update_element<E: Element>(element: &mut E, widget: E::Widget) {
    element.update(widget);  // ✅ Compiler enforces correct type!
}
```

### Still Works for Tree Storage

```rust
// Element tree still works!
HashMap<ElementId, Box<dyn AnyElement>>  // ✅ Heterogeneous storage
```

---

## Implementation Plan

### Step 1: Create AnyElement trait

```rust
// element/any_element.rs (already created!)
pub trait AnyElement: Downcast + Debug + Send + Sync {
    fn id(&self) -> ElementId;
    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>);
    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)>;
    // ... all object-safe methods
}
```

### Step 2: Create Element trait with associated types

```rust
// element/traits.rs
pub trait Element: AnyElement + Sized {
    type Widget: crate::Widget;

    fn update(&mut self, new_widget: Self::Widget);
    fn widget(&self) -> &Self::Widget;
}
```

### Step 3: Blanket impl AnyElement for Element

```rust
impl<T: Element> AnyElement for T {
    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>) {
        // Type-safe downcast
        let widget = new_widget
            .downcast::<T::Widget>()
            .expect("Widget type mismatch");
        self.update(*widget);
    }

    // Forward all other methods to Element
}
```

### Step 4: Update ComponentElement

```rust
impl<W: StatelessWidget> Element for ComponentElement<W> {
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        self.widget = new_widget;
        self.dirty = true;
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}

// AnyElement auto-implemented via blanket impl
```

### Step 5: Update all uses

```rust
// Element tree: Box<dyn Element> → Box<dyn AnyElement>
HashMap<ElementId, Box<dyn AnyElement>>

// Concrete updates: use type-safe Element::update()
element.update(new_widget);  // Not update_any()
```

---

## Breaking Changes

### What Breaks

1. **Type signatures with `dyn Element`**
   ```rust
   // BEFORE
   fn foo(element: &dyn Element) { }
   HashMap<ElementId, Box<dyn Element>>

   // AFTER
   fn foo(element: &dyn AnyElement) { }
   HashMap<ElementId, Box<dyn AnyElement>>
   ```

2. **Manual Element implementations**
   ```rust
   // BEFORE
   impl Element for MyElement {
       fn update(&mut self, new_widget: Box<dyn Any>) { }
   }

   // AFTER
   impl Element for MyElement {
       type Widget = MyWidget;

       fn update(&mut self, new_widget: MyWidget) { }
       fn widget(&self) -> &MyWidget { &self.widget }
   }
   // AnyElement is auto-implemented
   ```

### What Doesn't Break

1. **Element tree storage** - Just change `dyn Element` to `dyn AnyElement`
2. **Element traversal** - Works the same
3. **Lifecycle methods** - No changes needed

---

## Alternatives Considered

### Alternative 1: RenderObject Associated Type

```rust
pub trait Element: AnyElement {
    type Widget: crate::Widget;
    type RenderObject: crate::RenderObject;  // ❌ Not all elements have RenderObject
}
```

**Decision:** Rejected - too complex. RenderObject is optional, so keep it as `Option<&dyn RenderObject>`

### Alternative 2: Child Associated Type

```rust
pub trait Element: AnyElement {
    type Widget: crate::Widget;
    type Child: ElementChild;  // Single, Multi, or None
}
```

**Decision:** Rejected - over-engineered. Child traversal via `children_iter()` is sufficient.

### Alternative 3: Minimal Associated Types

```rust
pub trait Element: AnyElement {
    type Widget: crate::Widget;  // ✅ Only essential associated type

    fn update(&mut self, new_widget: Self::Widget);
    fn widget(&self) -> &Self::Widget;
}
```

**Decision:** ✅ Accepted - Keep it simple! Just like we did for Widget trait.

---

## Success Criteria

✅ Zero-cost widget updates for concrete element types
✅ Element tree storage still works (`HashMap<ElementId, Box<dyn AnyElement>>`)
✅ All tests pass
✅ Clear migration guide
✅ Minimal breaking changes

---

**Decision:** Proceed with Two-Trait Pattern (AnyElement + Element) with minimal associated types
