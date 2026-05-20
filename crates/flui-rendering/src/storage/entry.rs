//! `RenderEntry` -- protocol-specific render-node storage.
//!
//! This module provides `RenderEntry<P>`, which stores a render object along
//! with its protocol-specific state and tree links. This is the internal
//! storage unit that gets wrapped by the `RenderNode` enum for heterogeneous
//! tree storage.
//!
//! # U2 exemplar refactor (2026-05-20)
//!
//! The render object is owned by plain value (`Box<dyn RenderObject<P>>`),
//! not wrapped in a lock. Mutable access goes through `&mut self`, which the
//! pipeline obtains via `&mut RenderTree` at phase boundaries (Build / Layout
//! / Paint). The previous shape `RwLock<Box<dyn RenderObject<P>>>` is the
//! canonical refusal-trigger violation documented in `docs/PORT.md` (Trigger 1
//! and Trigger 2). Single-writer-per-frame discipline is enforced by Rust's
//! borrow checker on `&mut RenderTree`, matching Flutter's single-threaded
//! pipeline invariant (`_debugDoingThisLayout` / `_debugDoingThisPaint`
//! debug asserts in `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart`).
//!
//! Pipeline bookkeeping bits that previously required a write lock on the
//! trait object (specifically `set_was_repaint_boundary`) now live on
//! `RenderState<P>::flags` (`crates/flui-rendering/src/storage/flags.rs`) and
//! are flipped through atomic stores without touching the trait surface.

use std::fmt::Debug;

use flui_foundation::RenderId;

use super::{links::NodeLinks, state::RenderState};
use crate::protocol::{Protocol, ProtocolConstraints, ProtocolGeometry, RenderObject};

/// Protocol-specific render entry.
///
/// This is the internal storage unit for a render object in the tree.
/// Each entry contains:
/// - The render object itself (owned by value -- mutation discipline is
///   enforced by `&mut self` access from the pipeline holding `&mut
///   RenderTree`)
/// - Protocol-specific state (geometry, constraints, flags)
/// - Tree structure links (parent, children, depth)
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
///
/// # Mutation discipline
///
/// The render object is owned plainly; there is no interior mutability on
/// the storage type. Pipeline phases obtain mutable access by holding `&mut
/// RenderTree` (via `PipelineOwner::render_tree_mut`) for the duration of
/// the phase. Re-entrant access from a parent to a child during layout uses
/// the disjoint-borrow primitive `RenderTree::get_two_mut`. State bits that
/// only need to be flipped (e.g. `WAS_REPAINT_BOUNDARY` written by the paint
/// phase) live on `RenderState<P>::flags` and are mutated through atomic
/// stores -- no lock acquisition required.
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
    /// The render object. Owned by value; mutation via `&mut self`.
    render_object: Box<dyn RenderObject<P>>,

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
            render_object,
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
            render_object,
            state: RenderState::new(),
            links: NodeLinks::with_parent(parent, depth),
        }
    }
}

// ============================================================================
// RENDER OBJECT ACCESS
// ============================================================================

impl<P: Protocol> RenderEntry<P> {
    /// Returns an immutable reference to the render object.
    ///
    /// Always succeeds. No locking; the borrow checker enforces that no
    /// concurrent `&mut` access exists.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject<P> {
        &*self.render_object
    }

    /// Returns a mutable reference to the render object.
    ///
    /// Requires `&mut self`. Pipeline phases obtain this via `&mut RenderTree`
    /// (`PipelineOwner::render_tree_mut`) for the duration of the phase. For
    /// parent-to-child re-entrant mutation during layout, use
    /// `RenderTree::get_two_mut`.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject<P> {
        &mut *self.render_object
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

    // NOTE: mark_needs_layout() and mark_needs_paint() removed from RenderEntry.
    // These methods require element_id and tree access for dirty propagation.
    // They should be called through RenderTree or PipelineOwner instead.
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
    /// Requires `&mut self`: the caller (typically `PipelineOwner` holding
    /// `&mut RenderTree`) has exclusive access to this entry for the duration
    /// of the layout call. Calls `perform_layout_raw` on the owned render
    /// object, stores the resulting geometry in state, and clears the
    /// `NEEDS_LAYOUT` flag.
    ///
    /// The `perform_layout_raw` call is wrapped in
    /// [`std::panic::catch_unwind`]; a panic surfaces as
    /// [`crate::error::RenderError::Poisoned`] (Mythos Step 12). On the
    /// panic path the state's geometry is **not** updated -- the previous
    /// geometry (or `None` if this is the first layout) remains valid.
    /// The `NEEDS_LAYOUT` flag is also left set so the pipeline can retry
    /// next frame after the offending node has been removed or fixed.
    ///
    /// Returns the computed geometry on success.
    pub fn layout(
        &mut self,
        constraints: ProtocolConstraints<P>,
    ) -> crate::error::RenderResult<ProtocolGeometry<P>>
    where
        ProtocolGeometry<P>: Clone,
        ProtocolConstraints<P>: Clone,
    {
        // Capture the debug name BEFORE the &mut reborrow -- a shared
        // borrow against `&*self.render_object` cannot coexist with the
        // &mut needed inside the unwind closure, so we read the name
        // upfront and let it outlive the closure.
        let debug_name = self.render_object.debug_name();

        // SAFETY of AssertUnwindSafe: the render object's internal state
        // is opaque to us. If it panics, we treat the state as torn and
        // surface RenderError::Poisoned -- callers are required to drop
        // or replace the node before reusing it. The pipeline-side state
        // (geometry / constraints / flags) on `self.state` is not touched
        // before the panic site, so the render tree stays consistent.
        let render_object = &mut *self.render_object;
        let constraints_for_call = constraints.clone();
        let geometry = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_object.perform_layout_raw(constraints_for_call)
        }))
        .map_err(|_| crate::error::RenderError::poisoned(debug_name, "layout"))?;

        // Update state -- only on the success path. On panic, state remains
        // untouched and NEEDS_LAYOUT stays set so a retry is possible.
        self.state.set_geometry(geometry.clone());
        self.state.set_constraints(constraints);
        self.state.clear_needs_layout();

        Ok(geometry)
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
