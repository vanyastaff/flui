# AI Agent Prompts for Generic Refactoring

Structured prompts for AI agents to perform FLUI generic architecture refactoring.

---

## System Prompt (Foundation)

```markdown
You are an expert Rust systems programmer specializing in zero-cost abstractions and type-safe API design.

Your task is to refactor the FLUI UI framework to use generic types instead of trait objects, ensuring:
1. **Compile-time safety**: Errors caught at compile time, not runtime
2. **Zero-cost abstractions**: No vtable overhead, full monomorphization
3. **Rust 1.90+ features**: Use const generics, GATs, LazyLock, etc.
4. **Crate independence**: Minimize dependencies between crates

**Core Principles:**
- Use `T: Trait` instead of `dyn Trait` wherever possible
- Type erasure ONLY at storage boundaries
- Const generics for compile-time validation
- Generic Associated Types (GATs) for flexible APIs
- No `unwrap()` or `panic!()` in release code
- All unsafe code must have SAFETY comments

**Architecture:**
```
flui_types (no deps)
    ↓
flui-foundation (→ types)
    ↓
flui-tree (→ foundation, types)
    ↓
flui_rendering (→ tree, foundation, types)
    ↓
flui_core (→ rendering, tree, ...)
```

**Key Types to Make Generic:**
1. `Arity` trait - child count validation
2. `Protocol` trait - Box vs Sliver layout
3. `RenderElement` - owns RenderObject
4. `RenderTree` - generic over storage

Work incrementally, test after each change, and ensure each crate compiles independently.
```

---

## Phase 1: flui-tree Const Generics

### Agent Prompt

```markdown
# Task: Add Const Generics to flui-tree Arity System

## Context
The `flui-tree` crate defines an `Arity` trait for compile-time child count validation. Currently it uses runtime checks. Refactor to use const generics for zero-cost compile-time validation.

## Current State (BAD)
```rust
// File: crates/flui-tree/src/arity/mod.rs
pub trait Arity {
    fn min(&self) -> usize;
    fn max(&self) -> Option<usize>;
    fn is_valid(&self, count: usize) -> bool;
}

pub enum RuntimeArity {
    Leaf,
    Single,
    Variable,
}
```

## Target State (GOOD)
```rust
// File: crates/flui-tree/src/arity/mod.rs
use std::marker::PhantomData;

/// Arity trait with const generics (Rust 1.82+)
pub trait Arity: 'static + Send + Sync + Copy {
    /// Minimum children (compile-time constant)
    const MIN: usize;

    /// Maximum children (None = unbounded)
    const MAX: Option<usize>;

    /// Compile-time validation
    #[inline]
    const fn is_valid(count: usize) -> bool {
        count >= Self::MIN &&
        Self::MAX.map_or(true, |max| count <= max)
    }

    /// Child accessor type (GAT)
    type Accessor<'a, T>: ChildrenAccess<'a, T>
    where
        T: 'a;
}

/// Zero children (Leaf node)
#[derive(Debug, Copy, Clone)]
pub struct Leaf;

impl Arity for Leaf {
    const MIN: usize = 0;
    const MAX: Option<usize> = Some(0);

    type Accessor<'a, T> = NoChildren
    where
        T: 'a;
}

/// Exactly one child
#[derive(Debug, Copy, Clone)]
pub struct Single;

impl Arity for Single {
    const MIN: usize = 1;
    const MAX: Option<usize> = Some(1);

    type Accessor<'a, T> = SingleChild<'a, T>
    where
        T: 'a;
}

/// Optional child (0 or 1)
#[derive(Debug, Copy, Clone)]
pub struct Optional;

impl Arity for Optional {
    const MIN: usize = 0;
    const MAX: Option<usize> = Some(1);

    type Accessor<'a, T> = OptionalChild<'a, T>
    where
        T: 'a;
}

/// Variable number of children (unbounded)
#[derive(Debug, Copy, Clone)]
pub struct Variable;

