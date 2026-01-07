//! RenderNode - Type-erased render node for heterogeneous tree storage.
//!
//! This module provides `RenderNode`, an enum that wraps protocol-specific
//! `RenderEntry<P>` variants for storage in `RenderTree`.

use flui_foundation::RenderId;

use crate::protocol::{BoxProtocol, Protocol, RenderObject, SliverProtocol};

use super::entry::RenderEntry;
use super::links::NodeLinks;

/// Render node enum for heterogeneous tree storage.
///
/// This enum wraps protocol-specific `RenderEntry<P>` variants, allowing
/// a single `RenderTree` to store both Box and Sliver nodes.
///
/// # Protocols
///
/// - `Box`: 2D cartesian layout (most widgets)
/// - `Sliver`: Scrollable content layout (lists, grids)
///
/// # Usage
///
/// Most operations work through enum matching or convenience methods:
///
/// ```rust,ignore
/// let node: &RenderNode = tree.get(id)?;
///
/// // Common operations work on any variant
/// let parent = node.parent();
/// let needs_layout = node.needs_layout();
///
/// // Protocol-specific access
/// if let Some(box_entry) = node.as_box() {
///     let size = box_entry.state().geometry();
/// }
/// ```
#[derive(Debug)]
pub enum RenderNode {
    /// Box protocol node (2D cartesian layout).
    Box(RenderEntry<BoxProtocol>),

    /// Sliver protocol node (scrollable layout).
    Sliver(RenderEntry<SliverProtocol>),
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl RenderNode {
    /// Creates a new Box protocol node.
    pub fn new_box(render_object: Box<dyn RenderObject<BoxProtocol>>) -> Self {
        Self::Box(RenderEntry::new(render_object))
    }

    /// Creates a new Box protocol node with a parent.
    pub fn new_box_with_parent(
        render_object: Box<dyn RenderObject<BoxProtocol>>,
        parent: RenderId,
        depth: u16,
    ) -> Self {
        Self::Box(RenderEntry::with_parent(render_object, parent, depth))
    }

    /// Creates a new Sliver protocol node.
    pub fn new_sliver(render_object: Box<dyn RenderObject<SliverProtocol>>) -> Self {
        Self::Sliver(RenderEntry::new(render_object))
    }

    /// Creates a new Sliver protocol node with a parent.
    pub fn new_sliver_with_parent(
        render_object: Box<dyn RenderObject<SliverProtocol>>,
        parent: RenderId,
        depth: u16,
    ) -> Self {
        Self::Sliver(RenderEntry::with_parent(render_object, parent, depth))
    }
}

// ============================================================================
// FROM CONVERSIONS (Idiomatic Rust pattern)
// ============================================================================

impl From<Box<dyn RenderObject<BoxProtocol>>> for RenderNode {
    fn from(render_object: Box<dyn RenderObject<BoxProtocol>>) -> Self {
        Self::new_box(render_object)
    }
}

impl From<Box<dyn RenderObject<SliverProtocol>>> for RenderNode {
    fn from(render_object: Box<dyn RenderObject<SliverProtocol>>) -> Self {
        Self::new_sliver(render_object)
    }
}

// ============================================================================
// PROTOCOL CHECK
// ============================================================================

impl RenderNode {
    /// Returns true if this is a Box protocol node.
    #[inline]
    pub fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Returns true if this is a Sliver protocol node.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }

    /// Returns the protocol name.
    pub fn protocol_name(&self) -> &'static str {
        match self {
            Self::Box(_) => "Box",
            Self::Sliver(_) => "Sliver",
        }
    }
}

// ============================================================================
// TYPED ACCESS
// ============================================================================

impl RenderNode {
    /// Returns a reference to the Box entry, if this is a Box node.
    #[inline]
    pub fn as_box(&self) -> Option<&RenderEntry<BoxProtocol>> {
        match self {
            Self::Box(entry) => Some(entry),
            _ => None,
        }
    }

    /// Returns a mutable reference to the Box entry, if this is a Box node.
    #[inline]
    pub fn as_box_mut(&mut self) -> Option<&mut RenderEntry<BoxProtocol>> {
        match self {
            Self::Box(entry) => Some(entry),
            _ => None,
        }
    }

    /// Returns a reference to the Box entry, panics if this is not a Box node.
    ///
    /// Use this when you know the node is Box protocol (e.g., in PipelineOwner
    /// which only works with Box nodes currently).
    #[inline]
    pub fn as_box_unchecked(&self) -> &RenderEntry<BoxProtocol> {
        self.as_box().expect("Expected Box protocol node")
    }

