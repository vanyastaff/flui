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

use flui_animation::Vsync;
use flui_foundation::PresentationId;
use flui_interaction::{
    FocusManager, GestureBinding, InteractionDispatchHandle, TextInputHandle, TextInputOwner,
};
use flui_layer::{LayerTree, PerformanceOverlayLayer, PerformanceStats};
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
use flui_scheduler::{
    AsyncDriver, FrameClock, LocalPostFrameHandle, PostFrameHandle, UpdateScheduler,
    input_to_present_histogram, produce_to_present_histogram,
};
use flui_semantics::{
    AccessibilityNodeId, SemanticsActionError, SemanticsActionRequest, semantics_action_args_for,
    semantics_action_for,
};
use flui_types::HapticFeedback;
use flui_view::{GlobalKeyScope, WidgetsBinding};
use web_time::{Duration, Instant};

use super::semantics_host::SemanticsHost;
use crate::bindings::RenderingFlutterBinding;

fn format_millis(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1_000.0)
}

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
    /// The realm's owner-local post-frame callback capability — addresses
    /// the realm's [`flui_scheduler::LocalPostFrameLane`] directly, so it can
    /// capture `Rc`/`RefCell` widget state.
    pub(crate) local_post_frame_handle: LocalPostFrameHandle,
    /// The realm's interaction dispatch lane.
    pub(crate) interaction_dispatch_handle: InteractionDispatchHandle,
    /// The realm's own scheduler — borrowed only for the duration of
    /// assembly; the constructed [`RenderingFlutterBinding`] keeps just a
    /// `WeakUpdateScheduler` derived from it.
    pub(crate) scheduler: &'a UpdateScheduler,
    /// The realm's platform wake capability, cloned into this
    /// presentation's pipeline as its `on_need_visual_update` callback.
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    /// A cross-thread sender into the realm's command inbox, already
    /// stamped with this presentation's id. Handed to the platform
    /// accessibility bridge's action listener, so an assistive-technology
    /// request marshals onto the owner thread as a
    /// [`SemanticsActionRequest`] and resolves at the next Idle drain —
    /// never on the adapter's own thread.
    pub(crate) command_sender: super::ui_realm::UiCommandSender,
}

/// A realm-backed test window carrying an optional platform text-input
/// capability — for `UiRealm::for_test_with_text_input`, which needs a real
/// [`RealmCapabilities`]-assembled presentation (not the standalone
/// [`PresentationState::new_for_test`] path), just with a test window.
#[cfg(test)]
pub(crate) fn test_platform_window(
    platform_text_input: Option<Arc<dyn PlatformTextInput>>,
) -> Arc<dyn PlatformWindow> {
    use super::window_test_support::TestWindow;
    Arc::new(
        TestWindow::new()
            .focused(true)
            .with_text_input(platform_text_input),
    )
}

