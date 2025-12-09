# Typestate Proposal: ViewHandle Architecture

## Overview

This document analyzes two approaches for fixing child mounting architecture:
1. **Simple AnyView**: Runtime state tracking
2. **Typestate ViewHandle**: Compile-time state enforcement

## Current Problem

```rust
// Current implementation - WRONG!
pub struct Child {
    inner: Option<Box<dyn ViewObject>>,  // ❌ Stores state, not config
}

impl Padding {
    pub fn child<V: IntoView>(mut self, view: V) -> Self {
        self.child = Child::new(view);  // Immediately converts to ViewObject
        self  // View config lost!
    }
}
```

**Issues:**
- View configuration lost after conversion to ViewObject
- Can't hot-reload (no way to recreate from config)
- Can't reconcile (can't compare configs)
- Violates Flutter's immutable-config pattern

---

## Approach 1: Simple AnyView (Runtime State)

### Core Types

```rust
/// Type-erased immutable View configuration
pub struct AnyView {
    type_id: TypeId,
    debug_name: &'static str,

    // Factory to create ViewObject from stored config
    create: Arc<dyn Fn(&dyn Any) -> Box<dyn ViewObject> + Send + Sync>,

    // Stored view configuration (type-erased)
    view_data: Box<dyn Any + Send + Sync>,
}

impl AnyView {
    pub fn new<V: IntoView + Clone + 'static>(view: V) -> Self {
        Self {
            type_id: TypeId::of::<V>(),
            debug_name: std::any::type_name::<V>(),
            create: Arc::new(|data| {
                let view = data.downcast_ref::<V>().unwrap().clone();
                view.into_view()
            }),
            view_data: Box::new(view),
        }
    }

    pub fn create_view_object(&self) -> Box<dyn ViewObject> {
        (self.create)(&*self.view_data)
    }
}

/// Child stores immutable config
pub struct Child {
    inner: Option<AnyView>,  // ✅ Stores config!
}

impl Child {
    pub fn new<V: IntoView + Clone>(view: V) -> Self {
        Self {
            inner: Some(AnyView::new(view)),
        }
    }
}
```

### Element Integration

```rust
pub struct Element {
    // Immutable configuration (for hot-reload)
    view_config: Option<AnyView>,

    // Live ViewObject (for build)
    view_object: Option<Box<dyn ViewObject>>,

    // Mounted state
    is_mounted: bool,
}

impl Element {
    /// Phase 1: Mount - creates ViewObject from config
    pub fn mount(&mut self) {
        if let Some(config) = &self.view_config {
            self.view_object = Some(config.create_view_object());
            self.is_mounted = true;
        }
    }

    /// Phase 2: Build - constructs children
    pub fn build(&mut self, ctx: &dyn BuildContext) {
        if let Some(view_obj) = &mut self.view_object {
            // Build children...
        }
    }

    /// Hot-reload: Recreate ViewObject from stored config
    pub fn hot_reload(&mut self) {
        if let Some(config) = &self.view_config {
            self.view_object = Some(config.create_view_object());
            // Tree will be rebuilt automatically
        }
    }
}
```

### Pros & Cons

**✅ Pros:**
- Simple to implement
- Flexible runtime behavior
- Easy integration with existing code
- Supports hot-reload by storing config
- Supports reconciliation (can compare AnyView configs)

**❌ Cons:**
- Runtime overhead (type erasure, Arc cloning)
- All Views must implement Clone
- No compile-time guarantees about mount state
- Can accidentally use unmounted view

---

## Approach 2: Typestate ViewHandle (Compile-Time State)

### Core Types

```rust
/// Marker trait for view states
pub trait ViewState {
    type Inner;
}

/// Unmounted state: holds immutable config + factory
pub struct Unmounted;
pub struct UnmountedInner<V> {
    view_config: V,
}

impl ViewState for Unmounted {
    type Inner = UnmountedInner<V>;  // ❌ Problem: V is unconstrained!
}

/// Mounted state: holds live ViewObject + tree info
pub struct Mounted;
pub struct MountedInner {
    view_object: Box<dyn ViewObject>,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
}

impl ViewState for Mounted {
    type Inner = MountedInner;
}

/// Type-safe view handle with state enforcement
pub struct ViewHandle<S: ViewState> {
    type_id: TypeId,
    debug_name: &'static str,
    inner: S::Inner,
}
```

**⚠️ Problem: Associated Type Needs Generic Parameter**

The `ViewState` trait needs a generic parameter for the view type:

