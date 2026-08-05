//! Owner-thread state for one presentation of a UI realm.
//!
//! This is deliberately crate-private. It is the UI-owner domain, not a
//! cross-thread god object: native event-loop ownership remains in the
//! runner/window host and raster/surface ownership remains in
//! `flui_engine::RasterOwner`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use flui_foundation::PresentationId;
use flui_interaction::{
    FocusManager, GestureBinding, InteractionDispatchHandle, TextInputHandle, TextInputOwner,
};
use flui_layer::{Layer, LayerTree, PerformanceOverlayLayer, PerformanceStats};
#[cfg(test)]
use flui_platform::traits::PlatformTextInput;
use flui_platform::{
    CursorIcon,
    traits::{CursorError, PlatformWindow},
};
use flui_rendering::binding::RendererBinding as _;
use flui_rendering::pipeline::PipelineCell;
#[cfg(test)]
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{AsyncDriver, PostFrameHandle, Scheduler};
use flui_semantics::{SemanticsActionError, SemanticsActionRequest};
use flui_types::HapticFeedback;
use flui_view::{GlobalKeyScope, WidgetsBinding};

use super::semantics_host::SemanticsHost;
use crate::bindings::RenderingFlutterBinding;

/// Realm-supplied capabilities threaded into a presentation at assembly
/// time (ADR-0043 §1): [`Self::global_key_scope`] is installed FIRST — the
/// underlying `BuildOwner::set_global_key_scope` setter panics with `BUG:`
/// if called after this owner's own `GlobalKey` registration has begun —
/// then the realm's shared dispatch handles, before this presentation's own
/// focus/IME are wired into its fresh `WidgetsBinding`, all before
/// attach/mount. See [`PresentationState::new`].
pub(crate) struct RealmCapabilities<'a> {
    /// The realm's cross-tree `GlobalKey` uniqueness domain (ADR-0043).
    pub(crate) global_key_scope: GlobalKeyScope,
    /// The realm's shared async-task driver (`build_owner.rs:296` is
    /// realm-level; see the presentation-teardown contract for the
    /// consequence of that when this presentation closes).
    pub(crate) async_driver: AsyncDriver,
    /// The realm's local post-frame callback lane.
    pub(crate) post_frame_handle: PostFrameHandle,
    /// The realm's interaction dispatch lane.
    pub(crate) interaction_dispatch_handle: InteractionDispatchHandle,
    /// The realm's own scheduler — borrowed only for the duration of
    /// assembly; the constructed [`RenderingFlutterBinding`] keeps just a
    /// `WeakScheduler` derived from it.
    pub(crate) scheduler: &'a Scheduler,
    /// The realm's platform wake capability, cloned into this
    /// presentation's pipeline as its `on_need_visual_update` callback.
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
}

/// A realm-backed test window carrying an optional platform text-input
/// capability — for `UiRealm::for_test_with_text_input`, which needs a real
/// [`RealmCapabilities`]-assembled presentation (not the standalone
/// [`PresentationState::new_for_test`] path), just with a test window.
#[cfg(test)]
pub(crate) fn test_platform_window(
    platform_text_input: Option<Arc<dyn PlatformTextInput>>,
) -> Arc<dyn PlatformWindow> {
    Arc::new(TestPresentationWindow::new(platform_text_input))
}

#[cfg(test)]
struct TestPresentationWindow {
    text_input: Option<Arc<dyn PlatformTextInput>>,
    cursor: parking_lot::Mutex<CursorIcon>,
}

#[cfg(test)]
impl TestPresentationWindow {
    fn new(text_input: Option<Arc<dyn PlatformTextInput>>) -> Self {
        Self {
            text_input,
            cursor: parking_lot::Mutex::new(CursorIcon::Default),
        }
    }
}

#[cfg(test)]
impl PlatformWindow for TestPresentationWindow {
    fn id(&self) -> flui_platform::traits::WindowId {
        flui_platform::traits::WindowId(1)
    }

