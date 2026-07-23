//! `ChildManager` — object-safe service hook for lazy-sliver element backends.
//!
//! # What this is
//!
//! [`ChildManager`] is the single authority that bridges a [`RenderSliverList`]
//! (the render half of a lazy list) with its element-tree child source (the
//! adaptor half — `SliverListAdaptorElement` in `sliver_adaptor.rs`).
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
//! # FR-036 / Port-check #9
//!
//! `dyn ChildManager` is a sanctioned `dyn`-boundary — added to the
//! `fr036_allowed` allowlist in `scripts/port-check.sh`. The erasure is
//! required because the registry maps `RenderId → Arc<Mutex<dyn ChildManager>>`
//! without knowing the concrete manager type at registry time (the registry lives
//! on `BuildOwner`; the concrete type lives on the adaptor element). This is the
//! same FR-029 #6 rationale as `SliverLayoutCtxErased`.
//!
//! [`RenderSliverList`]: flui_objects::RenderSliverList

use std::{collections::HashMap, sync::Arc};

use flui_foundation::RenderId;
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::{Mutex, RwLock};

use crate::{ElementOwner, tree::ElementTree};

/// Registry mapping a sliver's [`RenderId`] to its live [`ChildManager`].
///
/// Type alias to satisfy `clippy::type_complexity`. Carried by
/// [`BuildOwner`](crate::BuildOwner) as an owned `Arc` and borrowed by
/// [`ElementOwner`] as `&'a ChildManagerRegistry`.
///
/// The outer `Arc<Mutex<…>>` lets `service_child_requests` clone individual
/// manager `Arc`s out of the registry before calling `service` (releasing
/// the registry lock before the potentially long service call, and avoiding
/// re-entrancy deadlocks when `service` triggers `on_mount`/`on_unmount`).
pub(crate) type ChildManagerRegistry = Arc<Mutex<HashMap<RenderId, Arc<Mutex<dyn ChildManager>>>>>;

/// Object-safe hook called by [`BuildOwner::service_child_requests`] after each
/// layout pass to build missing lazy children and evict off-band ones.
///
/// # Implementors
///
/// `SliverListAdaptorBehavior` (in `sliver_adaptor.rs`) is the only
/// production implementor. The trait exists so the `BuildOwner` registry can
/// hold heterogeneous manager types — one per live lazy sliver — without generic
/// parameters on the owner itself.
///
/// # Object safety
///
/// The trait is object-safe by design: all parameters are concrete types or
/// references; no associated types or generic methods. The `+ Send` bound allows
/// the registry arc to cross thread boundaries (required by `Arc<Mutex<…>>`).
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
        pipeline: &Arc<RwLock<PipelineOwner>>,
    ) -> bool;
}