impl Arity for Variable {
    const MIN: usize = 0;
    const MAX: Option<usize> = None;

    type Accessor<'a, T> = VariableChildren<'a, T>
    where
        T: 'a;
}

/// Exact count (const generic)
#[derive(Debug, Copy, Clone)]
pub struct Exact<const N: usize>;

impl<const N: usize> Arity for Exact<N> {
    const MIN: usize = N;
    const MAX: Option<usize> = Some(N);

    type Accessor<'a, T> = ExactChildren<'a, T, N>
    where
        T: 'a;
}

/// Range of children (const generics)
#[derive(Debug, Copy, Clone)]
pub struct Range<const MIN: usize, const MAX: usize>;

impl<const MIN: usize, const MAX: usize> Arity for Range<MIN, MAX> {
    const MIN: usize = MIN;
    const MAX: Option<usize> = Some(MAX);

    type Accessor<'a, T> = RangeChildren<'a, T, MIN, MAX>
    where
        T: 'a;
}
```

## Child Accessor Trait
```rust
/// Generic child access trait
pub trait ChildrenAccess<'a, T> {
    /// Get iterator over children
    fn iter(&self) -> impl Iterator<Item = &'a T>;
}

/// No children accessor (Leaf)
pub struct NoChildren;

impl<'a, T> ChildrenAccess<'a, T> for NoChildren {
    fn iter(&self) -> impl Iterator<Item = &'a T> {
        std::iter::empty()
    }
}

/// Single child accessor
pub struct SingleChild<'a, T> {
    child: &'a T,
}

impl<'a, T> SingleChild<'a, T> {
    /// Get the single child (compile-time guaranteed)
    pub fn get(&self) -> &'a T {
        self.child
    }
}

impl<'a, T> ChildrenAccess<'a, T> for SingleChild<'a, T> {
    fn iter(&self) -> impl Iterator<Item = &'a T> {
        std::iter::once(self.child)
    }
}

/// Variable children accessor
pub struct VariableChildren<'a, T> {
    children: &'a [T],
}

impl<'a, T> ChildrenAccess<'a, T> for VariableChildren<'a, T> {
    fn iter(&self) -> impl Iterator<Item = &'a T> {
        self.children.iter()
    }
}

/// Optional child accessor
pub struct OptionalChild<'a, T> {
    child: Option<&'a T>,
}

impl<'a, T> OptionalChild<'a, T> {
    /// Get child if present
    pub fn get(&self) -> Option<&'a T> {
        self.child
    }
}

impl<'a, T> ChildrenAccess<'a, T> for OptionalChild<'a, T> {
    fn iter(&self) -> impl Iterator<Item = &'a T> {
        self.child.into_iter()
    }
}

/// Exact children accessor
pub struct ExactChildren<'a, T, const N: usize> {
    children: &'a [T; N],
}

impl<'a, T, const N: usize> ExactChildren<'a, T, N> {
    /// Get children as array (compile-time size)
    pub fn as_array(&self) -> &'a [T; N] {
        self.children
    }
}

impl<'a, T, const N: usize> ChildrenAccess<'a, T> for ExactChildren<'a, T, N> {
    fn iter(&self) -> impl Iterator<Item = &'a T> {
        self.children.iter()
    }
}

/// Range children accessor
pub struct RangeChildren<'a, T, const MIN: usize, const MAX: usize> {
    children: &'a [T],
}

