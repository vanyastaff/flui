//! BuildOwner - Manages the build phase.
//!
//! The BuildOwner is responsible for:
//! - Tracking dirty elements that need rebuilding
//! - Processing rebuilds in depth-first order
//! - Managing GlobalKey registry
//! - Coordinating InheritedElement lookups

#[path = "retirement_slots.rs"]
mod retirement_slots;
use retirement_slots::{OwnerBuildScheduledSlot, OwnerReactive, OwnerTreeObserverSlot};

use std::{
    any::Any,
    cell::Cell,
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::{ElementId, RebuildReasons, RenderId, ViewKey};
use flui_interaction::FocusManager;
use flui_rendering::pipeline::DetachedRenderSubtrees;
use parking_lot::Mutex;

use crate::{
    element::child_manager::{ChildManager, ChildManagerRegistry},
    owner::{
        DuplicateGlobalKey, GlobalKeyRegistry, GlobalKeyReservations, LifecyclePanicHandoff,
        RebuildReason, RecoveredPanic, StagedRecoveredPanic, global_key_reservations,
        global_key_scope,
        global_key_scope::{GlobalKeyScope, OwnerTag},
        inherited_dependencies::InheritedDependencies,
        layout_builder::LayoutBuilderRegistry,
    },
    tree::ElementTree,
    view::View,
};

pub(crate) use super::external_build_inbox::ExternalBuildInbox;

#[cfg(test)]
thread_local! {
    static LAYOUT_SCOPE_CLASSIFICATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Upper bound on RE-ENTRY mid-drain absorbs per frame (issue #1180).
///
/// A "re-entry" is a `Vacant` landing for an id that already completed a
/// build in this `build_scope` call — most often that same element
/// rescheduling itself from inside its own `build`, but identically a child
/// notifying its already-built parent, or one half of an A↔B ping-pong. The
/// FIRST time any id lands in a frame is always free, however many
/// independent elements do it (a page whose N unrelated parents each
/// retarget an implicit animation in one frame performs N first-time
/// absorbs, all legitimate) — see [`BuildOwner::built_this_frame`]. Only an
/// id that keeps getting notified again after each of its own builds spends
/// this budget, so 16 is already a pathological tree, not a realistic one:
/// a downward notification (parent notifies a descendant below it) costs
/// nothing beyond its own first-time absorb.
const MAX_MID_DRAIN_ABSORBS: usize = 16;

/// A cloneable, owned handle that lets a listener callback — an animation tick
/// fired *outside* any frame, with no `&mut BuildOwner` in scope — enqueue an
/// element for the next [`BuildOwner::build_scope`] drain and request a frame.
///
/// It carries a mark-dirty request from an element to the owner's build queue
/// and the scheduler's frame request: an
/// `AnimatedView`'s mark-dirty callback captures one of these at mount (via
/// [`ElementOwner::external_scheduler`](super::ElementOwner::external_scheduler))
/// and calls [`schedule`](Self::schedule) when the listenable changes. The
/// pending ids accumulate in a shared inbox that `build_scope` absorbs onto
/// its dirty heap at the top of every heap pop — not just once at frame
/// start — so a schedule landing mid-drain (a build calling another
/// element's [`RebuildHandle`](super::RebuildHandle) synchronously) joins
/// the SAME drain instead of waiting for the next frame (issue #1180). The
/// listener still never needs to touch the owner.
///
/// The inbox carries the element id and every cause accumulated since it was
/// last absorbed. The dirty-heap ordering key (tree depth) is read from the
/// node at absorb time, not captured here: a GlobalKey move between the
/// schedule and the absorb can change it.
#[derive(Clone)]
pub(crate) struct ExternalBuildScheduler {
    /// Shared inbox drained by `build_scope`; one accumulated cause set per
    /// element.
    inbox: Arc<ExternalBuildInbox>,
    /// Frame-request hook (the binding's `on_build_scheduled`), so a tick
    /// between frames asks the platform for a new frame. `None` in headless
    /// tests, which drive `build_scope` directly.
    request_frame: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ExternalBuildScheduler {
    pub(crate) fn is_active(&self) -> bool {
        !self.inbox.is_closed()
    }
    /// Enqueue `id` for the next `build_scope` drain and request a frame.
    ///
    /// Deduplicating: a repeat tick for an id already queued consumes no extra
    /// inbox slot. With healthy delivery a burst still costs one frame request;
    /// while that delivery is in flight or owed, a concurrent caller may race
    /// or retry it so a failed wake cannot strand the batch. Thread-safe: the
    /// inbox lock is held only for the insert and released before
    /// `request_frame` runs (no lock across the platform wake).
    pub(crate) fn schedule(&self, id: ElementId, reason: RebuildReason) {
        self.schedule_many(std::iter::once(id), reason);
    }

    /// Enqueue a related set of ids as one durable batch, then request one
    /// frame after every id is visible to the drain. A panicking wake can
    /// therefore delay the batch, but cannot expose only a prefix of it.
    pub(crate) fn schedule_many(
        &self,
        ids: impl IntoIterator<Item = ElementId>,
        reason: RebuildReason,
    ) {
        let any_newly_queued = {
            let mut inbox = self.inbox.lock();
            if self.inbox.is_closed() {
                return;
            }
            let mut any_newly_queued = false;
            for id in ids {
                match inbox.entry(id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(RebuildReasons::from_reason(reason));
                        any_newly_queued = true;
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().insert(reason);
                    }
                }
            }
            any_newly_queued
        };
        self.inbox
            .request_frame_if_needed(any_newly_queued, self.request_frame.as_deref());
    }

    /// Build a scheduler from the shared inbox + frame-request handle. Used by
    /// [`ElementOwner::external_scheduler`](super::ElementOwner::external_scheduler).
    pub(crate) fn from_parts(
        inbox: Arc<ExternalBuildInbox>,
        request_frame: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            inbox,
            request_frame,
        }
    }
}

impl std::fmt::Debug for ExternalBuildScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `try_lock`, not `lock`: `parking_lot::Mutex` is non-reentrant, so a
        // `{:?}` while the inbox is already held (e.g. instrumenting the drain)
        // would otherwise deadlock silently.
        f.debug_struct("ExternalBuildScheduler")
            .field("pending", &self.inbox.try_len())
            .field("has_request_frame", &self.request_frame.is_some())
            .finish()
    }
}

/// Entry in the dirty elements heap.
///
/// Sorted by depth (shallowest first) for top-down processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirtyElement {
    id: ElementId,
    depth: usize,
}

/// Which finalize step a lazy-sliver service pass ends with — see
/// [`BuildOwner::service_child_requests_between_passes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceFinalize {
    /// Unmount the evicted children and verify the frame's `GlobalKey`
    /// reservations (the post-`run_frame` call).
    Frame,
    /// Unmount the evicted children only; the ledger keeps accumulating
    /// until the frame's own finalize (a fixpoint pass).
    UnmountOnly,
}

/// What a lazy-sliver service pass does with the child-build requests the
/// last layout pass recorded — see
/// [`BuildOwner::service_child_requests_evict_only`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceRequests {
    /// Build the requested children (and evict by the retain bands).
    Build,
    /// Evict by the retain bands only; the requests stay queued on the
    /// pipeline for the frame's post-`run_frame` service to build.
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildScopeTarget {
    /// Drain every dirty element. Used when no layout-builder scopes exist.
    All,
    /// Drain only elements outside every live layout-builder scope.
    Global,
    /// Drain the builder itself and descendants whose nearest live scope is it.
    LayoutBuilder(ElementId),
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct BuildDrainResult {
    pub(crate) any_rebuilt: bool,
    pub(crate) target_rebuilt: bool,
}

/// Drain-scoped accumulator for ids [`BuildOwner::absorb_mid_drain_inbox`]
/// capped this drain (see that method's doc for why this exists instead of
/// putting each one straight back into the shared inbox). `Drop` — not a
/// flush call placed after `drain_build_scope`'s loop — is what actually
/// gets the accumulated ids back to `external_inbox`, because the loop's own
/// per-element build and reconcile panics are caught, patched up, and
/// RE-RAISED via `std::panic::resume_unwind` from several arms inside that
/// loop: each one unwinds `drain_build_scope`'s stack frame immediately,
/// skipping any code placed after the loop. Without this guard, an id
/// already marked dirty on the tree and removed from `external_inbox` (by
/// `absorb_mid_drain_inbox`'s own `drain()`) but capped by the budget would
/// end up in none of `dirty_elements`, `dirty_reasons`, or `external_inbox`
/// the moment a LATER build in the same drain panics — a silently lost
/// rebuild request. A `Drop` impl runs on every exit path, unwind included,
/// so this is the house pattern for cleanup that must survive a panicking
/// callback (see the reentrancy-depth guard around ticker callbacks for the
/// same shape). Holds a cloned `Arc`, not a borrow of `BuildOwner`, so it can
/// coexist with `&mut self` calls elsewhere in the loop.
struct CappedLeftoverGuard {
    inbox: Arc<ExternalBuildInbox>,
    leftover: HashMap<ElementId, RebuildReasons>,
}

impl CappedLeftoverGuard {
    fn new(inbox: Arc<ExternalBuildInbox>) -> Self {
        Self {
            inbox,
            leftover: HashMap::new(),
        }
    }
}

impl Drop for CappedLeftoverGuard {
    fn drop(&mut self) {
        if self.leftover.is_empty() {
            return;
        }
        let mut inbox = self.inbox.lock();
        for (id, reasons) in self.leftover.drain() {
            match inbox.entry(id) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(reasons);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(reasons);
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct BuildScopeQueues {
    /// Dirty work currently outside every live layout-builder scope.
    root: BinaryHeap<Reverse<DirtyElement>>,
    /// Dirty work keyed by its current nearest live layout-builder scope.
    isolated: HashMap<ElementId, BinaryHeap<Reverse<DirtyElement>>>,
    /// Reused while rerouting after scope removal or tree reparenting.
    scratch: Vec<DirtyElement>,
}

/// Per-cause rebuild counters for one `build_scope`, indexed by the
/// `RebuildReason` discriminant (`#[repr(u8)]`), sized by `RebuildReason::COUNT`
/// so a new variant grows the table with it.
#[derive(Debug, Default)]
struct FrameBuildCounts {
    /// Which causes appeared this frame — the bitset is also the stable
    /// iteration order for the report.
    seen: Option<RebuildReasons>,
    counts: [usize; RebuildReason::COUNT],
    /// Every completed build this frame, re-entries included.
    builds_run: usize,
}

impl FrameBuildCounts {
    fn clear(&mut self) {
        self.seen = None;
        self.counts = [0; RebuildReason::COUNT];
        self.builds_run = 0;
    }

    fn record(&mut self, reason: RebuildReason) {
        let index = reason as u8 as usize;
        debug_assert!(
            index < self.counts.len(),
            "BUG: RebuildReason outgrew the counter table"
        );
        if let Some(slot) = self.counts.get_mut(index) {
            *slot += 1;
        }
        match &mut self.seen {
            Some(seen) => seen.insert(reason),
            None => self.seen = Some(RebuildReasons::from_reason(reason)),
        }
    }

    fn by_reason(&self) -> Vec<(RebuildReason, usize)> {
        self.seen
            .into_iter()
            .flat_map(RebuildReasons::iter)
            .map(|reason| {
                (
                    reason,
                    self.counts.get(reason as u8 as usize).copied().unwrap_or(0),
                )
            })
            .collect()
    }
}

/// Per-frame rebuild telemetry, see [`BuildOwner::last_frame_build_report`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FrameBuildReport {
    /// Distinct elements rebuilt by the last `build_scope`.
    pub elements_built: usize,
    /// Every build run this frame, re-entries included.
    ///
    /// Equal to [`elements_built`](Self::elements_built) unless an element
    /// built more than once in the frame — a self-rescheduling build or an
    /// A↔B ping-pong, which the distinct count cannot show.
    pub builds_run: usize,
    /// Rebuilt-element count per cause, in stable diagnostic-name order.
    pub by_reason: Vec<(RebuildReason, usize)>,
}

impl FrameBuildReport {
    /// How many rebuilt elements carried `reason`.
    #[must_use]
    pub fn count(&self, reason: RebuildReason) -> usize {
        self.by_reason
            .iter()
            .find(|(candidate, _)| *candidate == reason)
            .map_or(0, |(_, count)| *count)
    }
}

impl DirtyElement {
    /// Construct a new dirty-elements heap entry.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self { id, depth }
    }

    /// The element id queued for rebuild.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order the heap (shallowest first).
    ///
    /// Used when an unwinding rebuild is restored to the active queue without
    /// changing the ordering key it had at the start of the attempt.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
}

impl Ord for DirtyElement {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Min-heap by depth (process shallowest first)
        self.depth.cmp(&other.depth)
    }
}

impl PartialOrd for DirtyElement {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Manages the build phase of the element lifecycle.
///
/// BuildOwner tracks which elements need rebuilding and processes them
/// in the correct order (depth-first, shallowest first).
///
/// # Responsibilities
///
/// - Maintain list of dirty elements
/// - Process rebuilds in correct order
/// - Manage GlobalKey registry
/// - Track inactive elements for finalization
///
/// O(1) InheritedElement lookup is NOT here — it lives structurally in each
/// node's [`inherited`](crate::tree::ElementNode) map, built at mount.
pub struct BuildOwner {
    /// Elements that need rebuild, sorted by depth.
    ///
    /// `pub(crate)` so [`ElementOwner`](super::ElementOwner)'s
    /// split-borrow can pin a `&mut` reference to just this field
    /// during the recursive Element traversal — no full `&mut
    /// BuildOwner` needed.
    pub(crate) dirty_elements: BinaryHeap<Reverse<DirtyElement>>,

    /// Accumulated rebuild causes for every id present in `dirty_elements`.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) dirty_reasons: HashMap<ElementId, RebuildReasons>,

    /// GlobalKey registry: key hash -> element ID.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) global_keys: GlobalKeyRegistry,

    /// This frame's `GlobalKey` declarations, verified and cleared by
    /// [`Self::finalize_tree`].
    ///
    /// Boxed: this is per-frame scratch state, empty in every frame of every
    /// tree that uses no `GlobalKey`s, and it is touched once per keyed
    /// child rather than per element. Keeping its four containers behind one
    /// pointer keeps them out of the owner's inline footprint, which is
    /// otherwise the owner's hot working set (dirty heap, dependency maps,
    /// capability handles).
    pub(crate) global_key_reservations: Box<GlobalKeyReservations>,

    /// Duplicate-`GlobalKey` reports produced by the most recent frame
    /// boundaries, waiting to be drained by
    /// [`Self::take_global_key_diagnostics`].
    ///
    /// A duplicate key is caller-controlled input, so it is surfaced as a
    /// typed diagnostic rather than a panic; the frame that produced it
    /// still completes.
    pub(crate) global_key_diagnostics: Vec<DuplicateGlobalKey>,

    /// Elements that have been deactivated and are pending unmount.
    /// These are unmounted in `finalize_tree()`.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) inactive_elements: Vec<InactiveElement>,

    /// Elements that received an inherited-dependency change since their
    /// last build. `build_scope` consults this set right before each
    /// dirty element's `perform_build` and fires
    /// `ElementBase::notify_dependency_change` (which routes through the
    /// behavior to call `ViewState::did_change_dependencies`) when the
    /// id is present, then removes the entry.
    ///
    /// Populated by [`InheritedBehavior::on_view_updated`](crate::element::InheritedBehavior)
    /// when `update_should_notify == true`. Cleared on element unmount
    /// (the dependent leaves the tree before its rebuild ever runs).
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) pending_dependency_changes: std::collections::HashSet<ElementId>,

    /// Sparse dependent -> providers ownership index.
    ///
    /// Provider elements keep the forward notification map. This reverse index
    /// is the lifecycle authority used to remove an element from every
    /// provider on deactivate/unmount without adding a collection to every
    /// [`ElementNode`](crate::tree::ElementNode).
    pub(crate) inherited_dependencies: InheritedDependencies,
    /// ADR-0074: the realm's reactive graph. Constructed with the owner,
    /// re-pointed at the external inbox whenever the frame-request callback
    /// changes (`set_on_build_scheduled`).
    reactive: OwnerReactive,

    /// Keep-alive holds on lazy sliver children — which children band eviction
    /// must skip. Presentation-scoped like every other lifecycle capability,
    /// and cheap to clone into the split-borrow `ElementOwner`.
    pub(crate) keep_alive: super::KeepAliveHolds,

    /// The realm's tree-observer slot (ADR-0040). `None` = observation off
    /// (one branch per emission site). `pub(crate)` for the
    /// [`ElementOwner`](super::ElementOwner) split-borrow.
    pub(crate) tree_observer: OwnerTreeObserverSlot,

    /// Lifecycle-hook panics caught and contained by a per-child
    /// containment seam this frame, waiting to be drained by
    /// [`Self::take_recovered_panics`]. `pub(crate)` for the
    /// [`ElementOwner`](super::ElementOwner) split-borrow.
    pub(crate) recovered_panics: Vec<RecoveredPanic>,

    /// Owner-local state for a bounded lifecycle window's transient staged
    /// diagnostic. The immediate containing catch takes and disarms it before
    /// returning or resuming the unwind. `pub(crate)` for the split-borrow.
    pub(crate) lifecycle_panic_handoff: Cell<LifecyclePanicHandoff>,

    /// Whether we're currently in a build phase.
    #[cfg(debug_assertions)]
    building: bool,

    /// Build scope nesting depth.
    #[cfg(debug_assertions)]
    scope_depth: usize,

    /// Callback to be called when a build is scheduled.
    ///
    /// `pub(crate)` so the [`ElementOwner`](super::ElementOwner)
    /// split-borrow can fire it from `schedule_build_for` without
    /// re-borrowing the owner. Stored as `Arc` (not `Box`) so an
    /// `ExternalBuildScheduler` captured by an animation listener can clone
    /// and fire it as a frame request from outside a frame.
    pub(crate) on_build_scheduled: OwnerBuildScheduledSlot,

    /// Inbox of element ids and causes scheduled through an
    /// `ExternalBuildScheduler` handle — typically an animation/listenable
    /// tick firing *outside* any frame (no `&mut BuildOwner` in scope), but
    /// the identical route a `build` running *inside* a drain takes when it
    /// calls another element's [`RebuildHandle`](super::RebuildHandle)
    /// synchronously. The map deduplicates element ids while retaining
    /// distinct causes. Absorbed onto [`Self::dirty_elements`] (or a
    /// layout-builder scope bucket) at the top of every heap pop in
    /// [`Self::drain_build_scope`] — not only once at frame start — where
    /// each id's tree depth is looked up (issue #1180). Shared (`Arc`) so
    /// the listener callbacks and the owner reference the same queue.
    pub(crate) external_inbox: Arc<ExternalBuildInbox>,

    /// Remaining RE-ENTRY mid-drain absorb budget for the current frame
    /// (issue #1180, [`MAX_MID_DRAIN_ABSORBS`]). Reset at every
    /// [`Self::build_scope`] entry and shared by every drain that frame
    /// runs — including the layout-builder fixpoint's own
    /// `drain_prepared_build_target` calls AND
    /// `Self::service_child_requests_impl`'s lazy-sliver service pass —
    /// because `build_scope` is only the reset plus a call into
    /// [`Self::build_scope_impl`], and every one of those mid-frame
    /// re-entrant callers reaches `build_scope_impl` directly instead of
    /// `build_scope`, so none of them re-runs the reset. See
    /// [`Self::built_this_frame`] for what counts as a re-entry.
    pub(crate) mid_drain_absorbs_left: usize,

    /// Whether the mid-drain absorb budget ran out at some point since the
    /// last time it was clear — gates the exhaustion `tracing::warn!` to once
    /// per streak rather than once per capped id. There is no
    /// frame-complete hook on `BuildOwner`, so this clears retroactively: at
    /// the next `build_scope` entry, if the PREVIOUS frame ended with budget
    /// still left, the streak is over and the warning may fire again.
    pub(crate) mid_drain_cap_streak: bool,

    /// Element ids that have completed a build during the CURRENT
    /// `build_scope` call. Cleared at `build_scope` entry.
    ///
    /// A mid-drain absorb for an id already dirty (on the heap or in a
    /// deferred layout-builder-scope bucket) is always a free merge. A
    /// Vacant absorb for an id NOT in this set is the first time it has come
    /// up this frame and is also free — a page whose N unrelated elements
    /// each get notified once in one frame performs N such absorbs, all
    /// legitimate. A Vacant absorb for an id ALREADY in this set is a
    /// RE-ENTRY — it built, its `dirty_reasons` entry was removed, and it
    /// landed in the inbox again before the next pop, whether from itself
    /// (a self-reschedule), from a child it just built notifying it back,
    /// or from the other half of an A↔B ping-pong — and spends one unit of
    /// [`Self::mid_drain_absorbs_left`].
    pub(crate) built_this_frame: HashSet<ElementId>,
    /// How many elements this frame's drain rebuilt for each cause — the
    /// per-frame telemetry ADR-0074 §8 measures against (`FrameStats` has no
    /// rebuild figure). Reset with `built_this_frame` at every `build_scope`.
    /// Boxed so the owner grows by one pointer, not by a counter table
    /// (`build_owner_tests::test_build_owner_memory_size` budgets this struct).
    frame_builds: Box<FrameBuildCounts>,

    /// Registry of live lazy-sliver [`ChildManager`]s, one per live adaptor
    /// element. Keyed by the sliver's `RenderId`; populated at mount and
    /// cleared at unmount by `SliverAdaptorBehavior` via the
    /// `ElementOwner::register_child_manager` / `unregister_child_manager`
    /// split-borrow methods.
    ///
    /// `Arc<Mutex<…>>` (not a plain `HashMap`) so `ElementOwner` can carry a
    /// `&'a Arc<…>` reference — the same pattern as `external_inbox`. The outer
    /// `Arc` lets `service_child_requests` clone individual manager `Arc`s out
    /// of the registry before calling service (releasing the registry lock
    /// before the potentially long service call).
    pub(crate) child_manager_registry: ChildManagerRegistry,

    /// Registry of live build-during-layout nodes, one per
    /// mounted layout-builder element, keyed by its render object's `RenderId`.
    /// Drained every layout pass by
    /// [`service_layout_builders`](Self::service_layout_builders).
    ///
    /// Empty unless a `LayoutBuilder` is mounted; the seam is inert until one is.
    pub(crate) layout_builder_registry: LayoutBuilderRegistry,

    /// How many fixpoint passes may service lazy-sliver child requests per
    /// frame before the rest is deferred to the next frame — see
    /// `owner/layout_builder.rs`. Only a test lowers it.
    pub(crate) lazy_band_pass_budget: usize,

    /// Lazily allocated root/isolated dirty-work buckets. A nonempty isolated
    /// bucket is the sole pending-scope authority; no parallel scope set can
    /// drift from the queued work.
    pub(crate) build_scope_queues: Option<Box<BuildScopeQueues>>,

    /// The focus tree owned by this element tree.
    ///
    /// Every `BuildOwner` has exactly one manager. Presentation composition
    /// passes its manager through [`Self::with_focus_manager`]; detached owners
    /// created by [`Self::new`] receive a fresh isolated manager.
    focus_manager: Rc<FocusManager>,
    lifecycle_handle: Option<crate::LifecycleHandle>,

    /// The binding's frame-driven async task driver, installed by
    /// whichever binding owns this owner. `None` until then — a tree built with
    /// no binding cannot spawn tasks, and `LifecycleContext::async_driver` reports
    /// that honestly rather than silently spawning into a driver nobody polls.
    ///
    /// This must be the driver the binding's frame step actually polls:
    /// `HeadlessBinding` drives its own binding-local `UpdateScheduler`; production
    /// drives the realm's own owned `UpdateScheduler` (`UiRealm.scheduler`). Reaching
    /// for the wrong one from a widget would make headless tests spawn into a
    /// driver that never runs.
    pub(crate) async_driver: Option<flui_scheduler::AsyncDriver>,

    /// The binding's post-frame capability. `None` when no binding
    /// installed one, which makes `LifecycleContext::post_frame_handle` report the
    /// absence rather than silently scheduling onto a global.
    pub(crate) post_frame_handle: Option<flui_scheduler::PostFrameHandle>,

    /// The binding's OWNER-LOCAL post-frame capability — see
    /// `LifecycleContext::local_post_frame_handle`'s doc for why this is a
    /// separate handle rather than a second field on `PostFrameHandle`
    /// itself. `None` under the same conditions as `post_frame_handle`.
    pub(crate) local_post_frame_handle: Option<flui_scheduler::LocalPostFrameHandle>,

    /// The binding's IME/text-input attach-detach capability. `None` when no
    /// binding installed one, which makes `LifecycleContext::text_input_handle`
    /// report the absence rather than a widget silently having no way to
    /// attach an IME client.
    pub(crate) text_input_handle: Option<flui_interaction::TextInputHandle>,

    /// The presentation's plain-text clipboard. `None` only on a bare owner;
    /// a realm installs one over its platform's clipboard.
    pub(crate) clipboard_handle: Option<flui_interaction::ClipboardHandle>,

    /// The realm's byte storage. `None` on a bare owner and under a realm
    /// that has none, which `LifecycleContext::storage` reports as such.
    pub(crate) storage: Option<std::sync::Arc<dyn flui_platform_api::Storage>>,

    /// The host's flush registry, which `Persisted` publishes into through
    /// the crate-private `LifecycleContext::flush_registry_in_crate`. `None`
    /// on a bare owner, and under every realm until the host's installer
    /// lands with the registry's wiring.
    pub(crate) flush_registry: Option<crate::flush_registry::FlushRegistry>,

    /// The binding's owner-local interaction dispatch capability (ADR-0027).
    ///
    /// `None` means the owner was built detached from a runtime interaction lane;
    /// render-object lifecycle contexts report that as a typed inactive realm.
    pub(crate) interaction_dispatch: Option<flui_interaction::InteractionDispatchHandle>,
    pub(crate) hit_test_handle: Option<flui_interaction::HitTestHandle>,

    /// This owner's identity for [`GlobalKeyScope`] claim tagging (ADR-0043).
    ///
    /// Assigned once, from a process-wide monotonic counter, at construction —
    /// stable for the owner's whole lifetime regardless of whether a scope is
    /// ever installed.
    owner_tag: OwnerTag,

    /// The shared cross-owner `GlobalKey` uniqueness domain (ADR-0043).
    /// `None` until [`Self::set_global_key_scope`] installs one; the first
    /// `register_global_key` call after that lazily self-owns a private,
    /// single-tenant scope, so a standalone/test owner that never calls the
    /// setter behaves exactly as before — a private scope with one tenant
    /// never conflicts with itself.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) global_key_scope: Option<GlobalKeyScope>,
}

/// An element that has been deactivated and is pending unmount.
///
/// Made `pub(crate)` so [`ElementOwner`](super::ElementOwner) can hold a
/// `&mut Vec<InactiveElement>` split-borrow reference. End-of-frame
/// finalization (`BuildOwner::finalize_tree`) drains the queue
/// deepest-first using the recorded `depth`.
#[derive(Debug)]
pub(crate) struct InactiveElement {
    id: ElementId,
    depth: usize,
    detached_render_subtrees: Option<DetachedRenderSubtrees>,
}

impl InactiveElement {
    /// Construct a new inactive-element record.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self {
            id,
            depth,
            detached_render_subtrees: None,
        }
    }

    /// Construct a record that also owns the element's render-relocation token.
    pub(crate) fn with_detached_render_subtrees(
        id: ElementId,
        depth: usize,
        detached_render_subtrees: DetachedRenderSubtrees,
    ) -> Self {
        Self {
            id,
            depth,
            detached_render_subtrees: Some(detached_render_subtrees),
        }
    }

    /// The element id queued for end-of-frame unmount.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order finalization (deepest first).
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Whether this record still owns a render-relocation token.
    ///
    /// Lets a retake reject *before* dequeueing: removing an entry it cannot
    /// use would strand the element with no finalization record and no right
    /// to reattach its detached render subtree.
    pub(crate) fn holds_detached_render_subtrees(&self) -> bool {
        self.detached_render_subtrees.is_some()
    }

    /// Take the record's render-relocation token, leaving the record behind.
    pub(crate) fn take_detached_render_subtrees(&mut self) -> Option<DetachedRenderSubtrees> {
        self.detached_render_subtrees.take()
    }
}