    fn physical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
        flui_types::geometry::Size::default()
    }

    fn logical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::Pixels> {
        flui_types::geometry::Size::default()
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn request_redraw(&self) {}

    fn is_focused(&self) -> bool {
        true
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input.clone()
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        *self.cursor.lock() = cursor;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Lifecycle of the owner-thread half of a presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentationLifecycle {
    /// Identity exists, but no render surface is attached yet.
    ///
    /// Constructor-internal and production-unreachable once construction
    /// returns: `PresentationState::new` self-attaches its surface before
    /// handing the value back to its caller, so no arm dispatching on this
    /// state ever observes `Created` outside that constructor.
    Created,
    /// The presentation accepts input and produces frames.
    SurfaceAttached,
    /// The surface is retained but frame production is paused.
    Suspended,
    /// Teardown has started; new work is rejected.
    Closing,
    /// Owner-local resources have been released.
    Closed,
}

/// Direct owner of mutable UI state scoped to one presentation.
///
/// It owns behavior-bearing subsystems as concrete values. It does not expose
/// a provider trait, service locator, erased resource bag, or arbitrary
/// executor. Cross-thread ingress is handled by closed commands stamped with
/// this presentation's generational identity.
pub(crate) struct PresentationState {
    id: PresentationId,
    lifecycle: Cell<PresentationLifecycle>,
    pipeline: PipelineCell,
    window: Weak<dyn PlatformWindow>,
    gestures: GestureBinding,
    focus: Rc<FocusManager>,
    text_input: Rc<TextInputOwner>,
    /// This presentation's semantics enablement gate and platform
    /// accessibility delivery. `close()` clears its announce/event
    /// callbacks unconditionally (production write); `UiRealm::construct`
    /// reads `platform_semantics_enabled_handle()` to wire the realm's
    /// renderer fan-out (production read) — announce/event delivery itself
    /// still has no production caller (see [`Self::semantics_host`]'s doc).
    semantics: SemanticsHost,
    /// Owner-local widget framework state. One instance per presentation
    /// (ADR-0043) — the realm-level singular binding this used to be
    /// dissolves here; every widget-tree operation for this surface enters
    /// through this presentation and activates this binding's own GlobalKey
    /// registry (composed into the realm's whole-frame composite by
    /// `UiRealm::enter`, never activated standalone in production).
    widgets: WidgetsBinding,
    /// Render tree, layout/paint pipeline coordination, and this
    /// presentation's own semantics-enablement fan-out. Moved from the
    /// retired realm-level singular `UiRealm::renderer`: `render_views`,
    /// `first_frame_sent`, and the semantics-enabled listener are
    /// per-presentation-window facts, not shareable once a realm hosts more
    /// than one presentation.
    renderer: RenderingFlutterBinding,
    /// Total frames rendered successfully for this presentation. Moved here
    /// from the retired `AppBinding`: per-window frame accounting, beside
    /// its consumer [`Self::performance_overlay`].
    /// `Cell`, not `AtomicU64`: `PresentationState` is owner-thread-confined
    /// (`!Send` transitively, via `Rc`-backed fields), so an atomic buys
    /// nothing here that `Cell`'s cheaper interior mutability does not
    /// already give `&self` callers.
    frames_rendered: Cell<u64>,
    /// Frames dropped due to surface errors. See [`Self::frames_rendered`].
    frames_dropped: Cell<u64>,
    /// Performance-overlay state. `Some` IS the enable flag: the rolling
    /// frame-time window only exists while the overlay is on, so "enabled
    /// but no stats" is unrepresentable and a disabled overlay costs one
    /// `RefCell` borrow and a `None` check per frame. Moved here from the
    /// retired `AppBinding` — per-window stats, not a process-wide concern.
    performance_overlay: RefCell<Option<PerformanceStats>>,
}

impl PresentationState {
    /// Assemble the gesture arena, wiring its mouse-tracker cursor callback
    /// to `window` (shared by every constructor below — production and
    /// test alike — since the callback shape never varies with capability
    /// wiring).
    fn build_gestures(id: PresentationId, window: &Arc<dyn PlatformWindow>) -> GestureBinding {
        let gestures = GestureBinding::new();
        let cursor_window = Arc::downgrade(window);
        gestures
            .mouse_tracker()
            .set_cursor_change_callback(Rc::new(move |device_id, cursor| {
                let Some(window) = cursor_window.upgrade() else {
                    tracing::trace!(
                        ?id,
                        ?device_id,
                        ?cursor,
                        "dropping cursor update after the platform window closed"
                    );
                    return;
                };
                if let Err(error) = window.set_cursor(cursor) {
                    match error {
                        CursorError::Unsupported => {
                            tracing::trace!(
                                ?id,
                                ?device_id,
                                ?cursor,
                                "window backend has no pointer-cursor facility"
                            );
                        }
                        CursorError::Backend(_) => {
                            tracing::warn!(
                                ?id,
                                ?device_id,
                                ?cursor,
                                ?error,
                                "failed to apply the presentation cursor"
                            );
                        }
                    }
                }
            }));
        gestures
    }

    /// Assemble a presentation wired into a realm (ADR-0043 §1): installs
    /// `capabilities.global_key_scope` FIRST, then the realm's shared
    /// dispatch handles, before this presentation's own focus/IME are
    /// wired to its fresh [`WidgetsBinding`] and [`RenderingFlutterBinding`]
    /// — all before the caller ever attaches/mounts a root widget.
    pub(crate) fn new(
        id: PresentationId,
        pipeline: PipelineCell,
        window: Arc<dyn PlatformWindow>,
        capabilities: RealmCapabilities<'_>,
    ) -> Self {
        let gestures = Self::build_gestures(id, &window);
        let focus = FocusManager::new();
        let text_input = TextInputOwner::new(window.text_input());

        let widgets = WidgetsBinding::with_focus_manager(Rc::clone(&focus));
        widgets.set_pipeline_owner(pipeline.clone());
        widgets.with_build_owner_mut(|owner| {
            owner.set_global_key_scope(capabilities.global_key_scope);
            owner.set_async_driver(capabilities.async_driver);
            owner.set_post_frame_handle(capabilities.post_frame_handle);
            owner.set_interaction_dispatch_handle(capabilities.interaction_dispatch_handle);
            owner.set_text_input_handle(text_input.handle());
        });

        let renderer =
            RenderingFlutterBinding::new_with_pipeline(pipeline.clone(), capabilities.scheduler);

        // Idle-wake wiring: a dirty mark (mark_needs_layout / mark_needs_paint)
        // fires this callback so a quiescent event loop produces the frame.
        // Reentrancy-safe: the callback fires while the CALLER holds the
        // pipeline cell checked out, and `wake` only touches `Send + Sync`
        // runtime-level state — never this presentation's own `widgets` /
        // `renderer` / gesture state.
        let visual_wake = capabilities.wake;
        pipeline.with_mut(|owner| owner.set_on_need_visual_update(move || visual_wake()));

        let semantics = SemanticsHost::new();
        // Semantics-enabled fan-out -> this presentation's own SemanticsHost.
        let semantics_flag = semantics.platform_semantics_enabled_handle();
        renderer.add_semantics_enabled_listener(Arc::new(move |enabled| {
            semantics_flag.store(enabled, Ordering::Relaxed);
        }));

        let state = Self {
            id,
            lifecycle: Cell::new(PresentationLifecycle::Created),
            pipeline,
            window: Arc::downgrade(&window),
            gestures,
            focus,
            text_input,
            semantics,
            widgets,
            renderer,
            frames_rendered: Cell::new(0),
            frames_dropped: Cell::new(0),
            performance_overlay: RefCell::new(None),
        };
        state.attach_surface();
        state
    }

    /// Standalone assembly with no realm above it: this presentation's
    /// `WidgetsBinding` lazily self-owns a private `GlobalKeyScope` on first
    /// `GlobalKey` registration (never shared, so it never conflicts with
    /// anything), and its `RenderingFlutterBinding` owns its own throwaway
    /// `Scheduler` (see [`RenderingFlutterBinding::new_for_test_with_pipeline`]).
    /// Used only by this module's own unit tests, which exercise
    /// presentation-local behavior (gestures/focus/haptics/overlay) in
    /// isolation; realm-backed tests use [`Self::new`] through
    /// `UiRealm::for_test`, exactly like production.
    #[cfg(test)]
    pub(crate) fn new_for_test_with_window(
        id: PresentationId,
        pipeline: PipelineCell,
        window: Arc<dyn PlatformWindow>,
    ) -> Self {
        let gestures = Self::build_gestures(id, &window);
        let focus = FocusManager::new();
        let text_input = TextInputOwner::new(window.text_input());

        let widgets = WidgetsBinding::with_focus_manager(Rc::clone(&focus));
        widgets.set_pipeline_owner(pipeline.clone());

        let renderer = RenderingFlutterBinding::new_for_test_with_pipeline(pipeline.clone());

        let semantics = SemanticsHost::new();
        let semantics_flag = semantics.platform_semantics_enabled_handle();
        renderer.add_semantics_enabled_listener(Arc::new(move |enabled| {
            semantics_flag.store(enabled, Ordering::Relaxed);
        }));

        let state = Self {
            id,
            lifecycle: Cell::new(PresentationLifecycle::Created),
            pipeline,
            window: Arc::downgrade(&window),
            gestures,
            focus,
            text_input,
            semantics,
            widgets,
            renderer,
            frames_rendered: Cell::new(0),
            frames_dropped: Cell::new(0),
            performance_overlay: RefCell::new(None),
        };
        state.attach_surface();
        state
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        id: PresentationId,
        pipeline: PipelineCell,
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let window: Arc<dyn PlatformWindow> =
            Arc::new(TestPresentationWindow::new(platform_text_input));
        Self::new_for_test_with_window(id, pipeline, window)
    }

    /// This presentation's own widget framework state.
    #[must_use]
    pub(crate) fn widgets(&self) -> &WidgetsBinding {
        &self.widgets
    }

    /// This presentation's own render tree / pipeline coordination binding.
    #[must_use]
    pub(crate) fn renderer(&self) -> &RenderingFlutterBinding {
        &self.renderer
    }

    #[must_use]
    pub(crate) fn id(&self) -> PresentationId {
        self.id
    }

    #[must_use]
    pub(crate) fn lifecycle(&self) -> PresentationLifecycle {
        self.lifecycle.get()
    }

    #[must_use]
    pub(crate) fn pipeline(&self) -> &PipelineCell {
        &self.pipeline
    }

    #[must_use]
    pub(crate) fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    /// The exact focus tree owned by this presentation.
    #[must_use]
    pub(crate) fn focus_manager(&self) -> Rc<FocusManager> {
        Rc::clone(&self.focus)
    }

    #[must_use]
    pub(crate) fn text_input(&self) -> &TextInputOwner {
        &self.text_input
    }

    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Self::new wires set_text_input_handle from the local \
                      text_input binding directly (before self exists to call \
                      this wrapper through); this accessor's one production \
                      caller moved inline when assembly moved into this \
                      constructor, kept for tests and any future external caller"
        )
    )]
    pub(crate) fn text_input_handle(&self) -> TextInputHandle {
        self.text_input.handle()
    }

    /// This presentation's semantics enablement gate and platform
    /// accessibility delivery — the per-window home the retired
    /// `SemanticsBinding` singleton's enablement/announce/event state moved
    /// into. `Self::new` reads the underlying flag directly (before `self`
    /// exists to call this wrapper through) to wire the renderer's
    /// semantics-enabled fan-out; announce/event delivery itself still has
    /// no production caller (future platform-embedder wiring).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this accessor's one production caller moved inline into \
                      Self::new's own assembly; kept for tests and any future \
                      external caller"
        )
    )]
    pub(crate) fn semantics_host(&self) -> &SemanticsHost {
        &self.semantics
    }

    // ========================================================================
    // Window access, haptics (moved from the retired `AppBinding`)
    // ========================================================================

    /// Access the presentation's window, if it is still live.
    ///
    /// `window` is `Weak`: the platform owns the strong `Arc`, and this
    /// presentation must not keep it alive past the platform's own teardown.
    /// Returns `None` once the window has been dropped — the same
    /// degradation the retired `AppBinding::with_window` used before any
    /// window was installed; here it is "the window this presentation was
    /// built with is gone" instead of "no window installed yet", since a
    /// presentation always has one from construction.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read only by perform_haptic_feedback, itself unreached \
                      outside tests until a production caller wires haptics \
                      through a presentation"
        )
    )]
    pub(crate) fn with_window<R>(&self, f: impl FnOnce(&dyn PlatformWindow) -> R) -> Option<R> {
        self.window.upgrade().map(|window| f(window.as_ref()))
    }

    /// Perform haptic feedback on this presentation's window, via
    /// [`PlatformWindow::haptics`].
    ///
    /// Silent no-op — no panic, no error — when the window is gone, or the
    /// window's backend has no [`PlatformHaptics`](flui_platform::traits::PlatformHaptics)
    /// capability (desktop winit targets, for instance). Mirrors Flutter's own `HapticFeedback`
    /// degradation contract: every call is fire-and-forget best-effort, with
    /// no availability-discovery API to check first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- haptics through a \
                      presentation is future wiring, forwarded today only by \
                      UiRealm::perform_haptic_feedback (also uncalled in \
                      production)"
        )
    )]
    pub(crate) fn perform_haptic_feedback(&self, feedback: HapticFeedback) {
        let haptics = self.with_window(|window| window.haptics()).flatten();
        if let Some(haptics) = haptics {
            haptics.perform(feedback);
        }
    }

    // ========================================================================
    // Frame accounting and the performance overlay (moved from the retired
    // `AppBinding`)
    // ========================================================================

    /// Total frames rendered successfully.
    pub(crate) fn frames_rendered(&self) -> u64 {
        self.frames_rendered.get()
    }

    /// Frames dropped due to surface errors.
    #[expect(
        dead_code,
        reason = "no production caller yet, and no test reads it back -- \
                  forwarded only by UiRealm::frames_dropped (also uncalled); \
                  record_frame_dropped is the production write side"
    )]
    pub(crate) fn frames_dropped(&self) -> u64 {
        self.frames_dropped.get()
    }

    /// Record a successfully presented frame.
    pub(crate) fn record_frame_rendered(&self) {
        self.frames_rendered.set(self.frames_rendered.get() + 1);
    }

    /// Record a frame dropped due to a surface error.
    pub(crate) fn record_frame_dropped(&self) {
        self.frames_dropped.set(self.frames_dropped.get() + 1);
    }

    /// Turn the performance overlay on or off. Enabling starts a fresh
    /// rolling window, so toggling it at runtime does not report frame times
    /// from before the toggle.
    pub(crate) fn set_performance_overlay(&self, enabled: bool) {
        *self.performance_overlay.borrow_mut() = enabled.then(PerformanceStats::default);
    }

    /// Record this frame and append the overlay layer to `layer_tree`.
    ///
    /// No-op when the overlay is off, or when the tree has no root to parent
    /// the overlay under (a frame that painted nothing). The overlay is
    /// added as the root's LAST child so it composites above the
    /// presentation's own content.
    pub(crate) fn attach_performance_overlay(&self, layer_tree: &mut LayerTree) {
        let mut slot = self.performance_overlay.borrow_mut();
        let Some(stats) = slot.as_mut() else {
            return;
        };
        let Some(root) = layer_tree.root() else {
            return;
        };

        stats.record_frame();

        let mut overlay =
            PerformanceOverlayLayer::all_stats(PerformanceOverlayLayer::default_bounds());
        overlay.update_stats(stats);

        let overlay_id = layer_tree.insert(Layer::PerformanceOverlay(Box::new(overlay)));
        // `insert` does not link the node — parent and child sides are set
        // explicitly, same as every other layer-tree insertion.
        if let Some(node) = layer_tree.get_mut(overlay_id) {
            node.set_parent(Some(root));
        }
        if let Some(root_node) = layer_tree.get_mut(root) {
            root_node.add_child(overlay_id);
        }
    }

    fn attach_surface(&self) {
        if self.lifecycle.get() == PresentationLifecycle::Created {
            self.lifecycle.set(PresentationLifecycle::SurfaceAttached);
        }
    }

    pub(crate) fn suspend(&self) {
        if self.lifecycle.get() == PresentationLifecycle::SurfaceAttached {
            self.lifecycle.set(PresentationLifecycle::Suspended);
        }
    }

    pub(crate) fn resume(&self) {
        if self.lifecycle.get() == PresentationLifecycle::Suspended {
            self.lifecycle.set(PresentationLifecycle::SurfaceAttached);
        }
    }

    /// Resolve an accessibility action through this presentation's exact
    /// semantics owner, then invoke it after releasing the pipeline
    /// checkout.
    ///
    /// `debug_assert!(is_free())` makes the Idle-commit contract this
    /// dispatch site depends on an explicit, production-checked invariant:
    /// `UiRealm::drain_commands` (the sole caller) only runs at a frame
    /// boundary, so nothing should still hold the pipeline checked out by
    /// the time a semantics-action handler runs. Registry:
    /// `runtime-contract.toml`'s `semantics-two-phase-borrow` contract.
    pub(crate) fn dispatch_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<(), SemanticsActionError> {
        if matches!(
            self.lifecycle.get(),
            PresentationLifecycle::Closing | PresentationLifecycle::Closed
        ) {
            return Err(SemanticsActionError::PresentationClosed);
        }
        let invocation = self
            .pipeline
            .with(|pipeline| pipeline.resolve_semantics_action(request))?;
        debug_assert!(
            self.pipeline.is_free(),
            "BUG: the pipeline must be free before invoking a semantics-action handler — \
             drain_commands runs only at a frame boundary, so nothing should still hold it \
             checked out here"
        );
        invocation.invoke();
        Ok(())
    }

    /// Apply a hot-reload tier to this presentation's own element tree.
    /// Returns whether a redraw is required.
    #[cfg(feature = "hot-reload")]
    pub(crate) fn apply_hot_reload(&self, tier: flui_hot_reload::HotReloadTier) -> bool {
        use flui_hot_reload::HotReloadTier;

        match tier {
            HotReloadTier::HotReload => {
                self.widgets.perform_reassemble();
                self.pipeline
                    .with_mut(flui_rendering::pipeline::PipelineOwner::reassemble);
                tracing::info!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                    "hot reload reassembled element and render trees"
                );
                true
            }
            HotReloadTier::HotRestart => {
                tracing::warn!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                    "HotRestart root remount is not implemented; applying reassemble"
                );
                self.widgets.perform_reassemble();
                self.pipeline
                    .with_mut(flui_rendering::pipeline::PipelineOwner::reassemble);
                true
            }
            HotReloadTier::FullRestart => {
                tracing::debug!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                    "FullRestart is owned by the CLI process supervisor"
                );
                false
            }
        }
    }

    /// Begin deterministic owner-local teardown.
    pub(crate) fn close(&self) {
        match self.lifecycle.get() {
            PresentationLifecycle::Closing | PresentationLifecycle::Closed => return,
            PresentationLifecycle::Created
            | PresentationLifecycle::SurfaceAttached
            | PresentationLifecycle::Suspended => {}
        }
        self.lifecycle.set(PresentationLifecycle::Closing);

        self.gestures.cancel_all_pointer_sequences();
        self.gestures.mouse_tracker().clear_cursor_change_callback();
        // A stray announce/event racing this teardown must not reach a
        // platform accessibility bridge that is itself about to go away —
        // see `SemanticsHost::clear_announce_callback`'s doc for the
        // announce-after-close decision this pins.
        self.semantics.clear_announce_callback();
        self.semantics.clear_event_callback();
        if let Some(window) = self.window.upgrade()
            && let Err(error) = window.set_cursor(CursorIcon::Default)
            && !matches!(error, CursorError::Unsupported)
        {
            tracing::warn!(
                { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                ?error,
                "failed to restore the default cursor while closing the presentation"
            );
        }
        self.focus.close();
        self.text_input.close();
        self.lifecycle.set(PresentationLifecycle::Closed);
    }
}