    /// Returns a mutable reference to the Box entry, panics if this is not a Box node.
    #[inline]
    pub fn as_box_unchecked_mut(&mut self) -> &mut RenderEntry<BoxProtocol> {
        self.as_box_mut().expect("Expected Box protocol node")
    }

    /// Returns a reference to the Sliver entry, if this is a Sliver node.
    #[inline]
    pub fn as_sliver(&self) -> Option<&RenderEntry<SliverProtocol>> {
        match self {
            Self::Sliver(entry) => Some(entry),
            _ => None,
        }
    }

    /// Returns a mutable reference to the Sliver entry, if this is a Sliver node.
    #[inline]
    pub fn as_sliver_mut(&mut self) -> Option<&mut RenderEntry<SliverProtocol>> {
        match self {
            Self::Sliver(entry) => Some(entry),
            _ => None,
        }
    }
}

// ============================================================================
// LINKS ACCESS (Common across all protocols)
// ============================================================================

impl RenderNode {
    /// Returns a reference to the tree links.
    #[inline]
    pub fn links(&self) -> &NodeLinks {
        match self {
            Self::Box(entry) => entry.links(),
            Self::Sliver(entry) => entry.links(),
        }
    }

    /// Returns a mutable reference to the tree links.
    #[inline]
    pub fn links_mut(&mut self) -> &mut NodeLinks {
        match self {
            Self::Box(entry) => entry.links_mut(),
            Self::Sliver(entry) => entry.links_mut(),
        }
    }

    // Convenience methods

    /// Returns the parent ID.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.links().parent()
    }

    /// Sets the parent ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<RenderId>) {
        self.links_mut().set_parent(parent);
    }

    /// Returns the children IDs.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        self.links().children()
    }

    /// Returns the depth in the tree.
    #[inline]
    pub fn depth(&self) -> u16 {
        self.links().depth()
    }

    /// Sets the depth.
    #[inline]
    pub fn set_depth(&mut self, depth: u16) {
        self.links_mut().set_depth(depth);
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.links().child_count()
    }

    /// Adds a child.
    #[inline]
    pub fn add_child(&mut self, child: RenderId) {
        self.links_mut().add_child(child);
    }

    /// Removes a child.
    #[inline]
    pub fn remove_child(&mut self, child: RenderId) -> bool {
        self.links_mut().remove_child(child)
    }

    /// Returns true if this is a root node.
    #[inline]
    pub fn is_root(&self) -> bool {
        self.links().is_root()
    }

    /// Returns true if this is a leaf node.
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.links().is_leaf()
    }
}

// ============================================================================
// STATE ACCESS (Common across all protocols)
// ============================================================================

