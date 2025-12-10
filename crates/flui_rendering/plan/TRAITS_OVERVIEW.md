# FLUI Rendering Traits Overview

Based on analysis of Flutter rendering architecture and `impl/` documentation.

## Trait Hierarchy Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              CORE TRAITS                                         │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐           │
│  │   RenderState   │     │    Protocol     │     │   Constraints   │           │
│  │  (typestate)    │     │  (layout type)  │     │  (layout input) │           │
│  ├─────────────────┤     ├─────────────────┤     ├─────────────────┤           │
│  │ Detached        │     │ BoxProtocol     │     │ BoxConstraints  │           │
│  │ Attached        │     │ SliverProtocol  │     │ SliverConstraints│          │
│  │ LaidOut         │     └─────────────────┘     └─────────────────┘           │
│  │ Painted         │                                                            │
│  └─────────────────┘                                                            │
│                                                                                 │
│  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐           │
│  │    Geometry     │     │   ParentData    │     │ HitTestResult   │           │
│  │  (layout out)   │     │ (child data)    │     │  (hit testing)  │           │
│  ├─────────────────┤     ├─────────────────┤     ├─────────────────┤           │
│  │ Size            │     │ BoxParentData   │     │ BoxHitTestResult│           │
│  │ SliverGeometry  │     │ SliverParentData│     │ SliverHitTestRes│           │
│  └─────────────────┘     └─────────────────┘     └─────────────────┘           │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                           RENDER NODE TRAITS                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│                         ┌───────────────────┐                                   │
│                         │   RenderObject    │  (base behavior)                  │
│                         └─────────┬─────────┘                                   │
│                                   │                                             │
│              ┌────────────────────┼────────────────────┐                        │
│              │                    │                    │                        │
│              ▼                    ▼                    ▼                        │
│  ┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐             │
│  │  LayoutProtocol   │ │  PaintProtocol    │ │  HitTestProtocol  │             │
│  │  <P: Protocol>    │ │                   │ │  <P: Protocol>    │             │
│  ├───────────────────┤ ├───────────────────┤ ├───────────────────┤             │
│  │ perform_layout()  │ │ paint()           │ │ hit_test()        │             │
│  │ sized_by_parent() │ │ paint_bounds()    │ │ hit_test_self()   │             │
│  │ compute_dry_layout│ │ is_repaint_bound  │ │ hit_test_children │             │
│  └───────────────────┘ └───────────────────┘ └───────────────────┘             │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                           RENDER HANDLE                                          │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  ┌──────────────────────────────────────────────────────────────────────────┐  │
│  │  RenderHandle<P: Protocol, S: NodeState>                                 │  │
│  │    - render_object: Box<dyn RenderProtocol<P>>                           │  │
│  │    - depth: Depth                                                        │  │
│  │    - parent: Option<RenderId>                                            │  │
│  │                                                                          │  │
│  │  Implements Deref → dyn RenderProtocol<P>                                │  │
│  │  Allows: handle.perform_layout() directly (no .get() needed)             │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
│                                                                                 │
│  Type Aliases:                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │ BoxHandle<S>    = RenderHandle<BoxProtocol, S>                          │   │
│  │ SliverHandle<S> = RenderHandle<SliverProtocol, S>                       │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                           CHILD STORAGE TYPES                                    │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  Flutter-like types for storing children inside RenderObject:                   │
│                                                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  Child<P: Protocol>                                                     │   │
│  │    Flutter: RenderObjectWithChildMixin                                  │   │
│  │    Use for: Single child (Padding, Align, Transform)                    │   │
│  │    inner: Option<RenderHandle<P, Mounted>>                              │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  Children<P: Protocol, PD: ParentData = ()>                             │   │
│  │    Flutter: ContainerRenderObjectMixin                                  │   │
│  │    Use for: Multiple children (Flex, Stack, Wrap)                       │   │
│  │    items: Vec<(RenderHandle<P, Mounted>, ContainerParentData<PD>)>      │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  Slots<P: Protocol, S: SlotKey>                                         │   │
│  │    Flutter: SlottedContainerRenderObjectMixin                           │   │
│  │    Use for: Named slots (ListTile, InputDecorator)                      │   │
│  │    items: HashMap<S, (RenderHandle<P, Mounted>, Offset)>                │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
│  Key difference from Arity:                                                     │
│  - Children stored INSIDE RenderObject (like Flutter), not in separate tree    │
│  - Enables: self.child.perform_layout() directly                               │
│  - No need for tree lookup during layout                                       │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────────┐
│                           BASE STRUCT PATTERNS                                   │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  Base structs use Deref chain for method delegation:                            │
│                                                                                 │
│  ┌───────────────────┐     ┌───────────────────┐     ┌───────────────────┐     │
│  │  SingleChildBase  │ --> │  ShiftedBoxBase   │ --> │  AligningBoxBase  │     │
│  │   child: Child<P> │     │   child_offset    │     │   alignment       │     │
│  └───────────────────┘     └───────────────────┘     └───────────────────┘     │
│          │                          │                          │               │
│          │                          │                          │               │
│          ▼                          ▼                          ▼               │
│  ┌───────────────────┐     ┌───────────────────┐     ┌───────────────────┐     │
│  │   RenderOpacity   │     │   RenderPadding   │     │   RenderAlign     │     │
│  │    alpha: f32     │     │  padding: Insets  │     │  width_factor?    │     │
│  └───────────────────┘     └───────────────────┘     └───────────────────┘     │
│                                                                                 │
│  Deref enables: self.child.perform_layout() without boilerplate                 │
│                                                                                 │
│  ┌───────────────────┐     ┌───────────────────┐                               │
│  │   ProxyBoxBase    │     │   ContainerBase   │                               │
│  │   extends Single  │     │ children: Children│                               │
│  │   (delegates all) │     │  <P, PD>          │                               │
│  └───────────────────┘     └───────────────────┘                               │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## 1. Core Traits

