//! Type-erased interface for heterogeneous RenderElement storage.
//!
//! This module provides the type erasure boundary that allows storing
//! different `RenderElement<R, P>` instances in a single collection.
//!
//! # Architecture
//!
//! ```text
//! RenderElement<R, P>           Concrete, fully typed
//!        │
//!        ▼
//! dyn RenderElementNode         Type-erased interface for storage
//!        │
//!        ▼
//! ElementNodeStorage            Wrapper for Box<dyn RenderElementNode>
//! ```
//!
//! # Type Safety
//!
//! The generic `RenderElement<R, P>` provides compile-time type safety,
//! while `RenderElementNode` trait provides the runtime interface for
//! tree operations that work across different element types.

use std::any::Any;
use std::fmt;

use flui_foundation::ElementId;
use flui_types::{Offset, Size, SliverGeometry};

use crate::flags::AtomicRenderFlags;
use crate::lifecycle::RenderLifecycle;
use crate::parent_data::ParentData;
use crate::protocol::ProtocolId;
use crate::state::{BoxRenderState, SliverRenderState};
use crate::tree::RenderId;
use crate::BoxConstraints;
use flui_tree::RuntimeArity;
use flui_types::SliverConstraints;

// ============================================================================
// RENDER ELEMENT NODE TRAIT
// ============================================================================

/// Type-erased interface for RenderElement storage.
///
/// This trait provides the runtime interface for working with heterogeneous
/// `RenderElement<R, P>` instances. It enables storing different element
/// types in a single tree structure while preserving type information
/// for downcasting when needed.
///
/// # Design Principles
///
/// 1. **Minimal interface**: Only methods needed for tree operations
/// 2. **Safe downcasting**: Full `Any` support for typed access
/// 3. **Protocol-aware**: Access to constraints/geometry via protocol ID
/// 4. **Flutter-compliant**: Matches Flutter's RenderObject interface
pub trait RenderElementNode: Any + Send + Sync + fmt::Debug {
    // ========================================================================
    // IDENTITY
    // ========================================================================

    /// Returns this element's ID (if mounted).
    fn id(&self) -> Option<ElementId>;

    /// Returns parent element ID (None for root).
    fn parent(&self) -> Option<ElementId>;

    /// Returns child element IDs.
    fn children(&self) -> &[ElementId];

    /// Returns depth in tree (0 for root).
    fn depth(&self) -> usize;

    // ========================================================================
    // PROTOCOL & ARITY
    // ========================================================================

    /// Returns the protocol identifier (Box or Sliver).
    fn protocol_id(&self) -> ProtocolId;

    /// Returns runtime arity (child count validation).
    fn arity(&self) -> RuntimeArity;

    /// Returns true if this is a Box protocol element.
    fn is_box(&self) -> bool {
        self.protocol_id() == ProtocolId::Box
    }

    /// Returns true if this is a Sliver protocol element.
    fn is_sliver(&self) -> bool {
        self.protocol_id() == ProtocolId::Sliver
    }

    // ========================================================================
    // RENDER ID
    // ========================================================================

    /// Returns reference to RenderObject in RenderTree.
    fn render_id(&self) -> Option<RenderId>;

    /// Sets reference to RenderObject in RenderTree.
    fn set_render_id(&mut self, render_id: Option<RenderId>);

    // ========================================================================
    // OFFSET (protocol-agnostic)
    // ========================================================================

    /// Returns offset relative to parent.
    fn offset(&self) -> Offset;

    /// Sets offset relative to parent.
    fn set_offset(&mut self, offset: Offset);

    // ========================================================================
    // FLAGS (atomic, lock-free)
    // ========================================================================

    /// Returns reference to atomic render flags.
    fn flags(&self) -> &AtomicRenderFlags;

    /// Returns true if layout is needed.
    fn needs_layout(&self) -> bool {
        self.flags().needs_layout()
    }

    /// Returns true if paint is needed.
    fn needs_paint(&self) -> bool {
        self.flags().needs_paint()
    }