impl<'a, T, const MIN: usize, const MAX: usize> ChildrenAccess<'a, T>
for RangeChildren<'a, T, MIN, MAX> {
    fn iter(&self) -> impl Iterator<Item = &'a T> {
        debug_assert!(
            self.children.len() >= MIN && self.children.len() <= MAX,
            "Range arity violation: expected {}-{}, got {}",
            MIN, MAX, self.children.len()
        );
        self.children.iter()
    }
}
```

## Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_arity() {
        assert_eq!(Leaf::MIN, 0);
        assert_eq!(Leaf::MAX, Some(0));
        assert!(Leaf::is_valid(0));
        assert!(!Leaf::is_valid(1));
    }

    #[test]
    fn test_single_arity() {
        assert_eq!(Single::MIN, 1);
        assert_eq!(Single::MAX, Some(1));
        assert!(!Single::is_valid(0));
        assert!(Single::is_valid(1));
        assert!(!Single::is_valid(2));
    }

    #[test]
    fn test_variable_arity() {
        assert_eq!(Variable::MIN, 0);
        assert_eq!(Variable::MAX, None);
        assert!(Variable::is_valid(0));
        assert!(Variable::is_valid(100));
        assert!(Variable::is_valid(usize::MAX));
    }

    #[test]
    fn test_exact_arity() {
        assert_eq!(Exact::<3>::MIN, 3);
        assert_eq!(Exact::<3>::MAX, Some(3));
        assert!(!Exact::<3>::is_valid(2));
        assert!(Exact::<3>::is_valid(3));
        assert!(!Exact::<3>::is_valid(4));
    }

    #[test]
    fn test_range_arity() {
        assert_eq!(Range::<1, 5>::MIN, 1);
        assert_eq!(Range::<1, 5>::MAX, Some(5));
        assert!(!Range::<1, 5>::is_valid(0));
        assert!(Range::<1, 5>::is_valid(3));
        assert!(!Range::<1, 5>::is_valid(6));
    }

    #[test]
    fn test_const_evaluation() {
        const LEAF_MIN: usize = Leaf::MIN;
        const SINGLE_MAX: Option<usize> = Single::MAX;

        assert_eq!(LEAF_MIN, 0);
        assert_eq!(SINGLE_MAX, Some(1));
    }
}
```

## Acceptance Criteria
- [ ] All Arity types use const generics
- [ ] GAT for Accessor type works
- [ ] Compile-time validation where possible
- [ ] All tests pass: `cargo test -p flui-tree`
- [ ] Builds cleanly: `cargo build -p flui-tree`
- [ ] No clippy warnings: `cargo clippy -p flui-tree -- -D warnings`
- [ ] rustfmt compliant: `cargo fmt -p flui-tree -- --check`

## Commands to Run
```bash
cd crates/flui-tree
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

## Success Verification
After implementation, this should compile:
```rust
use flui_tree::arity::{Leaf, Single, Exact};

// Compile-time constants
const LEAF_MIN: usize = Leaf::MIN;
const SINGLE_MAX: Option<usize> = Single::MAX;

// Generic function using arity
fn validate_arity<A: Arity>(count: usize) -> bool {
    A::is_valid(count)
}

// Const generic arity
type ExactThree = Exact<3>;
assert_eq!(ExactThree::MIN, 3);
```
```

---

## Phase 2: flui_rendering RenderElement<R, P, A>

### Agent Prompt

```markdown
# Task: Add Arity Parameter to RenderElement

## Context
`RenderElement` currently is `RenderElement<R, P>`. Add Arity as third parameter: `RenderElement<R, P, A>` for compile-time child count validation.

## Current State
```rust
// File: crates/flui_rendering/src/core/element.rs
pub struct RenderElement<R, P>
where
    R: RenderObject,
    P: Protocol,
{
    id: Option<ElementId>,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    render_object: R,
    state: RenderState<P>,
    lifecycle: RenderLifecycle,
    parent_data: Option<Box<dyn ParentData>>,
    debug_name: Option<&'static str>,
}
```

## Target State
```rust
// File: crates/flui_rendering/src/core/element.rs
use std::marker::PhantomData;
use flui_tree::arity::Arity;