### RenderHandle

Typestate handle for render objects with `Deref` for direct method calls:

```rust
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use flui_tree::{Depth, Mounted, Unmounted, NodeState};

/// Handle for render object with Protocol + NodeState typestate.
/// 
/// Implements Deref to call methods directly:
/// ```rust
/// // Instead of: handle.render_object().perform_layout(...)
/// // Just:       handle.perform_layout(...)
/// ```
pub struct RenderHandle<P: Protocol, S: NodeState> {
    render_object: Box<dyn RenderProtocol<P>>,
    depth: Depth,
    parent: Option<RenderId>,
    _marker: PhantomData<(P, S)>,
}

impl<P: Protocol, S: NodeState> Deref for RenderHandle<P, S> {
    type Target = dyn RenderProtocol<P>;
    fn deref(&self) -> &Self::Target {
        self.render_object.as_ref()
    }
}

impl<P: Protocol, S: NodeState> DerefMut for RenderHandle<P, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.render_object.as_mut()
    }
}

// State transitions
impl<P: Protocol> RenderHandle<P, Unmounted> {
    pub fn new<R: RenderProtocol<P> + 'static>(render: R) -> Self { ... }
    pub fn mount(self, parent: Option<RenderId>, depth: Depth) -> RenderHandle<P, Mounted> { ... }
}

impl<P: Protocol> RenderHandle<P, Mounted> {
    pub fn parent(&self) -> Option<RenderId> { ... }
    pub fn depth(&self) -> Depth { ... }
    pub fn unmount(self) -> RenderHandle<P, Unmounted> { ... }
    pub fn attach(&mut self) { ... }
    pub fn detach(&mut self) { ... }
}

// Type aliases
pub type BoxHandle<S> = RenderHandle<BoxProtocol, S>;
pub type SliverHandle<S> = RenderHandle<SliverProtocol, S>;
```

### RenderState (Typestate Markers)

```rust
mod sealed {
    pub trait Sealed {}
}

/// Marker trait for render lifecycle states.
pub trait RenderState: sealed::Sealed + Copy + Default + 'static {
    const IS_ATTACHED: bool;
    const IS_LAID_OUT: bool;
    const IS_PAINTED: bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Detached;

#[derive(Debug, Clone, Copy, Default)]
pub struct Attached;

#[derive(Debug, Clone, Copy, Default)]
pub struct LaidOut;

#[derive(Debug, Clone, Copy, Default)]
pub struct Painted;
```

### Protocol

```rust
/// Layout protocol (Box vs Sliver).
pub trait Protocol: sealed::Sealed + 'static {
    /// Constraints from parent.
    type Constraints: Constraints;
    
    /// Geometry computed during layout.
    type Geometry: Geometry;
    
    /// Data stored on child by parent.
    type ParentData: ParentData;
    
    /// Hit test result type.
    type HitTestResult: HitTestResultTrait;
    
    /// Hit test position type.
    type HitTestPosition: Copy;
    
    fn name() -> &'static str;
}

pub struct BoxProtocol;
pub struct SliverProtocol;
```

### Constraints

```rust
/// Layout input from parent to child.
pub trait Constraints: Clone + PartialEq + Debug + Send + Sync + 'static {
    /// Whether exactly one size satisfies these constraints.
    fn is_tight(&self) -> bool;
    
    /// Whether constraints are in canonical form.
    fn is_normalized(&self) -> bool;
}
```

### Geometry

```rust
/// Layout output from child to parent.
pub trait Geometry: Clone + Debug + Send + Sync + 'static {
    /// Check if geometry is valid.
    fn is_valid(&self) -> bool { true }
}

