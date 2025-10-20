# Phase 5: ProxyWidget Hierarchy - Design Document

**Date:** 2025-10-20
**Status:** Planning

---

## Overview

ProxyWidget is a fundamental pattern in Flutter's widget system. It's a widget that:
1. **Has exactly one child** (wraps another widget)
2. **Provides some service** to that child (data, configuration, constraints, etc.)
3. **Does not create a RenderObject** itself

ProxyWidget is the **base class** for:
- **InheritedWidget** - Propagates data down the tree
- **ParentDataWidget** - Configures parent data on RenderObject children
- **NotificationListener** - Listens to notifications bubbling up
- And many others...

---

## Flutter's ProxyWidget Architecture

```dart
// Flutter's ProxyWidget (simplified)
abstract class ProxyWidget extends Widget {
  final Widget child;

  ProxyWidget({required this.child});

  ProxyElement createElement() => ProxyElement(this);
}

abstract class ProxyElement extends ComponentElement {
  ProxyElement(ProxyWidget widget) : super(widget);

  @override
  Widget build() => (widget as ProxyWidget).child;

  // Called when widget updates
  void updated(covariant ProxyWidget oldWidget) {
    notifyClients(oldWidget);
  }

  // Override in subclasses to notify dependents
  void notifyClients(covariant ProxyWidget oldWidget) {}
}
```

### Key Features

1. **Single child:** ProxyWidget always has exactly one child
2. **No render object:** ProxyElement doesn't create RenderObject
3. **Build returns child:** `build()` just returns the child widget
4. **Update hook:** `updated()` is called when widget updates
5. **Notify hook:** `notifyClients()` lets subclasses notify dependents

---

## Current State in Flui

We already have **InheritedWidget** implemented directly as:
- `InheritedWidget` trait
- `InheritedElement<W>` struct

**Problem:** InheritedWidget should **extend** ProxyWidget, not be standalone!

**Current hierarchy:**
```
Widget
  ├─ StatelessWidget
  ├─ StatefulWidget
  └─ InheritedWidget ❌ (should be under ProxyWidget!)
```

**Target hierarchy:**
```
Widget
  ├─ StatelessWidget
  ├─ StatefulWidget
  └─ ProxyWidget
      ├─ InheritedWidget
      └─ ParentDataWidget
```

---

## Design for Rust Implementation

### 1. ProxyWidget Trait

```rust
/// Widget that wraps a single child and provides services
pub trait ProxyWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Get the child widget
    fn child(&self) -> &dyn AnyWidget;

    /// Optional key
    fn key(&self) -> Option<&dyn Key> {
        None
    }
}
```

**Simple:** Just requires a `child()` method.

### 2. ProxyElement Struct

```rust
/// Element for ProxyWidget (delegates to child)
pub struct ProxyElement<W: ProxyWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    tree: Option<Arc<RwLock<ElementTree>>>,
    child: Option<ElementId>,
}

impl<W: ProxyWidget> ProxyElement<W> {
    pub fn new(widget: W) -> Self { /* ... */ }

    /// Called when widget updates (hook for subclasses)
    pub fn updated(&mut self, old_widget: &W) {
        self.notify_clients(old_widget);
    }

    /// Override point for notifying dependents (default: no-op)
    pub fn notify_clients(&mut self, _old_widget: &W) {
        // Default: do nothing
        // InheritedElement will override this
    }
}
```

**Key points:**
- Generic over `W: ProxyWidget`
- Stores single child
- `updated()` and `notify_clients()` hooks

### 3. InheritedWidget extends ProxyWidget

**Refactor InheritedWidget to build on ProxyWidget:**

```rust
/// Propagates data down the tree (extends ProxyWidget)
pub trait InheritedWidget: ProxyWidget {
    type Data;

    fn data(&self) -> &Self::Data;
    fn update_should_notify(&self, old: &Self) -> bool;

    fn inherited_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// Element for InheritedWidget (extends ProxyElement behavior)
pub struct InheritedElement<W: InheritedWidget> {
    // Embed ProxyElement
    base: ProxyElement<W>,
    // InheritedWidget-specific state
    dependents: AHashSet<ElementId>,
}
```

**Benefits:**
- Code reuse (ProxyElement handles child management)
- Clear hierarchy (InheritedWidget **is a** ProxyWidget)
- Easy to add more ProxyWidget types (ParentDataWidget, etc.)

### 4. ParentDataWidget

```rust
/// Configures parent data on RenderObject children
pub trait ParentDataWidget<T: ParentData>: ProxyWidget {
    /// Apply parent data to the child's RenderObject
    fn apply_parent_data(&self, render_object: &mut dyn AnyRenderObject);

    /// Debug: Typical ancestor widget class
    fn debug_typical_ancestor_widget_class(&self) -> &'static str;

    /// Can this widget apply parent data out of turn?
    fn debug_can_apply_out_of_turn(&self) -> bool {
        false
    }
}

/// Element for ParentDataWidget
pub struct ParentDataElement<W, T>
where
    W: ParentDataWidget<T>,
    T: ParentData,
{
    base: ProxyElement<W>,
    _phantom: PhantomData<T>,
}
```

**Purpose:** Widgets like `Positioned` (for Stack), `Flexible` (for Row/Column) that configure how parent RenderObject lays out children.

---

## Implementation Plan

### Step 1: Create ProxyWidget Infrastructure
1. Create `crates/flui_core/src/widget/proxy.rs`
2. Implement `ProxyWidget` trait
3. Implement `ProxyElement<W>` struct
4. Implement `AnyElement` for `ProxyElement<W>`
5. Implement `Element` for `ProxyElement<W>`