impl std::fmt::Debug for PresentationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationState")
            .field("id", &self.id)
            .field("lifecycle", &self.lifecycle.get())
            .finish_non_exhaustive()
    }
}

impl Drop for PresentationState {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    static_assertions::assert_not_impl_any!(PresentationState: Send, Sync);

    fn presentation() -> PresentationState {
        PresentationState::new_for_test(
            PresentationId::new_gen(0, NonZeroU32::MIN),
            PipelineCell::new(PipelineOwner::new()),
            None,
        )
    }

    #[test]
    fn lifecycle_transitions_are_typed_and_close_is_idempotent() {
        let presentation = presentation();
        assert_eq!(
            presentation.lifecycle(),
            PresentationLifecycle::SurfaceAttached
        );

        presentation.suspend();
        assert_eq!(presentation.lifecycle(), PresentationLifecycle::Suspended);
        presentation.resume();
        assert_eq!(
            presentation.lifecycle(),
            PresentationLifecycle::SurfaceAttached
        );

        presentation.close();
        presentation.close();
        assert_eq!(presentation.lifecycle(), PresentationLifecycle::Closed);
    }

    /// If reverted: remove the lifecycle check from `dispatch_semantics_action`
    /// and this fails with `Ok(())` instead (the request would resolve
    /// against a node id that happens not to exist, which is a different,
    /// pre-existing refusal path — `PresentationClosed` must fire first).
    #[test]
    fn semantics_action_after_close_is_refused() {
        use flui_semantics::{AccessibilityNodeId, SemanticsAction};

        let presentation = presentation();
        presentation.close();

        let request = SemanticsActionRequest::new(
            AccessibilityNodeId::from(flui_foundation::RenderId::new(1)),
            SemanticsAction::Tap,
        );
        assert_eq!(
            presentation.dispatch_semantics_action(request),
            Err(SemanticsActionError::PresentationClosed)
        );
    }

