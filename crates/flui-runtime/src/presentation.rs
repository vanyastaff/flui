//! Owner-thread state for one presentation of a UI ui_runtime.
//!
//! Public only so the host that drives a UI runtime (`flui-app`) can name it; it
//! is not an embedder API (ADR-0027 §9). It is the UI-owner domain, not a
//! cross-thread god object: native event-loop ownership remains in the
//! runner/window host and raster/surface ownership remains in
//! `flui_engine::RasterExecutor`.

use std::cell::{Cell, RefCell};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};

use flui_animation::__runtime::{VsyncDriver, VsyncRetirement};
use flui_animation::{FrameTick, MotionClock};
use flui_foundation::PresentationId;
use flui_interaction::{
    binding::GestureBinding,
    focus::FocusManager,
    pointer::InteractionDispatchHandle,
    text_input::{TextInputBackend, TextInputHandle, TextInputOwner},
};
#[cfg(any(test, feature = "testing"))]
use flui_platform::PlatformTextInput;
use flui_platform::text::store::TextStoreHost;
use flui_platform::{Cursor, CursorError, CursorIcon, PlatformWindow};
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::{
    AsyncDriver, ClockSource, FrameClock, FramePhaseDurations, PresentationFrameScope,
    UpdateScheduler, frame_pace, input_to_present_histogram, produce_to_present_histogram,
};
use flui_semantics::platform::PlatformAccessibility;
use flui_view::__runtime::FramePhaseMarker;
use flui_view::__runtime::{BuildInProgress, CloseGuardSource, GlobalKeyScope, WidgetsBinding};
use web_time::{Duration, Instant};

use crate::epoch::TreeRevision;
use crate::frame_failure::SegmentPhase;
use crate::held_input::HeldPointerQueue;
use crate::occluded_frame::{AskedFrame, OccludedFrame};
use crate::renderer_binding::RenderingBinding;
use crate::semantics_host::SemanticsHost;
use crate::ui_runtime::text_preparation::TextPreparation;

mod accessibility;
mod assembly;
mod close;
mod frame_demand;
mod frame_failure;
mod input;
mod motion;
mod segment;

fn format_millis(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1_000.0)
}

