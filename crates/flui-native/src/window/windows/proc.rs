//! A window's native context and the window's part of FLUI's window
//! procedure: the context each window keeps in its `GWLP_USERDATA` slot, the
//! gates that reach it on the window's owner thread, the window's state as the
//! OS reports it, and [`WindowProc`], the hook that handles the messages of the
//! window itself (its size, place, mode, visibility, focus, paint, chrome and
//! closing).
//!
//! Some of those messages carry a step another feature follows: the input
//! follows the window's size, paint, focus loss, minimizing, scale, settings
//! and modal size-and-move loop, the drop target its scale, a move and a DPI
//! change move the input method's rectangles, destruction ends the input's
//! contacts and revokes the drop target, and the close callbacks having run
//! deactivates the text services. The window tells the class's hooks of each
//! step where the message needs it (`owner::windows::window_step`,
//! `window_destroyed`), and follows one step of another hook in turn: the
//! session's lock, which hides it. The window has no keys of its own.

use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{Brightness, SizeConstraints, WindowExtent, WindowId, WindowMode};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::{BeginPaint, ClientToScreen, EndPaint, InvalidateRect, PAINTSTRUCT, ScreenToClient},
    },
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWL_STYLE, GWLP_USERDATA, GetClientRect,
        GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HTCLIENT, IsIconic,
        IsWindowVisible, IsZoomed, MINMAXINFO, NCCALCSIZE_PARAMS, SC_KEYMENU, SC_RESTORE, SW_SHOW,
        SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SWP_NOSIZE, SendMessageW, WA_INACTIVE, WINDOWPOS,
        WM_ACTIVATE, WM_APP, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
        WM_ERASEBKGND, WM_GETDPISCALEDSIZE, WM_GETMINMAXINFO, WM_KILLFOCUS, WM_MOVE, WM_NCCALCSIZE,
        WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SETFOCUS, WM_SETTEXT, WM_SETTINGCHANGE,
        WM_SHOWWINDOW, WM_SIZE, WM_STYLECHANGED, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED,
        WM_WINDOWPOSCHANGING, WS_CAPTION, WS_MAXIMIZE, WS_MAXIMIZEBOX, WS_MINIMIZE,
        WS_OVERLAPPEDWINDOW, WS_POPUP, WS_THICKFRAME,
    },
};

use super::frame_timing::{RefreshTiming, refresh_timing};
use super::geometry::{DeviceRects, sampled_geometry, window_display};
use super::mode::NativeMode;
use super::record::WindowRecord;
use super::set::{WindowSet, checked_logical_to_device, restore_rectangle};
use super::window::WindowsWindow;

use crate::owner::hwnd_affinity::ContextLedger;
use crate::owner::window_delivery::{Publication, WindowDelivery};
use crate::owner::windows::HostDisplays;
use crate::owner::windows::control::{OwnerPreferences, OwnerQueue, PostRefusal};
use crate::owner::windows::modal_loop::{
    MODAL_DEADLINE_TIMER, ModalPacing, PaceClock, kill_window_timer,
};
use crate::owner::windows::{
    WindowChrome, WindowHooks, WindowIdentity, WindowMessages, WindowStep, window_step,
};
use crate::owner::windows::{get_x_lparam, get_y_lparam};
use crate::window::state::{ReportedFacts, Sample, WindowStateCell, mode_of, reported_frame};

/// Posted to a window nothing paints, so its frame still runs: a hidden window
/// never receives `WM_PAINT`. It names the window's identity ([`FramePost`]).
pub(crate) const REQUEST_FRAME: u32 = WM_APP + 1;

/// Posted by a close the handler granted behind a running method, which the
/// owner thread takes once the method returned. It destroys the window it
/// names without asking the veto again: the handler already granted it.
pub(crate) const CLOSE_WINDOW: u32 = WM_APP + 4;

/// Posted when another thread raised a delivery for a window: the owner
/// delivers what waits in the window's FIFO when it takes the message.
pub(crate) const DELIVER_QUEUED: u32 = WM_APP + 5;

/// One kind of work a window posts to itself from any thread, posted at most
/// once until the window takes it: a burst of asks is one message, and one
/// item for the owner's turn when the queue refuses it ([`OwnerQueue`]).
#[derive(Clone)]
pub(crate) struct WindowPost {
    /// The handle as an integer: only posted to, never dereferenced.
    hwnd: isize,
    identity: WindowIdentity,
    message: u32,
    /// Set from a post until the window takes it.
    posted: Arc<AtomicBool>,
    queue: OwnerQueue,
}

impl WindowPost {
    pub(crate) fn new(
        hwnd: HWND,
        identity: WindowIdentity,
        message: u32,
        queue: OwnerQueue,
    ) -> Self {
        Self {
            hwnd: hwnd.0 as isize,
            identity,
            message,
            posted: Arc::new(AtomicBool::new(false)),
            queue,
        }
    }

    /// Post the message unless one waits for the window already.
    pub(crate) fn ask(&self) {
        if self.posted.swap(true, Ordering::AcqRel) {
            return;
        }
        let posted = self
            .queue
            .post(HWND(self.hwnd as *mut _), self.message, self.identity);
        if posted.is_err() {
            // No window of the owner thread takes it: nothing waits.
            self.posted.store(false, Ordering::Release);
        }
    }

    /// The window took the message: an ask from here on posts again. Called
    /// before the work runs, so an ask the work makes is not folded into it.
    fn taken(&self) {
        self.posted.store(false, Ordering::Release);
    }
}

impl std::fmt::Debug for WindowPost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowPost")
            .field("hwnd", &self.hwnd)
            .field("message", &self.message)
            .field("posted", &self.posted.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// A window's frame requests, from any thread.
///
/// A window that is shown and not minimized is invalidated, and Win32 folds
/// every invalidation before its paint into one `WM_PAINT`. Win32 paints
/// neither a hidden window nor a minimized one, whose client area is empty:
/// those get [`REQUEST_FRAME`] posted through a [`WindowPost`], so a burst of
/// wakes is one frame there too and never fills the thread's queue. Their
/// frame is what earns a deferred window its reveal, and what polls the tasks
/// woken while nobody sees the window.
///
/// The visibility is read without holding the window: one hidden or restored
/// on its thread between the read and the request gets one frame request of
/// the other kind as well, an invalidation nothing paints or a posted frame
/// beside its paint. That costs at most one extra frame, never a lost one.
#[derive(Clone, Debug)]
pub(crate) struct FramePost(WindowPost);

impl FramePost {
    pub(crate) fn new(hwnd: HWND, identity: WindowIdentity, queue: OwnerQueue) -> Self {
        Self(WindowPost::new(hwnd, identity, REQUEST_FRAME, queue))
    }

    /// Ask for a frame of the window.
    pub(crate) fn request(&self) {
        let hwnd = HWND(self.0.hwnd as *mut _);
        // SAFETY: plain queries of a window handle with no pointer arguments,
        // legal from any thread; for a window that is already gone they
        // answer false, which leaves nothing to paint.
        let painted = unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
        if painted {
            // SAFETY: as above; an invalidation of a window that closed
            // meanwhile fails and draws nothing.
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            // A drag loop resting in OLE dispatches no paint: its pacing
            // paints the window once knocked.
            self.0.queue.wake_modal();
        } else {
            self.0.ask();
        }
    }

    /// The window took the posted request.
    fn taken(&self) {
        self.0.taken();
    }
}

/// The owner turn a window's delivery asks for when another thread raises
/// something for it: a [`DELIVER_QUEUED`] naming this window, posted once
/// until the window delivers what waits in its FIFO.
pub(crate) fn deliver_queued_turn(deliveries: WindowPost) -> crate::owner::signal::WindowWake {
    Arc::new(move || deliveries.ask())
}

pub(crate) use crate::owner::windows::WindowRef;

/// Close the window `hwnd` and `identity` name, without asking the veto, in a
/// turn of the owner thread: a posted [`CLOSE_WINDOW`], sent from the owner's
/// next turn when the queue is full.
///
/// # Errors
///
/// What [`OwnerQueue::post`] refuses: no window of the owner thread has the
/// handle, which is then closed already.
pub(crate) fn post_close(
    queue: &OwnerQueue,
    hwnd: HWND,
    identity: WindowIdentity,
) -> Result<(), PostRefusal> {
    queue.post(hwnd, CLOSE_WINDOW, identity)
}

/// The close of a `WM_CLOSE` the handler grants after it was asked behind a
/// running method ([`post_close`]), which the owner thread takes once the
/// method returned.
fn granted_close(
    hwnd: HWND,
    identity: WindowIdentity,
    queue: OwnerQueue,
) -> impl FnOnce() + Send + 'static {
    // The handle as an integer: it is only posted to, never dereferenced, so
    // it may travel with the queued request.
    let raw = hwnd.0 as isize;
    move || {
        // A window that is gone is closed already.
        let _: Result<(), PostRefusal> = post_close(&queue, HWND(raw as *mut _), identity);
    }
}

/// The window context a [`WindowRef`] names: the owner declares the
/// reference, and the window feature, whose context it is, reaches it.
impl WindowRef {
    /// [`with_window_context`] for this window only: `None` off its owner
    /// thread, after it closed, or when its handle now names another window.
    pub(crate) fn with_context<R>(
        self,
        op: &'static str,
        f: impl FnOnce(&WindowContext) -> R,
    ) -> Option<R> {
        with_window_context(self.hwnd(), op, |context| {
            (context.identity == self.identity()).then(|| f(context))
        })
        .flatten()
    }
}

/// Context data stored per window for event dispatch.
///
/// Lives in the window's `GWLP_USERDATA` slot and is only ever reached on
/// the window's owner thread, through [`with_window_context`] or
/// `window_proc`. That is what makes it the home of the window's callbacks:
/// a registration that cannot reach this context from the calling thread is
/// refused, so a callback never lands where a foreign thread could run or
/// drop it.
pub(crate) struct WindowContext {
    /// Window ID for event dispatch
    pub window_id: WindowId,
    /// Which native window this context belongs to; see [`WindowIdentity`].
    pub identity: WindowIdentity,
    /// The windows the platform tracks. Weak: tracking and cache access do
    /// not keep wrappers or their state alive.
    pub windows: std::sync::Weak<WindowSet>,
    /// What the window honours and observes, fixed when it opens: the build
    /// it runs on does not change under it.
    pub(crate) capabilities: flui_platform::WindowCapabilities,
    /// What the window procedure records of the window, shared with its
    /// wrappers. Weak: the context does not keep the wrappers' count up.
    pub(crate) window_state: std::rc::Weak<Cell<WindowRecord>>,
    /// The published window state, shared with the wrapper; refreshed by
    /// [`Self::publish_window_state`].
    pub snapshot: Arc<WindowStateCell>,
    /// Asks the owner to consult the exit policy once this window's
    /// retirement (`WindowContext::retire`) leaves the platform tracking no
    /// window.
    pub exit_policy: crate::owner::windows::control::ExitPolicyRequest,
    /// The owner thread's queue the window posts its own work to: a close
    /// granted behind a running method.
    pub(crate) queue: OwnerQueue,
    /// The window's frame requests, shared with its wrappers' surfaces.
    pub(crate) frame_post: FramePost,
    /// The window's requests to deliver what other threads raised for it,
    /// shared with its delivery.
    pub(crate) deliveries: WindowPost,
    /// The platform's monitors: a DPI change invalidates their snapshot.
    pub displays: Arc<dyn HostDisplays>,
    /// Per-window callbacks for event delivery, shared with the features the
    /// class's hooks announced the window to and released by the window's
    /// retirement (`WindowContext::retire`) on the owner thread.
    pub delivery: Arc<WindowDelivery>,
    /// The hooks of the window's class, which the window tells of its steps.
    pub(crate) hooks: WindowHooks,
    /// The system preferences: the window's gesture geometry is measured with
    /// them.
    pub(crate) preferences: Rc<dyn OwnerPreferences>,
    /// The window's DPI, for coordinate conversion: the one value the
    /// window's class hooks read too ([`CreatedWindow::scale`]).
    ///
    /// Interior-mutable: `window_proc` only ever holds a shared
    /// `&WindowContext` (see its `# Safety` section), and `WM_DPICHANGED` is
    /// the one message that updates it, through that shared reference instead
    /// of a second, aliasing `&mut WindowContext` forged from the raw
    /// `GWLP_USERDATA` pointer while the shared one is still live.
    ///
    /// [`CreatedWindow::scale`]: crate::owner::windows::CreatedWindow::scale
    pub(crate) scale: crate::owner::windows::WindowScale,
    /// Whether the last `WM_DPICHANGED` was refused (its DPI no ratio a screen
    /// can have), so a sender that repeats one is reported once per run.
    pub(crate) dpi_streak: std::cell::Cell<crate::display::scale::ScaleStreak>,
    /// Current window mode (replaces display_state + saved bounds)
    pub mode: std::cell::Cell<NativeMode>,
    /// Last known size (before minimization) for restore detection
    pub last_size: std::cell::Cell<Size<i32>>,
    /// The user's app theme as last sampled, which the window reports in
    /// the snapshot; the chrome is drawn in it while
    /// `chrome_follows_system`.
    pub appearance: std::cell::Cell<Brightness>,
    /// Whether the last `WM_ACTIVATE` made the window the active one of its
    /// desktop: it, or a window it owns, has the keyboard.
    pub(crate) active: std::cell::Cell<bool>,
    /// Whether the title bar follows the system theme; `false` once the app
    /// chose it through `Win32Window::set_dark_mode` or `set_theme`, or opened
    /// with a theme of its own (`Win32WindowOptions::theme`).
    pub chrome_follows_system: std::cell::Cell<bool>,
    /// The owner thread's COM apartment, kept entered until the context is
    /// freed, after its window's `WM_DESTROY` ran every hook's teardown (the
    /// drop target's revoke, the text services' retirement), even when the
    /// last wrapper let go of the window inside that `WM_DESTROY`.
    pub(crate) _apartment: Rc<crate::owner::windows::com_apartment::ComApartment>,
    /// The cursor this window shows over its client area, as its presentation
    /// selected it.
    pub cursor: std::cell::RefCell<super::cursor::NativeCursor>,
    /// The grab the window holds; `None` whenever it lost the focus.
    pub(crate) pointer_hold: Cell<Option<super::cursor::Held>>,
    /// Whether the window is kept below the others, a level Win32 has no
    /// style for: `WM_WINDOWPOSCHANGING` keeps it at the bottom.
    pub(crate) kept_below: Cell<bool>,
    /// The steps and the proportion the user's resize keeps, in logical
    /// pixels; `WM_SIZING` holds the rectangle to them at the window's scale.
    pub(crate) resize_increments: Cell<Option<WindowExtent>>,
    pub(crate) aspect_ratio: Cell<Option<flui_platform::AspectRatio>>,
    /// Set from the first `WM_SIZING` of a size-move loop to its end: the
    /// user is resizing the window by its frame, not moving it.
    pub(crate) live_resize: Cell<bool>,
    /// Window style bits before fullscreen (Windows-specific: WS_OVERLAPPEDWINDOW, etc.)
    ///
    /// Stored here instead of in `NativeMode` to keep the cross-platform enum
    /// free of platform-specific fields.
    pub restore_style: std::cell::Cell<u32>,
    /// The inner-size limits the app set, in logical pixels; `WM_GETMINMAXINFO`
    /// turns them into outer limits at the window's current DPI.
    pub constraints: std::cell::Cell<flui_platform::SizeConstraints>,
    /// Whether the last DPI the limits were read at was refused, so a run of
    /// refusals is reported once.
    pub scale_streak: std::cell::Cell<crate::display::scale::ScaleStreak>,
    /// Whether the app wants the window resizable. A window without a frame
    /// cannot be, but gets the frame back with this still set.
    pub resizable: std::cell::Cell<bool>,
    /// The frame the app asked for. `AppTitleBar` keeps the system's overlapped
    /// style and drops the caption in `WM_NCCALCSIZE`; it and `None` answer
    /// `WM_NCHITTEST` from [`Self::input_regions`].
    pub(crate) frame: std::cell::Cell<flui_platform::WindowFrame>,
    /// The device-pixel input regions the last composited frame handed the
    /// window, which `WM_NCHITTEST` answers from without running framework
    /// code (ADR-0286).
    pub(crate) input_regions: std::cell::RefCell<crate::input::regions::ProjectedInputRegions>,
    /// How many multi-step changes are in flight; see
    /// [`Self::hold_publishing`].
    pub publish_hold: std::cell::Cell<u32>,
    /// Set while the window is being put into fullscreen, before its mode says
    /// so: the size limits do not apply to the rectangle it is given.
    pub limits_waived: std::cell::Cell<bool>,
    /// The logical size the content kept at the window's last scale change,
    /// and the one the rectangle a fullscreen window returns to kept
    /// (`rescale::rescaled_outer`): each holds only while the pixels are still
    /// that size's, so a resize gives it way.
    pub(crate) kept_logical: std::cell::Cell<KeptLogical>,
    /// The auto-hide app bars of the monitor the window was last maximized
    /// on, as the shell answered, until the settings or the displays change
    /// ([`Self::auto_hide_edges_at`]).
    pub(crate) app_bars: std::cell::Cell<Option<super::app_bars::MonitorAppBars>>,
    /// How many times the app asked for the window to be shown; see
    /// [`Self::note_show_request`].
    pub show_requests: std::cell::Cell<u32>,
    /// What is still owed of the window's first show. A window opens hidden and
    /// in its mode, so the first sight of it is already maximized or
    /// fullscreen, not a normal window that jumps.
    pub(crate) first_show: std::cell::Cell<FirstShow>,
    /// What occludes a shown window in its snapshot: a cloak or a locked
    /// session.
    pub(crate) visibility: Visibility,
    /// Set between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`: a live resize,
    /// whose `WM_SIZE` renders at once.
    pub in_size_move: std::cell::Cell<bool>,
    /// Set while the default procedure carries out a placement system command
    /// (`SC_MAXIMIZE`, `SC_MINIMIZE`, `SC_RESTORE`) the user worked: a caption
    /// button, the system menu or a double click on the title bar. A system
    /// command this window sends itself is marked by
    /// [`Self::in_app_command`] and never sets it.
    pub in_system_command: std::cell::Cell<bool>,
    /// Set while this window sends itself a system command for an
    /// application's command (the restore before fullscreen): the sender marks
    /// it, so the command is not the user's.
    pub in_app_command: std::cell::Cell<bool>,
    /// The client size, in device pixels, and the scale the last resize
    /// callback delivered.
    pub last_resize: std::cell::Cell<Option<(Size<i32>, f64)>>,
    /// Delivers wake deadlines while a modal loop runs for the window, and
    /// reaches the modal loops running on the thread.
    pub(crate) modal_pacing: Rc<ModalPacing>,
    /// Borrow ledger deferring this context's free past every live borrow
    /// on the owner thread — see [`ContextLedger`] for the reentrancy
    /// hazard (a framework callback closing the window from inside a
    /// dispatch) this exists to survive, and [`ContextGuard`] for the RAII
    /// half. `RefCell`, not a lock: only the owning thread ever touches it
    /// (the affinity gates enforce that), and every borrow is a short
    /// non-reentrant method call.
    pub ledger: std::cell::RefCell<ContextLedger>,
    /// Set once the window's retirement began (`WindowContext::retire`): a
    /// close its callbacks ask for while it runs finds the window closing
    /// and does nothing.
    pub(crate) retiring: std::cell::Cell<bool>,
}

