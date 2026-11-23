//! The presentation capabilities a runtime installs on the owner (lifecycle, window, scheduler, focus, hit-test handles) and the tree observer.

use super::{BuildOwner, notify_detached};
use crate::owner::global_key_scope::GlobalKeyScope;
#[cfg(test)]
use crate::owner::global_key_scope::OwnerTag;
use flui_interaction::focus::FocusManager;
use std::rc::Rc;
use std::sync::Arc;

impl BuildOwner {
    #[doc(hidden)]
    pub fn set_lifecycle_handle(&mut self, handle: crate::LifecycleHandle) {
        self.lifecycle_handle = Some(handle);
    }

    /// Give the tree's elements the window they are presented in.
    #[doc(hidden)]
    pub fn set_window_handle(&mut self, handle: crate::WindowHandle) {
        self.window_handle = Some(handle);
    }

    /// Weak window capability, absent for a bare owner or one without a window.
    /// Test-support only: the borrowed `LiveBuildContext` reads it off its capabilities.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn window_handle(&self) -> Option<crate::WindowHandle> {
        self.window_handle.clone()
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
    /// Called once, at wiring time, by `UiRuntime`. Must
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
    /// Called once, at wiring time, by
    /// `UiRuntime`'s own wiring. It must name **that binding's** scheduler — the
    /// one whose `drive_frame` drains the queue. Each UI runtime, a headless
    /// host's included, drives its own owned `UpdateScheduler`
    /// (`UiRuntime.scheduler`) — there is no process-global scheduler singleton
    /// any more.
    pub fn set_post_frame_handle(&mut self, handle: flui_scheduler::PostFrameHandle) {
        self.post_frame_handle = Some(handle);
    }

    /// Install the binding's owner-local interaction dispatch handle (ADR-0027).
    pub fn set_interaction_dispatch_handle(
        &mut self,
        handle: flui_interaction::pointer::InteractionDispatchHandle,
    ) {
        self.interaction_dispatch = Some(handle);
    }

    /// Install this presentation's fresh-hit-test capability.
    ///
    /// Called during presentation assembly with a handle pairing the UI runtime's
    /// dispatch ticket with a probe over THIS presentation's pipeline.
    /// A bare `BuildOwner` (an element-tree unit test's) has none, so its tests observe
    /// `LifecycleContext::hit_test_handle() == None` honestly rather than reading
    /// some other tree.
    pub fn set_hit_test_handle(&mut self, handle: flui_interaction::pointer::HitTestHandle) {
        self.hit_test_handle = Some(handle);
    }

    /// Install the UI runtime's shared `GlobalKey` uniqueness domain (ADR-0043).
    ///
    /// Called once, at presentation-assembly time, before this owner's tree
    /// is mounted — the same post-construction wiring pattern as
    /// [`Self::set_async_driver`] / [`Self::set_post_frame_handle`] /
    /// [`Self::capabilities_mut`] / [`Self::set_interaction_dispatch_handle`].
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
    /// use flui_view::__runtime::{BuildOwner, GlobalKeyScope};
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

    /// This owner's fresh-hit-test capability, if a presentation installed one.
    ///
    /// Stored per owner, not derived from the UI runtime-wide interaction dispatch
    /// handle: a UI runtime may host several presentations, each with its own
    /// `PipelineOwner`, and a hit test must read the tree of the presentation
    /// that asked. A UI runtime-scoped probe would answer every one of them with
    /// whichever tree was installed first.
    #[must_use]
    pub fn hit_test_handle(&self) -> Option<&flui_interaction::pointer::HitTestHandle> {
        self.hit_test_handle.as_ref()
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
        self.on_build_scheduled = Some(Arc::new(callback));
        self.reactive.set_scheduler(self.external_scheduler());
    }

    /// The UI runtime's reactive graph (ADR-0074): signals and the
    /// reader registry that schedules exactly the elements that read a
    /// written signal.
    pub(crate) fn reactive(&self) -> &crate::reactive::Reactive {
        &self.reactive
    }

    /// The write capability of this owner's graph, for host code that writes
    /// a signal outside any widget: a cross-thread write delivered on the
    /// owner thread, a test harness. Each [`WriterSource::write`] opens its
    /// own [`EventContext`](crate::EventContext), and the build guard still refuses a
    /// write made while an element of this owner is building.
    ///
    /// [`WriterSource::write`]: crate::WriterSource::write
    #[must_use]
    pub fn writer_source(&self) -> crate::WriterSource {
        crate::WriterSource::new(self.reactive.clone())
    }

    /// [`writer_source`](Self::writer_source) when this owner's graph minted
    /// `slot`, `None` otherwise: how a UI runtime with several presentations
    /// finds the one a cross-thread write belongs to (ADR-0085 §1).
    #[must_use]
    pub fn writer_source_for(&self, slot: crate::SignalSlot) -> Option<crate::WriterSource> {
        (slot.graph() == self.reactive.id()).then(|| self.writer_source())
    }

    /// The graph itself, for tests that mint signals outside an element (with
    /// an explicit element owner where the test needs one) and inspect its
    /// reader registry. It writes nothing: the graph is not a write target and
    /// cannot mint a `WriterSource`, so a write still goes through
    /// [`writer_source`](Self::writer_source). No build context reaches a
    /// `BuildOwner`, so `build` reaches neither.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn signal_graph(&self) -> &crate::reactive::Reactive {
        &self.reactive
    }

    /// The phase this owner's builds are published into: the UI runtime's
    /// once [`set_build_in_progress`](Self::set_build_in_progress) joined it.
    /// A host binds the registries it owns (a presentation's `Vsync`) to it.
    #[must_use]
    pub fn build_phase(&self) -> flui_foundation::BuildPhase {
        self.reactive.build_phase()
    }
}

impl BuildOwner {
    /// Install the UI runtime's tree observer, replacing any previous one
    /// (ADR-0040). A replaced observer receives `detached()` first, and the
    /// replacement is logged so competing tools discover each other.
    ///
    /// Install at UI runtime setup or via
    /// `WidgetsBinding::install_tree_observer` — never from a frame phase.
    pub fn set_tree_observer(&mut self, observer: Arc<dyn flui_foundation::observe::TreeObserver>) {
        if let Some(previous) = self.tree_observer.replace(observer) {
            notify_detached(previous);
        }
    }

    /// Remove the observer (UI runtime teardown — install/clear symmetry).
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
}
