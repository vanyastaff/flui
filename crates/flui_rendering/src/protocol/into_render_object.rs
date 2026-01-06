//! IntoRenderObject trait for converting RenderBox/RenderSliver into storage-ready nodes.
//!
//! This module provides the `IntoRenderObject` trait that replaces the old wrapper approach.
//! Instead of wrapping concrete types in BoxWrapper/SliverWrapper, we directly convert them
//! into RenderEntry<Protocol> for storage in RenderTree.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                  User Implementation                         │
//! ├──────────────────────────────────────────────────────────────┤
//! │  struct MyColoredBox { color, size }                         │
//! │  impl RenderBox for MyColoredBox { ... }                     │
//! └──────────────────────────────────────────────────────────────┘
//!                           ↓ into_render_object()
//! ┌──────────────────────────────────────────────────────────────┐
//! │              RenderEntry<BoxProtocol>                        │
//! ├──────────────────────────────────────────────────────────────┤
//! │  render_object: RwLock<Box<dyn RenderObject<P>>>              │
//! │  state: RenderState<P>                                       │
//! │  links: NodeLinks                                            │
//! └──────────────────────────────────────────────────────────────┘
//!                           ↓ wrapped in
//! ┌──────────────────────────────────────────────────────────────┐
//! │                    RenderNode::Box                           │
//! ├──────────────────────────────────────────────────────────────┤
//! │  Box(RenderEntry<BoxProtocol>)                               │
//! └──────────────────────────────────────────────────────────────┘
//!                           ↓ stored in
//! ┌──────────────────────────────────────────────────────────────┐
//! │                      RenderTree                              │
//! ├──────────────────────────────────────────────────────────────┤
//! │  nodes: Slab<RenderNode>                                     │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Benefits over Wrapper Approach
//!
//! 1. **No Wrapper Boilerplate**: Direct conversion to RenderEntry
//! 2. **Better Type Safety**: Protocol system enforced at creation time
//! 3. **Cleaner API**: `my_box.into_render_object()` vs `BoxWrapper::new(my_box)`
//! 4. **Storage Efficiency**: One less layer of indirection
//! 5. **Protocol Flexibility**: Easy to add new protocols
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::prelude::*;
//!
//! struct MyColoredBox {
//!     color: Color,
//!     size: Size,
//! }
//!
//! impl RenderBox for MyColoredBox {
//!     type Arity = Leaf;
//!     type ParentData = BoxParentData;
//!
//!     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) {
//!         let size = ctx.constraints().constrain(self.size);
//!         ctx.complete_with_size(size);
//!     }
//!
//!     fn paint(&mut self, ctx: &mut BoxPaintContext<Leaf, BoxParentData>) {
//!         let rect = Rect::from_size(self.size).translate(ctx.offset());
//!         ctx.canvas().draw_rect(rect, self.color);
//!     }
//!
//!     fn hit_test(&self, ctx: &mut BoxHitTestContext<Leaf, BoxParentData>) -> bool {
//!         ctx.is_within_size(self.size.width, self.size.height)
//!     }
//!
//!     fn size(&self) -> Size { self.size }
//!     fn set_size(&mut self, size: Size) { self.size = size; }
//! }
//!
//! // Create and insert into tree
//! let my_box = MyColoredBox { color: Color::RED, size: Size::new(100.0, 50.0) };
//! let node = my_box.into_render_node();
//! let id = tree.insert(node);
//! ```

use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::storage::{RenderEntry, RenderNode};
use crate::traits::{RenderBox, RenderSliver};

// ============================================================================
// IntoRenderObject Trait
// ============================================================================

/// Trait for converting concrete render objects into storage-ready nodes.
///
/// This trait provides a unified way to convert typed render objects
/// (implementing RenderBox or RenderSliver) into protocol-specific
/// RenderEntry instances suitable for storage in RenderTree.
///
/// # Implementations
///
/// - Auto-implemented for all `T: RenderBox` → `RenderEntry<BoxProtocol>`
/// - Auto-implemented for all `T: RenderSliver` → `RenderEntry<SliverProtocol>`
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
pub trait IntoRenderObject<P: Protocol>: Sized {
    /// Converts self into a RenderEntry for the given protocol.
    ///
    /// This consumes self and wraps it in the appropriate storage structure.
    fn into_render_entry(self) -> RenderEntry<P>;

    /// Converts self into a RenderNode enum variant.
    ///
    /// This is a convenience method that calls `into_render_entry()` and wraps
    /// the result in the appropriate RenderNode enum variant.
    fn into_render_node(self) -> RenderNode;
}

// ============================================================================
// Blanket Implementation for RenderBox
// ============================================================================

impl<T> IntoRenderObject<BoxProtocol> for T
where
    T: RenderBox + Send + Sync + 'static,
{
    fn into_render_entry(self) -> RenderEntry<BoxProtocol> {
        // Convert to RenderObject<P> adapter
        let adapter = BoxProtocolAdapter::new(self);
        RenderEntry::new(Box::new(adapter))
    }

    fn into_render_node(self) -> RenderNode {
        RenderNode::Box(self.into_render_entry())
    }
}

// ============================================================================
// Blanket Implementation for RenderSliver
// ============================================================================

impl<T> IntoRenderObject<SliverProtocol> for T
where
    T: RenderSliver + Send + Sync + 'static,
{
    fn into_render_entry(self) -> RenderEntry<SliverProtocol> {
        // Convert to RenderObject<P> adapter
        let adapter = SliverProtocolAdapter::new(self);
        RenderEntry::new(Box::new(adapter))
    }

    fn into_render_node(self) -> RenderNode {
        RenderNode::Sliver(self.into_render_entry())
    }
}