/// The compositor's last and next refresh around now, from the desktop window manager's timing
/// report and the performance counter it is measured on (ADR-0347). `None` when the window manager
/// is not compositing (it has no timing to report) or the report cannot be placed.
fn compositor_refresh() -> Option<RefreshTiming> {
    use windows::Win32::Graphics::Dwm::{DWM_TIMING_INFO, DwmGetCompositionTimingInfo};
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    let mut info = DWM_TIMING_INFO {
        cbSize: u32::try_from(std::mem::size_of::<DWM_TIMING_INFO>()).ok()?,
        ..DWM_TIMING_INFO::default()
    };
    let (mut now, mut frequency) = (0_i64, 0_i64);
    // SAFETY: `info` is an initialized structure whose `cbSize` is its size, which the call fills
    // in; a null window handle asks for the system-wide timing, which needs no window to be alive.
    // The counter calls write one `i64` each through pointers to initialized locals.
    unsafe {
        DwmGetCompositionTimingInfo(HWND::default(), &raw mut info).ok()?;
        QueryPerformanceCounter(&raw mut now).ok()?;
        QueryPerformanceFrequency(&raw mut frequency).ok()?;
    }
    refresh_timing(
        u64::try_from(now).ok()?,
        info.qpcVBlank,
        info.qpcRefreshPeriod,
        u64::try_from(frequency).ok()?.try_into().ok()?,
    )
}

impl WindowContext {
    /// Runs this window's frame callback; its callbacks count it on the
    /// platform only if one is registered to run. The request carries the
    /// compositor's last and next refresh when DWM reports them, and the
    /// event clock's reading otherwise.
    pub(crate) fn request_frame(&self) {
        match compositor_refresh() {
            Some(timing) => self.delivery.dispatch_frame(crate::frame_request_timed(
                timing.since_refresh,
                Some(timing.until_refresh),
            )),
            None => self.delivery.dispatch_request_frame(),
        }
    }

    /// Tell the class's hooks that this window, `hwnd`, took `step`. A hook
    /// may run user code, which may close the window: a caller that goes on
    /// asks [`Self::still_owns`] first where the next step would act on the
    /// handle.
    pub(crate) fn step(&self, hwnd: HWND, step: WindowStep) {
        let hooks = Rc::clone(&self.hooks);
        window_step(&hooks, WindowRef::new(hwnd, self.identity), step);
    }

    /// The window's client size is `device` pixels at its current scale:
    /// publish the snapshot, whose metrics read the client area.
    pub(crate) fn report_resize(&self, device: Size<i32>) {
        self.last_resize.set(Some((device, self.scale.scale())));
        self.delivery.publish_state();
    }

    /// Record what the window procedure learned of the window, before any
    /// native or user code that can call back runs: `change` takes the record
    /// and returns the next one ([`record::update`](super::record::update)).
    pub(crate) fn update_window_state(&self, change: impl FnOnce(WindowRecord) -> WindowRecord) {
        super::record::update(&self.window_state, change);
    }

    /// The window's published state for a caller that reads it
    /// (`PlatformWindow::state`), read from the OS again first when something
    /// invalidated it. A revision the read publishes is announced in a turn
    /// of the window's own message queue, never from the read.
    pub(crate) fn window_state(&self, hwnd: HWND) -> flui_platform::WindowState {
        // While a change takes several steps the window is half-way between two
        // states; a read from inside a callback it raises sees the last whole one.
        if self.publish_hold.get() > 0 {
            return self.snapshot.published();
        }
        let Some(record) = self.window_state.upgrade() else {
            return self.snapshot.published();
        };
        self.snapshot.read(&self.delivery, || {
            sample_state(hwnd, record.get(), self.tracking(hwnd))
        })
    }

    /// Read the window again for its state publisher, announcing nothing; a
    /// a change of several steps is announced once, when it ends.
    pub(crate) fn read_window_state(&self, hwnd: HWND) -> Publication {
        if self.publish_hold.get() > 0 {
            self.snapshot.invalidate();
            return Publication::Held;
        }
        let Some(record) = self.window_state.upgrade() else {
            self.snapshot.invalidate();
            return Publication::Unchanged;
        };
        self.snapshot.refresh(&self.delivery, || {
            sample_state(hwnd, record.get(), self.tracking(hwnd))
        })
    }

    /// Whether the application draws the window's title bar: the frame asked
    /// for is `AppTitleBar`, whose caption `WM_NCCALCSIZE` drops.
    pub(crate) fn draws_app_title_bar(&self) -> bool {
        matches!(self.frame.get(), flui_platform::WindowFrame::AppTitleBar(_))
    }

    /// Whether the chrome regions the application hands the window answer the
    /// system's hit test (ADR-0286 §7): the system draws no title bar, and the
    /// window is not fullscreen, where nothing moves or resizes it from a bar.
    /// Its caption buttons are then the application's.
    pub(super) fn chrome_answers_the_hit_test(&self) -> bool {
        !self.frame.get().has_system_title_bar() && !self.mode.get().holds_fullscreen()
    }

    /// What this context knows of the window beyond what Win32 reports.
    pub(crate) fn tracking(&self, hwnd: HWND) -> OwnerTracking {
        let (display, desktop) = window_display(&*self.displays, hwnd);
        OwnerTracking {
            mode: self.mode.get(),
            restore_style: self.restore_style.get(),
            constraints: self.constraints.get(),
            display,
            desktop,
            occluded: self.visibility.occluded(),
            appearance: self.appearance.get(),
            active: self.active.get(),
            hovered: self.delivery.hovered(),
            capabilities: self.capabilities,
            frame: self.frame.get(),
            interaction: if self.in_size_move.get() && self.live_resize.get() {
                flui_platform::WindowInteraction::UserResizing
            } else if self.in_size_move.get() || self.in_system_command.get() {
                flui_platform::WindowInteraction::UserPlacing
            } else {
                flui_platform::WindowInteraction::None
            },
            cursor_grab: self.cursor_grab(),
            kept_below: self.kept_below.get(),
        }
    }

    /// The window system reported a change, or a command was just carried
    /// out: read the window again, and raise `WindowHandler::state_changed`
    /// when that produced a new revision.
    pub(crate) fn publish_window_state(&self, hwnd: HWND) {
        // A change that takes several steps publishes once, when it is done:
        // a subscriber never sees the window half-changed.
        let record = (self.publish_hold.get() == 0)
            .then(|| self.window_state.upgrade())
            .flatten();
        let Some(record) = record else {
            self.snapshot.invalidate();
            return;
        };
        self.snapshot.publish_report(&self.delivery, || {
            sample_state(hwnd, record.get(), self.tracking(hwnd))
        });
    }

    /// Hold back publishing until the returned guard drops; the caller then
    /// publishes once.
    pub(crate) fn hold_publishing(&self) -> PublishHold<'_> {
        self.publish_hold
            .set(self.publish_hold.get().saturating_add(1));
        PublishHold(&self.publish_hold)
    }
}

/// Whether the live window behind `hwnd` is of this backend's registered
/// window class — the only class whose `GWLP_USERDATA` slot holds a
/// [`WindowContext`] pointer (`owner::windows::is_flui_window`).
fn hwnd_names_flui_class(hwnd: HWND) -> bool {
    crate::owner::windows::is_flui_window(hwnd)
}

/// RAII borrow of a [`WindowContext`]: counts itself in the context's
/// [`ContextLedger`] on acquisition and, on release, frees the allocation
/// iff it was the outermost borrow of a retired context. This is what makes
/// holding `&WindowContext` across reentrant message dispatch sound — a
/// nested retirement (a framework callback closing the window from inside
/// a dispatch) only *retires* the context; the memory outlives every frame
/// still borrowing it.
struct ContextGuard {
    context: *mut WindowContext,
}

impl ContextGuard {
    /// Registers a borrow of `context`.
    ///
    /// # Safety
    ///
    /// `context` must be the live (not yet freed) allocation installed in a
    /// FLUI window's `GWLP_USERDATA` slot, and the caller must be on that
    /// window's owning thread — both established by the gate or dispatch
    /// provenance at every call site. Single-threadedness is what makes the
    /// non-atomic ledger sound.
    unsafe fn acquire(context: *mut WindowContext) -> Self {
        // SAFETY: `context` is valid per the contract above.
        unsafe { (*context).ledger.borrow_mut().acquire() };
        Self { context }
    }

    /// The borrowed context, lifetime-bound to this guard.
    fn context(&self) -> &WindowContext {
        // SAFETY: `acquire`'s contract established validity, and the ledger
        // defers the only free (a retired context's outermost release) past
        // this guard's own `Drop` — so the allocation outlives `&self`.
        unsafe { &*self.context }
    }
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        // The allocation is still live here — it is freed only by the
        // outermost release of a retired context, which is at the earliest
        // THIS statement (`Self::context` carries that argument). The
        // `RefCell` borrow is a temporary that ends before the free below.
        let must_free = self.context().ledger.borrow_mut().release();
        if must_free {
            // SAFETY: this was the outermost borrow of a retired context:
            // the slot was cleared before retirement (no new borrow can be
            // minted), no other guard exists, and the pointer is the
            // `Box::into_raw` allocation `WindowsWindow::try_new` installed —
            // reclaiming it here is the unique free.
            drop(unsafe { Box::from_raw(self.context) });
        }
    }
}

