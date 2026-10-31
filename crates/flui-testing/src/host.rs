//! A headless host for a [`UiRuntime`]: the product frame transaction
//! (`UiRuntime::pump`, ADR-0083 §1) on a manual clock, a window double and a
//! sink that keeps what the frame composited.
//!
//! [`HeadlessHost`] is to a UI runtime what a runner is on screen: it owns the
//! window the UI runtime presents into, the sink a frame is submitted through and
//! the clock the frame reads, and it drives frames and input through the
//! UI runtime's own entry points. Nothing here re-implements a phase of the frame:
//! applying commands, begin frame, the pipeline, end frame and the text-store
//! commit anchor all run inside `UiRuntime::pump`.
//!
//! One [`ManualClock`] drives the whole ui_runtime: the UI runtime reads it as its
//! [`ClockSource`] (frame-time origin, gesture-arena deadlines, the
//! presentation's `FrameClock`), and [`HeadlessHost::pump`] hands a clone to
//! the pump as the frame's timestamp. A test advances time only through
//! [`HeadlessHost::pump`] or [`HeadlessHost::clock`].
//!
//! # Failures
//!
//! A UI runtime contains a panic that escapes a frame segment, and a pipeline
//! error, as a dropped frame (ADR-0048) and reports it to its frame-failure
//! handler; on screen the process survives. Under test that containment would
//! hide the failure, so [`HeadlessHost::pump`] raises the first dropped-frame
//! report of the pump as a panic once the pump has returned, carrying the
//! report's text (the UI runtime retains it verbatim here). A later panic that
//! unwinds out of the same pump does not replace it: the first failure stays
//! authoritative. A dropped-frame report the UI runtime makes between pumps is
//! raised by the next pump, before it frames. A lifecycle panic the tree recovered from (an `ErrorView`
//! substitution) is not raised: the frame it happened in completed.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Size};
use flui_foundation::{ManualClock, PresentationId};
use flui_platform::data_transfer::{RepresentationDescriptor, TransferActions};
use flui_platform::drag::{DragError, OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform::input_regions::{DeviceInputRegion, InputRegion};
use flui_platform::{
    CursorError, CursorIcon, DisplayId, PlatformInput, PlatformTextInput, PlatformWindow,
    Unsupported, UnsupportedReason, WindowId,
};
use flui_platform::{SessionEndAnswer, SessionEndPhase, TextStoreHost};
use flui_rendering::layer::{LayerTree, Scene};
use flui_runtime::dev_agent::{DevAgentAttachment, DevAgentHost};
use flui_runtime::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::pump::FrameOutcome;
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_scheduler::{ClockSource, PostFrameHandle};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, AssistiveFocusListener,
    PlatformAccessibility,
};
use flui_view::__runtime::{FlushHost, FlushWrite, OwnerNotify};
use flui_view::CloseReason;
use flui_view::dev_agent::DevAgentHook;
use flui_view::persist::{StorageCapability, StorageHandle};
use parking_lot::Mutex;

use crate::a11y::A11yTree;
use crate::text_store_host::RecordingTextStoreHost;

/// The window a [`HeadlessHost`] presents into: window 1 at scale factor 1 (a
/// window [opened](HeadlessHost::open_window) beside it takes the next
/// identity, and [`HeadlessHost::set_scale_factor`] moves it), focused and
/// visible, with the logical size the UI runtime was built at.
///
/// It records what the UI runtime asks of a window that a test asserts on: the
/// cursor a hovered region sets, and, when built with a text input, every
/// IME enable and cursor-area call.
pub struct HeadlessWindow {
    id: WindowId,
    size: Size<f64>,
    scale_factor: Mutex<f64>,
    cursor: Mutex<CursorIcon>,
    text_input: Option<Arc<RecordingTextInput>>,
    text_store_host: bool,
    gesture_preset: flui_interaction::GestureProfiles,
    display: Mutex<Option<DisplayId>>,
    /// What covers the window, as the test last reported it.
    insets: Mutex<flui_platform::WindowInsets>,
    /// The revision of the state [`PlatformWindow::state`] reports: advanced by
    /// every change this double makes to it.
    revision: Mutex<flui_platform::WindowRevision>,
    input_regions: Mutex<SentInputRegions>,
    haptics: Arc<flui_platform::testing::FakeHaptics>,
    os_drags: Arc<HeadlessOsDrags>,
    /// Whether the simulated platform reports when frames reached the screen
    /// ([`HeadlessWindow::with_presentation_reports`]).
    reports_presentation: bool,
}

impl std::fmt::Debug for HeadlessWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessWindow")
            .field("size", &self.size)
            .field("text_input", &self.text_input.is_some())
            .finish_non_exhaustive()
    }
}

/// What a [`HeadlessWindow`] handed its simulated OS of its input regions, played as a
/// backend plays it (ADR-0217 §5): a frame's logical regions are projected to device pixels
/// at the window's scale by the projection every backend uses
/// ([`DeviceInputRegion::project`]: rounded outward, a region that is empty, not finite or
/// beyond `i32` dropped), and the OS is handed the set only when the pixels, the scale or
/// the display changed. A window starts with nothing claimed, so an empty first set is no
/// change.
#[derive(Debug, Default)]
struct SentInputRegions {
    /// The scale (`f64::to_bits`) and display `sent` was projected for; `None` until the
    /// first projection.
    key: Option<(u64, Option<DisplayId>)>,
    sent: Vec<DeviceInputRegion>,
    updates: Vec<Vec<DeviceInputRegion>>,
}

impl SentInputRegions {
    fn update(&mut self, regions: &[InputRegion], scale: f64, display: Option<DisplayId>) {
        // A scale that is not a positive finite number projects nothing; the OS keeps
        // what it holds.
        let Some(ratio) = DevicePixelRatio::try_new(scale) else {
            return;
        };
        let projected: Vec<DeviceInputRegion> = regions
            .iter()
            .filter_map(|region| DeviceInputRegion::project(*region, ratio).ok())
            .collect();
        let key = Some((scale.to_bits(), display));
        let unchanged = if self.key.is_none() {
            projected.is_empty()
        } else {
            self.key == key && self.sent == projected
        };
        self.key = key;
        if !unchanged {
            self.updates.push(projected.clone());
            self.sent = projected;
        }
    }
}

impl HeadlessWindow {
    /// A window of `width` × `height` logical (and physical) pixels.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            id: WindowId(1),
            size: Size::new(f64::from(width), f64::from(height)),
            scale_factor: Mutex::new(1.0),
            cursor: Mutex::new(CursorIcon::Default),
            text_input: None,
            text_store_host: false,
            gesture_preset: flui_interaction::GestureSettings::default().into(),
            display: Mutex::new(None),
            insets: Mutex::new(flui_platform::WindowInsets::ZERO),
            revision: Mutex::new(flui_platform::WindowRevision::ZERO),
            input_regions: Mutex::default(),
            haptics: Arc::new(flui_platform::testing::FakeHaptics::new()),
            os_drags: Arc::new(HeadlessOsDrags::default()),
            reports_presentation: false,
        }
    }

    /// The operating system's drag sessions this window plays: what a drag
    /// that escalated out of the window asked for, and how the OS ends it.
    #[must_use]
    pub fn os_drags(&self) -> &HeadlessOsDrags {
        &self.os_drags
    }

    /// The recording actuator this window plays haptics on: every request the
    /// UI runtime handed the platform, with the outcome it returned.
    #[must_use]
    pub fn haptic_actuator(&self) -> &flui_platform::testing::FakeHaptics {
        &self.haptics
    }

    /// Recognize gestures with `preset`: one `GestureSettings` value for every
    /// pointer kind, or `GestureProfiles::preset(platform)` for each kind's
    /// own on the platform the test models. Without one, the platform-neutral
    /// touch preset applies to every kind: a headless test never takes its
    /// values from the machine that runs it (ADR-0212 §5).
    #[must_use]
    pub fn with_gesture_preset(
        mut self,
        preset: impl Into<flui_interaction::GestureProfiles>,
    ) -> Self {
        self.gesture_preset = preset.into();
        self
    }

    /// Offer a pull-model text input instead: the UI runtime's presentation gets
    /// a [`RecordingTextStoreHost`] ([`HeadlessHost::text_store_host`]) and
    /// tells it which field's store takes input, as it tells the Win32 text
    /// services. It wins over [`Self::with_text_input`].
    #[must_use]
    pub fn with_text_store_host(mut self) -> Self {
        self.text_store_host = true;
        self
    }

    /// Play a platform that reports when frames reached the screen: the
    /// window's capabilities say so, and [`HeadlessHost::report_presented`]
    /// reaches its frame requests. Without it the window reports none, as its
    /// capabilities say.
    #[must_use]
    pub fn with_presentation_reports(mut self) -> Self {
        self.reports_presentation = true;
        self
    }

    /// Offer a text-input capability that records every call the UI runtime's
    /// text-input owner makes, so the presentation installs a working
    /// `TextInputHandle` over it.
    #[must_use]
    pub fn with_text_input(mut self) -> Self {
        self.text_input = Some(Arc::new(RecordingTextInput::default()));
        self
    }

    /// The last cursor the UI runtime set on this window.
    #[must_use]
    pub fn cursor(&self) -> CursorIcon {
        *self.cursor.lock()
    }

    /// Every IME cursor area the UI runtime reported, in delivery order; `None`
    /// for a window built without a text input.
    #[must_use]
    pub fn ime_cursor_areas(&self) -> Option<Vec<Bounds<f64>>> {
        self.text_input
            .as_ref()
            .map(|input| input.cursor_areas.lock().clone())
    }

    /// Every IME enable (`true`) and disable (`false`) the UI runtime asked for,
    /// in delivery order; `None` for a window built without a text input.
    #[must_use]
    pub fn ime_allowed_calls(&self) -> Option<Vec<bool>> {
        self.text_input
            .as_ref()
            .map(|input| input.ime_allowed.lock().clone())
    }

    /// Every input-region set this window handed the OS, in device pixels and delivery
    /// order: one entry per change the backend's projection
    /// saw, none for a frame that re-sent an unchanged set (ADR-0217 §5).
    #[must_use]
    pub fn input_region_updates(&self) -> Vec<Vec<DeviceInputRegion>> {
        self.input_regions.lock().updates.clone()
    }

    /// The logical size in device pixels at the current scale factor: a
    /// window moved to another monitor keeps its logical size.
    fn physical_size_i32(&self) -> Size<i32> {
        let scale = *self.scale_factor.lock();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "built from u32 pixel counts at a test's small scale factor, far below \
                      i32::MAX"
        )]
        Size::new(
            (self.size.width * scale).round() as i32,
            (self.size.height * scale).round() as i32,
        )
    }
}

