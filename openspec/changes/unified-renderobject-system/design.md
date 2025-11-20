# Unified RenderObject Architecture - Technical Design

## Context

### Problem Statement

FLUI's current render object system suffers from three critical issues:

1. **Runtime Arity Validation**: Child count validation happens at runtime with panics, losing the opportunity for compile-time safety.
2. **Boilerplate Code**: Every render object implements `arity()` method and manual validation logic.
3. **Type Erasure Loss**: Converting to `Box<dyn RenderObject>` loses arity information, requiring runtime checks or downcasting.

Current architecture:
```rust
// Current: Separate traits per arity, no compile-time validation
impl LeafRender for RenderText { ... }        // 0 children
impl SingleRender for RenderPadding { ... }   // 1 child
impl MultiRender for RenderFlex { ... }       // N children

// Problem: Adding child to RenderText is a runtime panic
render_text.push_child(child);  // Panics at runtime!
```

### Design Goals

1. **Compile-Time Safety**: Arity violations should be compile errors, not runtime panics.
2. **Zero-Cost Abstraction**: No performance overhead from type safety layer.
3. **Thread-Safe Design**: Explicit lock ordering with deadlock prevention verification.
4. **Backwards Compatibility**: Existing code patterns should migrate smoothly.
5. **Single Source of Truth**: Protocol and arity stored only once, not duplicated.

## High-Level Architecture

### Three-Tier System

```
┌─────────────────────────────────────────────────────┐
│  Arity System (Generic<A>)                          │
│  - Leaf, Optional, Single, Pair, Variable, etc.    │
│  - Compile-time validation via type parameters     │
│  - HasTypedChildren<A> for context access          │
└─────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────┐
│  Public Traits (Render<A>, SliverRender<A>)        │
│  - User-facing, generic over arity                 │
│  - Context provides A::Children<'_> accessor       │
│  - Downcast-rs for safe type recovery              │
└─────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────┐
│  Internal RenderObject<P, A> + Type Erasure        │
│  - Blanket impls from Render<A>                    │
│  - Safe wrappers with zero unsafe code             │
│  - DynRenderObject hides protocol/arity            │
└─────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────┐
│  RenderElement (Storage + Coordination)            │
│  - Stores protocol and arity as source of truth    │
│  - Wraps type-erased DynRenderObject               │
│  - Transactional children update API               │
└─────────────────────────────────────────────────────┘
```

### Example: Type-Safe Child Access

```rust
// Current (runtime validation):
impl SingleRender for RenderPadding {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let child = ctx.children.single()  // May panic if wrong count
            .expect("Padding requires exactly 1 child");
        // ...
    }
}

// New (compile-time validation):
impl Render<Single> for RenderPadding {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single>) -> Size {
        let child = ctx.children().single();  // Guaranteed safe, no unwrap!
        // ...
    }
}
```

## Arity System Design

### Core Concept

Arity is a **compile-time property** of a render object that specifies how many children it accepts.

```rust
pub trait Arity: Sealed + Send + Sync + 'static {
    type Children<'a>: ChildrenAccess;
    
    fn runtime_arity() -> RuntimeArity;
    fn validate_count(count: usize) -> bool;
    
    #[inline(always)]
    fn from_slice(children: &[ElementId]) -> Self::Children<'_> { ... }
}
```

### Arity Types Hierarchy

```
Arity
├── Leaf              RuntimeArity::Exact(0)
├── Optional          RuntimeArity::Optional
├── Exact<N>          RuntimeArity::Exact(N)
│   ├── Single        RuntimeArity::Exact(1)
│   ├── Pair          RuntimeArity::Exact(2)
│   └── Triple        RuntimeArity::Exact(3)
├── AtLeast<N>        RuntimeArity::AtLeast(N)
└── Variable          RuntimeArity::Variable
```

### Children Accessors (Type-Safe)

Each arity type provides a different accessor:

| Arity | Accessor | Key Methods |
|-------|----------|-------------|
| Leaf | NoChildren | - |
| Optional | OptionalChild | get(), is_some(), is_none(), map(), unwrap_or() |
| Single | FixedChildren<1> | single() |
| Pair | FixedChildren<2> | pair(), first(), second() |
| Variable | SliceChildren | iter(), get(i), first(), last() |

**Key Design Decision**: All accessors implement `ChildrenAccess` trait with common `as_slice()` and `len()` methods, but specialized methods (e.g., `single()`) only exist on appropriate types.

### Validation Strategy

**Compile Time:**
```rust
// Type system enforces Single arity
impl Render<Single> for RenderPadding { ... }
// Attempting impl Render<Variable> would be a different type
```

**Debug Time** (zero-cost in release):
```rust
// In from_slice():
#[inline(always)]
fn from_slice(children: &[ElementId]) -> Self::Children<'_> {
    debug_assert!(
        children.len() == 1,
        "Single expects exactly 1 child, got {}",
        children.len()
    );
    FixedChildren { children: &[ElementId; 1] }  // Safe now
}
```