/// Runs `f` against the [`WindowContext`] stored in `hwnd`'s `GWLP_USERDATA`
/// slot — if, and only if, dereferencing it is sound for the calling thread.
/// Returns `None` (after tracing why) whenever it is not; callers fall back
/// to a safe default instead of touching the slot.
///
/// This gate is what makes the dereference sound. The wrapper is neither
/// `Send` nor `Sync`, but the slot is reached through a raw handle, and what
/// another thread holds of the window (its surface target, its delivery's
/// state publisher) reaches the same handle. The context box is freed exactly
/// once, after the window's retirement (a granted close, or `WM_DESTROY`) on
/// the window's owning thread, so a dereference is
/// sound only when made **on that same thread** (where it is serialized with
/// the free by same-thread message dispatch) against a live window of our
/// own class with a non-null slot. Every decision is delegated to the pure,
/// Linux-tested rules in [`crate::owner::hwnd_affinity`]; this shell only
/// performs the OS queries.
///
/// The borrow is guarded, not fragile: `f` may make calls that re-enter
/// `window_proc` on this same thread (an `SetWindowPos` inside the
/// fullscreen toggle, a dispatched framework callback that closes the
/// window). If such reentrancy reaches the window's retirement, the context is only
/// *retired* — its free is deferred, via the [`ContextLedger`], to the
/// outermost [`ContextGuard`] release — so the reference stays valid for
/// the whole closure; post-retirement it merely describes a window that is
/// already gone, and the remaining Win32 calls against the dead `hwnd`
/// fail cleanly instead of dangling.
pub(crate) fn with_window_context<R>(
    hwnd: HWND,
    op: &'static str,
    f: impl FnOnce(&WindowContext) -> R,
) -> Option<R> {
    with_window_context_checked(hwnd, op, f).ok()
}

/// [`with_window_context`] that reports why it refused. On refusal `f` is
/// dropped here, on the calling thread, untouched.
pub(crate) fn with_window_context_checked<R>(
    hwnd: HWND,
    op: &'static str,
    f: impl FnOnce(&WindowContext) -> R,
) -> Result<R, crate::owner::hwnd_affinity::UserDataRefusal> {
    use crate::owner::hwnd_affinity::{UserDataVerdict, classify_user_data_access};

    // SAFETY: `GetWindowThreadProcessId(hwnd, None)` takes an optional out
    // pointer (`None` — the process id is not needed) and returns 0 for a
    // dead handle; `GetCurrentThreadId` takes no arguments;
    // `GetWindowLongPtrW` reads a pointer-sized value with no dereference.
    // None of these calls pump this thread's message queue.
    let (owner_thread, current_thread, slot) = unsafe {
        (
            GetWindowThreadProcessId(hwnd, None),
            GetCurrentThreadId(),
            GetWindowLongPtrW(hwnd, GWLP_USERDATA),
        )
    };

    match classify_user_data_access(
        owner_thread,
        current_thread,
        hwnd_names_flui_class(hwnd),
        slot,
    ) {
        UserDataVerdict::Deref => {
            // SAFETY: the verdict established that this thread owns the
            // window (owner == current, both live), the window is of our
            // class (so the non-null slot value is the `Box::into_raw`
            // pointer `WindowsWindow::try_new` installed, not a foreign
            // protocol's), and the slot is non-null (so the window's
            // retirement has not retired it). Between the slot read above and this
            // acquisition no message dispatch happens on this thread, so
            // the context cannot have been retired-and-freed in between;
            // once the guard is held, any reentrant retirement inside
            // `f` defers the free past the guard's drop (see
            // `ContextGuard`).
            let guard = unsafe { ContextGuard::acquire(slot as *mut WindowContext) };
            Ok(f(guard.context()))
        }
        UserDataVerdict::Refuse(reason) => {
            tracing::debug!(
                op,
                handle = ?hwnd,
                ?reason,
                "refusing to touch this window's native context"
            );
            Err(reason)
        }
    }
}

/// What `hwnd` names now for the wrapper whose window `installed` describes:
/// its own live window, no window, or a window the OS recycled the handle
/// value for. The window's thread is compared with the one that created the
/// wrapper's window, and the context pointer as a value, never dereferenced,
/// with the window's current `GWLP_USERDATA` slot, so the queries are sound
/// from any thread: a surface target asks from the renderer's. Pure rule in
/// [`crate::owner::hwnd_affinity::handle_names`], which also states the
/// pointer-ABA residual; this shell only performs the OS queries.
pub(crate) fn handle_names(
    hwnd: HWND,
    installed: InstalledWindow,
) -> crate::owner::hwnd_affinity::HandleNames {
    // SAFETY: see the identical queries in `with_window_context` — no
    // pointers dereferenced, no messages pumped.
    let (owner_thread, slot) = unsafe {
        (
            GetWindowThreadProcessId(hwnd, None),
            GetWindowLongPtrW(hwnd, GWLP_USERDATA),
        )
    };
    crate::owner::hwnd_affinity::handle_names(
        owner_thread,
        installed.thread,
        hwnd_names_flui_class(hwnd),
        slot,
        installed.context,
    )
}

/// What a wrapper recorded of its window when it installed it: the thread
/// that created the window and the address of the context it put in the
/// window's `GWLP_USERDATA` slot. Plain integers, only compared, never
/// dereferenced: what [`handle_names`] tells this window from one the OS gave
/// its handle since, from any thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstalledWindow {
    /// The Win32 id of the thread that created the window.
    pub(crate) thread: u32,
    /// The context address installed in the window's slot.
    pub(crate) context: isize,
}

/// Runs `handle` with the context of `hwnd`, borrowed for one message of its
/// window procedure: `None` before the context is installed and after
/// the window's retirement cleared it. `handle` also gets the slot's value, against which
/// a hook can tell, after a callback, that the window closed and another took
/// its handle.
///
/// The borrow survives reentrant dispatch: a hook holds it across framework
/// callbacks, and a callback may close the window — running a nested
/// retirement on this same thread that RETIRES the context (clears the
/// slot, marks the ledger) without freeing it; the free happens when the
/// outermost borrow, possibly this one, ends (see `ContextGuard`).
///
/// # Safety
///
/// `hwnd` is the window Win32 delivered a message of FLUI's class to, and the
/// call runs inside that message's dispatch, on the thread that owns `hwnd`.
pub(crate) unsafe fn with_message_context<R>(
    hwnd: HWND,
    handle: impl FnOnce(Option<&WindowContext>, isize) -> R,
) -> R {
    // SAFETY: a pointer-sized read of the window's slot; nothing is
    // dereferenced here.
    let slot = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    let context = slot as *mut WindowContext;
    // SAFETY: this runs in Win32's dispatch for `hwnd` on its owning thread
    // (this function's contract), and a non-null slot means the context is
    // not yet retired — exactly `ContextGuard::acquire`'s contract.
    let guard = (!context.is_null()).then(|| unsafe { ContextGuard::acquire(context) });
    handle(guard.as_ref().map(ContextGuard::context), slot)
}

/// The window's hook on FLUI's window class: it answers the messages of the
/// window itself and passes every other message on.
pub(crate) struct WindowProc;

/// Whose caption buttons a window's are, as the input asks before it takes
/// the messages over one for the application's button.
impl WindowChrome for WindowProc {
    fn regions_answer_the_hit_test(&self, window: WindowRef) -> Option<bool> {
        window.with_context(
            "caption buttons",
            WindowContext::chrome_answers_the_hit_test,
        )
    }
}

impl WindowMessages for WindowProc {
    /// A locked session hides the window; every other step is the window's
    /// own.
    fn step(&self, window: WindowRef, step: WindowStep) {
        match step {
            WindowStep::SessionLocked(locked) => {
                window.with_context("session lock", |ctx| ctx.set_session_locked(locked));
            }
            WindowStep::Resized(_)
            | WindowStep::Minimized
            | WindowStep::FocusLost
            | WindowStep::Painting
            | WindowStep::SizeMoveStarted
            | WindowStep::SizeMoveEnded
            | WindowStep::Rescaled
            | WindowStep::ScaleApplied
            | WindowStep::SettingChanged
            | WindowStep::Closed
            | WindowStep::PointerLocked(_) => {}
        }
    }

    /// The window's own messages: its creation, closing and destruction, its
    /// size, place, limits, DPI, focus, paint, cursor and title, the modal
    /// size-move and menu loops with their deadline timer, the session and
    /// setting changes it follows, the frame, close and delivery requests
    /// posted to it, and the keyboard's system-menu command.
    fn answers(&self, msg: u32, wparam: WPARAM) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_ENTERMENULOOP, WM_ENTERSIZEMOVE, WM_EXITMENULOOP, WM_EXITSIZEMOVE, WM_SIZING,
            WM_TIMER,
        };
        match msg {
            WM_CREATE | WM_CLOSE | WM_DESTROY | WM_ERASEBKGND | WM_ENTERSIZEMOVE
            | WM_EXITSIZEMOVE | WM_SIZING | WM_ENTERMENULOOP | WM_EXITMENULOOP | WM_PAINT
            | WM_SIZE | WM_SHOWWINDOW | WM_MOVE | WM_DPICHANGED | WM_WINDOWPOSCHANGING
            | WM_WINDOWPOSCHANGED | CLOSE_WINDOW | DELIVER_QUEUED | WM_SETTEXT
            | WM_STYLECHANGED | WM_GETMINMAXINFO | REQUEST_FRAME | WM_SETCURSOR | WM_SETFOCUS
            | WM_KILLFOCUS | WM_ACTIVATE | WM_SETTINGCHANGE | WM_NCCALCSIZE | WM_NCHITTEST
            | WM_GETDPISCALEDSIZE | WM_DISPLAYCHANGE => true,
            WM_TIMER => wparam.0 == MODAL_DEADLINE_TIMER,
            WM_SYSCOMMAND => {
                (wparam.0 & 0xFFF0) == SC_KEYMENU as usize || is_user_placement_command(wparam)
            }
            _ => false,
        }
    }

    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        if !self.answers(msg, wparam) {
            return None;
        }
        // SAFETY: the arguments as Win32 delivered them to the class's
        // procedure, which is `with_message_context`'s and `handle`'s
        // contract too.
        unsafe { with_message_context(hwnd, |ctx, _| Self::handle(hwnd, msg, wparam, lparam, ctx)) }
    }
}

