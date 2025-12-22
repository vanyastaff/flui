# Protocol System Architecture

## Overview

The Protocol system defines the core contract between render objects and the rendering pipeline. A protocol specifies what types flow through each rendering phase (Layout, Paint, HitTest) without dictating HOW those phases are implemented.

## Design Philosophy

### Protocol = Type Contract, Not Implementation

```
┌─────────────────────────────────────────────────────────┐
│         PROTOCOL (Type System Level)                     │
├─────────────────────────────────────────────────────────┤
│ • WHAT types are used in each phase                     │
│ • HOW to create contexts for each phase                 │
│ • Default parent data for children                      │
└─────────────────────────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────┐
│      IMPLEMENTATION (Widget/RenderObject Level)          │
├─────────────────────────────────────────────────────────┤
│ • HOW to compute layout (flex, stack, grid)             │
│ • HOW to test hits (rect, circle, path)                 │
│ • HOW to paint (canvas operations)                      │
└─────────────────────────────────────────────────────────┘
```

**Key Principle**: Protocol defines the "what", widgets define the "how".

## Protocol Trait

### Core Definition

```rust
pub trait Protocol: Send + Sync + 'static {
    /// Layout capability - defines constraints and geometry
    type Layout: LayoutCapability;
    
    /// Hit test capability - defines position and results
    type HitTest: HitTestCapability;
    
    /// Paint capability - defines rendering backend
    type Paint: PaintCapability;
    
    /// Default ParentData for this protocol
    type DefaultParentData: ParentData;
    
    /// Protocol name for debugging
    fn protocol_name() -> &'static str;
    
    /// Protocol ID for runtime checks
    fn protocol_id() -> ProtocolId;
}
```

### Why Composition?

Instead of cramming all types into one giant trait, we use **capability composition**:

```rust
// ❌ Monolithic (old approach)
trait Protocol {
    type LayoutConstraints;
    type LayoutGeometry;
    type HitTestPosition;
    type HitTestResult;
    type PaintCanvas;
    type PaintLayering;
    // ... 15+ types
}

// ✅ Compositional (current approach)
trait Protocol {
    type Layout: LayoutCapability;    // Group 1: Layout types
    type HitTest: HitTestCapability;  // Group 2: HitTest types
    type Paint: PaintCapability;      // Group 3: Paint types
}
```

**Benefits**:
- Clear separation of concerns
- Easy to understand grouping
- Can reuse capabilities across protocols
- Can extend individual capabilities independently

## Capability Traits

### LayoutCapability (Simple)

```rust
pub trait LayoutCapability: Send + Sync + 'static {
    /// Input constraints (e.g., BoxConstraints, SliverConstraints)
    type Constraints: Clone + Debug + Send + Sync + 'static;
    
    /// Output geometry (e.g., Size, SliverGeometry)
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;
    
    /// Layout context type (GAT!)
    type Context<'ctx, A: Arity, P: ParentData>: LayoutContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
    
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }
}
```

**What it defines**: ONLY the input/output types for layout phase.

