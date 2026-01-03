# Variant 4: Trait Extension Pattern (No Macros)

## Core Idea

Separate **storage** (infrastructure) from **interface** (RenderObject API) using smart references and blanket implementations.

## Architecture

### Layer 1: User Implementation (Clean)

```rust
pub struct Padding {
    pub padding: EdgeInsets,
    // ❌ NO infrastructure!
}

impl RenderBox for Padding {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) {
        // Pure business logic
    }

    fn paint(&mut self, ctx: &mut BoxPaintContext<Single, BoxParentData>) {
        // Pure rendering logic
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<Single, BoxParentData>) -> bool {
        // Pure hit testing logic
    }
}
```

### Layer 2: Storage (Framework-Managed)

Infrastructure is stored in the render tree, NOT in user structs.

```rust
/// Storage node in render tree
pub struct RenderNode {
    // Protocol-agnostic infrastructure
    id: RenderId,
    depth: usize,
    parent: Option<RenderId>,
    owner: Option<*const PipelineOwner>,
    needs_layout: bool,
    needs_paint: bool,
    needs_compositing_bits_update: bool,
    parent_data: Option<Box<dyn ParentData>>,

    // Protocol-specific data (enum for different protocols)
    protocol_data: ProtocolData,

    // Type-erased implementation
    implementation: Box<dyn ProtocolImpl>,
}

/// Protocol-specific data storage
pub enum ProtocolData {
    Box(BoxProtocolData),
    Sliver(SliverProtocolData),
}

pub struct BoxProtocolData {
    size: Size,
    children: ChildStorage</* arity unknown at this level */>,
    cached_constraints: Option<BoxConstraints>,
    is_relayout_boundary: bool,
    // ... other box-specific infrastructure
}

pub struct SliverProtocolData {
    geometry: SliverGeometry,
    children: ChildStorage</* arity unknown */>,
    cached_constraints: Option<SliverConstraints>,
    // ... other sliver-specific infrastructure
}
```

### Layer 3: Protocol Implementation Trait (Type-Erased)

```rust
/// Type-erased protocol implementation
pub trait ProtocolImpl: Send + Sync {
    /// Performs layout with protocol-agnostic interface
    fn perform_layout_impl(
        &mut self,
        protocol_data: &mut ProtocolData,
        constraints: RenderConstraints,
    );

    /// Performs paint with protocol-agnostic interface
    fn perform_paint_impl(
        &mut self,
        protocol_data: &ProtocolData,
        context: &mut CanvasContext,
        offset: Offset,
    );

    /// Performs hit test with protocol-agnostic interface
    fn perform_hit_test_impl(
        &self,
        protocol_data: &ProtocolData,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool;

    /// Protocol name for debugging
    fn protocol_name(&self) -> &'static str;
}
```

### Layer 4: Blanket Implementation

Automatically implement ProtocolImpl for all RenderBox types:

```rust
/// Wrapper that implements ProtocolImpl for any RenderBox
pub struct BoxImpl<T: RenderBox> {
    inner: T,
    _phantom: PhantomData<(T::Arity, T::ParentData)>,
}

impl<T: RenderBox> ProtocolImpl for BoxImpl<T> {
    fn perform_layout_impl(
        &mut self,
        protocol_data: &mut ProtocolData,
        constraints: RenderConstraints,
    ) {
        // Extract box constraints
        let RenderConstraints::Box(box_constraints) = constraints else {
            panic!("BoxImpl received non-box constraints");
        };

        // Extract box protocol data
        let ProtocolData::Box(box_data) = protocol_data else {
            panic!("BoxImpl has non-box protocol data");
        };

        // Create context that borrows from box_data
        let mut ctx = BoxLayoutContext::new(
            box_constraints,
            &mut box_data.children,
            &mut box_data.size,
        );

        // Delegate to user implementation
        self.inner.perform_layout(&mut ctx);
    }

    fn perform_paint_impl(
        &mut self,
        protocol_data: &ProtocolData,
        canvas_ctx: &mut CanvasContext,
        offset: Offset,
    ) {
        let ProtocolData::Box(box_data) = protocol_data else {
            panic!("BoxImpl has non-box protocol data");
        };

        let mut ctx = BoxPaintContext::new(
            canvas_ctx,
            offset,
            &box_data.children,
            box_data.size,
        );

        self.inner.paint(&mut ctx);
    }

    fn perform_hit_test_impl(
        &self,
        protocol_data: &ProtocolData,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        let ProtocolData::Box(box_data) = protocol_data else {
            panic!("BoxImpl has non-box protocol data");
        };

        let mut ctx = BoxHitTestContext::new(
            result,
            position,
            &box_data.children,
            box_data.size,
        );

        self.inner.hit_test(&mut ctx)
    }

    fn protocol_name(&self) -> &'static str {
        "box"
    }
}
```

### Layer 5: RenderObject Implementation

RenderNode implements RenderObject:

```rust
impl RenderObject for RenderNode {
    fn layout(&mut self, constraints: RenderConstraints, parent_uses_size: bool) {
        if !self.needs_layout {
            return;
        }

        // Delegate to type-erased implementation
        self.implementation.perform_layout_impl(
            &mut self.protocol_data,
            constraints,
        );

        self.needs_layout = false;
    }

    fn paint(&self, context: &mut CanvasContext, offset: Offset) {
        // Delegate to type-erased implementation
        // SAFETY: ProtocolImpl is Send+Sync, we have &self
        let impl_mut = unsafe {
            &mut *(self.implementation.as_ref() as *const dyn ProtocolImpl as *mut dyn ProtocolImpl)
        };

        impl_mut.perform_paint_impl(
            &self.protocol_data,
            context,
            offset,
        );
    }

    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        self.implementation.perform_hit_test_impl(
            &self.protocol_data,
            result,
            position,
        )
    }

    // ... all other RenderObject methods
}
```

## Benefits of Variant 4

✅ **No macros** - Pure Rust, no proc macros
✅ **Clean user code** - No infrastructure in user structs
✅ **Type erasure** - Tree stores `RenderNode`, not generic types
✅ **Blanket impl** - Automatic ProtocolImpl for all RenderBox
✅ **Protocol-agnostic** - RenderNode and RenderObject are protocol-agnostic
✅ **Single storage** - Infrastructure centralized in RenderNode

## Construction API

Users never see RenderNode directly. Elements create them:

```rust
// In element layer:
impl Element for PaddingElement {
    fn create_render_object(&self) -> RenderNode {
        let padding = Padding {
            padding: self.padding.clone(),
        };

        RenderNode::new_box(padding)  // Helper that creates RenderNode
    }

    fn update_render_object(&self, node: &mut RenderNode) {
        // Downcast and update
        if let Some(padding) = node.as_box_impl_mut::<Padding>() {
            padding.padding = self.padding.clone();
        }
    }
}

impl RenderNode {
    /// Creates a new RenderNode for a box protocol implementation
    pub fn new_box<T: RenderBox>(implementation: T) -> Self {
        Self {
            id: RenderId::new(),
            depth: 0,
            parent: None,
            owner: None,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            parent_data: None,
            protocol_data: ProtocolData::Box(BoxProtocolData::new::<T::Arity, T::ParentData>()),
            implementation: Box::new(BoxImpl {
                inner: implementation,
                _phantom: PhantomData,
            }),
        }
    }

    /// Downcasts to specific RenderBox implementation
    pub fn as_box_impl<T: RenderBox>(&self) -> Option<&T> {
        self.implementation
            .as_any()
            .downcast_ref::<BoxImpl<T>>()
            .map(|wrapper| &wrapper.inner)
    }

    /// Downcasts to mutable specific RenderBox implementation
    pub fn as_box_impl_mut<T: RenderBox>(&mut self) -> Option<&mut T> {
        self.implementation
            .as_any_mut()
            .downcast_mut::<BoxImpl<T>>()
            .map(|wrapper| &mut wrapper.inner)
    }
}
```

## Challenges

### Challenge 1: Arity Type Erasure

`ChildStorage` is generic over `Arity`, but `ProtocolData` can't be generic.

**Solution**: Use enum for ChildStorage:

```rust
pub enum ChildStorage {
    Leaf,
    Single(Option<Box<RenderNode>>),
    Optional(Option<Box<RenderNode>>),
    Variable(Vec<Box<RenderNode>>),
}

impl ChildStorage {
    fn new_for_arity<A: Arity>() -> Self {
        // Use Arity trait methods to determine variant
        if A::MIN_CHILDREN == 0 && A::MAX_CHILDREN == Some(0) {
            ChildStorage::Leaf
        } else if A::MIN_CHILDREN == 1 && A::MAX_CHILDREN == Some(1) {
            ChildStorage::Single(None)
        } else if A::MIN_CHILDREN == 0 && A::MAX_CHILDREN == Some(1) {
            ChildStorage::Optional(None)
        } else {
            ChildStorage::Variable(Vec::new())
        }
    }
}
```

### Challenge 2: ParentData Type Erasure

ParentData is generic, but we need type erasure.

**Solution**: Already solved - use `Box<dyn ParentData>`

### Challenge 3: Mutable Access in Paint

Paint takes `&self` but needs to call `&mut self` on implementation for some cases.

**Solution**:
- Make paint take `&mut self` in RenderBox (correct approach)
- Or use interior mutability where needed

## Comparison with Wrapper Approach

| Aspect | Wrapper (BoxRenderObject<T>) | Variant 4 (RenderNode) |
|--------|------------------------------|------------------------|
| User code | Clean ✅ | Clean ✅ |
| Macros | No ✅ | No ✅ |
| Type erasure | Generic wrapper | Single RenderNode type ✅ |
| Tree storage | `Box<dyn RenderObject>` | `RenderNode` ✅ |
| Performance | One allocation per node | One allocation per node ✅ |
| Complexity | Medium | Higher (more indirection) |
| Flexibility | Less flexible | More flexible ✅ |

## Recommendation

**Variant 4** is better for:
- Unified storage (all nodes are RenderNode)
- No generic types in tree
- Easier to work with heterogeneous collections
- More aligned with Flutter's architecture

The main tradeoff is slightly more indirection (ProtocolImpl trait object), but this is negligible compared to benefits.