impl Default for BuildOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildOwner {
    pub(crate) fn withdraw_owner(
        &mut self,
        preserving: bool,
    ) -> Vec<global_key_scope::ScopedKeyOwner> {
        self.external_inbox.close();
        self.reactive.withdraw_owner(preserving);
        self.global_key_scope
            .as_ref()
            .map_or_else(Vec::new, |scope| scope.take_owner_claims(self.owner_tag))
    }
    pub(crate) fn owner_closed(&self) -> bool {
        self.reactive.owner_closed()
    }
    /// Create a build owner with a fresh, isolated focus manager.
    pub fn new() -> Self {
        Self::with_focus_manager(FocusManager::new())
    }

    /// Create a build owner using the presentation's exact focus manager.
    ///
    /// The manager is not replaceable after construction: the element tree and
    /// focus tree share one ownership lifetime.
    pub fn with_focus_manager(focus_manager: Rc<FocusManager>) -> Self {
        Self::admit(focus_manager, OwnerTag::fresh)
    }

    /// Admits an owner for `focus_manager` under the tag `mint` issues.
    ///
    /// The tag is reserved before the owner takes the manager. An exhausted
    /// tag counter refuses admission by panicking; the supplied manager,
    /// possibly the caller's last strong reference, is then retained rather
    /// than dropped by the unwind, so a refusal never closes a focus tree
    /// and tombstones its nodes when no owner was admitted for it.
    fn admit(focus_manager: Rc<FocusManager>, mint: impl FnOnce() -> OwnerTag) -> Self {
        let focus_manager = std::mem::ManuallyDrop::new(focus_manager);
        let owner_tag = mint();
        let focus_manager = std::mem::ManuallyDrop::into_inner(focus_manager);
        let owner = Self {
            dirty_elements: BinaryHeap::new(),
            dirty_reasons: HashMap::new(),
            global_keys: GlobalKeyRegistry::new(),
            global_key_reservations: Box::new(GlobalKeyReservations::new()),
            global_key_diagnostics: Vec::new(),
            inactive_elements: Vec::new(),
            pending_dependency_changes: std::collections::HashSet::new(),
            inherited_dependencies: InheritedDependencies::default(),
            reactive: OwnerReactive(Some(crate::reactive::Reactive::new())),
            keep_alive: super::KeepAliveHolds::default(),
            tree_observer: OwnerTreeObserverSlot(None),
            recovered_panics: Vec::new(),
            lifecycle_panic_handoff: Cell::new(LifecyclePanicHandoff::Disarmed),
            #[cfg(debug_assertions)]
            building: false,
            #[cfg(debug_assertions)]
            scope_depth: 0,
            on_build_scheduled: OwnerBuildScheduledSlot(None),
            external_inbox: Arc::new(ExternalBuildInbox::default()),
            mid_drain_absorbs_left: MAX_MID_DRAIN_ABSORBS,
            mid_drain_cap_streak: false,
            built_this_frame: HashSet::new(),
            frame_builds: Box::default(),
            child_manager_registry: Arc::new(Mutex::new(HashMap::new())),
            layout_builder_registry: LayoutBuilderRegistry::default(),
            lazy_band_pass_budget: super::layout_builder::MAX_LAZY_BAND_PASSES,
            build_scope_queues: None,
            focus_manager,
            lifecycle_handle: None,
            async_driver: None,
            post_frame_handle: None,
            local_post_frame_handle: None,
            text_input_handle: None,
            clipboard_handle: None,
            storage: None,
            flush_registry: None,
            interaction_dispatch: None,
            hit_test_handle: None,
            owner_tag,
            global_key_scope: None,
        };
        // ADR-0074: writes must reach the inbox from the first frame, before any
        // binding installs a frame-request callback (`set_on_build_scheduled`
        // re-points the graph when one arrives).
        owner.reactive.set_scheduler(owner.external_scheduler());
        owner
    }

    #[doc(hidden)]
    pub fn set_lifecycle_handle(&mut self, handle: crate::LifecycleHandle) {
        self.lifecycle_handle = Some(handle);
    }

    /// Weak presentation lifecycle capability, absent for a bare owner.
    pub fn lifecycle_handle(&self) -> Option<crate::LifecycleHandle> {
        self.lifecycle_handle.clone()
    }

    /// Return this element tree's focus manager.
    ///
    /// Acquire the manager from `ViewState::init_state` or
    /// `did_change_dependencies` and retain the returned `Rc` for later focus
    /// transitions. Imperative focus changes do not belong in `build`, layout,
    /// paint, or compositing, which is why `LifecycleContext`, not
    /// `BuildContext`, hands it out.
    #[must_use]
    pub fn focus_manager(&self) -> Rc<FocusManager> {
        Rc::clone(&self.focus_manager)
    }

    /// Install the binding's async task driver.
    ///
    /// Called once, at wiring time, by `HeadlessBinding` and `UiRealm`. Must
    /// be the same driver the binding's frame step polls.
    pub fn set_async_driver(&mut self, driver: flui_scheduler::AsyncDriver) {
        self.async_driver = Some(driver);
    }

    /// The binding's async task driver, if one was installed.
    #[must_use]
    pub fn async_driver(&self) -> Option<&flui_scheduler::AsyncDriver> {
        self.async_driver.as_ref()
    }

    /// Install the binding's post-frame capability.
    ///
    /// Called once, at wiring time, by `HeadlessBinding` and, in production, by
    /// `UiRealm`'s own wiring. It must name **that binding's** scheduler — the
    /// one whose `drive_frame` drains the queue. Headless owns a binding-local
    /// `UpdateScheduler`; production drives the realm's own owned `UpdateScheduler`
    /// (`UiRealm.scheduler`) — there is no process-global scheduler singleton
    /// any more.
    pub fn set_post_frame_handle(&mut self, handle: flui_scheduler::PostFrameHandle) {
        self.post_frame_handle = Some(handle);
    }

    /// Install the binding's owner-local post-frame capability.
    ///
    /// Called once, at wiring time, alongside [`Self::set_post_frame_handle`] —
    /// same binding, same underlying scheduler, but a handle that can capture
    /// `Rc`/`RefCell` state because it addresses its lane directly.
    pub fn set_local_post_frame_handle(&mut self, handle: flui_scheduler::LocalPostFrameHandle) {
        self.local_post_frame_handle = Some(handle);
    }

    /// Install the binding's IME/text-input attach-detach capability.
    ///
    /// Called during `UiRealm` construction with the weak handle minted by that
    /// presentation's `TextInputOwner`. `HeadlessBinding` installs none, so
    /// headless-tree tests observe `LifecycleContext::text_input_handle() == None`
    /// honestly rather than accepting attaches nobody delivers events to.
    pub fn set_text_input_handle(&mut self, handle: flui_interaction::TextInputHandle) {
        self.text_input_handle = Some(handle);
    }

    /// Install the presentation's plain-text clipboard.
    ///
    /// Called during presentation construction with a handle over the
    /// realm's platform clipboard, so `LifecycleContext::clipboard_handle`
    /// answers `Some` under every realm.
    pub fn set_clipboard_handle(&mut self, handle: flui_interaction::ClipboardHandle) {
        self.clipboard_handle = Some(handle);
    }

    /// Install the realm's byte storage.
    ///
    /// Called during presentation construction when the realm has storage,
    /// so `LifecycleContext::storage` answers `Some` under it.
    pub fn set_storage(&mut self, storage: std::sync::Arc<dyn flui_platform_api::Storage>) {
        self.storage = Some(storage);
    }

    /// Install the binding's owner-local interaction dispatch handle (ADR-0027).
    pub fn set_interaction_dispatch_handle(
        &mut self,
        handle: flui_interaction::InteractionDispatchHandle,
    ) {
        self.interaction_dispatch = Some(handle);
    }

    /// Install this presentation's fresh-hit-test capability.
    ///
    /// Called during presentation assembly with a handle pairing the realm's
    /// dispatch ticket with a probe over THIS presentation's pipeline.
    /// `HeadlessBinding` installs none, so headless-tree tests observe
    /// `LifecycleContext::hit_test_handle() == None` honestly rather than reading
    /// some other tree.
    pub fn set_hit_test_handle(&mut self, handle: flui_interaction::HitTestHandle) {
        self.hit_test_handle = Some(handle);
    }

