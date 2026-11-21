//! BuildOwner - Manages the build phase.
//!
//! The BuildOwner is responsible for:
//! - Tracking dirty elements that need rebuilding
//! - Processing rebuilds in depth-first order
//! - Managing GlobalKey registry
//! - Coordinating InheritedElement lookups
//!
//! The owner's behaviour is split by concept across the child modules: the
//! build pass (`build_pass`), the dirty queue's layout-builder partition
//! (`layout_scopes`), lazy-sliver child servicing (`child_requests`),
//! inactive elements and finalize (`inactive_elements`), the `GlobalKey`
//! registry (`global_keys`), installed presentation handles
//! (`presentation_handles`) and the per-frame report (`build_report`).

use rustc_hash::FxHashMap;
use std::collections::BinaryHeap;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::panic::FirstPanic;
use flui_foundation::{ElementId, RebuildReasons, SlotSet};
use flui_interaction::focus::FocusManager;

use super::dirty_queue::DirtyQueue;
use crate::{
    element::child_manager::ChildManagerRegistry,
    owner::{
        DuplicateGlobalKey, GlobalKeyRegistry, GlobalKeyReservations, LifecyclePanicHandoff,
        RebuildReason, RecoveredPanic, global_key_scope,
        global_key_scope::{GlobalKeyScope, OwnerTag},
        inherited_dependencies::InheritedDependencies,
        layout_builder::LayoutBuilderRegistry,
    },
};

use super::external_build_inbox::BuildPremise;
pub(crate) use super::external_build_inbox::ExternalBuildInbox;
use super::presentation_capabilities::PresentationCapabilities;

mod build_pass;
mod build_report;
mod build_scope_guard;
mod child_requests;
mod external_scheduler;
mod global_keys;
mod inactive_elements;
mod layout_scopes;
mod presentation_handles;

