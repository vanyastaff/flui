//! The Android window: [`AndroidWindow`] wraps `AndroidApp` to provide the
//! `PlatformWindow` trait, delegating to the native `ANativeWindow` for size
//! queries and raw-window-handle for GPU surface creation; beside it the
//! safe-area capture, the system-gesture exclusion and the system chrome
//! (the `Chrome` position, [`system_chrome::AndroidSystemChrome`]).
//!
//! The looper holds the window as its [`ActivityWindow`] and records the
//! Activity's lifecycle facts ([`ActivityFlags`]) the window reads.

use std::rc::Rc;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use android_activity::AndroidApp;
use android_activity::ndk::configuration::UiModeNight;
use cursor_icon::CursorIcon;
use flui_foundation::geometry::{EdgeInsets, Size};
use flui_platform::input_regions::{InputRegion, InputRegionKind};
use flui_platform::{
    AdmittedCommand, ApplyError, FrameRateHint, Unsupported, UnsupportedReason, WindowAppearance,
    WindowCapabilities, WindowGeometry, WindowOptions, WindowState,
};
use parking_lot::Mutex;

use crate::host::{self, WindowFacets, WindowPositions};
use crate::input::regions::ProjectedInputRegions;
use crate::owner::android::java_view::{ActivityDragSource, JavaView};
use crate::owner::android::{
    ActivityFlags, ActivityPreferences, ActivityRef, ActivityWindow, ActivityWindowId, AndroidLoop,
};
use crate::window::capabilities::{ActivityFacts, activity_capabilities, pointer_icon_absence};