    /// Install the realm's shared `GlobalKey` uniqueness domain (ADR-0043).
    ///
    /// Called once, at presentation-assembly time, before this owner's tree
    /// is mounted — the same post-construction wiring pattern as
    /// [`Self::set_async_driver`] / [`Self::set_post_frame_handle`] /
    /// [`Self::set_text_input_handle`] / [`Self::set_interaction_dispatch_handle`].
    /// Every owner sharing one [`GlobalKeyScope`] participates in one
    /// cross-owner `GlobalKey` uniqueness domain: mounting a key that another
    /// owner sharing the same scope already holds fails eagerly — traced,
    /// then a panic naming both owners — rather than silently aliasing across
    /// trees.
    ///
    /// An owner that never calls this self-owns a private scope lazily on
    /// its first `GlobalKey` registration, so a standalone or test owner
    /// keeps today's single-tenant behavior with no wiring required.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_view::{BuildOwner, GlobalKeyScope};
    ///
    /// let scope = GlobalKeyScope::new();
    /// let mut owner_a = BuildOwner::new();
    /// let mut owner_b = BuildOwner::new();
    /// owner_a.set_global_key_scope(scope.clone());
    /// owner_b.set_global_key_scope(scope);
    /// // `owner_a` and `owner_b` now share one cross-owner `GlobalKey`
    /// // uniqueness domain.
    /// ```
    ///
    /// # Panics
    ///
    /// The "called once, before mount" precondition is load-bearing, not
    /// advisory: a call after this owner has already registered a
    /// `GlobalKey` would silently discard those keys' claims — a lazily
    /// self-owned private scope's claims, or a previously installed shared
    /// scope's claims — while the owner's local map still lists them as
    /// registered, letting a sibling owner mount the "same" key with no
    /// conflict and no trace, and leaking the discarded scope's claims as
    /// unreachable. This is a programming-error invariant, not a
    /// caller-recoverable one, so the check runs and panics in every build,
    /// release included — the cost is one cold-path emptiness test on a
    /// setter called once at assembly time. A `tracing::error!` record
    /// precedes the panic so an embedder catching it still has a structured
    /// diagnostic.
    pub fn set_global_key_scope(&mut self, scope: GlobalKeyScope) {
        let already_wired = !self.global_keys.is_empty() || self.global_key_scope.is_some();
        if already_wired {
            tracing::error!(
                "BuildOwner::set_global_key_scope called after this owner already registered \
                 a GlobalKey or installed a scope — call it once, at presentation assembly, \
                 before any element is mounted"
            );
            panic!(
                "BUG: set_global_key_scope called after GlobalKey registration began (or \
                 after a scope was installed) — claims would be silently discarded"
            );
        }
        self.global_key_scope = Some(scope);
    }

    /// This owner's [`GlobalKeyScope`] claim tag.
    ///
    /// Crate-internal: production code never names an owner's tag directly
    /// (conflicts and reclamation surface through tracing and panics, not
    /// tag comparison). Exists for tests that construct an adversarial
    /// multi-owner rig and need to drive `GlobalKeyScope::reclaim_owner`
    /// directly, outside the normal drop path.
    #[cfg(test)]
    pub(crate) fn owner_tag(&self) -> OwnerTag {
        self.owner_tag
    }

    /// The binding's post-frame capability, if one was installed.
    #[must_use]
    pub fn post_frame_handle(&self) -> Option<&flui_scheduler::PostFrameHandle> {
        self.post_frame_handle.as_ref()
    }

    /// The binding's owner-local post-frame capability, if one was installed.
    #[must_use]
    pub fn local_post_frame_handle(&self) -> Option<&flui_scheduler::LocalPostFrameHandle> {
        self.local_post_frame_handle.as_ref()
    }

    /// This owner's fresh-hit-test capability, if a presentation installed one.
    ///
    /// Stored per owner, not derived from the realm-wide interaction dispatch
    /// handle: a realm may host several presentations, each with its own
    /// `PipelineOwner`, and a hit test must read the tree of the presentation
    /// that asked. A realm-scoped probe would answer every one of them with
    /// whichever tree was installed first.
    #[must_use]
    pub fn hit_test_handle(&self) -> Option<&flui_interaction::HitTestHandle> {
        self.hit_test_handle.as_ref()
    }

    /// The binding's IME/text-input attach-detach capability, if one was
    /// installed.
    #[must_use]
    pub fn text_input_handle(&self) -> Option<&flui_interaction::TextInputHandle> {
        self.text_input_handle.as_ref()
    }

    /// The presentation's plain-text clipboard, if one was installed.
    #[must_use]
    pub fn clipboard_handle(&self) -> Option<&flui_interaction::ClipboardHandle> {
        self.clipboard_handle.as_ref()
    }

    /// The realm's byte storage, if one was installed.
    #[must_use]
    pub fn storage(&self) -> Option<&std::sync::Arc<dyn flui_platform_api::Storage>> {
        self.storage.as_ref()
    }

