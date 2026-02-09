# RenderState & Protocol Architecture Plan

## Overview

This plan describes the architecture for `flui_rendering` using:
- **RenderState** - typestate for render lifecycle (Detached → Attached → LaidOut → Painted)
- **Protocol** - Box vs Sliver layout protocols
- **Arity** - reused from `flui-tree` (Leaf, Single, Optional, Variable)

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              flui-tree (reuse)                               │
├─────────────────────────────────────────────────────────────────────────────┤
│  Arity                        │  ArityStorage                               │
│  ├── Leaf                     │  Children<N, A, S>                          │
│  ├── Single                   │  ├── LeafChildren                           │
│  ├── Optional                 │  ├── SingleChild                            │
│  ├── Variable                 │  ├── OptionalChild                          │
│  ├── Exact<N>                 │  └── VariableChildren                       │
│  ├── AtLeast<N>               │                                             │
│  └── Range<MIN, MAX>          │                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        │ imports Arity
                                        ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            flui_rendering (new)                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  RenderState (typestate)      │  Protocol                                   │
│  ├── Detached                 │  ├── BoxProtocol                            │
│  ├── Attached                 │  │   ├── Constraints = BoxConstraints       │
│  ├── LaidOut                  │  │   ├── Geometry = Size                    │
│  └── Painted                  │  │   └── ParentData = BoxParentData         │
│                               │  │                                          │
│                               │  └── SliverProtocol                         │
│                               │      ├── Constraints = SliverConstraints    │
│                               │      ├── Geometry = SliverGeometry          │
│                               │      └── ParentData = SliverParentData      │
│                                                                             │
│  RenderNode<P: Protocol, A: Arity, S: RenderState>                          │
│  ├── children: RenderChildren<A, S>                                         │
│  ├── constraints: Option<P::Constraints>                                    │
│  ├── geometry: Option<P::Geometry>                                          │
│  └── parent_data: P::ParentData                                             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## RenderState Typestate

### State Diagram

```
                    ┌──────────────────────────────────────────────────┐
                    │                                                  │
                    ▼                                                  │
┌──────────┐   attach()   ┌──────────┐   layout()   ┌──────────┐   paint()   ┌──────────┐
│ Detached │ ──────────►  │ Attached │ ──────────►  │ LaidOut  │ ──────────► │ Painted  │
└──────────┘              └──────────┘              └──────────┘              └──────────┘
     ▲                         ▲                         ▲                         │
     │                         │                         │                         │
     │ detach()                │ mark_needs_layout()     │ mark_needs_paint()      │
     │                         │                         │                         │
     └─────────────────────────┴─────────────────────────┴─────────────────────────┘
```

### State Definitions

```rust
//! RenderState - typestate markers for render lifecycle.

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Detached {}
    impl Sealed for super::Attached {}
    impl Sealed for super::LaidOut {}
    impl Sealed for super::Painted {}
}

/// Marker trait for render states.
pub trait RenderState: sealed::Sealed + Send + Sync + Copy + Default + 'static {
    /// Whether this state is attached to a tree.
    const IS_ATTACHED: bool;
    
    /// Whether layout has been performed.
    const IS_LAID_OUT: bool;
    
    /// Whether paint has been performed.
    const IS_PAINTED: bool;
    
    /// Human-readable name.
    fn name() -> &'static str;
}

/// Detached state - node not in render tree.
/// 
/// - No parent reference
/// - No constraints/geometry
/// - Can be attached via `attach()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Detached;

impl RenderState for Detached {
    const IS_ATTACHED: bool = false;
    const IS_LAID_OUT: bool = false;
    const IS_PAINTED: bool = false;
    
    fn name() -> &'static str { "Detached" }
}

/// Attached state - node in tree, needs layout.
///
/// - Has parent reference
/// - Has owner (PipelineOwner)
/// - Constraints/geometry may be stale
/// - Can perform layout via `layout()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Attached;

impl RenderState for Attached {
    const IS_ATTACHED: bool = true;
    const IS_LAID_OUT: bool = false;
    const IS_PAINTED: bool = false;
    
    fn name() -> &'static str { "Attached" }
}

/// LaidOut state - layout performed, needs paint.
///
/// - Has valid constraints
/// - Has valid geometry (size)
/// - Can access geometry via `geometry()`
/// - Can perform paint via `paint()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LaidOut;

impl RenderState for LaidOut {
    const IS_ATTACHED: bool = true;
    const IS_LAID_OUT: bool = true;
    const IS_PAINTED: bool = false;
    