impl PlatformWindow for HeadlessWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn request_redraw(&self) {}

    /// Nothing to ask: the host runs a frame when its test pumps one.
    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(|| {})
    }

    /// No native window: [`flui_platform::testing::NoNativeSurface`].
    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(flui_platform::testing::NoNativeSurface)
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input
            .as_ref()
            .map(|input| Arc::clone(input) as Arc<dyn PlatformTextInput>)
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        *self.cursor.lock() = cursor;
        Ok(())
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.haptics) as Arc<dyn flui_platform::PlatformHaptics>)
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }

    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        Some(Arc::clone(&self.os_drags) as Arc<dyn PlatformDragSource>)
    }

    fn set_input_regions(&self, regions: &[InputRegion]) -> Result<(), flui_platform::Unsupported> {
        let scale = *self.scale_factor.lock();
        let display = *self.display.lock();
        self.input_regions.lock().update(regions, scale, display);
        Ok(())
    }

    /// The host's window takes no refresh-rate request: its capabilities say
    /// `Unavailable`.
    fn set_frame_rate_hint(
        &self,
        _: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        Err(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::NotOnThisPlatform,
        ))
    }

    /// Shown and focused, with no position (as on Wayland), its logical size
    /// at its scale, the display it was
    /// [moved to](HeadlessHost::move_to_display), and a revision each move
    /// advances. The host's window is always shown: there is no window
    /// manager to raise it, and it honours no command.
    fn state(&self) -> flui_platform::WindowState {
        let size = self.size;
        let mut state =
            flui_platform::WindowState::new(flui_platform::WindowGeometry::new(None, size), size)
                .with_focused(true)
                .with_display(*self.display.lock())
                .with_insets(*self.insets.lock())
                .with_metrics(
                    flui_foundation::geometry::WindowMetrics::from_logical(
                        size,
                        *self.scale_factor.lock(),
                    )
                    .ok(),
                );
        if self.reports_presentation {
            let mut capabilities = flui_platform::WindowCapabilities::NONE;
            capabilities.reports_presentation = flui_platform::Availability::Native;
            state = state.with_capabilities(capabilities);
        }
        state.revision = *self.revision.lock();
        state
    }

    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        command.against(&flui_platform::WindowCapabilities::NONE)?;
        Ok(())
    }
}

/// The operating system's drag sessions as a [`HeadlessWindow`] plays them: a
/// modal session that ends inside its start, as Win32's `DoDragDrop` does,
/// the way a test chose with [`end_with`](Self::end_with). Until a test
/// chooses, the OS refuses every session and the drag stays in its window.
#[derive(Debug, Default)]
pub struct HeadlessOsDrags {
    end: Mutex<Option<OsDragEnd>>,
    started: Mutex<Vec<StartedOsDrag>>,
}

/// What one escalated drag asked the OS for.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StartedOsDrag {
    /// What the session offers, ending with its identity.
    pub representations: Vec<RepresentationDescriptor>,
    /// The actions it advertises.
    pub allowed: TransferActions,
}

impl HeadlessOsDrags {
    /// End every later session with `end`, as a user dropping it on another
    /// application (or cancelling) would.
    pub fn end_with(&self, end: OsDragEnd) {
        *self.end.lock() = Some(end);
    }

    /// Every session started, in order.
    #[must_use]
    pub fn started(&self) -> Vec<StartedOsDrag> {
        self.started.lock().clone()
    }
}

impl PlatformDragSource for HeadlessOsDrags {
    fn start_timing(&self) -> OsDragStart {
        OsDragStart::OnLeavingWindow
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let end = (*self.end.lock()).ok_or(DragError::Unsupported(Unsupported::new(
            UnsupportedReason::NotRegistered,
        )))?;
        let OsDragRequest {
            representations,
            allowed,
            on_end,
            ..
        } = request;
        self.started.lock().push(StartedOsDrag {
            representations,
            allowed,
        });
        on_end(end);
        Ok(())
    }
}

/// The IME capability of a [`HeadlessWindow`] built with a text input.
#[derive(Debug, Default)]
struct RecordingTextInput {
    cursor_areas: Mutex<Vec<Bounds<f64>>>,
    ime_allowed: Mutex<Vec<bool>>,
}

impl PlatformTextInput for RecordingTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.ime_allowed.lock().push(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        self.cursor_areas.lock().push(area);
    }

    /// No input method composes in a headless window.
    fn discard_composition(&self) {}
}

/// The accessibility bridge of a [`HeadlessHost`]'s window: assistive
/// technology attaches when a test asks, published updates are folded into
/// the tree an adapter would hold, and the action listener the UI runtime
/// registers is handed to a test that plays the adapter.
#[derive(Default)]
struct HeadlessAccessibility {
    active: AtomicBool,
    activation: Mutex<Option<AccessibilityActivationListener>>,
    action: Mutex<Option<AccessibilityActionListener>>,
    assistive_focus: Mutex<Option<AssistiveFocusListener>>,
    published: Mutex<Option<accesskit::TreeUpdate>>,
}

impl HeadlessAccessibility {
    /// Attach assistive technology: the listener the UI runtime registered flips
    /// the presentation's semantics flag, which the next frame reconciles
    /// onto the pipeline.
    fn attach(&self) {
        self.active.store(true, Ordering::Relaxed);
        let listener = self.activation.lock().clone();
        if let Some(listener) = listener {
            listener(true);
        }
    }
}

impl PlatformAccessibility for HeadlessAccessibility {
    /// Fold `update` into the published tree as an adapter does: an update
    /// carrying tree data replaces it, any other replaces the nodes it names,
    /// adds the rest, and drops those no longer reachable from the root.
    fn publish(&self, update: accesskit::TreeUpdate) {
        let mut published = self.published.lock();
        match published.as_mut() {
            Some(tree) if update.tree.is_none() => {
                for (id, node) in update.nodes {
                    match tree.nodes.iter_mut().find(|(known, _)| *known == id) {
                        Some(slot) => slot.1 = node,
                        None => tree.nodes.push((id, node)),
                    }
                }
                tree.focus = update.focus;
                // An adapter drops the nodes no longer reachable from the
                // root, so the kept tree tracks the live one instead of
                // accumulating every node ever published.
                if let Some(root) = tree.tree.as_ref().map(|data| data.root) {
                    let mut reachable = std::collections::HashSet::from([root]);
                    let mut pending = vec![root];
                    while let Some(id) = pending.pop() {
                        if let Some((_, node)) = tree.nodes.iter().find(|(known, _)| *known == id) {
                            for &child in node.children() {
                                if reachable.insert(child) {
                                    pending.push(child);
                                }
                            }
                        }
                    }
                    tree.nodes.retain(|(id, _)| reachable.contains(id));
                }
            }
            _ => *published = Some(update),
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        *self.activation.lock() = Some(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        *self.action.lock() = Some(listener);
    }

    /// A test plays the screen reader, so the cursor is reported: through
    /// [`HeadlessHost::move_assistive_focus`].
    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        flui_semantics::AssistiveFocusSupport::Reported
    }

    fn set_assistive_focus_listener(&self, listener: AssistiveFocusListener) {
        *self.assistive_focus.lock() = Some(listener);
    }
}

/// The frame sink of a [`HeadlessHost`]: a surface of fixed size that
/// presents every scene and keeps the last one, unless told to defer some.
#[derive(Debug)]
pub struct HeadlessSink {
    size: (u32, u32),
    last_scene: Option<Scene>,
    submits: u64,
    deferred: u32,
}

impl HeadlessSink {
    /// A `width` × `height` surface that has presented nothing yet.
    #[must_use]
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            size: (width, height),
            last_scene: None,
            submits: 0,
            deferred: 0,
        }
    }

    /// The layer tree of the last scene submitted, retained across frames
    /// that painted nothing.
    #[must_use]
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.last_scene.as_ref().map(Scene::tree)
    }

    /// How many scenes have been submitted, not counting deferred ones.
    #[must_use]
    pub fn submits(&self) -> u64 {
        self.submits
    }
}

impl FrameSink for HeadlessSink {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn submit(&mut self, scene: Scene) -> SubmitVerdict {
        if let Some(deferred) = self.deferred.checked_sub(1) {
            self.deferred = deferred;
            return SubmitVerdict::Retry;
        }
        self.last_scene = Some(scene);
        self.submits = self.submits.saturating_add(1);
        SubmitVerdict::Presented
    }
}