/// Generic RenderElement with compile-time arity validation
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

    // Render object (concrete type - no dyn!)
    render_object: R,

    // Protocol state (concrete type - no dyn!)
    state: RenderState<P>,

    // Lifecycle
    lifecycle: RenderLifecycle,

    // Parent data (only dyn allowed here)
    parent_data: Option<Box<dyn ParentData>>,

    // Debug
    debug_name: Option<&'static str>,

    // Arity marker (zero-sized)
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
        let new_count = self.children.len() + 1;

        // Runtime check in debug mode
        debug_assert!(
            A::is_valid(new_count),
            "Arity violation: attempting to add child #{} but arity allows {}-{}",
            new_count,
            A::MIN,
            A::MAX.map_or("∞".to_string(), |m| m.to_string())
        );

        self.children.push(child);
    }

    /// Get children accessor (typed by arity)
    pub fn children_accessor(&self) -> A::Accessor<'_, ElementId> {
        A::create_accessor(&self.children)
    }

    /// Get render object (concrete type!)
    #[inline]
    pub fn render_object(&self) -> &R {
        &self.render_object
    }

    /// Get render object mutably (concrete type!)
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut R {
        &mut self.render_object
    }

    /// Get protocol state
    #[inline]
    pub fn state(&self) -> &RenderState<P> {
        &self.state
    }

    /// Get protocol state mutably
    #[inline]
    pub fn state_mut(&mut self) -> &mut RenderState<P> {
        &mut self.state
    }
}

// Debug impl
impl<R, P, A> fmt::Debug for RenderElement<R, P, A>
where
    R: RenderObject,
    P: Protocol,
    A: Arity,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderElement")
            .field("id", &self.id)
            .field("children_count", &self.children.len())
            .field("arity_min", &A::MIN)
            .field("arity_max", &A::MAX)
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}
```

## Type Erasure Boundary
```rust
// File: crates/flui_rendering/src/element_node.rs

use std::any::Any;
use std::fmt;
use flui_foundation::ElementId;
use crate::protocol::Protocol;
use crate::state::RenderState;
use crate::lifecycle::RenderLifecycle;

/// Type-erased interface for RenderElement storage
///
/// This is the ONLY place we use trait objects!
pub trait RenderElementNode<P: Protocol>: Any + Send + Sync + fmt::Debug {
    // Identity
    fn id(&self) -> Option<ElementId>;
    fn parent(&self) -> Option<ElementId>;
    fn children(&self) -> &[ElementId];

    // Lifecycle
    fn lifecycle(&self) -> RenderLifecycle;
    fn mount(&mut self, id: ElementId, parent: Option<ElementId>);
    fn unmount(&mut self);

    // Protocol state
    fn state(&self) -> &RenderState<P>;
    fn state_mut(&mut self) -> &mut RenderState<P>;

