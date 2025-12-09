# FLUI Architecture Refactoring Plan
## Goal: True Flutter-like View → Element → RenderObject separation

## Current Problems

### 1. **Child stores ViewObject (state) instead of View (config)**
```rust
// ❌ Current (wrong)
pub struct Padding {
    pub child: Child,  // Child = Option<Box<dyn ViewObject>> - mutable state!
}
```

**Issues:**
- View layer is not immutable
- Cannot recreate child without recreating ViewObject
- No reconciliation/diffing possible
- Hot reload impossible

### 2. **No View storage mechanism**
```rust
// ❌ Current flow
Text::headline("Hello")     // View (config) - consumed immediately
  → .into_view()            // ViewObject (state) - stored in Child
  → Child stores state      // Config lost forever!
```

**What we need:**
```rust
// ✅ Target flow
Text::headline("Hello")     // View (config)
  → AnyView::new()          // Type-erased View storage
  → Child stores AnyView    // Config preserved!
  → .create_view_object()   // Create ViewObject when needed
```

---

## Phase 1: Introduce AnyView (Type-Erased View Storage)

### 1.1 Create `AnyView` abstraction

```rust
// crates/flui-view/src/any_view.rs

/// Type-erased view that can create ViewObject on demand.
///
/// Stores the original View configuration and can recreate ViewObject multiple times.
/// This enables hot-reload, reconciliation, and proper separation of concerns.
pub struct AnyView {
    /// Unique type ID for reconciliation
    type_id: TypeId,

    /// Human-readable debug name
    debug_name: &'static str,

    /// Factory to create ViewObject from stored configuration
    ///
    /// Arc allows cheap cloning for storing in multiple places
    /// Box<dyn Any> stores the actual View value
    create: Arc<dyn Fn(&dyn Any) -> Box<dyn ViewObject> + Send + Sync>,

    /// Stored View value (immutable configuration)
    view_data: Box<dyn Any + Send + Sync>,
}

impl AnyView {
    /// Create from any IntoView type.
    pub fn new<V: IntoView + Clone + 'static>(view: V) -> Self {
        let type_id = TypeId::of::<V>();
        let debug_name = std::any::type_name::<V>();

        Self {
            type_id,
            debug_name,
            create: Arc::new(|data| {
                let view = data.downcast_ref::<V>().unwrap().clone();
                view.into_view()
            }),
            view_data: Box::new(view),
        }
    }

    /// Create ViewObject from stored View configuration.
    pub fn create_view_object(&self) -> Box<dyn ViewObject> {
        (self.create)(self.view_data.as_ref())
    }

    /// Get TypeId for reconciliation.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Get debug name.
    pub fn debug_name(&self) -> &'static str {
        self.debug_name
    }

    /// Check if two AnyViews are compatible (same type).
    pub fn can_update(&self, other: &AnyView) -> bool {
        self.type_id == other.type_id
    }

    /// Get underlying View data for comparison.
    pub fn view_data(&self) -> &dyn Any {
        self.view_data.as_ref()
    }
}

impl Clone for AnyView {
    fn clone(&self) -> Self {
        Self {
            type_id: self.type_id,
            debug_name: self.debug_name,
            create: Arc::clone(&self.create),
            view_data: self.view_data.clone(),  // Requires Clone bound
        }
    }
}

impl std::fmt::Debug for AnyView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyView")
            .field("type", &self.debug_name)
            .finish()
    }
}
```

**Key points:**
- ✅ Stores original View configuration (immutable)
- ✅ Can create ViewObject multiple times
- ✅ TypeId enables reconciliation
- ✅ Arc makes cloning cheap

**Limitation:** Requires `View: Clone`. This is acceptable because:
- Views should be cheap to clone (like Flutter Widgets)
- Can use `Arc<ViewData>` for expensive fields

---

### 1.2 Update `Child` and `Children` to store `AnyView`