use crate::text::WindowTextInput;
use crate::{
    input::availability::AndroidInputApis,
    owner::OpenWindowError,
    owner::window_delivery::WindowDelivery,
    version::gate::android::AndroidApi,
    version::gate::android::DragSource,
    version::gate::android::GestureExclusion,
    version::gate::android::TouchpadGestures,
    window::state::{ReportedFacts, WindowStateCell},
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
    /// The window's identity, by which features keep what they hold of it.
    id: ActivityWindowId,
    app: AndroidApp,
    delivery: Arc<WindowDelivery>,
    redraw_requested: Arc<AtomicBool>,
    activity: Arc<ActivityFlags>,
    owner: std::thread::ThreadId,
    owner_signal: Weak<crate::owner::signal::OwnerSignal>,
    geometry_live: Arc<AtomicBool>,
    safe_area: Arc<Mutex<safe_area::Capture>>,
    /// The exclusion rectangles last handed to the decor view, as projected.
    input_regions: Arc<Mutex<ProjectedInputRegions>>,
    /// What the table's positions made for the window: its actuator (the
    /// decor view and the vibrator, ADR-0210), its text input (the field the
    /// input method edits, and the capture of its text size), its bridge,
    /// and the system bars it styles.
    facets: WindowFacets,
    /// `FluiView`: the input method's editor, the drop target and the drag
    /// source (ADR-0247).
    java_view: Arc<JavaView>,
    /// The system drag source the drag transport made for the window.
    drag_source: Arc<dyn ActivityDragSource>,
    /// The preference source the loop keeps: the night mode, the density and
    /// the touch geometry the window reports, as it reads them.
    preferences: Arc<dyn ActivityPreferences>,
    /// The platform's API level, for the safe-area gates.
    api: AndroidApi,
    /// The night mode last reported, which the snapshot's appearance reads.
    appearance: Arc<Mutex<WindowAppearance>>,
    displays: Arc<dyn crate::owner::android::HostDisplays>,
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
    /// The Activity's window `window` names, with what `positions` make of the
    /// table's window-level positions for it.
    fn new(
        window: &ActivityRef,
        positions: &WindowPositions<ActivityRef>,
        activity: Arc<ActivityFlags>,
        displays: Arc<dyn crate::owner::android::HostDisplays>,
        preferences: Arc<dyn ActivityPreferences>,
        drag_source: Arc<dyn ActivityDragSource>,
        handler: Rc<dyn flui_platform::WindowHandler>,
    ) -> Self {
        let ActivityRef {
            id,
            app,
            view: java_view,
            api,
            owner,
            signal: owner_signal,
        } = window.clone();
        let delivery = Arc::new(WindowDelivery::new(
            handler,
            input_capabilities(api, java_view.is_ready()),
        ));
        // What another thread raises for the window waits for a looper turn.
        if let Some(signal) = owner_signal.upgrade() {
            delivery.resume_through_signal(&signal);
        }
        let appearance = night_mode(&app, preferences.as_ref());
        let snapshot = Arc::new(WindowStateCell::new(sample_state(
            &app,
            preferences.as_ref(),
            displays.as_ref(),
            api,
            &activity,
            &Kept {
                appearance,
                safe_area: EdgeInsets::ZERO,
                hovered: false,
            },
        )));
        Self {
            id,
            facets: positions.for_window(window),
            appearance: Arc::new(Mutex::new(appearance)),
            app,
            delivery,
            redraw_requested: Arc::new(AtomicBool::new(true)),
            activity,
            owner,
            owner_signal,
            geometry_live: Arc::new(AtomicBool::new(true)),
            safe_area: Arc::new(Mutex::new(safe_area::Capture::default())),
            input_regions: Arc::default(),
            java_view,
            drag_source,
            preferences,
            displays,
            frame_rate_hint: Arc::new(Mutex::new(FrameRateHint::Default)),
            snapshot,
            api,
        }
    }

    /// The native window's size in device pixels; zero while there is none.
    fn device_size(&self) -> Size<i32> {
        let (width, height) = self.native_size();
        Size::new(width, height)
    }

    fn accept_safe_area(&self, sample: Option<safe_area::Sample>) {
        let size = self.device_size();
        let scale = self.scale();
        {
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
        }
        ActivityWindow::publish_state(self);
        self.request_redraw();
    }

    fn query_safe_area(&self) {
        {
            let mut capture = self.safe_area.lock();
            if !matches!(capture.query, safe_area::Query::Idle)
                || !self.geometry_live.load(Ordering::Acquire)
            {
                return;
            }
            capture.query = safe_area::Query::Waiting;
        }
        let app = self.app.clone();
        let api = self.api;
        let capture = Arc::downgrade(&self.safe_area);
        let live = Arc::downgrade(&self.geometry_live);
        let signal = self.owner_signal.clone();
        self.app.run_on_java_main_thread(Box::new(move || {
            let sample = safe_area::sample(&app, api);
            if !live
                .upgrade()
                .is_some_and(|live| live.load(Ordering::Acquire))
            {
                return;
            }
            if let Some(capture) = capture.upgrade() {
                capture.lock().query = safe_area::Query::Ready(sample);
            }
            if let Some(signal) = signal.upgrade() {
                let _ = signal.wake();
            }
        }));
    }

    /// Get native window dimensions, returning (0, 0) if window is not
    /// available
    fn native_size(&self) -> (i32, i32) {
        native_extent(&self.app)
    }

    /// The window's published state, read again first when something
    /// invalidated it; `WindowHandler::state_changed` is raised when that produced a new
    /// revision.
    fn current_state(&self) -> WindowState {
        let published = self.snapshot.state_with(|| self.sample());
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }

    fn sample(&self) -> WindowState {
        sample_state(
            &self.app,
            self.preferences.as_ref(),
            self.displays.as_ref(),
            self.api,
            &self.activity,
            &Kept {
                appearance: *self.appearance.lock(),
                safe_area: self.safe_area.lock().accepted,
                hovered: self.delivery.hovered(),
            },
        )
    }

    /// Read the activity surface again for its state publisher, announcing
    /// nothing.
    fn read_state(&self) -> crate::owner::window_delivery::Publication {
        self.snapshot.invalidate();
        self.snapshot.state_with(|| self.sample()).outcome()
    }
}

impl ActivityWindow for AndroidWindow {
    fn activity_id(&self) -> ActivityWindowId {
        self.id
    }

    /// The activity surface may differ from its published state: read it again.
    fn publish_state(&self) {
        self.snapshot.invalidate();
        let _ = self.current_state();
    }