// Size implements Geometry for BoxProtocol
// SliverGeometry implements Geometry for SliverProtocol
```

### ParentData

```rust
/// Data stored on child by parent for positioning.
pub trait ParentData: Default + Debug + Send + Sync + 'static {
    /// Called when detaching from parent.
    fn detach(&mut self) {}
}
```

### HitTestResultTrait

```rust
/// Base trait for hit test results.
pub trait HitTestResultTrait: Default {
    /// Add target to hit path.
    fn add(&mut self, target: RenderNodeId);
    
    /// Push transform for subsequent entries.
    fn push_transform(&mut self, transform: Mat4);
    
    /// Pop most recent transform.
    fn pop_transform(&mut self);
    
    /// Whether any targets were hit.
    fn is_empty(&self) -> bool;
    
    /// Get hit path.
    fn path(&self) -> &[HitTestEntry];
}
```

## 2. Render Object Traits

### RenderObject (Base)

```rust
/// Base trait for all render objects.
/// Handles tree structure, dirty flags, attach/detach.
pub trait RenderObject: Send + Sync + 'static {
    /// Unique debug name.
    fn debug_name(&self) -> &'static str;
    
    /// Visit all children.
    fn visit_children(&self, visitor: &mut dyn FnMut(RenderNodeId));
    
    /// Visit children mutably.
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(RenderNodeId));
    
    /// Number of children.
    fn child_count(&self) -> usize;
}
```

### LayoutProtocol

```rust
/// Layout behavior for a specific protocol.
pub trait LayoutProtocol<P: Protocol>: RenderObject {
    /// Whether size is determined solely by constraints.
    /// If true, layout is split into performResize + performLayout.
    fn sized_by_parent(&self) -> bool { false }
    
    /// Perform layout and compute geometry.
    /// Called with constraints stored in node.
    fn perform_layout(
        &mut self,
        constraints: &P::Constraints,
        children: &mut dyn ChildLayouter<P>,
    ) -> P::Geometry;
    
    /// Compute layout without side effects (for intrinsics).
    fn compute_dry_layout(
        &self,
        constraints: &P::Constraints,
    ) -> P::Geometry {
        // Default: not supported
        panic!("compute_dry_layout not implemented")
    }
}
```

### PaintProtocol

```rust
/// Paint behavior.
pub trait PaintProtocol: RenderObject {
    /// Paint this render object.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    /// Estimated bounds for debugging.
    fn paint_bounds(&self) -> Rect;
    
    /// Whether this creates its own compositing layer.
    fn is_repaint_boundary(&self) -> bool { false }
    
    /// Whether this always needs compositing.
    fn always_needs_compositing(&self) -> bool { false }
}
```

### HitTestProtocol

```rust
/// Hit testing behavior for a specific protocol.
pub trait HitTestProtocol<P: Protocol>: RenderObject {
    /// Hit test at position.
    fn hit_test(
        &self,
        result: &mut P::HitTestResult,
        position: P::HitTestPosition,
    ) -> bool {
        if self.hit_test_self(position) {
            result.add(self.id());
        }
        self.hit_test_children(result, position)
    }
    
    /// Test if position hits this object (not children).
    fn hit_test_self(&self, position: P::HitTestPosition) -> bool;
    
    /// Test children, adding to result.
    fn hit_test_children(
        &self,
        result: &mut P::HitTestResult,
        position: P::HitTestPosition,
    ) -> bool;
}
```

## 3. Box Protocol Traits

### BoxLayout

```rust
/// Box-specific layout methods.
pub trait BoxLayout: LayoutProtocol<BoxProtocol> {
    /// Compute minimum intrinsic width.
    fn compute_min_intrinsic_width(&self, height: f64) -> f64 { 0.0 }
    
    /// Compute maximum intrinsic width.
    fn compute_max_intrinsic_width(&self, height: f64) -> f64 { 0.0 }
    
    /// Compute minimum intrinsic height.
    fn compute_min_intrinsic_height(&self, width: f64) -> f64 { 0.0 }
    
    /// Compute maximum intrinsic height.
    fn compute_max_intrinsic_height(&self, width: f64) -> f64 { 0.0 }
    
    /// Compute baseline distance from top.
    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> Option<f64> { None }
}
```

### BoxHitTest

```rust
/// Box-specific hit testing.
pub trait BoxHitTest: HitTestProtocol<BoxProtocol> {
    /// Default: hit if within size bounds.
    fn hit_test_self(&self, position: Offset) -> bool {
        let size = self.size();
        position.x >= 0.0 && position.x < size.width &&
        position.y >= 0.0 && position.y < size.height
    }
}
```

## 4. Sliver Protocol Traits

### SliverLayout

```rust
/// Sliver-specific layout methods.
pub trait SliverLayout: LayoutProtocol<SliverProtocol> {
    /// Calculate paint offset for a child at given layout offset.
    fn child_main_axis_position(&self, child: RenderNodeId) -> f64;
    