    // Downcast
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

/// Storage wrapper (type erasure at boundary)
pub enum ElementNodeStorage {
    Box {
        element: Box<dyn RenderElementNode<BoxProtocol>>,
    },
    Sliver {
        element: Box<dyn RenderElementNode<SliverProtocol>>,
    },
}
```

## Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use flui_tree::arity::{Leaf, Single, Variable};
    use crate::protocol::BoxProtocol;

    struct MockRenderObject;
    impl RenderObject for MockRenderObject {
        // ... minimal impl
    }

    #[test]
    fn test_leaf_element() {
        let element: RenderElement<MockRenderObject, BoxProtocol, Leaf> =
            RenderElement::new(MockRenderObject);

        assert_eq!(element.children.len(), 0);
    }

    #[test]
    #[should_panic] // Debug assertion fires
    #[cfg(debug_assertions)]
    fn test_leaf_cannot_have_children() {
        let mut element: RenderElement<MockRenderObject, BoxProtocol, Leaf> =
            RenderElement::new(MockRenderObject);

        element.add_child(ElementId::new(1)); // Should panic in debug
    }

    #[test]
    fn test_single_element() {
        let mut element: RenderElement<MockRenderObject, BoxProtocol, Single> =
            RenderElement::new(MockRenderObject);

        element.add_child(ElementId::new(1));
        assert_eq!(element.children.len(), 1);
    }

    #[test]
    fn test_variable_element() {
        let mut element: RenderElement<MockRenderObject, BoxProtocol, Variable> =
            RenderElement::new(MockRenderObject);

        for i in 0..10 {
            element.add_child(ElementId::new(i));
        }
        assert_eq!(element.children.len(), 10);
    }

    #[test]
    fn test_type_erasure() {
        let element: RenderElement<MockRenderObject, BoxProtocol, Single> =
            RenderElement::new(MockRenderObject);

        let erased: Box<dyn RenderElementNode<BoxProtocol>> = Box::new(element);

        // Can downcast back
        let concrete = erased.as_any()
            .downcast_ref::<RenderElement<MockRenderObject, BoxProtocol, Single>>()
            .unwrap();

        assert_eq!(concrete.children.len(), 0);
    }
}
```

## Acceptance Criteria
- [ ] RenderElement<R, P, A> compiles
- [ ] All tests pass
- [ ] Arity violations caught in debug mode
- [ ] RenderElementNode<P> trait works
- [ ] Type erasure via ElementNodeStorage works
- [ ] No clippy warnings
- [ ] Zero-sized PhantomData for A

## Commands
```bash
cd crates/flui_rendering
cargo build
cargo test
cargo clippy -- -D warnings
```
```

---

## Phase 3: flui_core TreeCoordinator

### Agent Prompt

```markdown
# Task: Create Generic TreeCoordinator in flui_core

## Context
Create `TreeCoordinator` to coordinate all 4 trees (View, Element, Render, Layer). Should be in `flui_core`, NOT in `flui_rendering`.

## File Structure
```
crates/flui_core/src/
├── tree_coordinator.rs (NEW)
├── element_tree.rs (update)
└── lib.rs (export)
```

## Implementation
```rust
// File: crates/flui_core/src/tree_coordinator.rs

use std::collections::HashSet;
use flui_foundation::ElementId;
use flui_rendering::{RenderTree, RenderTreeStorage};
use crate::element_tree::ElementTree;

/// Coordinates all four trees in FLUI architecture
pub struct TreeCoordinator {
    /// Element tree (owns RenderElements)
    element_tree: ElementTree,

    /// Render tree (adds dirty tracking)
    render_tree: RenderTree<ElementTree>,

    /// Root element
    root: Option<ElementId>,
}

impl TreeCoordinator {
    /// Create new coordinator
    pub fn new() -> Self {
        let element_tree = ElementTree::new();
        let render_tree = RenderTree::new(element_tree);

        Self {
            element_tree,
            render_tree,
            root: None,
        }
    }

    /// Full frame render pipeline
    pub fn render_frame(&mut self) -> Result<(), FrameworkError> {
        // 1. Build phase (Views → Elements)
        self.build_phase()?;

        // 2. Layout phase (Elements → Geometry)
        self.layout_phase()?;

        // 3. Paint phase (Geometry → Layers)
        self.paint_phase()?;

        // 4. Composite phase (Layers → GPU)
        self.composite_phase()?;

        Ok(())
    }

    /// Layout phase
    fn layout_phase(&mut self) -> Result<(), FrameworkError> {
        let root = self.root.ok_or(FrameworkError::NoRoot)?;

        // Flush all dirty layout elements
        self.render_tree.flush_layout(root)?;

        Ok(())
    }

    /// Paint phase
    fn paint_phase(&mut self) -> Result<(), FrameworkError> {
        let root = self.root.ok_or(FrameworkError::NoRoot)?;

        // Flush all dirty paint elements
        self.render_tree.flush_paint(root)?;

        Ok(())
    }

    // ... other methods
}
```

## ElementTree Implementation
```rust
// File: crates/flui_core/src/element_tree.rs

use slab::Slab;
use flui_foundation::ElementId;
use flui_rendering::{
    RenderElementNode, ElementNodeStorage,
    RenderTreeStorage, BoxProtocol, SliverProtocol,
    RenderElement, RenderObject,
};
use flui_tree::arity::Arity;
use flui_tree::TreeNav;

/// Element tree storage
pub struct ElementTree {
    /// Heterogeneous element storage
    nodes: Slab<ElementNodeStorage>,

    /// Root element ID
    root: Option<ElementId>,
}

impl ElementTree {
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Create box element with typed RenderObject and Arity
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

    /// Create sliver element
    pub fn create_sliver_element<R, A>(
        &mut self,
        render_object: R,
    ) -> ElementId
    where
        R: RenderObject + 'static,
        A: Arity + 'static,
    {
        let element = RenderElement::<R, SliverProtocol, A>::new(render_object);
        let storage = ElementNodeStorage::Sliver {
            element: Box::new(element),
        };
        let idx = self.nodes.insert(storage);
        ElementId::new(idx + 1)
    }
}

// Implement TreeNav
impl TreeNav for ElementTree {
    type Id = ElementId;

    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get_node(id).and_then(|node| node.parent())
    }

    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get_node(id)
            .map(|node| node.children())
            .unwrap_or(&[])
    }
}

// Implement RenderTreeStorage
impl RenderTreeStorage for ElementTree {
    fn element_box(&self, id: ElementId) -> Option<&dyn RenderElementNode<BoxProtocol>> {
        let idx = id.get() - 1;
        self.nodes.get(idx).and_then(|storage| match storage {
            ElementNodeStorage::Box { element } => Some(&**element),
            _ => None,
        })
    }

    fn element_box_mut(&mut self, id: ElementId) -> Option<&mut dyn RenderElementNode<BoxProtocol>> {
        let idx = id.get() - 1;
        self.nodes.get_mut(idx).and_then(|storage| match storage {
            ElementNodeStorage::Box { element } => Some(&mut **element),
            _ => None,
        })
    }

    fn element_sliver(&self, id: ElementId) -> Option<&dyn RenderElementNode<SliverProtocol>> {
        let idx = id.get() - 1;
        self.nodes.get(idx).and_then(|storage| match storage {
            ElementNodeStorage::Sliver { element } => Some(&**element),
            _ => None,
        })
    }

    fn element_sliver_mut(&mut self, id: ElementId) -> Option<&mut dyn RenderElementNode<SliverProtocol>> {
        let idx = id.get() - 1;
        self.nodes.get_mut(idx).and_then(|storage| match storage {
            ElementNodeStorage::Sliver { element } => Some(&mut **element),
            _ => None,
        })
    }
}
```

## Acceptance Criteria
- [ ] TreeCoordinator in flui_core (NOT flui_rendering)
- [ ] ElementTree implements RenderTreeStorage
- [ ] Typed creation methods work
- [ ] Full pipeline compiles
- [ ] Tests pass

## Commands
```bash
cd crates/flui_core
cargo build
cargo test
```
```

---

## Meta Prompt: How to Use These Prompts

```markdown
# How to Use AI Agent Prompts

## For Each Phase

1. **Read the System Prompt** - Understand the overall goals and constraints
2. **Execute Phase Prompt** - Work through one phase at a time
3. **Verify Success** - Run the acceptance criteria commands
4. **Commit Changes** - Commit after each successful phase
5. **Move to Next Phase** - Only proceed when current phase passes all tests

## Prompt Usage Pattern

```bash
# Phase 1: flui-tree
cat docs/AI_AGENT_PROMPTS.md | grep -A 500 "Phase 1: flui-tree"
# -> Give this to AI agent
# -> Agent implements const generics
# -> Run tests, verify, commit

# Phase 2: flui_rendering
cat docs/AI_AGENT_PROMPTS.md | grep -A 500 "Phase 2: flui_rendering"
# -> Give this to AI agent
# -> Agent adds Arity parameter
# -> Run tests, verify, commit

# Continue for all phases...
```

## Verification After Each Phase

```bash
# Build
cargo build -p <crate>

# Test
cargo test -p <crate>

# Lint
cargo clippy -p <crate> -- -D warnings

# Format
cargo fmt -p <crate> -- --check

# Commit
git add .
git commit -m "phase X: <description>"
git push
```

## Iterative Refinement

If a phase fails:
1. Read error messages carefully
2. Update prompt with clarifications
3. Re-run agent with updated prompt
4. Repeat until success criteria met

## Chain of Prompts

Execute in order:
1. System Prompt (context setting)
2. Phase 1 Prompt (flui-tree)
3. Phase 2 Prompt (flui_rendering)
4. Phase 3 Prompt (flui_core)
5. Verification Prompt (testing)

Each phase builds on previous phases - DO NOT skip!
```

---

## Additional Prompts

### Verification Prompt

```markdown
# Task: Verify Zero-Cost Abstractions

After completing all phases, verify that the generic refactoring achieved zero-cost abstractions.

## Commands to Run

```bash
# 1. Check assembly output (should see no vtable calls)
cargo asm flui_rendering::RenderElement::perform_layout --rust

# 2. Run benchmarks (should be equal or faster than before)
cargo bench --workspace

# 3. Check binary size (should not increase significantly)
cargo bloat --release -n 20

# 4. Verify monomorphization
RUSTFLAGS="-Z print-type-sizes" cargo +nightly build --release

# 5. Test compile-time errors
cargo test --test compile_fail
```

## Expected Results

### Assembly Output
Should see direct function calls, NOT:
```asm
call    qword ptr [rax + 16]  ; BAD: vtable call
```

Should see:
```asm
call    RenderPadding::perform_layout  ; GOOD: direct call
```

### Benchmark Results
```
Before: layout_1000_elements  250 µs
After:  layout_1000_elements  220 µs  (12% faster due to monomorphization)
```

### Type Size Report
```
RenderElement<RenderPadding, BoxProtocol, Single>: 128 bytes
RenderElement<RenderOpacity, BoxProtocol, Leaf>: 96 bytes
```

## Acceptance Criteria
- [ ] No vtable calls in hot paths
- [ ] Benchmarks equal or faster
- [ ] Binary size not significantly increased
- [ ] Compile-time errors work as expected
```

---

## Error Recovery Prompt

```markdown
# Task: Fix Compilation Errors

If compilation fails during refactoring, follow this systematic approach.

## Step 1: Categorize Error

Read error message and categorize:
- **Type mismatch**: Missing generic parameter
- **Trait not satisfied**: Missing trait bound
- **Lifetime issue**: Missing lifetime annotation
- **Borrow checker**: Concurrent mutable borrows

## Step 2: Common Fixes

### Type Mismatch
```rust
// Error: expected RenderElement<_, _, _>, found RenderElement<_, _>
// Fix: Add missing Arity parameter
- RenderElement<R, P>
+ RenderElement<R, P, A>
```

### Trait Not Satisfied
```rust
// Error: the trait bound `A: Arity` is not satisfied
// Fix: Add trait bound
- impl<R, P, A> RenderElement<R, P, A>
+ impl<R, P, A> RenderElement<R, P, A> where A: Arity
```

### Lifetime Issue
```rust
// Error: missing lifetime specifier
// Fix: Add lifetime to GAT
- type Accessor<T>;
+ type Accessor<'a, T> where T: 'a;
```

## Step 3: Incremental Compilation

Fix one error at a time:
```bash
# Fix first error
cargo build 2>&1 | head -50

# After fix, rebuild
cargo build

# Repeat until all errors fixed
```

## Step 4: Test After Each Fix

```bash
cargo test -p <crate>
cargo clippy -p <crate> -- -D warnings
```
```

---

**END OF AI AGENT PROMPTS**
