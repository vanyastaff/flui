# Generic Architecture Refactoring Roadmap

**Goal:** Transform FLUI to zero-cost generic architecture with compile-time safety using Rust 1.90+ features.

**Principles:**
1. **Generic-first**: Use `T: Trait` instead of `dyn Trait` wherever possible
2. **Compile-time safety**: Catch errors at compile time, not runtime
3. **Zero-cost abstractions**: No vtable overhead, full monomorphization
4. **Crate independence**: Each crate should minimize dependencies
5. **Rust 1.90+**: Use latest features (const generics, GATs, impl Trait in assoc types)

---

## Phase 1: Foundation Layer (No Dependencies)

### 1.1 flui_types (Already Good ✓)

**Status:** ✅ Already generic and zero-cost

**No changes needed:**
- Size, Offset, Rect - all Copy types
- No trait objects
- Pure data structures

**Verify:**
```bash
cargo build -p flui_types
cargo test -p flui_types
```

---

### 1.2 flui-foundation (Already Good ✓)

**Status:** ✅ Mostly generic

**Review:**
- ElementId - NonZeroUsize (good!)
- ChangeNotifier - already generic

**Acceptance Criteria:**
- [ ] No `Box<dyn Trait>` in public API
- [ ] All types are `Copy` or cheap to clone
- [ ] Compiles with `--no-default-features`

---

## Phase 2: Tree Abstraction Layer

### 2.1 flui-tree (START HERE!)

**Current State:**
- ✅ Has `trait Arity`
- ❌ Runtime arity (RuntimeArity enum)
- ❌ No const generics for compile-time validation

**Tasks:**

#### Task 2.1.1: Add Const Generics to Arity Trait

**File:** `crates/flui-tree/src/arity/mod.rs`

**Implementation:**
```rust
/// Arity trait with const generics (Rust 1.82+)
pub trait Arity: 'static + Send + Sync {
    /// Minimum number of children (compile-time constant)
    const MIN: usize;

    /// Maximum number of children (None = unbounded)
    const MAX: Option<usize>;

    /// Returns true if count is valid
    #[inline]
    const fn is_valid(count: usize) -> bool {
        count >= Self::MIN &&
        Self::MAX.map_or(true, |max| count <= max)
    }

    /// Child accessor type (using GATs)
    type Accessor<'a, T>: ChildrenAccess<'a, T>
    where
        T: 'a;
}

/// Zero children (Leaf node)
pub struct Leaf;

impl Arity for Leaf {
    const MIN: usize = 0;
    const MAX: Option<usize> = Some(0);

    type Accessor<'a, T> = NoChildren
    where
        T: 'a;
}

/// Exactly one child
pub struct Single;

impl Arity for Single {
    const MIN: usize = 1;
    const MAX: Option<usize> = Some(1);

    type Accessor<'a, T> = SingleChild<'a, T>
    where
        T: 'a;
}

/// Optional child (0 or 1)
pub struct Optional;

impl Arity for Optional {
    const MIN: usize = 0;
    const MAX: Option<usize> = Some(1);

    type Accessor<'a, T> = OptionalChild<'a, T>
    where
        T: 'a;
}

/// Variable number of children
pub struct Variable;

impl Arity for Variable {
    const MIN: usize = 0;
    const MAX: Option<usize> = None;

    type Accessor<'a, T> = VariableChildren<'a, T>
    where
        T: 'a;
}

/// Exact count (const generic parameter)
pub struct Exact<const N: usize>;

impl<const N: usize> Arity for Exact<N> {
    const MIN: usize = N;
    const MAX: Option<usize> = Some(N);

    type Accessor<'a, T> = ExactChildren<'a, T, N>
    where
        T: 'a;
}

/// Range of children (const generic parameters)
pub struct Range<const MIN: usize, const MAX: usize>;

impl<const MIN: usize, const MAX: usize> Arity for Range<MIN, MAX> {
    const MIN: usize = MIN;
    const MAX: Option<usize> = Some(MAX);

    type Accessor<'a, T> = RangeChildren<'a, T, MIN, MAX>
    where
        T: 'a;
}
```

**Acceptance Criteria:**
- [ ] All arity types use const generics
- [ ] Compile-time validation where possible
- [ ] No runtime arity checks in release builds
- [ ] Full GAT support for child accessors
- [ ] Builds with Rust 1.90+