    /// Calculate cross axis offset for child.
    fn child_cross_axis_position(&self, child: RenderNodeId) -> f64 { 0.0 }
    
    /// Get scroll offset correction to apply.
    fn scroll_offset_correction(&self) -> Option<f64> { None }
}
```

### SliverHitTest

```rust
/// Sliver-specific hit testing.
pub trait SliverHitTest: HitTestProtocol<SliverProtocol> {
    /// Test hit in sliver coordinate system.
    fn hit_test_self(&self, main_axis: f64, cross_axis: f64) -> bool;
}
```

## 5. Child Storage Types

Children are stored **inside** RenderObject (like Flutter), not in a separate tree.
This enables direct method calls: `self.child.perform_layout()`.

### Child\<P\> — Single Child

Flutter's `RenderObjectWithChildMixin`:

```rust
/// Single child storage (optional).
/// 
/// # Usage
/// ```rust
/// pub struct RenderPadding {
///     child: Child<BoxProtocol>,
///     padding: EdgeInsets,
/// }
/// ```
pub struct Child<P: Protocol> {
    inner: Option<RenderHandle<P, Mounted>>,
}

impl<P: Protocol> Child<P> {
    pub fn new() -> Self;
    pub fn with(child: RenderHandle<P, Mounted>) -> Self;
    
    pub fn get(&self) -> Option<&RenderHandle<P, Mounted>>;
    pub fn get_mut(&mut self) -> Option<&mut RenderHandle<P, Mounted>>;
    pub fn set(&mut self, child: Option<RenderHandle<P, Mounted>>);
    pub fn take(&mut self) -> Option<RenderHandle<P, Mounted>>;
    
    pub fn is_some(&self) -> bool;
    pub fn is_none(&self) -> bool;
    
    // Lifecycle helpers
    pub fn attach(&mut self);
    pub fn detach(&mut self);
    pub fn visit(&self, visitor: &mut dyn FnMut(&dyn RenderObject));
}

// Deref for ergonomic access
impl<P: Protocol> Deref for Child<P> {
    type Target = Option<RenderHandle<P, Mounted>>;
}
```

### Children\<P, PD\> — Multiple Children

Flutter's `ContainerRenderObjectMixin`:

```rust
/// Parent data wrapper with offset.
#[derive(Debug, Clone)]
pub struct ContainerParentData<PD: ParentData = ()> {
    pub offset: Offset,
    pub data: PD,
}

/// Multiple children storage with custom parent data.
/// 
/// # Type Parameters
/// - `P: Protocol` — BoxProtocol or SliverProtocol
/// - `PD: ParentData` — Custom data (e.g., FlexParentData)
/// 
/// # Usage
/// ```rust
/// pub struct RenderFlex {
///     children: Children<BoxProtocol, FlexParentData>,
///     direction: Axis,
/// }
/// ```
pub struct Children<P: Protocol, PD: ParentData = ()> {
    items: Vec<(RenderHandle<P, Mounted>, ContainerParentData<PD>)>,
}

impl<P: Protocol, PD: ParentData + Default> Children<P, PD> {
    // Flutter-like properties
    pub fn len(&self) -> usize;            // childCount
    pub fn first(&self) -> Option<&RenderHandle<P, Mounted>>;   // firstChild
    pub fn last(&self) -> Option<&RenderHandle<P, Mounted>>;    // lastChild
    
    // Flutter-like methods
    pub fn add(&mut self, child: RenderHandle<P, Mounted>);     // add
    pub fn add_with_data(&mut self, child: RenderHandle<P, Mounted>, data: PD);
    pub fn add_all(&mut self, children: impl IntoIterator<Item = RenderHandle<P, Mounted>>);
    pub fn insert(&mut self, index: usize, child: RenderHandle<P, Mounted>);
    pub fn remove(&mut self, index: usize) -> Option<RenderHandle<P, Mounted>>;
    pub fn clear(&mut self);               // removeAll
    pub fn move_child(&mut self, from: usize, to: usize);
    
    // Access
    pub fn get(&self, index: usize) -> Option<&RenderHandle<P, Mounted>>;
    pub fn get_mut(&mut self, index: usize) -> Option<&mut RenderHandle<P, Mounted>>;
    
