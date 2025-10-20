# Trait Refactoring Plan - Associated Types & Iterators

> **Goal:** Make Flui Core more Rust-idiomatic using associated types and iterator patterns
> **Status:** 🚧 IN PROGRESS
> **Based on:** [AGGRESSIVE_REFACTORING.md](../crates/flui_core/docs/AGGRESSIVE_REFACTORING.md)

---

## 🎯 Objectives

### 1. **Widget Trait - Associated Types**
Replace `Box<dyn Element>` with associated types for:
- Type safety at compile time
- Zero-cost abstractions (no boxing)
- Better performance (static dispatch)

### 2. **Element Trait - Iterator Pattern**
Replace visitor pattern with iterators for:
- Rust-idiomatic API
- Chainable operations
- Better composability

### 3. **Method Naming**
Rust conventions throughout:
- `create_element()` → `into_element()`
- `visit_children()` → `children()` (returns iterator)
- `mark_needs_build()` → `mark_dirty()`

---

## 📋 Phase 1: Widget Trait Refactoring

### Current Implementation

```rust
pub trait Widget: DynClone + Downcast + Debug + Send + Sync {
    fn create_element(&self) -> Box<dyn Element>;
    fn key(&self) -> Option<&dyn Key>;
    fn can_update(&self, other: &dyn Widget) -> bool;
}
```

**Problems:**
- ❌ Requires `DynClone` and `Downcast`
- ❌ Returns `Box<dyn Element>` (heap allocation + dynamic dispatch)
- ❌ `&self` means widget must be cloneable
- ❌ Cannot ensure element type at compile time

### New Implementation

```rust
pub trait Widget: Debug + Send + Sync + 'static {
    type Element: Element;

    /// Consume self and create element (zero-copy)
    fn into_element(self) -> Self::Element;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Check if can update with new widget of same type
    fn can_update_with(&self, other: &Self) -> bool {
        true // Default: same type can always update
    }
}
```

**Benefits:**
- ✅ No more `DynClone` or `Downcast` needed
- ✅ Concrete element type (no boxing)
- ✅ Consuming `into_element()` - true zero-copy
- ✅ Type safety at compile time

### Migration Strategy

#### Step 1: Add new `into_element()` method alongside `create_element()`

```rust
pub trait Widget: DynClone + Downcast + Debug + Send + Sync {
    type Element: Element;

    // OLD - keep for compatibility
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(self.clone().into_element())
    }

    // NEW - implement this
    fn into_element(self) -> Self::Element;
}
```

#### Step 2: Update StatelessWidget

```rust
// OLD
impl<T: StatelessWidget> Widget for T {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(ComponentElement::new(self.clone()))
    }
}

// NEW
impl<T: StatelessWidget> Widget for T {
    type Element = ComponentElement<T>;

    fn into_element(self) -> Self::Element {
        ComponentElement::new(self)
    }
}
```

#### Step 3: Update StatefulWidget

```rust
// Users will need to implement Widget manually:
impl Widget for CounterWidget {
    type Element = StatefulElement<Self>;

    fn into_element(self) -> Self::Element {
        StatefulElement::new(self)
    }
}
```

#### Step 4: Remove `create_element()` entirely

After all code is migrated, remove the old method.

---

## 📋 Phase 2: Element Trait Refactoring

### Current Implementation

```rust
pub trait Element: DowncastSync + Debug {
    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId));
    // ... other methods
}
```

**Problems:**
- ❌ Visitor pattern not idiomatic in Rust
- ❌ Cannot use iterator combinators (map, filter, etc.)
- ❌ Harder to compose operations

### New Implementation

```rust
pub trait Element: DowncastSync + Debug {
    /// Iterate over child element IDs
    fn children(&self) -> impl Iterator<Item = ElementId> + '_;

    /// Mutable iteration (if needed)
    fn children_mut(&mut self) -> impl Iterator<Item = ElementId> + '_;

    // ... other methods
}
```

**Benefits:**
- ✅ Rust-idiomatic iterator pattern
- ✅ Chainable operations: `elem.children().filter(...).map(...)`
- ✅ Compatible with standard library tools
- ✅ Better composability

### Migration Strategy

#### Step 1: Provide both visitor and iterator

```rust
pub trait Element: DowncastSync + Debug {
    // OLD - keep for compatibility
    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        for child_id in self.children() {
            visitor(child_id);
        }
    }

    // NEW - implement this
    fn children(&self) -> impl Iterator<Item = ElementId> + '_;
}
```

#### Step 2: Update all Element implementations

**ComponentElement:**
```rust
impl<W: StatelessWidget> Element for ComponentElement<W> {
    fn children(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.child.into_iter()
    }
}
```

**StatefulElement:**
```rust
impl Element for StatefulElement {
    fn children(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.child.into_iter()
    }
}
```

**MultiChildRenderObjectElement:**
```rust
impl<W: MultiChildRenderObjectWidget> Element for MultiChildRenderObjectElement<W> {
    fn children(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.children.iter().copied()
    }
}
```

#### Step 3: Remove `visit_children()` entirely