impl WindowProc {
    /// The window's own messages ([`WindowMessages::message`]), with `ctx`, its
    /// context borrowed for the message. Every dispatch of user callbacks
    /// stays inside the class procedure's panic boundary.
    ///
    /// # Safety
    ///
    /// Must only be called with arguments Win32 delivered to this class's
    /// `WNDPROC` for the message being handled — Win32 guarantees
    /// `hwnd`/`msg`/`wparam`/`lparam` are well-formed for that message, and
    /// always dispatches on the thread that owns the window's message queue,
    /// so the `GWLP_USERDATA` writes below do not race the retirement's
    /// clear+free precisely because both run here, serialized by that
    /// same-thread dispatch guarantee.
    unsafe fn handle(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        ctx: Option<&WindowContext>,
    ) -> Option<LRESULT> {
        // SAFETY: see the `# Safety` section above for the `GWLP_USERDATA`
        // contract the arms rely on. This single `unsafe` block covers every
        // Win32 call in the `match` below: `SetWindowLongPtrW` takes `hwnd`
        // and plain integers, no pointers; `BeginPaint`/`EndPaint`
        // (`WM_PAINT`, its own SAFETY note below) take `&raw` pointers to
        // stack-local, correctly-sized structs; `DestroyWindow` calls are
        // individually commented where they appear; `DefWindowProcW`
        // forwards a message verbatim with no pointer arguments of its own.
        unsafe {
            let answer = match msg {
                WM_CREATE => {
                    // SAFETY: this is the just-created owner-thread HWND. Windows
                    // owns notification delivery until its destruction.
                    if let Err(error) =
                        windows::Win32::UI::Controls::RegisterPointerDeviceNotifications(
                            hwnd, false,
                        )
                    {
                        tracing::warn!(%error, "pointer device notifications unavailable");
                    }
                    tracing::debug!(handle = ?hwnd, "WM_CREATE");
                    LRESULT(0)
                }

                WM_CLOSE => {
                    tracing::debug!(handle = ?hwnd, "WM_CLOSE");

                    if let Some(ctx) = ctx {
                        // Ask the handler whether the close proceeds;
                        // `KeepOpen` vetoes it. Asked behind a running method
                        // (a `WM_CLOSE` sent from inside `input`), it is kept
                        // for now and closed later, by the posted
                        // `CLOSE_WINDOW` naming this window, if granted.
                        let close_later = granted_close(hwnd, ctx.identity, ctx.queue.clone());
                        if ctx.delivery.dispatch_close_request(close_later)
                            == flui_platform::CloseDecision::Close
                        {
                            // SAFETY: `ctx` is `hwnd`'s context, borrowed for
                            // this message on its owner thread.
                            ctx.close_granted(hwnd);
                        }
                    } else if super::close::is_retired(hwnd) {
                        // Retired by a granted close: kept past it, or
                        // waiting for its handler's release, whose end
                        // destroys or keeps it. A raster thread may still
                        // present over a kept one.
                        tracing::debug!(
                            handle = ?hwnd,
                            "WM_CLOSE for a retired window: left to its retirement"
                        );
                    } else if let Err(error) = DestroyWindow(hwnd) {
                        tracing::warn!(
                            handle = ?hwnd,
                            ?error,
                            "DestroyWindow failed on WM_CLOSE (no WindowContext)"
                        );
                    }

                    LRESULT(0)
                }

                WM_DESTROY => {
                    tracing::debug!(handle = ?hwnd, "WM_DESTROY");

                    // A close the window granted retired it before destroying
                    // it, and cleared its slot: this finds no context. One
                    // that finds a context is the OS's own (the session
                    // ends, code that is not FLUI's destroys the window),
                    // and the window retires while Win32 destroys it: the
                    // HWND stays valid until `WM_NCDESTROY`.
                    if let Some(ctx) = ctx {
                        // SAFETY: `ctx` is `hwnd`'s context, borrowed for
                        // this message on its owner thread.
                        ctx.retire(hwnd, super::close::NativeEnd::Destroying);
                    }

                    LRESULT(0)
                }

                WM_ERASEBKGND => {
                    // Return 1 to prevent Windows from erasing background
                    // This allows Mica backdrop and other DWM effects to show through
                    tracing::debug!("WM_ERASEBKGND - preventing background erase");
                    LRESULT(1)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_ENTERSIZEMOVE => {
                    if let Some(ctx) = ctx {
                        ctx.step(hwnd, WindowStep::SizeMoveStarted);
                        ctx.in_size_move.set(true);
                        ctx.enter_modal_loop(hwnd);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_EXITSIZEMOVE => {
                    if let Some(ctx) = ctx {
                        ctx.exit_modal_loop();
                        ctx.in_size_move.set(false);
                        ctx.live_resize.set(false);
                        ctx.step(hwnd, WindowStep::SizeMoveEnded);
                        // The snapshot ends the user's placement here: the next
                        // report that changes the placement is not the user's
                        // unless it says so itself (ADR-0343 §2).
                        ctx.publish_window_state(hwnd);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // The user drags the frame: the loop is a live resize from
                // here to its end, and the rectangle keeps the steps and the
                // proportion the application asked for.
                windows::Win32::UI::WindowsAndMessaging::WM_SIZING => {
                    if let Some(ctx) = ctx {
                        if ctx.in_size_move.get() && !ctx.live_resize.replace(true) {
                            ctx.publish_window_state(hwnd);
                        }
                        // SAFETY: for this message `lParam` is the address of
                        // the dragged `RECT`, which the system owns for the call
                        // and reads back; the null check guards a sender that
                        // breaks that contract.
                        if let Some(proposed) = (lparam.0 as *mut RECT).as_mut()
                            && ctx.hold_sizing(hwnd, wparam.0, proposed)
                        {
                            return Some(LRESULT(1));
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_ENTERMENULOOP => {
                    if let Some(ctx) = ctx {
                        ctx.enter_modal_loop(hwnd);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_EXITMENULOOP => {
                    if let Some(ctx) = ctx {
                        ctx.exit_modal_loop();
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_TIMER
                    if wparam.0 == MODAL_DEADLINE_TIMER =>
                {
                    if let Some(ctx) = ctx {
                        ctx.pace_modal_loop(hwnd);
                    }
                    LRESULT(0)
                }

                WM_PAINT => {
                    // The frame follows the scrolling it shows.
                    if let Some(ctx) = ctx {
                        ctx.step(hwnd, WindowStep::Painting);
                    }
                    // SAFETY: `ps` is a stack-local `PAINTSTRUCT`; `&raw mut
                    // ps` gives `BeginPaint` a valid, correctly-sized
                    // out-parameter, and `&raw const ps` below hands the
                    // same (by now filled-in) struct back to the matching
                    // `EndPaint` — every `WM_PAINT` arm here calls
                    // `BeginPaint`/`EndPaint` exactly once, satisfying
                    // Win32's required pairing.
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &raw mut ps);
                    if !hdc.is_invalid() {
                        // No GDI painting here. This HWND carries a D3D12 flip-model
                        // swapchain (wgpu-on-DX12 is always flip-model). Per Microsoft,
                        // GDI and flip model cannot share an HWND: after the first
                        // `Present1`, GDI updates to the window are dropped, and the
                        // contention with the compositor produces visible jitter while
                        // the window is being live-resized. The background comes from the
                        // wgpu clear pass + the scene, not from a GDI fill.
                        //
                        // A minimized window's paint requests its frame too: the
                        // runtime, told the window is not visible, services its tasks
                        // and timers without drawing, and a paint dropped here would
                        // strand the wake that asked for it.
                        if let Some(ctx) = ctx {
                            ctx.request_frame();
                        }
                        let _ = EndPaint(hwnd, &raw const ps);
                    }
                    LRESULT(0)
                }

                WM_SIZE => {
                    use windows::Win32::UI::WindowsAndMessaging::{
                        SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED,
                    };

                    // WM_SIZE dimensions are unsigned16, unlike signed
                    // mouse and WM_MOVE coordinates packed in the same shape.
                    let width = (lparam.0 as u32 & 0xffff).max(1) as i32;
                    let height = hiword(lparam.0 as u32).max(1) as i32;
                    let size_type = wparam.0 as u32;

                    if let Some(ctx) = ctx {
                        let size = Size::new(width, height);
                        let prev_mode = ctx.mode.get();
                        if size_type != SIZE_MINIMIZED {
                            // The pointer the window holds follows its client area.
                            ctx.hold_again(hwnd);
                            ctx.step(hwnd, WindowStep::Resized((width, height)));
                            ctx.update_window_state(|record| WindowRecord {
                                bounds: Bounds {
                                    size: Size::new(
                                        width as f64 / ctx.scale.scale(),
                                        height as f64 / ctx.scale.scale(),
                                    ),
                                    ..record.bounds
                                },
                                ..record
                            });
                        }

                        // Handle the state transition
                        let new_mode = match size_type {
                            SIZE_MINIMIZED => {
                                tracing::debug!(handle = ?hwnd, "window minimized");
                                // A fullscreen window that is minimized comes back
                                // fullscreen, so it keeps what it returns to.
                                let candidate = NativeMode::Minimized {
                                    from_fullscreen: match prev_mode {
                                        NativeMode::Fullscreen { restore_bounds } => {
                                            Some(restore_bounds)
                                        }
                                        NativeMode::Minimized { from_fullscreen } => {
                                            from_fullscreen
                                        }
                                        _ => None,
                                    },
                                };
                                if prev_mode.can_transition_to(&candidate) {
                                    // Preserve the last non-minimized client size.
                                    candidate
                                } else {
                                    tracing::warn!(
                                        handle = ?hwnd,
                                        from = ?prev_mode,
                                        "a minimize from this mode is not a transition; ignored"
                                    );
                                    prev_mode
                                }
                            }
                            SIZE_MAXIMIZED => {
                                tracing::debug!(handle = ?hwnd, width, height, "window maximized");
                                let candidate = NativeMode::Maximized;
                                if prev_mode.is_fullscreen() {
                                    // A fullscreen window covers its monitor as a
                                    // popup; it does not become maximized.
                                    prev_mode
                                } else if prev_mode.can_transition_to(&candidate) {
                                    ctx.last_size.set(size);
                                    candidate
                                } else {
                                    tracing::warn!(
                                        handle = ?hwnd,
                                        from = ?prev_mode,
                                        "a maximize from this mode is not a transition; ignored"
                                    );
                                    prev_mode
                                }
                            }
                            SIZE_RESTORED => {
                                // SIZE_RESTORED covers two cases: a genuine restore FROM
                                // minimized/maximized (a real state change), and a plain
                                // resize while already Normal (same state — NOT a transition).
                                // `can_transition_to` rejects same-state by design, so it must
                                // NOT gate this report: a normal-state resize always resizes.
                                // Gating on it dropped the report (and the last_size update)
                                // on every live-resize drag step.
                                ctx.last_size.set(size);
                                if prev_mode.is_minimized() || prev_mode.is_maximized() {
                                    tracing::debug!(handle = ?hwnd, width, height, "window restored");
                                } else {
                                    tracing::trace!(handle = ?hwnd, width, height, "window resized");
                                }
                                match prev_mode {
                                    // A resize does not end fullscreen.
                                    NativeMode::Fullscreen { .. } => prev_mode,
                                    // Restoring a minimized fullscreen window
                                    // brings fullscreen back.
                                    NativeMode::Minimized {
                                        from_fullscreen: Some(restore_bounds),
                                    } => NativeMode::Fullscreen { restore_bounds },
                                    _ => NativeMode::Normal,
                                }
                            }
                            _ => {
                                // A resize that keeps the mode (`SIZE_MAXSHOW`,
                                // `SIZE_MAXHIDE`).
                                tracing::trace!(handle = ?hwnd, width, height, "window resized");
                                if prev_mode.is_normal() {
                                    ctx.last_size.set(size);
                                }
                                prev_mode
                            }
                        };

                        // Update state
                        ctx.mode.set(new_mode);
                        ctx.publish_window_state(hwnd);
                        // A minimised window sees no release of what was held
                        // in it. After the mode is published, so input code
                        // reads the window as minimised.
                        if size_type == SIZE_MINIMIZED {
                            ctx.step(hwnd, WindowStep::Minimized);
                        }

                        // Every size change but a minimize reaches the metrics.
                        if size_type != SIZE_MINIMIZED {
                            ctx.report_resize(size);
                        }

                        // During a live resize, render synchronously at the new size,
                        // in the same WM_SIZE that reconfigured the surface: Win32 does
                        // not post a WM_PAINT for every drag step of the modal loop, so
                        // the next frame would lag behind the window and the compositor
                        // would stretch the stale one. Any other resize (a command, a
                        // mode change, a DPI change) asks for one frame instead, which
                        // the owner draws after whatever else this turn changes: a
                        // frame per step of a multi-step change would show its
                        // intermediate sizes. A minimized window has nothing to present.
                        if size_type != SIZE_MINIMIZED {
                            if ctx.in_size_move.get() {
                                ctx.request_frame();
                            } else {
                                ctx.redraw_if_still_owned(hwnd);
                            }
                        }
                    }

                    LRESULT(0)
                }

                WM_SHOWWINDOW => {
                    // Only `lParam == 0` — a genuine
                    // `ShowWindow`-driven `WS_VISIBLE` change — is handled:
                    // the nonzero status values (`SW_PARENTCLOSING`,
                    // `SW_OTHERZOOM`, …) describe sibling/parent effects
                    // whose surface impact Win32 reports elsewhere, and
                    // treating them as show/hide would mis-gate a window
                    // that is still composed.
                    if lparam.0 == 0
                        && let Some(ctx) = ctx
                    {
                        let shown = wparam.0 != 0;
                        ctx.update_window_state(|record| WindowRecord {
                            visible: shown,
                            ..record
                        });
                        ctx.publish_window_state(hwnd);
                    }
                    // DefWindowProc continues the ordinary show/hide
                    // processing this message announces. (Already inside
                    // this function's one big SAFETY block, like the other
                    // pass-through arms.)
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_MOVE => {
                    // Packed WM_MOVE coordinates truncate beyond signed16.
                    // Query the actual client origin; retain the message fallback
                    // if Win32 cannot report it for this handle.
                    let mut origin = POINT::default();
                    let (x, y) = if ClientToScreen(hwnd, &raw mut origin).as_bool() {
                        (origin.x, origin.y)
                    } else {
                        (get_x_lparam(lparam), get_y_lparam(lparam))
                    };
                    tracing::debug!(handle = ?hwnd, x, y, "window moved");

                    if let Some(ctx) = ctx {
                        ctx.update_window_state(|record| WindowRecord {
                            bounds: Bounds {
                                origin: Point::new(
                                    x as f64 / ctx.scale.scale(),
                                    y as f64 / ctx.scale.scale(),
                                ),
                                ..record.bounds
                            },
                            ..record
                        });
                        // The pointer the window holds follows its client
                        // area, a lock at its pixel of it.
                        ctx.hold_again(hwnd);
                        ctx.publish_window_state(hwnd);
                    }

                    LRESULT(0)
                }

                WM_DPICHANGED => {
                    if let Some(ctx) = ctx {
                        // SAFETY: for this message `lParam` is the address of
                        // the suggested `RECT` the system owns for the call; the
                        // null check guards a sender that breaks that contract,
                        // and the rectangle is copied out (`RECT: Copy`).
                        let suggested = (lparam.0 as *const RECT).as_ref().copied();
                        ctx.change_dpi(hwnd, hiword(wparam.0 as u32), suggested);
                    }
                    LRESULT(0)
                }

                // A window without a sizing border is held to its size limits
                // here; the system does it for one that has one.
                WM_WINDOWPOSCHANGING => {
                    if let Some(ctx) = ctx {
                        // SAFETY: for this message `lParam` is the address of a
                        // `WINDOWPOS` the system owns for the call.
                        let pending = &mut *(lparam.0 as *mut WINDOWPOS);
                        ctx.clamp_pending_size(hwnd, pending);
                        // A window kept below the others goes back under them
                        // whenever something would raise it (an activation, a
                        // click): Win32 has no level for it. Fullscreen wins
                        // while it lasts (ADR-0354 §3), and the level applies
                        // again when it ends.
                        if ctx.kept_below.get()
                            && !ctx.mode.get().holds_fullscreen()
                            && !pending
                                .flags
                                .contains(windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER)
                        {
                            pending.hwndInsertAfter =
                                windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM;
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_WINDOWPOSCHANGED => {
                    // `WM_MOVE` and `WM_SIZE` come from the default procedure;
                    // the snapshot reads the monitor once the move has landed.
                    let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                    if let Some(ctx) = ctx {
                        // Position, size and z-order changes all land here.
                        ctx.publish_window_state(hwnd);
                        // A window shown on another virtual desktop is shown
                        // cloaked; later changes come through `CloakWatch`.
                        ctx.refresh_cloaked(hwnd);
                    }
                    result
                }

                // Deliveries another thread raised for this window wait in
                // its FIFO; this turn delivers them. Named by identity, like
                // `CLOSE_WINDOW`.
                DELIVER_QUEUED => {
                    if let Some(ctx) = ctx
                        && ctx.identity.halves() == (wparam.0, lparam.0)
                    {
                        ctx.deliveries.taken();
                        let delivery = Arc::clone(&ctx.delivery);
                        delivery.deliver_queued();
                    }
                    LRESULT(0)
                }

                // A close command applied off this thread. It names the
                // window it closes, so a handle reused since is left alone,
                // and it asks no veto, unlike `WM_CLOSE`.
                CLOSE_WINDOW => {
                    if let Some(ctx) = ctx
                        && ctx.identity.halves() == (wparam.0, lparam.0)
                    {
                        // SAFETY: `ctx` is `hwnd`'s context, borrowed for
                        // this message on its owner thread.
                        ctx.close_granted(hwnd);
                    }
                    LRESULT(0)
                }

                // The title and the style bits (resizable, decorated) are part
                // of the published state and change without a size or a move.
                WM_SETTEXT | WM_STYLECHANGED => {
                    let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                    if let Some(ctx) = ctx {
                        ctx.publish_window_state(hwnd);
                    }
                    result
                }

                WM_GETMINMAXINFO => {
                    if let Some(ctx) = ctx {
                        // SAFETY: for this message `lParam` is the address of a
                        // `MINMAXINFO` the system owns for the call.
                        let info = &mut *(lparam.0 as *mut MINMAXINFO);
                        ctx.fill_min_max_info(hwnd, info);
                        LRESULT(0)
                    } else {
                        DefWindowProcW(hwnd, msg, wparam, lparam)
                    }
                }

                // A frame request posted for a window nothing paints: a hidden
                // window never receives `WM_PAINT`. Named by identity, like
                // `CLOSE_WINDOW`.
                REQUEST_FRAME => {
                    if let Some(ctx) = ctx
                        && ctx.identity.halves() == (wparam.0, lparam.0)
                    {
                        ctx.frame_post.taken();
                        ctx.request_frame();
                    }
                    LRESULT(0)
                }

                // A title bar of the application's: the caption and the top
                // border go, the side and bottom borders stay the system's,
                // and a maximized one leaves an auto-hide taskbar the edge it
                // slides in from.
                WM_NCCALCSIZE => {
                    let Some(ctx) = ctx.filter(|ctx| {
                        wparam.0 != 0 && ctx.draws_app_title_bar() && has_caption(hwnd)
                    }) else {
                        return Some(DefWindowProcW(hwnd, msg, wparam, lparam));
                    };
                    let maximized = IsZoomed(hwnd).as_bool();
                    // SAFETY: with a nonzero `wParam`, `lParam` points at the
                    // `NCCALCSIZE_PARAMS` the system owns for this call, valid
                    // and writable until the message returns; the default
                    // procedure reads and writes it before this does.
                    let params = lparam.0 as *mut NCCALCSIZE_PARAMS;
                    let proposed_rect = (*params).rgrc[0];
                    let answer = DefWindowProcW(hwnd, msg, wparam, lparam);
                    let defaulted = edges((*params).rgrc[0]);
                    // The rectangle the window is about to take names the
                    // monitor, not the one it is on: a window restored straight
                    // to maximized is still parked off every monitor, and one
                    // moved to another monitor maximized is still on the first.
                    let auto_hide = if maximized {
                        ctx.auto_hide_edges_at(&proposed_rect)
                    } else {
                        super::chrome::AutoHideEdges::default()
                    };
                    let client = super::chrome::app_title_bar_client(
                        edges(proposed_rect),
                        defaulted,
                        maximized.then(|| super::chrome::Maximized {
                            frame_overhang: frame_thickness(hwnd),
                            auto_hide,
                        }),
                    );
                    (*params).rgrc[0] = RECT {
                        left: client.left,
                        top: client.top,
                        right: client.right,
                        bottom: client.bottom,
                    };
                    answer
                }

                // A frame of the application's: the system asks what lies under
                // the pointer, and the chrome regions the last composited frame
                // handed the window answer, without running the application.
                WM_NCHITTEST => {
                    let defaulted = DefWindowProcW(hwnd, msg, wparam, lparam);
                    let Some(ctx) = ctx.filter(|ctx| {
                        ctx.chrome_answers_the_hit_test() && defaulted.0 == HTCLIENT as isize
                    }) else {
                        return Some(defaulted);
                    };
                    let mut point = POINT {
                        x: get_x_lparam(lparam),
                        y: get_y_lparam(lparam),
                    };
                    let mut client = RECT::default();
                    if !ScreenToClient(hwnd, &raw mut point).as_bool()
                        || GetClientRect(hwnd, &raw mut client).is_err()
                    {
                        return Some(defaulted);
                    }
                    // The top border went with the caption: a band inside the
                    // client stands in for it while the user can resize the
                    // window from it.
                    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                    let top_band = (ctx.draws_app_title_bar()
                        && style & WS_THICKFRAME.0 != 0
                        && !IsZoomed(hwnd).as_bool())
                    .then(|| frame_thickness(hwnd));
                    let Ok(regions) = ctx.input_regions.try_borrow() else {
                        return Some(defaulted);
                    };
                    let code = super::chrome::client_hit_code(
                        regions.sent(),
                        flui_foundation::geometry::DevicePoint::new(point.x, point.y),
                        client.right - client.left,
                        top_band,
                    );
                    LRESULT(code as isize)
                }

                // The system asks, before a `WM_DPICHANGED`, what size the
                // window takes at the DPI it is moving to: the one that keeps
                // its content's logical size, where the system would scale
                // the whole window linearly.
                WM_GETDPISCALEDSIZE => {
                    // SAFETY: for this message `lParam` is the address of the
                    // `SIZE` the system owns for the call, read and written
                    // until the message returns; the null check guards a
                    // sender that breaks that contract.
                    let pending = (lparam.0 as *mut SIZE).as_mut();
                    let answered = match (ctx, pending) {
                        (Some(ctx), Some(pending)) => ctx.dpi_scaled_size(hwnd, wparam.0, pending),
                        _ => false,
                    };
                    LRESULT(isize::from(answered))
                }

                WM_SETCURSOR => {
                    if let Some(ctx) = ctx
                        && (lparam.0 as u32 & 0xffff) == HTCLIENT
                    {
                        // A locked pointer stays hidden whatever the hovered
                        // region asks for.
                        let shown = if ctx.cursor_grab() == flui_platform::CursorGrab::Locked {
                            super::cursor::NativeCursor::Hidden.show()
                        } else {
                            ctx.cursor.borrow().show()
                        };
                        match shown {
                            Ok(()) => return Some(LRESULT(1)),
                            Err(error) => {
                                tracing::warn!(
                                    window_id = ?ctx.window_id,
                                    ?error,
                                    "failed to restore the presentation cursor"
                                );
                            }
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // Activation, apart from focus: a window whose owned popup has
                // the keyboard stays active without the focus. The default
                // procedure then moves the focus as Win32 does.
                WM_ACTIVATE => {
                    if let Some(ctx) = ctx {
                        let active = (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE;
                        if ctx.active.replace(active) != active {
                            ctx.publish_window_state(hwnd);
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_SETFOCUS => {
                    tracing::debug!("Window Focused");

                    if let Some(ctx) = ctx {
                        ctx.update_window_state(|record| WindowRecord {
                            focused: true,
                            ..record
                        });
                        ctx.publish_window_state(hwnd);
                    }

                    LRESULT(0)
                }

                WM_KILLFOCUS => {
                    tracing::debug!("Window Unfocused");

                    if let Some(ctx) = ctx {
                        ctx.update_window_state(|record| WindowRecord {
                            focused: false,
                            ..record
                        });
                        // A grab ends with the focus.
                        ctx.let_pointer_go(hwnd);
                        ctx.step(hwnd, WindowStep::FocusLost);
                        ctx.publish_window_state(hwnd);
                    }

                    LRESULT(0)
                }

                // Every setting change arrives here, the app theme among
                // them ("ImmersiveColorSet"). Re-reading is cheaper than
                // trusting the section name, and only a changed theme
                // repaints the chrome and reaches the callback.
                WM_SETTINGCHANGE => {
                    if let Some(ctx) = ctx {
                        // Double-click time and size are settings too.
                        ctx.step(hwnd, WindowStep::SettingChanged);
                        let appearance = super::backdrop::system_appearance();
                        if ctx.appearance.replace(appearance) != appearance {
                            if ctx.chrome_follows_system.get() {
                                // A build that refuses the attribute keeps its chrome.
                                let _ = super::backdrop::apply_chrome(hwnd, appearance);
                            }
                            ctx.publish_window_state(hwnd);
                        }
                        // The taskbar's settings arrive here too: a bar set to
                        // hide itself, or moved to another edge.
                        ctx.app_bars_may_have_changed(hwnd);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // The monitors changed: what lies at their edges may have,
                // and the pointer the window holds is clipped where the
                // displays now show its client area, or let go.
                WM_DISPLAYCHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.app_bars_may_have_changed(hwnd);
                        ctx.hold_again(hwnd);
                        ctx.publish_window_state(hwnd);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // Unconsumed system keys reach `DefWindowProcW`, which turns
                // an Alt tap, F10 or Alt+letter into `SC_KEYMENU`. FLUI
                // windows have no menu bar, so the default would only park
                // the window in modal menu mode and swallow the next
                // keystroke. Alt+Space (`lparam` is the space character)
                // still opens the window's system menu; every other
                // system command (`SC_CLOSE` from Alt+F4 included) keeps its
                // default.
                WM_SYSCOMMAND
                    if (wparam.0 & 0xFFF0) == SC_KEYMENU as usize && lparam.0 != b' ' as isize =>
                {
                    LRESULT(0)
                }

                // A placement command the user worked (a caption button, the
                // system menu, a double click on the title bar): the window's
                // reports while the default procedure carries it out are the
                // user's (ADR-0343 §2). One this window sent itself for an
                // application's command is marked by its sender and is not.
                // The size-move commands enter the size-move loop, which says
                // so itself.
                WM_SYSCOMMAND if is_user_placement_command(wparam) => {
                    let ctx = ctx?;
                    if ctx.in_app_command.get() {
                        return Some(DefWindowProcW(hwnd, msg, wparam, lparam));
                    }
                    let was = ctx.in_system_command.replace(true);
                    let answer = DefWindowProcW(hwnd, msg, wparam, lparam);
                    ctx.in_system_command.set(was);
                    // The snapshot ends the interaction: the reports the
                    // command made said the user's, this one does not.
                    ctx.publish_window_state(hwnd);
                    answer
                }

                // A message `answers` lists that no arm takes after all (the
                // system menu opened by Alt+Space): the default procedure's.
                _ => return None,
            };
            Some(answer)
        }
    }
}

/// While it lives, the window context does not sample and publish its state;
/// see [`WindowContext::hold_publishing`].
#[must_use = "publishing resumes when the guard drops"]
pub(crate) struct PublishHold<'a>(&'a std::cell::Cell<u32>);

impl Drop for PublishHold<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

/// Get high word from u32
#[inline]
fn hiword(value: u32) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

/// What the owner context knows about a window that Win32 does not say: the
/// mode it tracks from `WM_SIZE` and its own fullscreen toggle, the style a
/// fullscreen window returns to, the limits the app set and the display the
/// window is on.
#[derive(Debug, Clone)]
pub(crate) struct OwnerTracking {
    pub(crate) mode: NativeMode,
    pub(crate) restore_style: u32,
    pub(crate) constraints: SizeConstraints,
    pub(crate) display: Option<flui_platform::DisplayId>,
    /// The displays as last published: what the window's position, and the
    /// position of the rectangle it returns to, are measured on.
    pub(crate) desktop: flui_platform::DisplaySnapshot,
    /// Whether a cloak or a locked session hides the window from the screen.
    pub(crate) occluded: bool,
    /// The user's theme as `WM_SETTINGCHANGE` last read it: never the
    /// dark-mode flag this host sets on the chrome to match (ADR-0222).
    pub(crate) appearance: Brightness,
    /// Whether the last `WM_ACTIVATE` made the window the active one.
    pub(crate) active: bool,
    /// Whether the mouse is over the window, as its input last reported it.
    pub(crate) hovered: bool,
    /// What the window honours and observes.
    pub(crate) capabilities: flui_platform::WindowCapabilities,
    /// The frame the app asked for: a window with a caption reports it, so an
    /// application title bar reads back as itself.
    pub(crate) frame: flui_platform::WindowFrame,
    /// Whether the user is placing the window: inside the size-move loop, or
    /// inside a system command the user worked (ADR-0343); resizing it, from
    /// the loop's first `WM_SIZING`.
    pub(crate) interaction: flui_platform::WindowInteraction,
    /// The grab the window holds.
    pub(crate) cursor_grab: flui_platform::CursorGrab,
    /// Whether the window is kept below the others; whether it is kept above
    /// them is its `WS_EX_TOPMOST` style, which another program may change.
    pub(crate) kept_below: bool,
}

/// Whether a `WM_SYSCOMMAND` is one that places the window: maximize, minimize
/// or restore.
fn is_user_placement_command(wparam: WPARAM) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SC_MAXIMIZE, SC_MINIMIZE};
    let command = wparam.0 & 0xFFF0;
    command == SC_MAXIMIZE as usize
        || command == SC_MINIMIZE as usize
        || command == SC_RESTORE as usize
}

/// Whether the window's style has a caption: a fullscreen window has been
/// stripped of it, and keeps no frame of the application's either.
fn has_caption(hwnd: HWND) -> bool {
    // SAFETY: a plain style query; a stale handle reads as no style.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 };
    style & WS_CAPTION.0 == WS_CAPTION.0
}

/// How thick the sizing frame of `hwnd` is at its DPI, in device pixels: the
/// band a window without a top border resizes from, and how far a maximized
/// window hangs its frame off the monitor.
fn frame_thickness(hwnd: HWND) -> i32 {
    use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows::Win32::UI::WindowsAndMessaging::{SM_CXPADDEDBORDER, SM_CYFRAME};

    // SAFETY: plain queries of a window's DPI and the system's metrics at it.
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        GetSystemMetricsForDpi(SM_CYFRAME, dpi)
            .saturating_add(GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi))
    }
}

/// A `RECT`'s edges as the frame rules read them.
pub(super) const fn edges(rect: RECT) -> super::chrome::Edges {
    super::chrome::Edges {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

/// The window's title as the system holds it, `None` when it cannot be read.
fn window_text(hwnd: HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    // SAFETY: the buffer is a live stack/heap slice the call fills up to its
    // length, terminator included; a stale handle only reports zero.
    unsafe {
        let length = usize::try_from(GetWindowTextLengthW(hwnd)).ok()?;
        let mut buffer = vec![0_u16; length + 1];
        let copied = usize::try_from(GetWindowTextW(hwnd, &mut buffer)).ok()?;
        Some(String::from_utf16_lossy(&buffer[..copied]))
    }
}

/// The window as Win32 reports it now (ADR-0237). `tracking.mode` is the mode
/// the owner context keeps from `WM_SIZE`; everything else is read from `hwnd`.
/// Win32 reports a minimized window at a parking position, so there the
/// outer rectangle is the last known size with no position.
///
/// Rectangles that, over the window's scale, describe no window cost the
/// sample its geometry and nothing else.
pub(crate) fn sample_state(hwnd: HWND, record: WindowRecord, tracking: OwnerTracking) -> Sample {
    let OwnerTracking {
        mode,
        restore_style,
        constraints,
        display,
        desktop,
        occluded,
        appearance,
        active,
        hovered,
        capabilities,
        frame,
        interaction,
        cursor_grab,
        kept_below,
    } = tracking;
    // SAFETY: a plain style query; a stale handle reads as no style.
    let topmost = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
        & windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST.0
        != 0;
    let level = if topmost {
        flui_platform::WindowLevel::AlwaysOnTop
    } else if kept_below {
        flui_platform::WindowLevel::AlwaysOnBottom
    } else {
        flui_platform::WindowLevel::Normal
    };
    let WindowRecord {
        bounds: Bounds { size: cached, .. },
        scale_factor,
        visible,
        focused,
    } = record;
    let scale = scale_factor.get();
    let mut window = RECT::default();
    let mut client = RECT::default();
    // SAFETY: both RECTs are stack locals and `&raw mut` gives each call a
    // valid, correctly sized out-parameter; an `Err` (stale handle) leaves
    // them defaulted and is answered with the cached size below.
    let (have_window, have_client, style) = unsafe {
        (
            GetWindowRect(hwnd, &raw mut window).is_ok(),
            GetClientRect(hwnd, &raw mut client).is_ok(),
            // A fullscreen window has been stripped of its frame; whether the
            // user can resize it or sees a title bar is the style it returns to.
            if mode.holds_fullscreen() {
                restore_style
            } else {
                GetWindowLongPtrW(hwnd, GWL_STYLE) as u32
            },
        )
    };
    // The rectangle the window returns to: Win32's own for a maximized or
    // minimized window (`rcNormalPosition`), and for a fullscreen one the
    // rectangle it was given on entering, taken from the same source.
    let restore = match mode {
        NativeMode::Fullscreen { restore_bounds }
        | NativeMode::Minimized {
            from_fullscreen: Some(restore_bounds),
        } => Some(restore_bounds),
        NativeMode::Maximized | NativeMode::Minimized { .. } => restore_rectangle(hwnd),
        NativeMode::Normal => None,
    };
    let geometry = sampled_geometry(
        DeviceRects {
            window: (have_window && !mode.is_minimized()).then_some(window),
            client: have_client.then_some(client),
            restore,
            cached,
            minimized: mode.is_minimized(),
            scale: scale_factor,
        },
        display,
        &desktop,
    );
    Sample::read(geometry, |state| {
        let state = state
            .with_mode(mode_of(mode.flags()))
            .with_visible(visible)
            .with_title(window_text(hwnd))
            .with_constraints(constraints)
            .with_resizable(style & WS_THICKFRAME.0 != 0)
            .with_frame(reported_frame(style & WS_CAPTION.0 == WS_CAPTION.0, frame))
            .with_display(display);
        ReportedFacts {
            // The client area in device pixels at the window's scale; a minimized
            // window's empty client area is not drawable and keeps the last.
            metrics: have_client
                .then(|| {
                    flui_foundation::geometry::WindowMetrics::try_new(
                        Size::new(
                            client.right.checked_sub(client.left)?,
                            client.bottom.checked_sub(client.top)?,
                        ),
                        scale,
                    )
                    .ok()
                })
                .flatten(),
            brightness: appearance,
            // A desktop window has no safe area, and Win32 never suspends one.
            insets: flui_platform::WindowInsets::ZERO,
            execution: flui_platform::WindowExecutionState::Running,
            // Win32 tells of a cloak and a locked session; a window covered by
            // others it does not report.
            occluded,
            hovered,
            focus: flui_platform::WindowFocus::reported(focused, active),
            capabilities,
            interaction,
            cursor_grab,
            level,
        }
        .onto(state)
    })
}

/// The rectangle a window leaving fullscreen returns to, in screen device
/// pixels, and how far the work area of the monitor it lands on starts from
/// that monitor's origin (what `WINDOWPLACEMENT` measures from). `saved`
/// stands where it reaches a monitor that is there; when none is (the monitor
/// it entered fullscreen on was disconnected), it is fitted to the work area
/// of the nearest one (`mode::fit_within`), so the window never comes back
/// off every screen.
fn returning_rectangle(saved: Bounds<i32>) -> (RECT, (i32, i32)) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO,
        MonitorFromRect,
    };

    let as_rect = |bounds: Bounds<i32>| RECT {
        left: bounds.origin.x,
        top: bounds.origin.y,
        right: bounds.origin.x.saturating_add(bounds.size.width),
        bottom: bounds.origin.y.saturating_add(bounds.size.height),
    };
    let rect = as_rect(saved);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `rect` and `info` are stack locals, `info` with `cbSize` set;
    // the calls read the one and fill the other and keep neither.
    let (on_a_monitor, found) = unsafe {
        let on_a_monitor = !MonitorFromRect(&raw const rect, MONITOR_DEFAULTTONULL).is_invalid();
        let nearest = MonitorFromRect(&raw const rect, MONITOR_DEFAULTTONEAREST);
        (
            on_a_monitor,
            GetMonitorInfoW(nearest, &raw mut info).as_bool(),
        )
    };
    if !found {
        return (rect, (0, 0));
    }
    let origin = (
        info.rcWork.left - info.rcMonitor.left,
        info.rcWork.top - info.rcMonitor.top,
    );
    if on_a_monitor {
        return (rect, origin);
    }
    let work = Bounds {
        origin: Point::new(info.rcWork.left, info.rcWork.top),
        size: Size::new(
            info.rcWork.right - info.rcWork.left,
            info.rcWork.bottom - info.rcWork.top,
        ),
    };
    (as_rect(super::mode::fit_within(saved, work)), origin)
}

/// The mode a window leaving fullscreen ends in: the one the application's
/// command names. A borderless fullscreen window has no way out of its own
/// (no frame, no key of the host's), so every exit is a command that names
/// where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullscreenExit {
    /// Back at the rectangle it had before fullscreen, with its frame.
    Normal,
    /// Maximized, with the rectangle it had before fullscreen as the one it
    /// is restored to.
    Maximized,
}

impl WindowsWindow {
    /// Take the fullscreen window `hwnd` out of fullscreen into `exit`, from
    /// fullscreen or from minimized out of fullscreen: its style comes back
    /// (keeping whether it is shown now), and the rectangle it had before
    /// fullscreen is the one it shows (`Normal`) or the one it is restored
    /// to (`Maximized`). That rectangle is first fitted to a monitor that is
    /// there: the one it was on may have been disconnected meanwhile.
    ///
    /// The window publishes once, in the mode it ends in: a subscriber never
    /// sees it normal on its way to maximized, nor a minimized window back in
    /// fullscreen on its way out, and no report says the user placed it (no
    /// system command is sent). A window that holds no fullscreen is left
    /// alone.
    ///
    /// Only effective on the window's own thread: elsewhere the context gate
    /// refuses it with a warning.
    pub(crate) fn leave_fullscreen_for_hwnd(hwnd: HWND, exit: FullscreenExit) {
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongPtrW, SW_SHOWMAXIMIZED, SW_SHOWNORMAL, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW,
            SetWindowPlacement, SetWindowPos, WS_VISIBLE,
        };

        let left = with_window_context(hwnd, "leave_fullscreen", |ctx| {
            let Some(restore_bounds) = ctx.mode.get().fullscreen_bounds() else {
                return;
            };
            let hold = ctx.hold_publishing();
            let (normal, work_origin) = returning_rectangle(restore_bounds);
            // SAFETY: `IsIconic`/`GetWindowLongPtrW`/`SetWindowLongPtrW`/
            // `SetWindowPos`/`SetWindowPlacement` take `hwnd` and plain values
            // or a pointer to the fully initialized stack local `placement`;
            // against a dead `hwnd` each fails with an error return. The
            // window can die inside them (`SetWindowPos` re-enters
            // `window_proc`, whose callbacks may close it), and `ctx` stays
            // valid regardless: the gate's `ContextGuard` defers the context's
            // free past this closure (`ContextLedger`).
            unsafe {
                let iconic = IsIconic(hwnd).as_bool();
                // Restore the style the window had, but not its visibility:
                // a window entered hidden and shown since must stay shown. A
                // minimized window keeps its minimized bit until the placement
                // below takes it out.
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                let kept = style & (WS_VISIBLE.0 | WS_MINIMIZE.0);
                let restore_style = (ctx.restore_style.get() & !WS_VISIBLE.0) | kept;
                SetWindowLongPtrW(hwnd, GWL_STYLE, restore_style as isize);

                // The window is normal from here on, so the size limits the
                // app set (also while it was fullscreen) apply to the rectangle
                // it returns to, and the `WM_SIZE` the placement below sends
                // reads a normal window: one restored from minimized is no
                // longer the fullscreen window it would come back as.
                ctx.mode.set(NativeMode::Normal);
                if exit == FullscreenExit::Normal && !iconic {
                    if let Err(error) = SetWindowPos(
                        hwnd,
                        None,
                        normal.left,
                        normal.top,
                        normal.right - normal.left,
                        normal.bottom - normal.top,
                        SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
                    ) {
                        tracing::warn!(handle = ?hwnd, ?error, "the window left fullscreen in place");
                    }
                } else {
                    // The frame is measured again for the restored style, then
                    // one placement shows the window in the mode it ends in,
                    // with the rectangle it had before fullscreen as its normal
                    // one: it shows neither that rectangle on the way to
                    // maximized nor fullscreen on the way out of minimized.
                    if let Err(error) = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    ) {
                        tracing::warn!(handle = ?hwnd, ?error, "the restored frame was not measured");
                    }
                    let show = match exit {
                        FullscreenExit::Normal => SW_SHOWNORMAL,
                        FullscreenExit::Maximized => SW_SHOWMAXIMIZED,
                    };
                    let placed = super::set::placement_of(hwnd).and_then(|mut placement| {
                        placement.showCmd = show.0 as u32;
                        placement.rcNormalPosition = to_workspace(normal, work_origin);
                        SetWindowPlacement(hwnd, &raw const placement)
                    });
                    if let Err(error) = placed {
                        tracing::warn!(handle = ?hwnd, ?error, ?exit, "the window left fullscreen in place");
                    }
                }
            }
            drop(hold);
            ctx.publish_window_state(hwnd);
        });
        if left.is_none() {
            tracing::warn!(handle = ?hwnd, "cannot leave fullscreen: no usable window context");
        }
    }

    /// Make `hwnd` borderless over its whole monitor. The style and the normal
    /// rectangle it returns to are kept first; a visible maximized or
    /// minimized window is restored on the way in, by a system command the
    /// window marks as the application's (ADR-0343 §2). The size limits do
    /// not apply while it is fullscreen. Publishes once, fullscreen. A window
    /// already fullscreen is left alone.
    ///
    /// Only effective on the window's own thread: elsewhere the context gate
    /// refuses it with a warning.
    pub(crate) fn enter_fullscreen_for_hwnd(hwnd: HWND) {
        use windows::Win32::{
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
                MonitorFromWindow,
            },
            UI::WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, GetWindowRect, HWND_TOP, SWP_FRAMECHANGED,
                SWP_NOACTIVATE, SetWindowLongPtrW, SetWindowPos, WS_POPUP, WS_VISIBLE,
            },
        };

        let entered = with_window_context(hwnd, "enter_fullscreen", |ctx| {
            let current_mode = ctx.mode.get();
            if current_mode.is_fullscreen() {
                return;
            }
            let hold = ctx.hold_publishing();

            // SAFETY: `GetWindowRect`/`GetWindowLongPtrW`/`SetWindowLongPtrW`/
            // `SetWindowPos`/`MonitorFromWindow`/`GetMonitorInfoW` below all
            // take `hwnd` or a `&raw mut` to a stack-local out-parameter
            // whose size matches what each call expects; against a dead
            // `hwnd` each fails with an error return. The window can die
            // inside them (`SetWindowPos` and the restore re-enter
            // `window_proc`, whose callbacks may close it), and `ctx` stays
            // valid regardless: the gate's `ContextGuard` defers the context's
            // free past this closure (`ContextLedger`).
            unsafe {
                // Get current window rect
                let mut rect = RECT::default();
                if let Err(error) = GetWindowRect(hwnd, &raw mut rect) {
                    tracing::warn!(
                        handle = ?hwnd,
                        ?error,
                        "GetWindowRect failed entering fullscreen; restore bounds will be zeroed"
                    );
                }

                // Save the style the window returns to. Whether it is
                // maximized or minimized is its state, not its style: that
                // is restored below, so it never comes back from fullscreen
                // flagged as something it no longer is.
                let current_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                ctx.restore_style
                    .set(current_style & !(WS_MAXIMIZE.0 | WS_MINIMIZE.0));

                // Save the bounds the window returns to: its normal
                // rectangle, which is not its current one when it was
                // maximized (or minimized) on the way in.
                let restore_bounds = restore_rectangle(hwnd).unwrap_or(Bounds {
                    origin: Point::new(rect.left, rect.top),
                    size: Size::new(rect.right - rect.left, rect.bottom - rect.top),
                });

                // The monitor the window is on. A minimized window is parked
                // off-screen, so it goes to the monitor of its normal rectangle.
                let iconic = IsIconic(hwnd).as_bool();
                let monitor = if iconic {
                    let normal = RECT {
                        left: restore_bounds.origin.x,
                        top: restore_bounds.origin.y,
                        right: restore_bounds
                            .origin
                            .x
                            .saturating_add(restore_bounds.size.width),
                        bottom: restore_bounds
                            .origin
                            .y
                            .saturating_add(restore_bounds.size.height),
                    };
                    MonitorFromRect(&raw const normal, MONITOR_DEFAULTTONEAREST)
                } else {
                    MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)
                };
                // Fullscreen starts from the normal state: a visible maximized
                // or minimized window is restored first. The restore is the
                // application's command, so its sender marks it as such.
                if (iconic || IsZoomed(hwnd).as_bool()) && IsWindowVisible(hwnd).as_bool() {
                    let was = ctx.in_app_command.replace(true);
                    SendMessageW(
                        hwnd,
                        WM_SYSCOMMAND,
                        Some(WPARAM(SC_RESTORE as usize)),
                        Some(LPARAM(0)),
                    );
                    ctx.in_app_command.set(was);
                }
                let mut monitor_info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if let Err(error) = GetMonitorInfoW(monitor, &raw mut monitor_info).ok() {
                    tracing::warn!(
                        handle = ?hwnd,
                        ?error,
                        "GetMonitorInfoW failed entering fullscreen; monitor rect will be zeroed"
                    );
                }

                let monitor_rect = monitor_info.rcMonitor;

                // Set borderless style, shown only if the window is.
                let shown = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_VISIBLE.0;
                let fullscreen_style = WS_POPUP.0 | shown;
                SetWindowLongPtrW(hwnd, GWL_STYLE, fullscreen_style as isize);

                // Position window to cover entire monitor. The size limits do not
                // apply: a fullscreen window covers its monitor whatever they say.
                ctx.limits_waived.set(true);
                if let Err(error) = SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    monitor_rect.left,
                    monitor_rect.top,
                    monitor_rect.right - monitor_rect.left,
                    monitor_rect.bottom - monitor_rect.top,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE,
                ) {
                    tracing::warn!(handle = ?hwnd, ?error, "SetWindowPos (enter fullscreen) failed");
                }
                ctx.limits_waived.set(false);
                ctx.mode.set(NativeMode::Fullscreen { restore_bounds });
            }
            drop(hold);
            ctx.publish_window_state(hwnd);
        });
        if entered.is_none() {
            tracing::warn!(handle = ?hwnd, "cannot enter fullscreen: no usable window context");
        }
    }
}

/// `extent` at a scale `ratio` times the one it was measured at, or `None`
/// when that is not a device value a window command may name.
fn rescaled(extent: i32, ratio: f64) -> Option<i32> {
    checked_logical_to_device(f64::from(extent), ratio)
}

impl WindowContext {
    /// Ask for a frame of `hwnd` after a callback ran, unless that callback
    /// closed the window: the handle may by then name a window opened in its
    /// place, which this context's slot no longer is.
    pub(crate) fn redraw_if_still_owned(&self, hwnd: HWND) {
        if self.still_owns(hwnd) {
            self.frame_post.request();
        }
    }

    /// Whether `hwnd` is still this context's window: a callback that closed
    /// it may have let a window opened in its place take the handle.
    fn still_owns(&self, hwnd: HWND) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GWLP_USERDATA, GetWindowLongPtrW};

        // SAFETY: a pointer-sized read of the window's userdata slot; nothing
        // is dereferenced.
        let slot = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
        slot == std::ptr::from_ref(self) as isize
    }

    /// `WM_DPICHANGED`: the window is now at `dpi`, and the system suggests
    /// the rectangle `suggested` for it.
    ///
    /// The message is the sender's word: any process may send one, with any
    /// number in it. A DPI of zero is no DPI, and the window keeps its scale
    /// and its rectangle.
    pub(crate) fn change_dpi(&self, hwnd: HWND, dpi: u16, suggested: Option<RECT>) {
        use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

        // The DPI goes through the hosts' one scale rule before it is stored:
        // any process can send the message, with any DPI, in a loop. One that
        // is no ratio a screen can have is refused, reported once per run of
        // them for this window (each report is a call into the subscriber,
        // with nothing borrowed here), and the window keeps its scale.
        let mut streak = self.dpi_streak.get();
        let told = streak.admit(
            crate::owner::windows::WindowDpi::try_new(u32::from(dpi)),
            crate::owner::windows::dpi_scale(u32::from(dpi)),
            "WM_DPICHANGED",
        );
        self.dpi_streak.set(streak);
        let dpi = match told {
            Ok(dpi) => dpi,
            Err(refused) => {
                if let Some(refused) = refused {
                    refused.report();
                }
                return;
            }
        };
        let old_dpi = self.scale.dpi();
        self.scale.rescale(dpi);
        let new_scale = self.scale.scale();
        tracing::info!(handle = ?hwnd, scale = new_scale, "window scale changed");
        // The display's scale changed, so its snapshot entry did.
        self.displays.invalidate();
        self.step(hwnd, WindowStep::Rescaled);
        // The scale and the rectangle change together: a subscriber sees the
        // window once, at its new size.
        let hold = self.hold_publishing();
        let ratio = self.scale.ratio();
        self.update_window_state(|record| WindowRecord {
            scale_factor: ratio,
            ..record
        });
        self.apply_dpi_rectangle(hwnd, suggested, old_dpi, dpi.get());
        // The state callback the rectangle's `WM_SIZE` ran may have closed the
        // window: its resize and frame are then not for whatever window has
        // the handle now.
        if !self.still_owns(hwnd) {
            return;
        }
        drop(hold);
        self.publish_window_state(hwnd);

        // When the rectangle kept the client size in device pixels, or the
        // move failed, no `WM_SIZE` brought the new metrics.
        let mut client = RECT::default();
        // SAFETY: `client` is a stack local out-parameter.
        let measured = unsafe { GetClientRect(hwnd, &raw mut client) }.is_ok();
        if measured && !self.mode.get().is_minimized() {
            let size = Size::new(
                (client.right - client.left).max(1),
                (client.bottom - client.top).max(1),
            );
            if self.last_resize.get() != Some((size, new_scale)) {
                self.update_window_state(|record| WindowRecord {
                    bounds: Bounds {
                        size: Size::new(
                            f64::from(size.width) / new_scale,
                            f64::from(size.height) / new_scale,
                        ),
                        ..record.bounds
                    },
                    ..record
                });
                self.report_resize(size);
                self.redraw_if_still_owned(hwnd);
            }
        }
        // The field's screen rectangles follow the new scale and rectangle.
        self.step(hwnd, WindowStep::ScaleApplied);
    }

    /// `WM_GETDPISCALEDSIZE`: the outer size the window takes at `dpi`, its
    /// pending size `pending` now, written back into `pending`. The size keeps
    /// the content's logical size ([`rescale`](super::rescale)), where the
    /// system's linear scaling would not.
    ///
    /// Answers whether it wrote one. A window that is not in its normal mode
    /// answers none: a maximized or fullscreen window covers its monitor at
    /// any DPI (the fullscreen one's way back is rescaled with
    /// [`apply_dpi_rectangle`](Self::apply_dpi_rectangle)), and a minimized
    /// one has no size on screen. Neither does a DPI no screen has, which the
    /// `WM_DPICHANGED` that follows refuses and reports.
    fn dpi_scaled_size(&self, hwnd: HWND, dpi: usize, pending: &mut SIZE) -> bool {
        use crate::owner::windows::WindowDpi;

        if !self.mode.get().is_normal() {
            return false;
        }
        let Some(to) = u32::try_from(dpi).ok().and_then(WindowDpi::try_new) else {
            return false;
        };
        // SAFETY: a plain style query of the live window the message was sent to.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        let outer = super::limits::PixelSize::new(pending.cx, pending.cy);
        let mut kept = self.kept_logical.get();
        let Some(rescaled) = self.rescaled_outer(
            hwnd,
            style,
            outer,
            (self.scale.dpi(), to.get()),
            kept.content,
        ) else {
            return false;
        };
        kept.content = Some(rescaled.logical);
        self.kept_logical.set(kept);
        pending.cx = rescaled.outer.width;
        pending.cy = rescaled.outer.height;
        true
    }

    /// The outer size of a window of `style` that is `outer` at the first DPI
    /// of `dpis`, once it is at the second with its content at the same
    /// logical size (`kept` where the pixels are still its); the frame is
    /// measured for `style` at each DPI. `None` where either DPI is no
    /// screen's, the frame cannot be measured, or the content is no size.
    fn rescaled_outer(
        &self,
        hwnd: HWND,
        style: u32,
        outer: super::limits::PixelSize,
        (from, to): (u32, u32),
        kept: Option<Size<f64>>,
    ) -> Option<super::rescale::Rescaled> {
        use super::rescale::{FramedScale, rescaled_outer};

        // SAFETY: a plain style query of the live window.
        let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let app_title_bar = self.draws_app_title_bar();
        let framed = |dpi: u32| {
            let ratio = crate::owner::windows::WindowDpi::try_new(dpi)?.ratio();
            let (width, height) =
                super::set::outer_size(style, ex_style, dpi, (0, 0), app_title_bar).ok()?;
            Some(FramedScale {
                ratio,
                frame: super::limits::PixelSize::new(width, height),
            })
        };
        rescaled_outer(outer, framed(from)?, framed(to)?, kept)
    }

    /// Give the window the rectangle its new DPI asks for; it was at `from`
    /// DPI and is at `to` now.
    ///
    /// A window that is not fullscreen takes the rectangle the system
    /// suggests, which for a normal window is the size it answered
    /// `WM_GETDPISCALEDSIZE` with. A fullscreen window covers its monitor
    /// again instead, which the suggestion, scaled from its old size, need not
    /// do; the rectangle it returns to is rescaled with it, its content kept
    /// at the same logical size under the style it returns to.
    pub(crate) fn apply_dpi_rectangle(
        &self,
        hwnd: HWND,
        suggested: Option<RECT>,
        from: u32,
        to: u32,
    ) {
        use windows::Win32::{
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            },
            UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos},
        };

        let mode = self.mode.get();
        if let Some(bounds) = mode.fullscreen_bounds() {
            let outer = super::limits::PixelSize::new(bounds.size.width, bounds.size.height);
            // The frame is the one the window returns to; a way back whose
            // content is no size is scaled as the system would scale it.
            let ratio = f64::from(to) / f64::from(from);
            let mut kept = self.kept_logical.get();
            let rescaled_bounds = self.rescaled_outer(
                hwnd,
                self.restore_style.get(),
                outer,
                (from, to),
                kept.restore,
            );
            kept.restore = rescaled_bounds.map(|rescaled| rescaled.logical);
            self.kept_logical.set(kept);
            let size = rescaled_bounds
                .map(|rescaled| (rescaled.outer.width, rescaled.outer.height))
                .or_else(|| {
                    rescaled(bounds.size.width, ratio).zip(rescaled(bounds.size.height, ratio))
                });
            if let Some((width, height)) = size {
                self.mode.set(mode.with_fullscreen_bounds(Bounds {
                    origin: bounds.origin,
                    size: Size::new(width, height),
                }));
            }
        }
        let target = if mode.is_fullscreen() {
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            // SAFETY: `monitor` is a stack local with `cbSize` set; the
            // monitor handle comes from the live window.
            let read = unsafe {
                GetMonitorInfoW(
                    MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                    &raw mut monitor,
                )
            };
            read.as_bool().then_some(monitor.rcMonitor)
        } else {
            suggested
        };
        let Some(rect) = target else {
            return;
        };
        let (Some(width), Some(height)) = (
            rect.right.checked_sub(rect.left),
            rect.bottom.checked_sub(rect.top),
        ) else {
            return;
        };
        // A fullscreen window covers its monitor whatever its limits say.
        self.limits_waived.set(mode.is_fullscreen());
        // SAFETY: plain integer and flag arguments on the live window, with
        // `None` as the documented no-op z-order handle.
        let moved = unsafe {
            SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        self.limits_waived.set(false);
        if let Err(error) = moved {
            tracing::warn!(handle = ?hwnd, ?error, "SetWindowPos (DPI change) failed");
        }
    }
}

/// The logical sizes a window keeps across scale changes
/// (`rescale::rescaled_outer`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct KeptLogical {
    /// Its content's.
    pub(crate) content: Option<Size<f64>>,
    /// The content's of the rectangle a fullscreen window returns to.
    pub(crate) restore: Option<Size<f64>>,
}

/// What hides a window whose surface is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Occlusion {
    /// The desktop window manager cloaks the window.
    pub(crate) cloaked: bool,
    /// The user's session is locked.
    pub(crate) session_locked: bool,
}

/// What occludes a shown window in its snapshot.
pub(crate) struct Visibility {
    occlusion: Cell<Occlusion>,
}

impl Visibility {
    /// A window in a session that is locked now or not.
    pub(crate) fn new() -> Self {
        Self {
            occlusion: Cell::new(Occlusion {
                cloaked: false,
                session_locked: session_locked_now(),
            }),
        }
    }

    /// Whether something hides the window whatever its surface.
    pub(crate) fn occluded(&self) -> bool {
        let occlusion = self.occlusion.get();
        occlusion.cloaked || occlusion.session_locked
    }
}

impl WindowContext {
    /// The session locked (`true`) or unlocked, as the lifecycle's hook tells
    /// the window ([`WindowStep::SessionLocked`]).
    pub(crate) fn set_session_locked(&self, locked: bool) {
        let occlusion = self.visibility.occlusion.get();
        self.visibility.occlusion.set(Occlusion {
            session_locked: locked,
            ..occlusion
        });
        self.settle_visibility();
    }

    /// Read whether the desktop window manager cloaks the window.
    pub(crate) fn refresh_cloaked(&self, hwnd: HWND) {
        let mut cloaked = 0_u32;
        // SAFETY: `cloaked` is a live `u32`, the size `DWMWA_CLOAKED` writes,
        // passed with that size.
        let read = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast(),
                size_of::<u32>() as u32,
            )
        };
        if read.is_err() {
            return;
        }
        let occlusion = self.visibility.occlusion.get();
        self.visibility.occlusion.set(Occlusion {
            cloaked: cloaked != 0,
            ..occlusion
        });
        self.settle_visibility();
    }

    fn settle_visibility(&self) {
        // A cloak or a lock occludes a shown window in its snapshot: read it
        // again and publish it, which says nothing when nothing changed.
        self.snapshot.invalidate();
        self.delivery.publish_state();
    }
}

/// Whether the session this process runs in is locked: a window opened while
/// it is gets no lock message. Unknown counts as unlocked.
fn session_locked_now() -> bool {
    use windows::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory,
        WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
    };

    let mut buffer = windows::core::PWSTR::null();
    let mut bytes = 0_u32;
    // SAFETY: both out-parameters are live locals; on success the system
    // allocates `buffer`, freed below with `WTSFreeMemory`.
    let queried = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &raw mut buffer,
            &raw mut bytes,
        )
    };
    if queried.is_err() || buffer.is_null() {
        return false;
    }
    let locked = (bytes as usize >= size_of::<WTSINFOEXW>()).then(|| {
        // SAFETY: for `WTSSessionInfoEx` the buffer holds a `WTSINFOEXW`, at
        // least as many bytes as checked above, read without assuming the
        // allocation's alignment; level 1 is the union's only member.
        let info = unsafe { std::ptr::read_unaligned(buffer.0.cast::<WTSINFOEXW>()) };
        info.Level == 1
            // SAFETY: `Level == 1` names the union's active member, which is
            // also its only one.
            && unsafe { info.Data.WTSInfoExLevel1 }.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK
    });
    // SAFETY: the buffer the query allocated, freed once.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    locked.unwrap_or(false)
}

/// Wake deadlines while the system runs a modal loop for the window (moving
/// or sizing it, its menus): a timer of the window delivers what is due while
/// the main loop waits (`owner::windows::modal_loop`).
impl WindowContext {
    /// A modal loop for the window starts.
    pub(crate) fn enter_modal_loop(&self, hwnd: HWND) {
        self.modal_pacing
            .enter(|| Some(PaceClock::WindowTimer(hwnd)));
    }

    /// A modal loop for the window ends.
    pub(crate) fn exit_modal_loop(&self) {
        self.modal_pacing.exit();
    }

    /// The pacing timer fired: deliver what is due and set it for the next
    /// delivery, or stop a timer that fired as its loop ended. The delivery
    /// runs user code; the pacing is cloned out, since that code may close
    /// the window.
    pub(crate) fn pace_modal_loop(&self, hwnd: HWND) {
        let pacing = Rc::clone(&self.modal_pacing);
        if !pacing.tick() {
            kill_window_timer(hwnd);
        }
    }
}

/// What is still owed of a window's first show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirstShow {
    /// Nothing: the window has been shown, or was hidden by the app.
    Done,
    /// The window waits for an explicit show, which gives it this mode.
    OnShow(WindowMode),
    /// The window waits for the embedder's first presented frame, which gives
    /// it this mode. Until then it is hidden for the system and visible for
    /// the runtime, whose frames must run to earn the reveal.
    AfterFirstFrame(WindowMode),
}

impl FirstShow {
    /// The mode the first show gives the window; `None` once it has happened.
    pub(crate) fn mode(self) -> Option<WindowMode> {
        match self {
            Self::Done => None,
            Self::OnShow(mode) | Self::AfterFirstFrame(mode) => Some(mode),
        }
    }