    fn name() -> &'static str { "LaidOut" }
}

/// Painted state - fully rendered.
///
/// - Has valid constraints
/// - Has valid geometry
/// - Visual output recorded
/// - Can mark dirty via `mark_needs_paint()` or `mark_needs_layout()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Painted;

impl RenderState for Painted {
    const IS_ATTACHED: bool = true;
    const IS_LAID_OUT: bool = true;
    const IS_PAINTED: bool = true;
    
    fn name() -> &'static str { "Painted" }
}
```

### State Transition Traits

```rust
/// Trait for nodes that can be attached to a render tree.
pub trait Attachable: Sized {
    type Id: Identifier;
    type Attached;
    
    /// Attach to render tree with given parent and owner.
    fn attach(self, parent: Option<Self::Id>, owner: &PipelineOwner) -> Self::Attached;
}

/// Trait for attached nodes.
pub trait AttachedNode: Sized {
    type Id: Identifier;
    type Detached;
    type LaidOut;
    
    /// Get parent node ID.
    fn parent(&self) -> Option<Self::Id>;
    
    /// Perform layout with constraints.
    fn layout<C: Constraints>(self, constraints: C) -> Self::LaidOut;
    
    /// Detach from render tree.
    fn detach(self, owner: &PipelineOwner) -> Self::Detached;
}

/// Trait for laid out nodes.
pub trait LaidOutNode: Sized {
    type Id: Identifier;
    type Geometry;
    type Attached;
    type Painted;
    
    /// Get computed geometry (compile-time guaranteed to exist).
    fn geometry(&self) -> &Self::Geometry;
    
    /// Perform paint.
    fn paint(self, context: &mut PaintingContext) -> Self::Painted;
    
    /// Mark needs layout - back to Attached.
    fn mark_needs_layout(self) -> Self::Attached;
}

/// Trait for painted nodes.
pub trait PaintedNode: Sized {
    type Id: Identifier;
    type Geometry;
    type Attached;
    type LaidOut;
    
    /// Get computed geometry.
    fn geometry(&self) -> &Self::Geometry;
    
    /// Mark needs paint - back to LaidOut.
    fn mark_needs_paint(self) -> Self::LaidOut;
    
    /// Mark needs layout - back to Attached.
    fn mark_needs_layout(self) -> Self::Attached;
}
```

## Protocol System

### Protocol Trait

```rust
/// Layout protocol marker trait.
///
/// Defines the constraint/geometry types for a layout system.
pub trait Protocol: sealed::ProtocolSealed + Send + Sync + 'static {
    /// Constraints type passed from parent to child.
    type Constraints: Constraints + Clone + Debug;
    
    /// Geometry type computed during layout.
    type Geometry: Geometry + Clone + Debug;
    
    /// Parent data type for child positioning.
    type ParentData: ParentData + Default + Debug;
    
    /// Protocol identifier for runtime checks.
    fn protocol_id() -> ProtocolId;
    
    /// Human-readable name.
    fn name() -> &'static str;
}

mod sealed {
    pub trait ProtocolSealed {}
    impl ProtocolSealed for super::BoxProtocol {}
    impl ProtocolSealed for super::SliverProtocol {}
}

/// Runtime protocol identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolId {
    Box,
    Sliver,
}
```

### BoxProtocol

```rust
/// 2D rectangular box layout protocol.
///
/// Used by most widgets: Padding, Center, Column, Row, etc.
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type ParentData = BoxParentData;
    
    fn protocol_id() -> ProtocolId { ProtocolId::Box }
    fn name() -> &'static str { "Box" }
}

/// Box constraints - min/max width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    pub min_width: f64,
    pub max_width: f64,
    pub min_height: f64,
    pub max_height: f64,
}

/// Box geometry - computed size.
pub type Size = euclid::Size2D<f64, euclid::UnknownUnit>;

/// Box parent data - offset from parent origin.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxParentData {
    pub offset: Offset,
}
```

### SliverProtocol

```rust
/// Scrollable content layout protocol.
///
/// Used by scrollable content: ListView, GridView, etc.
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type ParentData = SliverParentData;
    
    fn protocol_id() -> ProtocolId { ProtocolId::Sliver }
    fn name() -> &'static str { "Sliver" }
}

/// Sliver constraints - scroll state and available space.
#[derive(Debug, Clone, Copy)]
pub struct SliverConstraints {
    pub axis_direction: AxisDirection,
    pub growth_direction: GrowthDirection,
    pub scroll_offset: f64,
    pub remaining_paint_extent: f64,
    pub cross_axis_extent: f64,
    pub viewport_main_axis_extent: f64,
    // ...
}

