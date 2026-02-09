# Protocol Architecture

## Overview

FLUI uses a **composition-based protocol system** that separates layout, hit-testing, and painting concerns into composable capabilities. This architecture enables:

- **Type-safe rendering** - compile-time guarantees for constraints and geometry types
- **Backend flexibility** - swap Canvas implementations without changing render logic
- **Clear separation** - each capability groups related types
- **Easy extension** - add new protocols by composing existing capabilities

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                    CAPABILITY TRAITS                            │
│                    (Building Blocks)                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │LayoutCapability │  │HitTestCapability│  │ PaintCapability │ │
│  ├─────────────────┤  ├─────────────────┤  ├─────────────────┤ │
│  │ Constraints     │  │ Position        │  │ Canvas          │ │
│  │ Geometry        │  │ Result          │  │ Layering        │ │
│  │ Context<GAT>    │  │ Entry           │  │ Effects         │ │
│  │                 │  │ Context<GAT>    │  │ Caching         │ │
│  │                 │  │                 │  │ Context<GAT>    │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘ │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      PROTOCOL TRAIT                             │
│                   (Composition Layer)                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  trait Protocol {                                               │
│      type Layout: LayoutCapability;      // Constraints+Size    │
│      type HitTest: HitTestCapability;    // Position+Result     │
│      type Paint: PaintCapability;        // Canvas+Effects      │
│      type DefaultParentData;             // Child metadata      │
│  }                                                              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌──────────────────────┐        ┌──────────────────────┐
│     BoxProtocol      │        │    SliverProtocol    │
├──────────────────────┤        ├──────────────────────┤
│ Layout = BoxLayout   │        │ Layout = SliverLayout│
│ HitTest = BoxHitTest │        │ HitTest = SliverHT   │
│ Paint = StandardPaint│◄─────► │ Paint = StandardPaint│
│                      │ SHARED │                      │
└──────────────────────┘        └──────────────────────┘
```

## Core Concepts

### 1. Capabilities

Capabilities are trait groups that define related types and behaviors:

#### LayoutCapability
Groups layout-related types:
- `Constraints` - Input constraints (BoxConstraints, SliverConstraints)
- `Geometry` - Output geometry (Size, SliverGeometry)
- `Context<'ctx, A, P>` - GAT for layout context

```rust
pub trait LayoutCapability: Send + Sync + 'static {
    type Constraints: Clone + Debug + Send + Sync;
    type Geometry: Clone + Debug + Default + Send + Sync;
    type Context<'ctx, A: Arity, P: ParentData>: LayoutContextApi<'ctx, Self, A, P>
    where Self: 'ctx;
    
    fn default_geometry() -> Self::Geometry;
}
```

#### HitTestCapability
Groups hit-testing types:
- `Position` - Hit position type (Offset for Box, MainAxisPosition for Sliver)
- `Result` - Accumulated hit results
- `Entry` - Individual hit entry
- `Context<'ctx, A, P>` - GAT for hit-test context

```rust
pub trait HitTestCapability: Send + Sync + 'static {
    type Position: Clone + Debug + Default;
    type Result: Default;
    type Entry: Clone + Debug;
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where Self: 'ctx;
}
```

#### PaintCapability
Composed of four orthogonal strategies:
- `Canvas` - Drawing backend (Skia, wgpu, Canvas2D)
- `Layering` - Layer organization (Simple, Composited)
- `Effects` - Visual effects (Standard, Shader)
- `Caching` - Repaint optimization (RepaintBoundaries, GPU)

```rust
pub trait PaintCapability: Send + Sync + 'static {
    type Canvas: CanvasApi;
    type Layering: LayeringStrategy;
    type Effects: EffectsApi;
    type Caching: CachingStrategy;
    type Context<'ctx, A: Arity, P: ParentData>: PaintContextApi<'ctx, Self, A, P>
    where Self: 'ctx;
}
```

### 2. Protocol

Protocol composes three capabilities into a complete rendering protocol:

```rust
pub trait Protocol: Send + Sync + 'static {
    type Layout: LayoutCapability;
    type HitTest: HitTestCapability;
    type Paint: PaintCapability;
    type DefaultParentData: ParentData + Default;
    
    fn protocol_name() -> &'static str;
    fn protocol_id() -> ProtocolId;
}
```

