//! Android window implementation
//!
//! Wraps `AndroidApp` to provide the `PlatformWindow` trait, delegating
//! to the native ANativeWindow for size queries and raw-window-handle for GPU
//! surface creation.

use std::rc::Rc;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use android_activity::AndroidApp;
use android_activity::ndk::configuration::UiModeNight;
use cursor_icon::CursorIcon;
use flui_foundation::geometry::{EdgeInsets, Point, Size};
use flui_platform::input_regions::{InputRegion, InputRegionKind, ProjectedInputRegions};
use flui_platform::{
    DisplayId, FrameRateHint, WindowAppearance, WindowCapabilities, WindowCommand,
    WindowCommandError, WindowEvent, WindowGeometry, WindowOptions, WindowState,
};
use parking_lot::Mutex;

use crate::host;
use crate::owner::android::AndroidLoop;
use crate::owner::android::java_view::JavaView;
use crate::{
    owner::OpenWindowError, owner::handlers::WindowCallbacks, owner::slot::OwnerSlot,
    shared::input_availability::AndroidInputApis, shared::os_version::android::AndroidApi,
    shared::os_version::android::DragSource, shared::os_version::android::GestureExclusion,
    shared::os_version::android::TouchpadGestures, shared::window_state::WindowStateCell,
};
use flui_platform::{CursorError, PlatformWindow, WindowExecutionState, WindowId};

/// Android window wrapping the native ANativeWindow via `AndroidApp`
///
/// On Android there is only one window (the Activity surface). This struct
/// provides the `PlatformWindow` interface over that surface.
///
/// # Raw Window Handle
///
/// `AndroidApp` implements `HasWindowHandle` and `HasDisplayHandle`, so this
/// window can be used directly with wgpu for Vulkan surface creation.
#[derive(Clone)]
pub struct AndroidWindow {
    app: AndroidApp,
    callbacks: Arc<WindowCallbacks>,
    redraw_requested: Arc<AtomicBool>,
    activity: Arc<ActivityFlags>,
    owner: std::thread::ThreadId,
    owner_signal: Weak<crate::owner::signal::OwnerSignal>,
    geometry_live: Arc<AtomicBool>,
    text_sizing: Arc<crate::text::android::sizing::Acquisition>,
    safe_area: Arc<Mutex<super::safe_area::Capture>>,
    /// The exclusion rectangles last handed to the decor view, as projected.
    input_regions: Arc<Mutex<ProjectedInputRegions>>,
    /// The decor view and vibrator haptics play on (ADR-0210).
    haptics: Arc<super::haptics::AndroidHaptics>,
    /// `FluiView`: the input method's editor, the drop target and the drag
    /// source (ADR-0247).
    java_view: Arc<JavaView>,
    /// The window's text input, on the owner thread only.
    text_input: Arc<OwnerSlot<Rc<crate::text::android::AndroidTextInput>>>,
    drag_source: Arc<super::drag::AndroidDragSource>,
    system_chrome: Arc<super::system_chrome::AndroidSystemChrome>,
    /// The platform's API level, for the safe-area gates.
    api: AndroidApi,
    /// The night mode last reported to `on_appearance_changed`, which
    /// `appearance` returns.
    appearance: Arc<Mutex<WindowAppearance>>,
    displays: Arc<super::display::AndroidDisplays>,
    /// The last refresh-rate request, applied again to every native window that
    /// replaces the one it was made on.
    frame_rate_hint: Arc<Mutex<FrameRateHint>>,
    /// The published window state (ADR-0237).
    snapshot: Arc<WindowStateCell>,
}

impl std::fmt::Debug for AndroidWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidWindow")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