    #[test]
    fn semantics_host_is_exclusive_to_this_presentation() {
        let a = presentation();
        let b = presentation();

        assert!(!a.semantics_host().semantics_enabled());
        assert!(!b.semantics_host().semantics_enabled());

        let handle = a.semantics_host().ensure_semantics();
        assert!(a.semantics_host().semantics_enabled());
        assert!(
            !b.semantics_host().semantics_enabled(),
            "a's SemanticsHandle must not enable b's independently-owned SemanticsHost"
        );

        drop(handle);
        assert!(!a.semantics_host().semantics_enabled());
    }

    #[test]
    fn text_input_handle_is_bound_to_the_owned_text_input_state() {
        let presentation = presentation();
        let handle = presentation.text_input_handle();

        presentation.close();

        assert_eq!(
            handle.attach(Rc::new(|_| {})),
            Err(flui_interaction::TextInputError::Closed)
        );
    }

    #[test]
    fn mouse_tracker_applies_cursor_to_the_exact_owned_window() {
        use flui_foundation::RenderId;
        use flui_interaction::{
            events::{PointerType, make_move_event},
            routing::{HitTestEntry, HitTestResult, PointerMotionKind},
        };
        use flui_types::geometry::{Offset, Pixels};

        let window = Arc::new(TestPresentationWindow::new(None));
        let platform_window: Arc<dyn PlatformWindow> = window.clone();
        let presentation = PresentationState::new_for_test_with_window(
            PresentationId::new_gen(0, NonZeroU32::MIN),
            PipelineCell::new(PipelineOwner::new()),
            platform_window,
        );
        let position = Offset::new(Pixels(12.0), Pixels(8.0));
        let event = make_move_event(position, PointerType::Mouse);
        let mut hit_test = HitTestResult::new();
        hit_test.add(HitTestEntry::new(RenderId::new(1)).cursor(CursorIcon::Pointer));

        presentation.gestures().mouse_tracker().update_with_motion(
            &event,
            PointerMotionKind::Hover,
            &hit_test,
        );

        assert_eq!(*window.cursor.lock(), CursorIcon::Pointer);
        presentation.close();
        assert_eq!(*window.cursor.lock(), CursorIcon::Default);
    }

