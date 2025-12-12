# flui_rendering

FLUI rendering layer implementing Flutter-inspired protocol-based architecture.

## Architecture

The rendering system uses a **Protocol-based architecture** with compile-time type safety:

```rust
pub trait Protocol {
    type Object: ?Sized;           // dyn RenderBox or dyn RenderSliver
    type Constraints: Clone;       // BoxConstraints or SliverConstraints
    type ParentData: ParentData;   // BoxParentData or SliverParentData
    type Geometry: Clone;          // Size or SliverGeometry
}
```

## Core Components

### ✅ Implemented

- **Protocol System** (`protocol.rs`)
  - `Protocol` trait with associated types
  - `BoxProtocol` for 2D layout
  - `SliverProtocol` for scrollable content

- **Constraints** (`constraints/`)
  - `BoxConstraints` - min/max width/height bounds
  - `SliverConstraints` - viewport and scroll info

- **Geometry** (`geometry/`)
  - `Size` - width and height (re-exported from flui_types)
  - `SliverGeometry` - scroll/paint extents

- **Parent Data** (`parent_data/`)
  - `ParentData` trait with downcast-rs and dyn-clone
  - `BoxParentData` - offset positioning
  - `SliverParentData` - scroll axis positioning

- **Containers** (`containers/`) with **Arity Integration**
  - `Single<P, A>` - zero or one child with arity constraint
  - `Children<P, PD, A>` - multiple children with arity validation
  - `Proxy<P, A>` - pass-through with geometry (default: Exact<1>)
  - `Shifted<P, A>` - custom offset (default: Exact<1>)
  - `Aligning<P, A>` - alignment and size factors (default: Exact<1>)
  - `Adapter<C, ToProtocol>` - cross-protocol composition (Box ↔ Sliver)

- **Trait Hierarchy** (`traits/`) with **Ambassador Delegation**
  - `RenderObject` - base trait
  - **Box Protocol:**
    - `RenderBox` - 2D cartesian layout
    - `SingleChildRenderBox` - one child accessor
    - `RenderProxyBox` - pass-through (size = child size)
    - `RenderShiftedBox` - custom positioning
    - `RenderAligningShiftedBox` - alignment-based
    - `MultiChildRenderBox` - multiple children
  - **Sliver Protocol:**
    - `RenderSliver` - scrollable content
    - `RenderProxySliver` - pass-through sliver
    - `RenderSliverSingleBoxAdapter` - sliver wrapping box
    - `RenderSliverMultiBoxAdaptor` - sliver with boxes

## Key Benefits

✅ **Compile-time type safety**: Protocol mismatch caught at compile time
✅ **Arity validation**: Child count constraints enforced via ArityStorage
✅ **Zero-cost abstractions**: Generic containers with no runtime overhead
✅ **No downcasts**: Direct method access via `Protocol::Object`
✅ **Cross-protocol composition**: Adapter pattern for Box ↔ Sliver
✅ **Extensible**: Add new protocols without changing core system

## Usage

### Arity Constraints

Containers use ArityStorage from flui-tree for compile-time child count validation:

```rust
use flui_rendering::prelude::*;

// Optional: 0 or 1 child
let single: Single<BoxProtocol, Optional> = Single::new();

// Exact<1>: exactly 1 child (default for Proxy, Shifted, Aligning)
let proxy: ProxyBox = ProxyBox::new();  // Uses Exact<1>

// Variable: any number of children (default for Children)
let children: BoxChildren = BoxChildren::new();

// Range<MIN, MAX>: between MIN and MAX children
let ranged: Children<BoxProtocol, BoxParentData, Range<2, 4>> = Children::new();
```

### Cross-Protocol Adapters

Use adapters to compose Box and Sliver render objects:

```rust
// Wrap Box child in Sliver protocol
type BoxToSliver = Adapter<Single<BoxProtocol>, SliverProtocol>;

// Multiple Box children in Sliver (for lists, grids)
type MultiBoxToSliver = Adapter<Children<BoxProtocol>, SliverProtocol>;

struct RenderSliverToBoxAdapter {
    adapter: BoxToSliver,
}
```

### With Ambassador Delegation

```rust
use flui_rendering::prelude::*;
use ambassador::Delegate;

// ProxyBox pattern - minimal boilerplate!
#[derive(Delegate)]
#[delegate(SingleChildRenderBox, target = "proxy")]
#[delegate(RenderObject, target = "proxy")]
struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

// Just implement the marker trait!
impl RenderProxyBox for RenderOpacity {}

// Only override what you need to customize
impl RenderBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        RenderProxyBox::perform_layout(self, constraints)
    }

    fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        if self.opacity < 1.0 {
            // Apply opacity
        }
        RenderProxyBox::paint(self, context, offset);
    }
}
```

## Next Steps

The following components are documented but not yet implemented:

- Pipeline system (layout, paint, compositing)
- Render objects (85+ concrete implementations)
- Layer system (15 layer types)
- Delegates (CustomPainter, CustomClipper, etc.)
- Advanced traits (RenderProxyBox, RenderShiftedBox, etc.)

See `docs/` directory for complete implementation guide.

## Documentation

- `docs/README.md` - Architecture overview
- `docs/core/Protocol.md` - Protocol system details
- `docs/reference/Implementation Guide.md` - Step-by-step instructions
- `examples/basic_usage.rs` - Working example

## License

MIT OR Apache-2.0