impl RenderNode {
    /// Returns true if layout is needed.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        match self {
            Self::Box(entry) => entry.needs_layout(),
            Self::Sliver(entry) => entry.needs_layout(),
        }
    }

    /// Returns true if paint is needed.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        match self {
            Self::Box(entry) => entry.needs_paint(),
            Self::Sliver(entry) => entry.needs_paint(),
        }
    }

    // NOTE: mark_needs_layout() and mark_needs_paint() removed from RenderNode
    // These methods require element_id and tree access for dirty propagation.
    // They should be called through RenderTree or PipelineOwner instead.
    // See similar note in entry.rs for details.

    /// Returns true if this is a repaint boundary.
    pub fn is_repaint_boundary(&self) -> bool {
        match self {
            Self::Box(entry) => entry.render_object().is_repaint_boundary(),
            Self::Sliver(entry) => entry.render_object().is_repaint_boundary(),
        }
    }

    /// Returns true if this is a relayout boundary.
    pub fn is_relayout_boundary(&self) -> bool {
        match self {
            Self::Box(entry) => entry.render_object().is_relayout_boundary(),
            Self::Sliver(entry) => entry.render_object().is_relayout_boundary(),
        }
    }

    /// Returns the paint bounds for this render object.
    pub fn paint_bounds(&self) -> flui_types::Rect {
        match self {
            Self::Box(entry) => entry.render_object().paint_bounds(),
            Self::Sliver(entry) => entry.render_object().paint_bounds(),
        }
    }

    /// Returns the size for Box protocol nodes (None for Sliver nodes).
    pub fn size(&self) -> Option<flui_types::Size> {
        match self {
            Self::Box(entry) => entry.state().geometry(),
            Self::Sliver(_) => None,
        }
    }

    /// Returns the geometry for this node (Size for Box, SliverGeometry for Sliver).
    pub fn geometry_box(&self) -> Option<flui_types::Size> {
        self.as_box().and_then(|entry| entry.state().geometry())
    }

    /// Returns the sliver geometry for Sliver protocol nodes (None for Box nodes).
    pub fn geometry_sliver(&self) -> Option<crate::constraints::SliverGeometry> {
        self.as_sliver().and_then(|entry| entry.state().geometry())
    }

    /// Returns a read lock on the Box render object.
    ///
    /// Panics if this is not a Box node.
    pub fn box_render_object(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, Box<dyn RenderObject<BoxProtocol>>> {
        self.as_box_unchecked().render_object()
    }

    /// Returns a write lock on the Box render object.
    ///
    /// Panics if this is not a Box node.
    pub fn box_render_object_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, Box<dyn RenderObject<BoxProtocol>>> {
        self.as_box_unchecked().render_object_mut()
    }

    /// Returns a read lock on the Sliver render object.
    ///
    /// Panics if this is not a Sliver node.
    pub fn sliver_render_object(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, Box<dyn RenderObject<SliverProtocol>>> {
        self.as_sliver()
            .expect("Expected Sliver protocol node")
            .render_object()
    }

    /// Returns a write lock on the Sliver render object.
    ///
    /// Panics if this is not a Sliver node.
    pub fn sliver_render_object_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, Box<dyn RenderObject<SliverProtocol>>> {
        self.as_sliver()
            .expect("Expected Sliver protocol node")
            .render_object_mut()
    }

    /// Generic method to get render object for a specific protocol.
    ///
    /// Returns Some if the node matches the protocol, None otherwise.
    /// This is the idiomatic generic way to access render objects.
    pub fn render_object_for_protocol<P: Protocol>(
        &self,
    ) -> Option<parking_lot::RwLockReadGuard<'_, Box<dyn RenderObject<P>>>> {
        use std::any::TypeId;

        // Use TypeId to dispatch at runtime to the correct protocol
        if TypeId::of::<P>() == TypeId::of::<BoxProtocol>() {
            if self.is_box() {
                // SAFETY: We've verified P is BoxProtocol
                let guard = self.as_box_unchecked().render_object();
                // This transmute is safe because we know P == BoxProtocol
                Some(unsafe { std::mem::transmute(guard) })
            } else {
                None
            }
        } else if TypeId::of::<P>() == TypeId::of::<SliverProtocol>() {
            if self.is_sliver() {
                // SAFETY: We've verified P is SliverProtocol
                let guard = self.as_sliver().unwrap().render_object();
                // This transmute is safe because we know P == SliverProtocol
                Some(unsafe { std::mem::transmute(guard) })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Generic method to get mutable render object for a specific protocol.
    ///
    /// Returns Some if the node matches the protocol, None otherwise.
    /// This is the idiomatic generic way to access render objects mutably.
    pub fn render_object_mut_for_protocol<P: Protocol>(
        &self,
    ) -> Option<parking_lot::RwLockWriteGuard<'_, Box<dyn RenderObject<P>>>> {
        use std::any::TypeId;

        // Use TypeId to dispatch at runtime to the correct protocol
        if TypeId::of::<P>() == TypeId::of::<BoxProtocol>() {
            if self.is_box() {
                // SAFETY: We've verified P is BoxProtocol
                let guard = self.as_box_unchecked().render_object_mut();
                // This transmute is safe because we know P == BoxProtocol
                Some(unsafe { std::mem::transmute(guard) })
            } else {
                None
            }
        } else if TypeId::of::<P>() == TypeId::of::<SliverProtocol>() {
            if self.is_sliver() {
                // SAFETY: We've verified P is SliverProtocol
                let guard = self.as_sliver().unwrap().render_object_mut();
                // This transmute is safe because we know P == SliverProtocol
                Some(unsafe { std::mem::transmute(guard) })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Clears the needs_paint flag.
    #[inline]
    pub fn clear_needs_paint(&self) {
        match self {
            Self::Box(entry) => entry.clear_needs_paint(),
            Self::Sliver(entry) => entry.clear_needs_paint(),
        }
    }

    /// Clears the needs_layout flag.
    #[inline]
    pub fn clear_needs_layout(&self) {
        match self {
            Self::Box(entry) => entry.clear_needs_layout(),
            Self::Sliver(entry) => entry.clear_needs_layout(),
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: Add tests once we have concrete render objects
}