```rust
pub trait ViewState<V> {
    type Inner;
}

impl<V: Clone> ViewState<V> for Unmounted {
    type Inner = UnmountedInner<V>;
}

pub struct ViewHandle<V, S: ViewState<V>> {
    inner: S::Inner,
    _phantom: PhantomData<V>,
}
```

But now `ViewHandle` has 2 type parameters, making type erasure harder!

### Type Erasure Challenge

```rust
// Child needs to store heterogeneous views
pub struct Child {
    // ❌ Can't store ViewHandle<V, Unmounted> - V is different for each child!
    inner: Option<ViewHandle<???, Unmounted>>,
}

// Solution 1: Trait object
pub trait AnyUnmountedView {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject>;
}

impl<V: IntoView> AnyUnmountedView for ViewHandle<V, Unmounted> {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject> {
        self.inner.view_config.into_view()
    }
}

pub struct Child {
    inner: Option<Box<dyn AnyUnmountedView>>,  // Still type-erased!
}
```

### State Transitions

```rust
impl<V: IntoView + Clone> ViewHandle<V, Unmounted> {
    pub fn new(view: V) -> Self {
        Self {
            inner: UnmountedInner { view_config: view },
            _phantom: PhantomData,
        }
    }

    /// Transition: Unmounted → Mounted
    pub fn mount(self) -> ViewHandle<V, Mounted> {
        let view_object = self.inner.view_config.into_view();
        ViewHandle {
            inner: MountedInner {
                view_object,
                parent: None,
                children: Vec::new(),
            },
            _phantom: PhantomData,
        }
    }
}

impl<V> ViewHandle<V, Mounted> {
    pub fn view_object(&self) -> &dyn ViewObject {
        &*self.inner.view_object
    }

    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        &mut *self.inner.view_object
    }
}
```

### Element Integration

```rust
pub struct Element<S: ViewState> {
    handle: ViewHandle<???, S>,  // ❌ Problem: Unknown V type
}

// Alternative: Store state separately
pub struct Element {
    // Type-erased, but with state marker
    state: ElementState,
}

pub enum ElementState {
    Unmounted { config: Box<dyn AnyUnmountedView> },
    Mounted { view_object: Box<dyn ViewObject>, children: Vec<ElementId> },
}
```

### Pros & Cons

**✅ Pros:**
- **Compile-time safety**: Can't use unmounted view as mounted
- **Type-safe transitions**: `mount()` consumes `Unmounted` and returns `Mounted`
- **Clear state model**: State is part of the type system
- **Zero runtime cost**: PhantomData has no overhead

**❌ Cons:**
- **Complex generics**: ViewHandle needs 2 type parameters
- **Type erasure still needed**: Child stores heterogeneous views
- **No real advantage over runtime tracking**: Element already tracks mount state
- **More verbose API**: Type parameters everywhere
- **Harder to integrate**: Requires changing many existing APIs

---

## Detailed Comparison

| Feature | AnyView | Typestate ViewHandle |
|---------|---------|---------------------|
| **Compile-time safety** | ❌ No | ✅ Yes (for non-erased types) |
| **Type erasure complexity** | ⚠️ Moderate | ⚠️ High (needs 2 generics) |
| **Runtime overhead** | ⚠️ Some (Arc) | ✅ None (PhantomData) |
| **API simplicity** | ✅ Simple | ❌ Complex (type params) |
| **Hot-reload support** | ✅ Yes | ✅ Yes |
| **Reconciliation** | ✅ Easy | ⚠️ Harder (need compare trait) |
| **Integration effort** | ✅ Low | ❌ High |

---

## Code Example Comparison

### AnyView Approach

```rust
// Widget code - clean!
pub struct Padding {
    padding: EdgeInsets,
    child: Child,  // Stores AnyView
}

impl Padding {
    pub fn child<V: IntoView + Clone>(mut self, view: V) -> Self {
        self.child = Child::new(view);
        self
    }
}

// Child implementation
pub struct Child {
    inner: Option<AnyView>,
}

impl Child {
    pub fn new<V: IntoView + Clone>(view: V) -> Self {
        Self {
            inner: Some(AnyView::new(view)),
        }
    }
}

// Element code
impl Element {
    fn mount(&mut self) {
        if let Some(config) = &self.view_config {
            self.view_object = Some(config.create_view_object());
        }
    }
}
```

### Typestate Approach