    // Parent data
    pub fn parent_data(&self, index: usize) -> Option<&ContainerParentData<PD>>;
    pub fn parent_data_mut(&mut self, index: usize) -> Option<&mut ContainerParentData<PD>>;
    pub fn set_offset(&mut self, index: usize, offset: Offset);
    pub fn offset(&self, index: usize) -> Option<Offset>;
    
    // Iteration
    pub fn iter(&self) -> impl Iterator<Item = &RenderHandle<P, Mounted>>;
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut RenderHandle<P, Mounted>>;
    pub fn iter_with_data(&self) -> impl Iterator<Item = (&RenderHandle<P, Mounted>, &ContainerParentData<PD>)>;
    
    // Lifecycle
    pub fn attach_all(&mut self);
    pub fn detach_all(&mut self);
    pub fn visit_all(&self, visitor: &mut dyn FnMut(&dyn RenderObject));
}
```

### Slots\<P, S\> — Named Slots

Flutter's `SlottedContainerRenderObjectMixin`:

```rust
/// Marker trait for slot keys (usually enums).
pub trait SlotKey: Eq + Hash + Copy + 'static {}

/// Named slots storage.
/// 
/// # Type Parameters
/// - `P: Protocol` — BoxProtocol or SliverProtocol
/// - `S: SlotKey` — Slot enum type
/// 
/// # Usage
/// ```rust
/// #[derive(Clone, Copy, PartialEq, Eq, Hash)]
/// pub enum ListTileSlot { Leading, Title, Subtitle, Trailing }
/// 
/// pub struct RenderListTile {
///     slots: Slots<BoxProtocol, ListTileSlot>,
/// }
/// ```
pub struct Slots<P: Protocol, S: SlotKey> {
    items: HashMap<S, (RenderHandle<P, Mounted>, Offset)>,
}

impl<P: Protocol, S: SlotKey> Slots<P, S> {
    // Flutter-like methods
    pub fn get(&self, slot: S) -> Option<&RenderHandle<P, Mounted>>;  // childForSlot
    pub fn get_mut(&mut self, slot: S) -> Option<&mut RenderHandle<P, Mounted>>;
    pub fn set(&mut self, slot: S, child: Option<RenderHandle<P, Mounted>>);
    pub fn has(&self, slot: S) -> bool;
    pub fn remove(&mut self, slot: S) -> Option<RenderHandle<P, Mounted>>;
    
    // Offset
    pub fn offset(&self, slot: S) -> Option<Offset>;
    pub fn set_offset(&mut self, slot: S, offset: Offset);
    
    // Iteration
    pub fn len(&self) -> usize;
    pub fn children(&self) -> impl Iterator<Item = &RenderHandle<P, Mounted>>;
    pub fn iter(&self) -> impl Iterator<Item = (&S, &RenderHandle<P, Mounted>)>;
    pub fn iter_with_offset(&self) -> impl Iterator<Item = (&S, &RenderHandle<P, Mounted>, Offset)>;
    
    // Lifecycle
    pub fn attach_all(&mut self);
    pub fn detach_all(&mut self);
    pub fn visit_all(&self, visitor: &mut dyn FnMut(&dyn RenderObject));
}
```

### Summary Table

| Type | Flutter Equivalent | Use Case |
|------|-------------------|----------|
| `Child<P>` | `RenderObjectWithChildMixin` | Padding, Align, Transform, Clip |
| `Children<P, PD>` | `ContainerRenderObjectMixin` | Flex, Stack, Wrap, Flow |
| `Slots<P, S>` | `SlottedContainerRenderObjectMixin` | ListTile, InputDecorator |

## 6. Pipeline Traits

### PipelineOwner

```rust
/// Coordinates the rendering pipeline phases.
pub trait PipelineOwnerTrait {
    /// Request visual update (schedules frame).
    fn request_visual_update(&mut self);
    
    /// Flush layout for all dirty nodes.
    fn flush_layout(&mut self);
    
    /// Flush compositing bits update.
    fn flush_compositing_bits(&mut self);
    
    /// Flush paint for all dirty nodes.
    fn flush_paint(&mut self);
    
    /// Flush semantics update.
    fn flush_semantics(&mut self);
}
```

### ChildLayouter (Layout Helper)

```rust
/// Helper for laying out children during perform_layout.
pub trait ChildLayouter<P: Protocol> {
    /// Layout child with constraints.
    fn layout_child(
        &mut self,
        child: RenderNodeId,
        constraints: P::Constraints,
    ) -> P::Geometry;
    
    /// Get child's computed geometry (after layout).
    fn child_geometry(&self, child: RenderNodeId) -> &P::Geometry;
    
    /// Get child's parent data.
    fn child_parent_data(&self, child: RenderNodeId) -> &P::ParentData;
    