impl AndroidWindow {
    /// Create a new Android window wrapping the given `AndroidApp`
    pub(super) fn new(
        app: AndroidApp,
        activity: Arc<ActivityFlags>,
        owner_signal: Weak<crate::owner::signal::OwnerSignal>,
        java_view: Arc<JavaView>,
        displays: Arc<super::display::AndroidDisplays>,
        api: AndroidApi,
    ) -> Self {
        let callbacks = Arc::new(WindowCallbacks::new());
        callbacks.set_input_capabilities(input_capabilities(api, java_view.is_ready()));
        let text_input = Arc::new(OwnerSlot::new());
        if text_input
            .replace(crate::text::android::AndroidTextInput::new(
                Arc::clone(&java_view),
                api,
            ))
            .is_err()
        {
            tracing::warn!(
                target: "flui_native::android",
                "the window's text input was made off its owner thread"
            );
        }
        let drag_source = Arc::new(super::drag::AndroidDragSource::new(
            Arc::clone(&java_view),
            api,
        ));
        if let Some(signal) = owner_signal.upgrade() {
            callbacks.set_input_wake(crate::owner::signal::OwnerSignal::callbacks_wake(
                &signal, &callbacks,
            ));
        }
        let system_chrome = Arc::new(super::system_chrome::AndroidSystemChrome::new(
            app.clone(),
            api,
        ));
        let snapshot = Arc::new(WindowStateCell::new(sample_state(
            &app, &displays, &activity,
        )));
        Self {
            haptics: Arc::new(super::haptics::AndroidHaptics::new(app.clone(), api)),
            appearance: Arc::new(Mutex::new(night_mode(&app))),
            app,
            callbacks,
            redraw_requested: Arc::new(AtomicBool::new(true)),
            activity,
            owner: std::thread::current().id(),
            owner_signal,
            geometry_live: Arc::new(AtomicBool::new(true)),
            text_sizing: crate::text::android::sizing::Acquisition::new(),
            safe_area: Arc::new(Mutex::new(super::safe_area::Capture::default())),
            input_regions: Arc::default(),
            java_view,
            text_input,
            drag_source,
            system_chrome,
            displays,
            frame_rate_hint: Arc::new(Mutex::new(FrameRateHint::Default)),
            snapshot,
            api,
        }
    }

    /// Send the system the refresh-rate request this window last made, to the
    /// native window it has now. A request rides on one `ANativeWindow`, so a
    /// replacement starts without it.
    pub(crate) fn apply_frame_rate_hint(&self) {
        let hint = *self.frame_rate_hint.lock();
        if let Some(native) = self.app.native_window() {
            self.displays.request_frame_rate(&native, hint);
        }
    }

    /// Report a night-mode change from the configuration that just changed.
    /// A failed read keeps the night mode last reported.
    pub(crate) fn settle_appearance(&self) {
        let appearance = match super::preferences::night_mode(&self.app) {
            Ok(night) => appearance_of(night),
            Err(error) => {
                tracing::warn!(%error, "Android night-mode query failed");
                return;
            }
        };
        if std::mem::replace(&mut *self.appearance.lock(), appearance) != appearance {
            self.callbacks.dispatch_appearance_changed();
        }
    }

    /// The view's installation finished: what the window can do now.
    pub(crate) fn refresh_capabilities(&self) {
        self.callbacks
            .set_input_capabilities(input_capabilities(self.api, self.java_view.is_ready()));
    }

    /// The window's text input; `None` off the owner thread.
    pub(crate) fn text_input(&self) -> Option<Rc<crate::text::android::AndroidTextInput>> {
        let input = self.text_input.take()?;
        let shared = Rc::clone(&input);
        let _ = self.text_input.restore(input);
        Some(shared)
    }

    pub(crate) fn drag_source_of(&self) -> &Arc<super::drag::AndroidDragSource> {
        &self.drag_source
    }

    /// The window is going away: its field and its drag are detached.
    pub(crate) fn retire_java_view(&self) {
        if let Some(input) = self.text_input() {
            input.retire();
        }
        self.drag_source.retire();
    }

    /// Access the callback storage (used by `AndroidLoop` to dispatch
    /// events)
    pub fn callbacks(&self) -> &WindowCallbacks {
        &self.callbacks
    }