/// A development-agent hook attached the way a runner's event loop attaches
/// it: once, when this value is built, and detached when it is dropped.
///
/// Every [`HeadlessHost`] built [`with`](HeadlessHost::with_dev_agent) it
/// hands the hook its window, as a runner hands it each window it opens, so
/// one hook can serve several UI runtimes, and a UI runtime dropped before the hook is
/// a window that closed while the tool kept running. Containment is the
/// runtime's (`flui_runtime::dev_agent`): a hook that panics is dropped, and
/// the UI runtimes keep pumping.
pub struct HeadlessDevAgent {
    host: DevAgentHost,
    attachment: Option<DevAgentAttachment>,
}

impl HeadlessDevAgent {
    /// Take `hook` and attach it, as a runner does when its loop starts.
    #[must_use]
    pub fn attach(hook: impl DevAgentHook) -> Self {
        let host = DevAgentHost::new(hook);
        let attachment = host.attach();
        Self { host, attachment }
    }

    /// Whether the hook is attached: it did not panic in `attach` or since.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.attachment.is_some() && self.host.is_attached()
    }
}

impl std::fmt::Debug for HeadlessDevAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDevAgent")
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}
/// How long [`HeadlessHost::end_session`]'s `Ending` phase writes what the
/// documents published (ADR-0260).
pub use flui_view::__runtime::SESSION_END_BUDGET;

/// The capabilities a headless host provides when it is given no registry:
/// the clipboard, the primary selection and HTTP, each scripted by the test,
/// so no test reaches the network.
#[derive(Default)]
struct ScriptedBuiltIns {
    clipboard: crate::ScriptedClipboard,
    primary_selection: crate::ScriptedPrimarySelection,
    background: HeadlessBackground,
    http: crate::ScriptedHttp,
}

impl ScriptedBuiltIns {
    fn registrar(&self) -> flui_platform::capability::CapabilityRegistrar {
        let mut registrar = flui_platform::capability::CapabilityRegistrar::new();
        self.clipboard.register(&mut registrar);
        self.primary_selection.register(&mut registrar);
        self.background.register(&mut registrar);
        self.http.register(&mut registrar);
        registrar
    }
}

/// The host's background lanes: deterministic, on the test's thread, run
/// by [`HeadlessHost::pump`] before each frame unless held.
#[derive(Clone)]
struct HeadlessBackground {
    executors: flui_runtime::execution::DeterministicExecutors,
    /// The lanes as a widget reaches them, with the admission windows and
    /// shutdown of a hosted run.
    services: Arc<flui_runtime::execution::ExecutionServices>,
    held: Rc<std::cell::Cell<bool>>,
}

impl Default for HeadlessBackground {
    fn default() -> Self {
        let executors = flui_runtime::execution::DeterministicExecutors::new();
        let services = Arc::new(flui_runtime::execution::ExecutionServices::with_host(
            executors.host_executors(),
        ));
        Self {
            executors,
            services,
            held: Rc::default(),
        }
    }
}

impl HeadlessBackground {
    fn register(&self, registrar: &mut flui_platform::capability::CapabilityRegistrar) {
        let services = Arc::clone(&self.services);
        registrar.provide::<flui_view::background::BackgroundCapability>(
            move |cx: &flui_platform::capability::ProviderContext<'_>| {
                Ok(flui_view::background::Background::new(
                    Arc::clone(&services) as Arc<dyn flui_platform::executor::Executor>,
                    cx.completions().clone(),
                ))
            },
        );
    }

    /// Queued background jobs and futures not yet finished.
    fn pending(&self) -> usize {
        let (jobs, futures) = self.executors.pending();
        jobs + futures
    }

    /// Run queued work until none is runnable, unless held.
    fn drain_unless_held(&self) {
        if !self.held.get() {
            let _ran = self.executors.run_until_idle();
        }
    }
}

/// A [`UiRuntime`] hosted headlessly, driven frame by frame on a manual clock.
/// See the [module docs](self).
#[doc(alias = "HeadlessRuntime")]
pub struct HeadlessHost {
    ui_runtime: UiRuntime,
    clock: ManualClock,
    sink: HeadlessSink,
    window: Rc<HeadlessWindow>,
    accessibility: Arc<HeadlessAccessibility>,
    scripted: ScriptedBuiltIns,
    /// The windows opened beside the primary one, in opening order.
    secondary: Vec<SecondaryWindow>,
    /// The surfaces embedded in the primary window, in minting order.
    embedded: Vec<flui_interaction::InputSurface>,
    text_store_host: Option<Rc<RecordingTextStoreHost>>,
    /// Dropped-frame reports not yet raised, as the text a raised failure
    /// carries: those of the pump in progress, and any the UI runtime made
    /// between pumps, which the next pump raises before it frames.
    failures: Arc<Mutex<Vec<String>>>,
    /// The presentation report the next pump's frame requests carry
    /// ([`Self::report_presented`]).
    presented: Option<flui_platform::FramePresented>,
    /// The flush registry's host side, when the host was given storage: what
    /// the widgets' documents publish, written as a runner writes it.
    flush: Option<FlushHost>,
    /// The registry's writes waiting for the next pump to run them, as a
    /// runner's IO pool runs them.
    flush_writes: Arc<Mutex<Vec<FlushWrite>>>,
    /// Whether the primary window was closed.
    primary_closed: bool,
}

impl std::fmt::Debug for HeadlessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessHost")
            .field("ui_runtime", &self.ui_runtime)
            .field("window", &self.window)
            .field("sink", &self.sink)
            .finish_non_exhaustive()
    }
}

/// The storage a headless host gives its UI runtime: the host's own, and
/// the storage the application provides as the [`StorageCapability`] in its
/// place, if any. The host's flush writes each document's bytes to the
/// storage that document acquired.
#[derive(Clone)]
pub(crate) struct KitStorage {
    pub(crate) host: Arc<dyn flui_platform::Storage>,
    pub(crate) provided: Option<Arc<dyn flui_platform::Storage>>,
}

impl HeadlessHost {
    /// A UI runtime over `window`, whose surface has the window's size.
    ///
    /// # Panics
    ///
    /// If the UI runtime's interaction lane cannot be created (its identity space
    /// is exhausted).
    #[must_use]
    pub fn new(window: HeadlessWindow) -> Self {
        Self::with_storage(window, None)
    }

    /// [`Self::new`], its widgets acquiring platform capabilities from
    /// `registry` alone (ADR-0152): the host's [`clipboard`](Self::clipboard)
    /// reaches them only if `registry` provides it.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[must_use]
    pub fn with_capabilities(
        window: HeadlessWindow,
        registry: flui_runtime::capability::CapabilityRegistry,
    ) -> Self {
        Self::assemble(window, None, Some(registry), None)
    }

    /// [`Self::new`], its widgets acquiring what `plugin` registers beside
    /// the host's [`clipboard`](Self::clipboard) and
    /// [`primary_selection`](Self::primary_selection): how a package tests its
    /// plugin on a headless host (ADR-0152).
    ///
    /// # Panics
    ///
    /// When `plugin` registers a capability twice, or under the name of
    /// another; and as [`Self::new`].
    #[must_use]
    pub fn with_plugin(
        window: HeadlessWindow,
        plugin: &dyn flui_platform::capability::Plugin,
    ) -> Self {
        let scripted = ScriptedBuiltIns::default();
        let registry = flui_runtime::capability::CapabilityRegistry::builder()
            .built_ins(scripted.registrar())
            .plugin(plugin)
            .build()
            .unwrap_or_else(|conflict| panic!("the plugin's registrations conflict: {conflict}"));
        Self::assemble(window, None, Some(registry), Some(scripted))
    }

    /// [`Self::new`], its widgets acquiring `storage` as
    /// the [`StorageCapability`].
    pub(crate) fn with_storage(window: HeadlessWindow, storage: Option<KitStorage>) -> Self {
        Self::assemble(window, storage, None, None)
    }