### Step 2: Refactor InheritedWidget
1. Make `InheritedWidget` extend `ProxyWidget`
2. Refactor `InheritedElement` to use `ProxyElement` internally
3. Update tests to ensure no regressions

### Step 3: Implement ParentDataWidget
1. Create `crates/flui_core/src/widget/parent_data_widget.rs`
2. Implement `ParentDataWidget<T>` trait
3. Implement `ParentDataElement<W, T>` struct
4. Add parent data application logic

### Step 4: Update Element Hierarchy
1. Create `crates/flui_core/src/element/proxy.rs`
2. Move ProxyElement to element module (optional, for organization)

### Step 5: Testing
1. Unit tests for ProxyWidget
2. Unit tests for ParentDataWidget
3. Integration tests with InheritedWidget
4. Verify all existing InheritedWidget tests still pass

---

## API Examples

### Example 1: Custom ProxyWidget

```rust
#[derive(Debug, Clone)]
struct CustomProxy {
    child: Box<dyn AnyWidget>,
}

impl ProxyWidget for CustomProxy {
    fn child(&self) -> &dyn AnyWidget {
        &*self.child
    }
}

impl_widget_for_proxy!(CustomProxy);
```

### Example 2: InheritedWidget (new style)

```rust
#[derive(Debug, Clone)]
struct Theme {
    color: Color,
    child: Box<dyn AnyWidget>,
}

impl ProxyWidget for Theme {
    fn child(&self) -> &dyn AnyWidget {
        &*self.child
    }
}

impl InheritedWidget for Theme {
    type Data = Color;

    fn data(&self) -> &Self::Data {
        &self.color
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.color != old.color
    }
}

impl_widget_for_inherited!(Theme);
```

### Example 3: ParentDataWidget

```rust
#[derive(Debug, Clone)]
struct Flexible {
    flex: u32,
    child: Box<dyn AnyWidget>,
}

impl ProxyWidget for Flexible {
    fn child(&self) -> &dyn AnyWidget {
        &*self.child
    }
}

impl ParentDataWidget<FlexParentData> for Flexible {
    fn apply_parent_data(&self, render_object: &mut dyn AnyRenderObject) {
        if let Some(parent_data) = render_object.parent_data_mut::<FlexParentData>() {
            parent_data.flex = self.flex;
        }
    }

    fn debug_typical_ancestor_widget_class(&self) -> &'static str {
        "Flex"
    }
}
```

---

## Benefits of ProxyWidget Pattern

1. **Code reuse:** Common child management logic in ProxyElement
2. **Clear hierarchy:** Explicit "has one child" relationship
3. **Extensibility:** Easy to add new ProxyWidget types
4. **Type safety:** Compiler enforces single-child constraint
5. **Flutter compatibility:** Matches Flutter's architecture

---

## Migration Strategy

### For Users of InheritedWidget

**Before:**
```rust
impl InheritedWidget for MyWidget {
    fn child(&self) -> &dyn AnyWidget { /* ... */ }
    // ...
}

impl_widget_for_inherited!(MyWidget);
```

**After:**
```rust
// ProxyWidget is automatically satisfied via InheritedWidget
impl InheritedWidget for MyWidget {
    fn child(&self) -> &dyn AnyWidget { /* ... */ }
    // ...
}

impl_widget_for_inherited!(MyWidget);
```

**Impact:** Minimal! InheritedWidget automatically implements ProxyWidget.

### Breaking Changes

1. **InheritedWidget trait signature** - Now extends ProxyWidget
2. **InheritedElement internals** - Now uses ProxyElement (implementation detail)

**Mitigation:** Keep existing public API intact. Only internal structure changes.

---

## Open Questions

### Q1: Should ProxyElement be generic or use trait objects?

**Option A:** Generic `ProxyElement<W: ProxyWidget>`
- ✅ Zero-cost abstractions
- ✅ Type-safe
- ❌ One struct per widget type

**Option B:** Non-generic `ProxyElement` using `Box<dyn ProxyWidget>`
- ✅ Smaller binary
- ❌ Dynamic dispatch overhead
- ❌ Trait object limitations

**Decision:** Use **generic ProxyElement<W>** for zero-cost abstractions (matches our existing pattern).

### Q2: Should InheritedElement embed or extend ProxyElement?

**Option A:** Embed ProxyElement
```rust
struct InheritedElement<W> {
    base: ProxyElement<W>,
    dependents: AHashSet<ElementId>,
}
```

**Option B:** Separate implementation
```rust
struct InheritedElement<W> {
    id: ElementId,
    widget: W,
    dependents: AHashSet<ElementId>,
    // Duplicate all ProxyElement fields
}
```

**Decision:** **Embed ProxyElement** for code reuse (though we may duplicate for performance).

---

## Success Criteria

✅ ProxyWidget trait implemented
✅ ProxyElement working with single child
✅ InheritedWidget refactored to extend ProxyWidget
✅ ParentDataWidget trait and element implemented
✅ All existing InheritedWidget tests pass
✅ New tests for ProxyWidget and ParentDataWidget
✅ Documentation updated
✅ Zero performance regression

---

## Next Steps

1. Implement ProxyWidget trait
2. Implement ProxyElement struct
3. Refactor InheritedWidget
4. Implement ParentDataWidget
5. Write tests
6. Update ROADMAP

**Estimated time:** 2-3 hours