    fn scale(&self) -> f64 {
        native_scale(&self.app, self.preferences.as_ref())
    }

    /// The view's installation finished: what the window can do now.
    fn refresh_capabilities(&self) {
        self.delivery
            .set_input_capabilities(input_capabilities(self.api, self.java_view.is_ready()));
    }

    fn drag_ended(&self, result: bool) {
        self.drag_source.finish(result);
    }

    /// Ask the Java thread for the insets. A reply taken at a size the window
    /// no longer has is asked for once more; a request while a reply is out is
    /// asked again once that reply settles.
    fn request_safe_area(&self) {
        {
            let mut capture = self.safe_area.lock();
            capture.retry_stale = true;
            if !matches!(capture.query, safe_area::Query::Idle) {
                capture.requery = true;
                return;
            }
        }
        self.query_safe_area();
    }

    fn settle_safe_area(&self) {
        let (sample, requery) = {
            let mut capture = self.safe_area.lock();
            let safe_area::Query::Ready(sample) = capture.query else {
                return;
            };
            capture.query = safe_area::Query::Idle;
            (sample, std::mem::take(&mut capture.requery))
        };
        self.accept_safe_area(sample);
        if requery {
            // Asked directly, without arming the stale retry again, so this
            // asks once more and no more.
            self.query_safe_area();
        }
    }

    /// Report a night-mode change from the configuration that just changed,
    /// as the preference source reads it. A failed read keeps the night mode
    /// last reported.
    fn settle_appearance(&self) {
        let appearance = match self.preferences.night_mode() {
            Ok(night) => appearance_of(night),
            Err(error) => {
                tracing::warn!(%error, "Android night-mode query failed");
                return;
            }
        };
        if std::mem::replace(&mut *self.appearance.lock(), appearance) != appearance {
            ActivityWindow::publish_state(self);
        }
    }

    fn invalidate_text_sizing(&self) {
        self.facets.text.invalidate_sizing();
    }

    fn fence_text_sizing(&self) {
        self.facets.text.fence_sizing();
    }

    /// Replacing the Activity presentation retires this projection capability,
    /// even if an old handle still keeps the shared AndroidApp alive.
    fn revoke_geometry(&self) {
        self.geometry_live.store(false, Ordering::Release);
        self.facets.text.invalidate_sizing();
    }

    /// The window is going away: its field and its drag are detached.
    fn retire_java_view(&self) {
        self.facets.text.detach();
        self.facets.accessibility.detach();
        self.drag_source.retire();
    }

    /// Send the system the refresh-rate request this window last made, to the
    /// native window it has now. A request rides on one `ANativeWindow`, so a
    /// replacement starts without it.
    fn apply_frame_rate_hint(&self) {
        let hint = *self.frame_rate_hint.lock();
        if let Some(native) = self.app.native_window() {
            self.displays.request_frame_rate(&native, hint);
        }
    }

    /// Read whether a redraw is pending without consuming it. The event loop
    /// uses this only to choose its poll timeout; native lifecycle events may
    /// still suspend execution before delivery becomes possible.
    fn has_redraw_request(&self) -> bool {
        self.redraw_requested.load(Ordering::SeqCst)
    }

    /// Consume a redraw only when the same loop turn can deliver its frame.
    fn take_deliverable_redraw_request(&self) -> bool {
        take_deliverable_redraw_request(
            &self.redraw_requested,
            self.activity.resumed.load(Ordering::SeqCst),
        )
    }
}