    /// Set the callback for when a build is scheduled.
    ///
    /// This is called by `schedule_build_for` to notify the binding
    /// that a visual update is needed.
    ///
    /// Set this BEFORE mounting any element. Each element captures a clone of
    /// the current callback `Arc` into its `ExternalBuildScheduler` at mount
    /// (for out-of-frame rebuild requests); replacing the callback afterwards
    /// does not retroactively update already-mounted elements, which keep
    /// firing the previous `Arc`. The binding wires this once at startup.
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.on_build_scheduled = Some(Arc::new(callback));
        self.reactive.set_scheduler(self.external_scheduler());
    }

    /// The realm's reactive graph (ADR-0074): signals and the
    /// reader registry that schedules exactly the elements that read a
    /// written signal.
    pub fn reactive(&self) -> &crate::reactive::Reactive {
        &self.reactive
    }

    /// Schedule an element for rebuild.
    ///
    /// Elements are processed in depth order (shallowest first) so parent
    /// rebuilds happen before child rebuilds. The `depth` is a best-effort
    /// ordering hint: [`build_scope`](Self::build_scope) re-derives every
    /// queued element's authoritative tree depth from its node before draining
    /// (see `rekey_dirty_depths`), so a caller that
    /// only knows the sibling slot index (e.g. `setState` via
    /// `ElementCore::schedule_self_build`) cannot mis-order the drain.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize, reason: RebuildReason) {
        let newly_queued = match self.dirty_reasons.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(RebuildReasons::from_reason(reason));
                self.dirty_elements
                    .push(Reverse(DirtyElement::new(id, depth)));
                true
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().insert(reason);
                false
            }
        };
        self.external_inbox
            .request_frame_if_needed(newly_queued, self.on_build_scheduled.as_deref());
    }

    /// Mark every live element dirty so the next [`build_scope`](Self::build_scope)
    /// re-runs all `build()` methods.
    ///
    /// Used by hot reload. **Does not** unmount elements or dispose `State` — stateful
    /// elements keep their in-tree `ViewState` across the call.
    ///
    /// Both halves are required, and for a while only one was here: the drain
    /// skips any element whose own `is_dirty()` is false (its guard exists so a
    /// clean entry that reached the heap through an inherited-dependency change
    /// cannot have its children reconciled away), so marking the *heap* without
    /// marking the *element* queued work the drain then dropped — a hot reload
    /// that rebuilt nothing. `tree.mark_needs_build` sets the flag;
    /// `schedule_build_for` orders the entry. The external-inbox drain works the
    /// same two-step way.
    pub fn reassemble(&mut self, tree: &mut ElementTree) {
        let ids: Vec<(ElementId, usize)> = tree
            .iter_nodes()
            .map(|(id, node)| (id, node.depth()))
            .collect();
        for (id, depth) in ids {
            tree.mark_needs_build(id);
            self.schedule_build_for(id, depth, RebuildReason::HotReload);
        }
        tracing::info!(
            count = self.dirty_count(),
            "BuildOwner::reassemble — all elements marked dirty for hot reload"
        );
    }

    /// Re-key every queued dirty element to its authoritative TREE depth.
    ///
    /// The dirty heap orders by depth, but `schedule_build_for` is handed a
    /// depth by its caller and trusts it. A caller that passes a stale or
    /// wrong depth would let a deeply-nested rebuild sort as if it were
    /// shallow, so a child could rebuild before its parent — violating
    /// the shallowest-first contract. Rebuilding the heap keyed on each
    /// node's current depth (`ElementNode::depth`, the same authority the
    /// external-inbox drain uses) keeps the contract regardless of what
    /// `schedule_build_for` was told.
    fn rekey_dirty_depths(&mut self, tree: &ElementTree) {
        if self.dirty_elements.is_empty() {
            return;
        }
        let queued: Vec<ElementId> = std::mem::take(&mut self.dirty_elements)
            .into_iter()
            .map(|Reverse(dirty)| dirty.id())
            .collect();
        for id in queued {
            let depth = tree.get(id).map_or(0, crate::tree::ElementNode::depth);
            self.dirty_elements
                .push(Reverse(DirtyElement::new(id, depth)));
        }
    }

    /// Return the nearest mounted layout-builder scope containing `element`.
    /// The search includes `element` itself: a scope root belongs to its own
    /// scope.
    fn live_layout_scope_ids(&self, tree: &ElementTree) -> HashSet<ElementId> {
        self.layout_builder_registry.with_entries(|entries| {
            entries
                .values()
                .filter(|entry| tree.contains(entry.element))
                .map(|entry| entry.element)
                .collect()
        })
    }

    fn nearest_layout_builder_scope(
        tree: &ElementTree,
        element: ElementId,
        live_scopes: &HashSet<ElementId>,
    ) -> Option<ElementId> {
        #[cfg(test)]
        LAYOUT_SCOPE_CLASSIFICATIONS.with(|count| count.set(count.get() + 1));
        let mut cursor = Some(element);
        while let Some(candidate) = cursor {
            if live_scopes.contains(&candidate) && tree.contains(candidate) {
                return Some(candidate);
            }
            cursor = tree
                .get(candidate)
                .and_then(crate::tree::ElementNode::parent);
        }
        None
    }

    fn defer_dirty_element(&mut self, scope: Option<ElementId>, dirty: DirtyElement) {
        let queues = self
            .build_scope_queues
            .get_or_insert_with(|| Box::new(BuildScopeQueues::default()));
        if let Some(scope) = scope {
            queues
                .isolated
                .entry(scope)
                .or_default()
                .push(Reverse(dirty));
        } else {
            queues.root.push(Reverse(dirty));
        }
    }

    fn collect_partitioned_dirty(&mut self) {
        let mut scratch = self
            .build_scope_queues
            .as_mut()
            .map_or_else(Vec::new, |queues| std::mem::take(&mut queues.scratch));
        if let Some(queues) = self.build_scope_queues.as_mut() {
            scratch.extend(
                std::mem::take(&mut queues.root)
                    .into_iter()
                    .map(|Reverse(dirty)| dirty),
            );
            for bucket in std::mem::take(&mut queues.isolated).into_values() {
                scratch.extend(bucket.into_iter().map(|Reverse(dirty)| dirty));
            }
        }
        scratch.extend(
            std::mem::take(&mut self.dirty_elements)
                .into_iter()
                .map(|Reverse(dirty)| dirty),
        );
        self.build_scope_queues
            .get_or_insert_with(|| Box::new(BuildScopeQueues::default()))
            .scratch = scratch;
    }

    pub(crate) fn repartition_dirty(
        &mut self,
        tree: &ElementTree,
        live_scopes: &HashSet<ElementId>,
    ) {
        self.collect_partitioned_dirty();
        while let Some(mut dirty) = self
            .build_scope_queues
            .as_mut()
            .expect("BUG: repartition creates scoped queues")
            .scratch
            .pop()
        {
            dirty.depth = tree
                .get(dirty.id())
                .map_or(0, crate::tree::ElementNode::depth);
            let scope = Self::nearest_layout_builder_scope(tree, dirty.id(), live_scopes);
            self.defer_dirty_element(scope, dirty);
        }
    }

    pub(crate) fn activate_build_target(&mut self, target: BuildScopeTarget) {
        let Some(queues) = self.build_scope_queues.as_mut() else {
            return;
        };
        let mut active = match target {
            BuildScopeTarget::All => return,
            BuildScopeTarget::Global => std::mem::take(&mut queues.root),
            BuildScopeTarget::LayoutBuilder(scope) => {
                queues.isolated.remove(&scope).unwrap_or_default()
            }
        };
        self.dirty_elements.append(&mut active);
    }

    pub(crate) fn ready_layout_scopes(&self) -> Vec<ElementId> {
        self.build_scope_queues
            .as_ref()
            .map_or_else(Vec::new, |queues| queues.isolated.keys().copied().collect())
    }

    pub(crate) fn has_pending_layout_scope(&self, scope: ElementId) -> bool {
        self.build_scope_queues
            .as_ref()
            .is_some_and(|queues| queues.isolated.contains_key(&scope))
    }

    pub(crate) fn has_pending_global_builds(&self) -> bool {
        self.build_scope_queues
            .as_ref()
            .is_some_and(|queues| !queues.root.is_empty())
    }

    pub(crate) fn has_partitioned_builds(&self) -> bool {
        self.build_scope_queues.as_ref().is_some_and(|queues| {
            !queues.root.is_empty() || queues.isolated.values().any(|bucket| !bucket.is_empty())
        })
    }

    pub(crate) fn collapse_empty_build_scope_queues(&mut self) {
        let is_empty = self.build_scope_queues.as_ref().is_some_and(|queues| {
            queues.root.is_empty() && queues.isolated.is_empty() && queues.scratch.is_empty()
        });
        if is_empty {
            self.build_scope_queues = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn reset_layout_scope_classifications() {
        LAYOUT_SCOPE_CLASSIFICATIONS.with(|count| count.set(0));
    }

    #[cfg(test)]
    pub(crate) fn layout_scope_classifications() -> usize {
        LAYOUT_SCOPE_CLASSIFICATIONS.with(std::cell::Cell::get)
    }

    /// Acquire an [`ElementOwner`](super::ElementOwner) split-borrow
    /// handle for the duration of an Element lifecycle traversal.
    ///
    /// The returned handle holds disjoint `&mut` references to
    /// `global_keys`, `dirty_elements`, `dirty_reasons`, and
    /// `inactive_elements` — every field an `Element::mount` /
    /// `unmount` / `update` path may write. The borrow checker proves
    /// non-aliasing because each field is borrowed once.
    pub fn element_owner_mut(&mut self) -> super::ElementOwner<'_> {
        let partitioned_dirty_count = self.partitioned_dirty_count();
        super::ElementOwner {
            global_keys: &mut self.global_keys,
            global_key_reservations: &mut self.global_key_reservations,
            dirty_elements: &mut self.dirty_elements,
            partitioned_dirty_count,
            dirty_reasons: &mut self.dirty_reasons,
            inactive_elements: &mut self.inactive_elements,
            pending_dependency_changes: &mut self.pending_dependency_changes,
            inherited_dependencies: &mut self.inherited_dependencies,
            keep_alive: self.keep_alive.clone(),
            on_build_scheduled: self.on_build_scheduled.as_deref(),
            external_inbox: &self.external_inbox,
            external_request_frame: self.on_build_scheduled.as_ref(),
            // Lifecycle paths (mount/unmount/update) get no live-tree view;
            // only the `build_scope` drain sets `build_view`.
            build_view: None,
            child_manager_registry: &self.child_manager_registry,
            layout_builder_registry: &self.layout_builder_registry,
            focus_manager: &self.focus_manager,
            lifecycle_handle: &self.lifecycle_handle,
            async_driver: &self.async_driver,
            post_frame_handle: &self.post_frame_handle,
            local_post_frame_handle: &self.local_post_frame_handle,
            text_input_handle: &self.text_input_handle,
            clipboard_handle: &self.clipboard_handle,
            storage: &self.storage,
            flush_registry: &self.flush_registry,
            interaction_dispatch: &self.interaction_dispatch,
            hit_test_handle: &self.hit_test_handle,
            global_key_scope: &mut self.global_key_scope,
            owner_tag: self.owner_tag,
            tree_observer: &mut self.tree_observer,
            recovered_panics: &mut self.recovered_panics,
            build_recovered: None,
            reactive: &self.reactive,
            lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
        }
    }

    /// Install the realm's tree observer, replacing any previous one
    /// (ADR-0040). A replaced observer receives `detached()` first, and the
    /// replacement is logged so competing tools discover each other.
    ///
    /// Install at realm setup or via
    /// `WidgetsBinding::install_tree_observer` — never from a frame phase.
    pub fn set_tree_observer(&mut self, observer: Arc<dyn flui_foundation::observe::TreeObserver>) {
        if let Some(previous) = self.tree_observer.replace(observer) {
            notify_detached(previous);
        }
    }

    /// Remove the observer (realm teardown — install/clear symmetry).
    /// Fires `detached()` on the outgoing observer. Idempotent.
    pub fn clear_tree_observer(&mut self) {
        if let Some(previous) = self.tree_observer.take() {
            notify_detached(previous);
        }
    }

    /// The installed observer, if any. Clones the `Arc` out — no reference
    /// into private state, no guard.
    #[must_use]
    pub fn tree_observer(&self) -> Option<Arc<dyn flui_foundation::observe::TreeObserver>> {
        self.tree_observer.clone()
    }

    /// Record the reverse half of an inherited dependency registration.
    ///
    /// The caller writes the provider's forward map in the same critical
    /// section. Keeping this method crate-private prevents consumers from
    /// manufacturing an ownership edge without going through a real
    /// `BuildContext::depend_on` lookup.
    pub(crate) fn register_inherited_dependency(
        &mut self,
        dependent: ElementId,
        provider: ElementId,
    ) {
        self.inherited_dependencies.register(dependent, provider);
    }

    /// Build an [`ExternalBuildScheduler`] over this owner's shared inbox and
    /// frame-request hook.
    ///
    /// The single construction point for the out-of-frame rebuild channel:
    /// [`ElementOwner::external_scheduler`](super::ElementOwner::external_scheduler)
    /// and `ElementBuildContext::rebuild_handle` both route through it, so there
    /// is exactly one inbox and one frame-request path.
    pub(crate) fn external_scheduler(&self) -> ExternalBuildScheduler {
        ExternalBuildScheduler::from_parts(
            Arc::clone(&self.external_inbox),
            self.on_build_scheduled.clone(),
        )
    }

    /// A [`RebuildHandle`](super::RebuildHandle) for `element`, scheduling
    /// through this owner's inbox.
    pub(crate) fn rebuild_handle(&self, element: ElementId) -> super::RebuildHandle {
        super::RebuildHandle::new(self.external_scheduler(), element)
    }

    /// What the most recent `build_scope` rebuilt: the number of distinct
    /// elements built this frame and, per [`RebuildReason`], how many of them
    /// carried that cause (an element scheduled for two reasons counts once in
    /// `elements_built` and once under each reason). Reset at every
    /// `build_scope`, so read it after a pump, before the next one.
    pub fn last_frame_build_report(&self) -> FrameBuildReport {
        FrameBuildReport {
            elements_built: self.built_this_frame.len(),
            builds_run: self.frame_builds.builds_run,
            by_reason: self.frame_builds.by_reason(),
        }
    }

    /// Number of elements queued in the out-of-frame inbox, awaiting the next
    /// [`build_scope`](Self::build_scope) drain.
    ///
    /// Observability for the `RebuildHandle` channel: a
    /// `schedule(reason)` from a worker thread is visible here before any frame runs.
    /// Returns a count, never a guard — the lock stays private.
    #[must_use]
    pub fn pending_external_builds(&self) -> usize {
        self.external_inbox.lock().len()
    }

    /// Return the exact causes currently queued for `element`.
    ///
    /// The snapshot merges in-frame and out-of-frame scheduling, because a
    /// listener may enqueue another cause after an element was already placed
    /// on the dirty heap. `None` means no rebuild is pending. The queue and its
    /// lock stay private; callers cannot mutate scheduling state through this
    /// diagnostic API.
    #[must_use]
    pub fn pending_rebuild_reasons(&self, element: ElementId) -> Option<RebuildReasons> {
        let mut pending = self.dirty_reasons.get(&element).copied();
        let external = self.external_inbox.lock().get(&element).copied();
        if let Some(external) = external {
            match &mut pending {
                Some(reasons) => reasons.merge(external),
                None => pending = Some(external),
            }
        }
        pending
    }

    /// Check if there are dirty elements.
    pub fn has_dirty_elements(&self) -> bool {
        !self.dirty_elements.is_empty()
            || self.build_scope_queues.as_ref().is_some_and(|queues| {
                !queues.root.is_empty() || queues.isolated.values().any(|bucket| !bucket.is_empty())
            })
            // Externally scheduled rebuilds (RebuildHandle) land in a shared
            // inbox that build_scope absorbs throughout the drain (at the top
            // of every heap pop, not only once at the start) — so a pending
            // entry IS dirty work, and a frame gate that ignored it skipped
            // the very frame the handle's own frame-request hook woke: the
            // schedule stalled until some unrelated dirty state arrived.
            || !self.external_inbox.lock().is_empty()
    }

    /// Get the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len() + self.partitioned_dirty_count()
    }

    fn partitioned_dirty_count(&self) -> usize {
        self.build_scope_queues.as_ref().map_or(0, |queues| {
            queues.root.len() + queues.isolated.values().map(BinaryHeap::len).sum::<usize>()
        })
    }

    /// Process all dirty elements outside layout-builder scopes.
    ///
    /// Rebuilds elements in depth order (shallowest first). This ensures
    /// that when a parent rebuilds, any children that become dirty are
    /// processed after the parent.
    ///
    /// # Arguments
    ///
    /// * `tree` - The element tree to rebuild
    pub fn build_scope(&mut self, tree: &mut ElementTree) {
        // Per-frame mid-drain absorb accounting: re-arm the exhaustion streak
        // against how the PREVIOUS frame ended — BEFORE this frame's own
        // reset overwrites `mid_drain_absorbs_left` — then reset the budget
        // and the re-entry set for this frame. There is no frame-complete
        // hook on `BuildOwner`, so this retroactive check at the NEXT entry
        // is the only place "did the last frame end clean" can be answered.
        //
        // This reset must run exactly once per real frame, which is why it
        // lives here rather than in `build_scope_impl`: `build_scope` is the
        // frame-boundary entry point every production/headless binding calls
        // once per pump, but `service_child_requests_impl`'s lazy-sliver
        // service pass also needs to drain newly-built children's subtrees
        // — possibly several times in the SAME frame, once per fixpoint pass
        // — and calls `build_scope_impl` directly for that, so the budget it
        // shares stays a genuine per-frame bound instead of resetting on
        // every service pass.
        if self.mid_drain_absorbs_left > 0 {
            self.mid_drain_cap_streak = false;
        }
        self.mid_drain_absorbs_left = MAX_MID_DRAIN_ABSORBS;
        self.built_this_frame.clear();
        self.frame_builds.clear();

        self.build_scope_impl(tree);
    }

    /// The reusable core of [`Self::build_scope`]: the build span, the
    /// early-return when a mounted-but-clean layout builder has no work,
    /// drain-target selection, the drain itself, and collapsing the scoped
    /// queues when they empty out. Deliberately excludes the per-frame
    /// mid-drain accounting reset that only [`Self::build_scope`] performs —
    /// see that method's doc for why a mid-frame re-entrant caller (the
    /// lazy-sliver service pass) must reach this directly instead.
    fn build_scope_impl(&mut self, tree: &mut ElementTree) {
        // The build phase's span, matching the `layout`/`paint`/`compositing`
        // spans the pipeline already emits. Together the four are what a
        // profiler subscribes to: `flui-devtools` is layer 9 and nothing in
        // the framework may consume it, so tracing is the only seam by which
        // frame timings can reach it. `absorbed_mid_drain` is recorded by
        // `drain_build_scope` once the drain completes — declared empty here
        // so the field exists on the span from the start.
        let _span = tracing::debug_span!(
            "build",
            dirty_elements = self.dirty_count(),
            absorbed_mid_drain = tracing::field::Empty,
        )
        .entered();

        let has_partitioned_work = self
            .build_scope_queues
            .as_ref()
            .is_some_and(|queues| !queues.root.is_empty() || !queues.isolated.is_empty());
        if !self.layout_builder_registry.is_empty()
            && !has_partitioned_work
            && self.dirty_elements.is_empty()
            && self.external_inbox.lock().is_empty()
        {
            // A mounted but clean layout builder must not turn the lazy scoped
            // queues into a per-frame allocation. There is no work to route.
            return;
        }
        let target = if self.layout_builder_registry.is_empty() && !has_partitioned_work {
            // Preserve the pre-scope hot path exactly when the feature is inert.
            BuildScopeTarget::All
        } else {
            BuildScopeTarget::Global
        };
        self.build_scope_target(tree, target);
        self.collapse_empty_build_scope_queues();
    }

    pub(crate) fn build_scope_target(&mut self, tree: &mut ElementTree, target: BuildScopeTarget) {
        let live_scopes = match target {
            BuildScopeTarget::All => None,
            BuildScopeTarget::Global | BuildScopeTarget::LayoutBuilder(_) => {
                let live_scopes = self.live_layout_scope_ids(tree);
                self.repartition_dirty(tree, &live_scopes);
                self.activate_build_target(target);
                Some(live_scopes)
            }
        };
        let _ = self.drain_prepared_build_target(tree, target, live_scopes.as_ref());
    }

    /// Finish a parented lifecycle recovery after the panicking element has
    /// been restored to its slab slot.
    ///
    /// The configurable view factory is the last fallible step before the
    /// destructive replacement transaction begins. A factory unwind retains
    /// the original element and its dirty entry; the owned staged diagnostic
    /// is then dropped without touching earlier committed records. Once
    /// `replace_child_with` starts, its fatal no-rollback contract applies.
    #[cold]
    fn replace_failed_lifecycle_element(
        &mut self,
        tree: &mut ElementTree,
        failed: ElementId,
        replacement_location: (ElementId, usize),
        dirty: DirtyElement,
        staged: StagedRecoveredPanic,
        payload: Box<dyn Any + Send>,
    ) {
        let (parent, slot) = replacement_location;
        let error = crate::view::FrameworkError::from_panic(
            payload.as_ref(),
            "running a stateful lifecycle hook during rebuild",
        );
        let recovery_view = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::view::recovery_view_for(&error)
        })) {
            Ok(view) => view,
            Err(factory_payload) => {
                self.dirty_elements
                    .push(Reverse(DirtyElement::new(dirty.id(), dirty.depth())));
                std::panic::resume_unwind(factory_payload)
            }
        };

        self.dirty_reasons.remove(&failed);
        self.pending_dependency_changes.remove(&failed);
        let mut element_owner = self.element_owner_mut();
        tree.replace_child_with(parent, slot, recovery_view.0.as_ref(), &mut element_owner);
        element_owner.commit_staged_lifecycle_panic(staged);
    }

    pub(crate) fn drain_prepared_build_target(
        &mut self,
        tree: &mut ElementTree,
        target: BuildScopeTarget,
        live_scopes: Option<&HashSet<ElementId>>,
    ) -> BuildDrainResult {
        #[cfg(not(debug_assertions))]
        if matches!(target, BuildScopeTarget::All) {
            return self.drain_build_scope(tree, target, live_scopes);
        }

        #[cfg(debug_assertions)]
        {
            assert!(!self.building, "build_scope called while already building");
            self.building = true;
            self.scope_depth += 1;
        }

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.drain_build_scope(tree, target, live_scopes)
        }));

        if outcome.is_err() && !matches!(target, BuildScopeTarget::All) {
            let live_scopes = live_scopes.expect("BUG: scoped drain owns a live-scope snapshot");
            self.repartition_dirty(tree, live_scopes);
        }

        #[cfg(debug_assertions)]
        {
            self.building = false;
            self.scope_depth -= 1;
        }

        match outcome {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn drain_build_scope(
        &mut self,
        tree: &mut ElementTree,
        target: BuildScopeTarget,
        live_scopes: Option<&HashSet<ElementId>>,
    ) -> BuildDrainResult {
        let mut result = BuildDrainResult::default();
        let mut absorbed_mid_drain = 0usize;
        // Ids capped for the rest of THIS drain accumulate here instead of
        // being put straight back into `self.external_inbox` — see
        // `CappedLeftoverGuard`'s doc for why a guard, not a flush placed
        // after the loop, and `Self::absorb_mid_drain_inbox`'s doc for the
        // budget rules.
        let mut capped_leftover = CappedLeftoverGuard::new(Arc::clone(&self.external_inbox));

        // Re-key every element already on the heap to its current tree depth
        // before draining. `schedule_build_for` trusts the depth its caller
        // passes; a stale one (a subtree moved by a GlobalKey after the
        // schedule) would let a deeply-nested rebuild sort as if it were
        // shallow, so a child could build before its parent and violate
        // the shallowest-first build contract, which keys on the element's
        // real depth. Re-derive each id's depth from its node — the same authority
        // `absorb_mid_drain_inbox` uses for a freshly-absorbed id.
        self.rekey_dirty_depths(tree);

        // Process dirty elements in depth order, extract-then-apply
        // (E3 — atomic box→arena swap).
        //
        // The hard problem: the old loop held `&mut tree.get_mut(id).element`
        // while calling `perform_build`, so `perform_build` could not also
        // take `&mut ElementTree` to reconcile slab-resident children —
        // the exact double-borrow that cost the render-tree PRs. The fix
        // is the same extract-then-apply discipline E2.5 proves out, lifted
        // to the build seam:
        //
        //   1. Take a `&mut element` borrowed FROM the tree, run the
        //      behavior's build half (`build_into_views`), capture the
        //      OWNED child views, and DROP the element borrow.
        //   2. With a FRESH `&mut tree` borrow, feed those views to the
        //      id-reconciler, which inserts / updates / removes the
        //      slab-resident child nodes.
        //
        // No `&mut` into the slab is ever live across a second slab access.
        //
        // Absorb the out-of-frame inbox BEFORE every pop, not only once
        // before the first one (issue #1180): a build below can call another
        // element's `RebuildHandle::schedule` synchronously (the inbox is the
        // only route a cross-thread or listener-driven rebuild has), and that
        // schedule must join THIS drain rather than sit until the next
        // `build_scope`; the heap is re-sorted every iteration. See
        // `Self::absorb_mid_drain_inbox` for the Occupied/Vacant/budget rules.
        //
        // Each iteration pops one entry first so `pop()`'s mutation of
        // `self.dirty_elements` (a field the split-borrow handle aliases)
        // is released before the handle is reborrowed.
        //
        // `absorbed_mid_drain` only counts from the SECOND iteration onward:
        // the very first absorb of this call is ordinary intake — whatever
        // landed between the previous drain (or frame) and this one, before
        // a single element here has built — not something that arrived
        // "mid-drain" in the sense a profiler cares about. Only an absorb
        // that runs after this call has already popped and built at least
        // one entry observed something land while THIS drain was actively
        // in progress, which is what the field name promises.
        let mut first_pop = true;
        loop {
            let absorbed_this_pop =
                self.absorb_mid_drain_inbox(tree, &mut capped_leftover.leftover);
            if !first_pop {
                absorbed_mid_drain += absorbed_this_pop;
            }
            first_pop = false;
            let Some(Reverse(dirty)) = self.dirty_elements.pop() else {
                break;
            };
            let id = dirty.id();
            let nearest_scope = match target {
                BuildScopeTarget::All => None,
                BuildScopeTarget::Global | BuildScopeTarget::LayoutBuilder(_) => {
                    Self::nearest_layout_builder_scope(
                        tree,
                        id,
                        live_scopes.expect("BUG: scoped drain owns a live-scope snapshot"),
                    )
                }
            };
            let target_accepts = match target {
                BuildScopeTarget::All => true,
                BuildScopeTarget::Global => nearest_scope.is_none(),
                BuildScopeTarget::LayoutBuilder(scope) => nearest_scope == Some(scope),
            };
            if !target_accepts {
                self.defer_dirty_element(nearest_scope, dirty);
                continue;
            }

            let reasons_at_start = self
                .dirty_reasons
                .get(&id)
                .copied()
                .expect("BUG: every dirty heap entry must own rebuild reasons");

            // If this dependent received an inherited-dependency change since its
            // last build, fire `ViewState::did_change_dependencies` BEFORE
            // the build. Consumed here so the typed hook runs exactly once
            // per dependency-change-then-rebuild cycle.
            let needs_did_change = self.pending_dependency_changes.contains(&id);

            // Guard (still holding only a brief `&mut node`): an
            // inherited-dependency change marks an otherwise-clean dependent
            // dirty so its build re-runs against the new value; then skip
            // unless the element is both buildable (lifecycle) AND dirty. A
            // clean element's build half returns an empty view list, and the
            // phase-2 reconcile would then wrongly REMOVE all its children —
            // so a clean entry must never reach reconcile.
            {
                let Some(node) = tree.get_mut(id) else {
                    // Stale / removed id — nothing to build.
                    self.dirty_reasons.remove(&id);
                    self.pending_dependency_changes.remove(&id);
                    continue;
                };
                if needs_did_change {
                    node.element_mut().mark_needs_build();
                }
                if !node.element().lifecycle().can_build() || !node.element().is_dirty() {
                    self.dirty_reasons.remove(&id);
                    self.pending_dependency_changes.remove(&id);
                    continue;
                }
            }

            // This element is about to re-state, from scratch, which keyed
            // children it declares — so everything the frame recorded about
            // it up to now is superseded. Clearing here rather than after
            // the reconcile is what makes the newest build authoritative:
            // the reconcile below immediately re-reserves whatever this
            // build still declares, and anything it has dropped simply does
            // not come back.
            self.global_key_reservations.note_parent_rebuild(id);

            let rebuild_span = tracing::info_span!(
                "element_rebuild",
                element = ?id,
                reasons = %reasons_at_start,
            );
            let _rebuild_entered = rebuild_span.enter();

            // ── Phase 1: extract BY VALUE, build against a LIVE read view.
            // Taking the element out of its slot frees the tree for a shared
            // `&` borrow, so the element's `build()` can resolve InheritedView
            // / ancestor lookups against the REAL tree (via the `BuildCtx`
            // the behaviour builds from `ElementOwner::build_view`) — no empty
            // dummy, and no deadlock against the `Arc<RwLock>` write lock the
            // frame driver holds (the borrowed view sidesteps the lock).
            // Inherited dependents are buffered (the tree is read-only here)
            // and applied below once `&mut tree` is free again.
            let Some(mut element) = tree.take_element(id) else {
                continue;
            };
            let replacement_location = tree
                .get(id)
                .and_then(|node| node.parent().map(|parent| (parent, node.slot())));
            let dep_sink: parking_lot::Mutex<Vec<crate::context::DependentRecord>> =
                parking_lot::Mutex::new(Vec::new());

            // Run the build half under `catch_unwind` so the extracted element
            // is ALWAYS restored to its slot before this seam decides whether
            // to substitute or propagate. User `build()` is already recovered
            // one level down. The two stateful lifecycle calls reachable here
            // catch only their literal user-hook expressions, stage only when
            // this parented seam arms them, and rethrow into this guard. Any
            // other unwind remains unrecorded and propagates after restoration.
            // `AssertUnwindSafe` is sound because the sole cross-unwind
            // invariant — the slot is whole again — is re-established by the
            // unconditional `put_element` below.
            // Set by `build_or_recover` if this element's build panics and is
            // recovered (reset-on-build then keeps its masks).
            let build_recovered = std::cell::Cell::new(false);
            let partitioned_dirty_count = self.partitioned_dirty_count();
            let build_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut element_owner = super::ElementOwner {
                    global_keys: &mut self.global_keys,
                    global_key_reservations: &mut self.global_key_reservations,
                    dirty_elements: &mut self.dirty_elements,
                    partitioned_dirty_count,
                    dirty_reasons: &mut self.dirty_reasons,
                    inactive_elements: &mut self.inactive_elements,
                    pending_dependency_changes: &mut self.pending_dependency_changes,
                    inherited_dependencies: &mut self.inherited_dependencies,
                    keep_alive: self.keep_alive.clone(),
                    on_build_scheduled: self.on_build_scheduled.as_deref(),
                    external_inbox: &self.external_inbox,
                    external_request_frame: self.on_build_scheduled.as_ref(),
                    build_view: Some(super::BuildHandle {
                        tree: &*tree,
                        dep_sink: &dep_sink,
                    }),
                    child_manager_registry: &self.child_manager_registry,
                    layout_builder_registry: &self.layout_builder_registry,
                    focus_manager: &self.focus_manager,
                    lifecycle_handle: &self.lifecycle_handle,
                    async_driver: &self.async_driver,
                    post_frame_handle: &self.post_frame_handle,
                    local_post_frame_handle: &self.local_post_frame_handle,
                    text_input_handle: &self.text_input_handle,
                    clipboard_handle: &self.clipboard_handle,
                    storage: &self.storage,
                    flush_registry: &self.flush_registry,
                    interaction_dispatch: &self.interaction_dispatch,
                    hit_test_handle: &self.hit_test_handle,
                    global_key_scope: &mut self.global_key_scope,
                    owner_tag: self.owner_tag,
                    tree_observer: &mut self.tree_observer,
                    recovered_panics: &mut self.recovered_panics,
                    build_recovered: Some(&build_recovered),
                    reactive: &self.reactive,
                    lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
                };
                if needs_did_change {
                    if replacement_location.is_some() {
                        element_owner.arm_lifecycle_panic_handoff();
                    }
                    element
                        .element_mut()
                        .notify_dependency_change(&mut element_owner);
                    if replacement_location.is_some() {
                        assert!(
                            element_owner.take_staged_lifecycle_panic().is_none(),
                            "a successful dependency hook must leave its own window unrecorded"
                        );
                    }
                }
                if replacement_location.is_some() {
                    element_owner.arm_lifecycle_panic_handoff();
                }
                let views = element.element_mut().build_into_views(&mut element_owner);
                if replacement_location.is_some() {
                    assert!(
                        element_owner.take_staged_lifecycle_panic().is_none(),
                        "a successful initial-state/build call must leave its own window unrecorded"
                    );
                }
                views
            })); // `element_owner` + its `&*tree` borrow drop here.

            // Restore the element BEFORE anything else — the slot must be whole
            // whether the build returned or unwound. With `&mut tree` free
            // again we then apply the dependents buffered during the read-only
            // build onto their provider nodes; recording in the SAME iteration
            // (before the next dirty pop) preserves the record-before-notify
            // ordering.
            tree.put_element(id, element);

            let new_views: Vec<Box<dyn View>> = match build_outcome {
                Ok(views) => {
                    debug_assert!(
                        self.lifecycle_panic_handoff.take().is_disarmed(),
                        "successful lifecycle windows must disarm their handoff immediately"
                    );
                    views
                }
                Err(payload) => {
                    let staged = self.lifecycle_panic_handoff.take().into_staged();
                    let Some((parent, slot)) = replacement_location else {
                        self.dirty_elements
                            .push(Reverse(DirtyElement::new(dirty.id(), dirty.depth())));
                        std::panic::resume_unwind(payload)
                    };
                    let Some(staged) = staged else {
                        self.dirty_elements
                            .push(Reverse(DirtyElement::new(dirty.id(), dirty.depth())));
                        std::panic::resume_unwind(payload)
                    };

                    self.replace_failed_lifecycle_element(
                        tree,
                        id,
                        (parent, slot),
                        dirty,
                        staged,
                        payload,
                    );
                    result.any_rebuilt = true;

                    if !matches!(target, BuildScopeTarget::All) {
                        let fresh_live_scopes = self.live_layout_scope_ids(tree);
                        self.repartition_dirty(tree, &fresh_live_scopes);
                        self.activate_build_target(target);
                    }
                    continue;
                }
            };

            let reasons = self
                .dirty_reasons
                .remove(&id)
                .expect("BUG: a completed rebuild must retain its rebuild reasons");
            result.any_rebuilt = true;
            result.target_rebuilt |=
                matches!(target, BuildScopeTarget::LayoutBuilder(scope) if scope == id);
            self.pending_dependency_changes.remove(&id);
            // A LATER mid-drain absorb of this same id is a re-entry, not a
            // first-time absorb, whether it comes from this element
            // rescheduling itself, a child notifying it back, or one half of
            // an A↔B ping-pong — see `Self::absorb_mid_drain_inbox`.
            self.built_this_frame.insert(id);
            self.frame_builds.builds_run += 1;
            for reason in reasons.iter() {
                self.frame_builds.record(reason);
            }

            // ADR-0040: the build ran to completion (the resume_unwind branch
            // can no longer take it) and the slot is restored — safe window
            // for third-party observer code, and a parent's rebuilt event
            // precedes the child mutations its build produced (phase 2).
            if self.tree_observer.is_some() {
                let view_type_id = tree.get(id).map(|node| node.element().view_type_id());
                if let Some(view_type_id) = view_type_id {
                    super::emit_observation(&mut self.tree_observer, |o| {
                        o.element_rebuilt(&flui_foundation::observe::ElementRebuilt::new(
                            id,
                            view_type_id,
                            reasons,
                        ));
                    });
                }
            }
            // Reset-on-build (ADR-0074 §5.5; dependencies do not
            // accumulate until unmount): the fields this
            // element is recorded as reading at each of its providers go back
            // to NONE, the build's own reads are re-applied from the sink, and a
            // provider it no longer read drops the entry. A field read only in
            // an earlier build therefore stops rebuilding this element.
            //
            // A build that panicked and was recovered (ErrorView substituted;
            // `build_or_recover` sets the owner's `build_recovered` flag) may
            // have stopped before reading its providers: the empty sink says nothing
            // about what the element depends on, so its previous masks are kept
            // (the sink's records are still added) and it keeps receiving the
            // notifications that let a fixed condition rebuild it.
            let previous_providers = if build_recovered.get() {
                super::inherited_dependencies::ProviderIds::default()
            } else {
                self.inherited_dependencies.providers_of(id)
            };
            for provider in &previous_providers {
                if let Some(accessor) = tree
                    .get_mut(*provider)
                    .and_then(|node| node.element_mut().as_inherited_mut())
                {
                    accessor.reset_dependent_mask(crate::context::CrateToken::new(), id);
                }
            }
            for record in dep_sink.into_inner() {
                let Some(node) = tree.get_mut(record.provider) else {
                    continue;
                };
                let Some(accessor) = node.element_mut().as_inherited_mut() else {
                    continue;
                };
                if record.lifecycle {
                    accessor.record_lifecycle_dependent(
                        crate::context::CrateToken::new(),
                        record.dependent,
                        record.depth,
                        record.mask,
                    );
                } else {
                    accessor.record_dependent(
                        crate::context::CrateToken::new(),
                        record.dependent,
                        record.depth,
                        record.mask,
                    );
                }
                self.inherited_dependencies
                    .register(record.dependent, record.provider);
            }
            for provider in previous_providers {
                let pruned = tree
                    .get_mut(provider)
                    .and_then(|node| node.element_mut().as_inherited_mut())
                    .is_some_and(|accessor| {
                        accessor.prune_unread_dependent(crate::context::CrateToken::new(), id)
                    });
                if pruned {
                    self.inherited_dependencies.unregister(id, provider);
                }
            }

            // ── Phase 2: reconcile the returned views against the node's
            // slab-resident children with a fresh `&mut tree`. Newly inserted
            // children are scheduled inside the reconciler so this same drain
            // loop reaches them. The outer catch below is unwind hygiene, not
            // containment: an unbounded framework panic still propagates, but
            // the rebuilding parent regains its consumed reason and queue
            // entry before the scoped-drain guard repartitions all work.
            let partitioned_dirty_count = self.partitioned_dirty_count();
            let reconcile_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut element_owner = super::ElementOwner {
                    global_keys: &mut self.global_keys,
                    global_key_reservations: &mut self.global_key_reservations,
                    dirty_elements: &mut self.dirty_elements,
                    partitioned_dirty_count,
                    dirty_reasons: &mut self.dirty_reasons,
                    inactive_elements: &mut self.inactive_elements,
                    pending_dependency_changes: &mut self.pending_dependency_changes,
                    inherited_dependencies: &mut self.inherited_dependencies,
                    keep_alive: self.keep_alive.clone(),
                    on_build_scheduled: self.on_build_scheduled.as_deref(),
                    external_inbox: &self.external_inbox,
                    external_request_frame: self.on_build_scheduled.as_ref(),
                    build_view: None,
                    child_manager_registry: &self.child_manager_registry,
                    layout_builder_registry: &self.layout_builder_registry,
                    focus_manager: &self.focus_manager,
                    lifecycle_handle: &self.lifecycle_handle,
                    async_driver: &self.async_driver,
                    post_frame_handle: &self.post_frame_handle,
                    local_post_frame_handle: &self.local_post_frame_handle,
                    text_input_handle: &self.text_input_handle,
                    clipboard_handle: &self.clipboard_handle,
                    storage: &self.storage,
                    flush_registry: &self.flush_registry,
                    interaction_dispatch: &self.interaction_dispatch,
                    hit_test_handle: &self.hit_test_handle,
                    global_key_scope: &mut self.global_key_scope,
                    owner_tag: self.owner_tag,
                    tree_observer: &mut self.tree_observer,
                    recovered_panics: &mut self.recovered_panics,
                    build_recovered: None,
                    reactive: &self.reactive,
                    lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
                };
                crate::tree::id_reconcile::reconcile_children_by_id(
                    tree,
                    id,
                    &new_views,
                    &mut element_owner,
                );
            }));
            if let Err(payload) = reconcile_outcome {
                tree.mark_needs_build(id);
                match self.dirty_reasons.entry(id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(reasons);
                        self.dirty_elements
                            .push(Reverse(DirtyElement::new(dirty.id(), dirty.depth())));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().merge(reasons);
                    }
                }
                std::panic::resume_unwind(payload);
            }
        }

        // `capped_leftover`'s `Drop` flushes whatever this drain capped back
        // into the real inbox here, on this normal-return path exactly like
        // it would on an unwind through the loop above — see
        // `CappedLeftoverGuard`'s doc.

        // The build drained: every render child has attached. Settle each
        // render parent's children into element-slot order (no-op unless an
        // insert flagged a possible drift), so a render sibling that attached
        // before a component-deferred sibling does not invert their layout.
        tree.reorder_render_children_after_build();

        // Record onto whichever span is current — `build_scope`'s own "build"
        // span and `service_layout_builders`'s `during_layout` "build" span
        // both declare this field now, so a fixpoint pass's re-entries are
        // captured exactly like a top-level drain's (a span that never
        // declares a field silently drops a `record` for it, which is why
        // both call sites must).
        tracing::Span::current().record("absorbed_mid_drain", absorbed_mid_drain);

        result
    }

    /// Absorb every id currently sitting in the out-of-frame inbox onto this
    /// drain's dirty work, routed to whichever bucket `target` accepts.
    /// Returns how many ids were absorbed (merged into existing work or
    /// freshly scheduled) — never how many merely landed, since a capped
    /// re-entry is held in `capped_leftover` (the caller's drain-scoped
    /// accumulator), not absorbed.
    ///
    /// Called at the top of every [`Self::drain_build_scope`] pop, not once
    /// at drain start (issue #1180): `BuildOwner`'s own
    /// synchronous `schedule` — a build calling another element's
    /// [`RebuildHandle`](super::RebuildHandle) on the owner thread — joins
    /// THIS SAME drain instead of waiting for the next `build_scope` call,
    /// collapsing the double-frame the issue describes into one. There is no
    /// debug-mode assert that a mid-build
    /// schedule targets a descendant of the element currently building (see
    /// the `## Mapping decisions` entry in `crates/flui-view/ARCHITECTURE.md`
    /// for why: the inbox is the only route a cross-thread or
    /// listener-driven rebuild has, and a synchronous `schedule` call cannot
    /// tell "during my own build" from "after it").
    ///
    /// # Occupied vs Vacant
    ///
    /// At the top of a pop nothing is "being served" as a distinct state:
    /// `dirty_reasons` Occupied means the id is already on the heap or
    /// sitting in a deferred layout-builder-scope bucket, and the landed
    /// reasons simply merge into whichever bucket already holds it (a
    /// deferred id stays in its bucket). Vacant means a fresh entry is
    /// needed, keyed by the AUTHORITATIVE tree depth (never the depth
    /// captured at `schedule` time — `ElementCore` does not know its own
    /// tree depth), and pushed straight onto `self.dirty_elements` —
    /// `Self::drain_build_scope`'s own pop already re-derives this id's
    /// nearest layout-builder scope and runs `Self::defer_dirty_element` on
    /// it if `target` doesn't accept it, so classifying scope again here
    /// would only duplicate that pop-time routing, never change its
    /// outcome (a Global id landing during a `LayoutBuilder(scope)` drain
    /// still ends up deferred to the root bucket for the next Global drain,
    /// and the mirror still lands a scope-local id in `isolated[scope]`
    /// during a Global drain).
    ///
    /// # The re-entry budget
    ///
    /// A RE-ENTRY is a Vacant landing for an id that already completed a
    /// build in this `build_scope` call — [`Self::built_this_frame`] is how
    /// that is recognized. The common shape is an element rescheduling
    /// itself from inside its own build, but a child notifying its
    /// already-built parent, or one half of an A↔B ping-pong, land here
    /// identically. Every other Vacant id is a first-time absorb this frame
    /// and is free, however many independent listeners fire (each id can
    /// only be a first-time absorb once, so the free case is finite). A
    /// re-entry spends one [`Self::mid_drain_absorbs_left`]; at zero the id
    /// is held in `capped_leftover` for the rest of this drain (the existing
    /// `has_dirty_elements` gate already schedules the next `build_scope`
    /// once it is flushed back — see `Self::drain_build_scope`) and a
    /// `tracing::warn!` fires once per streak
    /// ([`Self::mid_drain_cap_streak`]), naming every id this call turned
    /// away.
    ///
    /// # Why `capped_leftover` is the caller's, not the inbox's
    ///
    /// Once the budget hits zero, every further re-entry for the rest of the
    /// drain is capped too — a self-rescheduler that keeps re-entering re-hits
    /// this every pop. Putting each one straight back into `self.external_inbox`
    /// would mean every LATER pop's `drain()` pulls the same already-capped
    /// ids back out, re-locks, and re-inserts them again, for no new
    /// information. Accumulating them in `capped_leftover` — the
    /// [`CappedLeftoverGuard`] [`Self::drain_build_scope`] owns, passed in
    /// here by `&mut` to its `leftover` field — means a capped id is looked
    /// at once, and `self.external_inbox` can go and stay empty for the
    /// remainder of the drain (the common case once budget is exhausted), so
    /// later pops hit this function's early return instead of a full
    /// lock-drain-collect cycle.
    ///
    /// # Locking
    ///
    /// The inbox lock is held only for the `drain()` that empties it into an
    /// owned `Vec`, released before anything else runs. It is never held
    /// across a build — `parking_lot::Mutex` is non-reentrant, and a build
    /// may call `schedule` synchronously. `capped_leftover` itself is a
    /// plain, unlocked map; `CappedLeftoverGuard::drop` re-locks the inbox
    /// exactly once, when the guard goes out of scope — on the drain's
    /// normal return OR on an unwind through the loop, so a capped id
    /// survives a later build's panic in the same drain instead of vanishing
    /// with the guard's stack frame.
    fn absorb_mid_drain_inbox(
        &mut self,
        tree: &mut ElementTree,
        capped_leftover: &mut HashMap<ElementId, RebuildReasons>,
    ) -> usize {
        let landed: Vec<(ElementId, RebuildReasons)> = {
            let mut inbox = self.external_inbox.lock();
            if inbox.is_empty() {
                return 0;
            }
            inbox.drain().collect()
        };

        let mut absorbed = 0usize;
        let mut newly_capped_ids: Vec<ElementId> = Vec::new();
        for (id, reasons) in landed {
            // Mark dirty unconditionally, Occupied or Vacant — matching the
            // pre-#1180 drain's own unconditional call. A `RebuildHandle`
            // carries no reference to the element's dirty flag (a plain
            // `(inbox, ElementId)` pair), so this is the one place that both
            // knows the id and holds `&mut tree`. A node since unmounted is a
            // no-op lookup.
            tree.mark_needs_build(id);
            if let Some(existing) = self.dirty_reasons.get_mut(&id) {
                existing.merge(reasons);
                absorbed += 1;
                continue;
            }
            if self.built_this_frame.contains(&id) {
                let Some(remaining) = self.mid_drain_absorbs_left.checked_sub(1) else {
                    newly_capped_ids.push(id);
                    match capped_leftover.entry(id) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(reasons);
                        }
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            entry.get_mut().merge(reasons);
                        }
                    }
                    continue;
                };
                self.mid_drain_absorbs_left = remaining;
            }
            self.dirty_reasons.insert(id, reasons);
            let depth = tree.get(id).map_or(0, crate::tree::ElementNode::depth);
            // Push straight onto the heap: `drain_build_scope`'s pop already
            // re-derives this id's scope and defers non-accepted ids, so
            // classifying scope here too would only duplicate that work.
            self.dirty_elements
                .push(Reverse(DirtyElement::new(id, depth)));
            absorbed += 1;
        }

        if !newly_capped_ids.is_empty() && !self.mid_drain_cap_streak {
            tracing::warn!(
                ids = ?newly_capped_ids,
                budget = MAX_MID_DRAIN_ABSORBS,
                "mid-drain absorb budget exhausted this frame; deferring the \
                 remaining re-entered id(s) to the next build_scope"
            );
            self.mid_drain_cap_streak = true;
        }

        absorbed
    }

    // ========================================================================
    // Lazy-sliver child manager service
    // ========================================================================

    /// Drive lazy-sliver child managers post-layout.
    ///
    /// Called by `HeadlessBinding::pump_frame` (and future production bindings)
    /// immediately **after** `PipelineOwner::run_frame` and the pipeline lock
    /// is released, so the render tree is quiescent and no `NodePtr` alias is
    /// live.
    ///
    /// # Ordering
    ///
    /// 1. Drain `PipelineOwner`'s `pending_child_requests` and
    ///    `pending_retain_bands` accumulated during the most recent layout pass.
    /// 2. Group by `RenderId`.
    /// 3. Clone each affected manager `Arc` out of the registry (releases the
    ///    registry lock before the service calls).
    /// 4. For each manager, call `ChildManager::service` with an inline
    ///    `ElementOwner` split-borrow — mirrors the pattern in `build_scope`'s
    ///    `catch_unwind` closure.
    /// 5. A second `build_scope` expands the newly-built children's subtrees
    ///    (the items' own views — e.g. `Padding(Text)` — need their own build
    ///    pass since `SparseChildren::ensure` only mounts the top-level node).
    /// 6. Mark each affected sliver as needing layout so the next frame
    ///    re-measures with the newly-present render nodes.
    /// 7. `finalize_tree` cleans up any evicted children pushed to inactive by
    ///    `retain_band` or `on_unmount`.
    ///
    /// # Production and headless bindings
    ///
    /// Called by both `HeadlessBinding::pump_frame` (step 6) and
    /// `UiRealm::draw_frame` (after `run_frame` drops the pipeline write-lock)
    /// — the two frame paths are now converged at this call site. Headless
    /// tests drive it directly via `HeadlessBinding`; a real window drives it via
    /// `WidgetsBinding::service_child_requests`, which `UiRealm::draw_frame`
    /// invokes after each `run_frame`.
    ///
    /// Returns `true` iff a manager built or evicted a child — the sliver was
    /// then marked `needs_layout`, and the caller (the layout↔build fixpoint
    /// in `run_frame_with_layout_builders`) must run another layout pass so
    /// the change is laid out and painted in this same frame.
    pub fn service_child_requests(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
    ) -> bool {
        self.service_child_requests_impl(
            tree,
            pipeline,
            ServiceFinalize::Frame,
            ServiceRequests::Build,
        )
    }

    /// [`Self::service_child_requests`] as run between two layout passes of
    /// the same frame's fixpoint.
    ///
    /// Identical servicing, but the finalize step only unmounts the evicted
    /// children — it does **not** verify (and so does not clear) the
    /// `GlobalKey` reservation ledger. Verification is a per-*frame* verdict
    /// (ADR-0050): two lazy slivers that both declare one key in the same
    /// frame must be seen by one verification, and the fixpoint may service
    /// them in different passes. The frame's post-`run_frame` call keeps
    /// the full finalize.
    pub(crate) fn service_child_requests_between_passes(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
    ) -> bool {
        self.service_child_requests_impl(
            tree,
            pipeline,
            ServiceFinalize::UnmountOnly,
            ServiceRequests::Build,
        )
    }

    /// Apply the retain bands the last layout pass recorded and leave its
    /// build requests queued: the evict-before-paint step of a frame whose
    /// lazy band did not settle within its pass budget.
    ///
    /// The fixpoint stops servicing when the budget trips, so the residents
    /// the last pass's band no longer covers would otherwise stay attached
    /// through the frame's final layout and paint — positioned nowhere by
    /// that layout (the walk positions in-band children only) yet painted,
    /// hit-tested, and assembled into semantics at whatever offset they
    /// last had. Evicting them here, and marking their sliver for the
    /// final layout, means nothing outside the committed band exists when
    /// the frame paints. The requests are not touched: they stay queued
    /// for the post-frame service, which builds them for the next frame —
    /// the deferral the budget asked for, without the stale residents.
    /// (Taking them here would cost that band a frame: a sliver the
    /// eviction left clean is not laid out again before the frame ends,
    /// so nothing would re-record them.)
    pub(crate) fn service_child_requests_evict_only(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
    ) -> bool {
        self.service_child_requests_impl(
            tree,
            pipeline,
            ServiceFinalize::UnmountOnly,
            ServiceRequests::Leave,
        )
    }

    fn service_child_requests_impl(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
        finalize: ServiceFinalize,
        requests: ServiceRequests,
    ) -> bool {
        let finalize = |owner: &mut Self, tree: &mut ElementTree| match finalize {
            ServiceFinalize::Frame => owner.finalize_tree(tree),
            ServiceFinalize::UnmountOnly => owner.unmount_inactive_elements(tree),
        };
        // 1. Drain pending buffers from the pipeline (a brief checkout).
        let (pending_requests, retain_bands) = pipeline.with_mut(|guard| {
            let requests = match requests {
                ServiceRequests::Build => guard.take_pending_child_requests(),
                ServiceRequests::Leave => Vec::new(),
            };
            let bands = guard.take_pending_retain_bands();
            (requests, bands)
        });

        // Finalize BEFORE the early-return: `build_scope` does not call
        // `finalize_tree`, so sparse children pushed to `inactive_elements` by
        // `SliverAdaptorBehavior::on_unmount` (F3) — during a reconcile that
        // removed their host — must be cleaned up here. Without this, those
        // elements and their render nodes are leaked until the next
        // `service_child_requests` call that has pending layout requests.
        if !self.inactive_elements.is_empty() {
            finalize(self, tree);
        }

        if pending_requests.is_empty() && retain_bands.is_empty() {
            return false;
        }

        tracing::debug!(
            requests = pending_requests.len(),
            bands = retain_bands.len(),
            "service_child_requests: draining lazy-sliver pending buffers"
        );

        // 2. Group requests and retain-bands by sliver RenderId.
        let mut requests_by_sliver: HashMap<RenderId, Vec<usize>> = HashMap::new();
        for (sliver_id, logical_index) in pending_requests {
            requests_by_sliver
                .entry(sliver_id)
                .or_default()
                .push(logical_index);
        }
        let mut bands_by_sliver: HashMap<RenderId, (usize, usize)> = HashMap::new();
        for (sliver_id, first, last) in retain_bands {
            bands_by_sliver.insert(sliver_id, (first, last));
        }

        // Collect all affected sliver ids (may have requests, bands, or both).
        let affected_ids: Vec<RenderId> = {
            let mut ids: HashSet<RenderId> = requests_by_sliver.keys().copied().collect();
            ids.extend(bands_by_sliver.keys().copied());
            ids.into_iter().collect()
        };

        // 3. Clone manager Arcs out of the registry before any service call.
        //    This releases the registry lock so that service calls that call
        //    `register_child_manager` / `unregister_child_manager` through an
        //    `ElementOwner` can re-enter the registry without deadlocking.
        let manager_arcs: Vec<(RenderId, Arc<Mutex<dyn ChildManager>>)> = {
            let registry = self.child_manager_registry.lock();
            affected_ids
                .iter()
                .filter_map(|&id| registry.get(&id).map(|m| (id, Arc::clone(m))))
                .collect()
        };

        if manager_arcs.is_empty() {
            tracing::debug!("service_child_requests: no registered managers for affected slivers");
            return false;
        }

        // 4. Call service on each manager. Each iteration builds an inline
        //    ElementOwner split-borrow, calls service (which may call
        //    `ensure`/`evict` and mutate the tree + dirty heap), then drops
        //    the inline owner — the borrow ends before the next iteration.
        //
        //    Track whether any manager did real work (built or evicted a child).
        //    When all managers return `false` (settled state — band unchanged,
        //    no new requests in-band) we skip `mark_needs_layout` so the sliver
        //    stays clean and the frame quiesces rather than looping forever.
        let mut any_service_did_work = false;
        for (sliver_id, manager_arc) in &manager_arcs {
            let requested = requests_by_sliver
                .get(sliver_id)
                .map_or(&[][..], Vec::as_slice);
            let (retain_first, retain_last) = bands_by_sliver
                .get(sliver_id)
                .copied()
                .unwrap_or((0, usize::MAX));

            // Inline split-borrow (same pattern as `build_scope` catch_unwind).
            let partitioned_dirty_count = self.partitioned_dirty_count();
            let mut inline_owner = super::ElementOwner {
                global_keys: &mut self.global_keys,
                global_key_reservations: &mut self.global_key_reservations,
                dirty_elements: &mut self.dirty_elements,
                partitioned_dirty_count,
                dirty_reasons: &mut self.dirty_reasons,
                inactive_elements: &mut self.inactive_elements,
                pending_dependency_changes: &mut self.pending_dependency_changes,
                inherited_dependencies: &mut self.inherited_dependencies,
                keep_alive: self.keep_alive.clone(),
                on_build_scheduled: self.on_build_scheduled.as_deref(),
                external_inbox: &self.external_inbox,
                external_request_frame: self.on_build_scheduled.as_ref(),
                build_view: None,
                child_manager_registry: &self.child_manager_registry,
                layout_builder_registry: &self.layout_builder_registry,
                focus_manager: &self.focus_manager,
                lifecycle_handle: &self.lifecycle_handle,
                async_driver: &self.async_driver,
                post_frame_handle: &self.post_frame_handle,
                local_post_frame_handle: &self.local_post_frame_handle,
                text_input_handle: &self.text_input_handle,
                clipboard_handle: &self.clipboard_handle,
                storage: &self.storage,
                flush_registry: &self.flush_registry,
                interaction_dispatch: &self.interaction_dispatch,
                hit_test_handle: &self.hit_test_handle,
                global_key_scope: &mut self.global_key_scope,
                owner_tag: self.owner_tag,
                tree_observer: &mut self.tree_observer,
                recovered_panics: &mut self.recovered_panics,
                build_recovered: None,
                reactive: &self.reactive,
                lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
            };

            let did_work = manager_arc.lock().service(
                requested,
                retain_first,
                retain_last,
                tree,
                &mut inline_owner,
                pipeline,
            );
            if did_work {
                any_service_did_work = true;
            }
        } // `inline_owner` drops here — all `&mut` borrows released.

        // 5. Second build_scope: expand newly-built children's subtrees.
        //    `SparseChildren::ensure` mounts the top-level lazy-child node and
        //    pushes it onto the dirty heap (F1), but the child's own sub-views
        //    (e.g. a Padding wrapping a Text) need a dedicated build pass.
        //    `build_scope_impl`, not `build_scope`: this can run more than
        //    once per frame (once per fixpoint pass), and must NOT reset the
        //    mid-drain absorb budget each time — that reset is exactly once
        //    per frame, in `build_scope` itself.
        self.build_scope_impl(tree);

        // 6. Mark each serviced sliver as needing re-layout ONLY if service
        //    did real work (built or evicted children). When all service calls
        //    returned `false`, the list has settled: the viewport, band, and
        //    built-child set are all unchanged. Skipping `mark_needs_layout`
        //    keeps the sliver clean so `flush_layout` skips it, `perform_layout`
        //    does not run, no new `emit_retain_band` fires, and
        //    `service_child_requests` early-returns on the next frame.
        //    This breaks the unconditional dirty-loop that prevented quiescence.
        if any_service_did_work {
            pipeline.with_mut(|guard| {
                for (sliver_id, _) in &manager_arcs {
                    guard.mark_needs_layout(*sliver_id);
                }
            });
        } else {
            tracing::debug!(
                "service_child_requests: no work done, skipping mark_needs_layout \
                 (frame quiesces)"
            );
        }

        // 7. Finalize: unmount evicted children (pushed to inactive by
        //    `retain_band` → `evict` → `tree.remove_subtree`) and the lazy
        //    children pushed by `on_unmount` (F3).
        finalize(self, tree);
        any_service_did_work
    }

    // ========================================================================
    // Inactive Elements (for finalization)
    // ========================================================================

    /// Add an element to the inactive list.
    ///
    /// Called when an element is deactivated (e.g., its parent rebuilds without
    /// it). The element will be unmounted in `finalize_tree()`.
    pub fn add_to_inactive(&mut self, id: ElementId, depth: usize) {
        self.inactive_elements.push(InactiveElement::new(id, depth));
    }

    /// Remove an element from the inactive list and return its relocation token.
    ///
    /// Called when an element is reactivated (e.g., moved via GlobalKey).
    /// A returned token must be reattached or released for finalization.
    #[must_use]
    pub fn remove_from_inactive(&mut self, id: ElementId) -> Option<DetachedRenderSubtrees> {
        let index = self
            .inactive_elements
            .iter()
            .position(|inactive| inactive.id() == id)?;
        self.inactive_elements
            .remove(index)
            .take_detached_render_subtrees()
    }

    /// Check if there are inactive elements pending unmount.
    pub fn has_inactive_elements(&self) -> bool {
        !self.inactive_elements.is_empty()
    }

    /// Complete the element build pass by unmounting inactive elements.
    ///
    /// This is called by `WidgetsBinding.draw_frame()` after `build_scope()`
    /// and `super.draw_frame()` (layout/paint).
    ///
    /// Elements are unmounted in reverse depth order (deepest first) to ensure
    /// children are unmounted before parents.
    pub fn finalize_tree(&mut self, tree: &mut ElementTree) {
        self.unmount_inactive_elements(tree);
        self.verify_global_key_reservations(tree);
    }

    /// Verify this frame's `GlobalKey` declarations, then clear them.
    ///
    /// Runs at the very end of [`Self::finalize_tree`], AFTER the inactive
    /// sweep, so a key whose only remaining claimant was unmounted this
    /// frame is not reported.
    ///
    /// Reports are appended to the diagnostic drain rather than raised, and
    /// each one has already had its losing parent's dangling child edge
    /// repaired — see [`super::global_key_reservations`].
    fn verify_global_key_reservations(&mut self, tree: &mut ElementTree) {
        if self.global_key_reservations.is_empty() {
            return;
        }
        let reports = global_key_reservations::verify(&mut self.global_key_reservations, tree);
        self.global_key_diagnostics.extend(reports);
    }

    /// The inactive-element unmount sweep — [`Self::finalize_tree`]'s first
    /// half, split out so the reservation verification after it runs on
    /// every frame, including the frames with nothing to unmount.
    fn unmount_inactive_elements(&mut self, tree: &mut ElementTree) {
        if self.inactive_elements.is_empty() {
            return;
        }

        tracing::debug!(
            count = self.inactive_elements.len(),
            "Finalizing tree - unmounting inactive elements"
        );

        // Sort by depth (deepest first for unmounting)
        self.inactive_elements
            .sort_by_key(|entry| std::cmp::Reverse(entry.depth()));

        // Take ownership of inactive elements to avoid borrow conflicts.
        // `mem::take` snapshots the queue before the unmount sweep so
        // mid-iteration `ElementOwner::push_inactive` calls (e.g. children
        // deactivating as a parent unmounts) land in the *next* frame's
        // queue rather than re-entering this drain — same snapshot-then-fire
        // discipline as `ChangeNotifier::notify_listeners` (foundation
        // notifier.rs:158-163).
        let inactive_elements: Vec<_> = std::mem::take(&mut self.inactive_elements);
        let mut inactive_elements = inactive_elements;

        // Consume every linear relocation token before ordinary unmount begins.
        // This authorizes finalization without deleting render nodes or firing
        // callbacks; the existing deepest-first unmount below remains the sole
        // lifecycle and render-disposal path.
        for inactive in &mut inactive_elements {
            let Some(token) = inactive.take_detached_render_subtrees() else {
                continue;
            };
            let pipeline = tree
                .get(inactive.id())
                .and_then(|node| node.element().pipeline_owner())
                .expect(
                    "BUG: an inactive element with a detached render token must retain its pipeline owner",
                );
            pipeline.with_mut(|pipeline_owner| {
                pipeline_owner
                    .release_detached_render_subtrees_for_finalization(token)
                    .expect(
                        "BUG: an inactive element's detached render token must remain valid until finalization",
                    );
            });
        }

        // Collect all elements to unmount (including children)
        let mut elements_to_unmount = Vec::new();
        for inactive in &inactive_elements {
            Self::collect_elements_to_unmount(tree, inactive.id(), &mut elements_to_unmount);
        }

        // Build the split-borrow handle once for the entire unmount sweep.
        // The handle survives `tree.get_mut` borrows because it points into
        // disjoint `BuildOwner` fields. No live build runs here, so the
        // build-time tree handle is absent.
        let partitioned_dirty_count = self.partitioned_dirty_count();
        let mut element_owner = super::ElementOwner {
            global_keys: &mut self.global_keys,
            global_key_reservations: &mut self.global_key_reservations,
            dirty_elements: &mut self.dirty_elements,
            partitioned_dirty_count,
            dirty_reasons: &mut self.dirty_reasons,
            inactive_elements: &mut self.inactive_elements,
            pending_dependency_changes: &mut self.pending_dependency_changes,
            inherited_dependencies: &mut self.inherited_dependencies,
            keep_alive: self.keep_alive.clone(),
            on_build_scheduled: self.on_build_scheduled.as_deref(),
            external_inbox: &self.external_inbox,
            external_request_frame: self.on_build_scheduled.as_ref(),
            build_view: None,
            child_manager_registry: &self.child_manager_registry,
            layout_builder_registry: &self.layout_builder_registry,
            focus_manager: &self.focus_manager,
            lifecycle_handle: &self.lifecycle_handle,
            async_driver: &self.async_driver,
            post_frame_handle: &self.post_frame_handle,
            local_post_frame_handle: &self.local_post_frame_handle,
            text_input_handle: &self.text_input_handle,
            clipboard_handle: &self.clipboard_handle,
            storage: &self.storage,
            flush_registry: &self.flush_registry,
            interaction_dispatch: &self.interaction_dispatch,
            hit_test_handle: &self.hit_test_handle,
            global_key_scope: &mut self.global_key_scope,
            owner_tag: self.owner_tag,
            tree_observer: &mut self.tree_observer,
            recovered_panics: &mut self.recovered_panics,
            build_recovered: None,
            reactive: &self.reactive,
            lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
        };

        // Finalize all elements (deepest first - already sorted by collect order).
        //
        // `remove_finalized` bypasses the soft-remove
        // path that `remove` takes for keyed elements. At this point
        // we've already given mid-frame state migration its chance —
        // anything still in the inactive queue is genuinely going away,
        // so we slab-remove + unregister the GlobalKey directly.
        for id in elements_to_unmount.iter().rev() {
            tree.remove_finalized(*id, &mut element_owner);
        }

        tracing::debug!("Inactive-element sweep complete");
    }

    /// Iteratively collect all element IDs to unmount, parent before
    /// children (pre-order DFS, children in
    /// [`ElementNode::child_ids`](crate::tree::ElementNode) slot order).
    ///
    /// E3: children come from the slab-resident `child_ids` list — the
    /// single element graph. `finalize_tree` reverses the collected order
    /// so `remove_finalized` runs deepest-first; pre-order guarantees a
    /// parent always precedes every one of its descendants, so the
    /// reversed sweep never frees a parent slot before its children.
    ///
    /// The walk is driven by an explicit `Vec` work-stack instead of
    /// recursion: the element tree nests several times deeper than the
    /// render tree, and a recursive shape overflowed the 1 MiB Windows
    /// main-thread stack on deep chains (the failure class PR #177
    /// closed for the render-tree walks). To preserve the recursive
    /// shape's visit order on a LIFO stack, children are pushed in
    /// reverse slot order so the leftmost child is popped next — same
    /// discipline as `WidgetsBinding::collect_all_elements`.
    ///
    /// Complexity: O(n) time over the n reachable nodes, average and
    /// worst case (each node pushed/popped exactly once); the work-stack
    /// peaks at O(n) heap in the degenerate all-siblings case and O(tree
    /// height) for a chain. Call-stack usage is constant.
    fn collect_elements_to_unmount(tree: &ElementTree, id: ElementId, result: &mut Vec<ElementId>) {
        let mut stack: Vec<ElementId> = vec![id];
        while let Some(id) = stack.pop() {
            result.push(id);
            // The `tree.get` shared borrow ends with the statement; the
            // extend writes only into the local stack, never the slab.
            if let Some(node) = tree.get(id) {
                stack.extend(node.child_ids().iter().rev().copied());
            }
        }
    }

    /// Lock the build scope (for debugging).
    ///
    /// Returns a guard that unlocks when dropped.
    #[cfg(debug_assertions)]
    pub fn lock_build_scope(&mut self) -> BuildScopeGuard<'_> {
        assert!(!self.building, "Already in build scope");
        self.building = true;
        BuildScopeGuard { owner: self }
    }

    // ========================================================================
    // GlobalKey Registry
    // ========================================================================

    /// Register a GlobalKey for an element.
    ///
    /// GlobalKeys allow elements to be found and reparented across the tree.
    /// If a [`GlobalKeyScope`] is installed (or, absent that, this owner's
    /// lazily self-owned private one — see [`Self::set_global_key_scope`]),
    /// this first claims `key` in that scope: a different owner already
    /// holding the same hash there is a cross-owner collision, traced then
    /// panicked (ADR-0043) rather than silently recorded here.
    ///
    /// # Panics
    ///
    /// Panics if `key` is already claimed, in the installed
    /// [`GlobalKeyScope`], by a *different* owner (see
    /// [`GlobalKeyScope`]'s contract). Re-registering a key this same owner
    /// already holds never panics. This panic is fatal, not recoverable: it
    /// fires from a post-mount registration site, the same timing the
    /// pre-existing intra-tree duplicate-key panic already has, so this
    /// owner's tree is left with an incomplete mount. A host must not catch
    /// it and keep using this owner's tree.
    pub fn register_global_key(&mut self, key: &dyn ViewKey, element: ElementId) {
        global_key_scope::claim_and_register(
            &mut self.global_key_scope,
            self.owner_tag,
            key,
            element,
            &mut self.global_keys,
        );
    }

    /// Unregister a GlobalKey.
    ///
    /// Also releases this owner's scope claim on `key`, if one is
    /// installed — tag-checked, so this is a no-op against a claim a
    /// different owner has since taken (ADR-0043).
    pub fn unregister_global_key(&mut self, key: &dyn ViewKey) {
        global_key_scope::release_and_unregister(
            self.global_key_scope.as_ref(),
            self.owner_tag,
            key,
            &mut self.global_keys,
        );
    }

    /// Look up an element by GlobalKey.
    ///
    /// Resolution is by key identity — a different key that merely hashes
    /// alike never answers this lookup.
    pub fn element_for_global_key(&self, key: &dyn ViewKey) -> Option<ElementId> {
        self.global_keys.get(key)
    }

    /// Atomically remove and return the element registered under
    /// `key` for a reparent operation.
    ///
    /// Closes the race window that a
    /// two-call sequence (`element_for_global_key` followed by
    /// `unregister_global_key`) would leave open if any other code
    /// path mutates the registry between the two calls — a real
    /// risk in Phase 2 testing infrastructure where multiple
    /// parents may rebuild concurrently in test fixtures.
    ///
    /// The caller (the keyed reconciler's middle-walk) consults
    /// this method on an unmatched-by-position keyed view whose
    /// `is_global_key()` is true; on `Some`, it claims the element
    /// for the new parent. Returning `Some` AND removing the entry
    /// in one operation guarantees a second concurrent claim of
    /// the same key sees `None`, not a stale id.
    ///
    /// Re-registering at the new parent is the caller's
    /// responsibility — typically through the standard
    /// [`Self::register_global_key`] path after the element is
    /// re-attached to its new slot.
    pub fn take_global_key_for_reparent(&mut self, key: &dyn ViewKey) -> Option<ElementId> {
        self.global_keys.remove(key)
    }

    /// Drain the duplicate-`GlobalKey` reports the frame boundaries since
    /// the last drain produced.
    ///
    /// A duplicate `GlobalKey` is caller-controlled input, so
    /// [`Self::finalize_tree`] reports it as data rather than panicking:
    /// the offending frame still completes, with the losing parent's
    /// dangling child edge already repaired. Hosts that want a hard failure
    /// can drain this and escalate; tests assert on it directly. The verdict
    /// is reached in every build profile.
    pub fn take_global_key_diagnostics(&mut self) -> Vec<DuplicateGlobalKey> {
        std::mem::take(&mut self.global_key_diagnostics)
    }

    /// The duplicate-`GlobalKey` reports currently pending, without
    /// draining them.
    pub fn global_key_diagnostics(&self) -> &[DuplicateGlobalKey] {
        &self.global_key_diagnostics
    }

    /// Drain this frame's recovered panics — every lifecycle-hook panic a
    /// per-child containment seam caught and recovered from since the last
    /// drain.
    ///
    /// Intended for a presentation host to call after its build segment and
    /// forward into its diagnostic route. Until such a consumer is wired, the
    /// next `WidgetsBinding::draw_frame` entry discards an undrained prior
    /// batch with one aggregate warning. `RecoveredPanic::internal_invariant`
    /// only classifies each entry (a `BUG:`-prefixed panic is still
    /// contained); this drain does not route anything itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_view::BuildOwner;
    ///
    /// let mut owner = BuildOwner::new();
    /// let recovered = owner.take_recovered_panics();
    /// assert!(recovered.is_empty());
    /// ```
    #[must_use = "discarding the drain loses lifecycle-panic diagnostics"]
    pub fn take_recovered_panics(&mut self) -> Vec<RecoveredPanic> {
        std::mem::take(&mut self.recovered_panics)
    }

    /// Discard recovered panics left undrained from the previous frame while
    /// retaining the queue allocation for this frame's records.
    ///
    /// Returns the number discarded so the frame entry point can emit one
    /// aggregate warning rather than one warning per stale record.
    pub(crate) fn discard_stale_recovered_panics(&mut self) -> usize {
        let discarded = self.recovered_panics.len();
        self.recovered_panics.clear();
        discarded
    }

    /// Number of `GlobalKey`s currently registered.
    ///
    /// Test surface — production code reads
    /// [`BuildOwner::element_for_global_key`] on a single key rather
    /// than scanning size. Tests use this to confirm the registry
    /// stays at the expected size across mount / unmount cycles.
    pub fn global_keys_len(&self) -> usize {
        self.global_keys.len()
    }

    /// Check if we're currently building.
    #[cfg(debug_assertions)]
    pub fn is_building(&self) -> bool {
        self.building
    }

    /// Get the current scope depth.
    #[cfg(debug_assertions)]
    pub fn scope_depth(&self) -> usize {
        self.scope_depth
    }
}