/// Sliver geometry - computed extents.
#[derive(Debug, Clone, Copy)]
pub struct SliverGeometry {
    pub scroll_extent: f64,
    pub paint_extent: f64,
    pub layout_extent: f64,
    pub max_paint_extent: f64,
    pub visible: bool,
    // ...
}

/// Sliver parent data.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverParentData {
    pub paint_offset: f64,
}
```

## RenderNode

### Core Structure

```rust
/// A node in the render tree.
///
/// # Type Parameters
///
/// - `P: Protocol` - layout protocol (Box or Sliver)
/// - `A: Arity` - child count constraint (from flui-tree)
/// - `S: RenderState` - lifecycle state (Detached, Attached, LaidOut, Painted)
pub struct RenderNode<P: Protocol, A: Arity, S: RenderState = Detached> {
    // === Tree Structure ===
    parent: Option<RenderNodeId>,
    children: RenderChildren<A, S>,
    depth: u32,
    
    // === Protocol State ===
    constraints: Option<P::Constraints>,
    geometry: Option<P::Geometry>,
    parent_data: P::ParentData,
    
    // === Dirty Flags ===
    needs_layout: bool,
    needs_paint: bool,
    needs_compositing_bits_update: bool,
    needs_semantics_update: bool,
    
    // === Render Properties ===
    is_repaint_boundary: bool,
    
    // === Typestate Marker ===
    _state: PhantomData<S>,
}

/// Children storage for RenderNode.
///
/// Uses Arity from flui-tree but with RenderState.
pub struct RenderChildren<A: Arity, S: RenderState> {
    storage: A::Storage<RenderNodeId>,
    _state: PhantomData<S>,
}
```

### State-Specific Implementations

```rust
// ═══════════════════════════════════════════════════════════════════
// DETACHED STATE
// ═══════════════════════════════════════════════════════════════════

impl<P: Protocol, A: Arity> RenderNode<P, A, Detached> {
    /// Create a new detached render node.
    pub fn new() -> Self {
        Self {
            parent: None,
            children: RenderChildren::new(),
            depth: 0,
            constraints: None,
            geometry: None,
            parent_data: P::ParentData::default(),
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            needs_semantics_update: true,
            is_repaint_boundary: false,
            _state: PhantomData,
        }
    }
    
