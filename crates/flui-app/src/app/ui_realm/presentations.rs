//! Presentations hosted by a `UiRealm`: installation, entry, per-presentation access, hide and close.

use super::UiRealm;
use super::commands::UiCommandSender;
use crate::app::presentation::{PresentationState, RealmCapabilities};
use flui_foundation::PresentationId;
use flui_interaction::{FocusManager, GestureBinding};
use flui_platform::traits::PlatformWindow;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_view::GlobalKeyRegistryComposite;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;

impl UiRealm {
    /// Current presentation incarnation.
    #[must_use]
    pub fn presentation_id(&self) -> PresentationId {
        self.presentations.primary().id()
    }

    /// True if `id` names this realm's ONLY currently-hosted presentation.
    ///
    /// `runner.rs`'s `RealmTask::ClosePresentation` handling reads this
    /// BEFORE calling [`Self::close_presentation_entered`]: closing the
    /// realm's sole presentation would leave the forest empty, and every
    /// other method on this realm (`widgets()`, `presentation_id()`, a
    /// future frame pump) assumes `PresentationForest::primary()` always
    /// resolves — closing the last presentation is closing the REALM, and
    /// must route there instead (see that match arm's own doc).
    #[must_use]
    pub(crate) fn is_sole_presentation(&self, id: PresentationId) -> bool {
        self.presentations.len() == 1 && self.presentations.get(id).is_some()
    }

    /// The presentation id that would become this realm's PRIMARY if `id`
    /// were removed right now — WITHOUT actually removing it.
    ///
    /// `runner.rs`'s `RealmTask::ClosePresentation` handling reads this
    /// BEFORE running `id`'s own teardown/dispose hooks (`close_presentation_
    /// entered`'s step 2-3), not after: `RealmSlot::address.presentation_id`
    /// must already name the SURVIVING primary by the time a dispose hook
    /// could re-enter `dispatch_platform_realm` with `id`'s own dispatcher,
    /// or that reentrant dispatch would still compare EQUAL against the
    /// not-yet-updated address, pass the `StalePresentation` check, and get
    /// enqueued (same-realm reentrancy) instead of refused — running later
    /// in the same drain loop against whatever survives the close, silently
    /// misaddressed rather than refused outright.
    ///
    /// Mirrors exactly what [`PresentationForest::remove`]'s `Vec::remove`
    /// shift produces: if `id` is the CURRENT primary, the new primary is
    /// whichever presentation is next in mount order; otherwise removing
    /// `id` cannot move index 0 at all, so the primary is unchanged.
    /// `None` only if `id` names the realm's only presentation — unreachable
    /// from that same dispatch handling, which checks
    /// [`Self::is_sole_presentation`] first and never reaches this call in
    /// that case.
    #[must_use]
    pub(crate) fn primary_id_excluding(&self, id: PresentationId) -> Option<PresentationId> {
        if self.presentations.primary().id() != id {
            return Some(self.presentations.primary().id());
        }
        self.presentations
            .iter()
            .find(|presentation| presentation.id() != id)
            .map(PresentationState::id)
    }

    /// Number of presentations this realm currently hosts — for the
    /// isolation test suite (production topology allows any number since
    /// this slice lifted `PresentationForest`'s former ratchet).
    #[cfg(test)]
    pub(crate) fn presentation_count(&self) -> usize {
        self.presentations.len()
    }

    /// This realm's `WidgetsBinding` for the presentation named `id` — for
    /// the isolation test suite, which uses it to register a `GlobalKey`
    /// directly into a NON-primary presentation without going through
    /// [`Self::enter`] (registration itself needs no active composite; only
    /// resolution via [`flui_view::GlobalKey::current_element`] does).
    /// `pub(crate)` for the same cross-module reason as
    /// [`Self::install_second_presentation_for_test`] above.
    #[cfg(test)]
    pub(crate) fn presentation_widgets_for_test(
        &self,
        id: PresentationId,
    ) -> &flui_view::WidgetsBinding {
        self.presentations
            .get(id)
            .expect("BUG: presentation_widgets_for_test called with an unknown id")
            .widgets()
    }

    /// This realm's `GestureBinding` for the presentation named `id` — the
    /// addressed counterpart to [`Self::gestures`] (primary-only), for the
    /// tests proving focus-loss cancellation reaches exactly the binding
    /// the addressed presentation's pointer input lands in.
    #[cfg(test)]
    pub(crate) fn presentation_gestures_for_test(&self, id: PresentationId) -> &GestureBinding {
        self.presentations
            .get(id)
            .expect("BUG: presentation_gestures_for_test called with an unknown id")
            .gestures()
    }

