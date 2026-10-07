//! Presentations hosted by a `UiRuntime`: installation, entry, per-presentation access, hide and close.

use super::UiRuntime;
use super::commands::UiCommandSender;
use crate::presentation::{PresentationState, PresentationWindow, RuntimeCapabilities};
use flui_foundation::PresentationId;
#[cfg(any(test, feature = "test-support"))]
use flui_interaction::FocusManager;
use flui_interaction::GestureBinding;
use flui_view::__runtime::GlobalKeyRegistryComposite;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
#[cfg(any(test, feature = "test-support"))]
use std::rc::Rc;
use std::sync::Arc;

impl UiRuntime {
    /// Current presentation incarnation.
    #[must_use]
    pub fn presentation_id(&self) -> PresentationId {
        self.presentations.primary().id()
    }

    /// True if `id` names this UI runtime's ONLY currently-hosted presentation.
    ///
    /// `runner.rs`'s `RuntimeTask::ClosePresentation` handling reads this
    /// BEFORE calling [`Self::close_presentation_entered`]: closing the
    /// UI runtime's sole presentation would leave the forest empty, and every
    /// other method on this UI runtime (`widgets()`, `presentation_id()`, a
    /// future frame pump) assumes `PresentationForest::primary()` always
    /// resolves — closing the last presentation is closing the UI runtime, and
    /// must route there instead (see that match arm's own doc).
    #[must_use]
    pub fn is_sole_presentation(&self, id: PresentationId) -> bool {
        self.presentations.len() == 1 && self.presentations.get(id).is_some()
    }

    /// The presentation id that would become this UI runtime's PRIMARY if `id`
    /// were removed right now — WITHOUT actually removing it.
    ///
    /// `runner.rs`'s `RuntimeTask::ClosePresentation` handling reads this
    /// BEFORE running `id`'s own teardown/dispose hooks (`close_presentation_
    /// entered`'s step 2-3), not after: `RuntimeSlot::address.presentation_id`
    /// must already name the SURVIVING primary by the time a dispose hook
    /// could re-enter `dispatch_platform_ui_runtime` with `id`'s own dispatcher,
    /// or that reentrant dispatch would still compare EQUAL against the
    /// not-yet-updated address, pass the `StalePresentation` check, and get
    /// enqueued (same-UI runtime reentrancy) instead of refused — running later
    /// in the same drain loop against whatever survives the close, silently
    /// misaddressed rather than refused outright.
    ///
    /// Mirrors exactly what `PresentationForest::remove`'s `Vec::remove`
    /// shift produces: if `id` is the CURRENT primary, the new primary is
    /// whichever presentation is next in mount order; otherwise removing
    /// `id` cannot move index 0 at all, so the primary is unchanged.
    /// `None` only if `id` names the UI runtime's only presentation — unreachable
    /// from that same dispatch handling, which checks
    /// [`Self::is_sole_presentation`] first and never reaches this call in
    /// that case.
    #[must_use]
    pub fn primary_id_excluding(&self, id: PresentationId) -> Option<PresentationId> {
        if self.presentations.primary().id() != id {
            return Some(self.presentations.primary().id());
        }
        self.presentations
            .iter()
            .find(|presentation| presentation.id() != id)
            .map(PresentationState::id)
    }