/// What the window's input can do at `api`, with `FluiView` installed or not.
fn input_capabilities(
    api: AndroidApi,
    java_view: bool,
) -> flui_platform::input_capabilities::InputCapabilities {
    crate::input::availability::android(AndroidInputApis {
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

/// Device pixels per logical pixel: the density the preference source reads
/// from the Activity's display metrics, else the native configuration's.
fn native_scale(app: &AndroidApp, preferences: &dyn ActivityPreferences) -> f64 {
    if let Ok(ratio) = preferences.pixel_ratio() {
        return ratio.get();
    }
    // android-activity config returns density as DPI / 160
    // Default to 2.0 if config is unavailable
    let config = app.config();
    let density = config.density().unwrap_or(320);
    density as f64 / 160.0
}

/// What the window kept of facts the Activity reported earlier: the night
/// mode its configuration last said, the safe area last accepted and the
/// hover last reported.
struct Kept {
    appearance: WindowAppearance,
    safe_area: EdgeInsets,
    hovered: bool,
}

/// The activity surface as the system reports it now, with what the window
/// `kept`. The activity fills its display with no frame, title or position of
/// its own.
fn sample_state(
    app: &AndroidApp,
    preferences: &dyn ActivityPreferences,
    displays: &dyn crate::owner::android::HostDisplays,
    api: AndroidApi,
    activity: &ActivityFlags,
    kept: &Kept,
) -> WindowState {
    let (width, height) = native_extent(app);
    let scale = native_scale(app, preferences);
    let size = if scale > 0.0 {
        Size::new(f64::from(width) / scale, f64::from(height) / scale)
    } else {
        Size::new(f64::from(width), f64::from(height))
    };
    let focused = activity.focused.load(Ordering::SeqCst);
    let state = WindowState::new(WindowGeometry::new(None, size), size)
        .with_visible(activity.visible.load(Ordering::SeqCst))
        .with_focused(focused)
        .with_resizable(false)
        .with_decorated(false)
        .with_display(displays.activity_display());
    ReportedFacts {
        // No native window (terminated, or not yet made) is no drawable area.
        metrics: flui_foundation::geometry::WindowMetrics::new(Size::new(width, height), scale)
            .ok(),
        appearance: kept.appearance,
        safe_area: kept.safe_area,
        execution: execution_of(activity),
        hovered: kept.hovered,
        // An Activity has no activation apart from its window's focus.
        active: focused,
        capabilities: activity_capabilities(activity_facts(api, displays)),
    }
    .onto(state)
}

/// What the system said at `api` and at the displays' last sample that an
/// Activity's window capabilities rest on.
fn activity_facts(
    api: AndroidApi,
    displays: &dyn crate::owner::android::HostDisplays,
) -> ActivityFacts {
    ActivityFacts {
        api,
        frame_rate_hint: displays.frame_rate_hint(),
        cross_window_blur: displays.cross_window_blur(),
    }
}

/// Running while the Activity is resumed, independent of focus and of the
/// native window (ADR-0035).
fn execution_of(activity: &ActivityFlags) -> WindowExecutionState {
    if activity.resumed.load(Ordering::SeqCst) {
        WindowExecutionState::Running
    } else {
        WindowExecutionState::Suspended
    }
}

fn take_deliverable_redraw_request(redraw_requested: &AtomicBool, execution_running: bool) -> bool {
    execution_running && redraw_requested.swap(false, Ordering::SeqCst)
}

impl crate::host_window::CoreHostWindow for AndroidWindow {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl crate::host_window::HostWindow for AndroidWindow {
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

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
        self.facets.text.capture_sizing()
    }

    /// The window's input method host (ADR-0090, ADR-0247): `FluiView`'s
    /// `InputConnection` serves the focused field's store.
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform::text_store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::store_host)
    }
}

impl host::Window<AndroidLoop> for AndroidWindow {
    /// The Activity's one window is the loop's to track (its window slot);
    /// what each window shares is what it hands out of the window-level
    /// positions.
    type Shared = WindowPositions<ActivityRef>;
    /// A window by the Activity it shows and its identity.
    type Ref = ActivityRef;

    fn open(
        owner: &AndroidLoop,
        positions: &Arc<WindowPositions<ActivityRef>>,
        options: WindowOptions,
        handler: Rc<dyn flui_platform::WindowHandler>,
    ) -> Result<Arc<dyn crate::host_window::HostWindow>, OpenWindowError> {
        if options.input_latency == flui_platform::InputLatency::Low {
            // NativeActivity takes touch from the native input queue, which
            // `requestUnbufferedDispatch` does not reach (docs/gaps.md).
            tracing::warn!(
                "InputLatency::Low is not supported on Android yet; input stays batched"
            );
        }
        let id = owner.window_ids.next().ok_or(OpenWindowError::Backend {
            message: "the Activity's window identities are used up".to_owned(),
        })?;
        let cancelled = owner.input.capture_lost();
        let reference = ActivityRef {
            id,
            app: owner.app.clone(),
            view: Arc::clone(&owner.java_view),
            api: owner.api,
            owner: std::thread::current().id(),
            signal: Arc::downgrade(&owner.owner_signal),
        };
        let drag_source = owner
            .java_view
            .transfer()
            .drag_source(Arc::clone(&owner.java_view), owner.api);
        let window = Arc::new(AndroidWindow::new(
            &reference,
            positions,
            Arc::clone(&owner.activity),
            Arc::clone(&owner.displays),
            Arc::clone(&owner.preferences),
            drag_source,
            handler,
        ));
        window
            .delivery
            .publish_through(Arc::downgrade(&window), AndroidWindow::read_state);
        let previous = owner
            .window
            .lock()
            .replace(Arc::clone(&window) as Arc<dyn ActivityWindow>);
        if let Some(previous) = previous {
            previous.retire_java_view();
            previous.revoke_geometry();
            for event in cancelled {
                previous.delivery().dispatch_input(event);
            }
        }
        tracing::info!("Android window created (wrapping ANativeWindow)");
        Ok(window)
    }

    fn active(owner: &AndroidLoop, _: &WindowPositions<ActivityRef>) -> Option<WindowId> {
        owner.window.lock().as_ref().map(|_| WindowId(0))
    }

    fn capabilities(owner: &AndroidLoop, _: &WindowPositions<ActivityRef>) -> WindowCapabilities {
        activity_capabilities(activity_facts(owner.api, owner.displays.as_ref()))
    }

    fn frame_inset(_: &AndroidLoop, _: &WindowPositions<ActivityRef>, _: f64) -> Size<f64> {
        // The Activity fills its display with no frame, title or border.
        Size::new(0.0, 0.0)
    }
}

/// The light/dark half of the activity's configuration: night mode is the
/// system dark theme (or the app's `AppCompatDelegate` override of it), as the
/// preference source reads it. The native configuration stands in when the
/// Java read fails; it is current only until the first change the Activity
/// handles in place.
fn night_mode(app: &AndroidApp, preferences: &dyn ActivityPreferences) -> WindowAppearance {
    appearance_of(
        preferences
            .night_mode()
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

    /// Kept and sent with every native window the Activity gets, from API 30.
    fn set_frame_rate_hint(&self, hint: FrameRateHint) -> Result<(), Unsupported> {
        if !self.displays.frame_rate_hint().is_available() {
            return Err(Unsupported::new(UnsupportedReason::NotOnThisDevice));
        }
        *self.frame_rate_hint.lock() = hint;
        self.apply_frame_rate_hint();
        Ok(())
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
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::Unavailable,
            )
            .into());
        }
        self.preferences.gesture_geometry().map(Some)
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    /// Callable from any thread: the first request since the last frame wakes
    /// the looper, which otherwise blocks until something else does.
    fn request_redraw(&self) {
        if !self.redraw_requested.swap(true, Ordering::SeqCst) {
            self.app.create_waker().wake();
        }
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        // The system owns the surface and the Activity's lifetime: every
        // command is refused unless it asks for what the window already is.
        // An Activity cannot bring its task to the front itself: the user and
        // the system decide which task is shown.
        command.against(&self.snapshot.published_capabilities())?;
        Ok(())
    }

    fn state(&self) -> WindowState {
        self.current_state()
    }

    /// Not set: the reason [`pointer_icon_absence`] gives at this API level.
    fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
        Err(Unsupported::from(pointer_icon_absence(self.api)).into())
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
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
    fn set_input_regions(&self, regions: &[InputRegion]) -> Result<(), Unsupported> {
        if self.api.since::<GestureExclusion>().is_none() {
            return Err(Unsupported::new(UnsupportedReason::NotOnThisDevice));
        }
        let scale = self.scale();
        let rects: Vec<[i32; 4]> = {
            let mut projection = self.input_regions.lock();
            let Some(sent) = projection.update(regions, scale, None) else {
                return Ok(());
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
            if let Err(error) = gesture_exclusion::apply(&app, &rects) {
                tracing::warn!(
                    target: "flui_native::android",
                    %error,
                    "system gesture exclusion rectangles were not applied"
                );
            }
        }));
        Ok(())
    }

    // ==================== Callback Registration ====================

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
        // from this handle inside `WindowHandler::surface_changed(Release)`, which
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
}