```rust
// Widget code - verbose!
pub struct Padding<S: ViewState<V>, V: IntoView> {
    padding: EdgeInsets,
    child: Child<V>,  // ❌ Problem: V constrains entire struct!
}

// Alternative: Type-erase at widget boundary
pub struct Padding {
    padding: EdgeInsets,
    child: Child,  // Still uses Box<dyn AnyUnmountedView>
}

// Child implementation - still needs type erasure!
pub trait AnyUnmountedView {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject>;
}

pub struct Child {
    inner: Option<Box<dyn AnyUnmountedView>>,  // Same as AnyView!
}

// Element code - complex!
pub enum ElementState {
    Unmounted { config: Box<dyn AnyUnmountedView> },
    Mounted { view_object: Box<dyn ViewObject> },
}

impl Element {
    fn mount(&mut self) {
        let config = match &mut self.state {
            ElementState::Unmounted { config } => config.take(),
            _ => return,
        };
        let view_object = config.mount();
        self.state = ElementState::Mounted { view_object };
    }
}
```

---

## Key Insight: Typestate Doesn't Eliminate Type Erasure

The critical realization is:

**Child/Children MUST store heterogeneous views.**

```rust
// This is unavoidable:
Column {
    children: vec![
        Text("Hello"),      // Different type
        Padding::all(10),   // Different type
        Button::new(),      // Different type
    ]
}
```

Since we need type erasure anyway, typestate doesn't provide meaningful compile-time safety improvements over the simpler AnyView + runtime state tracking approach.

The only place typestate helps is in **non-erased** code, but:
1. Most of our code IS type-erased (Elements, Child, Children)
2. Element already tracks mount state at runtime
3. ViewObject trait already enforces proper usage

---

## Alternative: Hybrid Approach

Could we get the best of both worlds?

```rust
/// Type-safe handle before type erasure
pub struct ViewConfig<V> {
    view: V,
}

impl<V: IntoView + Clone> ViewConfig<V> {
    pub fn new(view: V) -> Self {
        Self { view }
    }

    pub fn into_any(self) -> AnyView {
        AnyView::new(self.view)
    }
}

/// Type-erased after conversion
pub struct AnyView {
    // Same as before
}

// Usage:
let config = ViewConfig::new(Text::new("Hello"));  // Type-safe
let any = config.into_any();  // Explicit type erasure point
```

**Benefits:**
- Explicit type erasure boundary
- Type-safe before erasure
- Simple after erasure

But this is just extra ceremony without real benefit, since IntoView already provides the type-safe API.

---

## Recommendation

**Use AnyView approach** for these reasons:

1. **Simpler implementation**: Single type parameter, straightforward type erasure
2. **Same capabilities**: Both approaches need type erasure for Child/Children
3. **Runtime state is sufficient**: Element already tracks mount state
4. **Better ergonomics**: Less ceremony in widget code
5. **Easier integration**: Minimal changes to existing code
6. **Still safe**: ViewObject trait + Element lifecycle enforce correct usage

Typestate would be valuable if we could avoid type erasure, but:
- Child/Children MUST store heterogeneous views
- This requires trait objects (type erasure)
- Typestate benefits disappear after erasure

---

## Implementation Plan (AnyView)

### Phase 1: Create AnyView (Week 1)

```rust
// File: crates/flui-view/src/any_view.rs
pub struct AnyView {
    type_id: TypeId,
    debug_name: &'static str,
    create: Arc<dyn Fn(&dyn Any) -> Box<dyn ViewObject> + Send + Sync>,
    view_data: Box<dyn Any + Send + Sync>,
}
```

### Phase 2: Update Child/Children (Week 2)

```rust
// File: crates/flui-view/src/children/child.rs
pub struct Child {
    inner: Option<AnyView>,  // Changed from ViewObject
}
```

### Phase 3: Update Views for Clone (Week 3)

```rust
#[derive(Clone)]
pub struct Padding {
    padding: EdgeInsets,
    child: Child,
}
```

### Phase 4: Separate Mount/Build (Week 4)

```rust
impl Element {
    pub fn mount(&mut self) {
        // Create ViewObject from config
    }

    pub fn build(&mut self, ctx: &dyn BuildContext) {
        // Build children
    }
}
```

### Phase 5: Add Reconciliation (Week 5)

```rust
impl Element {
    pub fn reconcile(&mut self, new_config: AnyView) {
        if self.view_config.as_ref().map(|c| c.type_id) == Some(new_config.type_id) {
            // Update
        } else {
            // Replace
        }
    }
}
```

---

## Questions for User

1. **Do you agree that type erasure is unavoidable** for Child/Children with heterogeneous views?
2. **Does the AnyView approach address your concerns** about safety and correctness?
3. **Should we proceed with AnyView implementation**, or do you want to explore typestate further?

The key question is: What specific safety guarantees does typestate provide that AnyView + Element lifecycle doesn't?
