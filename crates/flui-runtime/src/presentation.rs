//! Owner-thread state for one presentation of a UI ui_runtime.
//!
//! Public only so the host that drives a UI runtime (`flui-app`) can name it; it
//! is not an embedder API (ADR-0027 §9). It is the UI-owner domain, not a
//! cross-thread god object: native event-loop ownership remains in the
//! runner/window host and raster/surface ownership remains in
//! `flui_engine::RasterOwner`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use flui_animation::{FrameTick, MotionClock, Vsync};
use flui_foundation::PresentationId;
use flui_interaction::{
    FocusManager, GestureBinding, InteractionDispatchHandle, TextInputBackend, TextInputHandle,
    TextInputOwner,
};
use flui_layer::{LayerTree, PerformanceOverlayLayer, PerformanceOverlayOption, PerformanceSample};
use flui_platform_api::HapticFeedback;
#[cfg(any(test, feature = "test-support"))]
use flui_platform_api::PlatformTextInput;
use flui_platform_api::TextStoreHost;
use flui_platform_api::{Clipboard, CursorError, CursorIcon, PlatformWindow};
use flui_rendering::binding::RendererBinding as _;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{
    AsyncDriver, ClockSource, FrameClock, PostFrameHandle, UpdateScheduler,
    input_to_present_histogram, produce_to_present_histogram,
};
use flui_semantics::platform::PlatformAccessibility;
use flui_semantics::{
    AccessibilityNodeId, SemanticsActionError, SemanticsActionRequest, semantics_action_request_for,
};
use flui_view::{
    __runtime::{BindingRuntime as _, FramePhaseMarker},
    GlobalKeyScope, WidgetsBinding,
};
use web_time::{Duration, Instant};

use crate::epoch::{FrameCommitState, TreeRevision};
use crate::frame_failure::SegmentPhase;
use crate::held_input::HeldPointerQueue;
use crate::performance_stats::PerformanceStats;
use crate::renderer_binding::RenderingBinding;
use crate::semantics_host::SemanticsHost;

fn format_millis(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1_000.0)
}

/// Runtime-supplied capabilities threaded into a presentation at assembly
/// time (ADR-0043 §1): [`Self::global_key_scope`] is installed FIRST — the
/// underlying `BuildOwner::set_global_key_scope` setter panics with `BUG:`
/// if called after this owner's own `GlobalKey` registration has begun —
/// then the UI runtime's shared dispatch handles, before this presentation's own
/// focus/IME are wired into its fresh `WidgetsBinding`, all before
/// attach/mount. See [`PresentationState::new`].
pub(crate) struct RuntimeCapabilities<'a> {
    /// The UI runtime's cross-tree `GlobalKey` uniqueness domain (ADR-0043).
    pub(crate) global_key_scope: GlobalKeyScope,
    /// A `Weak` handle to the UI runtime's shared async tasks (UI runtime-level; see
    /// the presentation-teardown contract for the consequence of that when
    /// this presentation closes).
    pub(crate) async_driver: AsyncDriver,
    /// The UI runtime's owner-local post-frame callback capability — addresses
    /// the UI runtime's [`flui_scheduler::OwnerFrame`] directly, so it can
    /// capture `Rc`/`RefCell` widget state.
    pub(crate) post_frame_handle: PostFrameHandle,
    /// The UI runtime's interaction dispatch lane.
    pub(crate) interaction_dispatch_handle: InteractionDispatchHandle,
    /// The UI runtime's own scheduler — borrowed only for the duration of
    /// assembly; the constructed [`RenderingBinding`] keeps just a
    /// `WeakUpdateScheduler` derived from it.
    pub(crate) scheduler: &'a UpdateScheduler,
    /// The UI runtime's platform wake capability. It is wired as the UI runtime
    /// scheduler's `on_frame_scheduled` hook (in `UiRuntime::construct`) and
    /// handed to the platform accessibility bridge; the presentation's own
    /// pipeline wake now routes through the scheduler rather than cloning
    /// this directly.
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    /// A cross-thread sender into the UI runtime's command inbox, already
    /// stamped with this presentation's id. Handed to the platform
    /// accessibility bridge's action listener, so an assistive-technology
    /// request marshals onto the owner thread as a
    /// [`SemanticsActionRequest`] and resolves at the next Idle drain —
    /// never on the adapter's own thread.
    pub(crate) command_sender: super::ui_runtime::UiCommandSender,
    /// The UI runtime's platform clipboard, handed to widgets through
    /// `LifecycleContext::clipboard_handle`.
    pub(crate) clipboard: Arc<dyn Clipboard>,
    /// The UI runtime's byte storage, if it has one, handed to widgets through
    /// `LifecycleContext::storage`.
    pub(crate) storage: Option<Arc<dyn flui_platform_api::Storage>>,
    /// Where the UI runtime reads time: this presentation's gesture arena and
    /// [`FrameClock`] read the same source as the UI runtime's frame clock.
    pub(crate) clock: &'a ClockSource,
    /// The UI runtime's text context, installed on the presentation's pipeline so
    /// its layout measures text through the UI runtime (ADR-0092 §10 step 3).
    pub(crate) text: flui_rendering::TextContextHandle,
}

/// A fresh in-memory clipboard — the one the headless platform hands out —
/// for a test UI runtime's `UiRuntime::new`.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn test_clipboard() -> Arc<dyn Clipboard> {
    Arc::new(flui_platform_api::InMemoryClipboard::new())
}

/// Pointer interpolation policy owned by one presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PointerResampling {
    /// Preserve ordinary frame coalescing without interpolation.
    #[default]
    Disabled,
    /// Interpolate measured samples at the owner's frame time with the
    /// interaction layer's standard lookback and sampling window.
    FrameAligned,
}

/// A presentation's pointer sampling policy could not be changed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PointerResamplingError {
    /// The exact presentation is no longer hosted by this realm.
    #[error("pointer resampling presentation is unavailable")]
    PresentationUnavailable,
    /// A contact retains the policy admitted with its Down.
    #[error(transparent)]
    ActiveContact(#[from] flui_interaction::ResamplingModeChangeError),
}

/// The window a presentation is built on, with the accessibility bridge its
/// backend fixed when it built the window.
///
/// The framework drives a window only through [`PlatformWindow`], which names
/// no AccessKit type (ADR-0082 §1); the bridge is read from the host-side
/// window once, where the runner holds it (`runner::presentation_window`),
/// and travels here beside the window. Production code has no implicit
/// conversion into this type: a runner builds it through
/// `runner::presentation_window` or names the bridge explicitly in
/// [`Self::new`], so dropping the bridge is never an accident of a `.into()`.
///
/// It also carries the window's text-store host when its backend is
/// pull-model (ADR-0135): the runner reads it through owner-thread proof
/// beside the bridge, and the presentation's text-input owner then speaks to
/// the host instead of the window's push capability. Owner-thread state, so
/// a `PresentationWindow` is not `Send`.
pub struct PresentationWindow {
    window: Arc<dyn PlatformWindow>,
    accessibility: Option<Arc<dyn PlatformAccessibility>>,
    text_store_host: Option<Rc<dyn TextStoreHost>>,
    pointer_resampling: PointerResampling,
}

impl std::fmt::Debug for PresentationWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationWindow")
            .field("window", &self.window.id())
            .field("pointer_resampling", &self.pointer_resampling)
            .field("accessibility", &self.accessibility.is_some())
            .field("text_store_host", &self.text_store_host.is_some())
            .finish()
    }
}

impl PresentationWindow {
    /// Pairs `window` with the bridge its backend exposes, if any.
    pub fn new(
        window: Arc<dyn PlatformWindow>,
        accessibility: Option<Arc<dyn PlatformAccessibility>>,
    ) -> Self {
        Self {
            window,
            accessibility,
            text_store_host: None,
            pointer_resampling: PointerResampling::Disabled,
        }
    }