impl Drop for BuildOwner {
    /// Reclaim any [`GlobalKeyScope`] claims still tagged to this owner
    /// (ADR-0043). Asserts nothing: an owner dropped with every key already
    /// unregistered through the normal unmount path reclaims silently; any
    /// claim still tagged to it here is traced, not treated as a bug — a
    /// dropped owner can legitimately outlive its own finalize sweep (e.g. a
    /// test harness dropping a `BuildOwner` without ever unmounting its
    /// tree).
    fn drop(&mut self) {
        if let Some(scope) = &self.global_key_scope {
            scope.reclaim_owner(self.owner_tag);
        }
    }
}

impl std::fmt::Debug for BuildOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOwner")
            .field("dirty_count", &self.dirty_count())
            .field("global_keys", &self.global_keys.len())
            .finish_non_exhaustive()
    }
}

/// Guard for build scope (debug only).
#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct BuildScopeGuard<'a> {
    owner: &'a mut BuildOwner,
}

#[cfg(debug_assertions)]
impl Drop for BuildScopeGuard<'_> {
    fn drop(&mut self) {
        self.owner.building = false;
    }
}

/// `detached()` runs third-party observer code from realm setup/teardown —
/// the same containment that guards event emission applies here: a panic is
/// caught and logged, never unwound through the owner.
fn notify_detached(observer: Arc<dyn flui_foundation::observe::TreeObserver>) {
    if let Err(payload) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer.detached()))
    {
        flui_foundation::panic::retain_opaque_payload(payload);
        std::mem::forget(observer);
        let reported = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::error!("TreeObserver::detached() panicked; ignored");
        }));
        if let Err(payload) = reported {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use flui_foundation::panic::payload_text;
    use flui_objects::RenderSizedBox;
    use flui_rendering::{
        pipeline::{PipelineCell, PipelineOwner},
        protocol::BoxProtocol,
    };

    use super::*;
    use crate::{
        BoxedView, ErrorView, LifecycleHook, RecoveredAt, View, ViewExt, element::LayoutBuilder,
        tree::ElementTree,
    };

    /// A render-family leaf view with no child views.
    #[derive(Clone)]
    struct TestView;

    fn wake_debt_is_shared_and_only_a_hooked_scheduler_can_pay_it() {
        let inbox = Arc::new(ExternalBuildInbox::default());
        let wake_panics = Arc::new(AtomicBool::new(true));
        let wake_calls = Arc::new(AtomicUsize::new(0));
        let callback_panics = Arc::clone(&wake_panics);
        let callback_calls = Arc::clone(&wake_calls);
        let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            callback_calls.fetch_add(1, Ordering::Relaxed);
            assert!(!callback_panics.load(Ordering::Relaxed), "wake probe");
        });
        let failing =
            ExternalBuildScheduler::from_parts(Arc::clone(&inbox), Some(Arc::clone(&callback)));
        let no_hook = ExternalBuildScheduler::from_parts(Arc::clone(&inbox), None);
        let recovered = ExternalBuildScheduler::from_parts(Arc::clone(&inbox), Some(callback));
        let element = ElementId::new(1);

        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            failing.schedule(element, RebuildReason::SignalChange);
        }));
        assert!(first.is_err());
        no_hook.schedule(element, RebuildReason::StateChange);

        wake_panics.store(false, Ordering::Relaxed);
        recovered.schedule(element, RebuildReason::DependencyChange);
        recovered.schedule(element, RebuildReason::StateChange);
        assert_eq!(
            wake_calls.load(Ordering::Relaxed),
            2,
            "the second hooked handle pays shared debt exactly once"
        );

        inbox.lock().clear();
        recovered.schedule(element, RebuildReason::StateChange);
        assert_eq!(wake_calls.load(Ordering::Relaxed), 3);
    }

    fn same_id_schedule_racing_a_failed_wake_gets_a_compensating_wake() {
        let inbox = Arc::new(ExternalBuildInbox::default());
        let wake_calls = Arc::new(AtomicUsize::new(0));
        let entered_second_wake = Arc::new(std::sync::Barrier::new(2));
        let release_second_wake = Arc::new(std::sync::Barrier::new(2));
        let callback_calls = Arc::clone(&wake_calls);
        let callback_entered = Arc::clone(&entered_second_wake);
        let callback_release = Arc::clone(&release_second_wake);
        let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let call = callback_calls.fetch_add(1, Ordering::Relaxed) + 1;
            match call {
                1 => panic!("initial wake probe"),
                2 => {
                    callback_entered.wait();
                    callback_release.wait();
                    panic!("racing wake probe");
                }
                3 => {}
                _ => panic!("unexpected duplicate wake"),
            }
        });
        let scheduler = ExternalBuildScheduler::from_parts(inbox, Some(callback));
        let element = ElementId::new(1);

        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scheduler.schedule(element, RebuildReason::SignalChange);
        }));
        assert!(first.is_err());

        let retrying_scheduler = scheduler.clone();
        let retrying = std::thread::spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                retrying_scheduler.schedule(element, RebuildReason::StateChange);
            }))
        });
        entered_second_wake.wait();

        let racing_scheduler = scheduler.clone();
        let racing = std::thread::spawn(move || {
            racing_scheduler.schedule(element, RebuildReason::DependencyChange);
        });
        release_second_wake.wait();
        assert!(retrying.join().expect("retry thread joins").is_err());
        racing.join().expect("racing thread joins");
        assert_eq!(
            wake_calls.load(Ordering::Relaxed),
            3,
            "the same-id schedule racing the failed retry must receive one compensating wake"
        );

        scheduler.schedule(element, RebuildReason::StateChange);
        assert_eq!(
            wake_calls.load(Ordering::Relaxed),
            3,
            "a successful compensating wake clears the shared debt"
        );
    }

    fn reentrant_retry_preserves_the_first_panic_when_the_retry_payload_drop_panics() {
        struct DropBomb;

        impl Drop for DropBomb {
            fn drop(&mut self) {
                panic!("secondary payload drop");
            }
        }

        struct AggregatePayload(DropBomb, DropBomb);

        let inbox = Arc::new(ExternalBuildInbox::default());
        let scheduler_slot: Arc<Mutex<Option<ExternalBuildScheduler>>> = Arc::new(Mutex::new(None));
        let callback_slot = Arc::clone(&scheduler_slot);
        let wake_calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&wake_calls);
        let callback: Arc<dyn Fn() + Send + Sync> =
            Arc::new(
                move || match callback_calls.fetch_add(1, Ordering::Relaxed) + 1 {
                    1 => {
                        let reentrant = callback_slot
                            .lock()
                            .as_ref()
                            .cloned()
                            .expect("scheduler installed");
                        reentrant.schedule(ElementId::new(1), RebuildReason::DependencyChange);
                        panic!("primary wake panic");
                    }
                    2 => std::panic::panic_any(AggregatePayload(DropBomb, DropBomb)),
                    _ => panic!("unexpected extra wake"),
                },
            );
        let scheduler = ExternalBuildScheduler::from_parts(inbox, Some(callback));
        *scheduler_slot.lock() = Some(scheduler.clone());

        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scheduler.schedule(ElementId::new(1), RebuildReason::StateChange);
        }))
        .expect_err("the primary wake panic must resume");
        scheduler_slot.lock().take();

        assert_eq!(
            failure.downcast_ref::<&'static str>(),
            Some(&"primary wake panic")
        );
        assert_eq!(wake_calls.load(Ordering::Relaxed), 2);
    }

    fn direct_build_scheduling_retries_a_failed_wake() {
        let mut owner = BuildOwner::new();
        let wake_panics = Arc::new(AtomicBool::new(true));
        let wake_calls = Arc::new(AtomicUsize::new(0));
        let callback_panics = Arc::clone(&wake_panics);
        let callback_calls = Arc::clone(&wake_calls);
        owner.set_on_build_scheduled(move || {
            callback_calls.fetch_add(1, Ordering::Relaxed);
            assert!(!callback_panics.load(Ordering::Relaxed), "wake probe");
        });
        let element = ElementId::new(1);

        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.schedule_build_for(element, 0, RebuildReason::StateChange);
        }));
        assert!(first.is_err());

        wake_panics.store(false, Ordering::Relaxed);
        owner.schedule_build_for(element, 0, RebuildReason::DependencyChange);
        owner.schedule_build_for(element, 0, RebuildReason::StateChange);
        assert_eq!(wake_calls.load(Ordering::Relaxed), 2);
    }

    impl crate::RenderView for TestView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for TestView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    fn insert_child(
        tree: &mut ElementTree,
        owner: &mut BuildOwner,
        parent: ElementId,
        slot: usize,
    ) -> ElementId {
        tree.insert(&TestView, parent, slot, &mut owner.element_owner_mut())
    }

    fn settle_initial_builds(tree: &mut ElementTree, owner: &mut BuildOwner) {
        owner.build_scope(tree);
    }

    fn nested_scope_quarantines_work_until_its_own_drain() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let root = tree.mount_root(&TestView, &mut owner.element_owner_mut());
        settle_initial_builds(&mut tree, &mut owner);
        let outer = insert_child(&mut tree, &mut owner, root, 0);
        settle_initial_builds(&mut tree, &mut owner);
        let inner = insert_child(&mut tree, &mut owner, outer, 0);
        settle_initial_builds(&mut tree, &mut owner);
        let descendant = insert_child(&mut tree, &mut owner, inner, 0);
        settle_initial_builds(&mut tree, &mut owner);

        let _outer_cell = owner.register_layout_builder_for_test(RenderId::new(51), outer);
        let _inner_cell = owner.register_layout_builder_for_test(RenderId::new(52), inner);
        tree.mark_needs_build(descendant);
        owner.schedule_build_for(descendant, 3, RebuildReason::StateChange);

        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(outer));
        assert!(owner.pending_rebuild_reasons(descendant).is_some());
        assert!(owner.has_pending_layout_scope(inner));

        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(inner));
        assert!(owner.pending_rebuild_reasons(descendant).is_none());
    }

    #[cfg(test)]
    mod lifecycle_recovery_tests {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/build_owner_lifecycle_recovery.rs"
        ));
    }

    /// A keyed stateless view used to drive the REAL GlobalKey retake path
    /// (`try_retake_global_key`) so the reparent updates the moved subtree's
    /// node depths through production code.
    #[derive(Clone)]
    struct KeyedView {
        key: crate::GlobalKey<()>,
    }

    impl crate::StatelessView for KeyedView {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> impl crate::IntoView {
            TestView
        }
    }

    impl View for KeyedView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone)]
    struct KeyedDependencyView {
        key: crate::GlobalKey<()>,
        build_calls: Arc<AtomicUsize>,
        dependency_calls: Arc<AtomicUsize>,
    }

    struct KeyedDependencyState {
        dependency_calls: Arc<AtomicUsize>,
    }

    impl crate::StatefulView for KeyedDependencyView {
        type State = KeyedDependencyState;

        fn create_state(&self) -> Self::State {
            KeyedDependencyState {
                dependency_calls: Arc::clone(&self.dependency_calls),
            }
        }
    }

    impl crate::ViewState<KeyedDependencyView> for KeyedDependencyState {
        fn build(
            &self,
            view: &KeyedDependencyView,
            _ctx: &dyn crate::BuildContext,
        ) -> impl crate::IntoView {
            view.build_calls.fetch_add(1, Ordering::Relaxed);
            TestView
        }

        fn did_change_dependencies(&mut self, _ctx: &dyn crate::LifecycleContext) {
            self.dependency_calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl View for KeyedDependencyView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone)]
    struct ReparentingLayoutHost {
        children: Vec<BoxedView>,
    }

    impl crate::RenderView for ReparentingLayoutHost {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }

        fn has_children(&self) -> bool {
            true
        }

        fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
            for child in &self.children {
                visitor(child);
            }
        }
    }

    impl View for ReparentingLayoutHost {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    fn queued_work_follows_a_live_global_key_reparent_across_build_scopes() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline = PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ));
        let root = tree.mount_root_with_pipeline_owner(
            &TestView,
            Some(pipeline),
            &mut owner.element_owner_mut(),
        );
        settle_initial_builds(&mut tree, &mut owner);
        let scope_a = insert_child(&mut tree, &mut owner, root, 0);
        let scope_b = insert_child(&mut tree, &mut owner, root, 1);
        settle_initial_builds(&mut tree, &mut owner);
        let _scope_a_cell = owner.register_layout_builder_for_test(RenderId::new(81), scope_a);
        let _scope_b_cell = owner.register_layout_builder_for_test(RenderId::new(82), scope_b);

        let build_calls = Arc::new(AtomicUsize::new(0));
        let dependency_calls = Arc::new(AtomicUsize::new(0));
        let keyed = KeyedDependencyView {
            key: crate::GlobalKey::new(),
            build_calls: Arc::clone(&build_calls),
            dependency_calls: Arc::clone(&dependency_calls),
        };
        let moved = tree.insert(&keyed, scope_a, 0, &mut owner.element_owner_mut());
        settle_initial_builds(&mut tree, &mut owner);
        let builds_after_mount = build_calls.load(Ordering::Relaxed);

        tree.mark_needs_build(moved);
        owner.schedule_build_for(moved, 2, RebuildReason::StateChange);
        owner.schedule_build_for(moved, 2, RebuildReason::DependencyChange);
        owner.pending_dependency_changes.insert(moved);
        owner.build_scope(&mut tree);
        assert!(owner.pending_rebuild_reasons(moved).is_some());

        tree.remove(moved, &mut owner.element_owner_mut());
        let migrated = tree.insert(&keyed, scope_b, 0, &mut owner.element_owner_mut());
        assert_eq!(migrated, moved, "GlobalKey reparent preserves identity");
        assert_eq!(
            tree.get(moved).and_then(crate::tree::ElementNode::parent),
            Some(scope_b)
        );

        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(scope_a));
        assert!(
            owner.pending_rebuild_reasons(moved).is_some(),
            "the old scope cannot consume work after the live reparent"
        );
        assert!(owner.pending_dependency_changes.contains(&moved));
        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(scope_b));
        assert!(owner.pending_rebuild_reasons(moved).is_none());
        assert!(!owner.pending_dependency_changes.contains(&moved));
        assert_eq!(dependency_calls.load(Ordering::Relaxed), 1);
        assert_eq!(build_calls.load(Ordering::Relaxed), builds_after_mount + 1);

        tree.mark_needs_build(moved);
        owner.schedule_build_for(moved, 2, RebuildReason::StateChange);
        owner.schedule_build_for(moved, 2, RebuildReason::DependencyChange);
        owner.pending_dependency_changes.insert(moved);
        owner.build_scope(&mut tree);
        assert!(owner.pending_rebuild_reasons(moved).is_some());

        tree.remove(moved, &mut owner.element_owner_mut());
        let migrated_global = tree.insert(&keyed, root, 2, &mut owner.element_owner_mut());
        assert_eq!(migrated_global, moved);
        assert_eq!(
            tree.get(moved).and_then(crate::tree::ElementNode::parent),
            Some(root)
        );
        owner.build_scope(&mut tree);

        assert!(owner.pending_rebuild_reasons(moved).is_none());
        assert!(!owner.pending_dependency_changes.contains(&moved));
        assert_eq!(dependency_calls.load(Ordering::Relaxed), 2);
        assert_eq!(build_calls.load(Ordering::Relaxed), builds_after_mount + 2);
    }

    // ========================================================================
    // GlobalKeyScope — cross-owner uniqueness (ADR-0043)
    //
    // Every test below drives two (or three) fully standalone `BuildOwner` +
    // `ElementTree` pairs sharing one `GlobalKeyScope`, going through the
    // real `mount_root`/`insert`/`remove`/`finalize_tree` production paths —
    // no direct scope manipulation except where a test explicitly says it is
    // forcing the hazard the realm execution contract rules out. The unit
    // tests in `global_key_scope.rs` cover the mechanism (claim/release/
    // reclaim) in isolation; these cover it wired through real mounts.
    // ========================================================================

    /// Extract a panic payload's message, whether the panic macro produced a
    /// `&'static str` or an interpolated `String` — the conflict panic in
    /// `global_key_scope::claim_and_register` uses the latter.
    fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
        payload_text(payload)
            .map_or_else(|| String::from("<non-string panic payload>"), str::to_owned)
    }

    /// A `GlobalKey` mounted in one owner while a second owner sharing the
    /// same scope still holds it must fail eagerly (traced, then panicked),
    /// and the failed attempt must leave the first owner's tree completely
    /// undisturbed — not partially unmounted, not re-tagged.
    fn duplicate_global_key_across_owners_sharing_a_scope_fails_eagerly_and_leaves_first_tree_undisturbed()
     {
        let scope = GlobalKeyScope::new();
        let keyed = KeyedView {
            key: crate::GlobalKey::new(),
        };

        let mut tree_a = ElementTree::new();
        let mut owner_a = BuildOwner::new();
        owner_a.set_global_key_scope(scope.clone());
        let root_a = tree_a.mount_root(&keyed, &mut owner_a.element_owner_mut());
        assert_eq!(tree_a.len(), 1);
        let tag_a = owner_a.owner_tag();

        let mut tree_b = ElementTree::new();
        let mut owner_b = BuildOwner::new();
        owner_b.set_global_key_scope(scope);
        let tag_b = owner_b.owner_tag();

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree_b.mount_root(&keyed, &mut owner_b.element_owner_mut());
        }));
        let payload = outcome.expect_err("b must not silently alias a's live GlobalKey");
        let message = panic_message(&*payload);
        assert!(
            message.contains(&tag_a.to_string()),
            "panic must name a's tag (the claim's holder): {message}"
        );
        assert!(
            message.contains(&tag_b.to_string()),
            "panic must name b's tag (the rejected owner): {message}"
        );

        // a's tree is exactly as it was before b's rejected mount attempt.
        assert_eq!(tree_a.len(), 1);
        assert_eq!(owner_a.element_for_global_key(&keyed.key), Some(root_a));
    }

    /// Once a fully releases a key (unmount + finalize, not merely a
    /// soft-remove into the inactive queue), a second owner sharing the
    /// scope can mount the same key fresh. This is an ordinary new mount,
    /// not a retake: b's own local registry never had an entry for this
    /// hash, so `try_retake_global_key` finds no candidate and creates a
    /// genuinely new element — nothing to carry state over from even in
    /// principle.
    fn mount_in_b_after_a_finalizes_is_fresh_mount_no_state_carryover() {
        let scope = GlobalKeyScope::new();
        let keyed = KeyedView {
            key: crate::GlobalKey::new(),
        };
        let key = keyed.key.clone();

        let mut tree_a = ElementTree::new();
        let mut owner_a = BuildOwner::new();
        owner_a.set_global_key_scope(scope.clone());
        let root_a = tree_a.mount_root(&TestView, &mut owner_a.element_owner_mut());
        let child_a = tree_a.insert(&keyed, root_a, 0, &mut owner_a.element_owner_mut());

        // Full release: finalize runs inside a's own segment, exactly the
        // discipline the scope's claim-lifetime contract assumes.
        tree_a.remove(child_a, &mut owner_a.element_owner_mut());
        owner_a.finalize_tree(&mut tree_a);
        assert_eq!(
            owner_a.element_for_global_key(&key),
            None,
            "a released the key locally, not just in the shared scope"
        );

        let mut tree_b = ElementTree::new();
        let mut owner_b = BuildOwner::new();
        owner_b.set_global_key_scope(scope);

        assert_eq!(owner_b.element_for_global_key(&key), None);
        let root_b = tree_b.mount_root(&keyed, &mut owner_b.element_owner_mut());
        assert_eq!(tree_b.len(), 1);
        assert_eq!(owner_b.element_for_global_key(&key), Some(root_b));
    }

    // ========================================================================
    // Issue #1180: mid-drain external-build absorption
    // ========================================================================

    /// A render-family leaf, identical to `TestView` in every other respect,
    /// that opts into [`View::should_skip_rebuild`]. Every fixture below that
    /// returns a leaf from its own `build()` uses THIS type, not the shared
    /// `TestView` every other test in this module also mounts: without the
    /// skip, a parent rebuilding and re-declaring the same leaf every time
    /// would reschedule that hidden leaf too (and re-fire `on_build_scheduled`)
    /// on every one of the PARENT's own rebuilds — noise unrelated to
    /// whatever a #1180 test is actually driving. Scoped to this dedicated
    /// type, rather than added to `TestView` itself, because `TestView` is
    /// also the leaf ~50 pre-existing tests elsewhere in this module mount:
    /// putting the override there would silently change what code path those
    /// tests exercise (a config-changed child skipping its own reconcile
    /// instead of going through it) even though none of their assertions
    /// would break — a coverage change nothing would catch. The fixtures that
    /// mount it tolerate the extra wake noise without asserting on it.
    #[derive(Clone)]
    struct MidDrainStableLeaf;

    impl crate::RenderView for MidDrainStableLeaf {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for MidDrainStableLeaf {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }

        fn should_skip_rebuild(&self, _prev: &Self) -> bool
        where
            Self: Sized,
        {
            true
        }
    }

    /// A leaf that captures its own handle and, while `should_reschedule` is
    /// set, schedules ITSELF on every build — the self-rescheduler the
    /// mid-drain absorb budget exists for.
    #[derive(Clone)]
    struct MidDrainSelfRescheduler {
        build_calls: Arc<AtomicUsize>,
        should_reschedule: Arc<std::sync::atomic::AtomicBool>,
        handle_slot: Arc<Mutex<Option<crate::RebuildHandle>>>,
    }

    struct MidDrainSelfReschedulerState {
        build_calls: Arc<AtomicUsize>,
        should_reschedule: Arc<std::sync::atomic::AtomicBool>,
        handle_slot: Arc<Mutex<Option<crate::RebuildHandle>>>,
    }

    impl crate::StatefulView for MidDrainSelfRescheduler {
        type State = MidDrainSelfReschedulerState;

        fn create_state(&self) -> Self::State {
            MidDrainSelfReschedulerState {
                build_calls: Arc::clone(&self.build_calls),
                should_reschedule: Arc::clone(&self.should_reschedule),
                handle_slot: Arc::clone(&self.handle_slot),
            }
        }
    }

    impl crate::ViewState<MidDrainSelfRescheduler> for MidDrainSelfReschedulerState {
        fn init_state(&mut self, ctx: &dyn crate::LifecycleContext) {
            let _prev = self.handle_slot.lock().replace(ctx.rebuild_handle());
        }

        fn build(
            &self,
            _view: &MidDrainSelfRescheduler,
            _ctx: &dyn crate::BuildContext,
        ) -> impl crate::IntoView {
            self.build_calls.fetch_add(1, Ordering::Relaxed);
            if self.should_reschedule.load(Ordering::Relaxed)
                && let Some(handle) = self.handle_slot.lock().clone()
            {
                handle.schedule(RebuildReason::StateChange);
            }
            MidDrainStableLeaf
        }
    }

    impl View for MidDrainSelfRescheduler {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }
    }

    /// The tree's own root, panicking from `did_change_dependencies` when
    /// armed. Used ONLY as the panic source for the `capped_leftover`
    /// unwind-safety regression below: root is the one element
    /// `drain_build_scope`'s build-hook catch can never hand to
    /// `replace_failed_lifecycle_element` (that recovery needs a parent to
    /// re-point), so a panic here is the one legitimate way to reach the
    /// catch's bare, unconditional `std::panic::resume_unwind` arm through
    /// ordinary view code rather than through a framework-internal invariant
    /// violation.
    #[derive(Clone)]
    struct MidDrainRootPanic {
        trigger_panic: Arc<std::sync::atomic::AtomicBool>,
    }

    struct MidDrainRootPanicState {
        trigger_panic: Arc<std::sync::atomic::AtomicBool>,
    }

    impl crate::StatefulView for MidDrainRootPanic {
        type State = MidDrainRootPanicState;

        fn create_state(&self) -> Self::State {
            MidDrainRootPanicState {
                trigger_panic: Arc::clone(&self.trigger_panic),
            }
        }
    }

    impl crate::ViewState<MidDrainRootPanic> for MidDrainRootPanicState {
        fn did_change_dependencies(&mut self, _ctx: &dyn crate::LifecycleContext) {
            assert!(
                !self.trigger_panic.load(Ordering::Relaxed),
                "MidDrainRootPanic: deliberate did_change_dependencies panic"
            );
        }

        fn build(
            &self,
            _view: &MidDrainRootPanic,
            _ctx: &dyn crate::BuildContext,
        ) -> impl crate::IntoView {
            MidDrainStableLeaf
        }
    }

    impl View for MidDrainRootPanic {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
        }
    }

    /// Regression test for the repair-round finding that `capped_leftover`
    /// was flushed only textually after `drain_build_scope`'s loop: several
    /// arms inside that loop restore heap/dirty-reasons entries and then
    /// `std::panic::resume_unwind`, which unwinds the function's own stack
    /// frame before a LATER flush placed after the loop ever runs.
    ///
    /// Pre-arms the exact capping precondition directly on `owner`'s
    /// `pub(crate)` fields — a `victim` id already recorded in
    /// `built_this_frame` with a fresh landing sitting in the inbox and the
    /// budget already at zero — rather than grinding through
    /// `MAX_MID_DRAIN_ABSORBS` real re-entries to reach it. This makes the
    /// cap fire on the drain's very FIRST absorb call, before its first pop;
    /// `root` (the only thing actually scheduled onto the heap) then builds
    /// and panics from `did_change_dependencies` — the one hook whose panic
    /// both reaches `drain_build_scope`'s own catch (unlike `build()`, which
    /// `build_or_recover` absorbs, and unlike a fresh child's `init_state`,
    /// which is converted to a `Result` before it ever unwinds that far) AND
    /// takes its bare, unconditional `resume_unwind` arm rather than the
    /// recoverable one (since root has no parent to substitute).
    ///
    /// Calls `build_scope_impl` directly, not `build_scope`: the public
    /// entry point resets `mid_drain_absorbs_left`/`built_this_frame` at
    /// every call, which would erase the precondition this test just armed.
    fn mid_drain_panic_after_a_cap_still_flushes_the_capped_id_to_the_inbox() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let trigger_panic = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let root = tree.mount_root(
            &MidDrainRootPanic {
                trigger_panic: Arc::clone(&trigger_panic),
            },
            &mut owner.element_owner_mut(),
        );
        settle_initial_builds(&mut tree, &mut owner);

        // An otherwise-irrelevant already-mounted element stands in for "the
        // id this drain already built once and capped a re-entry for" —
        // only its presence in `built_this_frame` and the inbox matters,
        // not anything about its own view. The three writes below fake the
        // invariant a real frame establishes (`victim ∈ built_this_frame` ⇔
        // it completed a build this `build_scope`; budget exhausted; a fresh
        // schedule for it pending) so the test reaches the capped path
        // without grinding through sixteen genuine re-entries.
        let victim = insert_child(&mut tree, &mut owner, root, 0);
        owner.built_this_frame.insert(victim);
        owner.mid_drain_absorbs_left = 0;
        owner.external_inbox.lock().insert(
            victim,
            RebuildReasons::from_reason(RebuildReason::StateChange),
        );

        tree.mark_needs_build(root);
        owner.schedule_build_for(root, 0, RebuildReason::DependencyChange);
        owner.pending_dependency_changes.insert(root);
        trigger_panic.store(true, Ordering::Relaxed);

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.build_scope_impl(&mut tree);
        }));
        trigger_panic.store(false, Ordering::Relaxed);

        assert!(
            outcome.is_err(),
            "root's did_change_dependencies must panic and propagate out of build_scope_impl \
             — if this assertion fails, the fixture stopped exercising the bare \
             resume_unwind arm this test targets"
        );
        assert_eq!(
            owner.pending_external_builds(),
            1,
            "the pre-armed capped id must still reach the inbox even though root's build \
             panicked in the SAME drain — this is what CappedLeftoverGuard's Drop fixes; \
             before it, the panic unwound past the old post-loop flush and the capped id \
             vanished (0 here)"
        );
    }

    fn mid_drain_absorb_budget_caps_a_self_rescheduler_and_rearms_after_a_clean_frame() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let build_calls = Arc::new(AtomicUsize::new(0));
        let should_reschedule = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handle_slot = Arc::new(Mutex::new(None));

        let root = tree.mount_root(
            &MidDrainSelfRescheduler {
                build_calls: Arc::clone(&build_calls),
                should_reschedule: Arc::clone(&should_reschedule),
                handle_slot: Arc::clone(&handle_slot),
            },
            &mut owner.element_owner_mut(),
        );
        owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
        owner.build_scope(&mut tree); // build #0 (mount) — captures the handle
        let after_mount = build_calls.load(Ordering::Relaxed);
        assert_eq!(after_mount, 1);

        // Frame 1: reschedule itself from every build until the budget caps it.
        should_reschedule.store(true, Ordering::Relaxed);
        tree.mark_needs_build(root);
        owner.schedule_build_for(root, 0, RebuildReason::StateChange);
        let ((), log) = flui_testing::log_capture::capture(|| owner.build_scope(&mut tree));

        assert_eq!(
            build_calls.load(Ordering::Relaxed) - after_mount,
            1 + MAX_MID_DRAIN_ABSORBS,
            "one direct build plus MAX_MID_DRAIN_ABSORBS re-entries, then the cap stops \
             the drain"
        );
        assert_eq!(
            owner.pending_external_builds(),
            1,
            "the capped self-reschedule stays queued, not lost"
        );
        assert!(
            owner.has_dirty_elements(),
            "the leftover schedule is real dirty work the next frame must serve"
        );
        assert_eq!(
            log.count_containing("mid-drain absorb budget exhausted"),
            1,
            "exactly one warn for this capped streak; log:\n{log}"
        );

        // Frame 2: stop rescheduling — a clean frame serves the leftover for free.
        should_reschedule.store(false, Ordering::Relaxed);
        let before_frame_2 = build_calls.load(Ordering::Relaxed);
        owner.build_scope(&mut tree);
        assert_eq!(
            build_calls.load(Ordering::Relaxed),
            before_frame_2 + 1,
            "the next build_scope serves the leftover id for free (a fresh frame)"
        );
        assert_eq!(owner.pending_external_builds(), 0);
        assert!(!owner.has_dirty_elements());

        // Frame 3: cap again — proves the streak re-armed after frame 2's clean run.
        should_reschedule.store(true, Ordering::Relaxed);
        tree.mark_needs_build(root);
        owner.schedule_build_for(root, 0, RebuildReason::StateChange);
        let ((), log2) = flui_testing::log_capture::capture(|| owner.build_scope(&mut tree));
        assert_eq!(
            log2.count_containing("mid-drain absorb budget exhausted"),
            1,
            "the warn must re-arm after a clean frame, not stay silenced forever; log:\n{log2}"
        );

        // Cleanup: leave the owner in a clean state.
        should_reschedule.store(false, Ordering::Relaxed);
        owner.build_scope(&mut tree);
    }

    /// A refused admission leaves the caller's focus manager, even its last
    /// strong reference, open with its nodes still focusable.
    fn exhausted_owner_tag_refusal_retains_the_supplied_focus_manager() {
        use flui_interaction::routing::FocusNode;
        let manager = FocusManager::new();
        let node = FocusNode::with_debug_label("survives-refusal");
        let _attachment = manager
            .root_scope()
            .attach_node(&node)
            .expect("a fresh node attaches under the root scope");
        let weak = Rc::downgrade(&manager);
        let exhausted = std::sync::atomic::AtomicU64::new(0);
        let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            BuildOwner::admit(manager, || OwnerTag::fresh_with_counter(&exhausted))
        }));
        assert!(refusal.is_err(), "an exhausted counter refuses admission");
        let manager = weak
            .upgrade()
            .expect("the refusal retained the supplied manager");
        assert!(!manager.is_closed(), "the refusal closed the focus tree");
        assert!(node.is_attached());
        let _ = node.request_focus();
        assert!(
            manager
                .primary_focus()
                .is_some_and(|focused| Rc::ptr_eq(&focused, &node)),
            "the node stayed focusable under its manager"
        );
    }

    #[test]
    fn build_owner_scheduling_matrix() {
        crate::table_test::run_table(
            "build_owner_scheduling_matrix",
            &[
                ("exhausted_owner_tag_counter_preserves_claim_authority", OwnerTag::exhausted_owner_tag_counter_preserves_claim_authority as fn()),
                ("exhausted_owner_tag_refusal_retains_the_supplied_focus_manager", exhausted_owner_tag_refusal_retains_the_supplied_focus_manager as fn()),
                ("wake_debt_is_shared_and_only_a_hooked_scheduler_can_pay_it", wake_debt_is_shared_and_only_a_hooked_scheduler_can_pay_it as fn()),
                ("same_id_schedule_racing_a_failed_wake_gets_a_compensating_wake", same_id_schedule_racing_a_failed_wake_gets_a_compensating_wake as fn()),
                ("reentrant_retry_preserves_the_first_panic_when_the_retry_payload_drop_panics", reentrant_retry_preserves_the_first_panic_when_the_retry_payload_drop_panics as fn()),
                ("direct_build_scheduling_retries_a_failed_wake", direct_build_scheduling_retries_a_failed_wake as fn()),
                ("nested_scope_quarantines_work_until_its_own_drain", nested_scope_quarantines_work_until_its_own_drain as fn()),
                ("queued_work_follows_a_live_global_key_reparent_across_build_scopes", queued_work_follows_a_live_global_key_reparent_across_build_scopes as fn()),
                ("duplicate_global_key_across_owners_sharing_a_scope_fails_eagerly_and_leaves_first_tree_undisturbed", duplicate_global_key_across_owners_sharing_a_scope_fails_eagerly_and_leaves_first_tree_undisturbed as fn()),
                ("mount_in_b_after_a_finalizes_is_fresh_mount_no_state_carryover", mount_in_b_after_a_finalizes_is_fresh_mount_no_state_carryover as fn()),
                ("mid_drain_panic_after_a_cap_still_flushes_the_capped_id_to_the_inbox", mid_drain_panic_after_a_cap_still_flushes_the_capped_id_to_the_inbox as fn()),
                ("mid_drain_absorb_budget_caps_a_self_rescheduler_and_rearms_after_a_clean_frame", mid_drain_absorb_budget_caps_a_self_rescheduler_and_rearms_after_a_clean_frame as fn()),
            ],
        );
    }
}