```rust
// crates/flui-view/src/children/child.rs

/// Optional single child wrapper.
///
/// Stores the View configuration, not the ViewObject state.
/// This enables hot-reload, reconciliation, and proper immutability.
pub struct Child {
    inner: Option<AnyView>,  // ✅ Stores View config, not state!
}

impl Child {
    /// Creates an empty child.
    pub const fn none() -> Self {
        Self { inner: None }
    }

    /// Creates a child from a view.
    pub fn new<V: IntoView + Clone + 'static>(view: V) -> Self {
        Self {
            inner: Some(AnyView::new(view)),
        }
    }

    /// Check if has child.
    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Take the AnyView out.
    pub fn take(&mut self) -> Option<AnyView> {
        self.inner.take()
    }

    /// Get reference to AnyView.
    pub fn as_ref(&self) -> Option<&AnyView> {
        self.inner.as_ref()
    }

    /// Create ViewObject from stored View.
    pub fn create_view_object(&self) -> Option<Box<dyn ViewObject>> {
        self.inner.as_ref().map(|v| v.create_view_object())
    }
}

impl Clone for Child {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()  // AnyView is cheaply cloneable (Arc inside)
        }
    }
}
```

```rust
// crates/flui-view/src/children/children.rs

/// Multiple children wrapper.
pub struct Children {
    inner: Vec<AnyView>,  // ✅ Stores View configs!
}

impl Children {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn push<V: IntoView + Clone + 'static>(&mut self, view: V) {
        self.inner.push(AnyView::new(view));
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &AnyView> {
        self.inner.iter()
    }

    /// Create ViewObjects from all stored Views.
    pub fn create_view_objects(&self) -> Vec<Box<dyn ViewObject>> {
        self.inner.iter()
            .map(|v| v.create_view_object())
            .collect()
    }
}

impl Clone for Children {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
}
```

**Benefits:**
- ✅ View configuration preserved
- ✅ Can recreate ViewObject anytime
- ✅ Clone is cheap (Arc inside AnyView)
- ✅ Ready for reconciliation

---

## Phase 2: Update View Layer (Padding, Text, etc.)

### 2.1 All Views must implement Clone

```rust
// crates/flui_widgets/src/basic/padding.rs

#[derive(Debug, Clone, Builder)]  // ✅ Add Clone
pub struct Padding {
    pub padding: EdgeInsets,
    pub child: Child,  // Child now stores AnyView
}

impl Padding {
    pub fn child<V: IntoView + Clone + 'static>(mut self, view: V) -> Self {
        self.child = Child::new(view);  // ✅ Stores AnyView, not ViewObject
        self
    }
}
```

```rust
// crates/flui_widgets/src/basic/text.rs

#[derive(Debug, Clone, Builder)]  // ✅ Add Clone
pub struct Text {
    pub data: String,
    pub size: f32,
    pub color: Color,
    // ...
}
```

**Note:** Most Views are already cheap to clone. For expensive fields, use `Arc<T>`.

---

## Phase 3: Update Element Mounting Flow

### 3.1 Element creates children from AnyView during mount

```rust
// crates/flui_core/src/pipeline/tree_coordinator.rs

impl TreeCoordinator {
    fn mount_element(&mut self, element: Element, parent_id: Option<ElementId>) -> ElementId {
        // 1. Mount element itself
        let id = self.elements.insert(element);

        // 2. Mount children from stored AnyViews
        self.mount_children(id);

        id
    }

    fn mount_children(&mut self, parent_id: ElementId) {
        // Get ViewObject from element
        let view_object = self.elements.get(parent_id)
            .and_then(|e| e.view_object());

        // For RenderView wrappers, children come from View.child field
        if let Some(wrapper) = view_object.downcast_ref::<RenderViewWrapper<_>>() {
            if let Some(view) = wrapper.view() {
                // Get child from View (now it's AnyView!)
                if let Some(child_any_view) = view.child_as_any_view() {
                    // Create ViewObject from AnyView
                    let child_view_object = child_any_view.create_view_object();

                    // Create child Element
                    let child_element = Element::from_view_object(child_view_object);

                    // Recursively mount
                    let child_id = self.mount_element(child_element, Some(parent_id));

                    // Add to parent's children
                    self.elements.get_mut(parent_id)
                        .unwrap()
                        .add_child(child_id);
                }
            }
        }
    }
}
```

