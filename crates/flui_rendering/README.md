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

- **Containers** (`containers/`)
  - `Single<P>` - zero or one child
  - `Children<P, PD>` - multiple children
  - `Proxy<P>` - pass-through with geometry
  - `Shifted<P>` - custom offset
  - `Aligning<P>` - alignment and size factors

- **Traits** (`traits/`)
  - `RenderObject` - base trait for all render objects
  - `RenderBox` - 2D cartesian layout
  - `RenderSliver` - scrollable content

## Key Benefits

✅ **Compile-time type safety**: Protocol mismatch caught at compile time
✅ **Zero-cost abstractions**: Generic containers with no runtime overhead
✅ **No downcasts**: Direct method access via `Protocol::Object`
✅ **Extensible**: Add new protocols without changing core system

## Usage

```rust
use flui_rendering::prelude::*;

struct RenderMyWidget {
    proxy: ProxyBox,  // Uses BoxProtocol automatically
}

impl RenderBox for RenderMyWidget {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = if let Some(child) = self.proxy.child_mut() {
            child.perform_layout(constraints)  // Direct access, no downcast!
        } else {
            constraints.smallest()
        };
        self.proxy.set_geometry(size);
        size
    }

    fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        // Paint implementation
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