    /// Replacing the Activity presentation retires this projection capability,
    /// even if an old handle still keeps the shared AndroidApp alive.
    pub(crate) fn revoke_geometry(&self) {
        self.geometry_live.store(false, Ordering::Release);
        self.text_sizing.invalidate();
    }

    pub(crate) fn invalidate_text_sizing(&self) {
        self.text_sizing.invalidate();
        if let Some(signal) = self.owner_signal.upgrade() {
            let _ = signal.wake();
        }
    }

    pub(crate) fn fence_text_sizing(&self) {
        self.text_sizing.fence();
    }

    pub(crate) fn settle_safe_area(&self) {
        let (sample, requery) = {
            let mut capture = self.safe_area.lock();
            let super::safe_area::Query::Ready(sample) = capture.query else {
                return;
            };
            capture.query = super::safe_area::Query::Idle;
            (sample, std::mem::take(&mut capture.requery))
        };
        self.accept_safe_area(sample);
        if requery {
            // Asked directly, without arming the stale retry again, so this
            // asks once more and no more.
            self.query_safe_area();
        }
    }

    fn accept_safe_area(&self, sample: Option<super::safe_area::Sample>) {
        let size = self.physical_size();
        let scale = self.scale_factor();
        let changed = {
            let mut capture = self.safe_area.lock();
            let Some(sample) = sample else {
                return;
            };
            if !sample.is_for(size) {
                // Asked before a resize and answered after it. Asked again
                // once: a decor view that never reaches the window's size
                // must not keep the Java thread busy.
                if std::mem::take(&mut capture.retry_stale) {
                    drop(capture);
                    self.query_safe_area();
                }
                return;
            }
            let Some(insets) = sample.logical(size, scale) else {
                return;
            };
            if !self.geometry_live.load(Ordering::Acquire) || insets == capture.accepted {
                return;
            }
            capture.accepted = insets;
            insets
        };
        self.callbacks.dispatch_safe_area_change(changed);
        self.request_redraw();
    }

    /// Ask the Java thread for the insets. A reply taken at a size the window
    /// no longer has is asked for once more; a request while a reply is out is
    /// asked again once that reply settles.
    pub(crate) fn request_safe_area(&self) {
        {
            let mut capture = self.safe_area.lock();
            capture.retry_stale = true;
            if !matches!(capture.query, super::safe_area::Query::Idle) {
                capture.requery = true;
                return;
            }
        }
        self.query_safe_area();
    }

    fn query_safe_area(&self) {
        {
            let mut capture = self.safe_area.lock();
            if !matches!(capture.query, super::safe_area::Query::Idle)
                || !self.geometry_live.load(Ordering::Acquire)
            {
                return;
            }
            capture.query = super::safe_area::Query::Waiting;
        }
        let app = self.app.clone();
        let api = self.api;
        let capture = Arc::downgrade(&self.safe_area);
        let live = Arc::downgrade(&self.geometry_live);
        let signal = self.owner_signal.clone();
        self.app.run_on_java_main_thread(Box::new(move || {
            let sample = super::safe_area::sample(&app, api);
            if !live
                .upgrade()
                .is_some_and(|live| live.load(Ordering::Acquire))
            {
                return;
            }
            if let Some(capture) = capture.upgrade() {
                capture.lock().query = super::safe_area::Query::Ready(sample);
            }
            if let Some(signal) = signal.upgrade() {
                let _ = signal.wake();
            }
        }));
    }

    /// Read whether a redraw is pending without consuming it. The event loop
    /// uses this only to choose its poll timeout; native lifecycle events may
    /// still suspend execution before delivery becomes possible.
    pub(crate) fn has_redraw_request(&self) -> bool {
        self.redraw_requested.load(Ordering::SeqCst)
    }