    /// Whether the first presented frame shows the window.
    pub(crate) fn awaits_frame(self) -> bool {
        matches!(self, Self::AfterFirstFrame(_))
    }

    /// The same wait with another mode; nothing to change once shown.
    pub(crate) fn staged(self, mode: WindowMode) -> Self {
        match self {
            Self::Done => Self::Done,
            Self::OnShow(_) => Self::OnShow(mode),
            Self::AfterFirstFrame(_) => Self::AfterFirstFrame(mode),
        }
    }

    /// An explicit show still gives the mode; the first frame no longer does.
    pub(crate) fn without_frame_wait(self) -> Self {
        match self {
            Self::AfterFirstFrame(mode) => Self::OnShow(mode),
            other => other,
        }
    }
}

/// The style bits that make up a window's frame: a title bar and borders when
/// it is decorated, a bare popup when it is not; and, when resizable, the
/// sizing border and the maximize box. A window without a frame cannot be
/// resized by its user, so `resizable` has no bit to set on a popup.
pub(crate) fn frame_style(decorated: bool, resizable: bool) -> u32 {
    if !decorated {
        WS_POPUP.0
    } else if resizable {
        WS_OVERLAPPEDWINDOW.0
    } else {
        WS_OVERLAPPEDWINDOW.0 & !(WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0)
    }
}

/// The style a window has after its frame changes to `decorated`/`resizable`:
/// `style` with its frame bits replaced and everything else (visible,
/// maximized, minimized, clip bits) kept.
pub(crate) fn restyled(style: u32, decorated: bool, resizable: bool) -> u32 {
    let frame_bits = WS_OVERLAPPEDWINDOW.0 | WS_POPUP.0;
    (style & !frame_bits) | frame_style(decorated, resizable)
}

/// The show command that gives a window that has never been shown the mode it
/// opened in. Fullscreen is entered while the window is still hidden, so it
/// is shown like a normal one.
pub(crate) fn first_show_command(
    mode: WindowMode,
) -> windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD {
    match mode {
        WindowMode::Maximized => SW_SHOWMAXIMIZED,
        WindowMode::Minimized => SW_SHOWMINIMIZED,
        _ => SW_SHOW,
    }
}

/// A rectangle in screen coordinates as the workspace coordinates
/// `WINDOWPLACEMENT::rcNormalPosition` uses: measured from the work area,
/// which a taskbar on the top or left edge moves away from the monitor's origin
/// by `origin`.
///
/// Saturating: a rectangle at the edge of the range stays there instead of
/// wrapping to the other side of the desktop.
pub(crate) fn to_workspace(rect: RECT, origin: (i32, i32)) -> RECT {
    RECT {
        left: rect.left.saturating_sub(origin.0),
        top: rect.top.saturating_sub(origin.1),
        right: rect.right.saturating_sub(origin.0),
        bottom: rect.bottom.saturating_sub(origin.1),
    }
}

/// The inverse of [`to_workspace`].
pub(crate) fn to_screen(rect: RECT, origin: (i32, i32)) -> RECT {
    RECT {
        left: rect.left.saturating_add(origin.0),
        top: rect.top.saturating_add(origin.1),
        right: rect.right.saturating_add(origin.0),
        bottom: rect.bottom.saturating_add(origin.1),
    }
}

impl WindowContext {
    /// Before a window is first shown in `mode`: a window shown minimized is
    /// already minimized when the system announces the show, so the show does
    /// not read as the window appearing.
    pub(crate) fn begin_first_show(&self, mode: WindowMode) {
        if mode == WindowMode::Minimized {
            self.mode.set(NativeMode::Minimized {
                from_fullscreen: None,
            });
        }
    }