**Key changes:**
- ✅ Children created from AnyView, not from pending_children
- ✅ Can recreate children anytime (hot-reload)
- ✅ Clear separation: View → AnyView → ViewObject → Element

---

## Phase 4: Implement Reconciliation (Widget Diffing)

### 4.1 Element.update() checks if View changed

```rust
// crates/flui-element/src/element.rs

impl Element {
    /// Update element with new View configuration.
    ///
    /// Returns true if children need to be reconciled.
    pub fn update(&mut self, new_view: &AnyView) -> bool {
        // Get current ViewObject
        let Some(view_object) = self.view_object_mut() else {
            return false;
        };

        // Check if types match (can update)
        let Some(current_any_view) = self.stored_view() else {
            // No stored view, must rebuild
            return true;
        };

        if !current_any_view.can_update(new_view) {
            // Different type, must rebuild
            return true;
        }

        // Same type, check if data changed
        // (This requires View to implement PartialEq or custom comparison)
        let needs_rebuild = self.compare_views(current_any_view, new_view);

        if needs_rebuild {
            // Store new view config
            self.set_stored_view(new_view.clone());

            // Recreate ViewObject
            let new_view_object = new_view.create_view_object();
            self.set_view_object(new_view_object);

            true  // Children need reconciliation
        } else {
            false  // No changes
        }
    }

    /// Reconcile children (Flutter-like updateChild).
    pub fn reconcile_children(&mut self, tree: &mut TreeCoordinator) {
        // Get new children from View
        let new_children = self.get_child_any_views();
        let old_children = self.children();

        // Reconciliation algorithm:
        // 1. Try to update matching children (same type & key)
        // 2. Remove old children that don't match
        // 3. Insert new children

        for (i, new_child_view) in new_children.iter().enumerate() {
            if let Some(old_child_id) = old_children.get(i) {
                let old_child = tree.elements.get_mut(*old_child_id).unwrap();

                if old_child.can_update(new_child_view) {
                    // Update existing child
                    old_child.update(new_child_view);
                } else {
                    // Different type, replace child
                    tree.remove_child(*old_child_id);
                    let new_child = tree.mount_from_any_view(new_child_view, self.id());
                    self.set_child(i, new_child);
                }
            } else {
                // New child, mount it
                let new_child = tree.mount_from_any_view(new_child_view, self.id());
                self.add_child(new_child);
            }
        }

        // Remove extra old children
        while self.children().len() > new_children.len() {
            let removed = self.children().pop().unwrap();
            tree.remove_child(removed);
        }
    }
}
```

**This enables:**
- ✅ Hot-reload (replace View, update Element)
- ✅ Efficient updates (only changed subtrees)
- ✅ Animation state preservation (Element reused)

---

## Phase 5: Update RenderView to not store children

### 5.1 RenderView trait remains simple

```rust
// RenderView only creates RenderObject, doesn't manage children
pub trait RenderView<P: Protocol, A: Arity>: Send + Sync + 'static {
    type RenderObject: RenderObjectFor<P, A>;

    fn create(&self) -> Self::RenderObject;
    fn update(&self, render: &mut Self.RenderObject) -> UpdateResult;
}
```

### 5.2 Children handled by Element layer

```rust
impl Padding {
    // ✅ View stores child as AnyView
    pub child: Child,  // Child = Option<AnyView>
}

impl RenderView<BoxProtocol, Optional> for Padding {
    fn create(&self) -> RenderPadding {
        RenderPadding::new(self.padding)
        // ✅ No child passed here!
    }
}

// ✅ Children mounted by Element.mount_children() from AnyView
```

---

## Phase 6: Migration Strategy

### 6.1 Step-by-step migration