    fn assemble(
        window: HeadlessWindow,
        storage: Option<KitStorage>,
        capabilities: Option<flui_runtime::capability::CapabilityRegistry>,
        scripted: Option<ScriptedBuiltIns>,
    ) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the window size came from u32 pixel counts"
        )]
        let surface = (window.size.width as u32, window.size.height as u32);
        let text_store_host = window.text_store_host.then(RecordingTextStoreHost::new);
        let gesture_preset = window.gesture_preset.clone();
        let window = Rc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let scripted = scripted.unwrap_or_default();
        let clock = ManualClock::new();
        // A collection of its own: the ui_runtime owns a `TextContext` over it
        // (ADR-0092 §3), exactly as a hosted ui_runtime does. Deliberately
        // bundled-only, unlike the app's host-fed one
        // (`FontCollection::with_host_fonts`), so text measures the same on
        // every host a test runs on.
        let fonts = flui_painting::FontCollection::new();
        // The wake sets the redraw flag, as a runner's frame wake does, so the
        // runtime's demand for a next frame is observable here.
        let needs_redraw = Arc::new(AtomicBool::new(false));
        let wake_flag = Arc::clone(&needs_redraw);
        let flush_writes = Arc::new(Mutex::new(Vec::new()));
        let flush = storage.as_ref().map(|_| {
            let queued = Arc::clone(&flush_writes);
            FlushHost::new(
                Box::new(move |write| {
                    queued.lock().push(write);
                    Ok(())
                }),
                // Every pump settles the registry, so a signal has nothing to wake.
                OwnerNotify::new(|_| Ok(())),
            )
        });
        let mut host = RuntimeHostServices::new(
            Arc::new(move || wake_flag.store(true, Ordering::Relaxed)),
            needs_redraw,
            &fonts,
            ClockSource::Manual(clock.clone()),
        );
        let registry = capabilities.unwrap_or_else(|| {
            let mut built_ins = scripted.registrar();
            let mut overrides = flui_platform::capability::CapabilityRegistrar::new();
            if let Some(KitStorage { host, provided }) = &storage {
                let host = Arc::clone(host);
                built_ins.provide::<StorageCapability>(
                    move |_: &flui_platform::capability::ProviderContext<'_>| {
                        Ok(StorageHandle::new(Arc::clone(&host)))
                    },
                );
                if let Some(provided) = provided {
                    let provided = Arc::clone(provided);
                    overrides.provide::<StorageCapability>(
                        move |_: &flui_platform::capability::ProviderContext<'_>| {
                            Ok(StorageHandle::new(Arc::clone(&provided)))
                        },
                    );
                }
            }
            flui_runtime::capability::CapabilityRegistry::builder()
                .overrides(overrides)
                .built_ins(built_ins)
                .build()
                .expect("BUG: one built-in and one override per capability cannot conflict")
        });
        host = host.with_capabilities(registry);
        if let Some(flush) = &flush {
            host = host.with_flush_registry(flush.registry());
        }
        host = host.with_close_vetoes(&[
            CloseReason::User,
            CloseReason::Program,
            CloseReason::SessionEnd,
        ]);
        let ui_runtime = UiRuntime::try_new(
            PresentationWindow::new(
                Rc::clone(&window) as Rc<dyn PlatformWindow>,
                Some(Arc::clone(&accessibility) as Arc<dyn PlatformAccessibility>),
            )
            .with_text_store_host(
                text_store_host
                    .clone()
                    .map(|host| host as Rc<dyn TextStoreHost>),
            )
            .with_gesture_profiles(gesture_preset),
            1.0,
            host,
        )
        .expect("BUG: interaction lane identity exhausted");
        install_window_control(&ui_runtime, ui_runtime.presentation_id(), &window);
        let failures = Arc::new(Mutex::new(Vec::new()));
        ui_runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
        let recorded = Arc::clone(&failures);
        ui_runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            if let Some(text) = dropped_frame_text(report) {
                recorded.lock().push(text);
            }
        })));
        Self {
            ui_runtime,
            clock,
            sink: HeadlessSink::new(surface.0, surface.1),
            window,
            accessibility,
            scripted,
            text_store_host,
            secondary: Vec::new(),
            embedded: Vec::new(),
            failures,
            presented: None,
            flush,
            flush_writes,
            primary_closed: false,
        }
    }

    /// Report that a frame reached the screen, as a host's platform does
    /// (DXGI statistics, `wp_presentation_time`): the next
    /// [`pump`](Self::pump)'s frame requests carry it
    /// ([`FrameRequest::presented`](flui_platform::FrameRequest::presented)),
    /// and each presentation's frame clock keeps it. Its time is on the
    /// event timeline input and frames are stamped on: the time since the
    /// [`clock`](Self::clock) started. A later report before that pump
    /// replaces it. A primary window whose capabilities say it reports no
    /// presentation (made without
    /// [`HeadlessWindow::with_presentation_reports`]) ignores it, as a host
    /// whose platform reports none does.
    pub fn report_presented(&mut self, presented: flui_platform::FramePresented) {
        if self.window.reports_presentation {
            self.presented = Some(presented);
        }
    }

    /// Hand this UI runtime's window to `agent`'s hook, as a runner hands it a
    /// window it has installed. The window's first semantics tree is built
    /// by the next [`pump`](Self::pump); dropping the UI runtime closes the window,
    /// and the hook's handle to it answers `gone` from then on.
    ///
    /// Does nothing when the hook is not attached.
    #[must_use]
    pub fn with_dev_agent(self, agent: &HeadlessDevAgent) -> Self {
        agent
            .host
            .publish(&self.ui_runtime, self.ui_runtime.presentation_id());
        self
    }

    /// Attach `view` as the UI runtime's root widget, the root view sized to the
    /// window. The UI runtime wraps it in its root scopes (`GestureArenaScope`,
    /// `VsyncScope`, `FocusRoot`, `MediaQuery`); the first
    /// [`pump`](Self::pump) builds, lays out and paints it.
    ///
    /// # Errors
    ///
    /// [`flui_view::AttachError`] if a root is already attached.
    pub fn attach<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.ui_runtime.attach_root_widget_with_size(
            view,
            self.window.size.width,
            self.window.size.height,
        )
    }

    /// Advance the clock by `dt`, then run one frame through
    /// `UiRuntime::pump` at the new instant.
    ///
    /// The clock moves before the frame starts, once: a caller that catches
    /// a raised failure and pumps again with `Duration::ZERO` retries at the
    /// same instant.
    ///
    /// # Panics
    ///
    /// With a dropped-frame report the UI runtime made since the last pump, before
    /// this one frames or moves the clock; with the first dropped-frame
    /// report of this pump (a segment panic or a pipeline error the UI runtime
    /// contained), after the pump returned; or with a panic that unwound out
    /// of the pump when nothing was reported before it. See the
    /// [module docs](self#failures).
    pub fn pump(&mut self, dt: Duration) -> FrameOutcome {
        let pending = std::mem::take(&mut *self.failures.lock());
        if let Some(failure) = pending.into_iter().next() {
            panic!("{failure}");
        }
        self.clock.advance(dt);
        self.settle_lifecycle();
        // Background work runs before the frame, so an answer it posts
        // settles in this pump.
        self.scripted.background.drain_unless_held();
        // The frame is delivered as `HostInput::Frame` would be: every
        // window's vsync is the clock's reading, on the event timeline, so a
        // recording sees the frame where its deadlines fire.
        let vsync = self.event_now();
        let mut request = flui_platform::FrameRequest::new(vsync);
        if let Some(presented) = self.presented.take() {
            request = request.with_presented(presented);
        }
        for presentation in self.presentation_ids() {
            self.ui_runtime.note_frame_request(presentation, request);
        }
        let mut frame_clock = self.clock.clone();
        let Self {
            ui_runtime, sink, ..
        } = self;
        let attempt = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump(&mut frame_clock, sink)));
        let first_failure = {
            let mut failures = self.failures.lock();
            let first = failures.first().cloned();
            failures.clear();
            first
        };
        match (attempt, first_failure) {
            (Ok(outcome), None) => outcome,
            (Ok(_), Some(failure)) => panic!("{failure}"),
            (Err(payload), None) => resume_unwind(payload),
            (Err(payload), Some(failure)) => {
                tracing::error!(
                    first_failure = %failure,
                    "a panic unwound out of the pump after a contained frame failure; the \
                     contained failure is raised and the later panic is discarded"
                );
                // The payload is an arbitrary user value whose destructor may
                // itself panic; dropping it here could replace or abort the
                // unwind of the first failure, so it is leaked instead.
                std::mem::forget(payload);
                panic!("{failure}")
            }
        }
    }

    /// Defer the next `frames` submits, as a surface not yet ready to show
    /// its window defers them (`SubmitVerdict::Retry`): those frames render,
    /// their tree stays uncommitted, and pointer input waits for the first
    /// presented frame to be dispatched (ADR-0048).
    pub fn defer_presents(&mut self, frames: u32) {
        self.sink.deferred = frames;
    }

    /// Deliver `input` to the UI runtime's primary presentation, as a runner
    /// delivers a platform event, then flush the pointer moves it queued.
    ///
    /// The UI runtime coalesces pointer moves and dispatches them at the next
    /// frame; the flush makes a synthetic move observable before that frame,
    /// so a test sees its effect immediately. It runs the same queue and
    /// dispatch code the frame would. An opted-in resampling presentation
    /// retains measured moves until its owner frame is pumped.
    ///
    /// # Panics
    ///
    /// With the first panic a dispatch raised; a later one is logged and
    /// discarded.
    pub fn dispatch(&self, input: PlatformInput) {
        self.dispatch_to(self.ui_runtime.presentation_id(), input);
    }

    /// [`dispatch`](Self::dispatch) to `presentation`'s window.
    fn dispatch_to(&self, presentation: PresentationId, input: PlatformInput) {
        self.dispatch_on_to(presentation, flui_interaction::InputSurface::Window, input);
    }

    /// [`dispatch`](Self::dispatch) as `surface` of `presentation`'s window.
    fn dispatch_on_to(
        &self,
        presentation: PresentationId,
        surface: flui_interaction::InputSurface,
        input: PlatformInput,
    ) {
        self.ui_runtime.enter(|ui_runtime| {
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                ui_runtime.handle_input_on(presentation, surface, input);
            }));
            let flushed = catch_unwind(AssertUnwindSafe(|| {
                if !ui_runtime.gestures().is_resampling_enabled() {
                    ui_runtime.gestures().flush_pending_moves();
                }
                ui_runtime.gestures().drain_deferred_arena_resolutions();
            }));
            match (delivered, flushed) {
                (Err(first), Err(later)) => {
                    tracing::error!(
                        "flushing queued pointer moves panicked after the input dispatch \
                         panicked; only the first panic is resumed"
                    );
                    std::mem::forget(later);
                    resume_unwind(first);
                }
                (Err(payload), Ok(())) | (Ok(()), Err(payload)) => resume_unwind(payload),
                (Ok(()), Ok(())) => {}
            }
        });
        // The dispatch is over: the OS drag sessions it decided on start here,
        // as a host's outermost callback starts them (ADR-0211 §7).
        self.ui_runtime.enter_pending_os_drags();
    }

    /// Set exactly this window's presentation policy through the runtime owner.
    ///
    /// # Errors
    /// The runtime refuses a mode change while the presentation has a contact.
    pub fn set_pointer_resampling(
        &self,
        window: HeadlessWindowId,
        policy: crate::PointerResampling,
    ) -> Result<(), crate::PointerResamplingError> {
        self.ui_runtime.set_pointer_resampling(window.0, policy)
    }

    /// The pointer left the window: the UI runtime sweeps hover state, so every
    /// hovered region gets its exit and the cursor resets.
    pub fn dispatch_hover_left(&self) {
        self.observe(
            self.ui_runtime.presentation_id(),
            flui_runtime::owner::WindowObservation::Hover(false),
        );
    }

    /// Attach assistive technology to the window, as a platform adapter does:
    /// the next [`pump`](Self::pump) turns semantics on and assembles the
    /// tree.
    pub fn enable_semantics(&self) {
        self.accessibility.attach();
    }

    /// Whether the window's pipeline holds a semantics tree: assistive
    /// technology or a development agent asked for one, and a
    /// [`pump`](Self::pump) has run since. It stops once nothing asks and
    /// the next pump has run.
    #[must_use]
    pub fn collects_semantics(&self) -> bool {
        self.ui_runtime
            .presentation_collects_semantics_for_test(self.ui_runtime.presentation_id())
            .expect("BUG: a headless ui_runtime's window stays resident while the ui_runtime lives")
    }

    /// The listener the UI runtime registered for actions assistive technology
    /// requests. It is `Send + Sync`: a test plays the platform adapter by
    /// calling it, from any thread, and the UI runtime queues the request in its
    /// owner inbox for the next pump to apply.
    #[must_use]
    pub fn accessibility_action_listener(&self) -> Option<AccessibilityActionListener> {
        self.accessibility.action.lock().clone()
    }

    /// Move the screen reader's cursor to `node` of the published tree, or
    /// off it for `None`, as VoiceOver or TalkBack reports it: the UI runtime
    /// queues the move in its owner inbox for the next [`pump`](Self::pump),
    /// which tells the subtrees the cursor left and entered.
    ///
    /// # Panics
    ///
    /// Before the UI runtime registered its listener, which it does as the
    /// window opens.
    pub fn move_assistive_focus(&self, node: Option<accesskit::NodeId>) {
        let listener = self
            .accessibility
            .assistive_focus
            .lock()
            .clone()
            .expect("BUG: the UI runtime registers its cursor listener as the window opens");
        listener(node);
    }

    /// Run `f` inside the UI runtime's owner scope (its interaction lane, global
    /// key registry and post-frame lane), as every UI runtime entry point does.
    ///
    /// Crate-private, as is [`ui_runtime`](Self::ui_runtime): the UI runtime's frame entry
    /// points are the host's (ADR-0083 §2), and `flui-runtime` is not an
    /// embedder API, so a test reaches the UI runtime only through this host.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&UiRuntime) -> R) -> R {
        self.ui_runtime.enter(f)
    }

    /// The hosted UI runtime.
    pub(crate) fn ui_runtime(&self) -> &UiRuntime {
        &self.ui_runtime
    }

    /// The owner-local post-frame handle the UI runtime installed on its build
    /// owner: a callback scheduled through it runs in the post-frame phase
    /// after this presentation has coherent geometry. An incomplete frame
    /// retains it until repair; an ordinary host-frame callback uses the
    /// scheduler's unscoped lane instead.
    ///
    /// # Panics
    ///
    /// If the UI runtime installed no owner-local post-frame handle, which a
    /// UI runtime's presentation always does.
    #[must_use]
    pub fn post_frame_handle(&self) -> PostFrameHandle {
        self.ui_runtime
            .widgets()
            .with_build_owner(|owner| owner.post_frame_handle().cloned())
            .expect("BUG: a ui_runtime's presentation installs its owner-local post-frame handle")
    }

    /// Ask the UI runtime for a frame, as a widget's `setState` would: the next
    /// [`pump`](Self::pump) runs the pipeline even with nothing dirty.
    pub fn request_frame(&self) {
        self.ui_runtime.request_redraw();
    }

    /// Apply application motion policy to this host's UI runtime and all its
    /// presentations. Existing animations observe it at the next frame;
    /// presentations opened later inherit it before their first root build.
    ///
    /// # Panics
    ///
    /// Propagates the first inherited-data or frame-request callback failure,
    /// after the runtime has committed the preference to its presentations.
    pub fn set_motion_override(&self, preference: flui_animation::MotionOverride) {
        self.enter(|runtime| runtime.set_motion_override(preference));
    }

    /// The clock the UI runtime reads. Advancing it moves the frame time, the
    /// gesture-arena deadlines and the produce gate together.
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.clock
    }

    /// The sink frames are submitted through.
    #[must_use]
    pub fn sink(&self) -> &HeadlessSink {
        &self.sink
    }

    /// The window the UI runtime presents into.
    #[must_use]
    pub fn window(&self) -> &HeadlessWindow {
        &self.window
    }

    /// The pull-model host the UI runtime's presentation speaks to, for a window
    /// built [`HeadlessWindow::with_text_store_host`].
    #[must_use]
    pub fn text_store_host(&self) -> Option<&Rc<RecordingTextStoreHost>> {
        self.text_store_host.as_ref()
    }

    /// The clipboard the UI runtime hands its widgets.
    #[must_use]
    pub fn clipboard(&self) -> crate::ScriptedClipboard {
        self.scripted.clipboard.clone()
    }

    /// The HTTP client the UI runtime hands its widgets: every request
    /// answers what the test scripted for its URL, and no request reaches the
    /// network.
    #[must_use]
    pub fn http(&self) -> crate::ScriptedHttp {
        self.scripted.http.clone()
    }

    /// The primary selection the UI runtime hands its widgets.
    #[must_use]
    pub fn primary_selection(&self) -> crate::ScriptedPrimarySelection {
        self.scripted.primary_selection.clone()
    }

    /// The recording text store backend of a particular presentation.
    #[must_use]
    pub fn text_store_host_on(
        &self,
        window: HeadlessWindowId,
    ) -> Option<&Rc<RecordingTextStoreHost>> {
        if window == self.primary_window() {
            self.text_store_host.as_ref()
        } else {
            self.secondary_of(window).text_store_host.as_ref()
        }
    }

    /// The window the UI runtime was built on.
    #[must_use]
    pub fn primary_window(&self) -> HeadlessWindowId {
        HeadlessWindowId(self.ui_runtime.presentation_id())
    }

    /// Ask to close `window` for `reason`, as its platform does when the user
    /// clicks its close button (or as the program or the session asks). The
    /// window's close guard is asked first: a close it refuses is recorded,
    /// nothing is torn down, and the pump after the last hold is released
    /// carries it out. `true` when the window closed now; `false` when the
    /// guard refused or the window is already gone.
    pub fn request_close(&mut self, window: HeadlessWindowId, reason: CloseReason) -> bool {
        if self.ui_runtime.close_refused(window.0, reason) {
            return false;
        }
        self.ui_runtime.commit_composition_for_close(window.0);
        self.close_presentation(window.0)
    }

    /// One phase of the user's session ending, as the operating system reports
    /// it (ADR-0260). `Query` asks every window's guard for the session's
    /// end and answers [`Block`](SessionEndAnswer::Block) when one refuses;
    /// `Cancelled` withdraws only those session-end records; `Ending` writes
    /// what the documents published, within [`SESSION_END_BUDGET`].
    pub fn end_session(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let windows = self.presentation_ids();
        match phase {
            SessionEndPhase::Query => {
                let mut answer = SessionEndAnswer::Proceed;
                for id in windows {
                    if self.ui_runtime.close_refused(id, CloseReason::SessionEnd) {
                        answer = SessionEndAnswer::Block;
                    }
                }
                answer
            }
            SessionEndPhase::Cancelled => {
                for id in windows {
                    self.ui_runtime.withdraw_close(id, CloseReason::SessionEnd);
                }
                SessionEndAnswer::Proceed
            }
            SessionEndPhase::Ending => {
                self.run_flush_writes();
                if let Some(flush) = &self.flush {
                    let _report =
                        flush.flush_within(std::time::Instant::now() + SESSION_END_BUDGET);
                }
                SessionEndAnswer::Proceed
            }
            // A phase added later asks this host nothing.
            _ => SessionEndAnswer::Proceed,
        }
    }

    /// Open `window` beside the existing ones, as a runner installs a
    /// presentation for a window the app opened: the UI runtime builds it a
    /// pipeline of its own at the window's scale factor. It shows nothing
    /// until a view is [attached](Self::attach_to) to it.
    ///
    /// The window gets the next free window identity, so no two windows of
    /// this host share one.
    pub fn open_window(&mut self, mut window: HeadlessWindow) -> HeadlessWindowId {
        let next = u64::try_from(self.secondary.len())
            .expect("BUG: a test opens far fewer than u64::MAX windows")
            + 2;
        window.id = WindowId(next);
        let text_store_host = window.text_store_host.then(RecordingTextStoreHost::new);
        let gesture_preset = window.gesture_preset.clone();
        let window = Rc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let presentation = self.ui_runtime.assemble_presentation(
            PresentationWindow::new(
                Rc::clone(&window) as Rc<dyn PlatformWindow>,
                Some(Arc::clone(&accessibility) as Arc<dyn PlatformAccessibility>),
            )
            .with_text_store_host(
                text_store_host
                    .clone()
                    .map(|host| host as Rc<dyn TextStoreHost>),
            )
            .with_gesture_profiles(gesture_preset),
        );
        let id = self.ui_runtime.install_presentation(presentation);
        install_window_control(&self.ui_runtime, id, &window);
        self.secondary.push(SecondaryWindow {
            text_store_host,
            id,
            window,
            accessibility,
        });
        HeadlessWindowId(id)
    }

    /// Attach `view` as the root widget of `window`. The primary window's
    /// root is attached with [`attach`](Self::attach).
    ///
    /// # Errors
    ///
    /// [`flui_view::AttachError`] if a root is already attached.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn attach_to<V>(
        &self,
        window: HeadlessWindowId,
        view: &V,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        if window == self.primary_window() {
            return self.attach(view);
        }
        // The same environment the primary root gets: this window's own
        // `MediaQuery` and its size.
        let size = self.secondary_of(window).window.size;
        self.ui_runtime
            .attach_root_widget_with_size_to(window.0, view, size.width, size.height)
    }

    /// Attach assistive technology to `window`; see
    /// [`enable_semantics`](Self::enable_semantics).
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn enable_semantics_on(&self, window: HeadlessWindowId) {
        self.accessibility_of(window).attach();
    }

    /// Move `window` to a monitor of scale `scale_factor`, as a runner
    /// delivers the platform's scale change: the window keeps its logical
    /// size and reports the new scale, the primary window's surface takes
    /// the new device size, and the window's own presentation (never a
    /// sibling's) lays out, paints and publishes semantics at it from the
    /// next [`pump`](Self::pump).
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn set_scale_factor(&mut self, window: HeadlessWindowId, scale_factor: f64) {
        // The runner's path: a metrics observation of the window, which a
        // recording sees as the `Metrics` row it is.
        let size = self.window_of(window).size;
        // A scale that makes no metrics is refused, as the runner refuses it.
        let Ok(metrics) =
            flui_foundation::geometry::WindowMetrics::from_logical(size, scale_factor)
        else {
            return;
        };
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Metrics(metrics),
        );
        self.size_sink_to_primary();
    }

    /// Move `window` to the display `display` (`None`: the backend can name none) at its
    /// current scale, as a runner delivers the platform's move: the window publishes a
    /// state on the new display, and the UI runtime reads it when the runner reports
    /// that the state changed. A move to the display the window is on publishes
    /// nothing.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn move_to_display(&mut self, window: HeadlessWindowId, display: Option<DisplayId>) {
        let native = self.window_of(window);
        if std::mem::replace(&mut *native.display.lock(), display) == display {
            return;
        }
        let mut revision = native.revision.lock();
        *revision = revision
            .next()
            .expect("BUG: a test moves a window far fewer than u64::MAX times");
        drop(revision);
        self.observe(window.0, flui_runtime::owner::WindowObservation::State);
    }

    /// Deliver `display` as the display `window` is on, as a runner delivers
    /// the one it derived from the platform's display snapshot: its refresh
    /// period is what the UI runtime paces `window`'s presentation by.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn set_window_display(
        &self,
        window: HeadlessWindowId,
        display: Option<flui_platform::WindowDisplay>,
    ) {
        let _ = self.window_of(window);
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Display(
                flui_runtime::owner::DisplayUpdate::new(display),
            ),
        );
    }

    /// Report that `window` gained OS focus, as a backend's focus callback
    /// does: keyboard input goes to its presentation from now on, whatever
    /// window it is stamped for, and the window that held focus loses it.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn focus_window(&self, window: HeadlessWindowId) {
        let _ = self.window_of(window);
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Focus(true),
        );
    }

    /// The accessibility tree `window` last published to its platform
    /// adapter, every incremental update folded in; `None` before
    /// assistive technology attached and a [`pump`](Self::pump) ran.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    #[must_use]
    pub fn published_a11y_tree(&self, window: HeadlessWindowId) -> Option<A11yTree> {
        self.accessibility_of(window)
            .published
            .lock()
            .clone()
            .map(A11yTree::new)
    }

    /// Apply `values` as the next system-preference revision, as a host's
    /// preference source delivers one (ADR-0172): gesture timing, motion and
    /// text values reach the windows' new sequences from now on.
    pub fn set_system_preferences(&self, values: flui_platform::SystemPreferences) {
        self.ui_runtime
            .enter(|ui_runtime| ui_runtime.apply_system_preferences_for_test(values));
    }

    /// Report the primary window shown (`true`) or hidden, minimized or fully
    /// occluded (`false`), as a backend's visibility callback does.
    pub fn set_window_visible(&self, visible: bool) {
        let id = self.ui_runtime.presentation_id();
        self.ui_runtime.enter(|ui_runtime| {
            ui_runtime.synchronize_window_snapshot(
                id,
                flui_platform::WindowExecutionState::Running,
                true,
                visible,
            );
        });
    }

    /// Report that the primary window gained (`true`) or lost OS focus, as a
    /// backend's focus callback does: the host delivers
    /// `WindowObservation::Focus(focused)`. A window that loses focus forgets
    /// its pressed keys and cancels its pointer sequences; the focus inside
    /// its tree stays where it was. [`focus_window`](Self::focus_window)
    /// moves the OS focus between windows this host opened.
    pub fn set_window_focused(&self, focused: bool) {
        self.observe(
            self.ui_runtime.presentation_id(),
            flui_runtime::owner::WindowObservation::Focus(focused),
        );
    }

    /// Report `insets` as what covers `window`, as a host reports a change of
    /// its safe area, keyboard or system gesture regions: the presentation's
    /// `MediaQuery` carries them from the next [`pump`](Self::pump), and a
    /// moving keyboard is sampled at each frame's time until it settles.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn set_window_insets(&self, window: HeadlessWindowId, insets: flui_platform::WindowInsets) {
        *self.window_of(window).insets.lock() = insets;
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Insets(insets),
        );
    }

    /// Start moving `window`'s keyboard to `target` now, over `duration` on
    /// `curve`, as a host reports a keyboard that begins to show or hide: the
    /// motion starts where the keyboard is at this instant, and the safe area
    /// and system gesture regions stay as they were. Returns the keyboard as
    /// reported, whose [`at`](flui_platform::KeyboardInset::at) names the
    /// occlusion each later frame lays out with.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host, or the motion is invalid: a
    /// negative or non-finite `target`, a zero `duration`, or an invalid
    /// `curve`.
    pub fn move_keyboard(
        &self,
        window: HeadlessWindowId,
        target: flui_foundation::geometry::EdgeInsets,
        duration: Duration,
        curve: flui_platform::InsetCurve,
    ) -> flui_platform::KeyboardInset {
        let now = self.event_now();
        let insets = *self.window_of(window).insets.lock();
        let motion =
            flui_platform::InsetMotion::try_new(insets.keyboard().at(now), now, duration, curve)
                .expect("a keyboard motion with a duration and a valid curve");
        let keyboard = flui_platform::KeyboardInset::try_moving(target, motion)
            .expect("a keyboard target that is finite and not negative");
        self.set_window_insets(window, insets.with_keyboard(keyboard));
        keyboard
    }

    /// The primary window's gesture arena, the one its detectors compete in:
    /// observe it to watch the arena decide what this host dispatches.
    #[must_use]
    pub fn arena(&self) -> &flui_interaction::GestureArena {
        self.ui_runtime.gestures().arena()
    }

    /// Install a tap from `factory` on this host's runtime for the rest of
    /// its life, as `flui-app` installs an application's on every runtime it
    /// builds: the factory is handed what the recording starts from.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`](flui_foundation::IdExhausted) when the runtime issued
    /// every tap registration generation.
    pub fn install_input_tap(
        &self,
        factory: &flui_runtime::InputTapFactory,
    ) -> Result<(), flui_foundation::IdExhausted> {
        factory.install_on(&self.ui_runtime)
    }

    /// The UI runtime's tap registry: [`record`](crate::recording::record)
    /// into it to record everything this host applies (ADR-0215 §1).
    #[must_use]
    pub fn input_taps(&self) -> &flui_runtime::InputTaps {
        self.ui_runtime.input_taps()
    }

    /// The header of a recording of this host: its virtual clock, every
    /// window with its scale factor and the per-kind gesture values its
    /// contacts are admitted with, the system-preference revision in effect,
    /// and the text kept under `privacy`. A headless host has no hardware, so
    /// it names no device.
    #[must_use]
    pub fn recording_header(
        &self,
        privacy: crate::recording::Privacy,
    ) -> crate::recording::RecordingHeader {
        let mut header = crate::recording::RecordingHeader::starting(
            crate::recording::RecordingClock::Virtual,
            &self.ui_runtime.recording_start(),
        );
        header.privacy = privacy;
        header
    }

    /// Embed a surface in the primary window, as an embedder does through
    /// its presentation's gesture binding: input [`dispatch_on`](Self::dispatch_on)
    /// that surface keeps its own contacts, hover and latches (ADR-0207 §5).
    ///
    /// # Errors
    ///
    /// [`SurfaceIdsExhausted`](flui_interaction::SurfaceIdsExhausted) once the
    /// binding minted every identity.
    pub fn embed_surface(
        &mut self,
    ) -> Result<flui_interaction::InputSurface, flui_interaction::SurfaceIdsExhausted> {
        let surface = self.ui_runtime.gestures().embed_surface()?;
        self.embedded.push(surface);
        Ok(surface)
    }

    /// [`dispatch`](Self::dispatch) `input` as `surface` of the primary window
    /// reported it.
    ///
    /// # Panics
    ///
    /// As [`dispatch`](Self::dispatch).
    pub fn dispatch_on(&self, surface: flui_interaction::InputSurface, input: PlatformInput) {
        self.dispatch_on_to(self.ui_runtime.presentation_id(), surface, input);
    }

    /// Replay `recording` on this host, row by row on its virtual clock, and
    /// return the arena decisions the replay made, to compare with
    /// [`ReplayableRecording::decisions`](crate::recording::ReplayableRecording::decisions).
    ///
    /// Before the first row the header's starting state is restored: each
    /// recorded window moves to its recorded scale (as a runner delivers a
    /// scale change) and its per-kind gesture values replace this host's, and
    /// the recorded system-preference values apply. Each input is then
    /// applied at its recorded arrival: the clock advances to it first, and
    /// never back, since pointer input the recorded host held for its first
    /// commit is recorded where it was dispatched. Input the recorded host
    /// accepted and never delivered (`InputOutcome::Queued`) is not applied. The
    /// windows the recording names must be this host's (the same
    /// [`WindowId`]s), and an embedded surface it names one this host
    /// [embedded](Self::embed_surface) in the same order; preference
    /// revisions apply at their positions.
    ///
    /// # Panics
    ///
    /// If the recording names a window this host has not opened or a surface
    /// it has not embedded, or with the first panic an applied input raised.
    pub fn replay(
        &mut self,
        recording: &crate::recording::ReplayableRecording,
    ) -> Vec<flui_runtime::DecisionRow> {
        use flui_runtime::{HostInput, InputRecord};
        let decisions = Rc::new(std::cell::RefCell::new(Vec::new()));
        let handle = {
            let decisions = Rc::clone(&decisions);
            self.input_taps()
                .install(move |record: &InputRecord| {
                    if let InputRecord::Decision(decision) = record {
                        decisions.borrow_mut().push(decision.clone());
                    }
                })
                .expect("BUG: a headless host issues a replay its tap registration")
        };
        let header = recording.header();
        for window in &header.windows {
            let presentation = self.presentation_of(window.window);
            let id = HeadlessWindowId(presentation);
            if *self.window_of(id).scale_factor.lock() != window.scale_factor {
                self.set_scale_factor(id, window.scale_factor);
            }
            if let Some(profiles) = &window.gesture_profiles {
                self.ui_runtime
                    .replace_gesture_profiles(presentation, profiles.clone());
            }
        }
        if let Some(preferences) = &header.preferences {
            self.set_system_preferences((*preferences.values).clone());
        }
        let mut last_frame = None;
        for row in recording.rows() {
            match row {
                // Held input its host discarded: nothing dispatched it.
                InputRecord::Input(row)
                    if row.outcome == flui_runtime::owner::InputOutcome::Queued =>
                {
                    continue;
                }
                InputRecord::Input(row) => {
                    let presentation = self.presentation_of(row.window);
                    self.advance_to(row.arrival);
                    match &row.input {
                        HostInput::Frame { .. } => {
                            // One pump delivers a frame to every window.
                            if last_frame != Some(row.arrival) {
                                let _ = self.pump(Duration::ZERO);
                            }
                            last_frame = Some(row.arrival);
                            continue;
                        }
                        HostInput::Platform(input) => {
                            let surface = self.surface_of(presentation, row.surface);
                            self.dispatch_on_to(presentation, surface, input.clone());
                        }
                        HostInput::Window(observation) => {
                            self.observe(presentation, *observation);
                            self.size_sink_to_primary();
                        }
                        HostInput::Accessibility(request) => {
                            self.ui_runtime.enter(|ui_runtime| {
                                ui_runtime.dispatch_accessibility(presentation, request);
                            });
                        }
                        HostInput::AssistiveFocus(node) => {
                            self.ui_runtime.enter(|ui_runtime| {
                                ui_runtime.dispatch_assistive_focus(presentation, *node);
                            });
                        }
                        other => panic!("this host cannot replay {other:?}"),
                    }
                }
                InputRecord::Preferences(row) => {
                    self.set_system_preferences((*row.values).clone());
                }
                _ => {}
            }
            last_frame = None;
        }
        handle
            .finish()
            .expect("BUG: the replay's own tap records every row");
        decisions.take()
    }

    /// Deliver `script` to the primary window on the virtual clock: each step
    /// at its offset from now, a frame step as a frame. Several events
    /// between two frame steps reach the runtime within one frame, and an
    /// event between two vsyncs keeps its own time.
    ///
    /// # Panics
    ///
    /// With the first panic a delivered input or frame raised.
    pub fn run_script(&mut self, script: &crate::replay::PointerScript) {
        let window = self.ui_runtime.presentation_id();
        let base = self.event_now();
        for (step, input) in script.steps().iter().zip(script.inputs_from(base)) {
            self.advance_to(base + step.at());
            match input {
                flui_runtime::HostInput::Platform(input) => self.dispatch_to(window, input),
                _ => {
                    let _ = self.pump(Duration::ZERO);
                }
            }
        }
    }

    /// Where a script finds the control it drives: the center, in logical
    /// pixels, of `window`'s single semantics node with `role` and `label`,
    /// as assistive technology sees it.
    ///
    /// # Errors
    ///
    /// [`A11yQueryError`](crate::A11yQueryError) when no node, or more than
    /// one, matches.
    ///
    /// # Panics
    ///
    /// If `window` has published no semantics tree (attach assistive
    /// technology and pump first), or the node has no bounds.
    pub fn find_center(
        &self,
        window: HeadlessWindowId,
        role: accesskit::Role,
        label: &str,
    ) -> Result<flui_foundation::geometry::Offset<f64>, crate::A11yQueryError> {
        let tree = self
            .published_a11y_tree(window)
            .expect("the window published no semantics tree; enable semantics and pump first");
        let node = tree.find_by_role_and_label(role, label)?;
        let bounds = tree
            .window_bounds(&node)
            .expect("a laid-out semantics node carries bounds");
        let scale = *self.window_of(window).scale_factor.lock();
        Ok(flui_foundation::geometry::Offset::new(
            bounds.x0.midpoint(bounds.x1) / scale,
            bounds.y0.midpoint(bounds.y1) / scale,
        ))
    }

    /// How long a host's loop waits before its next frame: none while a frame
    /// is asked for or work is pending, unless a dropped frame waits for its
    /// retry; otherwise until the earliest deadline the UI runtime armed, such
    /// as a gesture's or a dropped frame's retry. `None` when the loop would
    /// wait for input alone.
    #[must_use]
    pub fn time_to_next_frame(&self) -> Option<Duration> {
        let runtime = &self.ui_runtime;
        let now = flui_foundation::MonotonicClock::now(&self.clock);
        // A dropped frame waiting for its retry runs nothing sooner, though a
        // gesture deadline before it still wakes the loop.
        let retrying = runtime
            .failure_retry_deadline()
            .is_some_and(|retry| retry > now);
        if !retrying
            && (runtime.needs_redraw()
                || runtime.scheduler().is_frame_scheduled()
                || runtime.has_pending_work()
                || self.background_runs_next_pump())
        {
            return Some(Duration::ZERO);
        }
        let wake = runtime.next_wake()?;
        Some(wake.saturating_duration_since(now))
    }

    /// Stop [`pump`](Self::pump) running background work, so a test sees a
    /// widget while its work is still pending; the work waits until
    /// [`release_background`](Self::release_background) or
    /// [`run_background`](Self::run_background).
    pub fn hold_background(&mut self) {
        self.scripted.background.held.set(true);
    }

    /// Let [`pump`](Self::pump) run background work again, from its next
    /// call.
    pub fn release_background(&mut self) {
        self.scripted.background.held.set(false);
    }

    /// Run queued background work now, held or not, until none is runnable.
    /// Its answers settle at the next pump. Returns the jobs run plus the
    /// polls made.
    pub fn run_background(&mut self) -> usize {
        self.scripted.background.executors.run_until_idle()
    }

    /// Background jobs and futures queued and not yet finished. Counted by
    /// [`pump_until_settled`](Self::pump_until_settled): held work keeps a
    /// host unsettled.
    #[must_use]
    pub fn pending_background(&self) -> usize {
        self.scripted.background.pending()
    }

    /// Whether the next pump runs background work: a parked future waits
    /// for its wake, not for a frame.
    fn background_runs_next_pump(&self) -> bool {
        !self.scripted.background.held.get() && self.scripted.background.executors.runnable() > 0
    }

    /// Wait as a host's loop does, [`time_to_next_frame`](Self::time_to_next_frame),
    /// then pump. `None`, pumping nothing, when the loop would wait for input
    /// alone.
    ///
    /// # Panics
    ///
    /// As [`pump`](Self::pump) does.
    pub fn pump_next_frame(&mut self) -> Option<FrameOutcome> {
        let wait = self.time_to_next_frame()?;
        Some(self.pump(wait))
    }

    /// Pump frames of `frame` until nothing is left to do — no redraw or
    /// frame asked for (a running animation asks for each next one), no
    /// pending work, no armed deadline — at most `max_frames` of
    /// them. Returns how many frames it took.
    ///
    /// # Errors
    ///
    /// [`NotSettled`] when work remains after `max_frames` frames, such as a
    /// repeating animation.
    pub fn pump_until_settled(
        &mut self,
        frame: Duration,
        max_frames: usize,
    ) -> Result<usize, NotSettled> {
        for frames in 0..=max_frames {
            let runtime = &self.ui_runtime;
            if !runtime.needs_redraw()
                && !runtime.scheduler().is_frame_scheduled()
                && !runtime.has_pending_work()
                && self.pending_background() == 0
                && runtime.next_wake().is_none()
            {
                return Ok(frames);
            }
            if frames < max_frames {
                let _ = self.pump(frame);
            }
        }
        Err(NotSettled { frames: max_frames })
    }

    /// Apply `observation` of `presentation`'s window as a runner delivers
    /// one; a metrics change resizes the window's surface.
    fn observe(
        &self,
        presentation: PresentationId,
        observation: flui_runtime::owner::WindowObservation,
    ) {
        if let flui_runtime::owner::WindowObservation::Metrics(metrics) = observation {
            *self
                .window_of(HeadlessWindowId(presentation))
                .scale_factor
                .lock() = metrics.scale().get();
        }
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime.id(),
            presentation_id: presentation,
        };
        self.ui_runtime.enter(|ui_runtime| {
            ui_runtime.apply_window_observation(address, observation, &|_| {});
        });
    }

    /// Size the sink to the primary window's device size.
    fn size_sink_to_primary(&mut self) {
        let physical = self.window.physical_size_i32();
        self.sink.size = (
            u32::try_from(physical.width).expect("BUG: a window's device size is positive"),
            u32::try_from(physical.height).expect("BUG: a window's device size is positive"),
        );
    }

    /// The presentation of the window `window` names.
    fn presentation_of(&self, window: WindowId) -> PresentationId {
        if self.window.id == window {
            return self.ui_runtime.presentation_id();
        }
        self.secondary
            .iter()
            .find(|secondary| secondary.window.id == window)
            .map_or_else(
                || panic!("the recording names {window:?}, a window this host has not opened"),
                |secondary| secondary.id,
            )
    }

    /// The surface of `presentation`'s window a recording names.
    fn surface_of(
        &self,
        presentation: PresentationId,
        surface: flui_runtime::RecordedSurface,
    ) -> flui_interaction::InputSurface {
        match surface {
            flui_runtime::RecordedSurface::Window => flui_interaction::InputSurface::Window,
            flui_runtime::RecordedSurface::Embedded(raw) => self
                .embedded
                .iter()
                .copied()
                .find(|embedded| {
                    presentation == self.ui_runtime.presentation_id()
                        && matches!(embedded, flui_interaction::InputSurface::Embedded(id) if id.get() == raw)
                })
                .unwrap_or_else(|| {
                    panic!("the recording names embedded surface {raw}, which this host has not embedded")
                }),
        }
    }

    /// Advance the clock to `time` on the event timeline, if it is ahead.
    fn advance_to(&self, time: flui_platform::EventTime) {
        let now = self.event_now();
        self.clock.advance(time.saturating_duration_since(now));
    }

    /// The clock's reading on the event timeline.
    fn event_now(&self) -> flui_platform::EventTime {
        flui_platform::EventTime::from_nanos(
            u64::try_from(self.clock.elapsed().as_nanos())
                .expect("BUG: a headless test's virtual time fits the event timeline"),
        )
    }

    /// Every window's presentation, the primary first.
    /// The owner turn's settle, before its frame: carry out the closes the
    /// windows' guards now let through, run the registry's queued writes and
    /// wake what they resolved.
    fn settle_lifecycle(&mut self) {
        for (id, _reason) in self.ui_runtime.settle_close_guards().due {
            self.close_presentation(id);
        }
        self.run_flush_writes();
        if let Some(flush) = &self.flush {
            for waker in flush.settle() {
                waker.wake();
            }
        }
    }

    /// Poll every queued registry write once, keeping those still waiting
    /// for their storage. The queue's lock is not held while a write runs.
    fn run_flush_writes(&self) {
        let writes = std::mem::take(&mut *self.flush_writes.lock());
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        let waiting: Vec<FlushWrite> = writes
            .into_iter()
            .filter_map(|mut write| write.as_mut().poll(&mut cx).is_pending().then_some(write))
            .collect();
        self.flush_writes.lock().extend(waiting);
    }

    /// Close presentation `id`: a secondary window's presentation is closed
    /// and removed, as a runner closes a window; the primary one stops and
    /// its tree stays mounted for the test to inspect, as a runner's last
    /// window stays until the loop's teardown. `false` when it is already
    /// closed.
    fn close_presentation(&mut self, id: PresentationId) -> bool {
        if id == self.ui_runtime.presentation_id() {
            if std::mem::replace(&mut self.primary_closed, true) {
                return false;
            }
            self.ui_runtime
                .enter(|ui_runtime| ui_runtime.stop_presentation_for_test(id));
            return true;
        }
        self.secondary.retain(|secondary| secondary.id != id);
        self.ui_runtime.close_presentation_entered(id)
    }

    fn presentation_ids(&self) -> Vec<PresentationId> {
        std::iter::once(self.ui_runtime.presentation_id())
            .chain(self.secondary.iter().map(|secondary| secondary.id))
            .collect()
    }

    fn window_of(&self, window: HeadlessWindowId) -> &HeadlessWindow {
        if window == self.primary_window() {
            return &self.window;
        }
        &self.secondary_of(window).window
    }

    fn accessibility_of(&self, window: HeadlessWindowId) -> &HeadlessAccessibility {
        if window == self.primary_window() {
            return &self.accessibility;
        }
        &self.secondary_of(window).accessibility
    }

    fn secondary_of(&self, window: HeadlessWindowId) -> &SecondaryWindow {
        self.secondary
            .iter()
            .find(|secondary| secondary.id == window.0)
            .expect("a HeadlessWindowId names a window this host opened")
    }
}