    /// After a window was first shown, or restored and hidden again: publish
    /// what the window is now. A window that came up minimized has the
    /// minimized mode in its snapshot, whatever the first show asked for (a
    /// launcher can choose the first show state).
    pub(crate) fn settle_shown(&self) {
        self.delivery.publish_state();
    }

    /// Note that the app asked for the window to be shown, so a restore that
    /// hides it again afterwards can tell it was overtaken.
    pub(crate) fn note_show_request(&self) {
        self.show_requests
            .set(self.show_requests.get().wrapping_add(1));
    }
}

impl WindowContext {
    /// The app's size limits as outer sizes at the window's current DPI, or
    /// `None` when there are none or the window is fullscreen (it covers its
    /// monitor whatever the limits say).
    fn outer_limits(&self, hwnd: HWND) -> Option<super::limits::TrackLimits> {
        if self.mode.get().holds_fullscreen() || self.limits_waived.get() {
            return None;
        }
        // SAFETY: a plain query of the live window the message was sent to.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        self.limits_with_frame_of(hwnd, style)
    }

    /// The app's size limits as outer sizes, whatever mode the window is in,
    /// with the frame measured for `style`: the limits the rectangle a window
    /// returns to must keep. `None` when there are none.
    pub(super) fn limits_with_frame_of(
        &self,
        hwnd: HWND,
        style: u32,
    ) -> Option<super::limits::TrackLimits> {
        use super::limits::{PixelSize, track_limits};

        let constraints = self.constraints.get();
        if constraints.min.is_none() && constraints.max.is_none() {
            return None;
        }
        // SAFETY: plain queries of the live window.
        let (dpi, ex_style) = unsafe {
            (
                windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd),
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32,
            )
        };
        // The frame the window adds around its content at this DPI, measured
        // for the style given, not the rectangle it has.
        let (frame_width, frame_height) =
            super::set::outer_size(style, ex_style, dpi, (0, 0), self.draws_app_title_bar())
                .unwrap_or((0, 0));
        // A DPI of zero, which Windows reports for a handle it cannot
        // measure, reads as the base DPI by the hosts' one rule.
        let mut streak = self.scale_streak.get();
        let (ratio, refused) = streak.read(f64::from(dpi) / 96.0, "Win32 window DPI");
        self.scale_streak.set(streak);
        let limits = track_limits(
            constraints.min.map(WindowExtent::get),
            constraints.max.map(WindowExtent::get),
            ratio,
            PixelSize::new(frame_width, frame_height),
        );
        if let Some(refused) = refused {
            refused.report();
        }
        Some(limits)
    }