    /// Get mutable child parent data.
    fn child_parent_data_mut(&mut self, child: RenderNodeId) -> &mut P::ParentData;
}
```

## 7. Context Traits

### PaintingContext

```rust
/// Context for paint phase operations.
/// Owns canvas, manages compositing layers.
pub trait PaintingContextTrait {
    /// Get current canvas.
    fn canvas(&mut self) -> &mut dyn Canvas;
    
    /// Paint a child at offset.
    fn paint_child(&mut self, child: RenderNodeId, offset: Offset);
    
    /// Push clip rect (may create layer).
    fn push_clip_rect<F>(&mut self, rect: Rect, painter: F)
    where F: FnOnce(&mut Self);
    
    /// Push opacity (creates layer).
    fn push_opacity<F>(&mut self, alpha: f32, offset: Offset, painter: F)
    where F: FnOnce(&mut Self);
    
    /// Push transform (may create layer).
    fn push_transform<F>(&mut self, transform: Mat4, painter: F)
    where F: FnOnce(&mut Self);
    
    /// Add composited layer directly.
    fn add_layer(&mut self, layer: Layer);
    
    /// Hint: painting is complex, consider caching.
    fn set_is_complex_hint(&mut self);
    
    /// Hint: will change next frame, don't cache.
    fn set_will_change_hint(&mut self);
}
```

## 8. Combined Protocol Traits (Flutter-style)

Flutter имеет `RenderBox` и `RenderSliver` как объединённые классы.
Мы можем сделать то же самое через супер-трейты:

### RenderBox

```rust
/// Combined trait for box layout render objects.
/// Equivalent to Flutter's RenderBox class.
pub trait RenderBox: 
    RenderObject + 
    LayoutProtocol<BoxProtocol> + 
    PaintProtocol + 
    HitTestProtocol<BoxProtocol> +
    BoxLayout +
    BoxHitTest
{
    // === Geometry Access ===
    
    /// Get computed size (valid after layout).
    fn size(&self) -> Size;
    
    /// Set size during layout.
    fn set_size(&mut self, size: Size);
    
    // === Constraints Access ===
    
    /// Get current constraints.
    fn constraints(&self) -> &BoxConstraints;
    
    // === Intrinsic Dimensions (with caching) ===
    
    fn get_min_intrinsic_width(&mut self, height: f64) -> f64 {
        // Caching wrapper around compute_min_intrinsic_width
        self.compute_min_intrinsic_width(height)
    }
    
    fn get_max_intrinsic_width(&mut self, height: f64) -> f64 {
        self.compute_max_intrinsic_width(height)
    }
    
    fn get_min_intrinsic_height(&mut self, width: f64) -> f64 {
        self.compute_min_intrinsic_height(width)
    }
    
    fn get_max_intrinsic_height(&mut self, width: f64) -> f64 {
        self.compute_max_intrinsic_height(width)
    }
    
    // === Dry Layout ===
    
    fn get_dry_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(&constraints)
    }
    
    // === Baseline ===
    
    fn get_distance_to_baseline(&mut self, baseline: TextBaseline) -> Option<f64> {
        self.compute_distance_to_actual_baseline(baseline)
    }
    
    // === Coordinate Conversion ===
    
    /// Convert local position to global.
    fn local_to_global(&self, point: Offset) -> Offset;
    
    /// Convert global position to local.
    fn global_to_local(&self, point: Offset) -> Offset;
    
    // === Default Hit Test ===
    
    /// Default implementation: hit if within bounds.
    fn default_hit_test_self(&self, position: Offset) -> bool {
        let size = self.size();
        position.x >= 0.0 && position.x < size.width &&
        position.y >= 0.0 && position.y < size.height
    }
}
```

### RenderSliver

```rust
/// Combined trait for sliver layout render objects.
/// Equivalent to Flutter's RenderSliver class.
pub trait RenderSliver:
    RenderObject +
    LayoutProtocol<SliverProtocol> +
    PaintProtocol +
    HitTestProtocol<SliverProtocol> +
    SliverLayout +
    SliverHitTest
{
    // === Geometry Access ===
    
    /// Get computed sliver geometry (valid after layout).
    fn geometry(&self) -> &SliverGeometry;
    
    /// Set geometry during layout.
    fn set_geometry(&mut self, geometry: SliverGeometry);
    
    // === Constraints Access ===
    
    /// Get current sliver constraints.
    fn constraints(&self) -> &SliverConstraints;
    
    // === Scroll Helpers ===
    
    /// Calculate the paint offset for given scroll offset.
    fn calculate_paint_offset(
        &self,
        constraints: &SliverConstraints,
        from: f64,
        to: f64,
    ) -> f64 {
        let a = constraints.scroll_offset;
        let b = constraints.scroll_offset + constraints.remaining_paint_extent;
        (to.min(b) - from.max(a)).max(0.0)
    }
    
    /// Calculate the cache offset.
    fn calculate_cache_offset(
        &self,
        constraints: &SliverConstraints,
        from: f64,
        to: f64,
    ) -> f64 {
        let a = constraints.scroll_offset + constraints.cache_origin;
        let b = a + constraints.remaining_cache_extent;
        (to.min(b) - from.max(a)).max(0.0)
    }
    
    // === Child Positioning ===
    
    /// Get paint offset for child in main axis direction.
    fn child_scroll_offset(&self, child: RenderNodeId) -> Option<f64>;
    
    // === Hit Test Helpers ===
    
    /// Transform main axis position to child's coordinate.
    fn child_main_axis_position_for_hit_test(
        &self,
        child: RenderNodeId,
        main_axis_position: f64,
    ) -> f64 {
        main_axis_position - self.child_main_axis_position(child)
    }
}
```

### RenderProxyBox

```rust
/// A RenderBox that delegates everything to a single child.
/// Base for effects: opacity, transform, clip, etc.
/// Equivalent to Flutter's RenderProxyBox.
pub trait RenderProxyBox: RenderBox {
    fn child(&self) -> Option<RenderNodeId>;
    fn child_box(&self) -> Option<&dyn RenderBox>;
    fn child_box_mut(&mut self) -> Option<&mut dyn RenderBox>;
    