**Commands:**
```bash
cd crates/flui-tree
cargo build
cargo test
cargo clippy -- -D warnings
```

---

#### Task 2.1.2: Generic Tree Traits

**File:** `crates/flui-tree/src/traits/mod.rs`

**Implementation:**
```rust
/// Generic tree navigation (no trait objects!)
pub trait TreeNav {
    /// Element identifier type
    type Id: Copy + Eq + Hash;

    /// Get parent ID
    fn parent(&self, id: Self::Id) -> Option<Self::Id>;

    /// Get children IDs
    fn children(&self, id: Self::Id) -> &[Self::Id];

    /// Get child count
    #[inline]
    fn child_count(&self, id: Self::Id) -> usize {
        self.children(id).len()
    }
}

/// Generic tree read access
pub trait TreeRead: TreeNav {
    /// Element type stored in tree
    type Element;

    /// Read element immutably
    fn get(&self, id: Self::Id) -> Option<&Self::Element>;
}

/// Generic tree write access
pub trait TreeWrite: TreeRead {
    /// Write element mutably
    fn get_mut(&mut self, id: Self::Id) -> Option<&mut Self::Element>;

    /// Insert new element
    fn insert(&mut self, element: Self::Element) -> Self::Id;

    /// Remove element
    fn remove(&mut self, id: Self::Id) -> Option<Self::Element>;
}

/// RenderTree access with generics (no dyn Any!)
pub trait RenderTreeAccess<R, P>: TreeNav
where
    R: RenderObject,
    P: Protocol,
{
    /// Get render element immutably
    fn render_element(&self, id: Self::Id) -> Option<&RenderElement<R, P>>;

    /// Get render element mutably
    fn render_element_mut(&mut self, id: Self::Id) -> Option<&mut RenderElement<R, P>>;

    /// Get protocol state immutably
    fn render_state(&self, id: Self::Id) -> Option<&RenderState<P>>;

    /// Get protocol state mutably
    fn render_state_mut(&mut self, id: Self::Id) -> Option<&mut RenderState<P>>;
}
```

**Acceptance Criteria:**
- [ ] No `dyn Any` or `dyn Trait` in trait definitions
- [ ] Generic associated types (GATs) where needed
- [ ] All methods use concrete types at call sites
- [ ] Zero runtime overhead

---

## Phase 3: Rendering Layer

### 3.1 flui_rendering (IN PROGRESS)

**Current State:**
- ✅ RenderElement<R, P> - already generic!
- ❌ Missing Arity parameter
- ❌ RenderElementNode still uses dyn
- ❌ tree_storage.rs doesn't compile

**Tasks:**

#### Task 3.1.1: Add Arity Parameter to RenderElement

**File:** `crates/flui_rendering/src/core/element.rs`

**Implementation:**
```rust
/// Generic RenderElement with compile-time arity checking
pub struct RenderElement<R, P, A>
where
    R: RenderObject,
    P: Protocol,
    A: Arity,
{
    // Identity
    id: Option<ElementId>,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    depth: usize,

    // Render object (concrete type R - no dyn!)
    render_object: R,

    // Protocol state (concrete type P - no dyn!)
    state: RenderState<P>,

    // Lifecycle
    lifecycle: RenderLifecycle,

    // Parent data (only this can be dyn - set by parent)
    parent_data: Option<Box<dyn ParentData>>,

    // Debug
    debug_name: Option<&'static str>,

    // Arity (zero-sized type marker)
    _arity: PhantomData<A>,
}

impl<R, P, A> RenderElement<R, P, A>
where
    R: RenderObject,
    P: Protocol,
    A: Arity,
{
    /// Create new element with compile-time arity
    pub fn new(render_object: R) -> Self {
        Self {
            id: None,
            parent: None,
            children: Vec::new(),
            depth: 0,
            render_object,
            state: RenderState::<P>::new(),
            lifecycle: RenderLifecycle::Detached,
            parent_data: None,
            debug_name: None,
            _arity: PhantomData,
        }
    }

    /// Add child with compile-time validation
    pub fn add_child(&mut self, child: ElementId) {
        // Compile-time check if possible
        if A::MAX.is_some() {
            let new_count = self.children.len() + 1;
            debug_assert!(
                A::is_valid(new_count),
                "Arity violation: max {} children, trying to add {}",
                A::MAX.unwrap(),
                new_count
            );
        }
        self.children.push(child);
    }

    /// Get children with typed accessor
    pub fn children_accessor(&self) -> A::Accessor<'_, ElementId> {
        A::create_accessor(&self.children)
    }
}
```

