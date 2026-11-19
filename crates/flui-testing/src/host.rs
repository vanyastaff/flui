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
//! The same clock is the timeline of the host's [`Scenario`]: the scripted
//! clipboard, primary selection and any plugin built on
//! [`HeadlessHost::with_scenario`] answer on it, and each pump applies the
//! scenario's changes due at its instant before the frame's turn, so a
//! change scheduled at 3 s settles in the first pump at or after 3 s.
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

use std::num::NonZeroU32;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::portal_exports::HeadlessPortalExports;
use std::time::Duration;

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Size};
use flui_foundation::panic::FirstPanic;
use flui_foundation::{ManualClock, PresentationId};
use flui_platform::capability::scripted::{AlreadyDriven, Scenario, ScenarioDriver};
use flui_platform::data_transfer::{RepresentationDescriptor, TransferActions};
use flui_platform::drag::{
    DragError, OsDragEnd, OsDragParts, OsDragRequest, OsDragStart, PlatformDragSource,
};
use flui_platform::input_regions::{DeviceInputRegion, InputRegion};
use flui_platform::text_store::TextStoreHost;
use flui_platform::text_store::TextStoreStatus;
use flui_platform::{
    Cursor, CursorError, CursorIcon, DisplayId, PlatformInput, PlatformTextInput, PlatformWindow,
    Unsupported, UnsupportedReason, WindowId, WindowIdMint,
};

use flui_runtime::dev_agent::DevAgentHost;
use flui_runtime::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use flui_runtime::owner_turn::{
    CloseRequestHandler, CloseRequestRouter, CloseResponse, PreparedCloseRequest, QuitRequest,
    SessionEnd,
};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::pump::FrameOutcome;
use flui_runtime::window_system::WindowSystem;

use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_scheduler::{ClockSource, PostFrameHandle};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, AssistiveFocusListener,
    PlatformAccessibility,
};
use flui_view::__runtime::{FlushHost, FlushWrite, OwnerNotify};
use flui_view::CloseReason;
use flui_view::dev_agent::DevAgentHook;
use parking_lot::Mutex;

use crate::a11y::A11yTree;
use crate::text_store_host::RecordingTextStoreHost;

mod registrations;
mod sink;
mod turn;

pub(crate) use registrations::AppRegistrations;
use registrations::{Registry, compose_registry};
pub use sink::{HeadlessSink, UnshownFrame};

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
    size: flui_platform::WindowExtent,
    scale_factor: Mutex<DevicePixelRatio>,
    cursor: Mutex<Cursor>,
    text_input: Option<Arc<RecordingTextInput>>,
    text_store_host: bool,
    gesture_preset: flui_interaction::gesture::GestureProfiles,
    platform: flui_platform::TargetPlatform,
    /// Whether the window plays a browser's page ([`HeadlessWindow::in_browser`]).
    in_browser: bool,
    /// The application's assets its widgets read (ADR-0384 §5): none
    /// unless the test names them.
    assets: flui_platform::assets::AssetBundle,
    display: Mutex<Option<DisplayId>>,
    /// Whether the platform lets the window run, as the test last reported it.
    execution: Mutex<flui_platform::WindowExecutionState>,
    /// Whether the window system keeps the window off the screen, as the test
    /// last reported it ([`HeadlessHost::set_window_occluded`]).
    occluded: Mutex<bool>,
    /// What the simulated window system does with the window's last frame
    /// while it is off the screen ([`HeadlessWindow::with_occlusion_reports`]).
    occluded_surface: flui_platform::OccludedSurface,
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
    /// The commands the window honours, published once for what the window plays
    /// ([`HeadlessWindow::published_capabilities`]): what its commands are
    /// admitted against and opened under.
    capabilities: flui_platform::WindowCapabilities,
    /// The frame and mode the window was set to and the close requests it received,
    /// through its `WindowHandle` or a command applied to it.
    chrome: Mutex<KitChrome>,
    /// Whether the host closed the window: its commands answer `Closed`.
    closed: AtomicBool,
    /// How the window is named to a portal, when it plays an exported
    /// Wayland window ([`HeadlessWindow::with_portal_export`]).
    portal_exports: Option<HeadlessPortalExports>,
    /// Whether the window system fails to make a cursor of an image
    /// ([`HeadlessWindow::failing_cursor_images`]).
    fails_cursor_images: bool,
    /// The application's answer to the window's close requests, taken by the
    /// host that opens the window
    /// ([`HeadlessWindow::with_close_request_handler`]).
    close_request_handler: Option<CloseRequestHandler>,
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
    fn update(
        &mut self,
        regions: &[InputRegion],
        ratio: DevicePixelRatio,
        display: Option<DisplayId>,
    ) {
        let projected: Vec<DeviceInputRegion> = regions
            .iter()
            .filter_map(|region| DeviceInputRegion::project(*region, ratio).ok())
            .collect();
        let key = Some((ratio.get().to_bits(), display));
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

/// What of a [`HeadlessWindow`]'s frame its widgets change: its frame and mode, as the
/// window commands it honours set them, and the close requests it received.
#[derive(Debug, Default)]
struct KitChrome {
    frame: flui_platform::WindowFrame,
    mode: flui_platform::WindowMode,
    close_requests: usize,
    /// Whether the state changed since the host last told the UI runtime.
    unobserved: bool,
    /// The instant the window was asked for a frame at and the host has not
    /// served ([`PlatformWindow::request_frame_at`]).
    timed_frame: Option<flui_platform::EventTime>,
    /// Every timed frame request the window took, oldest first.
    timed_frame_requests: Vec<Option<flui_platform::EventTime>>,
}

impl HeadlessWindow {
    /// The commands the window honours, as it published them.
    fn capabilities(&self) -> flui_platform::WindowCapabilities {
        self.capabilities
    }

    /// The commands a window honours: its frame and its mode, which have no
    /// window system to refuse them, and every cursor, an application's image
    /// included, as a desktop window shows it; published, as a host's first
    /// snapshot is. The reports a test asks for are added by its builders,
    /// and [`without_cursor_images`](Self::without_cursor_images) takes the
    /// images away.
    fn published_capabilities() -> flui_platform::WindowCapabilities {
        use flui_platform::WindowCapability::Native;
        let mut capabilities = flui_platform::WindowCapabilities::NONE.into_parts();
        capabilities.sets_frame = Native;
        capabilities.app_title_bar = Native;
        capabilities.minimizes = Native;
        capabilities.maximizes = Native;
        capabilities.cursor_image = flui_platform::Availability::Native;
        Self::publish(capabilities)
    }

    /// `parts` as a new publication.
    fn publish(parts: flui_platform::CapabilityParts) -> flui_platform::WindowCapabilities {
        parts
            .into_capabilities()
            .publish()
            .expect("BUG: a test opens far fewer than u64::MAX windows")
    }

    /// Carry out an admitted `command`: a frame or a mode is taken at once, and the
    /// host tells the UI runtime at its next pump.
    ///
    /// # Errors
    /// [`ApplyError::Platform`](flui_platform::ApplyError::Platform) for a
    /// command its capabilities honour that this double has no arm for; its
    /// capabilities name none today.
    fn take(&self, command: flui_platform::WindowCommand) -> Result<(), flui_platform::ApplyError> {
        let mut chrome = self.chrome.lock();
        match command {
            flui_platform::WindowCommand::SetFrame(frame) => chrome.frame = frame,
            flui_platform::WindowCommand::SetMode(mode) => chrome.mode = mode,
            other => {
                return Err(flui_platform::ApplyError::failed(format!(
                    "the headless window does not carry out {other:?}"
                )));
            }
        }
        chrome.unobserved = true;
        drop(chrome);
        self.advance_revision();
        Ok(())
    }

    /// The window's state changed: what [`PlatformWindow::state`] reports is
    /// at the next revision.
    fn advance_revision(&self) {
        let mut revision = self.revision.lock();
        *revision = revision
            .next()
            .expect("BUG: a test changes a window far fewer than u64::MAX times");
    }

    /// Whether the window changed since the host last told the UI runtime, forgotten.
    fn take_unobserved(&self) -> bool {
        std::mem::take(&mut self.chrome.lock().unobserved)
    }

    /// How many times the window was asked to close through its
    /// `WindowHandle`, one closed window's asks included. The host's next
    /// turn carries an ask out ([`HeadlessHost::pump`]); this count only
    /// observes them.
    #[must_use]
    pub fn close_requests(&self) -> usize {
        self.chrome.lock().close_requests
    }

    /// Every timed frame request the window took
    /// ([`PlatformWindow::request_frame_at`]), oldest first: an instant on
    /// the host's event timeline, or `None` for a withdrawal. The frame a
    /// covered window owes its surface is asked for this way (ADR-0389).
    #[must_use]
    pub fn timed_frame_requests(&self) -> Vec<Option<flui_platform::EventTime>> {
        self.chrome.lock().timed_frame_requests.clone()
    }

    /// The instant of the window's timed frame request the host has not
    /// served yet.
    fn pending_timed_frame(&self) -> Option<flui_platform::EventTime> {
        self.chrome.lock().timed_frame
    }

    /// Serve the window's timed frame request if its instant is not later
    /// than `now`: it is asked once.
    fn serve_timed_frame(&self, now: flui_platform::EventTime) {
        let mut chrome = self.chrome.lock();
        if chrome.timed_frame.is_some_and(|at| at <= now) {
            chrome.timed_frame = None;
        }
    }

    /// Whether the host closed the window.
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}
impl HeadlessWindow {
    /// A window of `width` × `height` logical (and physical) pixels.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            // The host that opens the window gives it its id.
            id: WindowIdMint::new()
                .next()
                .expect("BUG: a fresh mint issues its first id"),
            size: flui_platform::WindowExtent::try_new(Size::new(
                f64::from(width),
                f64::from(height),
            ))
            .expect("BUG: a side in whole pixels is finite and not negative"),
            scale_factor: Mutex::new(DevicePixelRatio::ONE),
            cursor: Mutex::new(Cursor::Icon(CursorIcon::Default)),
            text_input: None,
            text_store_host: false,
            gesture_preset: flui_interaction::gesture::GestureSettings::default().into(),
            platform: flui_platform::TargetPlatform::Unknown,
            in_browser: false,
            assets: flui_platform::assets::AssetBundle::new(Arc::new(
                flui_platform::testing::MemoryAssets::new(),
            )),
            display: Mutex::new(None),
            execution: Mutex::new(flui_platform::WindowExecutionState::Running),
            occluded: Mutex::new(false),
            occluded_surface: flui_platform::OccludedSurface::Discarded,
            insets: Mutex::new(flui_platform::WindowInsets::ZERO),
            revision: Mutex::new(flui_platform::WindowRevision::ZERO),
            input_regions: Mutex::default(),
            haptics: Arc::new(flui_platform::testing::FakeHaptics::new()),
            os_drags: Arc::new(HeadlessOsDrags::default()),
            reports_presentation: false,
            capabilities: Self::published_capabilities(),
            chrome: Mutex::default(),
            closed: AtomicBool::new(false),
            portal_exports: None,
            fails_cursor_images: false,
            close_request_handler: None,
        }
    }

    /// Answer the window's close requests with `handler`, as an application's
    /// `AppConfig::close_request_handler` answers its runner's windows: asked
    /// after the window's close guard for a close the user or the program
    /// asks for, and before it for the session's end
    /// ([`CloseRequestRouter`]). A phone's window system never asks, so on
    /// Android and iOS it is never asked, as there.
    #[must_use]
    pub fn with_close_request_handler(mut self, handler: CloseRequestHandler) -> Self {
        self.close_request_handler = Some(handler);
        self
    }

    /// Name the window to a desktop portal as a Wayland window the compositor
    /// exports under `handle`: every request is answered
    /// `wayland:<handle>` and holds an export of its own until it ends or the
    /// window closes, which [`HeadlessPortalExports::live`] counts; a closed
    /// window answers no window. Without it the window answers no window, as
    /// one of a system with no portals does.
    #[must_use]
    pub fn with_portal_export(mut self, handle: impl Into<String>) -> Self {
        let handle: String = handle.into();
        self.portal_exports = Some(HeadlessPortalExports::new(Arc::from(handle)));
        self
    }

    /// The window's portal exports, when it was built
    /// [`with_portal_export`](Self::with_portal_export).
    #[must_use]
    pub fn portal_exports(&self) -> Option<HeadlessPortalExports> {
        self.portal_exports.clone()
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
        preset: impl Into<flui_interaction::gesture::GestureProfiles>,
    ) -> Self {
        self.gesture_preset = preset.into();
        self
    }

    /// Follow `platform`'s conventions: the runtime publishes it above the
    /// root as a [`PlatformConvention`](flui_view::environment::PlatformConvention),
    /// so word-jump chords, refresh keys, scroll physics and the rest are
    /// that platform's. Without one, [`TargetPlatform::Unknown`](flui_platform::TargetPlatform::Unknown)'s
    /// platform-neutral conventions: a headless test never takes them from
    /// the machine that runs it.
    #[must_use]
    pub fn with_platform(mut self, platform: flui_platform::TargetPlatform) -> Self {
        self.platform = platform;
        self
    }

    /// Serve `assets` as the application's assets: what a widget reads
    /// through the [`AssetBundleCapability`](flui_platform::assets::AssetBundleCapability).
    /// Without them the window's application has no assets, and every key is
    /// `NotFound`: a headless test never reads the checkout by accident
    /// (ADR-0384 §5). A test's own directory is a
    /// [`DirectoryAssets`](flui_platform::assets::DirectoryAssets), a map of
    /// bytes a [`MemoryAssets`](flui_platform::testing::MemoryAssets).
    #[must_use]
    pub fn with_assets(mut self, assets: impl flui_platform::assets::AssetSource) -> Self {
        self.assets = flui_platform::assets::AssetBundle::new(Arc::new(assets));
        self
    }

    /// Play a browser's page instead of the window system of the platform
    /// the window names, as a runner built for the web does, on whatever
    /// target the test runs: no close is put to the window's guard or
    /// handler (the browser never asks), and the session's end is the
    /// page's `pagehide`, which delivers the notice alone. The window still
    /// follows the conventions of its [platform](Self::with_platform), as a
    /// page follows its host's.
    ///
    /// The window system is the host's: the primary window sets it, and a
    /// window [opened](HeadlessHost::open_window) beside it that plays
    /// another is refused ([`WindowSystemMismatch`]).
    #[must_use]
    pub fn in_browser(mut self) -> Self {
        self.in_browser = true;
        self
    }

    /// The window system the window plays.
    fn window_system(&self) -> WindowSystem {
        if self.in_browser {
            WindowSystem::Browser
        } else {
            WindowSystem::Platform(self.platform)
        }
    }

    /// Open the window with `insets` already covering it, as a window a
    /// native host opens while the keyboard is up: its presentation is built
    /// from them, with no report of a change.
    /// [`HeadlessHost::set_window_insets`] reports a later one.
    #[must_use]
    pub fn with_insets(self, insets: flui_platform::WindowInsets) -> Self {
        *self.insets.lock() = insets;
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
        let mut parts = self.capabilities.into_parts();
        parts.reports_presentation = flui_platform::WindowCapability::Native;
        self.capabilities = Self::publish(parts);
        self
    }

    /// Play a window system that tells a covering by other windows
    /// ([`WindowCapabilities::reports_occlusion`](flui_platform::WindowCapabilities::reports_occlusion))
    /// and keeps a covered window's last frame
    /// ([`OccludedSurface::Kept`](flui_platform::OccludedSurface::Kept)), as
    /// AppKit does: an occluded snapshot ([`HeadlessHost::set_window_occluded`])
    /// may then be a covering whose surface is shown again on uncovering.
    /// Without it the window plays one whose occlusion is never a covering (a
    /// locked session, a background app).
    #[must_use]
    pub fn with_occlusion_reports(mut self) -> Self {
        let mut parts = self.capabilities.into_parts();
        parts.reports_occlusion = flui_platform::WindowCapability::Native;
        self.capabilities = Self::publish(parts);
        self.occluded_surface = flui_platform::OccludedSurface::Kept;
        self
    }

    /// Play a platform that shows no application's cursor image (a Wayland
    /// compositor without shared memory, a phone): the window's
    /// `cursor_image` capability is unavailable and it refuses an image
    /// `Unsupported`, as such a host does. Without it the window shows every
    /// cursor.
    #[must_use]
    pub fn without_cursor_images(mut self) -> Self {
        let mut parts = self.capabilities.into_parts();
        parts.cursor_image = flui_platform::Availability::Unavailable;
        self.capabilities = Self::publish(parts);
        self
    }

    /// Play a window system that takes cursor images and fails to make one
    /// (a Win32 `CreateIconIndirect` or an AppKit `NSCursor` that fails): the
    /// window refuses every image with `CursorError::Platform` though its
    /// `cursor_image` capability is available.
    #[must_use]
    pub fn failing_cursor_images(mut self) -> Self {
        self.fails_cursor_images = true;
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
    pub fn cursor(&self) -> Cursor {
        self.cursor.lock().clone()
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

    /// Every field status the UI runtime told the window's text input (a
    /// field attached, or its status changed), in delivery order; `None` for
    /// a window built without a text input.
    #[must_use]
    pub fn ime_field_statuses(&self) -> Option<Vec<TextStoreStatus>> {
        self.text_input
            .as_ref()
            .map(|input| input.field_statuses.lock().clone())
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
        let scale = self.scale_factor.lock().get();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "built from u32 pixel counts at a test's small scale factor, far below \
                      i32::MAX"
        )]
        Size::new(
            (self.size.width() * scale).round() as i32,
            (self.size.height() * scale).round() as i32,
        )
    }
}