1. **Add AnyView** (new file, no breaking changes)
2. **Add Child v2** alongside current Child (parallel implementation)
3. **Migrate Padding first** as proof-of-concept
4. **Test thoroughly** (hot-reload, reconciliation)
5. **Migrate remaining widgets** (Text, Center, etc.)
6. **Remove old Child/Children** implementations
7. **Update all examples**

### 6.2 Compatibility layer during migration

```rust
// Temporary bridge
impl Child {
    pub fn from_legacy(view_object: Box<dyn ViewObject>) -> Self {
        // Wrap ViewObject temporarily until migration complete
    }
}
```

---

## Expected Benefits

### 1. **Hot Reload** ✅
```rust
// Old View
let old_padding = Padding::all(10.0).child(Text::new("Old"));

// New View (hot reloaded)
let new_padding = Padding::all(20.0).child(Text::new("New"));

// Element.update(new_padding)
// - Compares old vs new padding value
// - Updates RenderPadding.padding
// - Reconciles child (Text)
// - Preserves animation state
```

### 2. **Efficient Updates** ✅
```rust
// Only changed subtrees rebuild
element.update(new_view);  // Smart diffing
// - Unchanged children: skipped
// - Changed children: updated
// - Removed children: unmounted
// - New children: mounted
```

### 3. **Animation State Preservation** ✅
```rust
// AnimatedOpacity keeps animation state across rebuilds
AnimatedOpacity { opacity: 0.5, child: Text::new("Fade") }
// Text content changes → Element reused → animation continues
```

### 4. **Clean Architecture** ✅
```
View (immutable config)
  ↓ AnyView (type-erased storage)
  ↓ create_view_object()
ViewObject (build logic)
  ↓ build()
Element (tree node + lifecycle)
  ↓ mount()
RenderObject (layout/paint)
```

---

## Implementation Timeline

### Week 1: Core Infrastructure
- [ ] Implement `AnyView`
- [ ] Add `Child::new_v2()` with AnyView
- [ ] Add `Children::new_v2()` with AnyView
- [ ] Write comprehensive tests

### Week 2: Element Layer
- [ ] Update `Element::mount()` to use AnyView
- [ ] Implement `Element::update()` reconciliation
- [ ] Implement `Element::reconcile_children()`
- [ ] Test with simple cases

### Week 3: Widget Migration
- [ ] Migrate Padding (proof of concept)
- [ ] Migrate Text
- [ ] Migrate Center, Align
- [ ] Migrate Container

### Week 4: Complex Widgets
- [ ] Migrate Row, Column, Flex
- [ ] Migrate Stack
- [ ] Migrate all remaining widgets
- [ ] Update all examples

### Week 5: Polish & Testing
- [ ] Remove old Child/Children
- [ ] Performance testing
- [ ] Hot reload testing
- [ ] Animation state testing
- [ ] Documentation

---

## Breaking Changes

### API Changes
```rust
// Old (removed)
impl Child {
    fn new<V: IntoView>(view: V) -> Self { ... }
}

// New (requires Clone)
impl Child {
    fn new<V: IntoView + Clone + 'static>(view: V) -> Self { ... }
}
```

**Impact:** All Views must implement `Clone`. This is acceptable because:
- Most Views already cheap to clone
- Can use `Arc<T>` for expensive fields
- Matches Flutter's Widget model

---

## Success Criteria

- [ ] All widgets use AnyView-based Child/Children
- [ ] Hot reload works (update View → Element updates)
- [ ] Reconciliation works (efficient subtree updates)
- [ ] Animation state preserved across rebuilds
- [ ] All tests pass
- [ ] Performance equal or better than current
- [ ] All examples work
- [ ] Documentation complete

---

## Next Steps

1. Review this plan
2. Get approval for breaking changes
3. Create feature branch `refactor/any-view-architecture`
4. Implement Phase 1 (AnyView)
5. Proof of concept with Padding
6. Full migration

---

**This is a significant refactoring but will give us:**
- ✅ True Flutter-like architecture
- ✅ Hot reload support
- ✅ Efficient reconciliation
- ✅ Animation state preservation
- ✅ Clean separation of concerns
- ✅ Future-proof foundation