**Acceptance Criteria:**
- [ ] RenderElement<R, P, A> compiles
- [ ] Arity violations caught at compile time where possible
- [ ] Debug assertions for runtime validation
- [ ] Zero-sized PhantomData for A

---

#### Task 3.1.2: Type Erasure Boundary

**File:** `crates/flui_rendering/src/element_node.rs`

**Strategy:** Type erasure only at storage boundary, NOT in core types.

**Implementation:**
```rust
/// Type-erased wrapper for heterogeneous storage
///
/// This is the ONLY place we use trait objects in rendering!
pub enum ElementNodeStorage {
    /// Box protocol with concrete types
    Box {
        /// Type-erased element (generic R, fixed P=BoxProtocol)
        element: Box<dyn RenderElementNode<BoxProtocol>>,
    },
    /// Sliver protocol with concrete types
    Sliver {
        /// Type-erased element (generic R, fixed P=SliverProtocol)
        element: Box<dyn RenderElementNode<SliverProtocol>>,
    },
}

/// Generic trait for type erasure (minimized interface!)
pub trait RenderElementNode<P: Protocol>: Any + Send + Sync + fmt::Debug {
    // Only methods needed for tree operations
    fn id(&self) -> Option<ElementId>;
    fn parent(&self) -> Option<ElementId>;
    fn children(&self) -> &[ElementId];

    // Lifecycle
    fn lifecycle(&self) -> RenderLifecycle;
    fn mount(&mut self, id: ElementId, parent: Option<ElementId>);
    fn unmount(&mut self);

    // State access (protocol-specific)
    fn state(&self) -> &RenderState<P>;
    fn state_mut(&mut self) -> &mut RenderState<P>;

    // Downcast support
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// Implement for all RenderElement<R, P, A>
impl<R, P, A> RenderElementNode<P> for RenderElement<R, P, A>
where
    R: RenderObject + 'static,
    P: Protocol + 'static,
    A: Arity + 'static,
{
    fn id(&self) -> Option<ElementId> {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn children(&self) -> &[ElementId] {
        &self.children
    }

    fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    fn mount(&mut self, id: ElementId, parent: Option<ElementId>) {
        self.id = Some(id);
        self.parent = parent;
        self.lifecycle = RenderLifecycle::Attached;
    }

    fn unmount(&mut self) {
        self.id = None;
        self.parent = None;
        self.lifecycle = RenderLifecycle::Detached;
    }

    fn state(&self) -> &RenderState<P> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut RenderState<P> {
        &mut self.state
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

**Key Design Decision:**
- Generic types (`RenderElement<R, P, A>`) used everywhere in business logic
- Type erasure (`Box<dyn RenderElementNode<P>>`) ONLY at storage boundary
- Protocol fixed at storage level (Box vs Sliver storage)
- RenderObject type erased via trait but protocol preserved

**Acceptance Criteria:**
- [ ] RenderElementNode trait has minimal interface
- [ ] Works with RenderElement<R, P, A> for any R, A
- [ ] Safe downcasting via as_any()
- [ ] Protocol preserved at type level

---

#### Task 3.1.3: Generic RenderTree Storage

**File:** `crates/flui_rendering/src/core/tree_storage.rs`

**Implementation:**
```rust
/// Generic RenderTree wrapper over any storage
pub struct RenderTree<T> {
    /// Generic storage (e.g., ElementTree)
    storage: T,

    /// Dirty tracking sets
    needs_layout: HashSet<ElementId>,
    needs_paint: HashSet<ElementId>,
    needs_compositing: HashSet<ElementId>,
    needs_semantics: HashSet<ElementId>,
}

/// Storage requirements (generic!)
pub trait RenderTreeStorage: TreeNav<Id = ElementId> {
    /// Get element node (protocol-specific)
    fn element_box(&self, id: ElementId) -> Option<&dyn RenderElementNode<BoxProtocol>>;
    fn element_box_mut(&mut self, id: ElementId) -> Option<&mut dyn RenderElementNode<BoxProtocol>>;

