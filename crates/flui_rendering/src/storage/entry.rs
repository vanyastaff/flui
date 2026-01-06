//! RenderEntry - Protocol-specific render node storage.
//!
//! This module provides `RenderEntry<P>`, which stores a render object along with
//! its protocol-specific state and tree links. This is the internal storage unit
//! that gets wrapped by `RenderNode` enum for heterogeneous tree storage.

use std::fmt::Debug;

use flui_foundation::RenderId;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::protocol::{Protocol, ProtocolConstraints, ProtocolGeometry, RenderObject};

use super::links::NodeLinks;
use super::state::RenderState;

/// Protocol-specific render entry.
///
/// This is the internal storage unit for a render object in the tree.
/// Each entry contains:
/// - The render object itself (behind RwLock for interior mutability)
/// - Protocol-specific state (geometry, constraints, flags)
/// - Tree structure links (parent, children, depth)
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
///
/// # Interior Mutability
///
/// The render object is wrapped in `RwLock` to enable:
/// - Parent calling `child.layout()` during its own layout
/// - Thread-safe access from multiple threads
/// - Lock-free state access via `RenderState`
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::storage::{RenderEntry, NodeLinks};
/// use flui_rendering::protocol::BoxProtocol;
///
/// let entry = RenderEntry::<BoxProtocol>::new(Box::new(MyRenderBox::new()));
/// assert!(entry.state().needs_layout());
/// ```
pub struct RenderEntry<P: Protocol> {
    /// The render object (RwLock for interior mutability during layout).
    render_object: RwLock<Box<dyn RenderObject<P>>>,

    /// Protocol-specific state (geometry, constraints, flags).
    state: RenderState<P>,

    /// Tree structure links (parent, children, depth).
    links: NodeLinks,
}

impl<P: Protocol> Debug for RenderEntry<P>
where
    ProtocolGeometry<P>: Debug,
    ProtocolConstraints<P>: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderEntry")
            .field("state", &self.state)
            .field("links", &self.links)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Creates a new render entry with the given render object.
    ///
    /// The entry starts with:
    /// - Default state (needs_layout = true, needs_paint = true)
    /// - No parent (root node)
    /// - No children
    /// - Depth 0
    pub fn new(render_object: Box<dyn RenderObject<P>>) -> Self {
        Self {
            render_object: RwLock::new(render_object),
            state: RenderState::new(),
            links: NodeLinks::new(),
        }
    }

    /// Creates a new render entry with a parent.
    pub fn with_parent(
        render_object: Box<dyn RenderObject<P>>,
        parent: RenderId,
        depth: u16,
    ) -> Self {
        Self {
            render_object: RwLock::new(render_object),
            state: RenderState::new(),
            links: NodeLinks::with_parent(parent, depth),
        }
    }
}

// ============================================================================
// RENDER OBJECT ACCESS
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Returns a read lock on the render object.
    ///
    /// # Blocking
    ///
    /// This will block if another thread holds a write lock.
    #[inline]
    pub fn render_object(&self) -> RwLockReadGuard<'_, Box<dyn RenderObject<P>>> {
        self.render_object.read()
    }

    /// Returns a write lock on the render object.
    ///
    /// # Blocking
    ///
    /// This will block if another thread holds any lock.
    #[inline]
    pub fn render_object_mut(&self) -> RwLockWriteGuard<'_, Box<dyn RenderObject<P>>> {
        self.render_object.write()
    }

    /// Try to acquire a read lock on the render object.
    ///
    /// Returns `None` if a write lock is held.
    #[inline]
    pub fn try_render_object(&self) -> Option<RwLockReadGuard<'_, Box<dyn RenderObject<P>>>> {
        self.render_object.try_read()
    }

    /// Try to acquire a write lock on the render object.
    ///
    /// Returns `None` if any lock is held.
    #[inline]
    pub fn try_render_object_mut(&self) -> Option<RwLockWriteGuard<'_, Box<dyn RenderObject<P>>>> {
        self.render_object.try_write()
    }
}

// ============================================================================
// STATE ACCESS
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Returns a reference to the render state.
    #[inline]
    pub fn state(&self) -> &RenderState<P> {
        &self.state
    }

    /// Returns a mutable reference to the render state.
    #[inline]
    pub fn state_mut(&mut self) -> &mut RenderState<P> {
        &mut self.state
    }

    // Convenience methods for common state operations

    /// Returns true if layout is needed.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.state.needs_layout()
    }

    /// Returns true if paint is needed.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.state.needs_paint()
    }

    /// Marks as needing layout.
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.state.mark_needs_layout();
    }

    /// Marks as needing paint.
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.state.mark_needs_paint();
    }
}

// ============================================================================
// LINKS ACCESS
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Returns a reference to the tree links.
    #[inline]
    pub fn links(&self) -> &NodeLinks {
        &self.links
    }

    /// Returns a mutable reference to the tree links.
    #[inline]
    pub fn links_mut(&mut self) -> &mut NodeLinks {
        &mut self.links
    }

    // Convenience methods for common link operations

    /// Returns the parent ID.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.links.parent()
    }

    /// Returns the children IDs.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        self.links.children()
    }

    /// Returns the depth in the tree.
    #[inline]
    pub fn depth(&self) -> u16 {
        self.links.depth()
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.links.child_count()
    }
}

// ============================================================================
// LAYOUT
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Performs layout on this entry.
    ///
    /// This acquires a write lock on the render object, calls `perform_layout`,
    /// stores the resulting geometry in state, and clears the needs_layout flag.
    ///
    /// Returns the computed geometry.
    pub fn layout(&self, constraints: ProtocolConstraints<P>) -> ProtocolGeometry<P>
    where
        ProtocolGeometry<P>: Clone,
        ProtocolConstraints<P>: Clone,
    {
        // Perform layout
        let geometry = {
            let mut obj = self.render_object.write();
            obj.perform_layout_raw(constraints.clone())
        };

        // Update state
        self.state.set_geometry(geometry.clone());
        self.state.set_constraints(constraints);
        self.state.clear_needs_layout();

        geometry
    }
}

// ============================================================================
// COMPATIBILITY METHODS (for gradual migration from old RenderObject API)
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Clears the needs_paint flag.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.state.clear_needs_paint();
    }

    /// Clears the needs_layout flag.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.state.clear_needs_layout();
    }
}