    /// This realm's `TextInputHandle` for the presentation named `id` — the
    /// addressed counterpart to [`Self::text_input_handle`] (primary-only),
    /// for the isolation suite proving IME sessions stay exclusive to the
    /// exact presentation they were attached on
    /// (`ime_event_addressed_to_b_does_not_reach_as_session`).
    #[must_use]
    #[cfg(test)]
    pub(crate) fn presentation_text_input_handle_for_test(
        &self,
        id: PresentationId,
    ) -> flui_interaction::TextInputHandle {
        self.presentations
            .get(id)
            .expect("BUG: presentation_text_input_handle_for_test called with an unknown id")
            .text_input_handle()
    }

    /// Assemble another presentation for this realm, sharing its exact
    /// `GlobalKeyScope` and realm-level dispatch handles — WITHOUT
    /// installing it into the forest yet. Deliberately split from
    /// [`Self::install_presentation`] below: a caller that must register a
    /// `WindowRegistry` mapping before this presentation is safely
    /// dispatchable (`runner.rs::install_presentation_alongside`) can do so
    /// BETWEEN assembly and installation — if registration fails, the
    /// assembled-but-never-installed `PresentationState` is simply dropped
    /// (no forest membership to roll back; its construction has no
    /// observable side effect on anything this realm shares, since nothing
    /// has attached/mounted into it yet).
    ///
    /// `window` becomes this presentation's own native window (cursor,
    /// haptics, redraw-poke — see `PresentationState::new`'s wiring).
    /// Its pipeline is seeded with `window.scale_factor()` before
    /// construction — the same DPR-before-first-frame ordering
    /// [`Self::construct`] uses for this realm's own primary presentation
    /// (there, the caller reads the window's scale factor and passes it in
    /// as `device_pixel_ratio`; here, `window` is already in hand, so this
    /// method reads it directly instead of requiring every caller to
    /// remember to). Without this, a non-primary presentation's
    /// `RenderView` and first frame would disagree with its OWN window's
    /// actual scale — silently defaulting to `1.0` regardless of what
    /// `window.scale_factor()` actually reports.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "reachable only through runner.rs::install_presentation_alongside, itself \
                      desktop-only"
        )
    )]
    pub(crate) fn assemble_presentation(
        &self,
        window: Arc<dyn PlatformWindow>,
    ) -> PresentationState {
        let (_, presentation_id) = crate::app::runtime::next_identity();
        let pipeline = PipelineCell::new(PipelineOwner::new());
        pipeline.with_mut(|owner| owner.set_device_pixel_ratio(window.scale_factor() as f32));
        // The prototype is stamped for the PRIMARY presentation; this
        // presentation's accessibility actions must address ITSELF, or the
        // drain would resolve them against a sibling's semantics tree.
        let command_sender = UiCommandSender {
            presentation_id,
            ..self.sender_prototype.clone()
        };
        PresentationState::new(
            presentation_id,
            pipeline,
            window,
            RealmCapabilities {
                global_key_scope: self.global_key_scope.clone(),
                async_driver: self.scheduler.async_driver().clone(),
                local_post_frame_handle: self.local_post_frame.local_handle(),
                interaction_dispatch_handle: self.interaction_lane.dispatch_handle(),
                scheduler: &self.scheduler,
                wake: Arc::clone(&self.wake),
                command_sender,
            },
        )
    }

    /// Install an already-[`assembled`](Self::assemble_presentation)
    /// presentation into this realm's forest — the production entry point
    /// (this slice lifted `PresentationForest::install`'s former
    /// `len()<=1` ratchet).
    ///
    /// The caller is responsible for minting this presentation's
    /// `WindowRegistry` mapping BEFORE calling this (typically between
    /// `assemble_presentation` and this call) — `UiRealm` has no access to
    /// that registry (ADR-0037 §2), exactly the same split
    /// [`Self::close_presentation_entered`]'s own doc states for teardown.
    /// `runner.rs::install_presentation_alongside` is that caller in
    /// production; `Self::install_second_presentation_for_test`
    /// (`cfg(test)`-only, no stable link target in a non-test doc build) is
    /// the test-only counterpart for `UiRealm`-only tests that never touch
    /// `AppRuntime`/`WindowRegistry` at all.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "reachable only through runner.rs::install_presentation_alongside, itself \
                      desktop-only"
        )
    )]
    pub(crate) fn install_presentation(
        &mut self,
        presentation: PresentationState,
    ) -> PresentationId {
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
    /// `UiRealm`-only tests (this module's own `mod tests`, plus
    /// `runner.rs`'s dispatch-seam tests) that exercise a genuine
    /// multi-presentation realm WITHOUT ever touching `AppRuntime`'s
    /// `WindowRegistry` — that layer is a different module's own test
    /// concern (`runner.rs`'s `install_presentation_alongside`, exercised by
    /// the three tests that need a REAL window mapping for the second
    /// presentation). `pub(crate)` so `runner.rs`'s own tests that only need
    /// forest membership, never registry routing, can still use it.
    #[cfg(test)]
    pub(crate) fn install_second_presentation_for_test(&mut self) -> PresentationId {
        let window = Arc::new(crate::app::window_test_support::TestWindow::new().focused(false));
        let presentation = self.assemble_presentation(window);
        self.install_presentation(presentation)
    }

    /// The presentation keyboard input currently routes to
    /// ([`FocusCoordinator`]) — for the isolation suite proving
    /// `WindowFocus` events move it.
    #[cfg(test)]
    pub(crate) fn active_presentation_for_test(&self) -> PresentationId {
        self.focus_coordinator.active()
    }

    /// Whether `id`'s own [`FrameClock`](flui_scheduler::FrameClock) is
    /// currently marked hidden — for
    /// the isolation suite (`runner.rs`) proving `WindowVisibility` events
    /// gate exactly the presentation they were addressed to, never a
    /// sibling's. `None` if `id` is not resident (already closed, or a
    /// forged/mixed address).
    #[cfg(test)]
    pub(crate) fn presentation_hidden_for_test(&self, id: PresentationId) -> Option<bool> {
        self.presentations.get(id).map(|p| p.clock().is_hidden())
    }

    /// The primary presentation's own `FrameClock::produced_count` — for
    /// tests in a sibling module (`runner.rs`) that need to observe the
    /// clock's produce count without reaching into the private
    /// `presentations` field directly.
    #[cfg(test)]
    pub(crate) fn primary_produced_count_for_test(&self) -> u64 {
        self.presentations.primary().clock().produced_count()
    }

    /// This realm's own scheduler — the strong root. `runner.rs`'s
    /// realm-scoped lifecycle sites (`UiRealm::update_host_lifecycle`, the
    /// per-backend frame pumps) read through here instead of a process-global
    /// singleton; see [`Self::scheduler`]'s field doc for the ownership
    /// story.
    #[must_use]
    pub(crate) fn scheduler(&self) -> &flui_scheduler::UpdateScheduler {
        &self.scheduler
    }

    /// This realm's owner-local post-frame lane. `runner.rs`'s per-backend
    /// frame pumps pass this to `UpdateScheduler::drive_frame_with_lane` so
    /// the frame drive drains it in the same total order as the shared
    /// queue — drain-by-parameter, the same reason [`Self::scheduler`]
    /// exists rather than a process-global lookup.
    #[must_use]
    pub(crate) fn local_post_frame_lane(&self) -> &flui_scheduler::LocalPostFrameLane {
        &self.local_post_frame
    }

    /// Enter this realm's owner scope.
    ///
    /// A composite `GlobalKey` registry spanning every presentation this
    /// realm hosts (ADR-0043 §1) is active for the entire dynamic
    /// extent of `f`, including lifecycle/build callbacks — whole-frame,
    /// exactly as a single presentation's own registry always was: command
    /// drains, deferred arena resolutions, and hot-reload reassemble all
    /// resolve keys realm-wide regardless of which presentation's own
    /// segment is running. Nested entry is stack-shaped and panic unwinding
    /// restores the previously active realm.
    ///
    /// Does NOT activate the local post-frame lane: `LocalPostFrameHandle`
    /// addresses its lane directly (a `Weak` pointer minted once per
    /// presentation), so `schedule_local` needs no ambient "active lane"
    /// scope to succeed. The frame drive drains that lane by passing it
    /// explicitly to `UpdateScheduler::drive_frame_with_lane`/
    /// `end_frame_with_lane`, not by anything entered here.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        self.interaction_lane.enter(|| {
            let composite = GlobalKeyRegistryComposite::assemble(
                self.presentations.iter().map(PresentationState::widgets),
            );
            composite.enter(|| f(self))
        })
    }

    /// [`Self::enter`], but the composite excludes presentation `closing`.
    ///
    /// Used ONLY by [`Self::close_presentation_entered`]'s step 2–3 phase.
    /// `PresentationState::close`'s final step (`detach_root_widget`) holds
    /// `closing`'s own `WidgetsBindingInner` write lock for the entire
    /// recursive subtree removal — the exclusive access that walk needs to
    /// unmount every descendant. `GlobalKeyRegistryComposite`'s lookup tries
    /// each member in insertion order regardless of which key is being
    /// resolved (`build_composite`'s `for` loop runs until one member's
    /// `lookup_element` returns `Some`, trying every earlier member first),
    /// so if `closing` were composed in, a dispose hook's `GlobalKey::
    /// current_element()` call — for ANY key, even one belonging to a
    /// different presentation entirely — would have `closing`'s own lookup
    /// closure try to re-acquire a read lock on the exact `RwLock` this call
    /// already holds for writing, on the same thread: an unconditional
    /// self-deadlock (`parking_lot::RwLock` is not reentrant), not a race.
    /// Caught by this issue's own red-exploit test
    /// (`dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter`
    /// in `runner.rs`, which hung before this exclusion existed).
    ///
    /// Excluding `closing` costs nothing real: every OTHER presentation
    /// still resolves normally, and querying a presentation's OWN GlobalKey
    /// while that exact presentation's registry is mid-teardown has no
    /// principled answer anyway — the key is about to be unregistered by
    /// the same call regardless of what a lookup returns for it right now.
    fn enter_for_close<R>(&self, closing: PresentationId, f: impl FnOnce(&Self) -> R) -> R {
        self.interaction_lane.enter(|| {
            let composite = GlobalKeyRegistryComposite::assemble(
                self.presentations
                    .iter()
                    .filter(|presentation| presentation.id() != closing)
                    .map(PresentationState::widgets),
            );
            composite.enter(|| f(self))
        })
    }

    /// Owner-local widgets binding. Crate-private so callers cannot bypass the
    /// guarded realm entry boundary.
    #[cfg(any(
        test,
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    ))]
    pub(crate) fn widgets(&self) -> &flui_view::WidgetsBinding {
        self.presentations.primary().widgets()
    }

    /// The PRIMARY presentation's live root media-query source (see the field
    /// doc).
    ///
    /// Infallible by this realm's own invariant: a realm always hosts at least
    /// one presentation, and [`Self::presentation_id`] is that primary — so
    /// the root-attach paths that wrap a tree in `MediaQueryRoot` can call
    /// this directly. An event ADDRESSED to a particular presentation (a
    /// resize, safe-area or appearance report stamped at enqueue time) must
    /// use [`Self::media_query_for`] instead: that id travels through a queue
    /// shared by every presentation the realm hosts, so it can name a
    /// presentation that was closed before the event was delivered.
    pub(crate) fn media_query(
        &self,
    ) -> &std::rc::Rc<crate::app::media_query_root::MediaQuerySource> {
        &self.presentations.primary().media_query
    }

    /// The ADDRESSED presentation's live root media-query source, or `None`
    /// when this realm no longer hosts it.
    ///
    /// `None` is an ordinary outcome of addressed delivery, not a bug: an
    /// event is admitted when it is ENQUEUED (its dispatcher's address is in
    /// the registry then) and delivered later, in FIFO order, behind whatever
    /// was already queued. A close for that same presentation enqueued in
    /// between therefore runs first and removes it, leaving this lookup with
    /// nothing to write to. Every sibling addressed handler in this realm
    /// ([`Self::handle_input_addressed`], [`Self::update_window_focus`],
    /// [`Self::update_window_execution`]) treats that same interleaving the
    /// same way: drop the addressed effect, keep the realm-wide one.
    pub(crate) fn media_query_for(
        &self,
        id: PresentationId,
    ) -> Option<&std::rc::Rc<crate::app::media_query_root::MediaQuerySource>> {
        self.presentations
            .get(id)
            .map(|presentation| &presentation.media_query)
    }

    pub(crate) fn gestures(&self) -> &GestureBinding {
        self.presentations.primary().gestures()
    }

    /// Cancel in-flight pointer sequences on the ADDRESSED presentation's
    /// own gesture binding.
    ///
    /// Pointer input routes per presentation
    /// ([`Self::handle_input_addressed`] dispatches to
    /// `presentation.gestures()`), so a focus-loss cancellation must reach
    /// the same binding that presentation's Downs landed in — the
    /// realm-level [`Self::gestures`] wrapper is primary-only, and under a
    /// shared-realm window policy it would cancel a sibling presentation's
    /// sequences while leaving the defocused window's own drag stranded. A
    /// presentation this realm no longer hosts is a traced no-op, matching
    /// the addressed-input path's posture for the same race.
    pub(crate) fn cancel_pointer_sequences_for(&self, presentation_id: PresentationId) {
        let Some(presentation) = self.presentations.get(presentation_id) else {
            tracing::debug!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = presentation_id.as_u64(),
                "dropping a pointer-sequence cancellation addressed to a presentation this \
                 realm no longer hosts"
            );
            return;
        };
        presentation.gestures().cancel_active_pointers();
    }

    /// Focus state for the realm's current primary presentation. Since
    /// issue #555's addressed-routing slice, [`Self::handle_input_addressed`] reads the ADDRESSED
    /// presentation's own `focus_manager()` directly instead of through this
    /// primary-only wrapper; kept for the tests that still exercise a
    /// single-presentation realm's focus tree without addressing.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production input dispatch reads the addressed presentation's own \
                      focus_manager() directly (handle_input_addressed); this primary-only \
                      wrapper is exercised only by tests"
        )
    )]
    pub(crate) fn focus_manager(&self) -> Rc<FocusManager> {
        self.presentations.primary().focus_manager()
    }

    /// Weak text-input capability for this exact presentation.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn text_input_handle(&self) -> flui_interaction::TextInputHandle {
        self.presentations.primary().text_input_handle()
    }

    /// Reassemble EVERY presentation this realm hosts, in mount order —
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
    pub(crate) fn apply_hot_reload(&self, tier: flui_hot_reload::HotReloadTier) -> bool {
        let mut needs_redraw = false;
        for presentation in self.presentations.iter() {
            if presentation.apply_hot_reload(tier) {
                needs_redraw = true;
            }
        }
        needs_redraw
    }

    /// Apply a hot reload at the given tier (Flutter parity entry point),
    /// requesting a redraw if it actually changed anything. Moved here from
    /// the retired `AppBinding::perform_hot_reload_entered`.
    #[cfg(feature = "hot-reload")]
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by the desktop runner and tests, neither in the wasm lib check"
        )
    )]
    pub(crate) fn perform_hot_reload_entered(&self, tier: flui_hot_reload::HotReloadTier) {
        if self.apply_hot_reload(tier) {
            self.request_redraw();
        }
    }

    /// Gate `presentation_id`'s own
    /// [`FrameClock`](flui_scheduler::FrameClock) on a real visibility
    /// signal — the presentation-level counterpart of the
    /// realm-wide `AppLifecycleState` derivation
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
    /// retained mask, this call wakes the realm unconditionally rather than
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
                "dropping a visibility change for a presentation this realm no longer hosts"
            );
            return;
        };
        presentation.clock().set_hidden(hidden);
        if !hidden && !presentation.clock().demand_mask().is_empty() {
            self.wake_frame();
        }
    }

    /// Close and remove exactly one presentation from this realm's forest.
    ///
    /// Called from `runner.rs`'s `dispatch_platform_realm` loop, which
    /// special-cases `RealmTask::ClosePresentation` to call this with
    /// `&mut self` directly (the realm sits checked out of `AppRuntime`'s
    /// registry as an owned local at that point, so `&mut` is naturally
    /// available — no other caller reaches this method). That same
    /// dispatch has ALREADY set `dispatched_realm_id` for the whole
    /// checkout, so a dispose callback this method's own step 3 runs is
    /// covered by the identical TLS deferral guard every other realm-map
    /// mutation already respects: one deferral authority, not a second one
    /// to keep in sync with it. See `runner.rs::close_presentation` for the
    /// request-shaped public entry point (`enqueue + wake`) that gets here.
    ///
    /// A no-op (returns `false`) if `id` does not name a presentation this
    /// realm currently hosts.
    ///
    /// # Contract — six steps, in order
    ///
    /// 1. **Unregistering this presentation's window mapping from the
    ///    process-wide `WindowRegistry` is NOT this method's job.**
    ///    `UiRealm` has no access to that registry (`AppRuntime`-owned,
    ///    ADR-0037 §2's single authority) — the caller must have already
    ///    removed the registry entry before calling this. The real (and
    ///    today's only) caller is `dispatch_platform_realm`'s
    ///    `RealmTask::ClosePresentation` handling in `runner.rs`, NOT
    ///    `AppRuntime`'s realm-teardown path (`apply_uninstall`/
    ///    `teardown_platform_realm` close a WHOLE realm and never call this
    ///    method at all — a realm's own `Drop` closes every remaining
    ///    presentation directly). That dispatch handling unregisters this
    ///    exact presentation's own window mapping first when a SIBLING
    ///    presentation is staying open, and routes to a full realm
    ///    uninstall instead of calling this method at all when `id` names
    ///    the realm's only presentation (seeing [`Self::is_sole_presentation`]
    ///    return `true`) — this method's own contract never has to reason
    ///    about leaving the forest empty.
    /// 2. IME detach + focus deactivate — [`PresentationState::close`]'s
    ///    first half.
    /// 3. `detach_root_widget` through this exact presentation's own
    ///    `WidgetsBinding` — [`PresentationState::close`]'s second half —
    ///    run inside [`Self::enter_for_close`], so a `State::dispose()` a
    ///    descendant runs here gets the SAME realm-shared capabilities
    ///    (post-frame/interaction handles, TLS deferral) any other
    ///    frame/lifecycle callback gets, and can resolve a `GlobalKey`
    ///    living in any OTHER presentation this realm hosts. It cannot
    ///    resolve one of its OWN keys through the registry mid-teardown —
    ///    `enter_for_close` deliberately excludes `id` itself from the
    ///    composite it assembles; see that method's own doc for the
    ///    self-deadlock excluding it avoids. An install/uninstall request a
    ///    dispose hook makes here defers through the dispatched-path TLS
    ///    guard described above. Any BUILD SCHEDULING it does still routes
    ///    only within THIS presentation's own tree:
    ///    `RebuildHandle`/`ExternalBuildScheduler` are minted one-per-owner
    ///    and never shared, so a dispose hook has no path to a sibling
    ///    presentation's build inbox even if it tries (see
    ///    `dispose_during_teardown_cannot_reach_sibling_or_dead_services`).
    /// 4. **Async-task disposition:** the realm-level `AsyncDriver` is not
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
    ///    (whose drop triggers step 5), `RenderingFlutterBinding`, and every
    ///    other owned resource — drops after [`Self::enter_for_close`]
    ///    returns.
    ///
    /// # Why this is two calls, not one `&mut`-threaded closure
    ///
    /// [`Self::enter_for_close`] hands its closure `&Self` (shared) — every
    /// existing caller only ever needed read access to the realm for the
    /// duration of a frame/lifecycle callback, so it was never shaped to
    /// also hand out `&mut`. Steps 2–3 only need `&PresentationState`
    /// (`PresentationState::close` takes `&self`), so they run inside one
    /// `self.enter_for_close(...)` call. Steps 4–6 (the actual `Vec`
    /// removal + drop) need `&mut self.presentations`, which happens in a
    /// SEPARATE, sequential statement after that call returns — not nested
    /// inside it, so there is no borrow conflict and no need to reshape
    /// `enter_for_close` itself or give `PresentationForest` interior
    /// mutability.
    pub(crate) fn close_presentation_entered(&mut self, id: PresentationId) -> bool {
        if self.presentations.get(id).is_none() {
            return false;
        }
        let mut first_panic = catch_unwind(AssertUnwindSafe(|| {
            self.enter_for_close(id, |realm| {
                if realm.focus_coordinator.active() == id
                    && let Some(surviving) = realm.primary_id_excluding(id)
                {
                    realm.focus_coordinator.note_focus_gained(surviving);
                }
                realm.stop_presentation(id);
            });
        }))
        .err();
        // Membership removal must complete even when an observer or disposer panics.
        let removed = self.presentations.remove(id);
        let failure = catch_unwind(AssertUnwindSafe(|| drop(removed))).err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_panic,
            failure,
            "removed presentation cleanup",
        );
        let failure = catch_unwind(AssertUnwindSafe(|| self.synchronize_window_lifecycle())).err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_panic,
            failure,
            "surviving presentation lifecycle",
        );
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
        true
    }
}