    fn element_sliver(&self, id: ElementId) -> Option<&dyn RenderElementNode<SliverProtocol>>;
    fn element_sliver_mut(&mut self, id: ElementId) -> Option<&mut dyn RenderElementNode<SliverProtocol>>;
}

impl<T: RenderTreeStorage> RenderTree<T> {
    /// Perform layout with generics
    fn layout_box<R, A>(&mut self, id: ElementId, constraints: BoxConstraints) -> Result<Size, RenderError>
    where
        R: RenderObject,
        A: Arity,
    {
        // Get element and downcast to concrete type
        let element = self
            .storage
            .element_box_mut(id)
            .and_then(|e| e.as_any_mut().downcast_mut::<RenderElement<R, BoxProtocol, A>>())
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // Call perform_layout on concrete RenderObject (no dyn!)
        let size = element.render_object_mut().perform_layout(
            id,
            constraints,
            &mut |child_id, child_constraints| {
                self.layout_box::<R, A>(child_id, child_constraints)
            },
        )?;

        // Cache result
        element.state_mut().set_size(size);
        element.state_mut().set_constraints(constraints);

        Ok(size)
    }
}
```

**Acceptance Criteria:**
- [ ] RenderTree<T> is fully generic over storage
- [ ] No concrete dependency on ElementTree
- [ ] Type erasure only via RenderElementNode trait
- [ ] Layout/paint use concrete types internally

---

#### Task 3.1.4: Context API (Generic!)

**File:** `crates/flui_rendering/src/core/context.rs`

**Implementation:**
```rust
/// Generic layout context
pub struct LayoutContext<'a, R, P, A, T>
where
    R: RenderObject,
    P: Protocol,
    A: Arity,
    T: RenderTreeStorage,
{
    tree: &'a mut T,
    element_id: ElementId,
    constraints: P::Constraints,
    children: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<(R, P)>,
}

impl<'a, R, P, A, T> LayoutContext<'a, R, P, A, T>
where
    R: RenderObject,
    P: Protocol,
    A: Arity,
    T: RenderTreeStorage,
{
    /// Layout single child (for Single arity)
    pub fn layout_child(&mut self, constraints: P::Constraints) -> Result<P::Geometry, RenderError>
    where
        A: Arity<MIN = 1, MAX = Some(1)>, // Compile-time check!
    {
        let child_id = self.children.single();
        self.tree.perform_layout(child_id, constraints)
    }

    /// Layout all children (for Variable arity)
    pub fn layout_children(&mut self, constraints: P::Constraints) -> Result<Vec<P::Geometry>, RenderError> {
        self.children
            .iter()
            .map(|child_id| self.tree.perform_layout(*child_id, constraints))
            .collect()
    }
}
```

**Acceptance Criteria:**
- [ ] LayoutContext is fully generic
- [ ] Arity constraints enforced at compile time
- [ ] No unsafe code
- [ ] Clean API for RenderObject implementors

---

## Phase 4: Core Framework Layer

### 4.1 flui_core

**Tasks:**

#### Task 4.1.1: Generic ElementTree

**File:** `crates/flui_core/src/element_tree.rs`

**Implementation:**
```rust
/// Generic element tree storage
pub struct ElementTree {
    /// Heterogeneous element storage (type-erased at boundary)
    nodes: Slab<ElementNodeStorage>,

    /// Root element
    root: Option<ElementId>,
}

impl TreeNav for ElementTree {
    type Id = ElementId;

    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get_node(id).and_then(|node| node.parent())
    }

    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get_node(id).map(|node| node.children()).unwrap_or(&[])
    }
}

impl RenderTreeStorage for ElementTree {
    fn element_box(&self, id: ElementId) -> Option<&dyn RenderElementNode<BoxProtocol>> {
        self.get_node(id).and_then(|node| match node {
            ElementNodeStorage::Box { element } => Some(&**element),
            _ => None,
        })
    }

    // ... similar for element_box_mut, element_sliver, etc.
}