mod safe_area {
    //! Capture immutable system-bar/cutout geometry on the Java view thread.

    use android_activity::AndroidApp;
    use flui_foundation::geometry::{EdgeInsets, Size};
    use jni::{JValue, jni_sig, jni_str};

    use crate::version::gate::android::{AndroidApi, DisplayCutout, InsetsByType};

    #[derive(Clone, Copy)]
    pub(super) struct Sample {
        size: Size<i32>,
        insets: [i32; 4],
    }

    impl Sample {
        /// Whether this sample was taken at `size`.
        pub(super) fn is_for(self, size: Size<i32>) -> bool {
            self.size == size
        }

        pub(super) fn logical(self, size: Size<i32>, scale: f64) -> Option<EdgeInsets> {
            if self.size != size
                || size.width <= 0
                || size.height <= 0
                || !scale.is_finite()
                || scale <= 0.0
            {
                return None;
            }
            let [top, right, bottom, left] = self.insets;
            let edges = [
                f64::from(top.clamp(0, size.height)) / scale,
                f64::from(right.clamp(0, size.width)) / scale,
                f64::from(bottom.clamp(0, size.height)) / scale,
                f64::from(left.clamp(0, size.width)) / scale,
            ];
            edges
                .iter()
                .all(|edge| edge.is_finite())
                .then(|| EdgeInsets::new(edges[0], edges[1], edges[2], edges[3]))
        }
    }