    /// Number of presentations this UI runtime currently hosts — for the
    /// isolation test suite (production topology allows any number since
    /// this slice lifted `PresentationForest`'s former ratchet).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn presentation_count(&self) -> usize {
        self.presentations.len()
    }

    /// This UI runtime's `WidgetsBinding` for the presentation named `id` — for
    /// the isolation test suite, which uses it to register a `GlobalKey`
    /// directly into a NON-primary presentation without going through
    /// [`Self::enter`] (registration itself needs no active composite; only
    /// resolution via [`flui_view::GlobalKey::current_element`] does).
    ///
    /// # Panics
    ///
    /// If `id` names no presentation of this UI runtime.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn presentation_widgets_for_test(&self, id: PresentationId) -> &flui_view::WidgetsBinding {
        self.presentations
            .get(id)
            .expect("BUG: presentation_widgets_for_test called with an unknown id")
            .widgets()
    }

    /// This UI runtime's `GestureBinding` for the presentation named `id` — the
    /// addressed counterpart to [`Self::gestures`] (primary-only), for the
    /// tests proving focus-loss cancellation reaches exactly the binding
    /// the addressed presentation's pointer input lands in.
    ///
    /// # Panics
    ///
    /// If `id` names no presentation of this UI runtime.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn presentation_gestures_for_test(&self, id: PresentationId) -> &GestureBinding {
        self.presentations
            .get(id)
            .expect("BUG: presentation_gestures_for_test called with an unknown id")
            .gestures()
    }

    /// Assemble another presentation for this UI runtime, sharing its exact
    /// `GlobalKeyScope` and UI runtime-level dispatch handles — WITHOUT
    /// installing it into the forest yet. Deliberately split from
    /// [`Self::install_presentation`] below: a caller that must register a
    /// `WindowRegistry` mapping before this presentation is safely
    /// dispatchable (`runner.rs::install_presentation_alongside`) can do so
    /// BETWEEN assembly and installation — if registration fails, the
    /// assembled-but-never-installed `PresentationState` is simply dropped
    /// (no forest membership to roll back; its construction has no
    /// observable side effect on anything this UI runtime shares, since nothing
    /// has attached/mounted into it yet).
    ///
    /// `window` becomes this presentation's own native window (cursor,
    /// haptics, redraw-poke — see `PresentationState::new`'s wiring).
    /// Its pipeline is seeded with `window.scale_factor()` before
    /// construction — the same DPR-before-first-frame ordering
    /// the UI runtime's constructor uses for its own primary presentation
    /// (there, the caller reads the window's scale factor and passes it in
    /// as `device_pixel_ratio`; here, `window` is already in hand, so this
    /// method reads it directly instead of requiring every caller to
    /// remember to). Without this, a non-primary presentation's
    /// `RenderView` and first frame would disagree with its OWN window's
    /// actual scale — silently defaulting to `1.0` regardless of what
    /// `window.scale_factor()` actually reports.
    #[must_use]
    pub fn assemble_presentation(
        &self,
        window: impl Into<PresentationWindow>,
    ) -> PresentationState {
        let window = window.into();
        let (_, presentation_id) = crate::runtime_services::next_identity();
        let device_pixel_ratio = window.window().scale_factor();
        // The prototype is stamped for the PRIMARY presentation; this
        // presentation's accessibility actions must address ITSELF, or the
        // drain would resolve them against a sibling's semantics tree.
        let command_sender = UiCommandSender {
            presentation_id,
            ..self.sender_prototype.clone()
        };
        PresentationState::new(
            presentation_id,
            Some(device_pixel_ratio),
            window,
            RuntimeCapabilities {
                global_key_scope: self.global_key_scope.clone(),
                async_driver: self.owner_frame.async_driver(),
                local_post_frame_handle: self.owner_frame.local_post_frame_handle(),
                interaction_dispatch_handle: self.interaction_lane.dispatch_handle(),
                scheduler: &self.scheduler,
                wake: Arc::clone(&self.wake),
                command_sender,
                clipboard: Arc::clone(&self.clipboard),
                storage: self.storage.clone(),
                clock: &self.clock,
                text: self.text.clone(),
            },
        )
    }

    /// Install an already-[`assembled`](Self::assemble_presentation)
    /// presentation into this UI runtime's forest — the production entry point
    /// (this slice lifted `PresentationForest::install`'s former
    /// `len()<=1` ratchet).
    ///
    /// The caller is responsible for minting this presentation's
    /// `WindowRegistry` mapping BEFORE calling this (typically between
    /// `assemble_presentation` and this call) — `UiRuntime` has no access to
    /// that registry (ADR-0037 §2), exactly the same split
    /// [`Self::close_presentation_entered`]'s own doc states for teardown.
    /// `runner.rs::install_presentation_alongside` is that caller in
    /// production; `Self::install_second_presentation_for_test`
    /// (`cfg(test)`-only, no stable link target in a non-test doc build) is
    /// the test-only counterpart for `UiRuntime`-only tests that never touch
    /// `AppRuntime`/`WindowRegistry` at all.
    pub fn install_presentation(&mut self, presentation: PresentationState) -> PresentationId {
        let presentation_id = presentation.id();
        if presentation.window_focused.get() && presentation.window_visible.get() {
            for previous in self.presentations.iter() {
                previous.window_focused.set(false);
            }
            self.focus_coordinator.note_focus_gained(presentation_id);
        }
        self.presentations.install(presentation);
        presentation_id
    }

    /// [`Self::install_presentation`], with a throwaway test window — for
    /// `UiRuntime`-only tests (this module's own `mod tests`, plus
    /// `runner.rs`'s dispatch-seam tests) that exercise a genuine
    /// multi-presentation UI runtime WITHOUT ever touching `AppRuntime`'s
    /// `WindowRegistry` — that layer is a different module's own test
    /// concern (`runner.rs`'s `install_presentation_alongside`, exercised by
    /// the three tests that need a REAL window mapping for the second
    /// presentation). `pub(crate)` so `runner.rs`'s own tests that only need
    /// forest membership, never registry routing, can still use it.
    #[cfg(test)]
    pub(crate) fn install_second_presentation_for_test(&mut self) -> PresentationId {
        let window: Arc<dyn flui_platform_api::PlatformWindow> =
            Arc::new(crate::testing::TestWindow::new().focused(false));
        let presentation = self.assemble_presentation(window);
        self.install_presentation(presentation)
    }

    /// The presentation keyboard input currently routes to
    /// (`FocusCoordinator`) — for the isolation suite proving
    /// `WindowFocus` events move it.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn active_presentation_for_test(&self) -> PresentationId {
        self.focus_coordinator.active()
    }

    /// Whether `id`'s own [`FrameClock`](flui_scheduler::FrameClock) is
    /// currently marked hidden — for
    /// the isolation suite (`runner.rs`) proving `WindowVisibility` events
    /// gate exactly the presentation they were addressed to, never a
    /// sibling's. `None` if `id` is not resident (already closed, or a
    /// forged/mixed address).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn presentation_hidden_for_test(&self, id: PresentationId) -> Option<bool> {
        self.presentations.get(id).map(|p| p.clock().is_hidden())
    }

    /// The device pixel ratio `id`'s render pipeline paints and publishes
    /// semantics bounds at — for the dispatcher test proving a `Resized`
    /// event rescales exactly the presentation it was addressed to. `None`
    /// if `id` is not resident.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn presentation_device_pixel_ratio_for_test(&self, id: PresentationId) -> Option<f64> {
        self.presentations.get(id).map(|presentation| {
            flui_rendering::binding::RendererBinding::root_pipeline_owner(presentation.renderer())
                .with(flui_rendering::PipelineOwner::device_pixel_ratio)
        })
    }

    /// Whether `id`'s pipeline holds a semantics tree: something (assistive
    /// technology, or an agent) asked for one and a frame has run since. `None`
    /// if `id` is not resident.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn presentation_collects_semantics_for_test(&self, id: PresentationId) -> Option<bool> {
        self.presentations.get(id).map(|presentation| {
            presentation
                .pipeline()
                .with(|pipeline| pipeline.semantics_owner().is_some())
        })
    }

    /// The primary presentation's own `FrameClock::produced_count` — for
    /// tests in a sibling module (`runner.rs`) that need to observe the
    /// clock's produce count without reaching into the private
    /// `presentations` field directly.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn primary_produced_count_for_test(&self) -> u64 {
        self.presentations.primary().clock().produced_count()
    }

    /// This UI runtime's own scheduler — the strong root. `runner.rs`'s
    /// UI runtime-scoped lifecycle sites (`UiRuntime::update_host_lifecycle`, the
    /// per-backend frame pumps) read through here instead of a process-global
    /// singleton; see [`Self::scheduler`]'s field doc for the ownership
    /// story.
    #[must_use]
    pub fn scheduler(&self) -> &flui_scheduler::UpdateScheduler {
        &self.scheduler
    }

    /// This UI runtime's owner-local frame state (its post-frame queue and async
    /// tasks). Test-only: production drives every frame through
    /// [`Self::pump`], whose frame drive passes it to
    /// `UpdateScheduler::drive_frame` itself so no host can drive a frame
    /// that forgets it. A test that hand-assembles a frame drive passes it the
    /// same way — by parameter, the same reason [`Self::scheduler`] exists
    /// rather than a process-global lookup.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn owner_frame(&self) -> &flui_scheduler::OwnerFrame {
        &self.owner_frame
    }

    /// Enter this UI runtime's owner scope.
    ///
    /// A composite `GlobalKey` registry spanning every presentation this
    /// UI runtime hosts (ADR-0043 §1) is active for the entire dynamic
    /// extent of `f`, including lifecycle/build callbacks — whole-frame,
    /// exactly as a single presentation's own registry always was: command
    /// drains, deferred arena resolutions, and hot-reload reassemble all
    /// resolve keys UI runtime-wide regardless of which presentation's own
    /// segment is running. Nested entry is stack-shaped and panic unwinding
    /// restores the previously active ui_runtime.
    ///
    /// Does NOT activate the owner frame: `LocalPostFrameHandle`
    /// addresses its lane directly (a `Weak` pointer minted once per
    /// presentation), so `schedule_local` needs no ambient "active lane"
    /// scope to succeed. The frame drive drains that lane by passing it
    /// explicitly to `UpdateScheduler::drive_frame`/
    /// `end_frame`, not by anything entered here.
    pub fn enter<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        self.interaction_lane.enter(|| {
            let composite = GlobalKeyRegistryComposite::assemble(
                self.presentations.iter().map(PresentationState::widgets),
            );
            composite.enter(|| f(self))
        })
    }

    /// The primary presentation's owner-local widgets binding. For the host
    /// only: a caller that drives the tree through it outside
    /// [`Self::enter`] bypasses the UI runtime's entry scope.
    #[must_use]
    pub fn widgets(&self) -> &flui_view::WidgetsBinding {
        self.presentations.primary().widgets()
    }

    /// The presentation whose `BuildOwner` holds the graph that minted `slot`,
    /// with a handle to that graph. `None` once that presentation has closed,
    /// or for a slot minted outside this UI runtime.
    ///
    /// Each binding's read lock is released before this returns, so the
    /// caller runs the write with no lock held.
    pub(super) fn signal_graph_for(
        &self,
        slot: flui_view::SignalSlot,
    ) -> Option<(&PresentationState, flui_view::Reactive)> {
        let graph = slot.graph();
        self.presentations.iter().find_map(|presentation| {
            presentation
                .widgets()
                .with_build_owner(|owner| {
                    let reactive = owner.reactive();
                    (reactive.id() == graph).then(|| reactive.clone())
                })
                .map(|reactive| (presentation, reactive))
        })
    }

    /// The PRIMARY presentation's live root media-query source (see the field
    /// doc).
    ///
    /// Infallible by this UI runtime's own invariant: a UI runtime always hosts at least
    /// one presentation, and [`Self::presentation_id`] is that primary — so
    /// the root-attach paths that wrap a tree in `MediaQueryRoot` can call
    /// this directly. An event ADDRESSED to a particular presentation (a
    /// resize, safe-area or appearance report stamped at enqueue time) must
    /// use [`Self::media_query_for`] instead: that id travels through a queue
    /// shared by every presentation the UI runtime hosts, so it can name a
    /// presentation that was closed before the event was delivered.
    pub(crate) fn media_query(&self) -> &std::rc::Rc<crate::media_query_root::MediaQuerySource> {
        &self.presentations.primary().media_query
    }

    /// The ADDRESSED presentation's live root media-query source, or `None`
    /// when this UI runtime no longer hosts it.
    ///
    /// `None` is an ordinary outcome of addressed delivery, not a bug: an
    /// event is admitted when it is ENQUEUED (its dispatcher's address is in
    /// the registry then) and delivered later, in FIFO order, behind whatever
    /// was already queued. A close for that same presentation enqueued in
    /// between therefore runs first and removes it, leaving this lookup with
    /// nothing to write to. Every sibling addressed handler in this UI runtime
    /// ([`Self::handle_input_addressed`], [`Self::update_window_focus`],
    /// [`Self::update_window_execution`]) treats that same interleaving the
    /// same way: drop the addressed effect, keep the UI runtime-wide one.
    pub fn media_query_for(
        &self,
        id: PresentationId,
    ) -> Option<&std::rc::Rc<crate::media_query_root::MediaQuerySource>> {
        self.presentations
            .get(id)
            .map(|presentation| &presentation.media_query)
    }

    /// The primary presentation's gesture binding.
    #[must_use]
    pub fn gestures(&self) -> &GestureBinding {
        self.presentations.primary().gestures()
    }

    /// Cancel in-flight pointer sequences on the ADDRESSED presentation's
    /// own gesture binding, and drop the open sequences its held pointer
    /// queue still waits to replay (`HeldPointerQueue::drop_open_sequences`).
    ///
    /// Pointer input routes per presentation
    /// ([`Self::handle_input_addressed`] dispatches to
    /// `presentation.gestures()`), so a focus-loss cancellation must reach
    /// the same binding that presentation's Downs landed in — the
    /// UI runtime-level [`Self::gestures`] wrapper is primary-only, and under a
    /// shared-UI runtime window policy it would cancel a sibling presentation's
    /// sequences while leaving the defocused window's own drag stranded. A
    /// presentation this UI runtime no longer hosts is a traced no-op, matching
    /// the addressed-input path's posture for the same race.
    pub(crate) fn cancel_pointer_sequences_for(&self, presentation_id: PresentationId) {
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a pointer-sequence cancellation addressed to a presentation this \
                 ui_runtime no longer hosts"
            );
            return;
        };
        // Held input first: an open sequence still waiting for a commit
        // would otherwise replay its Down after this cancel and open a route
        // whose Up went to another window. No user code runs while the queue
        // is borrowed.
        presentation
            .held_pointer_input()
            .borrow_mut()
            .drop_open_sequences();
        presentation.gestures().cancel_active_pointers();
    }

    /// Focus state for the UI runtime's current primary presentation. Since
    /// issue #555's addressed-routing slice, [`Self::handle_input_addressed`] reads the ADDRESSED
    /// presentation's own `focus_manager()` directly instead of through this
    /// primary-only wrapper; kept for the tests that still exercise a
    /// single-presentation UI runtime's focus tree without addressing.
    // Test-only: production input dispatch reads the addressed
    // presentation's own `focus_manager()` (`handle_input_addressed`).
    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub fn focus_manager(&self) -> Rc<FocusManager> {
        self.presentations.primary().focus_manager()
    }

    /// Weak text-input capability for this exact presentation.
    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub fn text_input_handle(&self) -> flui_interaction::TextInputHandle {
        self.presentations.primary().text_input_handle()
    }

    /// Reassemble EVERY presentation this UI runtime hosts, in mount order —
    /// under the whole-frame composite `enter()` already activates, so a
    /// key resolved mid-fan-out still finds any presentation's tree, not
    /// just the one currently reassembling.
    ///
    /// Deliberately does not short-circuit on the first presentation that
    /// reports a change: every presentation must reassemble regardless of
    /// what its predecessors reported, or a hot reload would silently skip
    /// later-mounted presentations whenever an earlier one happened to
    /// report no change. Returns whether ANY presentation requires a
    /// redraw.
    #[cfg(feature = "hot-reload")]
    #[must_use]
    pub(crate) fn apply_hot_reload(&self, tier: crate::reload::ReloadTier) -> bool {
        let mut needs_redraw = false;
        for presentation in self.presentations.iter() {
            if presentation.apply_hot_reload(tier) {
                needs_redraw = true;
            }
        }
        needs_redraw
    }

    /// Apply a hot reload at the given tier,
    /// requesting a redraw if it actually changed anything. Moved here from
    /// the retired `AppBinding::perform_hot_reload_entered`.
    #[cfg(feature = "hot-reload")]
    pub fn perform_hot_reload_entered(&self, tier: crate::reload::ReloadTier) {
        if self.apply_hot_reload(tier) {
            self.request_redraw();
        }
    }

    /// Gate `presentation_id`'s own
    /// [`FrameClock`](flui_scheduler::FrameClock) on a real visibility
    /// signal — the presentation-level counterpart of the
    /// UI runtime-wide `AppLifecycleState` derivation
    /// `PlatformToUi::WindowVisibility` (`runner.rs`) also drives. A stale
    /// or unknown `presentation_id` (already closed, or a forged/mixed
    /// address) is a
    /// traced no-op, matching [`Self::notify_presentation_focus_gained`]'s
    /// own discipline.
    ///
    /// While hidden,
    /// [`FrameClock::poll`](flui_scheduler::FrameClock::poll) returns
    /// `Skip(Hidden)` for this
    /// presentation: `draw_frame_entered`'s segment loop skips its
    /// build/layout/paint/submit entirely, demand retained. Becoming visible
    /// again does NOT self-produce — nothing wakes an idle platform loop on
    /// its own — so this is also the ungate→wake edge: with a nonzero
    /// retained mask, this call wakes the UI runtime unconditionally rather than
    /// consulting
    /// [`FrameClock::try_arm_redraw_request`](flui_scheduler::FrameClock::try_arm_redraw_request).
    /// That latch cannot
    /// be trusted here: it may already be armed from a demand mark that
    /// occurred BEFORE this presentation went hidden (a poke the platform
    /// loop already delivered and wasted, since `poll` returned `Skip(Hidden)`
    /// for it) — reading `try_arm_redraw_request()`'s return value would
    /// incorrectly stay silent in that sequence and strand the presentation
    /// gated forever with demand nobody ever wakes for.
    pub(crate) fn set_presentation_hidden(&self, presentation_id: PresentationId, hidden: bool) {
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a visibility change for a presentation this ui_runtime no longer hosts"
            );
            return;
        };
        presentation.clock().set_hidden(hidden);
        if !hidden && !presentation.clock().demand_mask().is_empty() {
            self.wake_frame();
        }
    }

    /// Close and remove exactly one presentation from this UI runtime's forest.
    ///
    /// Called from `runner.rs`'s `dispatch_platform_ui_runtime` loop, which
    /// special-cases `RuntimeTask::ClosePresentation` to call this with
    /// `&mut self` directly (the UI runtime sits checked out of `AppRuntime`'s
    /// registry as an owned local at that point, so `&mut` is naturally
    /// available — no other caller reaches this method). That same
    /// dispatch has ALREADY set `dispatched_ui_runtime_id` for the whole
    /// checkout, so a dispose callback this method's own step 3 runs is
    /// covered by the identical TLS deferral guard every other UI runtime-map
    /// mutation already respects: one deferral authority, not a second one
    /// to keep in sync with it. See `runner.rs::close_presentation` for the
    /// request-shaped public entry point (`enqueue + wake`) that gets here.
    ///
    /// A no-op (returns `false`) if `id` does not name a presentation this
    /// UI runtime currently hosts.
    ///
    /// # Contract — six steps, in order
    ///
    /// 1. **Unregistering this presentation's window mapping from the
    ///    process-wide `WindowRegistry` is NOT this method's job.**
    ///    `UiRuntime` has no access to that registry (`AppRuntime`-owned,
    ///    ADR-0037 §2's single authority) — the caller must have already
    ///    removed the registry entry before calling this. The real (and
    ///    today's only) caller is `dispatch_platform_ui_runtime`'s
    ///    `RuntimeTask::ClosePresentation` handling in `runner.rs`, NOT
    ///    `AppRuntime`'s UI runtime-teardown path (`apply_uninstall`/
    ///    `teardown_platform_ui_runtime` close a WHOLE UI runtime and never call this
    ///    method at all — a UI runtime's own `Drop` closes every remaining
    ///    presentation directly). That dispatch handling unregisters this
    ///    exact presentation's own window mapping first when a SIBLING
    ///    presentation is staying open, and routes to a full UI runtime
    ///    uninstall instead of calling this method at all when `id` names
    ///    the UI runtime's only presentation (seeing [`Self::is_sole_presentation`]
    ///    return `true`) — this method's own contract never has to reason
    ///    about leaving the forest empty.
    /// 2. IME detach + focus deactivate — `PresentationState::close`'s
    ///    first half.
    /// 3. `detach_root_widget` through this exact presentation's own
    ///    `WidgetsBinding` — `PresentationState::close`'s second half —
    ///    run inside [`Self::enter`], so a `State::dispose()` a
    ///    descendant runs here gets the SAME UI runtime-shared capabilities
    ///    (post-frame/interaction handles, TLS deferral) any other
    ///    frame/lifecycle callback gets, and can resolve a `GlobalKey`
    ///    living in any OTHER presentation this UI runtime hosts. The composite
    ///    includes `id` itself: until `detach_root_widget` takes its
    ///    binding's write lock, `id`'s own keys resolve (a lifecycle observer
    ///    told it is detaching sees them); during the removal walk its
    ///    registry reports itself busy and the composite skips it
    ///    (`flui-view`'s `key::registry`, "Re-entrancy"), so a dispose hook
    ///    resolves `id`'s keys to nothing rather than re-entering the lock
    ///    the walk holds. An install/uninstall request a
    ///    dispose hook makes here defers through the dispatched-path TLS
    ///    guard described above. Any BUILD SCHEDULING it does still routes
    ///    only within THIS presentation's own tree:
    ///    `RebuildHandle`/`ExternalBuildScheduler` are minted one-per-owner
    ///    and never shared, so a dispose hook has no path to a sibling
    ///    presentation's build inbox even if it tries.
    /// 4. **Async-task disposition:** the UI runtime-level `AsyncDriver` is not
    ///    told about this closure — an in-flight task this presentation
    ///    spawned is NOT cancelled here, matching `PresentationState`'s own
    ///    `Drop` (which also does not reach into the driver). Its eventual
    ///    completion fails closed against the now-dropped tree for the same
    ///    reason step 3's dispose hooks do:
    ///    `async_completion_after_presentation_teardown_fails_closed_no_sibling_reach`
    ///    is the proof.
    /// 5. `GlobalKeyScope` claims still tagged to this presentation's
    ///    `BuildOwner` are reclaimed, traced — automatic, via `BuildOwner`'s
    ///    own `Drop`, once step 6 drops the last reference to it.
    /// 6. The removed `PresentationState` — and with it its `WidgetsBinding`
    ///    (whose drop triggers step 5), `RenderingBinding`, and every
    ///    other owned resource — drops after [`Self::enter`] returns.
    ///
    /// # Why this is two calls, not one `&mut`-threaded closure
    ///
    /// [`Self::enter`] hands its closure `&Self` (shared) — every caller
    /// only needs read access to the UI runtime for the duration of a
    /// frame/lifecycle callback, so it does not hand out `&mut`. Steps 2–3
    /// only need `&PresentationState` (`PresentationState::close` takes
    /// `&self`), so they run inside one `self.enter(...)` call. Steps 4–6
    /// (the actual `Vec` removal + drop) need `&mut self.presentations`,
    /// which happens in a SEPARATE, sequential statement after that call
    /// returns — not nested inside it, so there is no borrow conflict and
    /// no need to give `PresentationForest` interior mutability.
    pub fn close_presentation_entered(&mut self, id: PresentationId) -> bool {
        if self.presentations.get(id).is_none() {
            return false;
        }
        let mut first_panic = catch_unwind(AssertUnwindSafe(|| {
            self.enter(|ui_runtime| {
                if ui_runtime.focus_coordinator.active() == id
                    && let Some(surviving) = ui_runtime.primary_id_excluding(id)
                {
                    ui_runtime.focus_coordinator.note_focus_gained(surviving);
                }
                ui_runtime.stop_presentation(id);
            });
        }))
        .err();
        // Membership removal must complete even when an observer or disposer panics.
        let removed = self.presentations.remove(id);
        let preserving = first_panic.is_some()
            || std::thread::panicking()
            || removed
                .as_ref()
                .is_some_and(crate::presentation::PresentationState::preserving_close);
        let failure = if preserving {
            // The closed envelope keeps generic field Drop out of a recovery
            // transaction whose first payload already belongs to the caller.
            if let Some(presentation) = &removed {
                let failure = catch_unwind(AssertUnwindSafe(|| {
                    presentation
                        .close_with_mode(flui_interaction::__runtime::CloseMode::PreservingFailure);
                }))
                .err();
                crate::lifecycle_state::preserve_first_lifecycle_panic(
                    &mut first_panic,
                    failure,
                    "removed presentation withdrawal",
                );
            }
            std::mem::forget(removed);
            None
        } else {
            catch_unwind(AssertUnwindSafe(|| drop(removed))).err()
        };
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_panic,
            failure,
            "removed presentation cleanup",
        );
        let failure = catch_unwind(AssertUnwindSafe(|| self.synchronize_window_lifecycle())).err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_panic,
            failure,
            "surviving presentation lifecycle",
        );
        if let Some(payload) = first_panic {
            if std::thread::panicking() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                resume_unwind(payload);
            }
        }
        true
    }
}