    /// Choose this presentation's initial input policy; disabled by default.
    #[must_use]
    pub fn with_pointer_resampling(mut self, policy: PointerResampling) -> Self {
        self.pointer_resampling = policy;
        self
    }

    /// The window's text-store host, when its backend offers one; the
    /// presentation's text input then uses it rather than the window's push
    /// capability.
    #[must_use]
    pub fn with_text_store_host(self, text_store_host: Option<Rc<dyn TextStoreHost>>) -> Self {
        Self {
            text_store_host,
            ..self
        }
    }

    /// The window itself.
    #[must_use]
    pub fn window(&self) -> &Arc<dyn PlatformWindow> {
        &self.window
    }

    /// How this window takes text input: through its host if it has one,
    /// else through its push capability, else not at all.
    fn text_input_backend(
        window: &dyn PlatformWindow,
        text_store_host: Option<Rc<dyn TextStoreHost>>,
    ) -> TextInputBackend {
        match (text_store_host, window.text_input()) {
            (Some(host), _) => TextInputBackend::Pull(host),
            (None, Some(platform)) => TextInputBackend::Push(platform),
            (None, None) => TextInputBackend::Unsupported,
        }
    }
}

/// A test-only conversion that bypasses any accessibility bridge the
/// concrete window has: the presentation is built with none, even when the
/// window is a headless `MockWindow` carrying a `FakeAccessibility`. A test
/// that needs the bridge wired names it in [`PresentationWindow::new`]
/// instead.
#[cfg(any(test, feature = "test-support"))]
impl From<Arc<dyn PlatformWindow>> for PresentationWindow {
    fn from(window: Arc<dyn PlatformWindow>) -> Self {
        Self::new(window, None)
    }
}

/// The `From<Arc<dyn PlatformWindow>>` conversion above
/// for a borrowed window, for tests that reuse one window across several
/// installs; it bypasses the window's bridge the same way.
#[cfg(any(test, feature = "test-support"))]
impl From<&Arc<dyn PlatformWindow>> for PresentationWindow {
    fn from(window: &Arc<dyn PlatformWindow>) -> Self {
        Self::new(Arc::clone(window), None)
    }
}