impl PlatformWindow for HeadlessWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    /// No window system, so no gesture geometry: the runtime keeps its own.
    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        Ok(None)
    }

    /// The host's only window is on no desktop with others.
    fn window_at(&self, _: flui_foundation::geometry::Point<f64>) -> flui_platform::WindowAt {
        flui_platform::WindowAt::Unknown
    }

    /// Shown from the start; there is no reveal to defer.
    fn reveal_after_first_frame(&self) {}

    fn request_redraw(&self) {}

    /// Served by the host for this window: its loop waits no later than `at`
    /// ([`HeadlessHost::time_to_next_frame`]), and the first pump at or after
    /// it serves the request, once. Recorded for the test
    /// ([`HeadlessWindow::timed_frame_requests`]).
    fn request_frame_at(&self, at: Option<flui_platform::EventTime>) {
        let mut chrome = self.chrome.lock();
        chrome.timed_frame = at;
        chrome.timed_frame_requests.push(at);
    }

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

    /// Records `cursor`; an image is refused `Unsupported`, as `set_cursor`
    /// answers, exactly when the window's `cursor_image` capability is
    /// unavailable, and `Platform` where the window plays one that fails to
    /// make it.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        if matches!(cursor, Cursor::Image(_)) {
            if !self.capabilities.cursor_image().is_available() {
                return Err(flui_platform::Unsupported::from(
                    flui_platform::CapabilityAbsence::NOT_ON_THIS_PLATFORM,
                )
                .into());
            }
            if self.fails_cursor_images {
                return Err(CursorError::failed(
                    "the window system made no cursor of the image".to_owned(),
                ));
            }
        }
        cursor.clone_into(&mut self.cursor.lock());
        Ok(())
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::haptics::PlatformHaptics>> {
        Some(Arc::clone(&self.haptics) as Arc<dyn flui_platform::haptics::PlatformHaptics>)
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

    /// A new export for each request when the window plays an exported
    /// Wayland window ([`HeadlessWindow::with_portal_export`]), else no window.
    fn portal_parent(
        &self,
        answer: flui_platform::capability::Resolver<flui_platform::capability::PortalParent>,
    ) {
        // A closed window names nothing, as on every host.
        let parent = match &self.portal_exports {
            Some(exports) if !self.closed.load(Ordering::Relaxed) => exports.export(),
            _ => flui_platform::capability::PortalParent::none(),
        };
        // The owner retiring first leaves nothing waiting for the answer.
        let _ = answer.resolve(parent);
    }

    /// Shown and focused, with no position (as on Wayland), its logical size
    /// at its scale, the display it was
    /// [moved to](HeadlessHost::move_to_display), and a revision each move
    /// advances, and the frame and mode its commands set. The host's window is always
    /// shown: there is no window
    /// manager to raise it, and it honours no other command.
    fn state(&self) -> flui_platform::WindowState {
        let extent = self.size;
        let mut state = flui_platform::WindowState::new(
            flui_platform::WindowGeometry::unplaced(extent),
            extent,
        )
        .with_focus(flui_platform::WindowFocus::Focused)
        .with_display(*self.display.lock())
        .with_insets(*self.insets.lock())
        .with_execution(*self.execution.lock())
        .with_occluded(*self.occluded.lock())
        .with_metrics(
            flui_foundation::geometry::WindowMetrics::from_logical(
                extent.get(),
                *self.scale_factor.lock(),
            )
            .ok(),
        );
        state = state.with_capabilities(self.capabilities());
        let chrome = self.chrome.lock();
        state.frame = chrome.frame;
        state.mode = chrome.mode;
        drop(chrome);
        state.revision = *self.revision.lock();
        state
    }

    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        let command = command.against(&self.capabilities())?;
        self.take(command)
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
        let OsDragParts { terms, on_end, .. } = request.into_parts();
        let allowed = terms.allowed();
        self.started.lock().push(StartedOsDrag {
            representations: terms.into_representations(),
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
    field_statuses: Mutex<Vec<TextStoreStatus>>,
}

impl PlatformTextInput for RecordingTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.ime_allowed.lock().push(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        self.cursor_areas.lock().push(area);
    }

    fn set_field_status(&self, status: TextStoreStatus) {
        self.field_statuses.lock().push(status);
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

impl HeadlessAccessibility {
    /// Detach assistive technology: the listener clears the presentation's
    /// semantics flag, and the next frame stops collecting unless an agent
    /// still holds the tree.
    fn detach(&self) {
        self.active.store(false, Ordering::Relaxed);
        let listener = self.activation.lock().clone();
        if let Some(listener) = listener {
            listener(false);
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
    fn assistive_focus_support(&self) -> flui_semantics::platform::AssistiveFocusSupport {
        flui_semantics::platform::AssistiveFocusSupport::Reported
    }

    fn set_assistive_focus_listener(&self, listener: AssistiveFocusListener) {
        *self.assistive_focus.lock() = Some(listener);
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
}

impl HeadlessDevAgent {
    /// Take `hook` and attach it, as a runner does when its loop starts.
    #[must_use]
    pub fn attach(hook: impl DevAgentHook) -> Self {
        let hook: Box<dyn DevAgentHook> = Box::new(hook);
        Self {
            host: DevAgentHost::attach([hook]),
        }
    }

    /// Whether the hook is attached: it served and did not panic in `attach`
    /// or since.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.host.is_serving()
    }
}

impl std::fmt::Debug for HeadlessDevAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDevAgent")
            .field("host", &self.host)
            .finish()
    }
}
/// How long [`HeadlessHost::end_session`]'s `Ending` phase writes what the
/// documents published (ADR-0260).
pub use flui_view::__runtime::SESSION_END_BUDGET;

/// The capabilities a headless host provides when it is given no registry,
/// each scripted by the test, so no test reaches the network: what a runner
/// provides on the platform the primary window names, with the features the
/// test's build has. The clipboard and the background lanes always; the
/// primary selection only where the platform has one (Linux, as X11 and
/// Wayland have it); HTTP only with this crate's `http` feature, which the
/// facade's `http` turns on with the runner's. All of them are on the host's
/// scenario, whose clock the host alone drives: HTTP reports an unscripted
/// URL there.
struct ScriptedBuiltIns {
    clock: HostClock,
    clipboard: crate::ScriptedClipboard,
    primary_selection: crate::ScriptedPrimarySelection,
    background: HeadlessBackground,
    #[cfg(feature = "http")]
    http: crate::ScriptedHttp,
}

/// A headless host's clock as a test sees it: read it, and move it only with
/// [`advance`](Self::advance), which applies what the host's scenario has
/// due at the new instant. A clone is the same clock, so a widget's callback
/// may hold one; there is no way to move the time and skip the scenario.
#[derive(Clone)]
pub struct HostClock {
    driver: Rc<ScenarioDriver>,
}

impl HostClock {
    /// The virtual time since the host started.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.driver.clock().elapsed()
    }

    /// Move the time forward by `dt` and apply every change of the host's
    /// scenario due at the new instant.
    ///
    /// # Panics
    ///
    /// With the first panic of a change it applied, once every due change has
    /// run.
    pub fn advance(&self, dt: Duration) {
        let _applied = self.driver.advance(dt);
    }

    fn scenario(&self) -> &Scenario {
        self.driver.scenario()
    }
}

impl flui_foundation::MonotonicClock for HostClock {
    fn now(&self) -> flui_scheduler::Instant {
        flui_foundation::MonotonicClock::now(self.driver.clock())
    }
}

impl std::fmt::Debug for HostClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostClock")
            .field("elapsed", &self.elapsed())
            .finish()
    }
}

