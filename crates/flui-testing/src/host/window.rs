//! The window double a [`HeadlessHost`] presents into: [`HeadlessWindow`] and what it
//! records of the UI runtime's calls.

#[cfg(doc)]
use super::HeadlessHost;
#[cfg(doc)]
use super::WindowSystemMismatch;
#[cfg(doc)]
use crate::text_store_host::RecordingTextStoreHost;
#[cfg(doc)]
use flui_runtime::owner_turn::CloseRequestRouter;

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use crate::portal_exports::HeadlessPortalExports;

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Size};
use flui_platform::input::regions::{DeviceInputRegion, InputRegion};
use flui_platform::text::store::{SelectionUi, TextStoreStatus};
use flui_platform::transfer::drag::PlatformDragSource;
use flui_platform::{
    Cursor, CursorError, CursorIcon, DisplayId, PlatformTextInput, PlatformWindow, WindowId,
    WindowIdMint,
};

use flui_runtime::owner_turn::CloseRequestHandler;
use flui_runtime::window_system::WindowSystem;

use super::os_drag::HeadlessOsDrags;
use super::text_input::RecordingTextInput;

/// The window a [`HeadlessHost`] presents into: window 1 at scale factor 1 (a
/// window [opened](HeadlessHost::open_window) beside it takes the next
/// identity, and [`HeadlessHost::set_scale_factor`] moves it), focused and
/// visible, with the logical size the UI runtime was built at.
///
/// It records what the UI runtime asks of a window that a test asserts on: the
/// cursor a hovered region sets, and, when built with a text input, every
/// IME enable and cursor-area call.
pub struct HeadlessWindow {
    pub(super) id: WindowId,
    pub(super) size: flui_platform::WindowExtent,
    pub(super) scale_factor: Cell<DevicePixelRatio>,
    cursor: RefCell<Cursor>,
    text_input: Option<Arc<RecordingTextInput>>,
    /// The pull host to offer, by who it says draws the selection UI.
    pub(super) text_store_host: Option<SelectionUi>,
    pub(super) gesture_preset: flui_interaction::gesture::GestureProfiles,
    pub(super) platform: flui_platform::TargetPlatform,
    /// Whether the window plays a browser's page ([`HeadlessWindow::in_browser`]).
    pub(super) in_browser: bool,
    /// The application's assets its widgets read (ADR-0384 §5): none
    /// unless the test names them.
    pub(super) assets: flui_platform::assets::AssetBundle,
    pub(super) display: Cell<Option<DisplayId>>,
    /// Whether the platform lets the window run, as the test last reported it.
    pub(super) execution: Cell<flui_platform::WindowExecutionState>,
    /// Whether the window system keeps the window off the screen, as the test
    /// last reported it ([`HeadlessHost::set_window_occluded`]).
    pub(super) occluded: Cell<bool>,
    /// What the simulated window system does with the window's last frame
    /// while it is off the screen ([`HeadlessWindow::with_occlusion_reports`]).
    pub(super) occluded_surface: flui_platform::OccludedSurface,
    /// What covers the window, as the test last reported it.
    pub(super) insets: Cell<flui_platform::WindowInsets>,
    /// The revision of the state [`PlatformWindow::state`] reports: advanced by
    /// every change this double makes to it.
    revision: Cell<flui_platform::WindowRevision>,
    input_regions: RefCell<SentInputRegions>,
    haptics: Arc<flui_platform::testing::FakeHaptics>,
    os_drags: Arc<HeadlessOsDrags>,
    /// Whether the simulated platform reports when frames reached the screen
    /// ([`HeadlessWindow::with_presentation_reports`]).
    pub(super) reports_presentation: bool,
    /// The commands the window honours, published once for what the window plays
    /// ([`HeadlessWindow::published_capabilities`]): what its commands are
    /// admitted against and opened under.
    capabilities: flui_platform::WindowCapabilities,
    /// The frame and mode the window was set to and the close requests it received,
    /// through its `WindowHandle` or a command applied to it.
    pub(super) chrome: RefCell<KitChrome>,
    /// Whether the host closed the window: its commands answer `Closed`.
    pub(super) closed: Cell<bool>,
    /// How the window is named to a portal, when it plays an exported
    /// Wayland window ([`HeadlessWindow::with_portal_export`]).
    pub(super) portal_exports: Option<HeadlessPortalExports>,
    /// Whether the window system fails to make a cursor of an image
    /// ([`HeadlessWindow::failing_cursor_images`]).
    fails_cursor_images: bool,
    /// The application's answer to the window's close requests, taken by the
    /// host that opens the window
    /// ([`HeadlessWindow::with_close_request_handler`]).
    pub(super) close_request_handler: Option<CloseRequestHandler>,
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
pub(super) struct KitChrome {
    frame: flui_platform::WindowFrame,
    mode: flui_platform::WindowMode,
    pub(super) close_requests: usize,
    /// Whether the state changed since the host last told the UI runtime.
    pub(super) unobserved: bool,
    /// The instant the window was asked for a frame at and the host has not
    /// served ([`PlatformWindow::request_frame_at`]).
    timed_frame: Option<flui_platform::EventTime>,
    /// Every timed frame request the window took, oldest first.
    timed_frame_requests: Vec<Option<flui_platform::EventTime>>,
}

impl HeadlessWindow {
    /// The commands the window honours, as it published them.
    pub(super) fn capabilities(&self) -> flui_platform::WindowCapabilities {
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
    pub(super) fn take(
        &self,
        command: flui_platform::WindowCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        let mut chrome = self.chrome.borrow_mut();
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
    pub(super) fn advance_revision(&self) {
        self.revision.set(
            self.revision
                .get()
                .next()
                .expect("BUG: a test changes a window far fewer than u64::MAX times"),
        );
    }

    /// Whether the window changed since the host last told the UI runtime, forgotten.
    pub(super) fn take_unobserved(&self) -> bool {
        std::mem::take(&mut self.chrome.borrow_mut().unobserved)
    }

    /// How many times the window was asked to close through its
    /// `WindowHandle`, one closed window's asks included. The host's next
    /// turn carries an ask out ([`HeadlessHost::pump`]); this count only
    /// observes them.
    #[must_use]
    pub fn close_requests(&self) -> usize {
        self.chrome.borrow().close_requests
    }

    /// Every timed frame request the window took
    /// ([`PlatformWindow::request_frame_at`]), oldest first: an instant on
    /// the host's event timeline, or `None` for a withdrawal. The frame a
    /// covered window owes its surface is asked for this way (ADR-0389).
    #[must_use]
    pub fn timed_frame_requests(&self) -> Vec<Option<flui_platform::EventTime>> {
        self.chrome.borrow().timed_frame_requests.clone()
    }

    /// The instant of the window's timed frame request the host has not
    /// served yet.
    pub(super) fn pending_timed_frame(&self) -> Option<flui_platform::EventTime> {
        self.chrome.borrow().timed_frame
    }

    /// Serve the window's timed frame request if its instant is not later
    /// than `now`: it is asked once. Whether it was served.
    pub(super) fn serve_timed_frame(&self, now: flui_platform::EventTime) -> bool {
        let mut chrome = self.chrome.borrow_mut();
        chrome.timed_frame.take_if(|at| *at <= now).is_some()
    }

    /// Whether the host closed the window.
    pub(super) fn is_closed(&self) -> bool {
        self.closed.get()
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
            scale_factor: Cell::new(DevicePixelRatio::ONE),
            cursor: RefCell::new(Cursor::Icon(CursorIcon::Default)),
            text_input: None,
            text_store_host: None,
            gesture_preset: flui_interaction::gesture::GestureSettings::default().into(),
            platform: flui_platform::TargetPlatform::Unknown,
            in_browser: false,
            assets: flui_platform::assets::AssetBundle::new(Arc::new(
                flui_platform::testing::MemoryAssets::new(),
            )),
            display: Cell::new(None),
            execution: Cell::new(flui_platform::WindowExecutionState::Running),
            occluded: Cell::new(false),
            occluded_surface: flui_platform::OccludedSurface::Discarded,
            insets: Cell::new(flui_platform::WindowInsets::ZERO),
            revision: Cell::new(flui_platform::WindowRevision::ZERO),
            input_regions: RefCell::default(),
            haptics: Arc::new(flui_platform::testing::FakeHaptics::new()),
            os_drags: Arc::new(HeadlessOsDrags::default()),
            reports_presentation: false,
            capabilities: Self::published_capabilities(),
            chrome: RefCell::default(),
            closed: Cell::new(false),
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
    pub fn haptic_actuator(&self) -> &super::FakeHaptics {
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
    pub(super) fn window_system(&self) -> WindowSystem {
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
        self.insets.set(insets);
        self
    }

    /// Offer a pull-model text input instead: the UI runtime's presentation gets
    /// a [`RecordingTextStoreHost`] ([`HeadlessHost::text_store_host`]) and
    /// tells it which field's store takes input, as it tells the Win32 text
    /// services. It wins over [`Self::with_text_input`].
    #[must_use]
    pub fn with_text_store_host(mut self) -> Self {
        self.text_store_host = Some(SelectionUi::Field);
        self
    }

    /// [`Self::with_text_store_host`], with a host that answers `ui` for who
    /// draws the served field's selection UI, as iOS's host answers
    /// [`SelectionUi::Platform`].
    #[must_use]
    pub fn with_text_store_host_drawing(mut self, ui: SelectionUi) -> Self {
        self.text_store_host = Some(ui);
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
        self.cursor.borrow().clone()
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
        self.input_regions.borrow().updates.clone()
    }

    /// The logical size in device pixels at the current scale factor: a
    /// window moved to another monitor keeps its logical size.
    pub(super) fn physical_size_i32(&self) -> Size<i32> {
        let scale = self.scale_factor.get().get();
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
        let mut chrome = self.chrome.borrow_mut();
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
        cursor.clone_into(&mut self.cursor.borrow_mut());
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
        let scale = self.scale_factor.get();
        let display = self.display.get();
        self.input_regions
            .borrow_mut()
            .update(regions, scale, display);
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
            Some(exports) if !self.closed.get() => exports.export(),
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
        .with_display(self.display.get())
        .with_insets(self.insets.get())
        .with_execution(self.execution.get())
        .with_occluded(self.occluded.get())
        .with_metrics(
            flui_foundation::geometry::WindowMetrics::from_logical(
                extent.get(),
                self.scale_factor.get(),
            )
            .ok(),
        );
        state = state.with_capabilities(self.capabilities());
        let chrome = self.chrome.borrow();
        state.frame = chrome.frame;
        state.mode = chrome.mode;
        drop(chrome);
        state.revision = self.revision.get();
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