// Helper to create typed elements
impl ElementTree {
    /// Create box element with concrete types
    pub fn create_box_element<R, A>(
        &mut self,
        render_object: R,
    ) -> ElementId
    where
        R: RenderObject + 'static,
        A: Arity + 'static,
    {
        let element = RenderElement::<R, BoxProtocol, A>::new(render_object);
        let storage = ElementNodeStorage::Box {
            element: Box::new(element),
        };
        let idx = self.nodes.insert(storage);
        ElementId::new(idx + 1)
    }
}
```

**Acceptance Criteria:**
- [ ] ElementTree implements RenderTreeStorage
- [ ] Type erasure only in storage (Slab)
- [ ] Typed creation methods for compile-time safety
- [ ] Efficient slab-based storage

---

#### Task 4.1.2: Generic TreeCoordinator

**File:** `crates/flui_core/src/tree_coordinator.rs`

**Implementation:**
```rust
/// Coordinates all four trees
pub struct TreeCoordinator {
    /// View tree (immutable)
    view_tree: ViewTree,

    /// Element tree (mutable, owns RenderElements)
    element_tree: ElementTree,

    /// Render tree wrapper (adds dirty tracking)
    render_tree: RenderTree<ElementTree>,

    /// Layer tree (for compositing)
    layer_tree: LayerTree,
}

impl TreeCoordinator {
    /// Full frame pipeline (generic!)
    pub fn render_frame(&mut self) -> Result<(), FrameworkError> {
        // 1. Build phase (Views → Elements)
        self.build_phase()?;

        // 2. Layout phase (Elements → RenderObjects)
        self.layout_phase()?;

        // 3. Paint phase (RenderObjects → Layers)
        self.paint_phase()?;

        // 4. Composite phase (Layers → GPU)
        self.composite_phase()?;

        Ok(())
    }

    /// Layout phase with type safety
    fn layout_phase(&mut self) -> Result<(), FrameworkError> {
        // Get dirty elements
        let dirty = self.render_tree.take_needs_layout();

        for id in dirty {
            // Determine protocol and dispatch
            if let Some(element) = self.element_tree.element_box(id) {
                // Box protocol layout
                self.layout_box_element(id)?;
            } else if let Some(element) = self.element_tree.element_sliver(id) {
                // Sliver protocol layout
                self.layout_sliver_element(id)?;
            }
        }

        Ok(())
    }
}
```

**Acceptance Criteria:**
- [ ] TreeCoordinator is protocol-agnostic
- [ ] Efficient dirty tracking
- [ ] Clear separation of concerns
- [ ] Type-safe dispatch

---

## Phase 5: Rust 1.90+ Features

### Features to Use:

#### 5.1 Const Generics (Stable 1.51+, improvements in 1.90+)

```rust
// Use for fixed arity
pub struct Exact<const N: usize>;
pub struct Range<const MIN: usize, const MAX: usize>;

// Const functions for validation
impl<const N: usize> Arity for Exact<N> {
    const MIN: usize = N;
    const MAX: Option<usize> = Some(N);

    const fn is_valid(count: usize) -> bool {
        count == N
    }
}
```

---

#### 5.2 Generic Associated Types (GATs) - Stable 1.65+

```rust
pub trait Arity {
    type Accessor<'a, T>: ChildrenAccess<'a, T>
    where
        T: 'a;
}
```

---

#### 5.3 impl Trait in Associated Types (TAIT) - Stable 1.75+

```rust
pub trait Protocol {
    type LayoutResult = impl Future<Output = Size>;
}
```

---

#### 5.4 Return Position impl Trait in Traits (RPITIT) - Stable 1.75+

```rust
pub trait RenderObject {
    fn layout(&mut self) -> impl Future<Output = Size> + '_;
}
```

---

#### 5.5 LazyLock / LazyCell - Stable 1.80+

```rust
// Replace OnceCell with std::sync::LazyLock
use std::sync::LazyLock;

static GLOBAL_CONFIG: LazyLock<Config> = LazyLock::new(|| {
    Config::load()
});
```

---

## Phase 6: Crate Independence

### Independence Rules:

1. **flui_types**: ZERO dependencies on other flui crates ✓
2. **flui-foundation**: Only depends on flui_types ✓
3. **flui-tree**: Only depends on foundation/types ✓
4. **flui_rendering**: Only depends on tree/foundation/types ✓
5. **flui_core**: Coordinates but doesn't infect lower layers

### Dependency Graph (Final):

```
flui_types (0 deps)
    ↓