    #[derive(Clone, Copy)]
    pub(super) enum Query {
        Idle,
        Waiting,
        Ready(Option<Sample>),
    }

    pub(super) struct Capture {
        pub(super) accepted: EdgeInsets,
        pub(super) query: Query,
        /// A reply taken at a size the window no longer has is asked for again.
        pub(super) retry_stale: bool,
        /// A change was reported while a reply was out: once it settles, ask
        /// again, since that reply may predate the change.
        pub(super) requery: bool,
    }

    impl Default for Capture {
        fn default() -> Self {
            Self {
                accepted: EdgeInsets::ZERO,
                query: Query::Idle,
                retry_stale: false,
                requery: false,
            }
        }
    }

    /// Must run on Android's Java main thread. The insets are the system bars and
    /// the display cutout; keyboard occlusion is not included. From API 30 they
    /// are the bars as they are now, so a hidden bar (immersive or fullscreen
    /// content) reserves no space. Below it the only reading without the keyboard
    /// is the stable one, which keeps a hidden bar's space.
    pub(super) fn sample(app: &AndroidApp, api: AndroidApi) -> Option<Sample> {
        crate::owner::android::activity::with_activity(app, |env, activity| {
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            let decor = env
                .call_method(
                    &window,
                    jni_str!("getDecorView"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            let insets = env
                .call_method(
                    &decor,
                    jni_str!("getRootWindowInsets"),
                    jni_sig!("()Landroid/view/WindowInsets;"),
                    &[],
                )?
                .l()?;
            if insets.is_null() {
                return Ok(None);
            }
            let width = env
                .call_method(&decor, jni_str!("getWidth"), jni_sig!("()I"), &[])?
                .i()?;
            let height = env
                .call_method(&decor, jni_str!("getHeight"), jni_sig!("()I"), &[])?
                .i()?;
            let mut edges = [0; 4];
            if api.since::<InsetsByType>().is_some() {
                let bars = env
                    .call_static_method(
                        jni_str!("android/view/WindowInsets$Type"),
                        jni_str!("systemBars"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()?;
                let cutout = env
                    .call_static_method(
                        jni_str!("android/view/WindowInsets$Type"),
                        jni_str!("displayCutout"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()?;
                let system = env
                    .call_method(
                        &insets,
                        jni_str!("getInsets"),
                        jni_sig!("(I)Landroid/graphics/Insets;"),
                        &[JValue::Int(bars | cutout)],
                    )?
                    .l()?;
                for (edge, name) in edges.iter_mut().zip([
                    jni_str!("top"),
                    jni_str!("right"),
                    jni_str!("bottom"),
                    jni_str!("left"),
                ]) {
                    *edge = env.get_field(&system, name, jni_sig!("I"))?.i()?;
                }
            } else {
                for (edge, name) in edges.iter_mut().zip([
                    jni_str!("getStableInsetTop"),
                    jni_str!("getStableInsetRight"),
                    jni_str!("getStableInsetBottom"),
                    jni_str!("getStableInsetLeft"),
                ]) {
                    *edge = env.call_method(&insets, name, jni_sig!("()I"), &[])?.i()?;
                }
            }
            if api.since::<DisplayCutout>().is_some() && api.since::<InsetsByType>().is_none() {
                let cutout = env
                    .call_method(
                        &insets,
                        jni_str!("getDisplayCutout"),
                        jni_sig!("()Landroid/view/DisplayCutout;"),
                        &[],
                    )?
                    .l()?;
                if !cutout.is_null() {
                    for (edge, name) in edges.iter_mut().zip([
                        jni_str!("getSafeInsetTop"),
                        jni_str!("getSafeInsetRight"),
                        jni_str!("getSafeInsetBottom"),
                        jni_str!("getSafeInsetLeft"),
                    ]) {
                        *edge =
                            (*edge).max(env.call_method(&cutout, name, jni_sig!("()I"), &[])?.i()?);
                    }
                }
            }
            Ok(Some(Sample {
                size: Size::new(width, height),
                insets: edges,
            }))
        })
        .ok()
        .flatten()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn android_safe_area_native_metrics() {
            let size = Size::new(1080, 2400);
            let sample = Sample {
                size,
                insets: [128, 0, 63, 0],
            };
            assert_eq!(
                sample.logical(size, 2.625),
                Some(EdgeInsets::new(128.0 / 2.625, 0.0, 24.0, 0.0))
            );
            assert!(
                sample.logical(Size::new(2400, 1080), 2.625).is_none(),
                "an old portrait reply cannot enter a landscape frame"
            );
            for scale in [0.0, f64::INFINITY, f64::NAN, f64::MIN_POSITIVE / 100.0] {
                assert!(sample.logical(size, scale).is_none());
            }
        }
    }
}

mod gesture_exclusion {
    //! Hand system-gesture exclusion rectangles to the Activity's decor view
    //! (`View.setSystemGestureExclusionRects`, API 29+).

    use android_activity::AndroidApp;
    use jni::{JValue, jni_sig, jni_str};

    /// Replaces the decor view's exclusion rectangles with `rects` (`[left, top, right,
    /// bottom]` in the view's pixels). Must run on Android's Java main thread, which owns
    /// the view hierarchy.
    pub(super) fn apply(
        app: &AndroidApp,
        rects: &[[i32; 4]],
    ) -> Result<(), flui_platform::PreferenceQueryError> {
        crate::owner::android::activity::with_activity(app, |env, activity| {
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            let decor = env
                .call_method(
                    &window,
                    jni_str!("getDecorView"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            let capacity = i32::try_from(rects.len()).unwrap_or(i32::MAX);
            let list = env.new_object(
                jni_str!("java/util/ArrayList"),
                jni_sig!("(I)V"),
                &[JValue::Int(capacity)],
            )?;
            for &[left, top, right, bottom] in rects {
                let rect = env.new_object(
                    jni_str!("android/graphics/Rect"),
                    jni_sig!("(IIII)V"),
                    &[
                        JValue::Int(left),
                        JValue::Int(top),
                        JValue::Int(right),
                        JValue::Int(bottom),
                    ],
                )?;
                env.call_method(
                    &list,
                    jni_str!("add"),
                    jni_sig!("(Ljava/lang/Object;)Z"),
                    &[JValue::Object(&rect)],
                )?;
            }
            env.call_method(
                &decor,
                jni_str!("setSystemGestureExclusionRects"),
                jni_sig!("(Ljava/util/List;)V"),
                &[JValue::Object(&list)],
            )?;
            Ok(())
        })
    }
}

pub(crate) mod system_chrome {
    //! The status-bar icon appearance a window's content asks for.
    //!
    //! "Light status bars" in Android's vocabulary means a light bar background
    //! with dark icons, so [`SystemUiOverlayStyle::Dark`] sets the flag. With
    //! [`InsetsController`] it goes through
    //! `WindowInsetsController.setSystemBarsAppearance`; otherwise through the
    //! `SYSTEM_UI_FLAG_LIGHT_STATUS_BAR` visibility flag, which the floor (API 23)
    //! has. View calls run on the Java main thread, in submission order.

    use android_activity::AndroidApp;
    use jni::{JValue, jni_sig, jni_str};

    use flui_platform::{PlatformSystemChrome, SystemUiOverlayStyle};

    use crate::version::gate::android::{AndroidApi, InsetsController};

    /// `WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS`.
    const APPEARANCE_LIGHT_STATUS_BARS: i32 = 0x8;
    /// `View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR`.
    const SYSTEM_UI_FLAG_LIGHT_STATUS_BAR: i32 = 0x2000;

    /// `PlatformSystemChrome` for the activity's window.
    pub struct AndroidSystemChrome {
        app: AndroidApp,
        api: AndroidApi,
    }

    impl std::fmt::Debug for AndroidSystemChrome {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AndroidSystemChrome")
                .field("api", &self.api)
                .finish_non_exhaustive()
        }
    }

    /// The Activity's window styles the status bar over it.
    impl crate::host::Chrome<crate::owner::android::ActivityRef> for AndroidSystemChrome {
        fn for_window(
            window: &crate::owner::android::ActivityRef,
        ) -> std::sync::Arc<dyn PlatformSystemChrome> {
            std::sync::Arc::new(Self {
                app: window.app.clone(),
                api: window.api,
            })
        }
    }

    impl PlatformSystemChrome for AndroidSystemChrome {
        fn set_overlay_style(&self, style: SystemUiOverlayStyle) {
            let dark_icons = style == SystemUiOverlayStyle::Dark;
            let app = self.app.clone();
            let api = self.api;
            self.app.run_on_java_main_thread(Box::new(move || {
                if let Err(error) = apply(&app, api, dark_icons) {
                    tracing::warn!(%error, "status bar appearance was not applied");
                }
            }));
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Must run on the Java main thread.
    fn apply(
        app: &AndroidApp,
        api: AndroidApi,
        dark_icons: bool,
    ) -> Result<(), flui_platform::PreferenceQueryError> {
        crate::owner::android::activity::with_activity(app, |env, activity| {
            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            if api.since::<InsetsController>().is_some() {
                let controller = env
                    .call_method(
                        &window,
                        jni_str!("getInsetsController"),
                        jni_sig!("()Landroid/view/WindowInsetsController;"),
                        &[],
                    )?
                    .l()?;
                if !controller.is_null() {
                    let appearance = if dark_icons {
                        APPEARANCE_LIGHT_STATUS_BARS
                    } else {
                        0
                    };
                    env.call_method(
                        &controller,
                        jni_str!("setSystemBarsAppearance"),
                        jni_sig!("(II)V"),
                        &[
                            JValue::Int(appearance),
                            JValue::Int(APPEARANCE_LIGHT_STATUS_BARS),
                        ],
                    )?;
                    return Ok(());
                }
            }
            let decor = env
                .call_method(
                    &window,
                    jni_str!("getDecorView"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            let flags = env
                .call_method(
                    &decor,
                    jni_str!("getSystemUiVisibility"),
                    jni_sig!("()I"),
                    &[],
                )?
                .i()?;
            let flags = if dark_icons {
                flags | SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
            } else {
                flags & !SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
            };
            env.call_method(
                &decor,
                jni_str!("setSystemUiVisibility"),
                jni_sig!("(I)V"),
                &[JValue::Int(flags)],
            )?;
            Ok(())
        })
    }
}