    /// Fill in `WM_GETMINMAXINFO` with the app's size limits at the window's
    /// current DPI. A fullscreen window keeps the system's rectangle.
    pub(super) fn fill_min_max_info(&self, hwnd: HWND, info: &mut MINMAXINFO) {
        use super::limits::{MinMaxInfo, PixelSize, constrain};

        if let Some((_, area)) = self.frameless_maximized_area(hwnd) {
            info.ptMaxPosition.x = area.left;
            info.ptMaxPosition.y = area.top;
            info.ptMaxSize.x = area.right.saturating_sub(area.left);
            info.ptMaxSize.y = area.bottom.saturating_sub(area.top);
        }
        let Some(limits) = self.outer_limits(hwnd) else {
            return;
        };
        let os = MinMaxInfo {
            max_size: PixelSize::new(info.ptMaxSize.x, info.ptMaxSize.y),
            min_track: PixelSize::new(info.ptMinTrackSize.x, info.ptMinTrackSize.y),
            max_track: PixelSize::new(info.ptMaxTrackSize.x, info.ptMaxTrackSize.y),
        };
        let constrained = constrain(os, limits);
        info.ptMaxSize.x = constrained.max_size.width;
        info.ptMaxSize.y = constrained.max_size.height;
        info.ptMinTrackSize.x = constrained.min_track.width;
        info.ptMinTrackSize.y = constrained.min_track.height;
        info.ptMaxTrackSize.x = constrained.max_track.width;
        info.ptMaxTrackSize.y = constrained.max_track.height;
    }