    /// Consume a redraw only when the same loop turn can deliver its frame.
    pub(crate) fn take_deliverable_redraw_request(&self) -> bool {
        take_deliverable_redraw_request(
            &self.redraw_requested,
            self.activity.resumed.load(Ordering::SeqCst),
        )
    }

    /// Get native window dimensions, returning (0, 0) if window is not
    /// available
    fn native_size(&self) -> (i32, i32) {
        native_extent(&self.app)
    }

    /// The window's published state, read again first when something
    /// invalidated it; `on_state_changed` is raised when that produced a new
    /// revision.
    fn current_state(&self) -> WindowState {
        let published = self
            .snapshot
            .state_with(|| sample_state(&self.app, &self.displays, &self.activity));
        if published.changed {
            self.callbacks.dispatch_state_changed();
        }
        published.state
    }

    /// The activity surface may differ from its published state: read it again.
    pub(crate) fn publish_state(&self) {
        self.snapshot.invalidate();
        let _ = self.current_state();
    }
}

/// What the window's input can do at `api`, with `FluiView` installed or not.
fn input_capabilities(
    api: AndroidApi,
    java_view: bool,
) -> flui_platform::input_capabilities::InputCapabilities {
    crate::shared::input_availability::android(AndroidInputApis {
        gesture_exclusion: api.since::<GestureExclusion>().is_some(),
        touchpad_gestures: api.since::<TouchpadGestures>().is_some(),
        drag_source: java_view && api.since::<DragSource>().is_some(),
    })
}

/// The native window's size in pixels, `(0, 0)` while there is none.
fn native_extent(app: &AndroidApp) -> (i32, i32) {
    if let Some(native_window) = app.native_window() {
        (native_window.width(), native_window.height())
    } else {
        (0, 0)
    }
}

/// Device pixels per logical pixel, from the configuration's density.
fn native_scale(app: &AndroidApp) -> f64 {
    if let Ok(ratio) = super::preferences::pixel_ratio(app) {
        return ratio.get();
    }
    // android-activity config returns density as DPI / 160
    // Default to 2.0 if config is unavailable
    let config = app.config();
    let density = config.density().unwrap_or(320);
    density as f64 / 160.0
}

/// The Activity's lifecycle as its callbacks reported it, shared by the
/// platform's event loop and the window it opened.
///
/// Three independent facts (see `shared::android_lifecycle`): the Activity is on
/// screen (`onStart` to `onStop`), it may run (`onResume` to `onPause`), and its
/// window has input focus. None implies another: a dialog takes focus from a
/// running, visible Activity. All three are false until their first callback.
#[derive(Debug, Default)]
pub(crate) struct ActivityFlags {
    pub(crate) visible: AtomicBool,
    pub(crate) resumed: AtomicBool,
    pub(crate) focused: AtomicBool,
}

/// The activity surface as the system reports it now. The activity fills its
/// display with no frame, title or position of its own.
fn sample_state(
    app: &AndroidApp,
    displays: &super::display::AndroidDisplays,
    activity: &ActivityFlags,
) -> WindowState {
    let (width, height) = native_extent(app);
    let scale = native_scale(app);
    let size = if scale > 0.0 {
        Size::new(f64::from(width) / scale, f64::from(height) / scale)
    } else {
        Size::new(f64::from(width), f64::from(height))
    };
    WindowState::new(WindowGeometry::new(None, size), size)
        .with_visible(activity.visible.load(Ordering::SeqCst))
        .with_focused(activity.focused.load(Ordering::SeqCst))
        .with_resizable(false)
        .with_decorated(false)
        .with_display(displays.activity_display())
}

fn take_deliverable_redraw_request(redraw_requested: &AtomicBool, execution_running: bool) -> bool {
    execution_running && redraw_requested.swap(false, Ordering::SeqCst)
}

impl crate::host_window::CoreHostWindow for AndroidWindow {}