impl ScriptedBuiltIns {
    /// The built-ins on the scenario `driver` moves.
    fn on(driver: ScenarioDriver) -> Self {
        let scenario = driver.scenario();
        #[cfg(feature = "http")]
        let http = crate::ScriptedHttp::default();
        #[cfg(feature = "http")]
        http.report_to(scenario.failure_report());
        let clipboard = crate::ScriptedClipboard::on(scenario);
        let primary_selection = crate::ScriptedPrimarySelection::on(scenario);
        Self {
            clock: HostClock {
                driver: Rc::new(driver),
            },
            clipboard,
            primary_selection,
            background: HeadlessBackground::default(),
            #[cfg(feature = "http")]
            http,
        }
    }

    /// The providers a runner ships on `platform`, scripted, its widgets
    /// reading `assets`, with `storage` as the host's storage.
    fn built_ins(
        &self,
        windows: WindowSystem,
        assets: &flui_platform::assets::AssetBundle,
        storage: impl flui_platform::capability::CapabilityProvider<
            flui_view::persist::StorageCapability,
        >,
    ) -> flui_runtime::capability::BuiltIns {
        let assets = assets.clone();
        let built_ins = flui_runtime::capability::BuiltIns::new(
            windows,
            self.clipboard.clone(),
            self.background.provider(),
            storage,
            move |_: &flui_platform::capability::ProviderContext<'_>| Ok(assets.clone()),
        )
        .primary_selection(self.primary_selection.clone());
        #[cfg(feature = "http")]
        let built_ins = {
            let http = self.http.handle();
            built_ins
                .http(move |_: &flui_platform::capability::ProviderContext<'_>| Ok(http.clone()))
        };
        built_ins
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
    /// The provider of the lanes a widget acquires.
    fn provider(
        &self,
    ) -> impl flui_platform::capability::CapabilityProvider<flui_view::background::BackgroundCapability>
    {
        let services = Arc::clone(&self.services);
        move |ctx: &flui_platform::capability::ProviderContext<'_>| {
            Ok(flui_view::background::Background::new(
                Arc::clone(&services) as Arc<dyn flui_platform::executor::Executor>,
                ctx.completions().clone(),
            ))
        }
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
    /// The UI runtime, until the host's last window closes: a real host
    /// retires its UI runtime then, and so does this one
    /// ([`close_presentation`](Self::close_presentation)).
    ui_runtime: Option<UiRuntime>,
    clock: ManualClock,
    sink: HeadlessSink,
    /// The window the UI runtime was built on, kept after it closed so a
    /// test still reads what its double recorded.
    primary: HostWindow,
    scripted: ScriptedBuiltIns,
    /// The windows open, in opening order: the primary first while it is.
    open: Vec<HostWindow>,
    /// Issues the id of every window the host opens, the primary first,
    /// never one twice: a window opened after another closed has an id of
    /// its own.
    window_ids: WindowIdMint,
    /// The surfaces embedded in the primary window, in minting order.
    embedded: Vec<flui_interaction::ids::InputSurface>,
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
    /// The closes the windows' widgets asked for, which the next turn
    /// carries out.
    close_asks: CloseAsks,
    /// The UI runtime's id, kept after it retired: half of every window's
    /// address.
    ui_runtime_id: flui_foundation::UiRuntimeId,
    /// Every open window's close handler and guard, which every close, quit
    /// and session-end question is put to, as a runner's.
    close_requests: Rc<CloseRequestRouter>,
    /// The closes the window's platform lets the application refuse
    /// ([`close_vetoes`](flui_runtime::owner_turn::close_vetoes)).
    close_vetoes: Vec<CloseReason>,
    /// Where a quit the application or the user asked for stands.
    quit: QuitRequest,
    /// A quit no window holds any more, which the turn carries out.
    quit_owed: bool,
    /// The session end being answered: the query's request and the windows
    /// that refused it.
    session_end: Rc<SessionEnd>,
    /// Whether a phone's session end answered `Block` and holds the
    /// background until its writes end.
    background_hold: bool,
    /// How many times the application let the platform's session end go on:
    /// a phone's background hold whose writes ended, or the last block a
    /// query's request held.
    session_end_releases: Rc<std::cell::Cell<usize>>,
}

impl std::fmt::Debug for HeadlessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessHost")
            .field("ui_runtime", &self.ui_runtime)
            .field("window", &self.primary.window)
            .field("open", &self.open.len())
            .field("sink", &self.sink)
            .finish_non_exhaustive()
    }
}