    // ========================================================================
    // Haptics — migrated from the retired `AppBinding`'s test module
    // (`binding.rs`, deleted alongside it).
    // ========================================================================
    mod haptics_capability {
        use super::*;

        fn headless_window_with_haptics() -> (
            Arc<dyn PlatformWindow>,
            Arc<dyn flui_platform::traits::PlatformHaptics>,
        ) {
            let platform = flui_platform::headless_platform();
            let window = platform
                .open_window(flui_platform::traits::WindowOptions::default())
                .expect("headless platform always opens a window");
            let haptics = window
                .haptics()
                .expect("headless backend supports PlatformHaptics");
            (window, haptics)
        }

        fn fake_haptics(
            haptics: &Arc<dyn flui_platform::traits::PlatformHaptics>,
        ) -> &flui_platform::FakeHaptics {
            haptics
                .as_any()
                .downcast_ref::<flui_platform::FakeHaptics>()
                .expect("the headless backend's PlatformHaptics is a FakeHaptics")
        }

        /// A minimal `PlatformWindow` implementing only the trait's required
        /// methods (`as_any` has no default; everything else here does), so
        /// `haptics()` falls through to the trait default (`None`).
        struct BareWindow;

        impl PlatformWindow for BareWindow {
            fn id(&self) -> flui_platform::traits::WindowId {
                flui_platform::traits::WindowId(1)
            }