/// The UI runtime still had work after a bounded run of frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the UI runtime had not settled after {frames} frames")]
pub struct NotSettled {
    /// The frames pumped.
    pub frames: usize,
}

/// A window of a [`HeadlessHost`]: its primary one or one it
/// [opened](HeadlessHost::open_window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeadlessWindowId(PresentationId);

/// The host's control of a [`HeadlessWindow`], behind the `WindowHandle` its
/// widgets reach through `LifecycleContext::window`, as a runner installs one
/// for each window it opens. The double honours no window command.
struct HeadlessWindowControl(Rc<HeadlessWindow>);

impl flui_platform::WindowControl for HeadlessWindowControl {
    fn state(&self) -> flui_platform::WindowState {
        self.0.state()
    }

    fn submit(
        &self,
        command: flui_platform::WindowCommand,
    ) -> Result<(), flui_platform::SubmitError> {
        command
            .admit(&flui_platform::WindowCapabilities::NONE)
            .map(drop)?;
        Ok(())
    }

    fn request_close(&self) {}
}

/// Attach the host's control of `window` to `presentation`, before its root
/// widget is attached.
fn install_window_control(
    ui_runtime: &UiRuntime,
    presentation: PresentationId,
    window: &Rc<HeadlessWindow>,
) {
    let source = flui_view::__runtime::WindowSource::new(HeadlessWindowControl(Rc::clone(window)));
    assert!(
        ui_runtime.install_window(presentation, source),
        "BUG: the window's presentation was just installed"
    );
}