    // All methods have default implementations that delegate to child
}

// Default implementations for RenderProxyBox
impl<T: RenderProxyBox> LayoutProtocol<BoxProtocol> for T {
    fn perform_layout(
        &mut self,
        constraints: &BoxConstraints,
        children: &mut dyn ChildLayouter<BoxProtocol>,
    ) -> Size {
        if let Some(child_id) = self.child() {
            let size = children.layout_child(child_id, constraints.clone());
            size
        } else {
            constraints.smallest()
        }
    }
}
```

### RenderShiftedBox

```rust
/// A RenderBox that positions child at a non-zero offset.
/// Base for: Padding, Align, CustomSingleChildLayout.
/// Equivalent to Flutter's RenderShiftedBox.
pub trait RenderShiftedBox: RenderBox {
    fn child(&self) -> Option<RenderNodeId>;
    fn child_offset(&self) -> Offset;
    fn set_child_offset(&mut self, offset: Offset);
    
    // Paint delegates to child at offset
    // Hit test transforms position by offset
}
```

### Trait Hierarchy Diagram (Updated)

```
                              RenderObject
                                   │
                    ┌──────────────┴──────────────┐
                    │                             │
                    ▼                             ▼
              ┌───────────┐                 ┌───────────┐
              │ RenderBox │                 │RenderSliver│
              └─────┬─────┘                 └─────┬─────┘
                    │                             │
        ┌───────────┼───────────┐                 │
        │           │           │                 │
        ▼           ▼           ▼                 ▼
   RenderProxy  RenderShifted  (custom)     RenderProxy
      Box          Box                        Sliver
        │           │
        │           │
        ▼           ▼
   RenderOpacity  RenderPadding
   RenderTransform RenderAlign
   RenderClip*    RenderPositioned
```

### Why Combined Traits?

| Approach | Pros | Cons |
|----------|------|------|
| **Small traits** | Flexible composition, clear responsibilities | Many trait bounds, verbose |
| **Combined traits** | Familiar to Flutter devs, simpler bounds | Less flexible |
| **Hybrid (recommended)** | Best of both | Need both |

**Recommendation:** Use combined traits (`RenderBox`, `RenderSliver`) as the main API,
but define them via super-traits so individual pieces can still be used:

```rust
// User implements the combined trait
impl RenderBox for MyWidget {
    fn size(&self) -> Size { ... }
    fn set_size(&mut self, size: Size) { ... }
    // etc.
}

// Or implements individual traits for more control
impl LayoutProtocol<BoxProtocol> for AdvancedWidget { ... }
impl PaintProtocol for AdvancedWidget { ... }
impl HitTestProtocol<BoxProtocol> for AdvancedWidget { ... }
```

## 9. Transition Traits

### Attachable

```rust
/// Nodes that can be attached to a render tree.
pub trait Attachable {
    type Attached;
    
    fn attach(self, owner: &mut dyn PipelineOwnerTrait) -> Self::Attached;
}
```

### Layoutable

```rust
/// Attached nodes that can perform layout.
pub trait Layoutable<P: Protocol> {
    type LaidOut;
    
    fn layout(self, constraints: P::Constraints) -> Self::LaidOut;
}
```

### Paintable

```rust
/// Laid out nodes that can paint.
pub trait Paintable {
    type Painted;
    