/// A realm-backed test window whose accessibility capability is the given
/// recording fake — for tests exercising the platform-accessibility wire
/// ([`PresentationState::wire_platform_accessibility`]) end-to-end while
/// keeping a typed handle on the fake to drive activation and actions.
#[cfg(test)]
pub(crate) fn test_platform_window_with_accessibility(
    accessibility: Arc<flui_platform::FakeAccessibility>,
) -> Arc<dyn PlatformWindow> {
    use super::window_test_support::TestWindow;
    Arc::new(
        TestWindow::new()
            .focused(true)
            .with_accessibility(accessibility),
    )
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
    /// This presentation's own wake-only redraw mark (ADR-0043 §3's pump
    /// segment). Set alongside the realm's own coalesced `needs_redraw` flag
    /// by every presentation-scoped operation that isn't otherwise
    /// re-derivable from live pipeline/build state (e.g. `attach_root_widget`
    /// scheduling the very first build); cleared at the START of this
    /// presentation's pump segment, before dirty is sampled, so a mark
    /// arriving WHILE the segment runs sets the bit again instead of being
    /// lost. This bit is wake-only, never the truth by itself: the segment's
    /// real dirty predicate is `take_redraw_pending() ||
    /// widgets().has_pending_builds() || <pipeline has_dirty_nodes>`
    /// (`Self::has_pending_work`) — see `UiRealm::draw_frame_entered`'s
    /// per-presentation loop.
    redraw_pending: Cell<bool>,
    /// This presentation's own controller registry for implicit animations
    /// (moved from the realm-level `UiRealm::vsync_slot`, issue #556: each
    /// surface paces its own animations independently). `RefCell`, not a
    /// plain field — mirrors `UiRealm::vsync_slot`'s old `Mutex`: `Self::
    /// set_vsync` replaces the whole handle through `&self`, and the
    /// per-frame `tick_all`/`has_running` calls operate on a cloned `Vsync`
    /// handle (sharing the inner `Arc<Mutex<VsyncInner>>`), so this cell is
    /// only ever borrowed for the length of a clone or a swap.
    vsync: RefCell<Vsync>,
    /// This presentation's own physical-time produce-gate state machine
    /// (issue #556) — the per-presentation half of the `UpdateScheduler`/
    /// `FrameClock`/raster three-owner split. `UiRealm::draw_frame_entered`'s
    /// per-presentation segment loop polls this instead of the old
    /// `take_redraw_pending() || has_pending_work()` predicate directly;
    /// first-frame deferral (`RenderingFlutterBinding::send_frames_to_engine`'s
    /// old counter) folds into this same clock, withholding only the
    /// submit — see `FrameClock`'s own module doc for the `.flutter/`
    /// citation that pins this.
    clock: FrameClock,
    /// (segment start, segment end) for the most recently completed
    /// build+layout+paint segment `UiRealm::draw_frame_entered`'s
    /// per-presentation loop ran for THIS presentation — a side channel for
    /// a caller whose segment-running step and submit-deciding step are two
    /// separate calls (`UiRealm::draw_frame_entered` runs the segment;
    /// `UiRealm::render_frame_entered`, its caller, decides whether/how to
    /// submit and is where `FrameClock::record_frame` actually runs, for
    /// whichever presentation's segment produced the outcome being
    /// submitted — see that call site's own doc). Lives here, not on the
    /// shared `FrameClock` (issue #556 review): a `pub` field on a type
    /// every presentation shares would let one presentation's caller read
    /// or clobber a SIBLING's in-flight span; keeping it private to this
    /// presentation makes that structurally impossible. [`Self::
    /// take_last_segment_span`] reads AND CLEARS it (never a plain `get`):
    /// a pump where this presentation's segment did NOT run must see
    /// `None` here, never a stale span latched by an earlier pump this
    /// presentation was the one to produce.
    last_segment_span: Cell<Option<(Instant, Instant)>>,
    /// How many frames IN A ROW have failed for this presentation — the
    /// `consecutive_failures` field of every
    /// [`FrameFailureReport`](super::frame_failure::FrameFailureReport)
    /// this presentation's failures produce. Incremented by `UiRealm::
    /// report_frame_failure` (both the structured-pipeline-error and the
    /// caught-segment-panic routes), reset by the next segment that
    /// completes without failing. Presentation-local on purpose: one
    /// window's failure streak must never color a sibling's reports.
    frame_failure_streak: Cell<u32>,
    /// Test-only fault injection: when set, runs at the top of this
    /// presentation's build+layout+paint segment (`UiRealm::
    /// draw_frame_for_presentation`), where a panic it raises escapes
    /// every inner recovery layer and reaches the realm's per-presentation
    /// `catch_unwind` boundary — the controllable stand-in for real
    /// escape paths (e.g. a panicking `ViewState::dispose` during tree
    /// finalization) that are hard to re-trigger repeatedly.
    #[cfg(test)]
    segment_probe: RefCell<Option<Box<dyn Fn()>>>,
    /// Test-only oracle: how many times this presentation's own
    /// build+layout+paint segment actually ran (`UiRealm::
    /// draw_frame_for_presentation`), regardless of whether anything was
    /// rebuilt or a scene reached present. This is the "flush count" the
    /// isolation suite's sibling-independence tests read — a rebuild count
    /// would not prove independence (a presentation with a settled, never-
    /// rebuilding tree still flushes every segment it runs), and a
    /// present-count would conflate this with GPU backend availability.
    #[cfg(test)]
    flush_count: Cell<u32>,
}

