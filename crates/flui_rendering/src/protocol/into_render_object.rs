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
//!     fn paint(&self, ctx: &mut CanvasContext, offset: Offset) {
//!         let rect = Rect::from_size(self.size).translate(offset);
//!         ctx.canvas().draw_rect(rect, self.color);
//!     }
//!
//!     fn hit_test(&self, ctx: &mut BoxHitTestContext<Leaf, BoxParentData>) -> bool {
//!         ctx.is_within_size(self.size.width, self.size.height)
//!     }
//!
//!     fn size(&self) -> &Size { &self.size }
//!     fn size_mut(&mut self) -> &mut Size { &mut self.size }
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
        // No adapter needed - blanket impl makes T: RenderObject<BoxProtocol>
        RenderEntry::new(Box::new(self))
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
        // No adapter needed - blanket impl makes T: RenderObject<SliverProtocol>
        RenderEntry::new(Box::new(self))
    }

    fn into_render_node(self) -> RenderNode {
        RenderNode::Sliver(self.into_render_entry())
    }
}

// ============================================================================
// Note: BoxProtocolAdapter and SliverProtocolAdapter removed
// ============================================================================
//
// Adapters are no longer needed because:
// 1. Blanket impl in render_box.rs automatically implements RenderObject<BoxProtocol> for all RenderBox types
// 2. Blanket impl in render_sliver.rs automatically implements RenderObject<SliverProtocol> for all RenderSliver types
// 3. This eliminates an unnecessary layer of indirection
// 4. Simpler API: Box::new(render_box) instead of Box::new(adapter)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::Leaf;
    use crate::context::{BoxHitTestContext, BoxLayoutContext};
    use crate::parent_data::BoxParentData;
    use flui_types::Size;

    #[derive(Debug)]
    struct TestBox {
        size: Size,
    }

    impl flui_foundation::Diagnosticable for TestBox {}

    impl RenderBox for TestBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
            // Test implementation
        }

        // paint() uses default no-op

        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            false
        }

        fn size(&self) -> &Size {
            &self.size
        }

        fn size_mut(&mut self) -> &mut Size {
            &mut self.size
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