**Runtime** (only if explicitly requested):
```rust
// Via try_from_slice() for dynamic validation
if let Some(children) = Single::try_from_slice(&slice) {
    // Safe to use
} else {
    // Arity mismatch
}
```

## Protocol System Design

### Motivation

Layout systems differ significantly:
- **Box Protocol**: Standard 2D layout (constraints → size)
- **Sliver Protocol**: Scrollable content (sliver constraints → sliver geometry)

Rather than having separate trait hierarchies, we use a **protocol parameter** to abstract both.

```rust
pub trait Protocol: Sealed + Send + Sync + 'static {
    type Constraints: Clone + Debug + Default + Send + Sync;
    type Geometry: Clone + Debug + Default + Send + Sync;
    type LayoutContext<'a, A: Arity>: HasTypedChildren<A> + Debug;
    type PaintContext<'a, A: Arity>: HasTypedChildren<A> + Debug;
    type HitTestContext<'a, A: Arity>: HasTypedChildren<A> + Debug;
    type HitTestResult: Debug + Default;
    
    const ID: LayoutProtocol;
    const NAME: &'static str;
}
```

### Public Traits Implement Protocol Indirectly

```rust
// Users never see Protocol trait directly
pub trait Render<A: Arity>: Downcast + Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &mut BoxLayoutContext<'_, A>) -> Size;
    fn paint(&self, ctx: &BoxPaintContext<'_, A>) -> Canvas;
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, A>) -> bool;
}

// Internal blanket impl connects public to protocol-agnostic layer
impl<A, R> RenderObject<BoxProtocol, A> for R
where
    A: Arity,
    R: Render<A>,
{ ... }
```

## Type Erasure Design (Critical Section)

### The Challenge

After creating a render object, we need to:
1. Store it (type-erased, in a trait object)
2. Dispatch to correct layout/paint method
3. Know the arity for child validation
4. Maintain thread safety with proper lock ordering

**Problem**: `Box<dyn DynRenderObject>` can't carry arity information at runtime.

### Solution: Source of Truth in RenderElement

```rust
pub struct RenderElement {
    render_object: Box<dyn DynRenderObject>,
    protocol: LayoutProtocol,        // Source of truth
    arity: RuntimeArity,             // Source of truth
    children: Vec<ElementId>,
    // ...
}
```

**Key Design Decision**: Protocol and arity are stored **only in RenderElement**, not duplicated in the render object itself or wrapper.

```rust
// Safe wrappers implement DynRenderObject WITHOUT protocol/arity
impl<A, R> DynRenderObject for BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,  // Erased constraint type
    ) -> DynGeometry {  // Erased geometry type
        // Validation happens here, with debug_assert (zero cost in release)
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {}, got {}",
            self.inner.debug_name(),
            A::runtime_arity(),
            children.len()
        );
        
        // Create typed context and delegate
        let mut ctx = BoxLayoutContext::<A>::new(tree, children, *constraints.as_box());
        DynGeometry::Box(self.inner.layout(&mut ctx))
    }
    // ...
}
```

## RenderElement Transactional API

### Problem: Batch Updates

Sometimes we need to remove children and add new ones atomically:

```rust
// Reconciliation during rebuild
element.remove_child(old_child_1);  // Temporarily violates arity!
element.remove_child(old_child_2);
element.push_child(new_child_1);
element.push_child(new_child_2);
```

For Single arity, removing the child first makes the element invalid temporarily.

### Solution: Transactional API

```rust
// Disable validation during transaction
element.begin_children_update();
{
    // All operations skip arity checks (intermediate state allowed)
    element.remove_child(old);
    element.push_child(new);
}
// Validate final state
element.commit_children_update();  // Panics if final state invalid
```

**Key Insight**: The `updating_children` flag allows temporary violation of invariants during complex operations, with final validation ensuring correctness.

## Thread Safety & Lock Ordering

### Lock Hierarchy

FLUI uses two locks per element:

```rust
pub struct RenderElement {
    render_object: RwLock<Box<dyn DynRenderObject>>,
    render_state: RwLock<RenderState>,
    // ...
}
```

**Rule**: Always acquire `render_object` lock BEFORE `render_state` lock.

```rust
// ✅ Correct
let _obj = element.render_object.write();      // Lock 1
let _state = element.render_state.write();     // Lock 2

// ❌ Wrong (deadlock possible)
let _state = element.render_state.write();     // Lock 2 first!
let _obj = element.render_object.write();      // Lock 1 second (DEADLOCK)
```

### Why This Matters

```
Thread A                          Thread B
─────────────────────────────────────────────
acquire render_object   (L1)      acquire render_state (L2)
wait for render_state   (L2)      wait for render_object (L1)
    └─────── DEADLOCK ───────┘
```

### Documentation & Verification