/// Show `cursor`, the cursor the hovered region asks for, on `window`.
///
/// A window that refuses an application's image (one that shows none, by its
/// `cursor_image` capability, or whose window system failed to make a cursor
/// of the pixels) shows the arrow instead, so the region does not keep the
/// cursor of the region the pointer came from. A window with no pointer
/// cursor at all refuses both, which is no failure.
fn show_cursor(
    window: &dyn PlatformWindow,
    cursor: &Cursor,
    id: PresentationId,
    device_id: flui_platform::input::pointer::PointerSource,
) {
    let refused = match window.set_cursor(cursor) {
        Ok(()) => return,
        Err(error) if matches!(cursor, Cursor::Image(_)) => {
            tracing::debug!(
                ?id,
                ?device_id,
                ?error,
                "the window refused the cursor image: it shows the arrow over the region"
            );
            match window.set_cursor(&Cursor::Icon(CursorIcon::Default)) {
                Ok(()) => return,
                Err(error) => error,
            }
        }
        Err(error) => error,
    };
    match refused {
        CursorError::Unsupported(unsupported) => {
            tracing::trace!(
                ?id,
                ?device_id,
                ?cursor,
                reason = %unsupported.reason,
                "window backend has no pointer-cursor facility"
            );
        }
        error => {
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
    /// Which build is running across this UI runtime's presentations (ADR-0074).
    pub(crate) build_in_progress: BuildInProgress,
    /// A `Weak` handle to the UI runtime's shared async tasks (UI runtime-level; see
    /// the presentation-teardown contract for the consequence of that when
    /// this presentation closes).
    pub(crate) async_driver: AsyncDriver,
    /// Completion authority for this presentation's coherent geometry.
    pub(crate) completion_scope: PresentationFrameScope,
    /// The UI runtime's interaction dispatch lane.
    pub(crate) interaction_dispatch_handle: InteractionDispatchHandle,
    /// The UI runtime's own scheduler — borrowed only for the duration of
    /// assembly; the constructed [`RenderingBinding`] keeps just a
    /// `WeakUpdateScheduler` derived from it.
    pub(crate) scheduler: &'a UpdateScheduler,
    /// The UI runtime's platform wake capability. It is wired as the UI runtime
    /// scheduler's `on_frame_scheduled` hook (in `UiRuntime::construct`) and
    /// handed to the platform accessibility bridge; the presentation's own
    /// pipeline wake routes through the scheduler rather than cloning
    /// this directly.
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    /// A cross-thread sender into the UI runtime's command inbox, already
    /// stamped with this presentation's id. Handed to the platform
    /// accessibility bridge's action listener, so an assistive-technology
    /// request marshals onto the owner thread as a
    /// [`SemanticsActionRequest`](flui_semantics::SemanticsActionRequest) and resolves at the next Idle drain —
    /// never on the adapter's own thread.
    pub(crate) command_sender: super::ui_runtime::UiCommandSender,
    /// The providers this presentation's widgets acquire platform
    /// capabilities from, and where their completions settle (ADR-0152).
    pub(crate) capabilities: crate::capability::CapabilityRegistry,
    pub(crate) completions: flui_platform::capability::CompletionSource,
    /// What providers keep for the UI runtime (ADR-0152 §5).
    pub(crate) locals: flui_platform::capability::RuntimeLocals,
    /// What the presentation closes and persists through: its close guard's
    /// vetoes and signal, and the host's flush registry.
    pub(crate) close: crate::runtime_services::CloseServices,
    /// Where the UI runtime reads time: this presentation's gesture arena and
    /// [`FrameClock`] read the same source as the UI runtime's frame clock.
    pub(crate) clock: &'a ClockSource,
    /// The UI runtime's text context, installed on the presentation's pipeline so
    /// its layout measures text through the UI runtime (ADR-0092 §10 step 3).
    pub(crate) text: flui_rendering::TextContextHandle,
    /// The UI runtime's drag sessions, which this presentation's widgets reach
    /// through `LifecycleContext::drag_and_drop`.
    pub(crate) drag: flui_interaction::drag::DragHandle,
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
    ActiveContact(#[from] flui_interaction::binding::ResamplingModeChangeError),
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
    window: Rc<dyn PlatformWindow>,
    accessibility: Option<Arc<dyn PlatformAccessibility>>,
    text_store_host: Option<Rc<dyn TextStoreHost>>,
    pointer_resampling: PointerResampling,
    gesture_profiles: flui_interaction::gesture::GestureProfiles,
    platform: flui_platform::TargetPlatform,
    occluded_surface: flui_platform::OccludedSurface,
}

impl std::fmt::Debug for PresentationWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationWindow")
            .field("window", &self.window.id())
            .field("pointer_resampling", &self.pointer_resampling)
            .field("accessibility", &self.accessibility.is_some())
            .field("text_store_host", &self.text_store_host.is_some())
            .field("gesture_profiles", &self.gesture_profiles)
            .field("platform", &self.platform)
            .field("occluded_surface", &self.occluded_surface)
            .finish()
    }
}

impl PresentationWindow {
    /// Pairs `window` with the bridge its backend exposes, if any.
    pub fn new(
        window: Rc<dyn PlatformWindow>,
        accessibility: Option<Arc<dyn PlatformAccessibility>>,
    ) -> Self {
        Self {
            window,
            accessibility,
            text_store_host: None,
            pointer_resampling: PointerResampling::Disabled,
            gesture_profiles: flui_interaction::gesture::GestureProfiles::preset(
                flui_platform::TargetPlatform::Unknown,
            ),
            platform: flui_platform::TargetPlatform::Unknown,
            occluded_surface: flui_platform::OccludedSurface::Discarded,
        }
    }

    /// What the window system does with the window's last frame while it
    /// keeps the window off the screen, as the host names it; discarded by
    /// default. Only a window whose surface is kept owes a frame while it is
    /// covered ([`OccludedSurface`](flui_platform::OccludedSurface), ADR-0357).
    #[must_use]
    pub fn with_occluded_surface(mut self, surface: flui_platform::OccludedSurface) -> Self {
        self.occluded_surface = surface;
        self
    }

    /// The platform whose conventions this presentation's tree follows: the
    /// runtime publishes it above the root as a
    /// [`PlatformConvention`](flui_view::environment::PlatformConvention).
    /// Without one, [`TargetPlatform::Unknown`](flui_platform::TargetPlatform::Unknown)
    /// and its platform-neutral conventions: a runner names the platform its
    /// host reports, and a headless test the one it models.
    #[must_use]
    pub fn with_platform(mut self, platform: flui_platform::TargetPlatform) -> Self {
        self.platform = platform;
        self
    }

    /// Choose this presentation's initial input policy; disabled by default.
    #[must_use]
    pub fn with_pointer_resampling(mut self, policy: PointerResampling) -> Self {
        self.pointer_resampling = policy;
        self
    }

    /// The gesture presets this presentation's host observations resolve
    /// over (ADR-0212 §4), usually `GestureProfiles::preset(platform)`: each
    /// contact is admitted, and hit-tested, with its pointer kind's profile.
    ///
    /// Without one, the platform-neutral presets apply, one per pointer kind,
    /// never the build host's: a runner names its platform, and a headless
    /// test its preset (one [`GestureSettings`](flui_interaction::gesture::GestureSettings)
    /// value is one profile for every kind).
    #[must_use]
    pub fn with_gesture_profiles(
        mut self,
        profiles: impl Into<flui_interaction::gesture::GestureProfiles>,
    ) -> Self {
        self.gesture_profiles = profiles.into();
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
    pub fn window(&self) -> &Rc<dyn PlatformWindow> {
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
/// window is a headless `HeadlessHostWindow` carrying a `FakeAccessibility`. A test
/// that needs the bridge wired names it in [`PresentationWindow::new`]
/// instead.
#[cfg(any(test, feature = "testing"))]
impl From<Rc<dyn PlatformWindow>> for PresentationWindow {
    fn from(window: Rc<dyn PlatformWindow>) -> Self {
        Self::new(window, None)
    }
}

/// The `From<Rc<dyn PlatformWindow>>` conversion above
/// for a borrowed window, for tests that reuse one window across several
/// installs; it bypasses the window's bridge the same way.
#[cfg(any(test, feature = "testing"))]
impl From<&Rc<dyn PlatformWindow>> for PresentationWindow {
    fn from(window: &Rc<dyn PlatformWindow>) -> Self {
        Self::new(Rc::clone(window), None)
    }
}

/// A UI runtime-backed test window carrying an optional platform text-input
/// capability — for `UiRuntime::for_test_with_text_input`, which needs a real
/// [`RuntimeCapabilities`]-assembled presentation (not the standalone
/// `PresentationState::new_for_test` path), just with a test window.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn test_platform_window(
    platform_text_input: Option<Arc<dyn PlatformTextInput>>,
) -> Rc<dyn PlatformWindow> {
    use flui_platform::testing::MockWindow;
    let window = MockWindow::new().focused(true);
    Rc::new(match platform_text_input {
        Some(text_input) => window.with_text_input(text_input),
        None => window,
    })
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

/// Where a window's content sits on the desktop, as a published
/// [`WindowState`](flui_platform::WindowState) reports it: the outer
/// top-left corner, the display, and what frame offsets the content
/// from that corner. A change re-reads the window's gesture geometry for new
/// contacts (ADR-0212 §2) and hands it its input regions again (ADR-0217 §5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContentPlacement {
    position: Option<flui_platform::WindowPosition>,
    display: Option<flui_platform::DisplayId>,
    frame: flui_platform::WindowFrame,
}

impl ContentPlacement {
    pub(crate) fn of(state: &flui_platform::WindowState) -> Self {
        Self {
            position: state.outer.position(),
            display: state.display,
            frame: state.frame,
        }
    }
}

/// What a segment probe runs, handed the presentation whose segment entered
/// the probed phase.
#[cfg(test)]
type SegmentProbeCallback = Box<dyn Fn(&PresentationState)>;

/// Phase-addressed frame fault injection used by the containment tests.
#[cfg(test)]
struct SegmentProbe {
    phase: SegmentPhase,
    callback: SegmentProbeCallback,
}

/// A presentation's wait before it tries a dropped frame again.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FailureRetry {
    /// The instant the wait ends; `None` when nothing waits.
    at: Option<Instant>,
    /// Whether a key or button press already tried early since the wait last
    /// ran out. Cleared when it does, and by a frame that succeeds.
    early_spent: bool,
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
    /// The platform whose conventions this presentation's tree follows.
    pub(super) platform: flui_platform::TargetPlatform,
    /// Presentation-local projection of the host's accepted interaction policy.
    /// Recognizers read it at admission; active sequences keep owned snapshots.
    pub(super) gesture_settings: flui_interaction::gesture::GestureSettingsSource,
    pub(crate) gesture_geometry: RefCell<crate::ui_runtime::preferences::GeometryProjection>,
    pub(crate) text_preparation: RefCell<TextPreparation>,
    pub(crate) sampled_motion_tick: Cell<Option<FrameTick>>,
    /// The refresh rate this presentation asks its display for.
    pub(crate) frame_rate: Cell<crate::frame_rate_demand::DemandGovernor>,
    pub(super) window_visible: Cell<bool>,
    pub(super) window_focused: Cell<bool>,
    /// The display this presentation's window is on, as the host last derived it
    /// from the platform's snapshot (ADR-0236). `None` is an unknown display.
    /// Shared with the haptics host, whose frame deadline reads its period.
    pub(super) window_display: Rc<Cell<Option<flui_platform::WindowDisplay>>>,
    pub(super) window_execution: Cell<flui_platform::WindowExecutionState>,
    pub(super) closing_requested: Cell<bool>,
    /// Tells the providers that answer this window later (a dialog modal to
    /// it) that it closed: closed when the close begins, and on drop.
    pub(super) window_closer: flui_platform::capability::WindowCloser,
    /// The request of the frame being delivered, on the event timeline
    /// (`HostInput::Frame`): the interaction layer samples motion at its vsync and the pump takes
    /// the frame's time from it (ADR-0347).
    pub(super) frame_request: Cell<Option<flui_platform::FrameRequest>>,
    /// The soft keyboard's occlusion as the window last reported it: sampled
    /// at each frame's vsync into `MediaQueryData::view_insets` while it moves
    /// (ADR-0277 §3).
    pub(super) keyboard: Cell<flui_platform::SoftKeyboard>,
    /// The system-bar overlay style its frames ask for, delivered on change.
    pub(crate) overlay_style: crate::overlay_style::OverlayStyleCoalescer,
    /// The retained scene its frames patch and its sink draws (ADR-0304 §8).
    pub(crate) scene: crate::presentation_scene::SceneSlot,
    /// External textures its widgets register; the host drains it before each frame renders.
    textures: flui_layer::TextureRegistry,
    lifecycle: Cell<PresentationLifecycle>,
    close_mode: Cell<flui_interaction::__runtime::CloseMode>,
    /// Dispatch captures a UI runtime-wide withdrawal took before this
    /// presentation's own close, which retires them.
    withdrawn_dispatch: RefCell<Option<flui_interaction::__runtime::DispatchCustody>>,
    pipeline: PipelineCell,
    /// Only the legacy test-only, scheduler-free constructor lacks this owner.
    completion_scope: Option<PresentationFrameScope>,
    /// This presentation's liveness, as a token others may watch weakly.
    ///
    /// Dropping the presentation drops it, which is the point: nothing else
    /// reliably says "closed". The pipeline's own allocation does not —
    /// `LifecycleContext::pipeline_owner()` hands out a strong `PipelineCell`, so
    /// a widget that stores one keeps the tree alive past the close — and
    /// under `WindowPolicy::Shared` the UI runtime outlives any single presentation too.
    alive: RefCell<Option<Rc<()>>>,
    window: std::rc::Weak<dyn PlatformWindow>,
    /// The window's identity, read once at assembly: recordings address input
    /// by it even after the window itself is gone (ADR-0215 §1).
    window_id: flui_platform::WindowId,
    /// The window's accessibility bridge, if its backend has one. `Weak`
    /// like [`Self::window`]: the backend window owns the bridge, and this
    /// presentation must not keep it alive past the window.
    accessibility: Option<Weak<dyn PlatformAccessibility>>,
    gestures: GestureBinding,
    /// The frame queue, outcome mailbox and observer of this presentation's
    /// haptics (ADR-0210), on the gesture clock.
    haptics: flui_interaction::haptics::HapticsHost,
    pub(crate) wheel_preferences: flui_interaction::pointer::WheelPreferencesSource,
    interaction_dispatch: Option<InteractionDispatchHandle>,
    /// Pointer input retained while this presentation has no committed tree.
    /// The queue is owner-thread-only and internally capped; replay detaches
    /// its batch before invoking dispatch so callbacks may enqueue reentrantly.
    held_pointer_input: RefCell<HeldPointerQueue>,
    /// The input regions collected when this frames scene was built, handed to the
    /// window once the sink accepts the scene (ADR-0217 §5).
    staged_input_regions: RefCell<Rc<[flui_platform::input::regions::InputRegion]>>,
    /// The input regions of the last committed frame, handed to the window again when
    /// it moves, so its backend can re-project them for a new display.
    published_input_regions: RefCell<Rc<[flui_platform::input::regions::InputRegion]>>,
    focus: Rc<FocusManager>,
    text_input: Rc<TextInputOwner>,
    /// This presentation's semantics enablement gate and platform
    /// accessibility delivery. `close()` clears its announce/event
    /// callbacks unconditionally (production write); attaching the platform
    /// bridge reads `platform_semantics_enabled_handle()` to wire the bridge's
    /// activation listener (production read) — announce/event delivery itself
    /// still has no production caller.
    semantics: SemanticsHost,
    /// The semantics agent a development agent hook reads this presentation
    /// through, once `UiRuntime::dev_agent_window` has vended it. The hook's
    /// `AgentWindow`s hold it weakly, so dropping it here (at close, or with
    /// the presentation) turns every handle `gone`; they hold the semantics
    /// handle themselves, so collection lasts while the hook keeps one.
    /// Owner thread only.
    pub(crate) dev_agent: RefCell<Option<crate::ui_runtime::DevAgentSlot>>,
    /// The development agent's owner side, once a window of this presentation was vended; never
    /// cleared, so the jobs an agent left are answered after the slot above goes (ADR-0402).
    pub(crate) agent_side: Cell<Option<&'static dyn crate::ui_runtime::AgentSide>>,
    /// The bounds the agent's actionability check last read, to tell whether
    /// an element holds still. Owner thread only.
    pub(crate) agent_stability: RefCell<crate::ui_runtime::AgentStability>,
    /// Actions the agent ran that owe a reading of the tree once a frame has
    /// passed. Answered by `UiRuntime::complete_agent_reads` after a frame,
    /// and as failed when the presentation closes first. Owner thread only.
    pub(crate) agent_reads: RefCell<Vec<crate::ui_runtime::PendingActRead>>,
    /// The synthetic input an agent injects: what each input lease holds, and the injections
    /// and releases queued. Advanced at each frame boundary; its queued work is answered `gone`
    /// when the presentation closes. Owner thread only.
    pub(crate) agent_input: RefCell<crate::ui_runtime::AgentInput>,
    /// Gestures and scrolls an agent asked for that hold out at frame boundaries until their
    /// target is ready or their timeout passes. Answered `gone` when the presentation closes
    /// first. Owner thread only.
    pub(crate) agent_acts: RefCell<crate::ui_runtime::HeldActs>,
    /// Waits an agent parked: conditions held out for until a frame makes them hold or
    /// their deadline passes. Answered `gone` when the presentation closes first. Owner
    /// thread only.
    pub(crate) agent_waits: RefCell<Vec<crate::ui_runtime::PendingWait>>,
    /// The presentation's URL-owner slot (ADR-0330 §5): the outermost Router of its tree
    /// registers into it through `LifecycleContext::url_owner`, and
    /// `UiRuntime::navigate` reaches that Router through it. Owner thread only.
    pub(crate) url_owner: flui_view::__runtime::UrlOwnerSlot,
    /// Reads of the layer tree parked for the next scene this window shows (owner thread only).
    pub(crate) agent_layer_reads: RefCell<Vec<crate::ui_runtime::PendingLayerRead>>,
    /// The frames handed to a sink that answered `Queued`, oldest first: what each commits once
    /// its settlement arrives (ADR-0305 §4). Owner thread only.
    pub(crate) pending_frames: RefCell<std::collections::VecDeque<crate::ui_runtime::PendingFrame>>,
    /// How many frames the sink this presentation's last frame reached may hold unsettled
    /// (`FrameSink::in_flight_limit`): the in-flight gate paces the presentation by it (ADR-0305 §5).
    in_flight_limit: Cell<NonZeroUsize>,
    /// The subscriptions an agent holds to this presentation's events, and how far they have
    /// been reported. Ended when the presentation closes. Owner thread only.
    pub(crate) agent_events: RefCell<crate::ui_runtime::EventHub>,
    /// The window this presentation is shown in, as the host controls it, once
    /// `UiRuntime::install_window` has attached it. Closed before the
    /// presentation's terminal notification, so a handle held past the close
    /// answers closed. Owner thread only.
    pub(crate) window_control: RefCell<Option<std::rc::Rc<flui_view::__runtime::WindowSource>>>,
    /// Where the window's content sat in the state last published through
    /// `window_control`: a publication that changes it is the window moving.
    /// Owner thread only.
    pub(crate) window_placement: Cell<Option<ContentPlacement>>,
    /// Owner-local widget framework state. One instance per presentation
    /// (ADR-0043), not one per UI runtime; every widget-tree operation for this surface enters
    /// through this presentation and activates this binding's own GlobalKey
    /// registry (composed into the UI runtime's whole-frame composite by
    /// `UiRuntime::enter`, never activated standalone in production).
    widgets: WidgetsBinding,
    /// The presentation's close guard: its widgets hold the close through
    /// [`CloseGuard`](flui_view::CloseGuard), the host asks it whether a close
    /// may proceed, and the owner settles it on its turn. One per
    /// presentation, so it outlives every remount of the root.
    close_guard: CloseGuardSource,
    /// This presentation's root pipeline owner, its hit testing and its
    /// first-frame gate: per-presentation facts, not shareable once a UI
    /// runtime hosts more than one presentation.
    renderer: RenderingBinding,
    /// Total frames rendered successfully for this presentation:
    /// per-window frame accounting, beside its consumer
    /// [`Self::performance_overlay`].
    /// `Cell`, not `AtomicU64`: `PresentationState` is owner-thread-confined
    /// (`!Send` transitively, via `Rc`-backed fields), so an atomic buys
    /// nothing here that `Cell`'s cheaper interior mutability does not
    /// already give `&self` callers.
    frames_rendered: Cell<u64>,
    /// Frames dropped due to surface errors. See [`Self::frames_rendered`].
    frames_dropped: Cell<u64>,
    /// Consecutive frames this presentation produced and handed to the
    /// backend that the backend could not put on screen, reset by a frame
    /// that presents or finds no damage and when the window is shown again
    /// after it was hidden.
    ///
    /// Bounds the retention of a withheld frame
    /// ([`Self::record_frame_withheld`]'s caller decides how long to keep
    /// retrying). A transient — the drawable is briefly unavailable while
    /// the compositor rearranges itself — must be ridden out, because the
    /// scene that was consumed producing it is gone and nothing else will
    /// redraw it. A *permanent* unavailability must not be, because the
    /// retry dirties this presentation on every attempt and would otherwise
    /// spin at the fallback pace indefinitely: past the budget each attempt
    /// waits out the dropped-frame backoff (ADR-0335 §2).
    not_shown_streak: Cell<u32>,
    /// Whether a withheld streak past its budget was reported
    /// ([`Self::first_withdrawal_past_budget`]): once per presentation, so a
    /// window covered again and again does not repeat it.
    withheld_reported: Cell<bool>,
    /// Whether this presentation owes its surface a frame while it cannot be
    /// seen, and since when it has drawn none ([`OccludedFrame`]).
    occluded_frame: Cell<OccludedFrame>,
    /// What the window was last asked for the frame it owes (ADR-0389), and
    /// whether that frame was held once its instant came. Shared with the
    /// [`OwedFrameRequest`](crate::occluded_frame::OwedFrameRequest) that records a delivery.
    owed_frame_asked: Rc<Cell<AskedFrame>>,
    /// Whether a frame of this presentation's window, delivered since its
    /// owed instant came, serves the frame it owes and no attempt has taken
    /// it yet (ADR-0389).
    owed_frame_served: Cell<bool>,
    /// What the window system does with the window's last frame while it
    /// keeps the window off the screen, as the host named it.
    occluded_surface: flui_platform::OccludedSurface,
    /// Whether the performance overlay is drawn. Its numbers come from the
    /// clock's frame history, which is kept either way.
    performance_overlay: Cell<bool>,
    /// The build a development tool reported last, drawn over the scene
    /// (`build.report`): the one state of the presentation an annotating
    /// agent method reaches. Owner thread only.
    pub(crate) build_banner: crate::build_overlay::BuildBanner,
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
    /// Whether the development agent asked for a frame of this presentation since the last
    /// pump began: a held act's next boundary, a wait that counts frames, a reading an action
    /// owes. It drives a frame as a redraw does but is no work of the application's, so the
    /// list of what keeps the application from being idle carries it as a wake alone
    /// (`UiRuntime::request_agent_frame_for`, ADR-0400 §3).
    pub(crate) agent_frame_asked: Cell<bool>,
    /// How many times [`Self::redraw_pending`] was marked, so the frame tail
    /// can tell a mark raised during the frame from one left by an earlier pump.
    redraw_requests: Cell<u64>,
    /// [`Self::redraw_requests`] when the current frame began.
    redraw_requests_watched: Cell<u64>,
    /// Set by the first platform redraw request this presentation makes for
    /// the next frame; reopened when a pump samples the presentation.
    redraw_poked: Arc<AtomicBool>,
    /// The driver of this presentation's controller registry; its tree holds
    /// only handles, and the presentation keeps the binding for its whole lifetime.
    vsync_driver: VsyncDriver,
    /// Kernels close before any teardown callout; delivery outlives opaque trees.
    animation_retirement: RefCell<Option<VsyncRetirement>>,
    /// This presentation's animation clock: maps the UI runtime's raw frame time
    /// to the monotonic animation time [`Self::vsync_driver`] ticks with.
    /// Borrowed only inside [`Self::motion_tick`], never across user code.
    motion_clock: RefCell<MotionClock>,
    /// This presentation's own physical-time produce-gate state machine
    /// — the per-presentation half of the `UpdateScheduler`/
    /// `FrameClock`/raster three-owner split. `UiRuntime::draw_frame_entered`'s
    /// per-presentation segment loop polls this rather than
    /// `take_redraw_pending() || has_pending_work()` directly;
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
    /// shared `FrameClock`: a `pub` field on a type
    /// every presentation shares would let one presentation's caller read
    /// or clobber a SIBLING's in-flight span; keeping it private to this
    /// presentation makes that structurally impossible. [`Self::
    /// take_last_segment_span`] reads AND CLEARS it (never a plain `get`):
    /// a pump where this presentation's segment did NOT run must see
    /// `None` here, never a stale span latched by an earlier pump this
    /// presentation was the one to produce.
    last_segment_span: Cell<Option<(Instant, Instant)>>,
    /// How long this presentation's most recent segment spent building,
    /// laying out and painting; recorded with [`Self::last_segment_span`].
    segment_phases: Cell<FramePhaseDurations>,
    /// Latest terminal (`Painted` or `Errored`) tree revision.
    tree_revision: Cell<TreeRevision>,
    /// Latest tree revision acknowledged by a successful submit verdict.
    presented_revision: Cell<TreeRevision>,
    /// Consecutive dropped-frame count for this presentation — the
    /// `consecutive_failures` field of every
    /// [`FrameFailureReport`](super::frame_failure::FrameFailureReport)
    /// this presentation produces. A terminal pipeline error, an escaped
    /// segment panic or a submit that settles `Failed` increments it. A
    /// contained lifecycle recovery reports the current value but neither
    /// increments nor resets it. A segment that completes without a terminal
    /// failure and paints nothing, or whose scene reaches no sink, resets it,
    /// after that attempt's contained reports are delivered; a painted
    /// scene resets it once its verdict
    /// commits the current tree (ADR-0334 §2). Presentation-local on
    /// purpose: one window's streak must never color a sibling's reports.
    frame_failure_streak: Cell<u32>,
    /// When this presentation may next try a frame it dropped, and whether a
    /// key or button press already tried early in this interval. See
    /// [`FAILURE_RETRY_CAP`](crate::frame_failure::FAILURE_RETRY_CAP).
    failure_retry: Cell<FailureRetry>,
    /// Where the rebuild of this presentation's lost GPU device stands, as its
    /// renderer's owner last said; `None` while the device is healthy. A given-up
    /// rebuild holds this presentation's frames until a new rebuild is asked
    /// for (ADR-0267).
    device_recovery: Cell<Option<crate::frame_failure::DeviceRecovery>>,
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
    /// The presentation's close guard.
    #[must_use]
    pub(crate) fn close_guard(&self) -> &CloseGuardSource {
        &self.close_guard
    }

    /// This presentation's own widget framework state.
    #[must_use]
    pub(crate) fn widgets(&self) -> &WidgetsBinding {
        &self.widgets
    }

    /// The fresh hit test over this presentation's tree that drags use to find
    /// its receivers (ADR-0211).
    pub(crate) fn hit_test_handle(&self) -> flui_interaction::pointer::HitTestHandle {
        self.widgets
            .with_build_owner(|owner| owner.hit_test_handle().cloned())
            .expect("BUG: every presentation pairs its hit-test handle when it is assembled")
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

    pub(crate) fn set_error_view_builder(&self, builder: Option<flui_view::ErrorViewBuilder>) {
        self.widgets
            .with_build_owner_mut(|owner| owner.capabilities_mut().error_view_builder = builder);
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
        not(any(test, feature = "testing")),
        expect(
            dead_code,
            reason = "Self::new installs the text input capability from the local \
                      text_input binding directly (before self exists to call \
                      this wrapper through); this accessor's one production \
                      caller moved inline when assembly moved into this \
                      constructor, kept for tests and any future external caller"
        )
    )]
    pub(crate) fn text_input_handle(&self) -> TextInputHandle {
        self.text_input.handle()
    }

    /// The external textures this presentation's widgets register.
    pub(crate) fn texture_registry(&self) -> &flui_layer::TextureRegistry {
        &self.textures
    }

    /// Whether a texture update waits that the held version owes a
    /// publication for: a retirement, or a frame of a texture it draws, while
    /// the version was applied for the target the pipeline now lays out for
    /// and the window does not refuse it (ADR-0304 §8, ADR-0334 §4). A frame
    /// finding no tree work publishes the held version for it, and the
    /// registry holds the update until the host takes it with a publication,
    /// so a frame that never reached the host leaves it for the next. A
    /// target change owes nothing here: the repaint it causes carries the
    /// update, and a repaint that fails leaves no texture work to spin on.
    /// The one predicate the runner's gate, sampling admission and the
    /// frame's republish read; `false` while a frame holds the scene.
    pub(crate) fn owes_texture_frame(&self) -> bool {
        let target = self.pipeline.with(|owner| {
            owner.root_constraints().map(|constraints| {
                crate::presentation_scene::SceneTarget::of(constraints, owner.device_pixel_ratio())
            })
        });
        self.scene.owes_texture_frame(&self.textures, target)
    }

    /// This presentation's semantics enablement gate and platform
    /// accessibility delivery — the per-window home the retired
    /// `SemanticsBinding` singleton's enablement/announce/event state moved
    /// into. `UiRuntime::dev_agent_window` acquires its enablement handle
    /// here; announce/event delivery itself still has no production caller
    /// (future platform-embedder wiring).
    #[must_use]
    pub(crate) fn semantics_host(&self) -> &SemanticsHost {
        &self.semantics
    }

    // ========================================================================
    // Window access, haptics
    // ========================================================================

    /// The identity of the window this presentation was built for.
    pub(crate) fn window_id(&self) -> flui_platform::WindowId {
        self.window_id
    }

    /// Access the presentation's window, if it is still live.
    ///
    /// `window` is `Weak`: the platform owns the strong `Arc`, and this
    /// presentation must not keep it alive past the platform's own teardown.
    /// Returns `None` once the window has been dropped: the window this
    /// presentation was built with is gone, since a presentation always has
    /// one from construction.
    pub(crate) fn with_window<R>(&self, f: impl FnOnce(&dyn PlatformWindow) -> R) -> Option<R> {
        self.window.upgrade().map(|window| f(window.as_ref()))
    }

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
    /// for a frame that presents or finds no damage, and when the window is
    /// shown again after it was hidden: until then the
    /// streak stays above an exhausted budget, so each further withheld
    /// attempt waits out the dropped-frame backoff instead of opening a new
    /// burst (ADR-0335 §2).
    pub(crate) fn clear_not_shown_streak(&self) {
        self.not_shown_streak.set(0);
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
    ///
    /// Read host scheduling state without applying invalidations or calling
    /// observers. Queued stale work and a checked-out pipeline conservatively
    /// contribute until the next owner preparation or frame operation.
    #[must_use]
    pub(crate) fn has_pending_work_snapshot(&self) -> bool {
        self.widgets.has_pending_builds()
            || self.has_pending_layout_builder_work()
            || self
                .renderer
                .root_pipeline_owner()
                .try_with(|owner| {
                    owner.has_pending_invalidations()
                        || owner.has_dirty_nodes() && !owner.layout_waits_for_input()
                        || owner.has_pending_announcements()
                })
                .unwrap_or(true)
    }

    /// Apply queued render invalidations before deciding whether to prepare a
    /// frame. This may mutate render state and invoke invalidation observers or
    /// wake callbacks; host scheduling reads use `has_pending_work_snapshot`.
    #[must_use]
    pub(crate) fn prepare_pending_work(
        &self,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) -> bool {
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
                // `try_recv` does not block, but applying a request can call
                // user code. The caller supplies the preparation recovery
                // scope; `run_frame` drains again before its first phase.
                owner.drain_pending_dirty_with_recovery(recovery);
                owner.has_dirty_nodes() && !owner.layout_waits_for_input()
                    || owner.has_pending_announcements()
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

    /// Turn the performance overlay on or off. It reads the clock's recent
    /// frame history, the same window its latency percentiles come from.
    pub(crate) fn set_performance_overlay(&self, enabled: bool) {
        self.performance_overlay.set(enabled);
    }

    /// This frame's debug readout, to draw above the presentation's content,
    /// `None` while the overlay is off.
    ///
    /// The scene records it at a node of its own, slotted after the
    /// pipeline's frame node, so it composites above the content and no
    /// boundary of the pipeline repaints for it; a readout equal to the one
    /// shown records nothing (`PresentationScene`).
    ///
    /// # The overlay is inside what it measures
    ///
    /// When enabled, this pulls `frames_since(None)` and rebuilds both
    /// histograms on every composited frame, and the scene shapes the
    /// readout's labels whenever they change. The cost is bounded — the
    /// telemetry ring is fixed-capacity, so it is O(ring), not O(session) —
    /// and no frame pays it while the overlay is off. But it is not free, and
    /// it lands *inside* the frames the overlay subsequently reports: read the
    /// displayed percentiles as the cost of running with the overlay on, not
    /// as the app's cost without it.
    pub(crate) fn performance_overlay_readout(
        &self,
    ) -> Option<crate::performance_overlay::Readout> {
        if !self.performance_overlay.get() {
            return None;
        }

        let snapshots = self.clock.frames_since(None);
        let pace = frame_pace(&snapshots);
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
        Some(crate::performance_overlay::Readout::of(
            &crate::performance_overlay::PerformanceSample {
                fps: pace.frames_per_second,
                frame_time_ms: pace.frame_work.as_secs_f64() * 1000.0,
                diagnostic_line: Some(&diagnostic_line),
            },
        ))
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

    /// Renews the scene's lineage before a reload: the code that painted
    /// every retained content may have changed, so the next frame repaints
    /// from the root into a new lineage and is published in full
    /// (ADR-0304 §10).
    #[cfg(feature = "hot-reload")]
    fn renew_scene(&self) {
        let released = self.scene.renew(flui_layer::ResyncReason::Rebuilt);
        // Dropped with the scene free: an annotation's `Drop` is user code.
        drop(released);
    }

    /// Apply a hot-reload tier to this presentation's own element tree.
    /// Returns whether a redraw is required.
    #[cfg(feature = "hot-reload")]
    pub(crate) fn apply_hot_reload(&self, tier: crate::reload::ReloadTier) -> bool {
        use crate::reload::ReloadTier;

        match tier {
            ReloadTier::Reassemble => {
                self.renew_scene();
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
                self.renew_scene();
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

    pub(crate) fn interaction_dispatch(&self) -> Option<&InteractionDispatchHandle> {
        self.interaction_dispatch.as_ref()
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

    use super::*;
    use flui_rendering::pipeline::PipelineOwner;

    static_assertions::assert_not_impl_any!(PresentationState: Send, Sync);

    fn presentation() -> PresentationState {
        PresentationState::new_for_test(
            PresentationId::new_gen(0, flui_foundation::generation::Generation::FIRST),
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
}
