# Wrapper Pattern Design for flui_rendering

## Goals

Allow users to write clean RenderBox implementations like:

```rust
pub struct Padding {
    pub padding: EdgeInsets,
    // ❌ NO size field!
    // ❌ NO child_id field!
}

impl RenderBox for Padding {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) {
        let child_size = ctx.layout_child(0, child_constraints);
        ctx.position_child(0, offset);
        ctx.complete_with_size(size);
    }

    fn paint(&mut self, ctx: &mut BoxPaintContext<Single, BoxParentData>) {
        ctx.paint_child(0);
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<Single, BoxParentData>) -> bool {
        ctx.hit_test_child(0, position)
    }
}
```

## Architecture

### Three Layers

```
┌─────────────────────────────────────────────┐
│  User Implementation (Padding, Center...)   │  ← Clean business logic
│  impl RenderBox for Padding                 │     NO infrastructure
└─────────────────────────────────────────────┘
                    ▲
                    │ wrapped by
                    │
┌─────────────────────────────────────────────┐
│  BoxRenderObject<T: RenderBox>              │  ← Infrastructure
│  - size: Size                               │     wrapper
│  - children: ChildStorage<A, PD>            │
│  - parent, owner, needs_layout, etc.        │
│  impl RenderObject                          │
└─────────────────────────────────────────────┘
                    ▲
                    │ integrates with
                    │
┌─────────────────────────────────────────────┐
│  RenderTree / PipelineOwner                 │  ← Framework
│  - Stores RenderObjects                     │
│  - Drives layout/paint pipeline             │
└─────────────────────────────────────────────┘
```

### Trait Separation

1. **RenderObject** - Protocol-agnostic lifecycle trait
   - Tree structure: parent, depth, attach/detach
   - Dirty state: needs_layout, needs_paint, mark_needs_*
   - Lifecycle callbacks: attach(), detach(), dispose()
   - ❌ NO layout() - protocol-specific!
   - ❌ NO paint() - protocol-specific!
   - ❌ NO BoxConstraints - protocol-specific!

2. **RenderBox** - Box protocol trait
   - Associated types: Arity, ParentData
   - Protocol methods: perform_layout(), paint(), hit_test()
   - Context-based API
   - ❌ NO infrastructure fields

3. **RenderSliver** - Sliver protocol trait
   - Similar to RenderBox but for scrollable content

## Wrapper Implementation

### BoxRenderObject<T: RenderBox>

```rust
pub struct BoxRenderObject<T: RenderBox> {
    // ════════════════════════════════════════
    // Core
    // ════════════════════════════════════════

    /// The actual RenderBox implementation
    inner: T,

    /// Size computed during layout
    size: Size,

    // ════════════════════════════════════════
    // Children
    // ════════════════════════════════════════

    /// Children storage (type-safe based on Arity)
    children: ChildStorage<T::Arity, T::ParentData>,

    // ════════════════════════════════════════
    // Tree Structure
    // ════════════════════════════════════════

    /// Parent render object
    parent: Option<*const dyn RenderObject>,

    /// Depth in render tree
    depth: usize,

    /// Pipeline owner
    owner: Option<*const PipelineOwner>,

    // ════════════════════════════════════════
    // Dirty State
    // ════════════════════════════════════════

    needs_layout: bool,
    needs_paint: bool,
    needs_compositing_bits_update: bool,

    // ════════════════════════════════════════
    // Layout
    // ════════════════════════════════════════

    cached_constraints: Option<BoxConstraints>,
    is_relayout_boundary: bool,

    // ════════════════════════════════════════
    // Paint
    // ════════════════════════════════════════

    is_repaint_boundary: bool,
    was_repaint_boundary: bool,
    needs_compositing: bool,
    layer_id: Option<LayerId>,
}

impl<T: RenderBox> RenderObject for BoxRenderObject<T> {
    fn layout(&mut self, constraints: BoxConstraints, parent_uses_size: bool) {
        // Standard layout logic
        if !self.needs_layout && constraints == self.cached_constraints {
            return;
        }

        self.cached_constraints = Some(constraints);

        // Create context with access to children and size
        let mut ctx = BoxLayoutContext::new(
            constraints,
            &mut self.children,
            &mut self.size,
        );

        // Delegate to inner implementation
        self.inner.perform_layout(&mut ctx);

        self.needs_layout = false;
    }

    fn paint(&self, context: &mut CanvasContext, offset: Offset) {
        // Create paint context
        let mut ctx = BoxPaintContext::new(
            context,
            offset,
            &self.children,
            self.size,
        );

        // Delegate to inner implementation
        self.inner.paint(&mut ctx);
    }

    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        let mut ctx = BoxHitTestContext::new(
            result,
            position,
            &self.children,
            self.size,
        );

        self.inner.hit_test(&mut ctx)
    }

    // ... all other RenderObject trait methods
}
```