1. **Inline Comments**: Every lock acquisition has explicit comments documenting order
2. **Loom Tests**: Concurrent tests verify correct order completes, incorrect order deadlocks
3. **Code Review**: Lock ordering checked during code review

## Performance Considerations

### Debug Assertions Zero Cost in Release

```rust
// In wrapper dyn_layout:
debug_assert!(
    A::validate_count(children.len()),
    "Arity violation: ...",
);
```

In release builds, this compiles to:
```rust
// (completely removed by compiler)
```

**Verification**: Benchmarks show identical performance to hand-rolled validation.

### Inline Accessors

```rust
#[inline(always)]
pub fn single(&self) -> ElementId {
    self.children[0]
}
```

Forces inlining, enabling further optimizations in caller.

### Lock-Free Flags

```rust
pub fn needs_layout(&self) -> bool {
    self.flags().needs_layout()
}
```

Where `flags()` returns `&AtomicRenderFlags`, read is lock-free atomic.

## Migration Path

### Phase 1: New API Side-by-Side

Both old and new traits coexist:
```rust
// Old API (deprecated)
impl LeafRender for RenderText { ... }

// New API (preferred)
impl Render<Leaf> for RenderText { ... }
```

### Phase 2: Migrate Leaf Widgets

Start with simple, leaf-only widgets (no children):
- RenderText
- RenderImage
- RenderSpacer

### Phase 3: Migrate Composite Widgets

Then move to widgets with children:
- RenderPadding (Single)
- RenderFlex (Variable)
- RenderSizedBox (Optional)

### Phase 4: Remove Old API

Once all widgets migrated, delete old LeafRender/SingleRender/MultiRender traits.

## Alternative Designs Considered

### 1. Keep Separate Traits Per Arity (Rejected)

```rust
impl Render0 for RenderText { ... }  // 0 children
impl Render1 for RenderPadding { ... }  // 1 child
impl RenderN for RenderFlex { ... }  // N children
```

**Reason**: More boilerplate, less elegant, doesn't solve type erasure problem.

### 2. Use Generic Associated Types (Rejected)

```rust
pub trait Render {
    type Arity: Arity;
    type Children<'a>: ChildrenAccess;
    // ...
}
```

**Reason**: More complex trait bounds, harder to reason about, not significantly clearer.

### 3. Store Protocol/Arity in Wrapper (Rejected)

```rust
pub struct BoxRenderObjectWrapper<A, R> {
    inner: R,
    protocol: LayoutProtocol,  // Redundant!
    arity: RuntimeArity,       // Redundant!
}
```

**Reason**: Violates single source of truth principle, allows inconsistency.

## Implementation Sequence

### Critical Path

1. Arity system (Phase 1)
2. Protocol + traits (Phase 2)
3. Type erasure + RenderElement (Phase 5)
4. ElementTree integration (Phase 6)
5. Render objects migration (Phase 7)

### Non-Critical Path

- Testing (Phase 8)
- Benchmarking (Phase 9)
- Documentation (Phase 10)

These can run parallel to phases 1-5 and accelerate phase 6-7.

## Risk Mitigation

### Risk: Performance Regression

**Mitigation**:
- Benchmarks at each phase
- debug_assert validation zero-cost verification
- Profile-guided optimization during phase 9

### Risk: Lock Ordering Deadlocks

**Mitigation**:
- Loom tests in phase 5
- Code review checklist for lock ordering
- CI validates lock order tests pass

### Risk: Migration Complexity

**Mitigation**:
- Side-by-side API support during phases 1-7
- Automated migration script for mechanical changes
- Comprehensive examples and migration guide

### Risk: Test Coverage Gaps

**Mitigation**:
- Property-based tests for arity combinations
- Integration tests for full pipeline
- 100% coverage requirement before phase 11

## Success Criteria

- ✅ All render objects migrated
- ✅ Benchmarks show ≤10% regression
- ✅ No unsafe code in wrappers
- ✅ Loom tests pass
- ✅ 100% test coverage for new code
- ✅ Documentation complete
- ✅ Examples demonstrate all arity types
- ✅ Performance profile shows zero overhead in release

## Open Questions

1. **Should we support const arity validation?** (Rust 1.91.0 const generics are stable)
   - Decided: Not initially, focus on runtime validation with compile-time type system

2. **Should we use GAT for contexts?** (Currently uses concrete types)
   - Decided: No, concrete types simpler and work well with arity parameter

3. **Should protocol be part of element creation or render object?**
   - Decided: RenderElement stores protocol, enables flexibility in future

4. **Should we support custom arity types?** (Beyond Leaf, Optional, Single, Variable, AtLeast)
   - Decided: No, fixed set is sufficient, covers all Flutter use cases

---

**Document**: Unified RenderObject Architecture - Technical Design  
**Status**: Proposal (awaiting approval)  
**Version**: 1.0  
**Last Updated**: November 2025