    /// Returns true if compositing is needed.
    fn needs_compositing(&self) -> bool {
        self.flags().needs_compositing()
    }

    // ========================================================================
    // LIFECYCLE
    // ========================================================================

    /// Returns current lifecycle state.
    fn lifecycle(&self) -> RenderLifecycle;

    /// Returns true if element is attached to tree.
    fn is_attached(&self) -> bool {
        self.lifecycle().is_attached()
    }

    /// Returns true if element is detached from tree.
    fn is_detached(&self) -> bool {
        self.lifecycle().is_detached()
    }

    // ========================================================================
    // PARENT DATA
    // ========================================================================

    /// Returns parent data (if set).
    fn parent_data(&self) -> Option<&dyn ParentData>;

    /// Returns mutable parent data.
    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData>;

    /// Sets parent data.
    fn set_parent_data(&mut self, parent_data: Box<dyn ParentData>);

    // ========================================================================
    // BOX PROTOCOL ACCESS
    // ========================================================================

    /// Returns BoxRenderState (if Box protocol).
    fn as_box_state(&self) -> Option<&BoxRenderState>;

    /// Returns mutable BoxRenderState (if Box protocol).
    fn as_box_state_mut(&mut self) -> Option<&mut BoxRenderState>;

    /// Returns size (Box protocol only).
    fn size(&self) -> Option<Size> {
        self.as_box_state().map(|s| s.size())
    }

    /// Returns box constraints (Box protocol only).
    fn constraints_box(&self) -> Option<BoxConstraints> {
        self.as_box_state().and_then(|s| s.constraints().copied())
    }

    // ========================================================================
    // SLIVER PROTOCOL ACCESS
    // ========================================================================

    /// Returns SliverRenderState (if Sliver protocol).
    fn as_sliver_state(&self) -> Option<&SliverRenderState>;

    /// Returns mutable SliverRenderState (if Sliver protocol).
    fn as_sliver_state_mut(&mut self) -> Option<&mut SliverRenderState>;

    /// Returns sliver geometry (Sliver protocol only).
    fn sliver_geometry(&self) -> Option<SliverGeometry> {
        self.as_sliver_state().and_then(|s| s.geometry())
    }

    /// Returns sliver constraints (Sliver protocol only).
    fn constraints_sliver(&self) -> Option<SliverConstraints> {
        self.as_sliver_state()
            .and_then(|s| s.constraints().copied())
    }

    // ========================================================================
    // DOWNCASTING
    // ========================================================================

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns self as mutable `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ========================================================================
    // DEBUG
    // ========================================================================

    /// Returns debug name.
    fn debug_name(&self) -> &'static str;
}

// ============================================================================
// ELEMENT NODE STORAGE
// ============================================================================

/// Storage wrapper for type-erased RenderElement.
///
/// This provides a convenient wrapper around `Box<dyn RenderElementNode>`
/// with additional helper methods for common operations.
///
/// # Usage
///
/// ```rust,ignore
/// // Create storage from typed element
/// let element: RenderElement<RenderPadding, BoxProtocol> = ...;
/// let storage = ElementNodeStorage::new(element);
///
/// // Access via trait methods
/// let offset = storage.offset();
/// let protocol = storage.protocol_id();
///
/// // Downcast back to typed element
/// if let Some(typed) = storage.downcast_ref::<RenderElement<RenderPadding, BoxProtocol>>() {
///     // Use typed element
/// }
/// ```
#[derive(Debug)]
pub struct ElementNodeStorage {
    node: Box<dyn RenderElementNode>,
}

impl ElementNodeStorage {
    /// Creates new storage from a type that implements RenderElementNode.
    pub fn new<T: RenderElementNode + 'static>(node: T) -> Self {
        Self {
            node: Box::new(node),
        }
    }

    /// Creates storage from a boxed trait object.
    pub fn from_boxed(node: Box<dyn RenderElementNode>) -> Self {
        Self { node }
    }