/// The storage a headless host gives its UI runtime: the host's own, and
/// the storage the application provides as the
/// [`StorageCapability`](flui_view::persist::StorageCapability) in its
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
        Self::assemble(window, None, Registry::Given(registry), None)
    }

    /// [`Self::new`], its widgets acquiring what `plugin` registers beside
    /// the host's built-ins (the [`clipboard`](Self::clipboard), and the
    /// [`primary_selection`](Self::primary_selection) where the window's
    /// platform has one): how a package tests its plugin on a headless host
    /// (ADR-0152).
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
        Self::with_scenario(window, &Scenario::new(), plugin)
    }

    /// [`Self::with_plugin`] on `scenario`: the host drives the scenario's
    /// clock (frames, deadlines and scheduled changes on one timeline), and
    /// its built-ins (the clipboard, the primary selection and the HTTP
    /// client, each where it is registered) are on it, so a
    /// plugin whose doubles are on `scenario` answers on the same timeline as
    /// everything else (ADR-0154 §10). A scenario drives one host at a time:
    /// time moves only through that host.
    ///
    /// # Panics
    ///
    /// When another live host already drives `scenario`
    /// ([`Self::try_with_scenario`] returns that refusal instead); and as
    /// [`Self::with_plugin`].
    #[must_use]
    pub fn with_scenario(
        window: HeadlessWindow,
        scenario: &Scenario,
        plugin: &dyn flui_platform::capability::Plugin,
    ) -> Self {
        Self::try_with_scenario(window, scenario, plugin)
            .unwrap_or_else(|refused| panic!("with_scenario: {refused}: one host per scenario"))
    }

    /// [`Self::with_scenario`], refusing instead of panicking when another
    /// live host already drives `scenario`.
    ///
    /// # Errors
    ///
    /// [`AlreadyDriven`] while another host drives `scenario`.
    ///
    /// # Panics
    ///
    /// As [`Self::with_plugin`].
    pub fn try_with_scenario(
        window: HeadlessWindow,
        scenario: &Scenario,
        plugin: &dyn flui_platform::capability::Plugin,
    ) -> Result<Self, AlreadyDriven> {
        let scripted = ScriptedBuiltIns::on(scenario.drive()?);
        let registry = compose_registry(
            &scripted,
            window.window_system(),
            &window.assets,
            None,
            &AppRegistrations::new(),
            Some(plugin),
        );
        Ok(Self::assemble(
            window,
            None,
            Registry::Given(registry),
            Some(scripted),
        ))
    }

    /// [`Self::new`], its widgets acquiring `storage` as
    /// the [`StorageCapability`](flui_view::persist::StorageCapability).
    pub(crate) fn with_storage(window: HeadlessWindow, storage: Option<KitStorage>) -> Self {
        Self::with_application(window, storage, AppRegistrations::default())
    }

    /// [`Self::with_storage`], with what the application registers over the
    /// built-ins: its overrides and its plugins, as a runner builds the
    /// registry of an `Application` (ADR-0152).
    ///
    /// # Panics
    ///
    /// When the application's registrations conflict, as a run fails before
    /// its platform starts; and as [`Self::new`].
    pub(crate) fn with_application(
        window: HeadlessWindow,
        storage: Option<KitStorage>,
        application: AppRegistrations,
    ) -> Self {
        Self::assemble(window, storage, Registry::Composed(application), None)
    }

    fn assemble(
        mut window: HeadlessWindow,
        storage: Option<KitStorage>,
        registry: Registry,
        scripted: Option<ScriptedBuiltIns>,
    ) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the window size came from u32 pixel counts"
        )]
        let surface = (window.size.width() as u32, window.size.height() as u32);
        let text_store_host = window.text_store_host.then(RecordingTextStoreHost::new);
        let gesture_preset = window.gesture_preset.clone();
        let platform = window.platform;
        let window_system = window.window_system();
        let close_handler = window.close_request_handler.take();
        let assets = window.assets.clone();
        let window_ids = WindowIdMint::new();
        window.id = window_ids
            .next()
            .expect("BUG: a fresh mint issues the primary window's id");
        let window = Rc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let scripted = scripted.unwrap_or_else(|| {
            ScriptedBuiltIns::on(
                Scenario::new()
                    .drive()
                    .expect("BUG: a scenario made here has no other driver"),
            )
        });
        let clock = scripted.clock.driver.clock().clone();
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
        let registry = match registry {
            Registry::Given(registry) => registry,
            Registry::Composed(application) => compose_registry(
                &scripted,
                window_system,
                &assets,
                storage.as_ref(),
                &application,
                None,
            ),
        };
        host = host.with_capabilities(registry);
        if let Some(flush) = &flush {
            host = host.with_flush_registry(flush.registry());
        }
        let close_vetoes = flui_runtime::owner_turn::close_vetoes(
            window_system,
            Some(session_end_support_of(window_system)),
        );
        host = host.with_close_vetoes(&close_vetoes);
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
            .with_gesture_profiles(gesture_preset)
            .with_occluded_surface(window.occluded_surface)
            .with_platform(platform),
            1.0,
            host,
        )
        .expect("BUG: interaction lane identity exhausted");
        let close_asks = CloseAsks::default();
        install_window_control(
            &ui_runtime,
            ui_runtime.presentation_id(),
            &window,
            &close_asks,
        );
        let close_requests = Rc::new(CloseRequestRouter::new());
        register_close_requests(
            &close_requests,
            &close_vetoes,
            &ui_runtime,
            ui_runtime.presentation_id(),
            &window,
            close_handler,
        );
        let failures = Arc::new(Mutex::new(Vec::new()));
        ui_runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
        let recorded = Arc::clone(&failures);
        ui_runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            if let Some(text) = dropped_frame_text(report) {
                recorded.lock().push(text);
            }
        })));
        let primary = HostWindow {
            id: ui_runtime.presentation_id(),
            window,
            accessibility,
            text_store_host,
        };
        Self {
            ui_runtime_id: ui_runtime.id(),
            ui_runtime: Some(ui_runtime),
            clock,
            sink: HeadlessSink::new(surface.0, surface.1),
            open: vec![primary.clone()],
            primary,
            scripted,
            window_ids,
            embedded: Vec::new(),
            failures,
            presented: None,
            flush,
            flush_writes,
            close_asks,
            close_requests,
            close_vetoes,
            quit: QuitRequest::new(),
            quit_owed: false,
            session_end: Rc::default(),
            background_hold: false,
            session_end_releases: Rc::default(),
        }
    }

    /// [`ui_runtime`](Self::ui_runtime), to change.
    ///
    /// # Panics
    ///
    /// Once the host's last window closed, which retired it.
    fn ui_runtime_mut(&mut self) -> &mut UiRuntime {
        self.ui_runtime
            .as_mut()
            .expect("a host whose last window closed has retired its UI runtime; `try_ui_runtime` answers None then")
    }

    /// Whether the host's last window closed, retiring its UI runtime as a
    /// real host does: what the UI runtime still held to answer ended as
    /// [`OwnerRetired`](flui_platform::capability::OwnerRetired), and a
    /// [`pump`](Self::pump) moves only time, the scenario, the writes and
    /// the background lanes, as [`time_to_next_frame`](Self::time_to_next_frame)
    /// and [`pump_until_settled`](Self::pump_until_settled) wait for. An entry
    /// that reaches the UI runtime (input, semantics, a window's attachment)
    /// panics on a retired host.
    #[must_use]
    pub fn is_retired(&self) -> bool {
        self.ui_runtime.is_none()
    }

    /// Report that the primary window's frame of `frame` reached the screen
    /// at `at`, on a display refreshing every `refresh` where it names a
    /// period, as a host's platform does (DXGI statistics,
    /// `wp_presentation_time`): the report names that frame
    /// ([`FramePresented::frame`](flui_platform::FramePresented::frame)), and
    /// the next [`pump`](Self::pump)'s frame request of the primary window
    /// carries it
    /// ([`FrameRequest::presented`](flui_platform::FrameRequest::presented)),
    /// which its frame clock keeps. Its time is on the event timeline input
    /// and frames are stamped on: the time since the [`clock`](Self::clock)
    /// started. A later report before that pump replaces it.
    ///
    /// Returns whether the report was made. As a real host reports only a
    /// frame it presented, a frame the sink did not present recently
    /// ([`HeadlessSink::last_presented`]) is not reported, and neither is any
    /// frame of a primary window whose capabilities say it reports no
    /// presentation (made without
    /// [`HeadlessWindow::with_presentation_reports`]).
    pub fn report_presented(
        &mut self,
        frame: flui_foundation::FrameEpoch,
        at: flui_platform::EventTime,
        refresh: Option<flui_platform::RefreshPeriod>,
    ) -> bool {
        if !self.primary.window.reports_presentation
            || !self.sink.presented(frame)
            || self.is_closed(self.primary_window())
        {
            return false;
        }
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime().id(),
            presentation_id: self.primary.id,
        };
        // The double draws on one surface, its first.
        let stamp = flui_foundation::FrameStamp::new(
            address,
            frame,
            flui_foundation::SurfaceGeneration::ZERO.next(),
        );
        let mut presented = flui_platform::FramePresented::new(stamp, at);
        if let Some(refresh) = refresh {
            presented = presented.refresh(refresh);
        }
        self.presented = Some(presented);
        true
    }

    /// Hand this UI runtime's window to `agent`'s hook, as a runner hands it a
    /// window it has installed. The window's first semantics tree is built
    /// by the next [`pump`](Self::pump); dropping the UI runtime closes the window,
    /// and the hook's handle to it answers `gone` from then on.
    ///
    /// Does nothing when the hook is not attached.
    #[must_use]
    pub fn with_dev_agent(self, agent: &mut HeadlessDevAgent) -> Self {
        agent.host.publish(self.ui_runtime(), self.primary.id);
        self
    }

    /// Hand `window`, which this host [opened](Self::open_window), to `agent`'s hook, as a
    /// runner hands each window it installs; closing it ([`Self::request_close`]) closes the
    /// hook's handle to it.
    ///
    /// Does nothing when the hook is not attached.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn serve_window(&self, window: HeadlessWindowId, agent: &mut HeadlessDevAgent) {
        let presentation = self.open_window_of(window).id;
        agent.host.publish(self.ui_runtime(), presentation);
    }

    /// Attach `view` as the UI runtime's root widget, the root view sized to the
    /// window. Like a `flui-app` runner, the host first wraps it in
    /// [`flui_widgets::app_root`] (Tab, Shift+Tab, the clipboard chords), and
    /// the UI runtime then wraps that in its root scopes (`GestureArenaScope`,
    /// `VsyncScope`, `FocusTreeRoot`, `MediaQuery`); the first
    /// [`pump`](Self::pump) builds, lays out and paints it.
    ///
    /// # Errors
    ///
    /// [`flui_view::AttachError`] if a root is already attached.
    pub fn attach<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.attach_to(self.primary_window(), view)
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
    /// of the pump when nothing was reported before it; after a frame that
    /// failed nothing, when a window's semantics publication found an author
    /// error (an attribute its node's role does not support, or in debug
    /// builds siblings sharing an identifier), in every open window at once;
    /// a frame that failed otherwise drops its findings with it. See the
    /// [module docs](self#failures).
    pub fn pump(&mut self, dt: Duration) -> FrameOutcome {
        let pending = std::mem::take(&mut *self.failures.lock());
        if let Some(failure) = pending.into_iter().next() {
            panic!("{failure}");
        }
        // The scenario's changes due at the new instant apply before the
        // turn, so their answers settle in this pump.
        self.advance(dt);
        self.settle_lifecycle();
        // Background work runs before the frame, so an answer it posts
        // settles in this pump.
        self.scripted.background.drain_unless_held();
        if self.is_retired() {
            // The last window closed in an earlier turn or in this one's
            // settle: there is nothing left to frame.
            return FrameOutcome::default();
        }
        // A frame or mode a widget set reaches the UI runtime before the frame, as a
        // window system's state notice does. Walked by position rather than
        // over a collected list, so an idle pump asks nothing of the heap.
        let mut position = 0;
        while let Some(presentation) = self.presentation_at(position) {
            position += 1;
            if self
                .window_of(HeadlessWindowId(presentation))
                .take_unobserved()
            {
                self.observe(presentation, flui_runtime::owner::WindowObservation::State);
            }
        }
        // The frame is delivered as `HostInput::Frame` would be: every
        // window's vsync is the clock's reading, on the event timeline, so a
        // recording sees the frame where its deadlines fire.
        let vsync = self.event_now();
        // A timed frame request whose instant has come is served by this frame,
        // once: the runtime grants the owed frame of a window whose instant has
        // come at its first poll.
        for window in &self.open {
            window.window.serve_timed_frame(vsync);
        }
        let mut request = flui_platform::FrameRequest::new(vsync);
        if let Some(presented) = self.presented.take() {
            request = request.presented(presented);
        }
        for window in &self.open {
            self.ui_runtime().note_frame_request(window.id, request);
        }
        let mut frame_clock = self.clock.clone();
        let Self {
            ui_runtime: Some(ui_runtime),
            sink,
            ..
        } = self
        else {
            unreachable!("BUG: a host that is not retired holds its UI runtime");
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the kit keeps a pump's panic to settle failures before resuming"
        )]
        // As a runner's wake gate: a runtime whose frames are off (no window
        // seen) takes a background turn, unless a covered window owes its
        // surface a frame.
        let attempt = catch_unwind(AssertUnwindSafe(|| {
            if ui_runtime.runs_frames() {
                ui_runtime.pump(&mut frame_clock, sink)
            } else {
                ui_runtime.pump_background();
                FrameOutcome::default()
            }
        }));
        let first_failure = {
            let mut failures = self.failures.lock();
            let first = failures.first().cloned();
            failures.clear();
            first
        };
        // As a runner's owner delivery does at the end of each work item.
        self.ask_owed_frames();
        // Taken on every outcome: a failed frame's findings are its own, and
        // the failure that is raised already fails the test.
        let semantics_findings = self.take_semantics_findings();
        match (attempt, first_failure) {
            (Ok(outcome), None) => {
                assert!(
                    semantics_findings.is_empty(),
                    "the frame's semantics publication found author errors, by presentation: \
                     {semantics_findings:?}"
                );
                outcome
            }
            (Ok(_), Some(failure)) => panic!("{failure}"),
            (Err(payload), None) => resume_unwind(payload),
            (Err(payload), Some(failure)) => {
                tracing::error!(
                    first_failure = %failure,
                    "a panic unwound out of the pump after a contained frame failure; the \
                     contained failure is raised and the later panic is discarded"
                );
                // The contained failure is raised; the later payload is
                // destroyed under containment (ADR-0281).
                flui_foundation::panic::discard_payload(payload);
                panic!("{failure}")
            }
        }
    }

    /// Takes every open window's semantics findings from the frame that just
    /// ran ([`crate::take_semantics_findings`]), each beside its presentation;
    /// none once the last window closed and the UI runtime retired.
    /// Every pump takes them, whatever its outcome, so a finding is never
    /// reported against a later frame.
    fn take_semantics_findings(
        &self,
    ) -> Vec<(
        flui_foundation::PresentationId,
        Vec<flui_rendering::semantics::SemanticsDiagnostic>,
    )> {
        let Some(ui_runtime) = self.try_ui_runtime() else {
            return Vec::new();
        };
        self.open
            .iter()
            .filter_map(|window| {
                let pipeline = ui_runtime
                    .presentation_widgets_for_test(window.id)
                    .pipeline_owner()?;
                let findings = crate::take_semantics_findings(&pipeline);
                (!findings.is_empty()).then_some((window.id, findings))
            })
            .collect::<Vec<_>>()
    }

    /// Leave the next `frames` frames unshown for the reason `how` names, as
    /// a real surface does: those frames render, their tree stays
    /// uncommitted, and the UI runtime recovers as `how` says. Replaces what
    /// an earlier call left to come; `frames` of 0 shows every frame again.
    pub fn leave_unshown(&mut self, frames: u32, how: UnshownFrame) {
        self.sink.unshown = NonZeroU32::new(frames).map(|frames| (frames, how));
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
        self.dispatch_to(self.primary_window(), input);
    }

    /// [`dispatch`](Self::dispatch) `input` as `window` reported it. Keyboard
    /// input goes to the window that holds the OS focus, whichever window
    /// reported it, as on every real host.
    ///
    /// # Panics
    ///
    /// As [`dispatch`](Self::dispatch).
    pub fn dispatch_to(&self, window: HeadlessWindowId, input: PlatformInput) {
        self.dispatch_on_to(window.0, flui_interaction::ids::InputSurface::Window, input);
    }

    /// [`dispatch`](Self::dispatch) as `surface` of `presentation`'s window.
    fn dispatch_on_to(
        &self,
        presentation: PresentationId,
        surface: flui_interaction::ids::InputSurface,
        input: PlatformInput,
    ) {
        // A real host routes nothing to a window that closed.
        if !self.is_open(HeadlessWindowId(presentation)) {
            return;
        }
        self.ui_runtime().enter(|ui_runtime| {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: the kit keeps the input panic so the flush still runs"
            )]
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                ui_runtime.handle_input_on(presentation, surface, input);
            }));
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: the kit keeps the flush panic so later input steps still run"
            )]
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
                    flui_foundation::panic::discard_payload(later);
                    resume_unwind(first);
                }
                (Err(payload), Ok(())) | (Ok(()), Err(payload)) => resume_unwind(payload),
                (Ok(()), Ok(())) => {}
            }
        });
        // The dispatch is over: the OS drag sessions it decided on start here,
        // as a host's outermost callback starts them (ADR-0211 §7).
        self.ui_runtime().enter_pending_os_drags();
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
        self.ui_runtime().set_pointer_resampling(window.0, policy)
    }

    /// The pointer left the window: the UI runtime sweeps hover state, so every
    /// hovered region gets its exit and the cursor resets.
    pub fn dispatch_hover_left(&self) {
        self.observe(
            self.primary.id,
            flui_runtime::owner::WindowObservation::Hover(false),
        );
    }

    /// Attach assistive technology to the window, as a platform adapter does:
    /// the next [`pump`](Self::pump) turns semantics on and assembles the
    /// tree.
    pub fn enable_semantics(&self) {
        self.primary.accessibility.attach();
    }

    /// Detach assistive technology from the window, as a platform adapter
    /// does when the last screen reader goes: the next [`pump`](Self::pump)
    /// stops collecting semantics unless a development agent still asks.
    pub fn disable_semantics(&self) {
        self.primary.accessibility.detach();
    }

    /// Whether the window's pipeline holds a semantics tree: assistive
    /// technology or a development agent asked for one, and a
    /// [`pump`](Self::pump) has run since. It stops once nothing asks and
    /// the next pump has run.
    #[must_use]
    pub fn collects_semantics(&self) -> bool {
        self.ui_runtime()
            .presentation_collects_semantics_for_test(self.primary.id)
            .expect("BUG: a headless ui_runtime's window stays resident while the ui_runtime lives")
    }

    /// The listener the UI runtime registered for actions assistive technology
    /// requests. It is `Send + Sync`: a test plays the platform adapter by
    /// calling it, from any thread, and the UI runtime queues the request in its
    /// owner inbox for the next pump to apply.
    #[must_use]
    pub fn accessibility_action_listener(&self) -> Option<AccessibilityActionListener> {
        self.primary.accessibility.action.lock().clone()
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
            .primary
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
        self.ui_runtime().enter(f)
    }

    /// The hosted UI runtime.
    ///
    /// # Panics
    ///
    /// Once the host's last window closed, which retired it.
    pub(crate) fn ui_runtime(&self) -> &UiRuntime {
        self.try_ui_runtime().expect(
            "a host whose last window closed has retired its UI runtime; `try_ui_runtime` \
             answers None then",
        )
    }

    /// The hosted UI runtime, `None` once the host's last window closed.
    pub(crate) fn try_ui_runtime(&self) -> Option<&UiRuntime> {
        self.ui_runtime.as_ref()
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
        self.ui_runtime()
            .widgets()
            .with_build_owner(|owner| owner.post_frame_handle().cloned())
            .expect("BUG: a ui_runtime's presentation installs its owner-local post-frame handle")
    }

    /// Ask the UI runtime for a frame, as a widget's `setState` would: the next
    /// [`pump`](Self::pump) runs the pipeline even with nothing dirty.
    pub fn request_frame(&self) {
        self.ui_runtime().request_redraw();
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

    /// The clock the UI runtime reads: the frame time, the gesture-arena
    /// deadlines and the produce gate. It moves only through
    /// [`HostClock::advance`], [`advance`](Self::advance) and
    /// [`pump`](Self::pump), each of which applies the scenario's changes
    /// that fall due.
    #[must_use]
    pub fn clock(&self) -> &HostClock {
        &self.scripted.clock
    }

    /// Move the clock forward by `dt` without a frame, and apply every
    /// change of the host's [scenario](Self::scenario) due at the new
    /// instant, as time passing between two input events does. Every path of
    /// the host that moves time goes through it.
    pub fn advance(&self, dt: Duration) {
        self.scripted.clock.advance(dt);
    }

    /// The sink frames are submitted through.
    #[must_use]
    pub fn sink(&self) -> &HeadlessSink {
        &self.sink
    }

    /// The window the UI runtime presents into.
    #[must_use]
    pub fn window(&self) -> &HeadlessWindow {
        &self.primary.window
    }

    /// The pull-model host the UI runtime's presentation speaks to, for a window
    /// built [`HeadlessWindow::with_text_store_host`].
    #[must_use]
    pub fn text_store_host(&self) -> Option<&Rc<RecordingTextStoreHost>> {
        self.primary.text_store_host.as_ref()
    }

    /// The scenario whose clock the host frames on and whose due changes
    /// each pump applies.
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        self.scripted.clock.scenario()
    }

    /// The clipboard the UI runtime hands its widgets.
    #[must_use]
    pub fn clipboard(&self) -> crate::ScriptedClipboard {
        self.scripted.clipboard.clone()
    }

    /// The HTTP client the UI runtime hands its widgets: every request
    /// answers what the test scripted for its URL, and no request reaches the
    /// network. Registered only with this crate's `http` feature, as a
    /// runner registers HTTP only with its own.
    #[cfg(feature = "http")]
    #[must_use]
    pub fn http(&self) -> crate::ScriptedHttp {
        self.scripted.http.clone()
    }

    /// The primary selection the UI runtime hands its widgets where the
    /// primary window's platform has one (Linux); elsewhere a widget is
    /// refused it, as on that platform, and what this double is told reaches
    /// nobody.
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
        self.host_window(window).text_store_host.as_ref()
    }

    /// The window the UI runtime was built on.
    #[must_use]
    pub fn primary_window(&self) -> HeadlessWindowId {
        HeadlessWindowId(self.primary.id)
    }

    /// Ask to close `window` for `reason`, as its platform does when the user
    /// clicks its close button (or as the program or the session asks). Where
    /// the window's platform lets the application refuse that close
    /// ([`close_vetoes`](flui_runtime::owner_turn::close_vetoes)), it is put
    /// to the window's close guard and its close-request handler
    /// ([`HeadlessWindow::with_close_request_handler`]) in the order a
    /// runner's window asks them ([`CloseRequestRouter::consult`]: the guard
    /// first, the handler first at the session's end): a close either refuses is
    /// recorded, nothing is torn down, and the pump after the last hold is
    /// released carries it out. `true` when the window closed now; `false`
    /// when it was refused or the window is already gone.
    ///
    /// A [`CloseReason::Program`] close a guard recorded is, when it comes
    /// due, the window's consent to a quit ([`request_quit`](Self::request_quit)),
    /// as a runner treats it: it closes nothing alone.
    ///
    /// A window another one outlives is removed and the OS focus moves to a
    /// survivor; the last one retires the UI runtime, as on a real host
    /// ([`is_retired`](Self::is_retired)).
    pub fn request_close(&mut self, window: HeadlessWindowId, reason: CloseReason) -> bool {
        if !self.is_open(window) {
            return false;
        }
        if self.close_vetoes.contains(&reason) {
            let router = Rc::clone(&self.close_requests);
            if router.consult(self.address(window.0), reason) != CloseResponse::Close {
                return false;
            }
        }
        self.ui_runtime().commit_composition_for_close(window.0);
        self.close_presentation(window.0)
    }

    /// The address of presentation `id` of this host's UI runtime.
    fn address(&self, id: PresentationId) -> flui_foundation::PresentationAddress {
        flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime_id,
            presentation_id: id,
        }
    }

    /// Whether `window` has closed: at once by [`request_close`](Self::request_close),
    /// by a pump after its close's last hold was released or a decision
    /// closed it, or because it is gone. A close refused and still recorded
    /// is not one.
    #[must_use]
    pub fn is_closed(&self, window: HeadlessWindowId) -> bool {
        !self.is_open(window)
            || self
                .ui_runtime()
                .presentation_closing_for_test(window.0)
                .unwrap_or(true)
    }

    /// Whether `window` is one of the windows open now.
    fn is_open(&self, window: HeadlessWindowId) -> bool {
        self.open.iter().any(|open| open.id == window.0)
    }

    /// Open `window` beside the existing ones, as a runner installs a
    /// presentation for a window the app opened: the UI runtime builds it a
    /// pipeline of its own at the window's scale factor. It shows nothing
    /// until a view is [attached](Self::attach_to) to it.
    ///
    /// The window gets a window identity no window of this host had, open
    /// or closed. It reports itself as it opened, focused and shown, as a
    /// native window does once installed: it takes the OS focus from the
    /// other windows, which forget their held keys, and the application's
    /// lifecycle moves with it.
    ///
    /// The window system is the host's, set by its primary window
    /// ([`HeadlessWindow::in_browser`]): a window that plays another is
    /// refused ([`try_open_window`](Self::try_open_window)). A window's
    /// [platform](HeadlessWindow::with_platform) names only its conventions.
    ///
    /// # Panics
    ///
    /// When `window` plays another window system than the host's; and resumes
    /// the panic of an observer that report reaches, with the window open.
    pub fn open_window(&mut self, window: HeadlessWindow) -> HeadlessWindowId {
        self.try_open_window(window)
            .unwrap_or_else(|refused| panic!("open_window: {refused}"))
    }

    /// [`open_window`](Self::open_window), refusing instead of panicking when
    /// `window` plays another window system than the host's.
    ///
    /// # Errors
    ///
    /// [`WindowSystemMismatch`] when `window` plays the browser and the host
    /// does not, or the other way round.
    ///
    /// # Panics
    ///
    /// Resumes the panic of an observer that report reaches, with the window
    /// open.
    pub fn try_open_window(
        &mut self,
        window: HeadlessWindow,
    ) -> Result<HeadlessWindowId, WindowSystemMismatch> {
        let host = self.primary.window.window_system();
        if window.in_browser != self.primary.window.in_browser {
            return Err(WindowSystemMismatch {
                host,
                window: window.window_system(),
            });
        }
        Ok(self.install_window(window))
    }

    /// Install `window` beside the open ones.
    fn install_window(&mut self, mut window: HeadlessWindow) -> HeadlessWindowId {
        window.id = self
            .window_ids
            .next()
            .expect("BUG: a test opens far fewer than u64::MAX windows");
        let text_store_host = window.text_store_host.then(RecordingTextStoreHost::new);
        let gesture_preset = window.gesture_preset.clone();
        let platform = window.platform;
        let close_handler = window.close_request_handler.take();
        let window = Rc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let presentation = self.ui_runtime().assemble_presentation(
            PresentationWindow::new(
                Rc::clone(&window) as Rc<dyn PlatformWindow>,
                Some(Arc::clone(&accessibility) as Arc<dyn PlatformAccessibility>),
            )
            .with_text_store_host(
                text_store_host
                    .clone()
                    .map(|host| host as Rc<dyn TextStoreHost>),
            )
            .with_gesture_profiles(gesture_preset)
            .with_occluded_surface(window.occluded_surface)
            .with_platform(platform),
        );
        let id = self.ui_runtime_mut().install_presentation(presentation);
        install_window_control(self.ui_runtime(), id, &window, &self.close_asks);
        register_close_requests(
            &self.close_requests,
            &self.close_vetoes,
            self.ui_runtime(),
            id,
            &window,
            close_handler,
        );
        let seed = window.state();
        self.open.push(HostWindow {
            id,
            window,
            accessibility,
            text_store_host,
        });
        // The window reports itself as it opened, as a native host does once
        // it is installed: the change it makes to the application's lifecycle
        // is delivered in the owner's turn.
        self.observe(
            id,
            flui_runtime::owner::WindowObservation::Snapshot {
                execution: seed.execution,
                focused: seed.focus.is_focused(),
                visible: seed.is_seen(),
            },
        );
        HeadlessWindowId(id)
    }

    /// Attach `view` as the root widget of `window`, wrapped as
    /// [`attach`](Self::attach) wraps the primary window's.
    ///
    /// # Errors
    ///
    /// [`flui_view::AttachError`] if a root is already attached.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host or has closed.
    pub fn attach_to<V>(
        &self,
        window: HeadlessWindowId,
        view: &V,
    ) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        // Every window's root is the same: the catalog's keyboard bindings,
        // then this window's own `MediaQuery` and its size.
        let size = self.open_window_of(window).window.size;
        self.ui_runtime().attach_root_widget_with_size_to(
            window.0,
            &flui_widgets::app_root(view.clone()),
            size.width(),
            size.height(),
        )
    }

    /// Hand `intent` to `window`'s URL owner, the outermost `Router` of its
    /// tree, as a runner hands it a URL the platform opened for that window
    /// (ADR-0330 §5). The Router's edit is built by the next
    /// [`pump`](Self::pump).
    ///
    /// # Errors
    ///
    /// What [`UiRuntime::navigate`] refuses with:
    /// [`NoPresentation`](flui_view::navigation::NavigationRefused::NoPresentation)
    /// for a window that has closed,
    /// [`NoUrlOwner`](flui_view::navigation::NavigationRefused::NoUrlOwner)
    /// for a tree with no `Router`, and the Router's own refusals.
    pub fn navigate(
        &self,
        window: HeadlessWindowId,
        intent: flui_view::navigation::NavigationIntent,
    ) -> Result<(), flui_view::navigation::NavigationRefused> {
        self.ui_runtime().navigate(window.0, intent)
    }

    /// Open `url` in `window`, as a runner opens a URL the platform hands
    /// that window: split, admitted by the declarations
    /// [`set_open_declarations`](Self::set_open_declarations) set, and
    /// applied to the window's `Router` as a `Go` (ADR-0330 §5). The Router's
    /// edit is built by the next [`pump`](Self::pump).
    ///
    /// # Errors
    ///
    /// What [`UiRuntime::open_url`] refuses with: a URL too long, malformed
    /// or not declared, and the navigation refusals of
    /// [`navigate`](Self::navigate).
    pub fn open_url(
        &self,
        window: HeadlessWindowId,
        url: &str,
    ) -> Result<(), flui_view::navigation::OpenUrlRefused> {
        self.ui_runtime().open_url(window.0, url)
    }

    /// Set the URL schemes and associated domains the application declares,
    /// which [`open_url`](Self::open_url) admits; a fresh host declares none.
    pub fn set_open_declarations(&self, declarations: flui_view::navigation::OpenDeclarations) {
        self.ui_runtime().set_open_declarations(declarations);
    }

    /// Run as a page of `origin` (`https://app.example.com`), as the web
    /// platform's runner sets it: [`open_url`](Self::open_url) admits that
    /// origin's URLs beside the declarations ([`UiRuntime::set_page_origin`]).
    ///
    /// # Errors
    ///
    /// What [`UiRuntime::set_page_origin`] refuses `origin` with.
    pub fn set_page_origin(
        &self,
        origin: &str,
    ) -> Result<(), flui_view::navigation::OpenUrlRefused> {
        self.ui_runtime().set_page_origin(origin)
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
    /// If `window` was not opened by this host, or its logical size at
    /// `scale_factor` is no drawable area (past `i32` device pixels, or below
    /// one); [`try_set_scale_factor`](Self::try_set_scale_factor) answers that
    /// instead.
    pub fn set_scale_factor(&mut self, window: HeadlessWindowId, scale_factor: DevicePixelRatio) {
        self.try_set_scale_factor(window, scale_factor).expect(
            "a scale the window's size has metrics at; try_set_scale_factor answers one it has not",
        );
    }

    /// [`set_scale_factor`](Self::set_scale_factor) for a scale that may make
    /// no metrics: the window changes only when it does.
    ///
    /// # Errors
    ///
    /// The [`InvalidMetrics`](flui_foundation::geometry::InvalidMetrics) the
    /// window's logical size at `scale_factor` makes: `Overflow` or
    /// `EmptyExtent` for a size that does not fit at it. Nothing changes.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn try_set_scale_factor(
        &mut self,
        window: HeadlessWindowId,
        scale_factor: DevicePixelRatio,
    ) -> Result<(), flui_foundation::geometry::InvalidMetrics> {
        // The runner's path: a metrics observation of the window, which a
        // recording sees as the `Metrics` row it is.
        let size = self.window_of(window).size;
        let metrics =
            flui_foundation::geometry::WindowMetrics::from_logical(size.get(), scale_factor)?;
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Metrics(metrics),
        );
        self.size_sink_to_primary();
        Ok(())
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
    /// period is what the UI runtime paces `window`'s presentation by. A
    /// window that closed hears nothing, as on a real host.
    pub fn set_window_display(
        &self,
        window: HeadlessWindowId,
        display: Option<flui_platform::WindowDisplay>,
    ) {
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Display(
                flui_runtime::owner::DisplayUpdate::new(display),
            ),
        );
    }

    /// Report that `window` gained OS focus, as a backend's focus callback
    /// does: keyboard input goes to its presentation from now on, whatever
    /// window it is stamped for, and the window that held focus loses it. A
    /// window that closed hears nothing, as on a real host.
    pub fn focus_window(&self, window: HeadlessWindowId) {
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
        self.ui_runtime()
            .enter(|ui_runtime| ui_runtime.apply_system_preferences_for_test(values));
    }

    /// Report the primary window shown (`true`) or hidden, minimized or fully
    /// occluded (`false`), as a backend's visibility callback does: the host
    /// delivers `WindowObservation::Visibility(visible)`, and the window's
    /// focus and execution state stay as they were.
    pub fn set_window_visible(&self, visible: bool) {
        self.observe(
            self.primary.id,
            flui_runtime::owner::WindowObservation::Visibility(visible),
        );
    }

    /// Report that the primary window gained (`true`) or lost OS focus, as a
    /// backend's focus callback does: the host delivers
    /// `WindowObservation::Focus(focused)`. A window that loses focus forgets
    /// its pressed keys and cancels its pointer sequences; the focus inside
    /// its tree stays where it was. [`focus_window`](Self::focus_window)
    /// moves the OS focus between windows this host opened.
    pub fn set_window_focused(&self, focused: bool) {
        self.observe(
            self.primary.id,
            flui_runtime::owner::WindowObservation::Focus(focused),
        );
    }

    /// Report whether the platform lets `window` run, as a phone's host
    /// reports its activity or scene stopping and starting (Android's
    /// `onStop`/`onStart`, a UIKit scene entering the background, a page
    /// frozen by the browser): the window's state says so, and the host
    /// delivers `WindowObservation::Execution(execution)`, which suspends
    /// (`Paused`) or resumes the window's lifecycle. Its focus and visibility
    /// stay as they were. A window that closed hears nothing, as on a real
    /// host.
    pub fn set_window_execution(
        &self,
        window: HeadlessWindowId,
        execution: flui_platform::WindowExecutionState,
    ) {
        let Some(open) = self.open.iter().find(|open| open.id == window.0) else {
            return;
        };
        *open.window.execution.lock() = execution;
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Execution(execution),
        );
    }

    /// Report whether the window system keeps `window` off the screen
    /// ([`WindowState::occluded`](flui_platform::WindowState::occluded)), as a
    /// host reports a covering, a lock or a background app. As a host does on
    /// a change, the window's state says so at a new revision, the host
    /// delivers the new snapshot (`WindowObservation::State`) and, when it
    /// changes whether the window is seen, that too
    /// (`WindowObservation::Visibility`); the same value again changes and
    /// delivers nothing. Whether the occlusion may be a covering is the
    /// window's [`with_occlusion_reports`](HeadlessWindow::with_occlusion_reports).
    /// A window that closed hears nothing, as on a real host.
    pub fn set_window_occluded(&self, window: HeadlessWindowId, occluded: bool) {
        let Some(open) = self.open.iter().find(|open| open.id == window.0) else {
            return;
        };
        let was_seen = open.window.state().is_seen();
        if std::mem::replace(&mut *open.window.occluded.lock(), occluded) == occluded {
            return;
        }
        open.window.advance_revision();
        let seen = open.window.state().is_seen();
        self.observe(window.0, flui_runtime::owner::WindowObservation::State);
        if seen != was_seen {
            self.observe(
                window.0,
                flui_runtime::owner::WindowObservation::Visibility(seen),
            );
        }
    }

    /// Deliver `notice` to every subscriber of the application, as a host
    /// delivers the system's notices: the machine about to sleep or waking,
    /// the session locked or unlocked, or the system asking for memory back.
    /// A session's end has its own entries
    /// ([`end_session`](Self::end_session)).
    ///
    /// A retired host has no subscriber left to tell.
    ///
    /// # Panics
    ///
    /// With the first panic of a subscriber, once every subscriber heard the
    /// notice.
    pub fn notify(&self, notice: flui_platform::SystemNotice) {
        if let Some(runtime) = self.try_ui_runtime() {
            runtime.deliver_host_notice(&flui_platform::HostNotice::from(notice));
        }
        self.ask_owed_frames();
    }

    /// Report `insets` as what covers `window`, as a host reports a change of
    /// its safe area, keyboard or system gesture regions: the presentation's
    /// `MediaQuery` carries them from the next [`pump`](Self::pump), and a
    /// moving keyboard is sampled at each frame's time until it settles. A
    /// window that closed hears nothing, as on a real host.
    pub fn set_window_insets(&self, window: HeadlessWindowId, insets: flui_platform::WindowInsets) {
        let Some(open) = self.open.iter().find(|open| open.id == window.0) else {
            return;
        };
        *open.window.insets.lock() = insets;
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Insets(insets),
        );
    }

    /// Start moving `window`'s keyboard to `placement` now, over `duration`
    /// on `curve`, as a host reports a keyboard that begins to show, hide,
    /// undock or dock: the inset moves from where it is at this instant, and
    /// the safe area and system gesture regions stay as they were. Returns
    /// the keyboard as reported, whose [`at`](flui_platform::SoftKeyboard::at)
    /// names the inset each later frame lays out with.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host, or the motion is invalid: a
    /// placement [`SoftKeyboard::try_moving`](flui_platform::SoftKeyboard::try_moving)
    /// refuses, a zero `duration`, or an invalid `curve`.
    pub fn move_keyboard(
        &self,
        window: HeadlessWindowId,
        placement: flui_platform::KeyboardPlacement,
        duration: Duration,
        curve: flui_platform::InsetCurve,
    ) -> flui_platform::SoftKeyboard {
        let now = self.event_now();
        let insets = *self.window_of(window).insets.lock();
        let motion =
            flui_platform::InsetMotion::try_new(insets.keyboard().at(now), now, duration, curve)
                .expect("a keyboard motion with a duration and a valid curve");
        let keyboard = flui_platform::SoftKeyboard::try_moving(placement, motion)
            .expect("a keyboard placement that is finite, not negative and not empty");
        self.set_window_insets(window, insets.with_keyboard(keyboard));
        keyboard
    }

    /// The primary window's gesture arena, the one its detectors compete in:
    /// observe it to watch the arena decide what this host dispatches.
    #[must_use]
    pub fn arena(&self) -> &flui_interaction::gesture::GestureArena {
        self.ui_runtime().gestures().arena()
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
        factory.install_on(self.ui_runtime())
    }

    /// The UI runtime's tap registry: [`record`](crate::recording::record)
    /// into it to record everything this host applies (ADR-0215 §1).
    #[must_use]
    pub fn input_taps(&self) -> &flui_runtime::InputTaps {
        self.ui_runtime().input_taps()
    }

    /// Where a native journal sends its lines for this host's recordings of the `native` layer
    /// (ADR-0288 §11). A headless host has no backend to journal, so a test plays the backend
    /// and sends the lines itself; until it calls this, `native` is refused.
    #[must_use]
    pub fn attach_native_journal(&self) -> flui_runtime::ui_runtime::NativeLines {
        let lines = flui_runtime::ui_runtime::NativeLines::default();
        self.ui_runtime().attach_native_journal(&lines);
        lines
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
            &self.ui_runtime().recording_start(),
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
    /// [`SurfaceIdsExhausted`](flui_interaction::ids::SurfaceIdsExhausted) once the
    /// binding minted every identity.
    pub fn embed_surface(
        &mut self,
    ) -> Result<flui_interaction::ids::InputSurface, flui_interaction::ids::SurfaceIdsExhausted>
    {
        let surface = self.ui_runtime().gestures().embed_surface()?;
        self.embedded.push(surface);
        Ok(surface)
    }

    /// [`dispatch`](Self::dispatch) `input` as `surface` of the primary window
    /// reported it.
    ///
    /// # Panics
    ///
    /// As [`dispatch`](Self::dispatch).
    pub fn dispatch_on(&self, surface: flui_interaction::ids::InputSurface, input: PlatformInput) {
        self.dispatch_on_to(self.primary.id, surface, input);
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
    /// it has not embedded, or with the first panic an applied input or frame
    /// raised, a frame's semantics author error included ([`pump`](Self::pump)):
    /// a recording of an application with one stops at that frame.
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
                self.ui_runtime()
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
                            self.ui_runtime().enter(|ui_runtime| {
                                ui_runtime.dispatch_accessibility(presentation, request);
                            });
                        }
                        HostInput::AssistiveFocus(node) => {
                            self.ui_runtime().enter(|ui_runtime| {
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
    /// With the first panic a delivered input or frame raised, a frame's
    /// semantics author error included ([`pump`](Self::pump)).
    pub fn run_script(&mut self, script: &crate::replay::PointerScript) {
        let window = self.primary_window();
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
            scale.to_logical(bounds.x0.midpoint(bounds.x1)),
            scale.to_logical(bounds.y0.midpoint(bounds.y1)),
        ))
    }

    /// How long a host's loop waits before its next frame: none while a frame
    /// is asked for or work is pending, unless a dropped frame waits for its
    /// retry; otherwise until the earliest deadline the UI runtime armed, such
    /// as a gesture's or a dropped frame's retry, the instant a window asked
    /// for a frame at (the frame a covered window owes its surface), or the
    /// next change the host's scenario scheduled. `None` when the loop would
    /// wait for input alone.
    #[must_use]
    pub fn time_to_next_frame(&self) -> Option<Duration> {
        let Some(runtime) = &self.ui_runtime else {
            // A retired host frames nothing: its loop wakes for the
            // background lanes and the scenario alone.
            if self.background_runs_next_pump() {
                return Some(Duration::ZERO);
            }
            return self.scenario_wait();
        };
        // What changed since the last turn (a signal written between pumps)
        // may leave a covered window owing a frame: its window is asked first,
        // as a runner's owner delivery asks at the end of the work item that
        // changed it, and the wait below counts the request.
        self.ask_owed_frames();
        let now = flui_foundation::MonotonicClock::now(&self.clock);
        // A dropped frame waiting for its retry runs nothing sooner, though a
        // gesture deadline before it still wakes the loop.
        let retrying = runtime
            .failure_retry_deadline()
            .is_some_and(|retry| retry > now);
        // A runtime whose frames are off (no window seen) has no frame to run
        // now: its pump is a background turn, which its deadlines, background
        // work and observations still ask for. The frame a covered window owes
        // is served only through its window's timed request, as on a native
        // host, never as work pending here.
        let frame_owed = runtime.scheduler().frames_enabled()
            && (runtime.needs_redraw()
                || runtime.scheduler().is_frame_scheduled()
                || runtime.has_pending_work());
        if !retrying
            && (frame_owed
                || self.background_runs_next_pump()
                || self.observations_pending()
                || self.turn_owed())
        {
            return Some(Duration::ZERO);
        }
        let runtime_wait = runtime
            .next_wake()
            .map(|wake| wake.saturating_duration_since(now));
        // A change the scenario has scheduled is a deadline like any other, and
        // so is a window's timed frame request, which the host serves.
        [runtime_wait, self.scenario_wait(), self.timed_frame_wait()]
            .into_iter()
            .flatten()
            .min()
    }

    /// How long until the scenario's next scheduled change, if one is.
    fn scenario_wait(&self) -> Option<Duration> {
        let scenario = self.scripted.clock.scenario();
        scenario
            .next_due()
            .map(|due| due.saturating_sub(scenario.now()))
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

    /// Whether the host owes a turn, as a runner's owner is woken for one: a
    /// close a widget asked for, a close guard that signalled since the last
    /// settle (a hold released, a decision made), or a quit to ask the
    /// windows, to commit, or that waits on a window that is gone.
    fn turn_owed(&self) -> bool {
        !self.close_asks.borrow().is_empty()
            || self
                .ui_runtime
                .as_ref()
                .is_some_and(UiRuntime::owes_close_settle)
            || self.quit_owed
            || self.quit.is_asked()
            || self
                .quit
                .waiting()
                .iter()
                .any(|waited| !self.is_open(HeadlessWindowId(waited.presentation_id)))
    }

    /// Whether a window changed since the host last told the UI runtime: the next pump
    /// delivers the change, so the host is not settled.
    fn observations_pending(&self) -> bool {
        self.presentation_ids().into_iter().any(|presentation| {
            self.window_of(HeadlessWindowId(presentation))
                .chrome
                .lock()
                .unobserved
        })
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
    /// pending work, no armed deadline, no change the scenario scheduled — at
    /// most `max_frames` of
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
            // A covered window owing a frame is asked before the host decides
            // it has nothing to do, as `time_to_next_frame` asks.
            self.ask_owed_frames();
            // A retired host owes only its background work and the scenario.
            // A runtime whose frames are off owes no frame, only its deadlines
            // and the timed requests of its windows.
            let runtime_idle = self.ui_runtime.as_ref().is_none_or(|runtime| {
                (!runtime.scheduler().frames_enabled()
                    || (!runtime.needs_redraw()
                        && !runtime.scheduler().is_frame_scheduled()
                        && !runtime.has_pending_work()))
                    && runtime.next_wake().is_none()
            });
            if runtime_idle
                && self.timed_frame_wait().is_none()
                && self.pending_background() == 0
                && !self.observations_pending()
                && !self.turn_owed()
                && self.scenario_wait().is_none()
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
        // A real host hears nothing more of a window that closed.
        if !self.is_open(HeadlessWindowId(presentation)) {
            return;
        }
        if let flui_runtime::owner::WindowObservation::Metrics(metrics) = observation {
            *self
                .window_of(HeadlessWindowId(presentation))
                .scale_factor
                .lock() = metrics.scale();
        }
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime().id(),
            presentation_id: presentation,
        };
        self.ui_runtime().enter(|ui_runtime| {
            ui_runtime.apply_window_observation(address, observation, &|_| {});
        });
        self.ask_owed_frames();
    }

    /// Ask each window for the frame its presentation owes its surface, or
    /// withdraw the instant asked before, as a runner's owner delivery does
    /// after each work item (ADR-0389). A retired host asks nothing.
    fn ask_owed_frames(&self) {
        let Some(runtime) = self.try_ui_runtime() else {
            return;
        };
        for request in runtime.owed_frame_requests() {
            request.deliver();
        }
    }

    /// How long until the earliest timed frame request of an open window the
    /// host has not served, if any.
    fn timed_frame_wait(&self) -> Option<Duration> {
        let now = self.event_now();
        self.open
            .iter()
            .filter_map(|open| open.window.pending_timed_frame())
            .min()
            .map(|at| at.saturating_duration_since(now))
    }

    /// Size the sink to the primary window's device size.
    fn size_sink_to_primary(&mut self) {
        let physical = self.primary.window.physical_size_i32();
        self.sink.size = (
            u32::try_from(physical.width).expect("BUG: a window's device size is positive"),
            u32::try_from(physical.height).expect("BUG: a window's device size is positive"),
        );
    }

    /// The presentation of the window `window` names.
    fn presentation_of(&self, window: WindowId) -> PresentationId {
        std::iter::once(&self.primary)
            .chain(&self.open)
            .find(|open| open.window.id == window)
            .map_or_else(
                || panic!("the recording names {window:?}, a window this host has not opened"),
                |open| open.id,
            )
    }

    /// The surface of `presentation`'s window a recording names.
    fn surface_of(
        &self,
        presentation: PresentationId,
        surface: flui_runtime::RecordedSurface,
    ) -> flui_interaction::ids::InputSurface {
        match surface {
            flui_runtime::RecordedSurface::Window => flui_interaction::ids::InputSurface::Window,
            flui_runtime::RecordedSurface::Embedded(raw) => self
                .embedded
                .iter()
                .copied()
                .find(|embedded| {
                    presentation == self.primary.id
                        && matches!(embedded, flui_interaction::ids::InputSurface::Embedded(id) if id.get() == raw)
                })
                .unwrap_or_else(|| {
                    panic!("the recording names embedded surface {raw}, which this host has not embedded")
                }),
        }
    }

    /// Advance the clock to `time` on the event timeline, if it is ahead.
    fn advance_to(&self, time: flui_platform::EventTime) {
        let now = self.event_now();
        self.advance(time.saturating_duration_since(now));
    }

    /// The clock's reading on the event timeline.
    fn event_now(&self) -> flui_platform::EventTime {
        flui_platform::EventTime::from_nanos(
            u64::try_from(self.clock.elapsed().as_nanos())
                .expect("BUG: a headless test's virtual time fits the event timeline"),
        )
    }

    /// Close presentation `id`, as a runner closes a window: a window another
    /// one outlives is closed and removed, and the OS focus moves to a
    /// survivor; the last window stops its presentation and retires the UI
    /// runtime with it, as a real host retires its UI runtime when its last
    /// window closes, so whatever the UI runtime still held to answer ends as
    /// [`OwnerRetired`](flui_platform::capability::OwnerRetired). `false`
    /// when the window is already closed.
    ///
    /// # Panics
    ///
    /// With the first panic of the code the close runs (observers,
    /// disposers, destructors), once the close is done.
    fn close_presentation(&mut self, id: PresentationId) -> bool {
        let Some(position) = self.open.iter().position(|window| window.id == id) else {
            return false;
        };
        let closed = self.open.remove(position);
        closed.window.closed.store(true, Ordering::Relaxed);
        if let Some(exports) = &closed.window.portal_exports {
            exports.release_all();
        }
        // A window that closes no longer holds the session end it refused,
        // however the close came; its handler is dropped outside the
        // router's table. A quit it held learns it is gone at the next turn.
        let address = self.address(id);
        self.session_end.release_refusal(address);
        drop(self.close_requests.take(address));
        if !self.open.is_empty() {
            return self.ui_runtime_mut().close_presentation_entered(id);
        }
        let runtime = self
            .ui_runtime
            .take()
            .expect("BUG: a host with an open window holds its UI runtime");
        // The presentation stops inside the runtime's scope, so its Detached
        // observers run as on a real host, and the runtime is then dropped,
        // which retires what it still owes; a panic in either is kept and
        // the other still runs (ADR-0281).
        let mut first = FirstPanic::new();
        first.contain(|| runtime.enter(UiRuntime::stop_presentations));
        first.contain(move || drop(runtime));
        first.resume();
        true
    }

    /// The presentations of the open windows, in opening order.
    fn presentation_ids(&self) -> Vec<PresentationId> {
        self.open.iter().map(|window| window.id).collect()
    }

    /// The presentation at `position` of [`presentation_ids`](Self::presentation_ids)'s order.
    fn presentation_at(&self, position: usize) -> Option<PresentationId> {
        self.open.get(position).map(|window| window.id)
    }

    /// The window `window` names: the primary one, open or closed, or one
    /// the host [opened](Self::open_window) and has not closed.
    ///
    /// # Panics
    /// When `window` names a secondary window that closed.
    #[must_use]
    pub fn window_of(&self, window: HeadlessWindowId) -> &HeadlessWindow {
        &self.host_window(window).window
    }

    fn accessibility_of(&self, window: HeadlessWindowId) -> &HeadlessAccessibility {
        &self.host_window(window).accessibility
    }

    /// The primary window, open or closed, or an open one.
    fn host_window(&self, window: HeadlessWindowId) -> &HostWindow {
        if window == self.primary_window() {
            return &self.primary;
        }
        self.open_window_of(window)
    }

    /// The open window `window` names.
    fn open_window_of(&self, window: HeadlessWindowId) -> &HostWindow {
        self.open
            .iter()
            .find(|open| open.id == window.0)
            .expect("a HeadlessWindowId names a window this host opened and has not closed")
    }
}

/// The UI runtime still had work after a bounded run of frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the UI runtime had not settled after {frames} frames")]
pub struct NotSettled {
    /// The frames pumped.
    pub frames: usize,
}

/// A window was opened that plays another window system than its host's:
/// the browser where the host's primary window plays a platform's, or the
/// other way round. A host has one window system, as a runner does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "a window playing {window:?} cannot open beside windows playing {host:?}: the window system \
     is the host's, set by its primary window"
)]
#[non_exhaustive]
pub struct WindowSystemMismatch {
    /// The window system the host's windows play.
    pub host: WindowSystem,
    /// The one the refused window plays.
    pub window: WindowSystem,
}