use build_pass::MAX_MID_DRAIN_ABSORBS;
use build_report::FrameBuildCounts;
pub use build_report::FrameBuildReport;
pub(crate) use external_scheduler::ExternalBuildScheduler;
pub(crate) use inactive_elements::InactiveElement;
pub(crate) use layout_scopes::{BuildScopeQueues, BuildScopeTarget};

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
    pub(crate) dirty_elements: DirtyQueue,

    /// Accumulated rebuild causes for every id present in `dirty_elements`.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) dirty_reasons: FxHashMap<ElementId, RebuildReasons>,

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
    pub(crate) pending_dependency_changes: SlotSet<ElementId>,

    /// Sparse dependent -> providers ownership index.
    ///
    /// Provider elements keep the forward notification map. This reverse index
    /// is the lifecycle authority used to remove an element from every
    /// provider on deactivate/unmount without adding a collection to every
    /// [`ElementNode`](crate::tree::ElementNode).
    pub(crate) inherited_dependencies: InheritedDependencies,
    /// ADR-0074: the UI runtime's reactive graph. Constructed with the owner,
    /// re-pointed at the external inbox whenever the frame-request callback
    /// changes (`set_on_build_scheduled`).
    reactive: crate::reactive::Reactive,

    /// Keep-alive holds on lazy sliver children — which children band eviction
    /// must skip. Presentation-scoped like every other lifecycle capability,
    /// and cheap to clone into the split-borrow `ElementOwner`.
    pub(crate) keep_alive: super::KeepAliveHolds,

    /// The hooks states registered to run when their elements unmount
    /// (`LifecycleContext::on_unmount`).
    pub(crate) unmount_hooks: super::UnmountHooks,

    /// The UI runtime's tree-observer slot (ADR-0040). `None` = observation off
    /// (one branch per emission site). `pub(crate)` for the
    /// [`ElementOwner`](super::ElementOwner) split-borrow.
    pub(crate) tree_observer: Option<Arc<dyn flui_foundation::observe::TreeObserver>>,

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
    pub(crate) on_build_scheduled: Option<Arc<dyn Fn() + Send + Sync>>,

    /// Inbox of element ids and causes scheduled through an
    /// `ExternalBuildScheduler` handle — typically an animation/listenable
    /// tick firing *outside* any frame (no `&mut BuildOwner` in scope), but
    /// the identical route a `build` running *inside* a drain takes when it
    /// calls another element's [`RebuildHandle`](super::RebuildHandle)
    /// synchronously. The map deduplicates element ids while retaining
    /// distinct causes. Absorbed onto [`Self::dirty_elements`] (or a
    /// layout-builder scope bucket) at the top of every heap pop in
    /// [`Self::drain_build_scope`] — not only once at frame start — where
    /// each id's tree depth is looked up. Shared (`Arc`) so
    /// the listener callbacks and the owner reference the same queue.
    pub(crate) external_inbox: Arc<ExternalBuildInbox>,

    /// The kept buffer `absorb_mid_drain_inbox` moves the inbox's requests
    /// into, so a steady frame's drain allocates nothing.
    landed_builds: Vec<(ElementId, RebuildReasons)>,

    /// Remaining RE-ENTRY mid-drain absorb budget for the current frame
    /// ([`MAX_MID_DRAIN_ABSORBS`]). Reset at every
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
    pub(crate) built_this_frame: SlotSet<ElementId>,
    /// How many elements this frame's drain rebuilt for each cause — the
    /// per-frame telemetry ADR-0074 §8 measures against (`FrameStats` has no
    /// rebuild figure). Reset with `built_this_frame` at every `build_scope`.
    /// Boxed so the owner grows by one pointer, not by a counter table
    /// (`build_owner_tests::test_build_owner_memory_size` budgets this struct).
    frame_builds: Box<FrameBuildCounts>,

    /// Registry of live lazy-sliver [`ChildManager`](crate::element::child_manager::ChildManager)s, one per live adaptor
    /// element. Keyed by the sliver's `RenderId`; populated at mount and
    /// cleared at unmount by `SliverAdaptorBehavior` via the
    /// `ElementOwner::register_child_manager` / `unregister_child_manager`
    /// split-borrow methods.
    ///
    /// A `RefCell` (not a plain `FxHashMap`) so `ElementOwner` can carry a
    /// shared `&'a` reference and still register and unregister managers.
    /// `service_child_requests` clones the manager `Rc`s out of it before
    /// calling service, releasing the registry borrow before the potentially
    /// long service call.
    pub(crate) child_manager_registry: ChildManagerRegistry,

    /// The build phase's scratch lists (ADR-0328 §3), reset as the render-edge sync ends.
    frame_arena: flui_foundation::arena::FrameArena,

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
    window_handle: Option<crate::WindowHandle>,

    /// The binding's frame-driven async task driver, installed by
    /// whichever binding owns this owner. `None` until then — a tree built with
    /// no binding cannot spawn tasks, and `LifecycleContext::async_driver` reports
    /// that honestly rather than silently spawning into a driver nobody polls.
    ///
    /// This must be the driver the binding's frame step actually polls:
    /// each UI runtime, a headless host's included, drives its own owned
    /// `UpdateScheduler` (`UiRuntime.scheduler`). Reaching
    /// for the wrong one from a widget would make headless tests spawn into a
    /// driver that never runs.
    pub(crate) async_driver: Option<flui_scheduler::AsyncDriver>,

    /// The binding's post-frame capability. `None` when no binding
    /// installed one, which makes `LifecycleContext::post_frame_handle` report the
    /// absence rather than silently scheduling onto a global.
    pub(crate) post_frame_handle: Option<flui_scheduler::PostFrameHandle>,

    /// The binding's OWNER-LOCAL post-frame capability — see
    /// `LifecycleContext::post_frame_handle`'s doc for why this is a
    /// separate handle rather than a second field on `PostFrameHandle`
    /// itself. `None` under the same conditions as `post_frame_handle`.

    /// The presentation capabilities its UI runtime installed; the `Default`
    /// (all absent) on a bare owner.
    pub(crate) capabilities: PresentationCapabilities,

    /// The host's flush registry, which `Persisted` publishes into through
    /// the crate-private `LifecycleContext::flush_registry_in_crate`. `None`
    /// on a bare owner and under a host without storage.
    pub(crate) flush_registry: Option<crate::flush_registry::FlushRegistry>,

    /// The binding's owner-local interaction dispatch capability (ADR-0027).
    ///
    /// `None` means the owner was built detached from a runtime interaction lane;
    /// render-object lifecycle contexts report that as a typed inactive ui_runtime.
    pub(crate) interaction_dispatch: Option<flui_interaction::pointer::InteractionDispatchHandle>,
    /// Whether the presentation collects its semantics tree: what exists
    /// only for a collected tree (a `Semantics` action route) is kept while
    /// this holds. `true` until a runtime says otherwise, so a tree mounted
    /// outside one is treated as collected.
    pub(crate) collects_semantics: bool,
    pub(crate) hit_test_handle: Option<flui_interaction::pointer::HitTestHandle>,

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
            dirty_elements: DirtyQueue::default(),
            dirty_reasons: FxHashMap::default(),
            global_keys: GlobalKeyRegistry::new(),
            global_key_reservations: Box::new(GlobalKeyReservations::new()),
            global_key_diagnostics: Vec::new(),
            inactive_elements: Vec::new(),
            pending_dependency_changes: SlotSet::new(),
            inherited_dependencies: InheritedDependencies::default(),
            reactive: crate::reactive::Reactive::new(),
            keep_alive: super::KeepAliveHolds::default(),
            unmount_hooks: super::UnmountHooks::default(),
            tree_observer: None,
            recovered_panics: Vec::new(),
            lifecycle_panic_handoff: Cell::new(LifecyclePanicHandoff::Disarmed),
            #[cfg(debug_assertions)]
            building: false,
            #[cfg(debug_assertions)]
            scope_depth: 0,
            on_build_scheduled: None,
            external_inbox: Arc::new(ExternalBuildInbox::default()),
            landed_builds: Vec::new(),
            mid_drain_absorbs_left: MAX_MID_DRAIN_ABSORBS,
            mid_drain_cap_streak: false,
            built_this_frame: SlotSet::new(),
            frame_builds: Box::default(),
            child_manager_registry: RefCell::new(FxHashMap::default()),
            frame_arena: flui_foundation::arena::FrameArena::new(),
            layout_builder_registry: LayoutBuilderRegistry::default(),
            lazy_band_pass_budget: super::layout_builder::MAX_LAZY_BAND_PASSES,
            build_scope_queues: None,
            focus_manager,
            lifecycle_handle: None,
            window_handle: None,
            async_driver: None,
            post_frame_handle: None,
            capabilities: PresentationCapabilities::default(),
            flush_registry: None,
            interaction_dispatch: None,
            collects_semantics: true,
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

    /// Schedule an element for rebuild.
    ///
    /// Elements are processed in depth order (shallowest first) so parent
    /// rebuilds happen before child rebuilds. The `depth` is a best-effort
    /// ordering hint: [`build_scope`](Self::build_scope) re-derives every
    /// queued element's authoritative tree depth from its node before draining
    /// (see `rekey_dirty_depths`), so a caller that
    /// only knows the sibling slot index cannot mis-order the drain.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize, reason: RebuildReason) {
        self.external_inbox.note_local_admission();
        let newly_queued = match self.dirty_reasons.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(RebuildReasons::from_reason(reason));
                self.dirty_elements.push(id, depth);
                true
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().insert(reason);
                false
            }
        };
        self.external_inbox
            .request_frame_contained(newly_queued, self.on_build_scheduled.as_deref());
    }

    /// Capture accepted local and external build input for a suspended segment.
    /// Queue drain, requeue and capped-work transport preserve this premise.
    #[doc(hidden)]
    pub fn build_premise(&self) -> BuildPremise {
        self.external_inbox.premise()
    }

    /// Whether this owner still accepts the captured build-input premise.
    /// Closed inboxes and exhausted stamps permanently refuse compatibility.
    #[doc(hidden)]
    #[must_use]
    pub fn accepts_build_premise(&self, saved: &BuildPremise) -> bool {
        self.external_inbox.accepts_premise(saved)
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
            unmount_hooks: self.unmount_hooks.clone(),
            frame_wake: super::element_owner::FrameWake::Now(self.on_build_scheduled.as_deref()),
            external_inbox: &self.external_inbox,
            external_request_frame: self.on_build_scheduled.as_ref(),
            // Lifecycle paths (mount/unmount/update) get no live-tree view;
            // only the `build_scope` drain sets `build_view`.
            build_view: None,
            child_manager_registry: &self.child_manager_registry,
            layout_builder_registry: &self.layout_builder_registry,
            focus_manager: &self.focus_manager,
            lifecycle_handle: &self.lifecycle_handle,
            window_handle: &self.window_handle,
            async_driver: &self.async_driver,
            post_frame_handle: &self.post_frame_handle,
            capabilities: &self.capabilities,
            flush_registry: &self.flush_registry,
            interaction_dispatch: &self.interaction_dispatch,
            collects_semantics: self.collects_semantics,
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
        let external = self.external_inbox.lock().get(element);
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
    /// use flui_view::__runtime::BuildOwner;
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
    ///
    /// Then ends the installed tree observer's stream with `detached()`,
    /// once: an observer already cleared or replaced was told then. It is
    /// called through its own `Arc`, with nothing of this owner borrowed,
    /// and the slot keeps the observer until the fields drop, so its
    /// destructor runs in field order. During an unwind `detached()` is not
    /// called; the observer is still destroyed with the slot (ADR-0281).
    fn drop(&mut self) {
        if let Some(scope) = &self.global_key_scope {
            scope.reclaim_owner(self.owner_tag);
        }
        if !std::thread::panicking()
            && let Some(observer) = self.tree_observer.as_ref()
        {
            notify_detached(Arc::clone(observer));
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

/// `detached()` runs third-party observer code from UI runtime setup/teardown —
/// the same containment that guards event emission applies here: a panic is
/// caught and logged, never unwound through the owner.
fn notify_detached(observer: Arc<dyn flui_foundation::observe::TreeObserver>) {
    let mut caught = FirstPanic::new();
    if caught.contain(|| observer.detached()).is_none() {
        // Each collector destroys its payload under containment.
        drop(caught);
        crate::drop_contained(observer);
        FirstPanic::new().contain(|| {
            tracing::error!("TreeObserver::detached() panicked; ignored");
        });
    }
}

/// Raise the panic a contained step left in `caught`, for a seam that cannot
/// go on once the step failed.
fn raise(caught: FirstPanic) -> ! {
    std::panic::resume_unwind(
        caught
            .into_payload()
            .expect("BUG: a contained step that returned nothing holds its panic"),
    )
}

#[cfg(test)]
mod tests {
    use flui_foundation::RenderId;
    use std::any::TypeId;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use flui_foundation::panic::payload_text;
    use flui_objects::RenderSizedBox;
    use flui_rendering::{
        pipeline::{PipelineCell, PipelineOwner},
        protocol::BoxProtocol,
    };
    use parking_lot::Mutex;

    use super::*;
    use crate::{
        BoxedView, ErrorView, View, ViewExt,
        element::LayoutBuilder,
        owner::{LifecycleHook, RecoveredAt},
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

        // The failed wake is reported, not unwound into the scheduling code.
        failing.schedule(element, RebuildReason::SignalChange);
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

        // The failed wake is reported, not unwound into the scheduling code.
        scheduler.schedule(element, RebuildReason::SignalChange);

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
        assert!(
            retrying.join().expect("retry thread joins").is_ok(),
            "a failed retry is reported, not unwound into the scheduling code"
        );
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

    fn reentrant_retry_preserves_the_first_panic() {
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
                    2 => panic!("retry wake panic"),
                    _ => panic!("unexpected extra wake"),
                },
            );
        let scheduler =
            ExternalBuildScheduler::from_parts(Arc::clone(&inbox), Some(Arc::clone(&callback)));
        *scheduler_slot.lock() = Some(scheduler);

        // The wake itself, as a scheduler requests it before containing it:
        // the first panic resumes and the retry's panic is dropped.
        let fresh = inbox.admit_batch(
            &[ElementId::new(1)],
            RebuildReasons::from_reason(RebuildReason::StateChange),
        );
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inbox.request_frame_if_needed(fresh, Some(&*callback));
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

        // The failed wake is reported, not unwound into the scheduling code.
        owner.schedule_build_for(element, 0, RebuildReason::StateChange);

        wake_panics.store(false, Ordering::Relaxed);
        owner.schedule_build_for(element, 0, RebuildReason::DependencyChange);
        owner.schedule_build_for(element, 0, RebuildReason::StateChange);
        assert_eq!(wake_calls.load(Ordering::Relaxed), 2);
    }

    /// A log subscriber that panics on every owner event, as a faulty one does.
    struct PanicsOnOwnerEvent;

    impl tracing::Subscriber for PanicsOnOwnerEvent {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            assert!(
                !event.metadata().target().starts_with("flui_view::owner"),
                "log subscriber failed"
            );
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    fn a_panicking_report_of_a_failed_wake_does_not_unwind() {
        let mut owner = BuildOwner::new();
        owner.set_on_build_scheduled(|| panic!("wake probe"));
        tracing::subscriber::with_default(PanicsOnOwnerEvent, || {
            owner.schedule_build_for(ElementId::new(1), 0, RebuildReason::StateChange);
        });
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
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    fn insert_child(
        tree: &mut ElementTree,
        owner: &mut BuildOwner,
        parent: ElementId,
        slot: usize,
    ) -> ElementId {
        tree.insert(
            &TestView.boxed(),
            parent,
            slot,
            &mut owner.element_owner_mut(),
        )
    }

    fn settle_initial_builds(tree: &mut ElementTree, owner: &mut BuildOwner) {
        owner.build_scope(tree);
    }

    fn nested_scope_quarantines_work_until_its_own_drain() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let root = tree.mount_root(&TestView.boxed(), &mut owner.element_owner_mut());
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
    #[derive(Clone, crate::StatelessView)]
    struct KeyedView {
        #[view(key)]
        key: crate::GlobalKey<()>,
    }

    impl crate::StatelessView for KeyedView {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> impl crate::IntoView {
            TestView
        }
    }

    #[derive(Clone, crate::StatefulView)]
    struct KeyedDependencyView {
        #[view(key)]
        key: crate::GlobalKey<()>,
        build_calls: Arc<AtomicUsize>,
        dependency_calls: Arc<AtomicUsize>,
    }

    struct KeyedDependencyState {
        dependency_calls: Arc<AtomicUsize>,
    }

    impl crate::StatefulView for KeyedDependencyView {
        type State = KeyedDependencyState;

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
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

        fn visit_child_views(&self, visitor: &mut dyn FnMut(&BoxedView)) {
            for child in &self.children {
                visitor(child);
            }
        }
    }

    impl View for ReparentingLayoutHost {
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
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
            &TestView.boxed(),
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
        let moved = tree.insert(
            &keyed.clone().boxed(),
            scope_a,
            0,
            &mut owner.element_owner_mut(),
        );
        // Its first build creates the state the dependency change reaches.
        owner.schedule_build_for(moved, 2, RebuildReason::InitialMount);
        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(scope_a));
        let builds_after_mount = build_calls.load(Ordering::Relaxed);

        tree.mark_needs_build(moved);
        owner.schedule_build_for(moved, 2, RebuildReason::StateChange);
        owner.schedule_build_for(moved, 2, RebuildReason::DependencyChange);
        owner.pending_dependency_changes.insert(moved);
        owner.build_scope(&mut tree);
        assert!(owner.pending_rebuild_reasons(moved).is_some());

        tree.remove(moved, &mut owner.element_owner_mut());
        let migrated = tree.insert(
            &keyed.clone().boxed(),
            scope_b,
            0,
            &mut owner.element_owner_mut(),
        );
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
        assert!(owner.pending_dependency_changes.contains(moved));
        owner.build_scope_target(&mut tree, BuildScopeTarget::LayoutBuilder(scope_b));
        assert!(owner.pending_rebuild_reasons(moved).is_none());
        assert!(!owner.pending_dependency_changes.contains(moved));
        assert_eq!(dependency_calls.load(Ordering::Relaxed), 1);
        assert_eq!(build_calls.load(Ordering::Relaxed), builds_after_mount + 1);

        tree.mark_needs_build(moved);
        owner.schedule_build_for(moved, 2, RebuildReason::StateChange);
        owner.schedule_build_for(moved, 2, RebuildReason::DependencyChange);
        owner.pending_dependency_changes.insert(moved);
        owner.build_scope(&mut tree);
        assert!(owner.pending_rebuild_reasons(moved).is_some());

        tree.remove(moved, &mut owner.element_owner_mut());
        let migrated_global = tree.insert(&keyed.boxed(), root, 2, &mut owner.element_owner_mut());
        assert_eq!(migrated_global, moved);
        assert_eq!(
            tree.get(moved).and_then(crate::tree::ElementNode::parent),
            Some(root)
        );
        owner.build_scope(&mut tree);

        assert!(owner.pending_rebuild_reasons(moved).is_none());
        assert!(!owner.pending_dependency_changes.contains(moved));
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
    // forcing the hazard the ui_runtime execution contract rules out. The unit
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
        let root_a = tree_a.mount_root(&keyed.clone().boxed(), &mut owner_a.element_owner_mut());
        assert_eq!(tree_a.len(), 1);
        let tag_a = owner_a.owner_tag();

        let mut tree_b = ElementTree::new();
        let mut owner_b = BuildOwner::new();
        owner_b.set_global_key_scope(scope);
        let tag_b = owner_b.owner_tag();

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree_b.mount_root(&keyed.clone().boxed(), &mut owner_b.element_owner_mut());
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
        let root_a = tree_a.mount_root(&TestView.boxed(), &mut owner_a.element_owner_mut());
        let child_a = tree_a.insert(
            &keyed.clone().boxed(),
            root_a,
            0,
            &mut owner_a.element_owner_mut(),
        );

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
        let root_b = tree_b.mount_root(&keyed.boxed(), &mut owner_b.element_owner_mut());
        assert_eq!(tree_b.len(), 1);
        assert_eq!(owner_b.element_for_global_key(&key), Some(root_b));
    }

    // ========================================================================
    // Mid-drain external-build absorption
    // ========================================================================

    /// A render-family leaf, identical to `TestView` in every other respect,
    /// that opts into [`View::should_skip_rebuild`]. Every fixture below that
    /// returns a leaf from its own `build()` uses THIS type, not the shared
    /// `TestView` every other test in this module also mounts: without the
    /// skip, a parent rebuilding and re-declaring the same leaf every time
    /// would reschedule that hidden leaf too (and re-fire `on_build_scheduled`)
    /// on every one of the PARENT's own rebuilds — noise unrelated to
    /// whatever an absorption test is actually driving. Scoped to this dedicated
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
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
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

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
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
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
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

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
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
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
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
            }
            .boxed(),
            &mut owner.element_owner_mut(),
        );
        // The state `did_change_dependencies` reaches is created by a first build.
        owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
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
        owner.external_inbox.lock().merge(
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
            }
            .boxed(),
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
        use flui_interaction::focus::FocusNode;
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
                ("reentrant_retry_preserves_the_first_panic", reentrant_retry_preserves_the_first_panic as fn()),
                ("direct_build_scheduling_retries_a_failed_wake", direct_build_scheduling_retries_a_failed_wake as fn()),
                ("a_panicking_report_of_a_failed_wake_does_not_unwind", a_panicking_report_of_a_failed_wake_does_not_unwind),
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