**What it does NOT define**: HOW to compute size, HOW to position children (that's widget logic).

### HitTestCapability (Simple)

```rust
pub trait HitTestCapability: Send + Sync + 'static {
    /// Position type (e.g., Offset, AxisPosition)
    type Position: Clone + Debug + Default + Send + Sync + 'static;
    
    /// Result accumulator
    type Result: Default + Send + Sync;
    
    /// Entry type in results
    type Entry: Clone + Debug;
    
    /// Hit test context type (GAT!)
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}
```

**What it defines**: ONLY the position representation and result types.

**What it does NOT define**: HOW to test (rect, circle, path), filtering, propagation (that's widget logic).

### PaintCapability (Decomposed)

```rust
pub trait PaintCapability: Send + Sync + 'static {
    /// Canvas backend (Skia, wgpu, Canvas2D, etc.)
    type Canvas: CanvasApi;
    
    /// Layering strategy (simple, composited, retained)
    type Layering: LayeringStrategy;
    
    /// Effects support (filters, blend modes, shaders)
    type Effects: EffectsApi;
    
    /// Caching strategy (repaint boundaries, GPU cache)
    type Caching: CachingStrategy;
    
    /// Paint context type (GAT!)
    type Context<'ctx, A: Arity, P: ParentData>: PaintContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}
```

**Why is Paint decomposed?**: Unlike Layout/HitTest, paint backends truly vary:
- Canvas: Skia vs wgpu vs Canvas2D vs WebGL
- Layering: Immediate mode vs retained mode vs GPU layers
- Effects: CPU filters vs GPU shaders
- Caching: CPU pictures vs GPU textures

These are **real backend differences**, not implementation details.

## Built-in Protocols

### BoxProtocol

```rust
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Layout = BoxLayout;
    type HitTest = BoxHitTest;
    type Paint = StandardPaint;
    type DefaultParentData = BoxParentData;
    
    fn protocol_name() -> &'static str { "Box" }
    fn protocol_id() -> ProtocolId { ProtocolId::BOX }
}

// Layout capability
pub struct BoxLayout;
impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;  // min/max width/height
    type Geometry = Size;                // width, height
    type Context<'ctx, A, P> = RenderContext<'ctx, BoxProtocol, LayoutPhase, A, P>;
}

// HitTest capability
pub struct BoxHitTest;
impl HitTestCapability for BoxHitTest {
    type Position = Offset;              // x, y coordinates
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A, P> = RenderContext<'ctx, BoxProtocol, HitTestPhase, A, P>;
}

// Paint capability (reuses standard)
pub type StandardPaint = ...;  // Skia + Simple + Standard + RepaintBoundaries
```

**Use case**: 2D rectangular widgets (Container, Padding, Align, etc.)

### SliverProtocol

```rust
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Layout = SliverLayout;
    type HitTest = SliverHitTest;
    type Paint = StandardPaint;  // ✅ Reuses paint!
    type DefaultParentData = SliverParentData;
    
    fn protocol_name() -> &'static str { "Sliver" }
    fn protocol_id() -> ProtocolId { ProtocolId::SLIVER }
}

// Layout capability
pub struct SliverLayout;
impl LayoutCapability for SliverLayout {
    type Constraints = SliverConstraints;  // scroll offset, remaining space
    type Geometry = SliverGeometry;        // paint extent, scroll extent
    type Context<'ctx, A, P> = RenderContext<'ctx, SliverProtocol, LayoutPhase, A, P>;
}

// HitTest capability
pub struct SliverHitTest;
impl HitTestCapability for SliverHitTest {
    type Position = AxisPosition;     // main_axis, cross_axis
    type Result = SliverHitTestResult;
    type Entry = SliverHitTestEntry;
    type Context<'ctx, A, P> = RenderContext<'ctx, SliverProtocol, HitTestPhase, A, P>;
}
```

**Use case**: Scrolling widgets (ListView, GridView, CustomScrollView)

## Context System

### RenderContext - Universal Context

All protocols and phases share a single context type:

```rust
pub struct RenderContext<'ctx, Pr: Protocol, Ph: Phase, A: Arity, P: ParentData> {
    /// Phase-specific data
    phase_data: PhaseData<'ctx, Pr, Ph>,
    
    /// Children access
    children: ChildrenAccess<'ctx, A, P, Ph>,
}

pub enum PhaseData<'ctx, Pr: Protocol, Ph: Phase> {
    Layout {
        constraints: Pr::Layout::Constraints,
    },
    Paint {
        painting_context: &'ctx mut PaintingContext,
        canvas: &'ctx mut Pr::Paint::Canvas,
        // ... other paint components
    },
    HitTest {
        position: Pr::HitTest::Position,
        result: &'ctx mut Pr::HitTest::Result,
    },
}
```

### Type Aliases

```rust
// Box protocol contexts
pub type BoxLayoutContext<'ctx, A, P = BoxParentData> = 
    RenderContext<'ctx, BoxProtocol, LayoutPhase, A, P>;

pub type BoxPaintContext<'ctx, A, P = BoxParentData> = 
    RenderContext<'ctx, BoxProtocol, PaintPhase, A, P>;

pub type BoxHitTestContext<'ctx, A, P = BoxParentData> = 
    RenderContext<'ctx, BoxProtocol, HitTestPhase, A, P>;

// Sliver protocol contexts
pub type SliverLayoutContext<'ctx, A, P = SliverParentData> = 
    RenderContext<'ctx, SliverProtocol, LayoutPhase, A, P>;

// ... etc
```

## Creating Custom Protocols

### Example: 3D Protocol

```rust
pub struct Protocol3D;

impl Protocol for Protocol3D {
    type Layout = Layout3D;
    type HitTest = HitTest3D;
    type Paint = GPUPaint;  // Different from StandardPaint!
    type DefaultParentData = Transform3DParentData;
    
    fn protocol_name() -> &'static str { "3D" }
    fn protocol_id() -> ProtocolId { ProtocolId(1000) }
}

// Custom layout
pub struct Layout3D;
impl LayoutCapability for Layout3D {
    type Constraints = Constraints3D;  // min/max width/height/depth
    type Geometry = Volume;            // width, height, depth
    type Context<'ctx, A, P> = RenderContext<'ctx, Protocol3D, LayoutPhase, A, P>;
}

// Custom hit test
pub struct HitTest3D;
impl HitTestCapability for HitTest3D {
    type Position = Point3D;      // x, y, z
    type Result = RaycastResult;  // Custom result for 3D
    type Entry = RaycastEntry;
    type Context<'ctx, A, P> = RenderContext<'ctx, Protocol3D, HitTestPhase, A, P>;
}

// Custom paint (GPU-accelerated)
pub struct GPUPaint;
impl PaintCapability for GPUPaint {
    type Canvas = WgpuCanvas;           // wgpu backend
    type Layering = CompositedLayering; // GPU layers
    type Effects = ShaderEffects;       // Custom shaders
    type Caching = GPUCaching;          // GPU texture cache
    type Context<'ctx, A, P> = RenderContext<'ctx, Protocol3D, PaintPhase, A, P>;
}
```

## Protocol vs Widget Logic

### What Belongs in Protocol

✅ **Type definitions** that vary between protocols:
- Constraints representation (BoxConstraints vs SliverConstraints)
- Geometry representation (Size vs SliverGeometry vs Volume)
- Position representation (Offset vs AxisPosition vs Point3D)
- Backend choices (Skia vs wgpu)

### What Does NOT Belong in Protocol

❌ **Implementation strategies** that vary between widgets:
- How to compute size (flex vs stack vs grid) → Widget logic
- How to position children (alignment, spacing) → Widget logic
- How to test hits (rect vs circle vs path) → Widget logic
- Filtering/behavior (opaque vs translucent) → Widget property

**Why?** Different widgets in the SAME protocol need different strategies!

```rust
// All Box protocol, different positioning:
RenderFlex   → FlexLayout utility (row/column)
RenderStack  → StackLayout utility (z-order)
RenderAlign  → AlignmentLayout utility (single centered)
RenderGrid   → GridLayout utility (rows/columns)

// All Box protocol, different hit testing:
RenderColoredBox   → Rectangle test
RenderCircleAvatar → Circle test
RenderCustomClipper → Path test
```

## Phase-Specific Extensions

### Extension Traits for Phase-Specific APIs

```rust
// Layout phase
pub trait LayoutPhaseContext<Pr: Protocol> {
    fn constraints(&self) -> &Pr::Layout::Constraints;
    fn constraints_mut(&mut self) -> &mut Pr::Layout::Constraints;
}

// HitTest phase
pub trait HitTestPhaseContext<Pr: Protocol> {
    fn position(&self) -> &Pr::HitTest::Position;
    fn result(&mut self) -> &mut Pr::HitTest::Result;
}

// Paint phase
pub trait PaintPhaseContext<Pr: Protocol> {
    fn canvas(&mut self) -> &mut Pr::Paint::Canvas;
    fn layering(&mut self) -> &mut Pr::Paint::Layering;
    fn effects(&mut self) -> &mut Pr::Paint::Effects;
}
```

These are implemented automatically for `RenderContext` with the appropriate phase type.

## Best Practices

### 1. Keep Protocols Simple

```rust
// ✅ Good - minimal, focused
pub trait LayoutCapability {
    type Constraints;
    type Geometry;
    type Context<...>;
}

// ❌ Bad - too many responsibilities
pub trait LayoutCapability {
    type Constraints;
    type Geometry;
    type SizingStrategy;      // Widget logic!
    type PositioningStrategy; // Widget logic!
    type IntrinsicsApi;       // Optional feature!
    type BaselineApi;         // Optional feature!
}
```

### 2. Reuse Capabilities When Possible

```rust
// ✅ Good - both protocols reuse StandardPaint
impl Protocol for BoxProtocol {
    type Paint = StandardPaint;
}

impl Protocol for SliverProtocol {
    type Paint = StandardPaint;  // Same!
}

// Only create new capability when backends truly differ
impl Protocol for Protocol3D {
    type Paint = GPUPaint;  // Different rendering approach
}
```

### 3. Use Type Aliases for Ergonomics

```rust
// ✅ Provide convenient aliases
pub type BoxLayoutContext<'ctx, A, P = BoxParentData> = 
    RenderContext<'ctx, BoxProtocol, LayoutPhase, A, P>;

// Users write this:
fn perform_layout(&mut self, ctx: BoxLayoutContext<'_, Optional>) -> Size

// Instead of this:
fn perform_layout(
    &mut self, 
    ctx: RenderContext<'_, BoxProtocol, LayoutPhase, Optional, BoxParentData>
) -> Size
```

### 4. Document Protocol Contracts

Each protocol should clearly document:
- What types it expects
- What guarantees it provides
- When to use it vs other protocols
- Examples of widgets that use it

## Future Extensions

### Potential New Protocols

- **TableProtocol**: Spreadsheet-like layout with merged cells
- **CanvasProtocol**: Free-form drawing with transforms
- **GraphProtocol**: Node-based layout (graphs, trees)
- **VRProtocol**: Virtual reality 3D environments

### Protocol Composition

Future: Allow protocols to compose/extend other protocols:

```rust
// Hypothetical future feature
pub struct TableProtocol: BoxProtocol {  // Extends Box
    type CellConstraints = ...;
    type MergeInfo = ...;
}
```

## Summary

**Protocol defines the type contract, not the implementation.**

| Aspect | Protocol Level | Widget Level |
|--------|---------------|--------------|
| Layout | Constraints → Geometry types | How to compute size, position children |
| HitTest | Position → Result types | How to test (rect/circle/path) |
| Paint | Canvas/Backend types | What to draw |
| Optional | Default ParentData | HasIntrinsics, HasBaseline |

**Key Insight**: Protocol is about what types flow through the rendering pipeline. Widgets decide how to use those types to implement their specific behavior.