/// A window of a [`HeadlessHost`]: its primary one or one it
/// [opened](HeadlessHost::open_window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeadlessWindowId(PresentationId);

/// The closes the windows' widgets asked for since the host's last turn, in
/// order, which the turn carries out as a runner's window queue does.
type CloseAsks = Rc<std::cell::RefCell<Vec<PresentationId>>>;

/// The host's control of a [`HeadlessWindow`], behind the `WindowHandle` its
/// widgets reach through `LifecycleContext::window`, as a runner installs one
/// for each window it opens. The double honours its frame and mode commands
/// at once and answers [`Closed`](flui_platform::SubmitError::Closed) once its
/// window closed; a close it is asked for waits for the host's next turn.
struct HeadlessWindowControl {
    window: Rc<HeadlessWindow>,
    presentation: PresentationId,
    close_asks: CloseAsks,
}

impl flui_platform::WindowControl for HeadlessWindowControl {
    fn state(&self) -> flui_platform::WindowState {
        self.window.state()
    }

    fn submit(
        &self,
        command: flui_platform::WindowCommand,
    ) -> Result<(), flui_platform::SubmitError> {
        if self.window.is_closed() {
            return Err(flui_platform::SubmitError::Closed);
        }
        let admitted = command.admit(&self.window.capabilities())?;
        let command = admitted
            .against(&self.window.capabilities())
            .expect("BUG: the double's capabilities never move");
        self.window
            .take(command)
            .expect("BUG: the double carries out every command its capabilities honour");
        Ok(())
    }