### 3. Contexts

Each capability defines a context type via GAT:

```rust
// Layout context for Box protocol
pub struct BoxLayoutCtx<'a, A: Arity, P: ParentData = BoxParentData> {
    pub constraints: BoxConstraints,
    pub children: ChildrenAccess<'a, A, P, LayoutPhase>,
}

// Paint context (shared across protocols)
pub struct PaintCtx<'a, A: Arity, P: ParentData> {
    pub canvas: &'a mut dyn CanvasApi,
    pub layering: &'a mut dyn LayeringStrategy,
    pub effects: &'a mut dyn EffectsApi,
    pub children: ChildrenAccess<'a, A, P, PaintPhase>,
}
```

## Type Parameters

### Arity
Compile-time child count constraint:
- `Leaf` - No children (Text, Image)
- `Single` - Exactly one child (Padding, Center)
- `Optional` - Zero or one child (Container)
- `Variable` - Any number of children (Row, Column, Stack)

### ParentData
Metadata stored on children by parent:
- `BoxParentData` - Basic offset
- `FlexParentData` - Flex factor, fit
- `StackParentData` - Positioned, alignment

### Phase
Compile-time phase marker:
- `LayoutPhase` - Can layout children, set offsets
- `PaintPhase` - Can paint children
- `HitTestPhase` - Can hit-test children

## Usage Example

```rust
impl RenderBox for RenderPadding {
    type Arity = Single;
    type ParentData = BoxParentData;
    
    fn perform_layout(&mut self, mut ctx: BoxLayoutCtx<Single>) -> Size {
        // Deflate constraints by padding
        let inner = ctx.constraints.deflate(self.padding);
        
        // Layout child with inner constraints
        let child_size = ctx.children.single(|child| {
            child.layout(inner)
        });
        
        // Set child offset
        ctx.children.single(|child| {
            child.set_offset(self.padding.top_left());
        });
        
        // Return padded size
        ctx.constraints.constrain(child_size + self.padding.size())
    }
    
    fn paint(&self, mut ctx: PaintCtx<Single>) {
        // Paint background
        ctx.canvas.draw_rect(self.bounds(), &self.background_paint);
        
        // Paint child
        ctx.children.single(|child| child.paint());
    }
    
    fn hit_test(&self, ctx: BoxHitTestCtx<Single>) -> bool {
        if !self.bounds().contains(ctx.position) {
            return false;
        }
        
        ctx.children.single(|child| {
            child.hit_test(ctx.position - self.offset)
        })
    }
}
```

## Comparison with Flutter

| Aspect | Flutter | FLUI |
|--------|---------|------|
| Protocol definition | Implicit via inheritance | Explicit trait composition |
| Type safety | Runtime checks | Compile-time via GATs |
| Child count | Runtime validation | Arity system |
| ParentData | Casting required | Type-safe access |
| Paint backend | Skia only | Pluggable via CanvasApi |
| Extensibility | Inheritance chains | Capability composition |

## File Organization

```
flui_rendering/src/
├── protocol/
│   ├── mod.rs
│   ├── core.rs              # Protocol trait
│   ├── capabilities/
│   │   ├── mod.rs
│   │   ├── layout.rs        # LayoutCapability
│   │   ├── hit_test.rs      # HitTestCapability
│   │   └── paint/
│   │       ├── mod.rs       # PaintCapability
│   │       ├── canvas.rs    # CanvasApi
│   │       ├── layering.rs  # LayeringStrategy
│   │       ├── effects.rs   # EffectsApi
│   │       └── caching.rs   # CachingStrategy
│   ├── box_protocol.rs      # BoxProtocol
│   └── sliver_protocol.rs   # SliverProtocol
├── context/
│   ├── mod.rs
│   ├── box_layout.rs        # BoxLayoutCtx
│   ├── box_hit_test.rs      # BoxHitTestCtx
│   ├── sliver_layout.rs     # SliverLayoutCtx
│   ├── sliver_hit_test.rs   # SliverHitTestCtx
│   └── paint.rs             # PaintCtx (shared)
└── traits/
    ├── render_box.rs        # RenderBox trait
    └── render_sliver.rs     # RenderSliver trait
```