impl crate::host_window::HostWindow for AndroidWindow {
    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        if self.owner != std::thread::current().id() {
            return Err(crate::TextSizingCaptureError::WrongThread);
        }
        if !self.geometry_live.load(Ordering::Acquire)
            || self.app.native_window().is_none()
            || !self
                .owner_signal
                .upgrade()
                .is_some_and(|signal| signal.accepting())
        {
            return Err(crate::TextSizingCaptureError::Unavailable);
        }
        self.text_sizing
            .poll(&self.app, &self.owner_signal, self.owner)
    }

    /// The window's input method host (ADR-0090, ADR-0247): `FluiView`'s
    /// `InputConnection` serves the focused field's store.
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform::text_store::TextStoreHost>> {
        self.text_input()
            .map(|input| input as Rc<dyn flui_platform::text_store::TextStoreHost>)
    }
}

impl host::Window<AndroidLoop> for AndroidWindow {
    fn open(
        owner: &AndroidLoop,
        options: WindowOptions,
    ) -> Result<Arc<dyn crate::host_window::HostWindow>, OpenWindowError> {
        if options.input_latency == flui_platform::InputLatency::Low {
            // NativeActivity takes touch from the native input queue, which
            // `requestUnbufferedDispatch` does not reach (docs/gaps.md).
            tracing::warn!(
                "InputLatency::Low is not supported on Android yet; input stays batched"
            );
        }
        let cancelled = owner
            .input_state
            .lock()
            .cancel_contacts(crate::shared::pointer_assembly::NativeCancel::CaptureLost);
        let window = Arc::new(AndroidWindow::new(
            owner.app.clone(),
            Arc::clone(&owner.activity),
            Arc::downgrade(&owner.owner_signal),
            Arc::clone(&owner.java_view),
            Arc::clone(&owner.displays),
            owner.api,
        ));
        let previous = owner.window.lock().replace(Arc::clone(&window));
        if let Some(previous) = previous {
            previous.retire_java_view();
            previous.revoke_geometry();
            for event in cancelled {
                previous.callbacks().dispatch_input(event);
            }
        }
        tracing::info!("Android window created (wrapping ANativeWindow)");
        Ok(window)
    }

    fn active(owner: &AndroidLoop) -> Option<WindowId> {
        owner.window.lock().as_ref().map(|_| WindowId(0))
    }

    fn capabilities(_: &AndroidLoop) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn frame_inset(_: &AndroidLoop, _: f64) -> Size<f64> {
        // The Activity fills its display with no frame, title or border.
        Size::new(0.0, 0.0)
    }

    fn on_event(owner: &AndroidLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        crate::owner::call_point::install(
            &owner.handler(|handlers| &mut handlers.window_event),
            callback,
        );
    }
}

/// The light/dark half of the activity's configuration: night mode is the
/// system dark theme (or the app's `AppCompatDelegate` override of it). The
/// native configuration stands in when the Java read fails; it is current only
/// until the first change the Activity handles in place.
fn night_mode(app: &AndroidApp) -> WindowAppearance {
    appearance_of(
        super::preferences::night_mode(app)
            .unwrap_or_else(|_| app.config().ui_mode_night() == UiModeNight::Yes),
    )
}

fn appearance_of(night: bool) -> WindowAppearance {
    if night {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }
}

impl PlatformWindow for AndroidWindow {
    // Android hosts exactly one `AndroidApp` surface for the process's
    // lifetime (see the struct docs above) — there is no second native
    // window this identity could ever collide with, so a fixed constant is
    // this backend's honest identity, not a stand-in for missing
    // information.
    fn id(&self) -> WindowId {
        WindowId(1)
    }

    fn physical_size(&self) -> Size<i32> {
        let (w, h) = self.native_size();
        Size::new(w, h)
    }

    fn logical_size(&self) -> Size<f64> {
        let (w, h) = self.native_size();
        let scale = self.scale_factor();
        if scale > 0.0 {
            Size::new(w as f64 / scale, h as f64 / scale)
        } else {
            Size::new(w as f64, h as f64)
        }
    }

