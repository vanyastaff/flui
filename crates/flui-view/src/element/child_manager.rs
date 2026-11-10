//! `ChildManager` — object-safe service hook for lazy-sliver element backends.
//!
//! # What this is
//!
//! [`ChildManager`] is the single authority that bridges a [`RenderSliverList`]
//! (the render half of a lazy list) with its element-tree child source (the
//! adaptor half — `SliverAdaptorElement` in `sliver_adaptor.rs`).
//!
//! After each layout pass, `PipelineOwner` holds two buffers:
//! - `pending_child_requests`: `(sliver_render_id, logical_index)` pairs for
//!   children whose render-slot was empty during layout.
//! - `pending_retain_bands`: `(sliver_render_id, first, last)` triples declaring
//!   which logical-index band `[first, last)` the sliver's cache window covers.
//!
//! `BuildOwner::service_child_requests` drains those buffers, groups entries by
//! sliver `RenderId`, looks up the registered `ChildManager`, and calls
//! [`ChildManager::service`]. The manager builds missing children via
//! `SparseChildren::ensure` and evicts off-band ones via
//! `SparseChildren::retain_band`.
//!
//! The service pass runs **inside the frame's layout↔build fixpoint**
//! (`BuildOwner::run_frame_with_layout_builders`, beside
//! `service_layout_builders`): layout requests the band, the manager builds
//! it, the next pass lays the fresh children out, and the frame paints them —
//! same frame, no reentrant build. A manager reports whether it did work so
//! the loop knows to run another pass.
//!
//! # Why `dyn ChildManager`
//!
//! `dyn ChildManager` is a deliberate `dyn` boundary. The erasure is
//! required because the registry maps `RenderId → Rc<RefCell<dyn ChildManager>>`
//! without knowing the concrete manager type at registry time (the registry lives
//! on `BuildOwner`; the concrete type lives on the adaptor element). This is the
//! same FR-029 #6 rationale as `SliverLayoutCtxErased`.
//!
//! [`RenderSliverList`]: flui_objects::RenderSliverList

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use flui_foundation::RenderId;
use flui_rendering::pipeline::PipelineCell;

use crate::{ElementOwner, tree::ElementTree};

/// Registry mapping a sliver's [`RenderId`] to its live [`ChildManager`].
///
/// Type alias to satisfy `clippy::type_complexity`. Owned by
/// [`BuildOwner`](crate::BuildOwner) and borrowed by [`ElementOwner`] as
/// `&'a ChildManagerRegistry`.
///
/// Each manager is its own `Rc<RefCell<…>>` so `service_child_requests` can
/// clone the managers it services out of the registry and release the
/// registry's borrow before calling `service`: the service call may register
/// or unregister managers (`on_mount`/`on_unmount`) without a re-entrant
/// borrow.
pub(crate) type ChildManagerRegistry = RefCell<HashMap<RenderId, Rc<RefCell<dyn ChildManager>>>>;

/// Object-safe hook called by [`BuildOwner::service_child_requests`] after each
/// layout pass to build missing lazy children and evict off-band ones.
///
/// # Implementors
///
/// `SliverAdaptorBehavior` (in `sliver_adaptor.rs`) is the only
/// production implementor. The trait exists so the `BuildOwner` registry can
/// hold heterogeneous manager types — one per live lazy sliver — without generic
/// parameters on the owner itself.
///
/// # Object safety
///
/// The trait is object-safe by design: all parameters are concrete types or
/// references; no associated types or generic methods. The registry is
/// owner-local (`Rc<RefCell<…>>`), so the manager needs no `Send` bound.
///
/// [`BuildOwner::service_child_requests`]: crate::BuildOwner::service_child_requests
pub(crate) trait ChildManager {
    /// Build any requested lazy children and evict those outside the retain band.
    ///
    /// # Parameters
    ///
    /// - `requested_indices`: logical indices the layout pass asked to build but
    ///   found absent. The manager calls `SparseChildren::ensure` for each.
    /// - `retain_first`, `retain_last`: the retained cache-window band
    ///   `[retain_first, retain_last)` in logical-index space. The manager calls
    ///   `SparseChildren::retain_band(retain_first, retain_last)` to evict
    ///   children that have scrolled out of the cache.
    /// - `tree`: the live element tree — threaded in for `ensure` / `evict`
    ///   operations.
    /// - `owner`: the build-phase split-borrow handle — used to schedule newly
    ///   built children via `schedule_build_for` and to push evicted keyed
    ///   children to the inactive queue.
    /// - `pipeline`: the shared render-owner — required to stamp the child's
    ///   `SliverMultiBoxAdaptorParentData` logical index at mount.
    ///
    /// Returns `true` if any children were built or evicted during this call,
    /// `false` if this service pass was a no-op (settled state). Callers
    /// (specifically `BuildOwner::service_child_requests`) gate the
    /// `mark_needs_layout` call on this return value: when `false`, the sliver
    /// is not dirtied and the frame becomes quiescent.
    fn service(
        &mut self,
        requested_indices: &[usize],
        retain_first: usize,
        retain_last: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &PipelineCell,
    ) -> bool;

    /// Drop the bookkeeping for `child`, which a `GlobalKey` retake has just
    /// grafted onto another parent. The child is no longer this sliver's to
    /// evict or refresh; touching it would reach into the new parent's
    /// subtree.
    fn forget_child(&mut self, child: flui_foundation::ElementId);

    /// Whether `child` is the resident at `logical_index`.
    fn holds_resident(&self, logical_index: usize, child: flui_foundation::ElementId) -> bool;

    /// Re-point `logical_index` from `failed` to `replacement`, the error child
    /// mounted in its place after `failed` panicked while it rebuilt.
    fn replace_resident(
        &mut self,
        logical_index: usize,
        failed: flui_foundation::ElementId,
        replacement: flui_foundation::ElementId,
    );

    /// Ownership edges for lifecycle traversal, distinct from dense reconcile slots.
    fn snapshot_children(&self) -> Vec<flui_foundation::ElementId>;

    /// Clear admitted retiring ownership before any descendant lifecycle hook.
    fn retire_children(&mut self);
}