    /// Counted as the window's observation, and queued for the host's next
    /// turn, which asks the window's guard and closes it (a window already
    /// gone has nothing left to close), as a runner's window queue does.
    fn request_close(&self) {
        self.window.chrome.lock().close_requests += 1;
        if !self.window.is_closed() {
            self.close_asks.borrow_mut().push(self.presentation);
        }
    }
}

/// Attach the host's control of `window` to `presentation`, before its root
/// widget is attached.
fn install_window_control(
    ui_runtime: &UiRuntime,
    presentation: PresentationId,
    window: &Rc<HeadlessWindow>,
    close_asks: &CloseAsks,
) {
    let source = flui_view::__runtime::WindowSource::new(HeadlessWindowControl {
        window: Rc::clone(window),
        presentation,
        close_asks: Rc::clone(close_asks),
    });
    assert!(
        ui_runtime.install_window(presentation, source),
        "BUG: the window's presentation was just installed"
    );
}

/// Register `presentation`, drawn in `window`, with `router`: its close guard
/// and, where its platform asks before a window closes (`close_vetoes` holds
/// the user's close), the application's `handler`, as a runner registers
/// each window it opens.
fn register_close_requests(
    router: &CloseRequestRouter,
    close_vetoes: &[CloseReason],
    ui_runtime: &UiRuntime,
    presentation: PresentationId,
    window: &Rc<HeadlessWindow>,
    handler: Option<CloseRequestHandler>,
) {
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: presentation,
    };
    // A phone's window system never asks, so a runner there registers no
    // handler: nothing would ever ask it.
    let handler = handler.filter(|_| close_vetoes.contains(&CloseReason::User));
    let native = Rc::clone(window) as Rc<dyn PlatformWindow>;
    let mut prepared = PreparedCloseRequest::new(address, &native, handler);
    if let Some(guard) = ui_runtime.close_guard_of(presentation) {
        prepared = prepared.guard(guard);
    }
    let publication = router
        .publication(&prepared)
        .expect("BUG: a presentation the host just installed has no registration");
    publication.publish(prepared);
}

/// How `windows` let the session end, as their host reports it: `Background`
/// on Android and iOS, `Delay` on Linux (logind), `Hold` on Windows, macOS
/// and the platform-neutral window, as the native headless platform does, and
/// `NotifyOnly` in a browser. A duration names the session end's budget.
fn session_end_support_of(windows: WindowSystem) -> flui_platform::SessionEndSupport {
    use flui_platform::{SessionEndSupport, TargetPlatform};
    match windows {
        WindowSystem::Platform(TargetPlatform::Android | TargetPlatform::iOS) => {
            SessionEndSupport::Background {
                window: SESSION_END_BUDGET,
            }
        }
        WindowSystem::Platform(TargetPlatform::Linux) => SessionEndSupport::Delay {
            max: SESSION_END_BUDGET,
        },
        WindowSystem::Browser => SessionEndSupport::NotifyOnly,
        _ => SessionEndSupport::Hold,
    }
}

/// A window of a [`HeadlessHost`]: its presentation and what the host plays
/// for it.
#[derive(Clone)]
struct HostWindow {
    id: PresentationId,
    window: Rc<HeadlessWindow>,
    accessibility: Arc<HeadlessAccessibility>,
    text_store_host: Option<Rc<RecordingTextStoreHost>>,
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