    /// Attach to render tree.
    pub fn attach(
        self,
        parent: Option<RenderNodeId>,
        owner: &mut PipelineOwner,
    ) -> RenderNode<P, A, Attached> {
        owner.add_to_dirty_layout_list(/* ... */);
        
        RenderNode {
            parent,
            children: self.children.transition(),
            depth: /* compute from parent */,
            constraints: None,
            geometry: None,
            parent_data: self.parent_data,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            needs_semantics_update: true,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// ATTACHED STATE
// ═══════════════════════════════════════════════════════════════════

impl<P: Protocol, A: Arity> RenderNode<P, A, Attached> {
    /// Get parent node ID.
    pub fn parent(&self) -> Option<RenderNodeId> {
        self.parent
    }
    
    /// Perform layout with given constraints.
    pub fn layout(
        mut self,
        constraints: P::Constraints,
    ) -> RenderNode<P, A, LaidOut> {
        self.constraints = Some(constraints);
        
        // Perform actual layout (call perform_layout, layout children)
        let geometry = self.perform_layout(&constraints);
        
        RenderNode {
            parent: self.parent,
            children: self.children.transition(),
            depth: self.depth,
            constraints: Some(constraints),
            geometry: Some(geometry),
            parent_data: self.parent_data,
            needs_layout: false,
            needs_paint: true,
            needs_compositing_bits_update: self.needs_compositing_bits_update,
            needs_semantics_update: self.needs_semantics_update,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
    
    /// Detach from render tree.
    pub fn detach(self, owner: &mut PipelineOwner) -> RenderNode<P, A, Detached> {
        owner.remove_from_dirty_lists(/* ... */);
        
        RenderNode {
            parent: None,
            children: self.children.transition(),
            depth: 0,
            constraints: None,
            geometry: None,
            parent_data: self.parent_data,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            needs_semantics_update: true,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// LAID OUT STATE
// ═══════════════════════════════════════════════════════════════════

impl<P: Protocol, A: Arity> RenderNode<P, A, LaidOut> {
    /// Get computed geometry (GUARANTEED to exist in this state).
    pub fn geometry(&self) -> &P::Geometry {
        // SAFETY: geometry is always set in LaidOut state
        self.geometry.as_ref().unwrap()
    }
    
    /// Get constraints used for layout.
    pub fn constraints(&self) -> &P::Constraints {
        self.constraints.as_ref().unwrap()
    }
    
    /// Perform paint.
    pub fn paint(
        self,
        context: &mut PaintingContext,
    ) -> RenderNode<P, A, Painted> {
        // Perform actual paint
        self.perform_paint(context);
        
        RenderNode {
            parent: self.parent,
            children: self.children.transition(),
            depth: self.depth,
            constraints: self.constraints,
            geometry: self.geometry,
            parent_data: self.parent_data,
            needs_layout: false,
            needs_paint: false,
            needs_compositing_bits_update: self.needs_compositing_bits_update,
            needs_semantics_update: self.needs_semantics_update,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
    
    /// Mark needs layout - invalidate geometry.
    pub fn mark_needs_layout(self) -> RenderNode<P, A, Attached> {
        RenderNode {
            parent: self.parent,
            children: self.children.transition(),
            depth: self.depth,
            constraints: self.constraints,
            geometry: None,  // Invalidate!
            parent_data: self.parent_data,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: self.needs_compositing_bits_update,
            needs_semantics_update: self.needs_semantics_update,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// PAINTED STATE
// ═══════════════════════════════════════════════════════════════════

impl<P: Protocol, A: Arity> RenderNode<P, A, Painted> {
    /// Get computed geometry.
    pub fn geometry(&self) -> &P::Geometry {
        self.geometry.as_ref().unwrap()
    }
    
    /// Mark needs paint - keep geometry, re-paint.
    pub fn mark_needs_paint(self) -> RenderNode<P, A, LaidOut> {
        RenderNode {
            parent: self.parent,
            children: self.children.transition(),
            depth: self.depth,
            constraints: self.constraints,
            geometry: self.geometry,  // Keep geometry!
            parent_data: self.parent_data,
            needs_layout: false,
            needs_paint: true,
            needs_compositing_bits_update: self.needs_compositing_bits_update,
            needs_semantics_update: self.needs_semantics_update,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
    
    /// Mark needs layout - invalidate everything.
    pub fn mark_needs_layout(self) -> RenderNode<P, A, Attached> {
        RenderNode {
            parent: self.parent,
            children: self.children.transition(),
            depth: self.depth,
            constraints: self.constraints,
            geometry: None,
            parent_data: self.parent_data,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: self.needs_compositing_bits_update,
            needs_semantics_update: self.needs_semantics_update,
            is_repaint_boundary: self.is_repaint_boundary,
            _state: PhantomData,
        }
    }
}
```

## Type Aliases

```rust
// ═══════════════════════════════════════════════════════════════════
// BOX PROTOCOL ALIASES
// ═══════════════════════════════════════════════════════════════════

/// Box leaf node (no children) - e.g., Text, Image, Spacer
pub type BoxLeaf<S = Detached> = RenderNode<BoxProtocol, Leaf, S>;

/// Box with single child - e.g., Padding, Opacity, Transform
pub type BoxSingle<S = Detached> = RenderNode<BoxProtocol, Single, S>;

/// Box with optional child - e.g., Container, SizedBox
pub type BoxOptional<S = Detached> = RenderNode<BoxProtocol, Optional, S>;

/// Box with variable children - e.g., Column, Row, Stack
pub type BoxVariable<S = Detached> = RenderNode<BoxProtocol, Variable, S>;

/// Box with exactly N children
pub type BoxExact<const N: usize, S = Detached> = RenderNode<BoxProtocol, Exact<N>, S>;

// ═══════════════════════════════════════════════════════════════════
// SLIVER PROTOCOL ALIASES
// ═══════════════════════════════════════════════════════════════════

/// Sliver with single box child - e.g., SliverToBoxAdapter
pub type SliverSingleBox<S = Detached> = RenderNode<SliverProtocol, Single, S>;

/// Sliver with variable children - e.g., SliverList, SliverGrid
pub type SliverVariable<S = Detached> = RenderNode<SliverProtocol, Variable, S>;
```

## Compile-Time Safety Examples

```rust
fn compile_time_safety_examples() {
    // ═══════════════════════════════════════════════════════════════
    // Example 1: Can't access geometry before layout
    // ═══════════════════════════════════════════════════════════════
    
    let node: BoxSingle<Attached> = /* ... */;
    
    // ❌ COMPILE ERROR: geometry() doesn't exist for Attached state
    // let size = node.geometry();
    
    // ✅ First do layout
    let node: BoxSingle<LaidOut> = node.layout(constraints);
    let size = node.geometry();  // OK!
    
    // ═══════════════════════════════════════════════════════════════
    // Example 2: Can't paint before layout
    // ═══════════════════════════════════════════════════════════════
    
    let attached: BoxSingle<Attached> = /* ... */;
    
    // ❌ COMPILE ERROR: paint() doesn't exist for Attached state
    // let painted = attached.paint(&mut ctx);
    
    // ✅ First layout, then paint
    let laid_out = attached.layout(constraints);
    let painted = laid_out.paint(&mut ctx);  // OK!
    
    // ═══════════════════════════════════════════════════════════════
    // Example 3: State transitions are explicit
    // ═══════════════════════════════════════════════════════════════
    
    let painted: BoxSingle<Painted> = /* ... */;
    
    // Mark needs paint - goes back to LaidOut
    let laid_out: BoxSingle<LaidOut> = painted.mark_needs_paint();
    
    // Mark needs layout - goes back to Attached
    let attached: BoxSingle<Attached> = laid_out.mark_needs_layout();
    
    // Detach - goes back to Detached
    let detached: BoxSingle<Detached> = attached.detach(&mut owner);
    
    // ═══════════════════════════════════════════════════════════════
    // Example 4: Arity is compile-time checked
    // ═══════════════════════════════════════════════════════════════
    
    let leaf: BoxLeaf<Detached> = BoxLeaf::new();
    
    // ❌ COMPILE ERROR: Leaf has no children methods
    // leaf.add_child(child);
    
    let mut single: BoxSingle<Detached> = BoxSingle::new();
    single.set_child(child);  // OK - Single has exactly one
    
    // ❌ COMPILE ERROR: Single doesn't have push()
    // single.push(child2);
    
    let mut variable: BoxVariable<Detached> = BoxVariable::new();
    variable.push(child1);  // OK - Variable has any number
    variable.push(child2);  // OK
    variable.push(child3);  // OK
}
```

## File Structure

```
crates/flui_rendering/src/
├── lib.rs                  # Re-exports
├── state/
│   ├── mod.rs              # RenderState trait and markers
│   ├── detached.rs         # Detached state
│   ├── attached.rs         # Attached state
│   ├── laid_out.rs         # LaidOut state
│   └── painted.rs          # Painted state
├── protocol/
│   ├── mod.rs              # Protocol trait
│   ├── box_protocol.rs     # BoxProtocol, BoxConstraints, Size
│   └── sliver_protocol.rs  # SliverProtocol, SliverConstraints, SliverGeometry
├── node/
│   ├── mod.rs              # RenderNode struct
│   ├── children.rs         # RenderChildren (wraps flui-tree Arity)
│   ├── transitions.rs      # State transition implementations
│   └── aliases.rs          # Type aliases (BoxLeaf, BoxSingle, etc.)
├── pipeline/
│   ├── mod.rs
│   └── owner.rs            # PipelineOwner
└── context/
    ├── mod.rs
    └── painting.rs         # PaintingContext
```

## Migration Path

1. **Phase 1**: Implement `RenderState` markers and `Protocol` trait
2. **Phase 2**: Create `RenderNode<P, A, S>` structure
3. **Phase 3**: Implement state transitions
4. **Phase 4**: Add type aliases
5. **Phase 5**: Migrate existing code to use new types
6. **Phase 6**: Remove old unsafe code

## Benefits

| Feature | Benefit |
|---------|---------|
| **RenderState typestate** | Compile-time guarantee: can't read geometry before layout |
| **Protocol generics** | Type-safe Box vs Sliver without runtime checks |
| **Arity reuse** | No code duplication with flui-tree |
| **State transitions** | Explicit, documented lifecycle |
| **Type aliases** | Clean API: `BoxSingle<Attached>` instead of `RenderNode<BoxProtocol, Single, Attached>` |
| **Zero-cost** | PhantomData has no runtime overhead |

## Open Questions

1. **Children state sync**: How to ensure children state matches parent state?
2. **Partial layout**: Can we have some children LaidOut while parent is Attached?
3. **Incremental paint**: How to handle repaint boundaries with typestate?
4. **Tree storage**: How does RenderTree store nodes with different states?