    fn paint(self, context: &mut dyn PaintingContextTrait) -> Self::Painted;
}
```

### Invalidatable

```rust
/// Nodes that can be invalidated (marked dirty).
pub trait Invalidatable {
    type NeedsLayout;
    type NeedsPaint;
    
    fn mark_needs_layout(self) -> Self::NeedsLayout;
    fn mark_needs_paint(self) -> Self::NeedsPaint;
}
```

## Summary Table

| Category | Trait/Type | Purpose |
|----------|------------|---------|
| **Handle** | `RenderHandle<P, S>` | Typestate handle with Deref |
| **Handle** | `BoxHandle<S>` | Alias for `RenderHandle<BoxProtocol, S>` |
| **Handle** | `SliverHandle<S>` | Alias for `RenderHandle<SliverProtocol, S>` |
| **State** | `RenderState` | Lifecycle typestate markers |
| **Protocol** | `Protocol` | Box vs Sliver layout system |
| **Core** | `Constraints` | Layout input (parent → child) |
| **Core** | `Geometry` | Layout output (child → parent) |
| **Core** | `ParentData` | Child positioning data |
| **Core** | `HitTestResultTrait` | Hit test path recording |
| **Node** | `RenderObject` | Base behavior, tree structure |
| **Node** | `LayoutProtocol<P>` | Protocol-specific layout |
| **Node** | `PaintProtocol` | Painting behavior |
| **Node** | `HitTestProtocol<P>` | Protocol-specific hit testing |
| **Box** | `BoxLayout` | Intrinsics, baseline |
| **Box** | `BoxHitTest` | Box coordinate hit testing |
| **Box** | **`RenderBox`** | **Combined: all Box traits** |
| **Box** | `RenderProxyBox` | Delegate to single Box child |
| **Box** | `RenderShiftedBox` | Box child at offset |
| **Sliver** | `SliverLayout` | Scroll positioning |
| **Sliver** | `SliverHitTest` | Axis coordinate hit testing |
| **Sliver** | **`RenderSliver`** | **Combined: all Sliver traits** |
| **Children** | `Child<P>` | Single child storage |
| **Children** | `Children<P, PD>` | Multiple children with parent data |
| **Children** | `Slots<P, S>` | Named slots storage |
| **Pipeline** | `PipelineOwnerTrait` | Phase coordination |
| **Context** | `PaintingContextTrait` | Canvas + layer management |
| **Transition** | `Attachable` | Detached → Attached |
| **Transition** | `Layoutable<P>` | Attached → LaidOut |
| **Transition** | `Paintable` | LaidOut → Painted |
| **Transition** | `Invalidatable` | Mark dirty (state regression) |

## File Organization

```
crates/flui_rendering/src/
├── lib.rs
├── handle.rs            # RenderHandle<P, S> with Deref
├── state.rs             # RenderState trait + Mounted/Unmounted
├── protocol/
│   ├── mod.rs           # Protocol trait
│   ├── box_protocol.rs  # BoxProtocol, BoxConstraints, Size
│   └── sliver_protocol.rs # SliverProtocol, SliverConstraints
├── children/
│   ├── mod.rs           # Re-exports
│   ├── child.rs         # Child<P> — single child
│   ├── children.rs      # Children<P, PD> — multiple children
│   └── slots.rs         # Slots<P, S> — named slots
├── traits/
│   ├── mod.rs           # Re-exports
│   ├── constraints.rs   # Constraints trait
│   ├── geometry.rs      # Geometry trait
│   ├── parent_data.rs   # ParentData trait
│   ├── hit_test.rs      # HitTestResultTrait, HitTestEntry
│   ├── render_object.rs # RenderObject base trait
│   ├── layout.rs        # LayoutProtocol, BoxLayout, SliverLayout
│   ├── paint.rs         # PaintProtocol, PaintingContext
│   └── hit_test_protocol.rs # HitTestProtocol, BoxHitTest, SliverHitTest
├── base/
│   ├── mod.rs           # Re-exports
│   ├── single_child.rs  # SingleChildBase
│   ├── shifted_box.rs   # ShiftedBoxBase
│   ├── aligning_box.rs  # AligningBoxBase
│   ├── proxy_box.rs     # ProxyBoxBase
│   └── container.rs     # ContainerBase
├── box/
│   ├── mod.rs           # Re-exports
│   └── render_box.rs    # RenderBox combined trait
├── sliver/
│   ├── mod.rs           # Re-exports
│   └── render_sliver.rs # RenderSliver combined trait
├── pipeline/
│   ├── mod.rs
│   └── owner.rs         # PipelineOwner
└── context/
    ├── mod.rs
    └── painting.rs      # PaintingContext
```