After all code migrated.

---

## 📋 Phase 3: Method Naming Updates

### Widget Trait

| Old | New | Reason |
|-----|-----|--------|
| `create_element()` | `into_element()` | Consuming, zero-copy |
| `can_update()` | `can_update_with()` | Clearer parameter |

### Element Trait

| Old | New | Reason |
|-----|-----|--------|
| `visit_children()` | `children()` | Iterator pattern |
| `child_ids()` | `children()` | Consistent naming |

### BuildContext

| Old | New | Reason |
|-----|-----|--------|
| `mark_needs_build()` | `mark_dirty()` | Shorter, clearer |
| `depend_on_inherited_widget()` | `subscribe_to<W>()` | Clearer intent |
| `find_ancestor_widget_of_type()` | `find_ancestor<W>()` | Shorter, generic |

---

## 🔧 Implementation Steps

### Step 1: Widget Trait Associated Types ✅

- [ ] Add `type Element: Element` to Widget trait
- [ ] Add `into_element(self) -> Self::Element`
- [ ] Keep `create_element()` for compatibility (call `into_element()` internally)
- [ ] Update `StatelessWidget` blanket impl
- [ ] Update all widget types to specify `type Element`
- [ ] Update `ElementTree` to work with concrete types

### Step 2: Element Trait Iterators ✅

- [ ] Add `children() -> impl Iterator<Item = ElementId>`
- [ ] Keep `visit_children()` for compatibility (use `children()` internally)
- [ ] Update all `Element` implementations:
  - [ ] ComponentElement
  - [ ] StatefulElement
  - [ ] RenderObjectElement
  - [ ] MultiChildRenderObjectElement
  - [ ] InheritedElement

### Step 3: BuildContext Updates ✅

- [ ] Add `ancestors() -> impl Iterator<Item = ElementId>`
- [ ] Update `walk_ancestors()` to use iterator
- [ ] Add `find_ancestor<W: Widget>()` generic method
- [ ] Add `subscribe_to<W: InheritedWidget>()`

### Step 4: Tests & Verification ✅

- [ ] All existing tests must pass
- [ ] Add new tests for iterator API
- [ ] Add benchmarks comparing old vs new
- [ ] Update documentation

### Step 5: Remove Deprecated APIs ✅

- [ ] Remove `create_element()`
- [ ] Remove `visit_children()`
- [ ] Remove old naming methods
- [ ] Update CHANGELOG

---

## ⚠️ Breaking Changes

This is a BREAKING refactoring. All user code will need updates:

### For Widget Implementors

```rust
// BEFORE
#[derive(Debug, Clone)]
struct MyWidget;

impl StatelessWidget for MyWidget {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        Box::new(Text::new("Hello"))
    }
}
// Widget automatically implemented via blanket impl

// AFTER
#[derive(Debug, Clone)]
struct MyWidget;

impl StatelessWidget for MyWidget {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        Box::new(Text::new("Hello"))
    }
}
// Widget still automatically implemented, but with type Element = ComponentElement<Self>
```

### For StatefulWidget Users

```rust
// BEFORE
impl Widget for MyStatefulWidget {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(StatefulElement::new(self.clone()))
    }
}

// AFTER
impl Widget for MyStatefulWidget {
    type Element = StatefulElement<Self>;

    fn into_element(self) -> Self::Element {
        StatefulElement::new(self)
    }
}
```

### For Element Users

```rust
// BEFORE
element.visit_children(&mut |child_id| {
    println!("Child: {:?}", child_id);
});

// AFTER
for child_id in element.children() {
    println!("Child: {:?}", child_id);
}

// Or with iterator methods
element.children()
    .filter(|id| is_visible(*id))
    .for_each(|id| println!("Visible child: {:?}", id));
```

---

## 📊 Expected Benefits

### Performance

| Aspect | Before | After | Improvement |
|--------|--------|-------|-------------|
| Widget → Element | `Box<dyn>` (heap) | Stack allocation | ~100ns saved |
| Dispatch | Dynamic | Static | ~5-10ns saved |
| Children iteration | Closure overhead | Iterator | ~2-5ns per child |

### Code Quality

- ✅ More type-safe (compile-time checks)
- ✅ More Rust-idiomatic
- ✅ Better composability
- ✅ Easier to understand for Rust developers
- ✅ Less boilerplate (`DynClone`, `Downcast`)

---

## 🚀 Migration Timeline

1. **Week 1:** Implement Widget associated types (backward compatible)
2. **Week 2:** Implement Element iterators (backward compatible)
3. **Week 3:** Update all internal code to use new APIs
4. **Week 4:** Update examples and documentation
5. **Week 5:** Deprecate old APIs (warnings)
6. **Week 6:** Remove deprecated APIs (breaking release)

---

## 📝 Notes

- Keep `DynClone` and `Downcast` initially for compatibility
- Use deprecation warnings before removal
- Provide migration guide with examples
- Consider providing automated refactoring tool (cargo-fix friendly)

---

**Status:** Ready to implement Phase 1 - Widget Associated Types