flui-foundation (→ types)
    ↓
flui-tree (→ foundation, types)
    ↓
flui_rendering (→ tree, foundation, types)
    ↓
flui_core (→ rendering, tree, foundation, types)
    ↓
flui_widgets (→ core)
```

### Verify Independence:

```bash
# Each crate should build independently
cd crates/flui-tree
cargo build --no-default-features

cd crates/flui_rendering
cargo build --no-default-features

# Check dependency tree
cargo tree -p flui_rendering --depth 1
```

---

## Testing Strategy

### Unit Tests (Per Crate):

```bash
# Foundation
cargo test -p flui_types
cargo test -p flui-foundation

# Tree
cargo test -p flui-tree

# Rendering
cargo test -p flui_rendering

# Core
cargo test -p flui_core
```

### Integration Tests:

```rust
// Test generic RenderElement with concrete types
#[test]
fn test_render_element_generic() {
    let padding = RenderPadding::new(EdgeInsets::all(8.0));
    let element: RenderElement<RenderPadding, BoxProtocol, Single> =
        RenderElement::new(padding);

    // Compile-time arity check
    // element.add_child(child1); // OK
    // element.add_child(child2); // Compile error if MAX=1!
}
```

### Compile-Time Tests:

```rust
// Should NOT compile (arity violation)
#[test]
#[compile_fail]
fn test_arity_violation() {
    let element: RenderElement<_, BoxProtocol, Leaf> =
        RenderElement::new(my_object);

    element.add_child(child); // ERROR: Leaf cannot have children!
}
```

---

## Success Criteria

### Compile-Time Safety:
- [ ] Arity violations caught at compile time where possible
- [ ] Protocol mismatches caught at compile time
- [ ] No `unwrap()` or `expect()` in hot paths
- [ ] All `unsafe` code has SAFETY comments

### Runtime Safety:
- [ ] Debug assertions for runtime validation
- [ ] Proper error types (no panics)
- [ ] Thread-safe (Send + Sync)

### Performance:
- [ ] Zero-cost abstractions (check assembly with `cargo asm`)
- [ ] No vtable overhead in hot paths
- [ ] Monomorphization benefits visible in benchmarks

### Code Quality:
- [ ] No clippy warnings with `-D warnings`
- [ ] rustfmt compliant
- [ ] Documentation for all public items
- [ ] Examples for complex APIs

---

## Execution Order

### Week 1: Foundation
- [ ] Task 2.1.1: flui-tree const generics
- [ ] Task 2.1.2: flui-tree generic traits

### Week 2: Rendering Core
- [ ] Task 3.1.1: RenderElement<R, P, A>
- [ ] Task 3.1.2: Type erasure boundary
- [ ] Task 3.1.3: Generic RenderTree

### Week 3: Context & Storage
- [ ] Task 3.1.4: Context API
- [ ] Task 4.1.1: Generic ElementTree
- [ ] Task 4.1.2: TreeCoordinator

### Week 4: Testing & Polish
- [ ] Integration tests
- [ ] Compile-time tests
- [ ] Performance benchmarks
- [ ] Documentation

---

## Commands Reference

### Build & Test:
```bash
# Build all crates in dependency order
cargo build -p flui_types
cargo build -p flui-foundation
cargo build -p flui-tree
cargo build -p flui_rendering
cargo build -p flui_core

# Test with all features
cargo test --workspace --all-features

# Check for issues
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

### Verify Zero-Cost:
```bash
# Check assembly output
cargo asm flui_rendering::RenderElement::perform_layout --rust

# Run benchmarks
cargo bench -p flui_rendering

# Check binary size
cargo bloat --release -n 20
```

---

## References

- Rust 1.90+ Features: https://releases.rs/
- Const Generics: https://rust-lang.github.io/rfcs/2000-const-generics.html
- GATs: https://blog.rust-lang.org/2022/10/28/gats-stabilization.html
- Zero-Cost Abstractions: https://doc.rust-lang.org/book/ch13-00-functional-features.html

---

## Notes

- This is a MAJOR refactoring - expect 3-4 weeks of work
- Incremental commits are essential
- Each phase should compile before moving to next
- Use feature flags for gradual migration if needed
- Keep backward compatibility during transition

Good luck! 🚀