    /// Returns reference to the inner node.
    #[inline]
    pub fn inner(&self) -> &dyn RenderElementNode {
        &*self.node
    }

    /// Returns mutable reference to the inner node.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut dyn RenderElementNode {
        &mut *self.node
    }

    /// Unwraps into the boxed trait object.
    #[inline]
    pub fn into_inner(self) -> Box<dyn RenderElementNode> {
        self.node
    }

    /// Attempts to downcast to a concrete type.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.node.as_any().downcast_ref::<T>()
    }

    /// Attempts to downcast to a mutable concrete type.
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.node.as_any_mut().downcast_mut::<T>()
    }

    /// Attempts to downcast, consuming self.
    pub fn downcast<T: Any>(self) -> Result<Box<T>, Self> {
        if self.node.as_any().is::<T>() {
            // SAFETY: We just checked the type
            let raw = Box::into_raw(self.node);
            Ok(unsafe { Box::from_raw(raw as *mut T) })
        } else {
            Err(self)
        }
    }
}

// Delegate RenderElementNode methods to inner
impl std::ops::Deref for ElementNodeStorage {
    type Target = dyn RenderElementNode;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &*self.node
    }
}

impl std::ops::DerefMut for ElementNodeStorage {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.node
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal test implementation
    #[derive(Debug)]
    struct TestElementNode {
        id: Option<ElementId>,
        protocol: ProtocolId,
    }

    impl RenderElementNode for TestElementNode {
        fn id(&self) -> Option<ElementId> {
            self.id
        }
        fn parent(&self) -> Option<ElementId> {
            None
        }
        fn children(&self) -> &[ElementId] {
            &[]
        }
        fn depth(&self) -> usize {
            0
        }
        fn protocol_id(&self) -> ProtocolId {
            self.protocol
        }
        fn arity(&self) -> RuntimeArity {
            RuntimeArity::Exact(0)
        }
        fn render_id(&self) -> Option<RenderId> {
            None
        }
        fn set_render_id(&mut self, _: Option<RenderId>) {}
        fn offset(&self) -> Offset {
            Offset::ZERO
        }
        fn set_offset(&mut self, _: Offset) {}
        fn flags(&self) -> &AtomicRenderFlags {
            static FLAGS: AtomicRenderFlags = AtomicRenderFlags::new_clean();
            &FLAGS
        }
        fn lifecycle(&self) -> RenderLifecycle {
            RenderLifecycle::Detached
        }
        fn parent_data(&self) -> Option<&dyn ParentData> {
            None
        }
        fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
            None
        }
        fn set_parent_data(&mut self, _: Box<dyn ParentData>) {}
        fn as_box_state(&self) -> Option<&BoxRenderState> {
            None
        }
        fn as_box_state_mut(&mut self) -> Option<&mut BoxRenderState> {
            None
        }
        fn as_sliver_state(&self) -> Option<&SliverRenderState> {
            None
        }
        fn as_sliver_state_mut(&mut self) -> Option<&mut SliverRenderState> {
            None
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
        fn debug_name(&self) -> &'static str {
            "TestElementNode"
        }
    }

    #[test]
    fn test_storage_creation() {
        let node = TestElementNode {
            id: Some(ElementId::new(1)),
            protocol: ProtocolId::Box,
        };

        let storage = ElementNodeStorage::new(node);

        assert_eq!(storage.id(), Some(ElementId::new(1)));
        assert_eq!(storage.protocol_id(), ProtocolId::Box);
        assert!(storage.is_box());
    }

    #[test]
    fn test_downcast() {
        let node = TestElementNode {
            id: Some(ElementId::new(42)),
            protocol: ProtocolId::Sliver,
        };

        let storage = ElementNodeStorage::new(node);

        let typed = storage.downcast_ref::<TestElementNode>();
        assert!(typed.is_some());
        assert_eq!(typed.unwrap().id, Some(ElementId::new(42)));
    }
}