            fn physical_size(
                &self,
            ) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
                flui_types::geometry::Size::default()
            }

            fn logical_size(&self) -> flui_types::geometry::Size<flui_types::Pixels> {
                flui_types::geometry::Size::default()
            }

            fn scale_factor(&self) -> f64 {
                1.0
            }

            fn request_redraw(&self) {}

            fn is_focused(&self) -> bool {
                false
            }

            fn is_visible(&self) -> bool {
                true
            }

            fn set_cursor(
                &self,
                _cursor: flui_platform::CursorIcon,
            ) -> Result<(), flui_platform::CursorError> {
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        /// Real-path proof: `perform_haptic_feedback` reads the
        /// presentation's window and calls through to its `PlatformHaptics`.
        #[test]
        fn perform_haptic_feedback_reaches_the_active_windows_platform_capability() {
            let (window, haptics) = headless_window_with_haptics();
            let fake = fake_haptics(&haptics);

            // `PresentationState.window` is a `Weak` (the platform owns the
            // strong `Arc` in production); this test's own `window` binding
            // is what keeps it alive here, so pass a clone rather than
            // moving the only strong reference in.
            let presentation = PresentationState::new_for_test_with_window(
                PresentationId::new_gen(0, NonZeroU32::MIN),
                PipelineCell::new(PipelineOwner::new()),
                Arc::clone(&window),
            );

            presentation.perform_haptic_feedback(HapticFeedback::SelectionClick);

            assert_eq!(
                fake.calls(),
                vec![HapticFeedback::SelectionClick],
                "perform_haptic_feedback must call through to the window's \
                 PlatformHaptics::perform"
            );
        }

        /// The presentation's window has been dropped (the platform side let
        /// go of its strong `Arc`) — a silent no-op, no panic. This is the
        /// per-presentation equivalent of the retired
        /// `AppBinding::perform_haptic_feedback_with_no_active_window_is_a_silent_no_op`:
        /// a presentation always has SOME window from construction, so "no
        /// window" here means "the window this presentation was built with
        /// is gone", not "never installed".
        #[test]
        fn perform_haptic_feedback_with_no_active_window_is_a_silent_no_op() {
            let window: Arc<dyn PlatformWindow> = Arc::new(BareWindow);
            let presentation = PresentationState::new_for_test_with_window(
                PresentationId::new_gen(0, NonZeroU32::MIN),
                PipelineCell::new(PipelineOwner::new()),
                Arc::clone(&window),
            );
            drop(window);

            presentation.perform_haptic_feedback(HapticFeedback::Vibrate);
        }

        /// A window whose backend has no `PlatformHaptics` capability
        /// (desktop winit's shape, reproduced here without a real display)
        /// is also a silent no-op.
        #[test]
        fn perform_haptic_feedback_on_a_window_without_haptics_is_a_silent_no_op() {
            let presentation = PresentationState::new_for_test_with_window(
                PresentationId::new_gen(0, NonZeroU32::MIN),
                PipelineCell::new(PipelineOwner::new()),
                Arc::new(BareWindow),
            );

            presentation.perform_haptic_feedback(HapticFeedback::MediumImpact);
        }
    }