    fn appearance(&self) -> WindowAppearance {
        *self.appearance.lock()
    }

    fn display_id(&self) -> Option<DisplayId> {
        self.displays.activity_display()
    }

    fn set_frame_rate_hint(&self, hint: FrameRateHint) {
        *self.frame_rate_hint.lock() = hint;
        self.apply_frame_rate_hint();
    }

    fn scale_factor(&self) -> f64 {
        native_scale(&self.app)
    }

    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        if self.owner != std::thread::current().id() {
            return Err(flui_platform::PreferenceQueryError::WrongThread);
        }
        if !self.geometry_live.load(Ordering::Acquire)
            || self.app.native_window().is_none()
            || !self
                .owner_signal
                .upgrade()
                .is_some_and(|signal| signal.accepting())
        {
            return Err(flui_platform::PreferenceQueryError::Unavailable);
        }
        super::preferences::geometry(&self.app).map(Some)
    }

    fn execution_state(&self) -> WindowExecutionState {
        if self.activity.resumed.load(Ordering::SeqCst) {
            WindowExecutionState::Running
        } else {
            WindowExecutionState::Suspended
        }
    }

    fn safe_area_insets(&self) -> EdgeInsets {
        self.safe_area.lock().accepted
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.system_chrome) as Arc<dyn flui_platform::PlatformSystemChrome>)
    }

    /// Callable from any thread: the first request since the last frame wakes
    /// the looper, which otherwise blocks until something else does.
    fn request_redraw(&self) {
        if !self.redraw_requested.swap(true, Ordering::SeqCst) {
            self.app.create_waker().wake();
        }
    }

    fn is_focused(&self) -> bool {
        self.activity.focused.load(Ordering::SeqCst)
    }

    fn is_visible(&self) -> bool {
        self.activity.visible.load(Ordering::SeqCst)
    }

    fn capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        // The system owns the surface: every command is refused unless it asks
        // for what the window already is.
        command.admit(&WindowCapabilities::NONE)?;
        Ok(())
    }

    fn state(&self) -> WindowState {
        self.current_state()
    }

    fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
        Err(CursorError::Unsupported)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.haptics) as Arc<dyn flui_platform::PlatformHaptics>)
    }

    /// `FluiView.startDragAndDrop`, once the view is installed, from API 24:
    /// what `InputCapabilities::outgoing_drag` claims.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        (self.java_view.is_ready() && self.api.since::<DragSource>().is_some()).then(|| {
            Arc::clone(&self.drag_source) as Arc<dyn flui_platform::drag::PlatformDragSource>
        })
    }

    /// Projects the regions to the surface's pixels and, when the projection changed,
    /// hands the exclusion rectangles to the decor view on the Java main thread. The
    /// native surface fills the decor view, so both share one pixel space. Before API 29
    /// the OS has no such call and the capability reports `Unavailable`.
    fn set_input_regions(&self, regions: &[InputRegion]) {
        if self.api.since::<GestureExclusion>().is_none() {
            return;
        }
        let scale = self.scale_factor();
        let rects: Vec<[i32; 4]> = {
            let mut projection = self.input_regions.lock();
            let Some(sent) = projection.update(regions, scale, None) else {
                return;
            };
            sent.iter()
                .filter(|region| region.kind() == InputRegionKind::SystemGestureExclusion)
                .map(|region| {
                    let rect = region.rect();
                    [rect.left(), rect.top(), rect.right(), rect.bottom()]
                })
                .collect()
        };
        let app = self.app.clone();
        // Posted in call order to the main looper, so the last projection is the one the
        // view keeps.
        self.app.run_on_java_main_thread(Box::new(move || {
            if let Err(error) = super::gesture_exclusion::apply(&app, &rects) {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "system gesture exclusion rectangles were not applied"
                );
            }
        }));
    }

    // ==================== Callback Registration ====================

    crate::owner::handlers::impl_window_callback_setters!(callbacks);

    // ==================== Window Handles (GPU integration) ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // Get the ANativeWindow pointer from the AndroidApp and construct the handle
        // manually. We can't delegate to NativeWindow::window_handle() because
        // that borrows a temporary.
        //
        // This already satisfies `PlatformWindow::window_handle`'s MUST
        // (see that trait method's doc): `native_window()` answers `None`
        // exactly when no ANativeWindow is live, and the `ok_or` below maps
        // that straight to `Unavailable` — no separate flag needed. The span
        // where it is `None` runs from `MainEvent::TerminateWindow` to the
        // next `MainEvent::InitWindow` (`owner/android/looper.rs`'s event
        // loop, module doc's "Surface Lifecycle" section), and that span is
        // open at both ends *inside* those callbacks: the field is still
        // `Some` while the `TerminateWindow` callback runs (the applier clears
        // it in `post_exec_cmd` once the callback has returned) and already
        // `Some` while the `InitWindow` callback runs (it is set in
        // `pre_exec_cmd` before the callback). Note what that span is
        // NOT: an ordinary `MainEvent::Pause` leaves `native_window()`
        // `Some`, because `AppCmd::TermWindow` is what clears it and a pause
        // does not apply that command.
        let native_window = self
            .app
            .native_window()
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        // NativeWindow::ptr() returns NonNull<ANativeWindow>; cast to NonNull<c_void>
        // for rwh
        let ptr = native_window.ptr().cast();
        let handle = raw_window_handle::AndroidNdkWindowHandle::new(ptr);
        let raw = raw_window_handle::RawWindowHandle::AndroidNdk(handle);
        // SAFETY: `ptr` is the ANativeWindow `AndroidApp` currently holds,
        // re-queried immediately above. The bound on that pointer is the
        // ANativeWindow refcount, and it is NOT this borrow. `ndk::NativeWindow`
        // is a refcounted wrapper (`Clone` calls `ANativeWindow_acquire`,
        // `Drop` calls `ANativeWindow_release`); `AndroidApp::native_window()`
        // returns a clone of the glue guard's field
        // (`android-activity` 0.6.1, `native_activity/mod.rs`); and the guard
        // holds the last strong clone the Rust side keeps, dropping it in
        // `post_exec_cmd(AppCmd::TermWindow)` — `guard.window = None`,
        // `native_activity/glue.rs` — which the main loop applies *after* the
        // `MainEvent::TerminateWindow` callback returns (`pre_exec_cmd` →
        // callback → `post_exec_cmd`). The `'_` on the returned handle is the
        // borrow of `self` and does not encode that bound: `AndroidLoop`
        // keeps holding its `AndroidWindow` across a pause, and the only
        // writes to that field are `Window::open` (this file), which fills it,
        // and `AndroidLoop::run`'s exit path (`owner/android/looper.rs`),
        // which takes it exactly once, after the last dispatch, so for every
        // handle this method ever returns, `self` and the borrow outlive the
        // pointer.
        //
        // The obligation that carries the weight is therefore
        // consumer-enforced, not type-enforced: nothing may dereference the
        // handle once the window has been terminated, and this type cannot
        // express that, because `native_window()` keeps answering `Some`
        // through the very callback that precedes the release. `flui-app`'s
        // Android runner discharges it by releasing the wgpu surface built
        // from this handle inside `on_surface_status_change(false)`, which
        // `MainEvent::Pause` and `MainEvent::TerminateWindow` both deliver —
        // the second one still inside the callback, before the release above.
        #[expect(unsafe_code)]
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // Android always uses the default Android display
        let handle = raw_window_handle::AndroidDisplayHandle::new();
        let raw = raw_window_handle::RawDisplayHandle::Android(handle);
        // SAFETY: The Android display handle is always valid while the app is running
        #[expect(unsafe_code)]
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }

    // ==================== Additional query methods ====================

    fn mouse_position(&self) -> Point<f64> {
        Point::default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