// ============================================================================
// Protocol Adapters
// ============================================================================

use crate::protocol::RenderObject;
use std::fmt;

/// Adapter that converts RenderBox into RenderObject<BoxProtocol>.
///
/// This adapter bridges the gap between the typed RenderBox API with Arity/ParentData
/// and the protocol-specific RenderObject<P> trait needed for storage.
pub struct BoxProtocolAdapter<T: RenderBox> {
    inner: T,
    /// Cached geometry from last layout
    geometry: flui_types::Size,
}

impl<T: RenderBox> BoxProtocolAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            geometry: flui_types::Size::ZERO,
        }
    }
}

impl<T: RenderBox> fmt::Debug for BoxProtocolAdapter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxProtocolAdapter")
            .field("inner", &self.inner)
            .field("geometry", &self.geometry)
            .finish()
    }
}

impl<T: RenderBox> flui_foundation::Diagnosticable for BoxProtocolAdapter<T> {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("geometry", format!("{:?}", self.geometry));
        builder.add("inner_type", std::any::type_name::<T>());
    }
}

impl<T: RenderBox> RenderObject<BoxProtocol> for BoxProtocolAdapter<T> {
    fn perform_layout_raw(
        &mut self,
        constraints: crate::protocol::ProtocolConstraints<BoxProtocol>,
    ) -> crate::protocol::ProtocolGeometry<BoxProtocol> {
        // TODO: Create proper BoxLayoutContext and call inner.perform_layout()
        // For now, return current size
        let size = self.inner.size();
        self.inner.set_size(size);
        size
    }

    fn paint(&self, _context: &mut crate::pipeline::CanvasContext, _offset: flui_types::Offset) {
        // TODO: Create proper BoxPaintContext and call inner.paint()
    }

    fn hit_test_raw(
        &self,
        _result: &mut crate::protocol::ProtocolHitResult<BoxProtocol>,
        _position: crate::protocol::ProtocolPosition<BoxProtocol>,
    ) -> bool {
        // TODO: Create proper BoxHitTestContext and call inner.hit_test()
        false
    }

    fn geometry(&self) -> &crate::protocol::ProtocolGeometry<BoxProtocol> {
        &self.geometry
    }

    fn set_geometry(&mut self, geometry: crate::protocol::ProtocolGeometry<BoxProtocol>) {
        self.geometry = geometry;
        self.inner.set_size(geometry);
    }

    fn paint_bounds(&self) -> flui_types::Rect {
        self.inner.box_paint_bounds()
    }
}

/// Adapter that converts RenderSliver into RenderObject<SliverProtocol>.
pub struct SliverProtocolAdapter<T: RenderSliver> {
    inner: T,
    /// Cached geometry from last layout
    geometry: crate::constraints::SliverGeometry,
}

impl<T: RenderSliver> SliverProtocolAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            geometry: crate::constraints::SliverGeometry::default(),
        }
    }
}

impl<T: RenderSliver> fmt::Debug for SliverProtocolAdapter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverProtocolAdapter")
            .field("inner", &self.inner)
            .field("geometry", &self.geometry)
            .finish()
    }
}

impl<T: RenderSliver> flui_foundation::Diagnosticable for SliverProtocolAdapter<T> {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("geometry", format!("{:?}", self.geometry));
        builder.add("inner_type", std::any::type_name::<T>());
    }
}

impl<T: RenderSliver> RenderObject<SliverProtocol> for SliverProtocolAdapter<T> {
    fn perform_layout_raw(
        &mut self,
        _constraints: crate::protocol::ProtocolConstraints<SliverProtocol>,
    ) -> crate::protocol::ProtocolGeometry<SliverProtocol> {
        // TODO: Create proper SliverLayoutContext and call inner.perform_layout()
        // For now, return default geometry
        crate::constraints::SliverGeometry::default()
    }

    fn paint(&self, _context: &mut crate::pipeline::CanvasContext, _offset: flui_types::Offset) {
        // TODO: Create proper SliverPaintContext and call inner.paint()
    }

    fn hit_test_raw(
        &self,
        _result: &mut crate::protocol::ProtocolHitResult<SliverProtocol>,
        _position: crate::protocol::ProtocolPosition<SliverProtocol>,
    ) -> bool {
        // TODO: Create proper SliverHitTestContext and call inner.hit_test()
        false
    }

    fn geometry(&self) -> &crate::protocol::ProtocolGeometry<SliverProtocol> {
        &self.geometry
    }

    fn set_geometry(&mut self, geometry: crate::protocol::ProtocolGeometry<SliverProtocol>) {
        self.geometry = geometry;
    }

    fn paint_bounds(&self) -> flui_types::Rect {
        // TODO: Implement sliver paint bounds
        flui_types::Rect::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::Leaf;
    use crate::parent_data::BoxParentData;
    use flui_types::Size;

    #[derive(Debug)]
    struct TestBox {
        size: Size,
    }

    impl RenderBox for TestBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
            // Test implementation
        }

        fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Leaf, BoxParentData>) {}

        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            false
        }

        fn size(&self) -> Size {
            self.size
        }

        fn set_size(&mut self, size: Size) {
            self.size = size;
        }
    }

    #[test]
    fn test_into_render_entry() {
        let test_box = TestBox {
            size: Size::new(100.0, 50.0),
        };
        let _entry: RenderEntry<BoxProtocol> = test_box.into_render_entry();
        // Entry created successfully
    }

    #[test]
    fn test_into_render_node() {
        let test_box = TestBox {
            size: Size::new(100.0, 50.0),
        };
        let node = test_box.into_render_node();
        assert!(node.is_box());
        assert!(!node.is_sliver());
    }
}