    // ========================================================================
    // Performance overlay — migrated from the retired `AppBinding`'s test
    // module.
    // ========================================================================
    mod performance_overlay_wiring {
        use flui_layer::CanvasLayer;

        use super::*;

        fn tree_with_root() -> (LayerTree, flui_layer::LayerId) {
            let mut tree = LayerTree::new();
            let root = tree.insert(Layer::Canvas(Box::new(CanvasLayer::new())));
            tree.set_root(Some(root));
            (tree, root)
        }

        #[test]
        fn overlay_off_leaves_the_layer_tree_untouched() {
            let presentation = presentation();
            let (mut tree, root) = tree_with_root();

            presentation.attach_performance_overlay(&mut tree);

            assert_eq!(tree.len(), 1, "no layer may be added while overlay is off");
            assert!(
                tree.get(root).expect("root node").children().is_empty(),
                "root must keep no children while overlay is off"
            );
        }

        #[test]
        fn overlay_on_appends_a_linked_overlay_layer_to_the_root() {
            let presentation = presentation();
            presentation.set_performance_overlay(true);
            let (mut tree, root) = tree_with_root();

            presentation.attach_performance_overlay(&mut tree);

            assert_eq!(tree.len(), 2, "exactly one overlay layer is added");
            let children = tree.get(root).expect("root node").children();
            let overlay_id = *children.last().expect("overlay is the root's last child");
            assert!(
                tree.get_layer(overlay_id)
                    .expect("overlay layer")
                    .is_performance_overlay(),
                "the appended layer is the performance overlay"
            );
            assert_eq!(
                tree.get(overlay_id).expect("overlay node").parent(),
                Some(root),
                "the overlay's parent side must be linked too, not just the root's child list"
            );
        }