impl PresentationState {
    /// Wire this window's platform accessibility bridge, when one exists,
    /// into all three directions of the semantics seam:
    ///
    /// - **Out** — every `SemanticsOwner` this pipeline creates publishes
    ///   its translated tree to the platform. The callback holds the bridge
    ///   `Weak`, so a flush racing window teardown degrades to a drop,
    ///   never a call into a dead adapter.
    /// - **Activation** — assistive technology attaching or detaching
    ///   toggles this presentation's [`SemanticsHost`] flag and wakes the
    ///   loop; the frame pump's reconcile (`UiRealm::draw_frame_entered`)
    ///   then drives `PipelineOwner::set_semantics_enabled`, so tree
    ///   assembly starts and stops on the OWNER thread — the listener runs
    ///   on the adapter's own thread and touches only `Send + Sync` state.
    /// - **In** — action requests marshal through the realm inbox as
    ///   [`SemanticsActionRequest`]s stamped for this exact presentation
    ///   and resolve at the next Idle drain. Requests FLUI cannot route (a
    ///   zero node id, an action with no counterpart, a full inbox) are
    ///   traced drops, mirroring how Flutter tolerates screen readers
    ///   acting on a stale snapshot. Typed action payloads
    ///   (`accesskit::ActionData`) translate via
    ///   [`semantics_action_args_for`]; a payload kind FLUI cannot express
    ///   routes the action argument-free with a trace rather than killing
    ///   the whole request.
    ///
    /// A window without the capability (`accessibility()` → `None`) wires
    /// nothing: the pipeline keeps its documented publish-nowhere
    /// placeholder.
    fn wire_platform_accessibility(
        window: &Arc<dyn PlatformWindow>,
        pipeline: &PipelineCell,
        semantics: &SemanticsHost,
        wake: &Arc<dyn Fn() + Send + Sync>,
        command_sender: super::ui_realm::UiCommandSender,
    ) {
        let Some(bridge) = window.accessibility() else {
            return;
        };

        let publish_bridge = Arc::downgrade(&bridge);
        pipeline.with_mut(|owner| {
            owner.set_semantics_update_callback(Arc::new(
                move |update: &flui_semantics::TreeUpdate| {
                    if let Some(bridge) = publish_bridge.upgrade() {
                        bridge.publish(update.clone());
                    }
                },
            ));
        });

        let enabled_flag = semantics.platform_semantics_enabled_handle();
        let republish_flag = semantics.full_republish_handle();
        let wake = Arc::clone(wake);
        let awaken_window = Arc::downgrade(window);
        bridge.set_activation_listener(Arc::new(move |active| {
            enabled_flag.store(active, Ordering::Relaxed);
            if active {
                // A (re)attached assistive technology's state is unknown —
                // it may have forgotten everything — and flushes publish
                // incrementally, so it must be answered with a
                // self-contained full tree, not the next diff.
                republish_flag.store(true, Ordering::Relaxed);
            }
            // The flag alone changes nothing until a frame runs: wake the
            // loop and poke this window so the reconcile actually happens.
            wake();
            if let Some(window) = awaken_window.upgrade() {
                window.request_redraw();
            }
        }));
        // Assistive technology may have attached before this window existed
        // — the transition the listener waits for has already happened.
        if bridge.is_active() {
            semantics.set_platform_semantics_enabled(true);
        }

        bridge.set_action_listener(Arc::new(move |request| {
            let Some(node_id) = AccessibilityNodeId::from_u64(request.target_node.0) else {
                tracing::warn!(
                    "dropping accessibility action addressed to the zero node id (out of \
                     contract: no published tree ever exports it)"
                );
                return;
            };
            let Some(action) = semantics_action_for(request.action) else {
                tracing::trace!(
                    action = ?request.action,
                    "dropping accessibility action FLUI has no counterpart for"
                );
                return;
            };
            let arguments = request.data.as_ref().and_then(|data| {
                let translated = semantics_action_args_for(data, request.target_node);
                if translated.is_none() {
                    // The action still routes; only its payload is lost.
                    // Traced because a SetValue without its value reaches a
                    // handler as a no-op edit, which is otherwise invisible.
                    // Two cases share this branch: a payload kind FLUI has no
                    // argument shape for, and a payload untranslatable for
                    // THIS request (a cross-node text selection).
                    tracing::trace!(
                        action = ?request.action,
                        "accessibility action payload not translatable (unsupported kind, or \
                         invalid for this target); routing the action argument-free"
                    );
                }
                translated
            });
            let mut semantics_request = SemanticsActionRequest::new(node_id, action);
            semantics_request.arguments = arguments;
            if let Err(error) = command_sender.send_semantics_action(semantics_request) {
                tracing::warn!(
                    ?error,
                    "dropping accessibility action: the realm inbox is full or gone"
                );
            }
        }));
    }

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
            owner.set_post_frame_handle(PostFrameHandle::new(capabilities.scheduler));
            owner.set_local_post_frame_handle(capabilities.local_post_frame_handle);
            owner.set_interaction_dispatch_handle(capabilities.interaction_dispatch_handle.clone());
            owner.set_text_input_handle(text_input.handle());
            // Paired here, the one place holding both halves: the realm's
            // dispatch ticket (identity) and THIS presentation's pipeline
            // (the tree). A realm may host several presentations, each with
            // its own `PipelineOwner`, so a probe installed once per realm
            // would answer every presentation with the first one's tree.
            owner.set_hit_test_handle(flui_interaction::HitTestHandle::new(
                capabilities.interaction_dispatch_handle,
                Rc::new(
                    flui_rendering::pipeline::hit_test_probe::PipelineHitTestProbe::new(&pipeline),
                ),
            ));
        });

        let renderer =
            RenderingFlutterBinding::new_with_pipeline(pipeline.clone(), capabilities.scheduler);

        // Idle-wake wiring: a dirty mark (mark_needs_layout / mark_needs_paint)
        // fires this callback so a quiescent event loop produces the frame.
        // Reentrancy-safe: the callback fires while the CALLER holds the
        // pipeline cell checked out, and `wake` only touches `Send + Sync`
        // runtime-level state — never this presentation's own `widgets` /
        // `renderer` / gesture state.
        //
        // Pokes THIS presentation's own window directly (`Weak`, exactly
        // like `Self::window` below — never a strong ref kept past the
        // platform's own teardown), in addition to the realm-wide
        // `capabilities.wake` call: `capabilities.wake` sets the shared
        // `needs_redraw` bit (still required — it is what wakes an idle
        // event loop at all) but only pokes whichever ONE window
        // `AppRuntime`'s own `redraw_window` slot happens to hold (issue
        // #555's still-single-window wake contract). Once a realm hosts
        // more than one presentation, each needs its OWN window poked when
        // IT dirties — never a sibling's — or a redraw request stamped for
        // this presentation would silently wake (or fail to wake) the wrong
        // native window. See `redraw_request_from_a_does_not_wake_bs_window`.
        let visual_wake = Arc::clone(&capabilities.wake);
        let redraw_window = Arc::downgrade(&window);
        pipeline.with_mut(|owner| {
            owner.set_on_need_visual_update(move || {
                visual_wake();
                if let Some(window) = redraw_window.upgrade() {
                    window.request_redraw();
                }
            });
        });

        let semantics = SemanticsHost::new();
        // Semantics-enabled fan-out -> this presentation's own SemanticsHost.
        let semantics_flag = semantics.platform_semantics_enabled_handle();
        renderer.add_semantics_enabled_listener(Arc::new(move |enabled| {
            semantics_flag.store(enabled, Ordering::Relaxed);
        }));

        Self::wire_platform_accessibility(
            &window,
            &pipeline,
            &semantics,
            &capabilities.wake,
            capabilities.command_sender,
        );

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
            redraw_pending: Cell::new(false),
            vsync: RefCell::new(Vsync::new()),
            clock: FrameClock::new(),
            last_segment_span: Cell::new(None),
            frame_failure_streak: Cell::new(0),
            #[cfg(test)]
            segment_probe: RefCell::new(None),
            #[cfg(test)]
            flush_count: Cell::new(0),
        };
        state.attach_surface();
        state
    }

    /// Standalone assembly with no realm above it: this presentation's
    /// `WidgetsBinding` lazily self-owns a private `GlobalKeyScope` on first
    /// `GlobalKey` registration (never shared, so it never conflicts with
    /// anything), and its `RenderingFlutterBinding` owns its own throwaway
    /// `UpdateScheduler` (see [`RenderingFlutterBinding::new_for_test_with_pipeline`]).
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
            redraw_pending: Cell::new(false),
            vsync: RefCell::new(Vsync::new()),
            clock: FrameClock::new(),
            last_segment_span: Cell::new(None),
            frame_failure_streak: Cell::new(0),
            #[cfg(test)]
            segment_probe: RefCell::new(None),
            #[cfg(test)]
            flush_count: Cell::new(0),
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
        let window: Arc<dyn PlatformWindow> = test_platform_window(platform_text_input);
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

    /// A clone of this presentation's own implicit-animation controller
    /// registry. `Vsync` is `Arc`-backed, so this is cheap and every clone
    /// observes the same registry — the same handle shape
    /// `UiRealm::vsync()` used to hand out from its own realm-level slot
    /// (issue #556: the registry moved here, one per presentation).
    #[must_use]
    pub(crate) fn vsync(&self) -> Vsync {
        self.vsync.borrow().clone()
    }

    /// Replace this presentation's registry with a pre-existing shared
    /// `Vsync` — see `UiRealm::set_vsync`'s doc for the one legitimate use
    /// (a `VsyncScope` built before this presentation's own registry was
    /// acquired).
    #[expect(
        dead_code,
        reason = "no production caller yet -- forwards from UiRealm::set_vsync, \
                  itself also uncalled in production (see that method's own doc)"
    )]
    pub(crate) fn set_vsync(&self, vsync: Vsync) {
        *self.vsync.borrow_mut() = vsync;
    }

    /// This presentation's own physical-time produce-gate state machine
    /// (issue #556). See [`FrameClock`]'s own doc for the produce decision
    /// it makes.
    #[must_use]
    pub(crate) fn clock(&self) -> &FrameClock {
        &self.clock
    }

    /// Record this pump's just-completed segment span for THIS
    /// presentation. Called exactly once per pump in which this
    /// presentation's own segment ran, by `UiRealm::draw_frame_entered`'s
    /// per-presentation loop, immediately after
    /// `UiRealm::draw_frame_for_presentation` returns. See
    /// [`Self::last_segment_span`]'s field doc for why this lives here and
    /// not on the shared `FrameClock`.
    pub(crate) fn set_last_segment_span(&self, start: Instant, end: Instant) {
        self.last_segment_span.set(Some((start, end)));
    }

    /// Read AND CLEAR the span [`Self::set_last_segment_span`] most
    /// recently recorded for this presentation. `take`, not `get`: called
    /// by `UiRealm::render_frame_entered` at most once per pump, exactly
    /// when it is about to decide whether to attach a `FrameSnapshot` to
    /// THIS presentation's clock — a pump in which this presentation's own
    /// segment did NOT run must see `None`, never a stale span this same
    /// presentation latched on an earlier pump.
    pub(crate) fn take_last_segment_span(&self) -> Option<(Instant, Instant)> {
        self.last_segment_span.take()
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

    /// Frames dropped due to a real submit failure (surface/device error) —
    /// never incremented by a `FrameClock` deferral (hidden/backpressure):
    /// see `FrameClock::produces_deferred`'s own doc for why the two stay
    /// structurally separate counter families.
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

    // ========================================================================
    // Pump segment (ADR-0043 §3): per-presentation dirty predicate and wake bit
    // ========================================================================

    /// Mark this presentation's own wake-only redraw bit. See
    /// [`Self::redraw_pending`]'s field doc.
    pub(crate) fn mark_redraw_pending(&self) {
        self.redraw_pending.set(true);
    }

    /// Read-and-clear this presentation's wake-only redraw bit — called at
    /// the START of this presentation's pump segment, before dirty is
    /// sampled, so a mark that arrives WHILE the segment runs sets the bit
    /// again rather than being silently absorbed by this read.
    pub(crate) fn take_redraw_pending(&self) -> bool {
        self.redraw_pending.replace(false)
    }

    /// This presentation's own pump dirty predicate: would its build phase
    /// do anything, or does its render pipeline have a dirty node to flush?
    /// Deliberately excludes gesture-pending state — ticking gesture
    /// deadlines happens once per pump, before any presentation's segment
    /// runs, and gesture state never gates what a segment itself does (only
    /// whether the OUTER runner wakes the loop at all); including it here
    /// would not change any segment's observable outcome, only make this
    /// predicate diverge from the exact condition the pipeline itself uses
    /// to decide "nothing to flush".
    /// Reconcile platform-driven semantics enablement onto this
    /// presentation's pipeline — the owner-thread half of the activation
    /// seam.
    ///
    /// The platform's activation listener may only flip the
    /// [`SemanticsHost`] flag and wake the loop (it runs on the adapter's
    /// thread); this is where the flag becomes pipeline state. Called at
    /// each segment start in `UiRealm::draw_frame_entered`, BEFORE dirty
    /// sampling, because enabling seeds the root as needing semantics —
    /// exactly the pending work the segment should then observe. A no-op
    /// whenever flag and pipeline already agree, which is every frame but
    /// the two transitions.
    pub(crate) fn reconcile_semantics_enablement(&self) {
        let wanted = self.semantics.semantics_enabled();
        // Consumed unconditionally: a request that arrives alongside a
        // deactivation must not linger and fire on some later re-enable.
        let full_republish = self.semantics.take_full_republish_request();
        self.pipeline.with_mut(|owner| {
            if owner.semantics_enabled() != wanted {
                owner.set_semantics_enabled(wanted);
            }
            if full_republish && wanted {
                // Activation with an owner already alive (a screen reader
                // restarting without an intervening deactivation, or one
                // attaching after a handle enabled semantics first): the
                // adapter must be re-answered with a self-contained tree.
                // On the enable transition just above this is a no-op-shaped
                // reinforcement — the fresh owner's first flush is full
                // anyway.
                owner.request_semantics_full_publish();
            }
        });
    }

    #[must_use]
    pub(crate) fn has_pending_work(&self) -> bool {
        self.widgets.has_pending_builds()
            || self.has_pending_layout_builder_work()
            || self.renderer.root_pipeline_owner().with_mut(|owner| {
                // Cross-thread dirty requests (`RenderInvalidationHandle` producers —
                // background asset loaders, the frames-reenable redirty) sit in a channel
                // until drained; `run_frame` itself always drains before its
                // first phase, so an UNGATED call here would eventually see
                // them regardless. This gate runs BEFORE `run_frame` now, so
                // it must drain first or it would read a stale
                // `has_dirty_nodes() == false` for a request that already
                // landed in the channel and is sitting there un-applied.
                // Non-blocking, idempotent (`try_recv`-based) — safe to call
                // here even though `run_frame`'s own segment drains again
                // immediately after.
                owner.drain_pending_dirty();
                owner.has_dirty_nodes()
            })
    }

    /// Whether a registered `LayoutBuilder` seam entry
    /// (`crates/flui-view/src/owner/layout_builder.rs`) exists.
    ///
    /// A live entry is pruned/serviced on every `run_frame_with_layout_
    /// builders` pass regardless of whether anything else is dirty — a
    /// stale entry never gets pruned, and a live one never gets its chance
    /// to rebuild on a constraint change, unless that call still happens
    /// with zero pending builds and zero dirty render nodes.
    ///
    /// Production has no way to populate this registry yet (the widget-side
    /// entry point, `LayoutBuilder`, has not landed — `BuildOwner::
    /// layout_builder_count` is a test-only accessor, planted only via
    /// `register_layout_builder_for_test`), so this is unconditionally
    /// `false` outside test builds: correct today because the registry is
    /// provably always empty in production, not because the check is
    /// skipped for convenience.
    #[cfg(test)]
    fn has_pending_layout_builder_work(&self) -> bool {
        self.widgets
            .with_build_owner(|owner| owner.layout_builder_count() > 0)
    }

    #[cfg(not(test))]
    #[expect(
        clippy::unused_self,
        reason = "the &self receiver is intentional: this must stay a method with the \
                  same signature as its #[cfg(test)] twin above, not an associated \
                  function, or the two cfg arms would diverge in call-site shape"
    )]
    fn has_pending_layout_builder_work(&self) -> bool {
        false
    }

    /// Record one more consecutive frame failure and return the new streak
    /// length. See [`Self::frame_failure_streak`]'s field doc.
    pub(crate) fn note_frame_failure(&self) -> u32 {
        let streak = self.frame_failure_streak.get().saturating_add(1);
        self.frame_failure_streak.set(streak);
        streak
    }

    /// A segment completed without failing; the next failure starts a
    /// fresh streak. See [`Self::frame_failure_streak`]'s field doc.
    pub(crate) fn reset_frame_failure_streak(&self) {
        self.frame_failure_streak.set(0);
    }

    /// Install (or clear) the segment fault-injection probe. See
    /// [`Self::segment_probe`]'s field doc.
    #[cfg(test)]
    pub(crate) fn set_segment_probe(&self, probe: Option<Box<dyn Fn()>>) {
        *self.segment_probe.borrow_mut() = probe;
    }

    /// Run the installed segment probe, if any. Called from the top of
    /// `UiRealm::draw_frame_for_presentation`, under a short immutable
    /// borrow of the probe slot — a probe must not call
    /// [`Self::set_segment_probe`] from inside itself. A panicking probe
    /// releases the borrow during unwind, so the boundary's retry pump can
    /// run (and re-panic) it again.
    #[cfg(test)]
    pub(crate) fn run_segment_probe(&self) {
        if let Some(probe) = self.segment_probe.borrow().as_ref() {
            probe();
        }
    }

    /// Record that this presentation's build+layout+paint segment ran. See
    /// [`Self::flush_count`]'s field doc for the oracle this backs.
    #[cfg(test)]
    pub(crate) fn record_flush(&self) {
        self.flush_count.set(self.flush_count.get() + 1);
    }

    /// This presentation's own flush count. Test-only oracle.
    #[cfg(test)]
    pub(crate) fn flush_count(&self) -> u32 {
        self.flush_count.get()
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
    ///
    /// # The overlay is inside what it measures
    ///
    /// When enabled, this pulls `frames_since(None)` and rebuilds both
    /// histograms on every composited frame. The cost is bounded — the
    /// telemetry ring is fixed-capacity, so it is O(ring), not O(session) —
    /// and no frame pays it while the overlay is off. But it is not free, and
    /// it lands *inside* the frames the overlay subsequently reports: read the
    /// displayed percentiles as the cost of running with the overlay on, not
    /// as the app's cost without it.
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

        let snapshots = self.clock.frames_since(None);
        let present_p99 = produce_to_present_histogram(&snapshots)
            .p99()
            .map_or_else(|| "n/a".to_owned(), format_millis);
        let input_p99 = input_to_present_histogram(&snapshots)
            .p99()
            .map_or_else(|| "n/a".to_owned(), format_millis);
        let input_truncated = snapshots
            .iter()
            .any(|snapshot| snapshot.input_epochs.overflowed());
        overlay.set_diagnostic_line(Some(format!(
            "present_p99={present_p99} input_p99={input_p99} deferred={} dropped={} input_truncated={input_truncated}",
            self.clock.produces_deferred(),
            self.frames_dropped(),
        )));

        let overlay_id = layer_tree.insert(overlay.into());
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

    /// Begin deterministic owner-local teardown (ADR-0043 §5's per-presentation
    /// ordering, the part of it this type alone can carry out): input/cursor
    /// first, IME and focus deactivate next, THEN the root widget detaches
    /// through this exact presentation's own `WidgetsBinding` — so any
    /// `State::dispose()` hook a descendant runs sees focus/IME already
    /// quiesced, never a live input surface mid-teardown. `GlobalKeyScope`
    /// reclamation is not this method's job: it happens when this
    /// presentation's `BuildOwner` itself drops (`GlobalKeyScope::
    /// reclaim_owner`, wired through `BuildOwner`'s own `Drop`), which
    /// dropping this `WidgetsBinding` triggers regardless of whether
    /// `detach_root_widget` found a root to unmount.
    ///
    /// Callers that need dispose hooks to resolve `GlobalKey` lookups
    /// correctly must run this inside the realm's own `enter()` (see
    /// `UiRealm`'s `Drop` impl) — this method itself does not activate any
    /// registry.
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
        // Withdraw from the platform accessibility bridge: detach both
        // listeners so an activation flip or action request arriving after
        // close is dropped at the platform seam (an action that slips
        // through anyway is still dropped at the drain's forest-membership
        // check — two independent gates, same verdict), and stop assembly
        // so the owner's disposed notifier fires while the pipeline is
        // still alive. Guarded on `is_free()` because `close()` also runs
        // from `Drop`, where a panicking unwind may hold the checkout.
        if let Some(window) = self.window.upgrade()
            && let Some(bridge) = window.accessibility()
        {
            bridge.set_activation_listener(Arc::new(|_| {}));
            bridge.set_action_listener(Arc::new(|_| {}));
        }
        if self.pipeline.is_free() {
            self.pipeline.with_mut(|owner| {
                if owner.semantics_enabled() {
                    owner.set_semantics_enabled(false);
                }
            });
        }
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
        // Detach LAST: a no-op if nothing was ever attached (many tests never
        // mount a root), and otherwise unmounts through this presentation's
        // OWN element tree only -- never a sibling's, since each
        // PresentationState owns an exclusive WidgetsBinding.
        self.widgets.detach_root_widget();
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

    /// `take_last_segment_span` reads AND CLEARS — a second call with no
    /// intervening `set_last_segment_span` must see `None`, never the same
    /// value again. Pins the per-pump discipline directly on the type: this
    /// is what makes "a segment ran THIS pump" (not "on some earlier pump")
    /// an invariant this presentation's OWN state enforces, rather than
    /// something only true by accident of how its one current caller
    /// (`UiRealm::record_submit_telemetry`, always addressed to the correct
    /// producer) happens to use it.
    #[test]
    fn take_last_segment_span_clears_on_read_a_second_take_sees_none() {
        let presentation = presentation();
        assert_eq!(
            presentation.take_last_segment_span(),
            None,
            "a fresh presentation has no segment span recorded yet"
        );

        let start = Instant::now();
        let end = start + std::time::Duration::from_millis(1);
        presentation.set_last_segment_span(start, end);

        assert_eq!(
            presentation.take_last_segment_span(),
            Some((start, end)),
            "the first take must return exactly what was set"
        );
        assert_eq!(
            presentation.take_last_segment_span(),
            None,
            "a second take with no intervening set must see None, not the stale value again"
        );
    }

    /// `close` must detach through this presentation's OWN `WidgetsBinding`
    /// — the ADR-0043 teardown ordering this method now carries out, not
    /// just the input/focus/IME steps that predate it.
    #[test]
    fn close_detaches_this_presentations_own_root_widget() {
        let presentation = presentation();
        presentation
            .widgets()
            .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
            .expect("fresh presentation attaches its first root");
        assert!(
            presentation.widgets().root_element().is_some(),
            "root must be attached before close"
        );

        presentation.close();

        assert!(
            presentation.widgets().root_element().is_none(),
            "close must detach the root through this presentation's own binding"
        );
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

        let window = Arc::new(crate::app::window_test_support::TestWindow::new().focused(true));
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

        assert_eq!(window.cursor(), CursorIcon::Pointer);
        presentation.close();
        assert_eq!(window.cursor(), CursorIcon::Default);
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
            let window: Arc<dyn PlatformWindow> =
                Arc::new(crate::app::window_test_support::TestWindow::new());
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
                Arc::new(crate::app::window_test_support::TestWindow::new()),
            );

            presentation.perform_haptic_feedback(HapticFeedback::MediumImpact);
        }
    }

    // ========================================================================
    // Performance overlay — migrated from the retired `AppBinding`'s test
    // module.
    // ========================================================================
    mod performance_overlay_wiring {
        use flui_layer::{CanvasLayer, Layer};

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

        #[test]
        fn overlay_line_surfaces_tail_quality_and_keeps_deferrals_distinct_from_drops() {
            let presentation = presentation();
            let clock = presentation.clock();
            let now = Instant::now();

            clock.set_hidden(true);
            clock.mark_demand(flui_scheduler::DemandKind::Host);
            assert!(matches!(
                clock.poll(now),
                flui_scheduler::PollDecision::Skip(flui_scheduler::SkipReason::Hidden)
            ));
            clock.set_hidden(false);
            assert!(matches!(
                clock.poll(now),
                flui_scheduler::PollDecision::Produce
            ));

            for _ in 0..=flui_scheduler::MAX_COALESCED_INPUT_EPOCHS {
                clock.stamp_input_epoch(now);
            }
            clock.record_frame(
                presentation.id(),
                now,
                now,
                now,
                now + Duration::from_millis(8),
                flui_scheduler::PresentOutcome::Presented,
            );
            presentation.record_frame_dropped();
            presentation.set_performance_overlay(true);

            let (mut tree, _) = tree_with_root();
            presentation.attach_performance_overlay(&mut tree);
            let overlay_id = *tree_root_children(&tree).last().expect("overlay present");
            let line = tree
                .get_layer(overlay_id)
                .expect("overlay layer")
                .as_performance_overlay()
                .expect("performance overlay variant")
                .diagnostic_line()
                .expect("runtime telemetry line");

            assert!(line.contains("present_p99=8.0ms"), "line was {line:?}");
            assert!(line.contains("deferred=1"), "line was {line:?}");
            assert!(line.contains("dropped=1"), "line was {line:?}");
            assert!(
                line.contains("input_truncated=true"),
                "an overflow-biased input tail must be labelled: {line:?}"
            );
        }

        fn tree_root_children(tree: &LayerTree) -> Vec<flui_layer::LayerId> {
            let root = tree.root().expect("root");
            tree.get(root).expect("root node").children().to_vec()
        }
    }
}