/// A UI runtime-backed test window carrying an optional platform text-input
/// capability — for `UiRuntime::for_test_with_text_input`, which needs a real
/// [`RuntimeCapabilities`]-assembled presentation (not the standalone
/// `PresentationState::new_for_test` path), just with a test window.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_platform_window(
    platform_text_input: Option<Arc<dyn PlatformTextInput>>,
) -> Arc<dyn PlatformWindow> {
    use crate::testing::TestWindow;
    Arc::new(
        TestWindow::new()
            .focused(true)
            .with_text_input(platform_text_input),
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

/// Phase-addressed frame fault injection used by the containment tests.
#[cfg(test)]
struct SegmentProbe {
    phase: SegmentPhase,
    callback: Box<dyn Fn()>,
}

/// Direct owner of mutable UI state scoped to one presentation.
///
/// It owns behavior-bearing subsystems as concrete values. It does not expose
/// a provider trait, service locator, erased resource bag, or arbitrary
/// executor. Cross-thread ingress is handled by closed commands stamped with
/// this presentation's generational identity.
pub struct PresentationState {
    id: PresentationId,
    pub(super) media_query: Rc<crate::media_query_root::MediaQuerySource>,
    /// Presentation-local projection of the host's accepted interaction policy.
    /// Recognizers read it at admission; active sequences keep owned snapshots.
    pub(super) gesture_settings: flui_interaction::GestureSettingsSource,
    pub(crate) gesture_geometry: RefCell<crate::ui_runtime::preferences::GeometryProjection>,
    pub(super) window_visible: Cell<bool>,
    pub(super) window_focused: Cell<bool>,
    pub(super) window_execution: Cell<flui_platform_api::WindowExecutionState>,
    pub(super) closing_requested: Cell<bool>,
    lifecycle: Cell<PresentationLifecycle>,
    close_mode: Cell<flui_interaction::__runtime::CloseMode>,
    /// Dispatch captures a UI runtime-wide withdrawal took before this
    /// presentation's own close, which retires them.
    withdrawn_dispatch: RefCell<Option<flui_interaction::__runtime::DispatchCustody>>,
    pipeline: PipelineCell,
    /// This presentation's liveness, as a token others may watch weakly.
    ///
    /// Dropping the presentation drops it, which is the point: nothing else
    /// reliably says "closed". The pipeline's own allocation does not —
    /// `LifecycleContext::pipeline_owner()` hands out a strong `PipelineCell`, so
    /// a widget that stores one keeps the tree alive past the close — and
    /// under `WindowPolicy::Shared` the UI runtime outlives any single presentation too.
    alive: RefCell<Option<Rc<()>>>,
    window: Weak<dyn PlatformWindow>,
    /// The window's accessibility bridge, if its backend has one. `Weak`
    /// like [`Self::window`]: the backend window owns the bridge, and this
    /// presentation must not keep it alive past the window.
    accessibility: Option<Weak<dyn PlatformAccessibility>>,
    gestures: GestureBinding,
    pub(crate) wheel_preferences: flui_interaction::WheelPreferencesSource,
    interaction_dispatch: Option<InteractionDispatchHandle>,
    /// Pointer input retained while this presentation has no committed tree.
    /// The queue is owner-thread-only and internally capped; replay detaches
    /// its batch before invoking dispatch so callbacks may enqueue reentrantly.
    held_pointer_input: RefCell<HeldPointerQueue>,
    focus: Rc<FocusManager>,
    text_input: Rc<TextInputOwner>,
    /// This presentation's semantics enablement gate and platform
    /// accessibility delivery. `close()` clears its announce/event
    /// callbacks unconditionally (production write); `UiRuntime::construct`
    /// reads `platform_semantics_enabled_handle()` to wire the UI runtime's
    /// renderer fan-out (production read) — announce/event delivery itself
    /// still has no production caller.
    semantics: SemanticsHost,
    /// The semantics agent a development agent hook reads this presentation
    /// through, once `UiRuntime::dev_agent_window` has vended it. The hook's
    /// `AgentWindow`s hold it weakly, so dropping it here (at close, or with
    /// the presentation) turns every handle `gone`; they hold the semantics
    /// handle themselves, so collection lasts while the hook keeps one.
    /// Owner thread only.
    pub(crate) dev_agent: RefCell<Option<crate::ui_runtime::DevAgentSlot>>,
    /// Owner-local widget framework state. One instance per presentation
    /// (ADR-0043) — the UI runtime-level singular binding this used to be
    /// dissolves here; every widget-tree operation for this surface enters
    /// through this presentation and activates this binding's own GlobalKey
    /// registry (composed into the UI runtime's whole-frame composite by
    /// `UiRuntime::enter`, never activated standalone in production).
    widgets: WidgetsBinding,
    /// Render tree, layout/paint pipeline coordination, and this
    /// presentation's own semantics-enablement fan-out. Moved from the
    /// retired UI runtime-level singular `UiRuntime::renderer`: `render_views`,
    /// `first_frame_sent`, and the semantics-enabled listener are
    /// per-presentation-window facts, not shareable once a UI runtime hosts more
    /// than one presentation.
    renderer: RenderingBinding,
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
    /// Consecutive frames this presentation produced and handed to the
    /// backend that the backend could not put on screen, reset by any frame
    /// that ends otherwise.
    ///
    /// Bounds the retention of a withheld frame
    /// ([`Self::record_frame_withheld`]'s caller decides how long to keep
    /// retrying). A transient — the drawable is briefly unavailable while
    /// the compositor rearranges itself — must be ridden out, because the
    /// scene that was consumed producing it is gone and nothing else will
    /// redraw it. A *permanent* unavailability must not be, because the
    /// retry dirties this presentation on every attempt and would otherwise
    /// spin at the fallback pace indefinitely.
    not_shown_streak: Cell<u32>,
    /// Performance-overlay state. `Some` IS the enable flag: the rolling
    /// frame-time window only exists while the overlay is on, so "enabled
    /// but no stats" is unrepresentable and a disabled overlay costs one
    /// `RefCell` borrow and a `None` check per frame. Moved here from the
    /// retired `AppBinding` — per-window stats, not a process-wide concern.
    performance_overlay: RefCell<Option<PerformanceStats>>,
    /// This presentation's own wake-only redraw mark (ADR-0043 §3's pump
    /// segment). Set alongside the UI runtime's own coalesced `needs_redraw` flag
    /// by every presentation-scoped operation that isn't otherwise
    /// re-derivable from live pipeline/build state (e.g. `attach_root_widget`
    /// scheduling the very first build); cleared at the START of this
    /// presentation's pump segment, before dirty is sampled, so a mark
    /// arriving WHILE the segment runs sets the bit again instead of being
    /// lost. This bit is wake-only, never the truth by itself: the segment's
    /// real dirty predicate is `take_redraw_pending() ||
    /// widgets().has_pending_builds() || <pipeline has_dirty_nodes>`
    /// (`Self::has_pending_work`) — see `UiRuntime::draw_frame_entered`'s
    /// per-presentation loop.
    redraw_pending: Cell<bool>,
    /// This presentation's controller registry. Handles share owner-local state;
    /// the presentation keeps its driver binding for its whole lifetime.
    vsync: Vsync,
    /// This presentation's animation clock: maps the UI runtime's raw frame time
    /// to the monotonic animation time [`Self::vsync`] is ticked with.
    /// Borrowed only inside [`Self::motion_tick`], never across user code.
    motion_clock: RefCell<MotionClock>,
    /// This presentation's own physical-time produce-gate state machine
    /// (issue #556) — the per-presentation half of the `UpdateScheduler`/
    /// `FrameClock`/raster three-owner split. `UiRuntime::draw_frame_entered`'s
    /// per-presentation segment loop polls this instead of the old
    /// `take_redraw_pending() || has_pending_work()` predicate directly;
    /// first-frame deferral (`RenderingBinding::send_frames_to_engine`'s
    /// old counter) folds into this same clock, withholding only the
    /// submit — see `FrameClock`'s own module doc for the reasoning that
    /// pins this.
    clock: FrameClock,
    /// (segment start, segment end) for the most recently completed
    /// build+layout+paint segment `UiRuntime::draw_frame_entered`'s
    /// per-presentation loop ran for THIS presentation — a side channel for
    /// a caller whose segment-running step and submit-deciding step are two
    /// separate calls (`UiRuntime::draw_frame_entered` runs the segment;
    /// `UiRuntime::render_frame_entered`, its caller, decides whether/how to
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
    /// Latest terminal (`Painted` or `Errored`) tree revision.
    tree_revision: Cell<TreeRevision>,
    /// Latest tree revision acknowledged by a successful submit verdict.
    presented_revision: Cell<TreeRevision>,
    /// Consecutive dropped-frame count for this presentation — the
    /// `consecutive_failures` field of every
    /// [`FrameFailureReport`](super::frame_failure::FrameFailureReport)
    /// this presentation produces. A terminal pipeline error or escaped
    /// segment panic increments it. A contained lifecycle recovery reports
    /// the current value but neither increments nor resets it. The next
    /// segment that completes without a terminal failure resets it only after
    /// that attempt's contained reports are delivered. Presentation-local on
    /// purpose: one window's streak must never color a sibling's reports.
    frame_failure_streak: Cell<u32>,
    /// Last frame segment entered for this presentation. Written before the
    /// segment's probe and work so the value remains unwind-correct.
    segment_phase: FramePhaseMarker<SegmentPhase>,
    /// Test-only fault injection addressed to one [`SegmentPhase`]. It runs
    /// immediately after that phase is stored and before its matching work,
    /// so a panic reaches the UI runtime's per-presentation `catch_unwind` with
    /// accurate attribution. The closure stays installed across retries and
    /// must arrange its own one-shot behavior when a clean retry is expected.
    #[cfg(test)]
    segment_probe: RefCell<Option<SegmentProbe>>,
    /// Test-only oracle: how many times this presentation's own
    /// build+layout+paint segment actually ran (`UiRuntime::
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
    ///   loop; the frame pump's reconcile (`UiRuntime::draw_frame_entered`)
    ///   then drives `PipelineOwner::set_semantics_enabled`, so tree
    ///   assembly starts and stops on the OWNER thread — the listener runs
    ///   on the adapter's own thread and touches only `Send + Sync` state.
    /// - **In** — action requests marshal through the UI runtime inbox as
    ///   [`SemanticsActionRequest`]s stamped for this exact presentation
    ///   and resolve at the next Idle drain. Requests FLUI cannot route (a
    ///   zero node id, an action with no counterpart, a full inbox) are
    ///   traced drops, since screen readers may act on a stale snapshot. Typed action payloads
    ///   (`accesskit::ActionData`) translate with the whole request through
    ///   [`semantics_action_request_for`]. Numeric setters remain distinct from
    ///   text edits, and expand/collapse preserve their explicit direction.
    ///   The current owner validates payloads before invoking a handler.
    ///
    /// A window without the capability (`bridge` is `None`) wires nothing:
    /// the pipeline keeps its documented publish-nowhere placeholder.
    fn wire_platform_accessibility(
        window: &Arc<dyn PlatformWindow>,
        bridge: Option<&Arc<dyn PlatformAccessibility>>,
        pipeline: &PipelineCell,
        semantics: &SemanticsHost,
        wake: &Arc<dyn Fn() + Send + Sync>,
        command_sender: super::ui_runtime::UiCommandSender,
    ) {
        let Some(bridge) = bridge else {
            return;
        };

        let publish_bridge = Arc::downgrade(bridge);
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
            if AccessibilityNodeId::from_u64(request.target_node.0).is_none() {
                tracing::warn!(
                    "dropping accessibility action addressed to the zero node id (out of \
                     contract: no published tree ever exports it)"
                );
                return;
            }
            let Some(semantics_request) = semantics_action_request_for(&request) else {
                tracing::trace!(action = ?request.action, "dropping unsupported accessibility action");
                return;
            };
            if let Err(error) = command_sender.send_semantics_action(semantics_request) {
                tracing::warn!(
                    ?error,
                    "dropping accessibility action: the ui_runtime inbox is full or gone"
                );
            }
        }));
    }

    /// Assemble the gesture arena, wiring its mouse-tracker cursor callback
    /// to `window` (shared by every constructor below — production and
    /// test alike — since the callback shape never varies with capability
    /// wiring).
    fn build_gestures(
        id: PresentationId,
        window: &Arc<dyn PlatformWindow>,
        clock: &ClockSource,
    ) -> GestureBinding {
        // The arena's deadlines read the ui_runtime's clock: a recognizer's
        // timeout and the frame that polls it share one timeline.
        let gestures = GestureBinding::with_clock(Arc::new(clock.clone()));
        flui_interaction::__runtime::set_pointer_capture_wake(&gestures, Arc::downgrade(window));
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

    /// Assemble a presentation wired into a UI runtime (ADR-0043 §1): installs
    /// `capabilities.global_key_scope` FIRST, then the UI runtime's shared
    /// dispatch handles, before this presentation's own focus/IME are
    /// wired to its fresh [`WidgetsBinding`] and [`RenderingBinding`]
    /// — all before the caller ever attaches/mounts a root widget.
    ///
    /// Builds the presentation's pipeline here, from the UI runtime's text
    /// context, so no presentation pipeline measures on any other; a
    /// `device_pixel_ratio` of `None` keeps the pipeline's default of `1.0`.
    pub(crate) fn new(
        id: PresentationId,
        device_pixel_ratio: Option<f64>,
        window: impl Into<PresentationWindow>,
        capabilities: RuntimeCapabilities<'_>,
    ) -> Self {
        let PresentationWindow {
            window,
            accessibility,
            text_store_host,
            pointer_resampling,
        } = window.into();
        let pipeline = PipelineCell::new(PipelineOwner::new(capabilities.text));
        if let Some(device_pixel_ratio) = device_pixel_ratio {
            pipeline.with_mut(|owner| owner.set_device_pixel_ratio(device_pixel_ratio));
        }
        let gestures = Self::build_gestures(id, &window, capabilities.clock);
        gestures
            .set_resampling_enabled(pointer_resampling == PointerResampling::FrameAligned)
            .expect("BUG: newly assembled presentation has no active contacts");
        let interaction_dispatch = flui_interaction::__runtime::presentation_dispatch(
            &capabilities.interaction_dispatch_handle,
        );
        let frame_clock = FrameClock::with_source(capabilities.clock.clone());
        let alive = Rc::new(());
        let focus = FocusManager::new();
        let text_input = TextInputOwner::new(PresentationWindow::text_input_backend(
            window.as_ref(),
            text_store_host,
        ));

        let widgets = WidgetsBinding::with_focus_manager(Rc::clone(&focus));
        widgets.set_pipeline_owner(pipeline.clone());
        widgets.with_build_owner_mut(|owner| {
            owner.set_global_key_scope(capabilities.global_key_scope);
            owner.set_async_driver(capabilities.async_driver);
            owner.set_post_frame_handle(capabilities.post_frame_handle);
            owner.set_interaction_dispatch_handle(interaction_dispatch.clone());
            owner.set_text_input_handle(text_input.handle());
            owner.set_clipboard_handle(flui_interaction::ClipboardHandle::new(
                capabilities.clipboard,
            ));
            if let Some(storage) = capabilities.storage {
                owner.set_storage(storage);
            }
            // Paired here, the one place holding both halves: the ui_runtime's
            // dispatch ticket (identity) and THIS presentation's pipeline
            // (the tree). A ui_runtime may host several presentations, each with
            // its own `PipelineOwner`, so a probe installed once per ui_runtime
            // would answer every presentation with the first one's tree.
            owner.set_hit_test_handle(flui_interaction::HitTestHandle::new(
                interaction_dispatch.clone(),
                Rc::new(
                    flui_rendering::pipeline::hit_test_probe::PipelineHitTestProbe::new(
                        &pipeline,
                        Rc::downgrade(&alive),
                    ),
                ),
            ));
        });

        let renderer =
            RenderingBinding::new_with_pipeline(pipeline.clone(), capabilities.scheduler);

        // Idle-wake wiring: a dirty mark (mark_needs_layout / mark_needs_paint)
        // fires this callback so a quiescent event loop produces the frame.
        // Reentrancy-safe: the callback fires while the CALLER holds the
        // pipeline cell checked out, and the scheduler's `ensure_visual_update`
        // only touches `Send + Sync` runtime-level state (its own atomic phase
        // and, briefly, the `frame_thread`/`on_frame_scheduled` slots — none of
        // which is held across a call back into the pipeline) — never this
        // presentation's own `widgets` / `renderer` / gesture state.
        //
        // Routes through the scheduler's phase gate so this presentation's
        // demand is subject to the SAME gate (and the SAME `frame_scheduled`
        // edge) as ticker/`end_of_frame` demand: a dirty mark issued mid-frame
        // on the driving thread no longer reaches `request_redraw` — the
        // in-flight frame's own surplus-frame guard is what picks it up. The
        // ui_runtime-wide wake still happens through the `on_frame_scheduled` hook
        // (`UiRuntime::construct` wires it to this same `wake`), so the closure
        // no longer calls `wake` directly.
        //
        // Still pokes THIS presentation's own window directly (`Weak`,
        // exactly like `Self::window` below — never a strong ref kept past
        // the platform's own teardown), because `capabilities.wake` only
        // pokes whichever ONE window `AppRuntime`'s own `redraw_window` slot
        // happens to hold (issue #555's still-single-window wake contract).
        // Once a ui_runtime hosts more than one presentation, each needs its OWN
        // window poked when IT dirties — never a sibling's. The poke is gated
        // on `ensure_visual_update`'s return so it fires only when the
        // scheduler actually accepted the demand.
        let scheduler = capabilities.scheduler.frame_waker();
        let redraw_window = Arc::downgrade(&window);
        let request_frame = Arc::new(move || {
            if scheduler.ensure_visual_update()
                && let Some(window) = redraw_window.upgrade()
            {
                window.request_redraw();
            }
        });
        let vsync = Vsync::new();
        let request_animation_frame = Arc::clone(&request_frame);
        let animation_owner = Rc::downgrade(&alive);
        vsync.set_frame_requester(Some(Rc::new(move || {
            if animation_owner.upgrade().is_some() {
                request_animation_frame();
            }
        })));
        // Install before mounting any element: rebuild handles capture this
        // hook at creation. Every presentation needs the same scheduling edge,
        // including those assembled without a desktop runner.
        widgets.with_build_owner_mut(|owner| {
            let request_frame = Arc::clone(&request_frame);
            owner.set_on_build_scheduled(move || request_frame());
        });
        {
            let request_frame = Arc::clone(&request_frame);
            widgets.set_on_need_frame(move || request_frame());
        }
        pipeline.with_mut(|owner| {
            owner.set_on_need_visual_update(move || request_frame());
        });

        let semantics = SemanticsHost::new();
        // Semantics-enabled fan-out -> this presentation's own SemanticsHost.
        let semantics_flag = semantics.platform_semantics_enabled_handle();
        renderer.add_semantics_enabled_listener(Arc::new(move |enabled| {
            semantics_flag.store(enabled, Ordering::Relaxed);
        }));

        Self::wire_platform_accessibility(
            &window,
            accessibility.as_ref(),
            &pipeline,
            &semantics,
            &capabilities.wake,
            capabilities.command_sender,
        );

        let state = Self {
            id,
            gesture_geometry: RefCell::new(
                crate::ui_runtime::preferences::GeometryProjection::default(),
            ),
            gesture_settings: flui_interaction::GestureSettingsSource::new(
                gestures.default_settings().clone(),
            ),
            media_query: Rc::new(crate::media_query_root::MediaQuerySource::from_window(
                window.as_ref(),
                pipeline.with(PipelineOwner::device_pixel_ratio),
            )),
            window_visible: Cell::new(window.is_visible()),
            window_focused: Cell::new(window.is_focused()),
            window_execution: Cell::new(window.execution_state()),
            closing_requested: Cell::new(false),
            lifecycle: Cell::new(PresentationLifecycle::Created),
            close_mode: Cell::new(flui_interaction::__runtime::CloseMode::Ordinary),
            withdrawn_dispatch: RefCell::new(None),
            pipeline,
            alive: RefCell::new(Some(alive)),
            window: Arc::downgrade(&window),
            accessibility: accessibility.as_ref().map(Arc::downgrade),
            gestures,
            wheel_preferences: flui_interaction::WheelPreferencesSource::new(
                flui_platform_api::WheelPreferences::default(),
            ),
            interaction_dispatch: Some(interaction_dispatch),
            held_pointer_input: RefCell::new(HeldPointerQueue::new(id)),
            focus,
            text_input,
            semantics,
            dev_agent: RefCell::new(None),
            widgets,
            renderer,
            frames_rendered: Cell::new(0),
            frames_dropped: Cell::new(0),
            not_shown_streak: Cell::new(0),
            performance_overlay: RefCell::new(None),
            redraw_pending: Cell::new(false),
            vsync,
            motion_clock: RefCell::new(MotionClock::new()),
            clock: frame_clock,
            last_segment_span: Cell::new(None),
            tree_revision: Cell::new(TreeRevision::ZERO),
            presented_revision: Cell::new(TreeRevision::ZERO),
            frame_failure_streak: Cell::new(0),
            segment_phase: FramePhaseMarker::new(SegmentPhase::Build),
            #[cfg(test)]
            segment_probe: RefCell::new(None),
            #[cfg(test)]
            flush_count: Cell::new(0),
        };
        state.attach_surface();
        state.clock.set_hidden(!state.window_visible.get());
        state
    }

    /// Standalone assembly with no UI runtime above it: this presentation's
    /// `WidgetsBinding` lazily self-owns a private `GlobalKeyScope` on first
    /// `GlobalKey` registration (never shared, so it never conflicts with
    /// anything), and its `RenderingBinding` owns its own throwaway
    /// `UpdateScheduler` (see [`RenderingBinding::new_for_test_with_pipeline`]).
    /// Used only by this module's own unit tests, which exercise
    /// presentation-local behavior (gestures/focus/haptics/overlay) in
    /// isolation; UI runtime-backed tests use [`Self::new`] through
    /// `UiRuntime::for_test`, exactly like production.
    #[cfg(test)]
    pub(crate) fn new_for_test_with_window(
        id: PresentationId,
        pipeline: PipelineCell,
        window: Arc<dyn PlatformWindow>,
    ) -> Self {
        let gestures = Self::build_gestures(id, &window, &ClockSource::Platform);
        let alive = Rc::new(());
        let focus = FocusManager::new();
        let text_input = TextInputOwner::new(PresentationWindow::text_input_backend(
            window.as_ref(),
            None,
        ));

        let widgets = WidgetsBinding::with_focus_manager(Rc::clone(&focus));
        widgets.set_pipeline_owner(pipeline.clone());

        let renderer = RenderingBinding::new_for_test_with_pipeline(pipeline.clone());
        // This path wires no platform accessibility (see the doc above).
        let accessibility: Option<Arc<dyn PlatformAccessibility>> = None;

        let semantics = SemanticsHost::new();
        let semantics_flag = semantics.platform_semantics_enabled_handle();
        renderer.add_semantics_enabled_listener(Arc::new(move |enabled| {
            semantics_flag.store(enabled, Ordering::Relaxed);
        }));

        let state = Self {
            id,
            gesture_geometry: RefCell::new(
                crate::ui_runtime::preferences::GeometryProjection::default(),
            ),
            gesture_settings: flui_interaction::GestureSettingsSource::new(
                gestures.default_settings().clone(),
            ),
            media_query: Rc::new(crate::media_query_root::MediaQuerySource::from_window(
                window.as_ref(),
                pipeline.with(PipelineOwner::device_pixel_ratio),
            )),
            window_visible: Cell::new(window.is_visible()),
            window_focused: Cell::new(window.is_focused()),
            window_execution: Cell::new(window.execution_state()),
            closing_requested: Cell::new(false),
            lifecycle: Cell::new(PresentationLifecycle::Created),
            close_mode: Cell::new(flui_interaction::__runtime::CloseMode::Ordinary),
            withdrawn_dispatch: RefCell::new(None),
            pipeline,
            alive: RefCell::new(Some(alive)),
            window: Arc::downgrade(&window),
            accessibility: accessibility.as_ref().map(Arc::downgrade),
            gestures,
            wheel_preferences: flui_interaction::WheelPreferencesSource::new(
                flui_platform_api::WheelPreferences::default(),
            ),
            interaction_dispatch: None,
            held_pointer_input: RefCell::new(HeldPointerQueue::new(id)),
            focus,
            text_input,
            semantics,
            dev_agent: RefCell::new(None),
            widgets,
            renderer,
            frames_rendered: Cell::new(0),
            frames_dropped: Cell::new(0),
            not_shown_streak: Cell::new(0),
            performance_overlay: RefCell::new(None),
            redraw_pending: Cell::new(false),
            vsync: Vsync::new(),
            motion_clock: RefCell::new(MotionClock::new()),
            clock: FrameClock::new(),
            last_segment_span: Cell::new(None),
            tree_revision: Cell::new(TreeRevision::ZERO),
            presented_revision: Cell::new(TreeRevision::ZERO),
            frame_failure_streak: Cell::new(0),
            segment_phase: FramePhaseMarker::new(SegmentPhase::Build),
            #[cfg(test)]
            segment_probe: RefCell::new(None),
            #[cfg(test)]
            flush_count: Cell::new(0),
        };
        state.attach_surface();
        state.clock.set_hidden(!state.window_visible.get());
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
    pub(crate) fn renderer(&self) -> &RenderingBinding {
        &self.renderer
    }

    /// This presentation's generational identity.
    #[must_use]
    pub fn id(&self) -> PresentationId {
        self.id
    }

    /// Where this presentation is in its owner-thread lifecycle.
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

    /// The bounded queue used while this presentation has no committed tree.
    #[must_use]
    pub(crate) fn held_pointer_input(&self) -> &RefCell<HeldPointerQueue> {
        &self.held_pointer_input
    }

    /// A clone of this presentation's own implicit-animation controller
    /// registry. `Vsync` is `Arc`-backed, so this is cheap and every clone
    /// observes the same registry — the same handle shape
    /// `UiRuntime::vsync()` used to hand out from its own UI runtime-level slot
    /// (issue #556: the registry moved here, one per presentation).
    #[must_use]
    pub(crate) fn vsync(&self) -> Vsync {
        self.vsync.clone()
    }

    /// This frame's animation tick for the UI runtime's raw frame time `raw`
    /// (measured from the UI runtime's start): monotonic and finite, so a stale
    /// or repeated `raw` repeats the previous tick.
    ///
    /// The clock's borrow ends inside this call, before the caller hands the
    /// tick to [`Vsync::tick_all`], which runs controller and listener code.
    ///
    /// The borrow ends before registry delivery invokes user code.
    pub(crate) fn motion_tick(&self, raw: Duration) -> FrameTick {
        self.motion_clock.borrow_mut().frame(raw)
    }

    pub(crate) fn motion_is_paused(&self) -> bool {
        self.motion_clock.borrow().is_paused()
    }

    pub(crate) fn apply_motion(
        &self,
        request: flui_protocol::MotionRequest,
    ) -> Result<(flui_protocol::MotionState, bool), flui_animation::InvalidPlaybackRate> {
        let rate = request
            .rate
            .map(flui_animation::PlaybackRate::new)
            .transpose()?;
        let mut clock = self.motion_clock.borrow_mut();
        let old_rate = clock.rate();
        let old_time = clock.now();
        if let Some(rate) = rate {
            clock.set_rate(rate);
        }
        if let Some(step_ms) = request.step_ms {
            clock.step(Duration::from_millis(step_ms));
        }
        let demand = clock.now() != old_time || (clock.rate() != old_rate && !clock.is_paused());
        Ok((
            flui_protocol::MotionState::new(
                clock.rate().get(),
                clock.now().as_duration().as_secs_f64() * 1000.0,
            ),
            demand,
        ))
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
    /// presentation's own segment ran, by `UiRuntime::draw_frame_entered`'s
    /// per-presentation loop, immediately after
    /// `UiRuntime::draw_frame_for_presentation` returns. See
    /// [`Self::last_segment_span`]'s field doc for why this lives here and
    /// not on the shared `FrameClock`.
    pub(crate) fn set_last_segment_span(&self, start: Instant, end: Instant) {
        self.last_segment_span.set(Some((start, end)));
    }

    /// Read AND CLEAR the span [`Self::set_last_segment_span`] most
    /// recently recorded for this presentation. `take`, not `get`: called
    /// by `UiRuntime::render_frame_entered` at most once per pump, exactly
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
        not(any(test, feature = "test-support")),
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
    /// into. `UiRuntime::semantics_agent` acquires its enablement handle
    /// here; announce/event delivery itself still has no production caller
    /// (future platform-embedder wiring).
    #[must_use]
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
    pub(crate) fn with_window<R>(&self, f: impl FnOnce(&dyn PlatformWindow) -> R) -> Option<R> {
        self.window.upgrade().map(|window| f(window.as_ref()))
    }

    /// Perform haptic feedback on this presentation's window, via
    /// [`PlatformWindow::haptics`].
    ///
    /// Silent no-op — no panic, no error — when the window is gone, or the
    /// window's backend has no [`PlatformHaptics`](flui_platform_api::PlatformHaptics)
    /// capability (desktop winit targets, for instance). Every call is fire-and-forget
    /// best-effort, with no availability-discovery API to check first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- haptics through a \
                      presentation is future wiring, forwarded today only by \
                      UiRuntime::perform_haptic_feedback (also uncalled in \
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

    /// Record a frame the backend could not put on screen, returning the new
    /// count of consecutive such frames. See the `not_shown_streak` field.
    ///
    /// The return value is the *post*-increment streak so a caller can bound
    /// its retention against this attempt without a second read.
    pub(crate) fn record_frame_withheld(&self) -> u32 {
        let streak = self.not_shown_streak.get().saturating_add(1);
        self.not_shown_streak.set(streak);
        streak
    }

    /// End a withheld streak: this frame was either shown or had nothing to
    /// show, so the surface is no longer *continuously* unavailable. Called
    /// from every frame outcome other than a withheld one, which is what
    /// makes "the next real event restarts the retry" true rather than
    /// assumed — a later withheld frame opens a fresh streak and retains
    /// again instead of inheriting an exhausted budget.
    pub(crate) fn clear_not_shown_streak(&self) {
        self.not_shown_streak.set(0);
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
    /// each segment start in `UiRuntime::draw_frame_entered`, BEFORE dirty
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

    /// Record one more consecutive dropped frame and return the new streak
    /// length. See [`Self::frame_failure_streak`]'s field doc.
    pub(crate) fn note_frame_failure(&self) -> u32 {
        let streak = self.frame_failure_streak.get().saturating_add(1);
        self.frame_failure_streak.set(streak);
        streak
    }

    /// Current consecutive frame-drop count without changing it.
    pub(crate) fn frame_failure_streak(&self) -> u32 {
        self.frame_failure_streak.get()
    }

    /// A segment completed without a terminal failure; the next dropped frame
    /// starts a fresh streak. See [`Self::frame_failure_streak`]'s field doc.
    pub(crate) fn reset_frame_failure_streak(&self) {
        self.frame_failure_streak.set(0);
    }

    /// Advance after one terminal frame result (`Painted` or `Errored`).
    pub(crate) fn advance_tree_revision(&self) -> TreeRevision {
        let tree_revision = self.tree_revision.get().next();
        self.tree_revision.set(tree_revision);
        tree_revision
    }

    /// Acknowledge every terminal tree revision through the current one.
    ///
    /// This method is the single commit point where input replay attaches:
    /// callers invoke it only after a painted frame receives a successful
    /// submit classification.
    pub(crate) fn commit_tree_revision(&self) -> TreeRevision {
        let committed_revision = self.tree_revision.get();
        self.presented_revision.set(committed_revision);
        committed_revision
    }

    /// Whether the current terminal tree state has been acknowledged.
    #[must_use]
    pub(crate) fn frame_commit_state(&self) -> FrameCommitState {
        let tree_revision = self.tree_revision.get();
        let presented_revision = self.presented_revision.get();
        assert!(
            presented_revision <= tree_revision,
            "BUG: presented tree revision exceeds terminal tree revision"
        );
        if presented_revision == tree_revision {
            FrameCommitState::Committed
        } else {
            FrameCommitState::Uncommitted {
                since: presented_revision.next(),
            }
        }
    }

    /// Install (or clear) the segment fault-injection probe. See
    /// [`Self::segment_probe`]'s field doc.
    #[cfg(test)]
    pub(crate) fn set_segment_probe(&self, phase: SegmentPhase, probe: Option<Box<dyn Fn()>>) {
        let _prev = std::mem::replace(
            &mut *self.segment_probe.borrow_mut(),
            probe.map(|callback| SegmentProbe { phase, callback }),
        );
    }

    /// Arm the data-only one-shot fault at the build-to-finalize boundary.
    #[cfg(test)]
    pub(crate) fn arm_finalize_phase_panic(&self) {
        self.segment_phase.arm_test_panic_once();
    }

    /// Enter a frame segment, then run its installed test probe, if any.
    ///
    /// The phase write deliberately precedes the probe and has no restoring
    /// guard: if either the probe or segment work unwinds, the last-entered
    /// phase remains available to the presentation-level catch.
    pub(crate) fn enter_segment_phase(&self, phase: SegmentPhase) {
        self.segment_phase.set(phase);
        #[cfg(test)]
        if let Some(probe) = self.segment_probe.borrow().as_ref()
            && probe.phase == phase
        {
            (probe.callback)();
        }
    }

    /// Last frame segment entered by this presentation.
    #[must_use]
    pub(crate) fn segment_phase(&self) -> SegmentPhase {
        self.segment_phase.get()
    }

    /// Data-only marker passed to the widget binding at the exact
    /// build-to-finalize boundary.
    #[must_use]
    pub(crate) fn segment_phase_marker(&self) -> &FramePhaseMarker<SegmentPhase> {
        &self.segment_phase
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
    /// When enabled, this pulls `frames_since(None)`, rebuilds both
    /// histograms and shapes the readout's seven labels through `text`, the
    /// UI runtime's text context, on every composited frame. The cost is bounded — the
    /// telemetry ring is fixed-capacity, so it is O(ring), not O(session) —
    /// and no frame pays it while the overlay is off. But it is not free, and
    /// it lands *inside* the frames the overlay subsequently reports: read the
    /// displayed percentiles as the cost of running with the overlay on, not
    /// as the app's cost without it.
    pub(crate) fn attach_performance_overlay(
        &self,
        layer_tree: &mut LayerTree,
        text: &flui_rendering::TextContextHandle,
    ) {
        let mut slot = self.performance_overlay.borrow_mut();
        let Some(stats) = slot.as_mut() else {
            return;
        };
        let root = layer_tree.root();

        stats.record_frame();

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
        let diagnostic_line = format!(
            "present_p99={present_p99} input_p99={input_p99} deferred={} dropped={} input_truncated={input_truncated}",
            self.clock.produces_deferred(),
            self.frames_dropped(),
        );
        let sample = PerformanceSample {
            fps: stats.fps(),
            frame_time_ms: stats.avg_frame_time_ms(),
            diagnostic_line: Some(&diagnostic_line),
        };

        // The readout's labels are shaped here, through the ui_runtime's text
        // context, so the backend only rasterizes them (ADR-0092). The
        // context is free at scene assembly; were it lent, a debug overlay
        // skips a frame rather than panic it.
        let Some(overlay) = text.try_with(|text| {
            PerformanceOverlayLayer::record(
                text,
                PerformanceOverlayLayer::default_bounds(),
                PerformanceOverlayOption::all(),
                &sample,
            )
        }) else {
            tracing::warn!("performance overlay skipped: the ui_runtime's text context is lent");
            return;
        };

        let _overlay_id = layer_tree.push_child(root, flui_layer::Layer::from(overlay));
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
    /// `UiRuntime::drain_commands` (the sole caller) only runs at a frame
    /// boundary, so nothing should still hold the pipeline checked out by
    /// the time a semantics-action handler runs. The `flui-testing` test
    /// `an_action_sent_off_thread_is_applied_by_the_next_harness_pump` runs
    /// through this assert.
    pub(crate) fn dispatch_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<(), SemanticsActionError> {
        if self.closing_requested.get()
            || matches!(
                self.lifecycle.get(),
                PresentationLifecycle::Closing | PresentationLifecycle::Closed
            )
        {
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
    pub(crate) fn apply_hot_reload(&self, tier: crate::reload::ReloadTier) -> bool {
        use crate::reload::ReloadTier;

        match tier {
            ReloadTier::Reassemble => {
                self.widgets.perform_reassemble();
                self.pipeline
                    .with_mut(flui_rendering::pipeline::PipelineOwner::reassemble);
                tracing::info!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                    "hot reload reassembled element and render trees"
                );
                true
            }
            ReloadTier::Restart => {
                tracing::warn!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                    "HotRestart root remount is not implemented; applying reassemble"
                );
                self.widgets.perform_reassemble();
                self.pipeline
                    .with_mut(flui_rendering::pipeline::PipelineOwner::reassemble);
                true
            }
            ReloadTier::ProcessRestart => {
                tracing::debug!(
                    { flui_foundation::diagnostics::PRESENTATION_ID } = self.id.as_u64(),
                    "FullRestart is owned by the CLI process supervisor"
                );
                false
            }
        }
    }

    /// Healthy close retires input before widget disposal. Exceptional close
    /// withdraws owner admission and key publication but retains the closed
    /// tree, so a caught first failure cannot be displaced by opaque Drop.
    pub(crate) fn preserving_close(&self) -> bool {
        self.close_mode.get() == flui_interaction::__runtime::CloseMode::PreservingFailure
    }

    pub(crate) fn interaction_dispatch(&self) -> Option<&InteractionDispatchHandle> {
        self.interaction_dispatch.as_ref()
    }

    fn close_interaction_with_mode(
        &self,
        lane: Option<&flui_interaction::InteractionLane>,
        mode: flui_interaction::__runtime::CloseMode,
    ) {
        if let Some(handle) = self.interaction_dispatch() {
            if let Some(lane) = lane {
                flui_interaction::__runtime::close_dispatch_in(lane, handle, mode);
            } else {
                flui_interaction::__runtime::close_dispatch(handle, mode);
            }
        }
    }

    /// Withdraw dispatch authority, keeping the captures for
    /// [`flui_interaction::__runtime::retire_dispatch`].
    fn withdraw_interaction(
        &self,
        lane: Option<&flui_interaction::InteractionLane>,
        mode: flui_interaction::__runtime::CloseMode,
    ) -> Option<flui_interaction::__runtime::DispatchCustody> {
        let handle = self.interaction_dispatch()?;
        match lane {
            Some(lane) => flui_interaction::__runtime::withdraw_dispatch_in(lane, handle, mode),
            None => flui_interaction::__runtime::withdraw_dispatch(handle, mode),
        }
    }

    pub(crate) fn close_with_mode_in(
        &self,
        mode: flui_interaction::__runtime::CloseMode,
        lane: &flui_interaction::InteractionLane,
    ) {
        self.close_impl(mode, Some(lane));
    }

    pub(crate) fn close(&self) {
        self.close_with_mode(flui_interaction::__runtime::CloseMode::Ordinary);
    }

    pub(crate) fn close_with_mode(&self, mode: flui_interaction::__runtime::CloseMode) {
        self.close_impl(mode, None);
    }

    fn close_impl(
        &self,
        mode: flui_interaction::__runtime::CloseMode,
        lane: Option<&flui_interaction::InteractionLane>,
    ) {
        use flui_interaction::__runtime::{
            close_focus, close_gestures, close_mouse_tracker, close_text_input,
        };
        // One reentry window spans every owner this close reaches.
        let _lifecycle_window = self.widgets.lifecycle_source().close_window();
        let mut window = flui_interaction::__runtime::CloseWindow::new();
        if let Some(handle) = self.interaction_dispatch() {
            window.dispatch(handle);
        }
        window.gestures(&self.gestures);
        window.focus(&self.focus);
        window.text_input(&self.text_input);
        let mut failure = PresentationCloseRecovery::new(mode, &self.close_mode, window);

        if matches!(
            self.lifecycle.get(),
            PresentationLifecycle::Closing | PresentationLifecycle::Closed
        ) {
            if failure.preserving() {
                self.withdraw_retained_ownership(&mut failure, lane);
            }
            failure.finish();
            return;
        }
        self.lifecycle.set(PresentationLifecycle::Closing);
        let window = self.window.upgrade();
        let bridge = self.accessibility.as_ref().and_then(Weak::upgrade);
        // Every capability is withdrawn before any capture is destroyed: a
        // dispatch target's destructor may hold saved handles, and must find
        // the graph, keys, focus and text input already closed.
        let mode = failure.mode();
        let withdrawn = self.withdrawn_dispatch.borrow_mut().take();
        let dispatch = withdrawn.or_else(|| {
            failure
                .invoke_with(|| self.withdraw_interaction(lane, mode))
                .flatten()
        });
        self.alive.borrow_mut().take();
        self.held_pointer_input.borrow_mut().clear();
        // Owner authority is withdrawn before any final user notification.
        // The withdrawn key owners stay in custody until focus, text input,
        // gestures and mouse tracking are closed too: a key's destructor may
        // hold a saved focus node or text-input handle (ADR-0123).
        let withdrawn_keys = if failure.preserving() {
            failure.invoke(|| self.widgets.withdraw_root_owner(true));
            Vec::new()
        } else {
            failure
                .invoke_with(|| self.widgets.withdraw_owner_authority())
                .unwrap_or_default()
        };
        let agent = self.dev_agent.take();
        if let Some(agent) = &agent {
            agent.withdraw();
        }
        let (announce, event) = self.semantics.take_close_callbacks();

        let source = self.widgets.lifecycle_source();
        source.begin_close();
        let _ = source.commit_terminal(flui_scheduler::AppLifecycleState::Detached);
        failure.run(|| source.drain());
        let mode = failure.mode();
        failure.invoke(|| source.finish_close_with_mode(mode));
        let mode = failure.mode();
        failure.invoke(|| close_mouse_tracker(self.gestures.mouse_tracker(), mode));
        failure.retire(announce);
        failure.retire(event);
        failure.retire(agent);
        let mode = failure.mode();
        failure.invoke(|| close_focus(&self.focus, mode));
        let mode = failure.mode();
        failure.invoke(|| close_text_input(&self.text_input, mode));
        // Gesture cancellation runs recognizer callbacks, so it follows every
        // withdrawal above: a rejected recognizer finds the presentation's
        // graph, keys, agent, focus and text input already closed.
        let mode = failure.mode();
        failure.invoke(|| close_gestures(&self.gestures, mode));
        // Every presentation authority is revoked: the withdrawn keys retire,
        // one at a time, so a failing destructor retains the rest.
        for key in withdrawn_keys {
            failure.retire(key);
        }
        if let Some(dispatch) = dispatch {
            let mode = failure.mode();
            failure.invoke(|| flui_interaction::__runtime::retire_dispatch(dispatch, mode));
        }
        if let Some(bridge) = &bridge {
            failure.invoke(|| bridge.set_activation_listener(Arc::new(|_| {})));
            failure.invoke(|| bridge.set_action_listener(Arc::new(|_| {})));
        }
        if self.pipeline.is_free() {
            failure.invoke(|| {
                self.pipeline.with_mut(|owner| {
                    if owner.semantics_enabled() {
                        owner.set_semantics_enabled(false);
                    }
                });
            });
        }
        if let Some(window) = &window {
            failure.invoke(|| {
                let _ = window.set_cursor(CursorIcon::Default);
            });
        }

        if !failure.preserving() {
            failure.invoke(|| self.widgets.withdraw_root_owner(false));
            failure.run(|| self.widgets.detach_root_widget());
        }
        // Reached in preserving mode, or when the healthy disposal above
        // failed part way: what the tree still owns is retained.
        if failure.preserving() {
            self.withdraw_retained_ownership(&mut failure, lane);
        }
        // The window and the accessibility bridge are framework-owned: they
        // are released even after a failure (ADR-0127).
        failure.release(bridge);
        failure.release(window);

        self.lifecycle.set(PresentationLifecycle::Closed);
        failure.finish();
    }
}

impl PresentationState {
    /// Withdraw this presentation's authority without running user code, as
    /// a UI runtime closing several presentations does for every one of them
    /// before any closes: dispatch targets, liveness, held input, the graph,
    /// rebuild and key authority, agent ports, focus and text input become
    /// unavailable, so a sibling's callbacks cannot drive this presentation
    /// (ADR-0123). The withdrawn key owners are returned for the caller to
    /// retire; the presentation's own close later retires everything else.
    pub(crate) fn withdraw_for_ui_runtime_close(
        &self,
        lane: &flui_interaction::InteractionLane,
    ) -> Vec<flui_view::__runtime::WithdrawnKey> {
        use flui_view::__runtime::BindingRuntime as _;
        if matches!(
            self.lifecycle.get(),
            PresentationLifecycle::Closing | PresentationLifecycle::Closed
        ) {
            return Vec::new();
        }
        if self.withdrawn_dispatch.borrow().is_none()
            && let Some(custody) = self.withdraw_interaction(Some(lane), self.close_mode.get())
        {
            self.withdrawn_dispatch.replace(Some(custody));
        }
        self.alive.borrow_mut().take();
        self.held_pointer_input.borrow_mut().clear();
        let keys = self.widgets.withdraw_owner_authority();
        if let Some(agent) = &*self.dev_agent.borrow() {
            agent.withdraw();
        }
        flui_interaction::__runtime::withdraw_focus(&self.focus);
        flui_interaction::__runtime::withdraw_text_input(&self.text_input);
        keys
    }

    /// Withdraws the root owner and closes every input owner in preserving
    /// mode, so the values they still hold are retained rather than dropped
    /// (ADR-0123, ADR-0127).
    fn withdraw_retained_ownership(
        &self,
        failure: &mut PresentationCloseRecovery<'_>,
        lane: Option<&flui_interaction::InteractionLane>,
    ) {
        use flui_interaction::__runtime::{
            CloseMode, close_focus, close_gestures, close_mouse_tracker, close_text_input,
        };
        failure.invoke(|| self.widgets.withdraw_root_owner(true));
        failure.invoke(|| close_gestures(&self.gestures, CloseMode::PreservingFailure));
        failure.invoke(|| {
            close_mouse_tracker(self.gestures.mouse_tracker(), CloseMode::PreservingFailure);
        });
        failure.invoke(|| close_focus(&self.focus, CloseMode::PreservingFailure));
        failure.invoke(|| close_text_input(&self.text_input, CloseMode::PreservingFailure));
        failure.invoke(|| self.close_interaction_with_mode(lane, CloseMode::PreservingFailure));
    }
}

struct PresentationCloseRecovery<'a> {
    first: Option<Box<dyn std::any::Any + Send>>,
    terminal: &'a Cell<flui_interaction::__runtime::CloseMode>,
    window: flui_interaction::__runtime::CloseWindow,
}

impl<'a> PresentationCloseRecovery<'a> {
    fn new(
        mode: flui_interaction::__runtime::CloseMode,
        terminal: &'a Cell<flui_interaction::__runtime::CloseMode>,
        window: flui_interaction::__runtime::CloseWindow,
    ) -> Self {
        if mode == flui_interaction::__runtime::CloseMode::PreservingFailure
            || std::thread::panicking()
        {
            terminal.set(flui_interaction::__runtime::CloseMode::PreservingFailure);
        }
        if terminal.get() == flui_interaction::__runtime::CloseMode::PreservingFailure {
            window.preserve();
        }
        Self {
            first: None,
            terminal,
            window,
        }
    }
    fn mode(&self) -> flui_interaction::__runtime::CloseMode {
        self.terminal.get()
    }
    fn preserving(&self) -> bool {
        self.mode() == flui_interaction::__runtime::CloseMode::PreservingFailure
    }
    fn invoke(&mut self, callback: impl FnOnce()) {
        let _ = self.invoke_with(callback);
    }
    fn invoke_with<T>(&mut self, callback: impl FnOnce() -> T) -> Option<T> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
            Ok(value) => Some(value),
            Err(payload) => {
                if self.preserving() {
                    flui_foundation::panic::retain_opaque_payload(payload);
                } else {
                    crate::lifecycle_state::preserve_first_lifecycle_panic(
                        &mut self.first,
                        Some(payload),
                        "presentation terminal cleanup",
                    );
                }
                self.terminal
                    .set(flui_interaction::__runtime::CloseMode::PreservingFailure);
                self.window.preserve();
                None
            }
        }
    }
    fn run(&mut self, callback: impl FnOnce()) {
        if !self.preserving() {
            self.invoke(callback);
        }
    }
    /// Drops a user-owned value, or retains it once the close is preserving.
    fn retire<T>(&mut self, value: T) {
        if self.preserving() {
            std::mem::forget(value);
        } else {
            self.invoke(|| drop(value));
        }
    }
    /// Drops a framework-owned handle even after a failure; only an unwind
    /// already in progress retains it.
    fn release<T>(&mut self, value: T) {
        if std::thread::panicking() {
            std::mem::forget(value);
        } else {
            self.invoke(|| drop(value));
        }
    }
    fn finish(self) {
        if let Some(payload) = self.first {
            if std::thread::panicking() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                std::panic::resume_unwind(payload);
            }
        }
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
            PipelineCell::new(PipelineOwner::new(
                flui_rendering::TextContextHandle::standalone(),
            )),
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

    /// While the UI runtime's text context is lent, the overlay skips the frame
    /// and leaves the tree as it was, rather than panic on the borrow; once
    /// the loan ends, the next frame attaches it. Fails if the overlay
    /// borrows the context unconditionally (a panic), or attaches an
    /// unshaped readout while the context is lent.
    #[test]
    fn a_lent_text_context_skips_the_overlay_frame() {
        let presentation = presentation();
        presentation.set_performance_overlay(true);
        let text = flui_rendering::TextContextHandle::standalone();
        let mut tree = LayerTree::new(flui_layer::Layer::from(flui_layer::OffsetLayer::zero()));

        text.with(|_| presentation.attach_performance_overlay(&mut tree, &text));
        assert_eq!(tree.len(), 1, "a lent context attaches no overlay");

        presentation.attach_performance_overlay(&mut tree, &text);
        assert_eq!(tree.len(), 2, "a free context attaches the overlay");
        assert!(
            tree.iter()
                .any(|(_, node)| node.layer().as_performance_overlay().is_some()),
            "the attached child is the overlay"
        );
    }
}