        #[test]
        fn overlay_on_a_rootless_tree_is_a_no_op() {
            let presentation = presentation();
            presentation.set_performance_overlay(true);
            let mut tree = LayerTree::new();

            presentation.attach_performance_overlay(&mut tree);

            assert_eq!(tree.len(), 0, "nothing to parent the overlay under");
        }

        #[test]
        fn disabling_the_overlay_stops_appending_and_resets_the_window() {
            let presentation = presentation();
            presentation.set_performance_overlay(true);
            let (mut tree, _root) = tree_with_root();
            presentation.attach_performance_overlay(&mut tree);
            presentation.attach_performance_overlay(&mut tree);
            assert_eq!(tree.len(), 3, "two frames, two overlay layers");

            presentation.set_performance_overlay(false);
            let (mut fresh, _) = tree_with_root();
            presentation.attach_performance_overlay(&mut fresh);
            assert_eq!(fresh.len(), 1, "disabled overlay adds nothing");

            presentation.set_performance_overlay(true);
            let (mut again, _) = tree_with_root();
            presentation.attach_performance_overlay(&mut again);
            let overlay_id = *tree_root_children(&again).last().expect("overlay present");
            let overlay = again.get_layer(overlay_id).expect("overlay layer");
            let stats = overlay
                .as_performance_overlay()
                .expect("performance overlay variant");
            assert_eq!(
                stats.total_frames(),
                1,
                "re-enabling starts a fresh window rather than resuming the old count"
            );
        }

        fn tree_root_children(tree: &LayerTree) -> Vec<flui_layer::LayerId> {
            let root = tree.root().expect("root");
            tree.get(root).expect("root node").children().to_vec()
        }
    }
}