    /// Keep a size the window is about to take within the app's limits.
    ///
    /// `DefWindowProc` asks for `WM_GETMINMAXINFO` while sizing only windows
    /// with a sizing border; a window without one (undecorated, or not
    /// resizable) is held to its limits here instead.
    pub(super) fn clamp_pending_size(&self, hwnd: HWND, pos: &mut WINDOWPOS) {
        if !self.mode.get().is_normal() || pos.flags.0 & SWP_NOSIZE.0 != 0 {
            return;
        }
        let Some(limits) = self.outer_limits(hwnd) else {
            return;
        };
        if let Some(min) = limits.min {
            pos.cx = pos.cx.max(min.width);
            pos.cy = pos.cy.max(min.height);
        }
        if let Some(max) = limits.max {
            pos.cx = pos.cx.min(max.width);
            pos.cy = pos.cy.min(max.height);
        }
    }
}

#[cfg(test)]
mod frame_tests {
    use windows::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{
            SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WS_CAPTION, WS_MAXIMIZE, WS_MAXIMIZEBOX,
            WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
        },
    };

    use super::{WindowMode, first_show_command, frame_style, restyled, to_screen, to_workspace};

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn same(a: RECT, b: RECT) -> bool {
        (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
    }

    /// A task bar on the top edge moves the work area down from the monitor's
    /// origin, so workspace coordinates are screen coordinates shifted up.
    fn a_taskbar_on_the_top_edge_shifts_workspace_coordinates() {
        let screen = rect(100, 200, 800, 700);
        let workspace = to_workspace(screen, (0, 48));
        assert!(same(workspace, rect(100, 152, 800, 652)), "{workspace:?}");
        assert!(same(to_screen(workspace, (0, 48)), screen));
    }

    /// A task bar on the left edge moves the work area right.
    fn a_taskbar_on_the_left_edge_shifts_workspace_coordinates() {
        let screen = rect(100, 200, 800, 700);
        let workspace = to_workspace(screen, (72, 0));
        assert!(same(workspace, rect(28, 200, 728, 700)), "{workspace:?}");
        assert!(same(to_screen(workspace, (72, 0)), screen));
    }

    fn a_rectangle_at_the_edge_of_the_range_saturates_in_both_directions() {
        let edge = rect(i32::MIN, i32::MIN, i32::MIN + 10, i32::MIN + 10);
        let moved = to_workspace(edge, (72, 48));
        assert_eq!((moved.left, moved.top), (i32::MIN, i32::MIN));
        let high = rect(i32::MAX - 5, i32::MAX - 5, i32::MAX, i32::MAX);
        let back = to_screen(high, (72, 48));
        assert_eq!((back.right, back.bottom), (i32::MAX, i32::MAX));
    }

    fn a_taskbar_on_the_bottom_or_right_leaves_the_origin_alone() {
        let screen = rect(-1920, 10, -1000, 500);
        assert!(same(to_workspace(screen, (0, 0)), screen));
    }

    fn the_first_show_gives_the_opening_mode() {
        assert_eq!(first_show_command(WindowMode::Maximized), SW_SHOWMAXIMIZED);
        assert_eq!(first_show_command(WindowMode::Minimized), SW_SHOWMINIMIZED);
        // Fullscreen is entered while hidden and shown like a normal window.
        assert_eq!(first_show_command(WindowMode::Normal), SW_SHOW);
        assert_eq!(first_show_command(WindowMode::Fullscreen), SW_SHOW);
    }

    fn a_frame_has_a_caption_and_sizing_only_as_asked() {
        let sizing = WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0;
        let resizable = frame_style(true, true);
        assert_eq!(resizable & WS_CAPTION.0, WS_CAPTION.0);
        assert_eq!(resizable & sizing, sizing);
        let fixed = frame_style(true, false);
        assert_eq!(fixed & WS_CAPTION.0, WS_CAPTION.0);
        assert_eq!(fixed & sizing, 0);
        for resizable in [true, false] {
            let bare = frame_style(false, resizable);
            assert_eq!(bare, WS_POPUP.0, "a popup has neither caption nor sizing");
        }
    }

    fn a_new_frame_keeps_the_state_bits_of_the_style() {
        let style = frame_style(true, true) | WS_VISIBLE.0 | WS_MAXIMIZE.0;
        let bare = restyled(style, false, true);
        assert_eq!(
            bare & (WS_VISIBLE.0 | WS_MAXIMIZE.0),
            WS_VISIBLE.0 | WS_MAXIMIZE.0
        );
        assert_eq!(bare & WS_CAPTION.0, 0);
        let back = restyled(bare, true, false);
        assert_eq!(back & WS_CAPTION.0, WS_CAPTION.0);
        assert_eq!(back & (WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0), 0);
        assert_eq!(back & WS_POPUP.0, 0, "the popup bit does not linger");
        assert_eq!(
            back & (WS_VISIBLE.0 | WS_MAXIMIZE.0),
            WS_VISIBLE.0 | WS_MAXIMIZE.0
        );
    }

    #[test]
    fn window_frame_rules() {
        crate::table_test::run_table(
            "window_frame_rules",
            &[
                (
                    "a_taskbar_on_the_top_edge_shifts_workspace_coordinates",
                    a_taskbar_on_the_top_edge_shifts_workspace_coordinates as fn(),
                ),
                (
                    "a_taskbar_on_the_left_edge_shifts_workspace_coordinates",
                    a_taskbar_on_the_left_edge_shifts_workspace_coordinates as fn(),
                ),
                (
                    "a_rectangle_at_the_edge_of_the_range_saturates_in_both_directions",
                    a_rectangle_at_the_edge_of_the_range_saturates_in_both_directions as fn(),
                ),
                (
                    "a_taskbar_on_the_bottom_or_right_leaves_the_origin_alone",
                    a_taskbar_on_the_bottom_or_right_leaves_the_origin_alone as fn(),
                ),
                (
                    "the_first_show_gives_the_opening_mode",
                    the_first_show_gives_the_opening_mode as fn(),
                ),
                (
                    "a_frame_has_a_caption_and_sizing_only_as_asked",
                    a_frame_has_a_caption_and_sizing_only_as_asked as fn(),
                ),
                (
                    "a_new_frame_keeps_the_state_bits_of_the_style",
                    a_new_frame_keeps_the_state_bits_of_the_style as fn(),
                ),
            ],
        );
    }
}