### ChildStorage<A: Arity, PD: ParentData>

Type-safe child storage based on arity:

```rust
pub enum ChildStorage<A: Arity, PD: ParentData> {
    /// No children (Leaf)
    Leaf(PhantomData<PD>),

    /// Exactly one child (Single)
    Single(Option<ChildHandle<PD>>),

    /// Zero or one child (Optional)
    Optional(Option<ChildHandle<PD>>),

    /// Variable children (Variable)
    Variable(Vec<ChildHandle<PD>>),
}

pub struct ChildHandle<PD: ParentData> {
    /// Reference to the child render object
    child: Box<dyn RenderObject>,

    /// Parent data stored on this child
    parent_data: PD,

    /// Cached size/geometry after layout
    geometry: Option<Size>,  // or SliverGeometry for slivers
}
```

## Context Implementation Changes

Contexts need access to wrapper's storage. Two approaches:

### Approach A: Borrow from Wrapper

```rust
pub struct BoxLayoutContext<'ctx, A: Arity, PD: ParentData> {
    constraints: BoxConstraints,
    children: &'ctx mut ChildStorage<A, PD>,
    size: &'ctx mut Size,
    complete: bool,
}

impl<'ctx, A: Arity, PD: ParentData> BoxLayoutContext<'ctx, A, PD> {
    pub fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        // Access children from wrapper
        self.children.get_mut(index).unwrap().layout(constraints)
    }

    pub fn position_child(&mut self, index: usize, offset: Offset) {
        // Store offset in parent data
        if let Some(pd) = self.children.get_parent_data_mut(index) {
            if let Some(box_pd) = pd.as_any_mut().downcast_mut::<BoxParentData>() {
                box_pd.offset = offset;
            }
        }
    }

    pub fn complete_with_size(&mut self, size: Size) {
        *self.size = size;
        self.complete = true;
    }
}
```

### Approach B: Pass Mutable References

```rust
// Context owns nothing, just provides API
pub struct BoxLayoutContext<'ctx> {
    // Internal state for API implementation
}

// Wrapper creates context and passes references
impl<T: RenderBox> BoxRenderObject<T> {
    fn layout(&mut self, constraints: BoxConstraints) {
        let mut ctx = BoxLayoutContext::with_state(
            constraints,
            &mut self.children,
            &mut self.size,
        );
        self.inner.perform_layout(&mut ctx);
    }
}
```

**Recommendation**: Approach A (borrow from wrapper) - cleaner, more idiomatic Rust.

## RenderObject Trait Refactoring

### Problem: Protocol-Specific Constraints

Current RenderObject uses `BoxConstraints` directly:
```rust
fn layout(&mut self, constraints: BoxConstraints, parent_uses_size: bool);
```

This is protocol-specific and won't work for slivers (which use `SliverConstraints`).

### Solution: Constraint Enum Dispatch

Use an enum to make RenderObject protocol-agnostic:

```rust
/// Protocol-agnostic constraint wrapper.
pub enum RenderConstraints {
    Box(BoxConstraints),
    Sliver(SliverConstraints),
}

pub trait RenderObject {
    // ✅ Keep: Tree structure
    fn parent(&self) -> Option<&dyn RenderObject>;
    fn depth(&self) -> usize;
    fn set_depth(&mut self, depth: usize);

    // ✅ Keep: Lifecycle
    fn attach(&mut self, owner: &PipelineOwner);
    fn detach(&mut self);
    fn dispose(&mut self);

    // ✅ Keep: Dirty state
    fn needs_layout(&self) -> bool;
    fn needs_paint(&self) -> bool;
    fn mark_needs_layout(&mut self);
    fn mark_needs_paint(&mut self);

    // ✅ Update: Protocol-agnostic via enum
    fn layout(&mut self, constraints: RenderConstraints, parent_uses_size: bool);
    fn paint(&self, context: &mut CanvasContext, offset: Offset);

    // ✅ Keep: Parent data (protocol-agnostic)
    fn setup_parent_data(&self, child: &mut dyn RenderObject);
    fn parent_data(&self) -> Option<&dyn ParentData>;

    // ✅ Keep: Children (protocol-agnostic)
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject));
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject));
}
```

### Wrapper Implementation with Enum Dispatch

Wrappers match on the enum and panic if wrong type:

```rust
impl<T: RenderBox> RenderObject for BoxRenderObject<T> {
    fn layout(&mut self, constraints: RenderConstraints, parent_uses_size: bool) {
        // Extract box constraints or panic
        let RenderConstraints::Box(box_constraints) = constraints else {
            panic!("BoxRenderObject received non-box constraints: {:?}", constraints);
        };

        // Standard layout logic
        if !self.needs_layout && Some(box_constraints) == self.cached_constraints {
            return;
        }

        self.cached_constraints = Some(box_constraints);

        // Create context with access to wrapper state
        let mut ctx = BoxLayoutContext::new(
            box_constraints,
            &mut self.children,
            &mut self.size,
        );

        // Delegate to inner implementation
        self.inner.perform_layout(&mut ctx);

        self.needs_layout = false;
    }

    fn paint(&self, canvas_ctx: &mut CanvasContext, offset: Offset) {
        let mut ctx = BoxPaintContext::new(
            canvas_ctx,
            offset,
            &self.children,
            self.size,
        );
        self.inner.paint(&mut ctx);
    }
}
```

### Benefits of Enum Approach

✅ **Protocol-agnostic**: RenderObject doesn't know about specific protocols
✅ **Type-safe**: Wrong constraint type causes panic (debug builds) or undefined behavior (release)
✅ **Object-safe**: Trait objects `&dyn RenderObject` work fine
✅ **Simple**: No complex associated types or generics
✅ **Extensible**: Adding new protocols just adds new enum variants

### Type Safety Note

The enum approach trades compile-time safety for runtime checks. This is acceptable because:
- Parents create children and know their protocol
- Mismatches indicate serious bugs in framework code
- Debug builds can panic to catch issues
- Matches Flutter's approach (Dart is dynamically typed)

## Migration Path

1. **Phase 2a**: Create wrapper types
   - Implement `BoxRenderObject<T>` and `SliverRenderObject<T>`
   - Update contexts to work with wrapper storage
   - Keep old RenderObject trait temporarily

2. **Phase 2b**: Refactor RenderObject trait
   - Remove protocol-specific methods
   - Add lifecycle hooks
   - Update wrappers to use new trait

3. **Phase 2c**: Update examples
   - Migrate Padding to new pattern
   - Create other simple examples
   - Verify clean API

4. **Phase 3**: Update all existing implementations
   - Migrate RenderBox implementations
   - Migrate RenderSliver implementations
   - Update tests

## Benefits

✅ **Clean separation**: RenderObject is protocol-agnostic
✅ **Type safety**: Arity enforced at compile time
✅ **Simple implementations**: Users write minimal code
✅ **Single responsibility**: Each trait has one job
✅ **Flutter equivalence**: Matches Flutter's architecture