/// A window [`HeadlessHost::open_window`] installed beside the primary one.
struct SecondaryWindow {
    text_store_host: Option<Rc<RecordingTextStoreHost>>,
    id: PresentationId,
    window: Rc<HeadlessWindow>,
    accessibility: Arc<HeadlessAccessibility>,
}

/// The text a dropped-frame report raises with; `None` for a contained
/// recovery, which does not drop the frame.
fn dropped_frame_text(report: &FrameFailureReport) -> Option<String> {
    if report.disposition != FailureDisposition::FrameDropped {
        return None;
    }
    Some(match &report.kind {
        FrameFailureKind::SamplingPanic {
            first,
            blocking,
            message,
            ..
        } => match blocking {
            Some(blocking) => format!(
                "frame sampling panicked in {:?} ({:?}); publication blocked in {:?} ({:?}): {message}",
                first.stage, first.origin, blocking.stage, blocking.origin,
            ),
            None => format!(
                "frame sampling panicked in {:?} ({:?}): {message}",
                first.stage, first.origin,
            ),
        },
        FrameFailureKind::SegmentPanic { message, phase, .. } => {
            format!("frame segment panicked in {phase:?}: {message}")
        }
        FrameFailureKind::Pipeline { error } => format!("frame pipeline failed: {error}"),
        other => format!("frame dropped: {other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::time::Duration;

    use super::{HeadlessHost, HeadlessWindow};

    /// A dropped-frame report the UI runtime made between pumps (from input
    /// delivery, say) is raised by the next pump, before it frames or moves
    /// the clock, and only once.
    ///
    /// Fails against a pump that clears the buffer before it runs: the
    /// report is erased unseen and the pump returns normally.
    #[test]
    fn a_report_made_between_pumps_is_raised_by_the_next_pump() {
        let mut ui_runtime = HeadlessHost::new(HeadlessWindow::new(40, 24));
        ui_runtime
            .failures
            .lock()
            .push("frame dropped: reported between pumps".to_owned());
        let before = flui_foundation::MonotonicClock::now(ui_runtime.clock());

        let raised = catch_unwind(AssertUnwindSafe(|| {
            ui_runtime.pump(Duration::from_millis(16))
        }))
        .expect_err("the leftover report is raised");

        assert_eq!(
            raised.downcast_ref::<String>().map(String::as_str),
            Some("frame dropped: reported between pumps")
        );
        assert_eq!(
            flui_foundation::MonotonicClock::now(ui_runtime.clock()),
            before,
            "the raising pump did not move the clock"
        );
        let _outcome = ui_runtime.pump(Duration::ZERO);
    }

    /// The kept tree holds what an adapter holds: a node an incremental
    /// update detaches from the root is dropped, not kept forever.
    ///
    /// Fails against a fold that only upserts: the detached node stays.
    #[test]
    fn a_detached_node_leaves_the_published_tree() {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

        use super::PlatformAccessibility as _;

        let parent = |children: Vec<NodeId>| {
            let mut node = Node::new(Role::Window);
            node.set_children(children);
            node
        };
        let bridge = super::HeadlessAccessibility::default();
        bridge.publish(TreeUpdate {
            nodes: vec![
                (NodeId(1), parent(vec![NodeId(2)])),
                (NodeId(2), Node::new(Role::Label)),
            ],
            tree: Some(TreeInfo::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        });
        bridge.publish(TreeUpdate {
            nodes: vec![
                (NodeId(1), parent(vec![NodeId(3)])),
                (NodeId(3), Node::new(Role::Label)),
            ],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        });

        let published = bridge.published.lock();
        let mut ids: Vec<_> = published
            .as_ref()
            .expect("a tree was published")
            .nodes
            .iter()
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, [NodeId(1), NodeId(3)]);
    }
}
