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
//! `window_destroyed`), and follows the steps of other hooks in turn: a key
//! the application let through, which the window's own keys answer, and the
//! session's lock, which hides it.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    SizeConstraints, WindowAppearance, WindowEvent, WindowGeometry, WindowId, WindowMode,
    WindowState,
};
use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::{BeginPaint, ClientToScreen, EndPaint, PAINTSTRUCT},
    },
    System::Threading::GetCurrentThreadId,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            CHILDID_SELF, DefWindowProcW, DestroyWindow, EVENT_OBJECT_CLOAKED,
            EVENT_OBJECT_UNCLOAKED, GWL_EXSTYLE, GWL_STYLE, GWLP_USERDATA, GetClientRect,
            GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HTCLIENT, IsIconic,
            IsWindowVisible, IsZoomed, KillTimer, MINMAXINFO, OBJID_WINDOW, PostMessageW,
            SC_KEYMENU, SC_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SWP_NOSIZE,
            SendMessageW, SetTimer, SetWindowLongPtrW, WA_INACTIVE, WINDOWPOS,
            WINEVENT_OUTOFCONTEXT, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_CREATE, WM_DESTROY,
            WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KILLFOCUS, WM_MOVE, WM_PAINT,
            WM_SETCURSOR, WM_SETFOCUS, WM_SETTEXT, WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SIZE,
            WM_STYLECHANGED, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WS_CAPTION,
            WS_MAXIMIZE, WS_MAXIMIZEBOX, WS_MINIMIZE, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_THICKFRAME,
        },
    },
};

use super::frame_timing::{RefreshTiming, refresh_timing};
use super::mode::NativeMode;
use super::set::{WindowSet, checked_logical_to_device, restore_rectangle};
use super::window::WindowsWindow;
use crate::owner::call_point::{Field, call};
use crate::owner::handlers::{PlatformHandlers, Publication, WindowCallbacks};
use crate::owner::hwnd_affinity::ContextLedger;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::windows::HostDisplays;
use crate::owner::windows::control::OwnerPreferences;
use crate::owner::windows::modal_loop::MODAL_DEADLINE_TIMER;
use crate::owner::windows::{
    WindowHooks, WindowIdentity, WindowMessages, WindowStep, window_destroyed, window_step,
};
use crate::owner::windows::{get_x_lparam, get_y_lparam};
use crate::window::config::WindowConfiguration;
use crate::window::state::{ReportedFacts, WindowStateCell, mode_of};
use crate::window::visibility::ReportedVisibility;

/// Posted to a window nothing paints, so its frame still runs: a hidden window
/// never receives `WM_PAINT`.
pub(crate) const REQUEST_FRAME: u32 = WM_APP + 1;

/// Posted by the fullscreen hotkey once the app let its key through
/// ([`WindowContext::apply_window_hotkey`]).
pub(crate) const TOGGLE_FULLSCREEN: u32 = WM_APP + 3;

pub(crate) use crate::owner::windows::WindowRef;

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
    pub(crate) window_state: std::sync::Weak<Mutex<WindowRecord>>,
    /// The published window state, shared with the wrapper; refreshed by
    /// [`Self::publish_window_state`].
    pub snapshot: Arc<WindowStateCell>,
    /// The platform-level handlers, shared with the owner control context.
    /// `Rc<RefCell<..>>`: every holder lives on the owner thread, and no
    /// borrow is held while a handler runs.
    pub handlers: Rc<RefCell<PlatformHandlers>>,
    /// Asks the owner to consult the exit policy once this window's
    /// `WM_DESTROY` leaves the platform tracking no window.
    pub exit_policy: crate::owner::windows::control::ExitPolicyRequest,
    /// The platform's monitors: a DPI change invalidates their snapshot.
    pub displays: Arc<dyn HostDisplays>,
    /// The monitor handle `WM_WINDOWPOSCHANGED` last saw this window on, `None`
    /// before the first message; a change raises `on_display_changed`.
    pub last_monitor: std::cell::Cell<Option<std::num::NonZeroIsize>>,
    /// Per-window callbacks for event delivery, shared with the features the
    /// class's hooks announced the window to and released by `WM_DESTROY` on
    /// the owner thread.
    pub callbacks: Arc<WindowCallbacks>,
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
    /// Current window mode (replaces display_state + saved bounds)
    pub mode: std::cell::Cell<NativeMode>,
    /// Last known size (before minimization) for restore detection
    pub last_size: std::cell::Cell<Size<i32>>,
    /// Window configuration (hotkeys, debouncing, etc.)
    pub config: WindowConfiguration,
    /// The user's app theme as last sampled and reported to
    /// `on_appearance_changed`; the chrome is drawn in it while
    /// `chrome_follows_system`.
    pub appearance: std::cell::Cell<WindowAppearance>,
    /// Whether the last `WM_ACTIVATE` made the window the active one of its
    /// desktop: it, or a window it owns, has the keyboard.
    pub(crate) active: std::cell::Cell<bool>,
    /// Whether the title bar follows the system theme; `false` once the app
    /// chose it through `Win32Window::set_dark_mode` or `set_theme`, or opened
    /// with a theme of its own (`Win32WindowOptions::theme`).
    pub chrome_follows_system: std::cell::Cell<bool>,
    /// Cursor selected by this exact window's presentation.
    pub cursor: std::cell::Cell<CursorIcon>,
    /// Window style bits before fullscreen (Windows-specific: WS_OVERLAPPEDWINDOW, etc.)
    ///
    /// Stored here instead of in `NativeMode` to keep the cross-platform enum
    /// free of platform-specific fields.
    pub restore_style: std::cell::Cell<u32>,
    /// The inner-size limits the app set, in logical pixels; `WM_GETMINMAXINFO`
    /// turns them into outer limits at the window's current DPI.
    pub constraints: std::cell::Cell<flui_platform::SizeConstraints>,
    /// Whether the app wants the window resizable. A window without a frame
    /// cannot be, but gets the frame back with this still set.
    pub resizable: std::cell::Cell<bool>,
    /// How many multi-step changes are in flight; see
    /// [`Self::hold_publishing`].
    pub publish_hold: std::cell::Cell<u32>,
    /// Set while the window is being put into fullscreen, before its mode says
    /// so: the size limits do not apply to the rectangle it is given.
    pub limits_waived: std::cell::Cell<bool>,
    /// Set while a hidden window is restored and hidden again: the show and
    /// the hide are not the window appearing, so no visibility is reported.
    pub visibility_muted: std::cell::Cell<bool>,
    /// How many times the app asked for the window to be shown; see
    /// [`Self::note_show_request`].
    pub show_requests: std::cell::Cell<u32>,
    /// What is still owed of the window's first show. A window opens hidden and
    /// in its mode, so the first sight of it is already maximized or
    /// fullscreen, not a normal window that jumps.
    pub(crate) first_show: std::cell::Cell<FirstShow>,
    /// The window's visibility as the runtime sees it (hidden-surface
    /// gating): the surface, which `WM_SIZE` (minimize/restore) and
    /// `WM_SHOWWINDOW` (hide/show) report through the pure rules in
    /// `super::surface`, less a cloak or a locked session. Seeded
    /// from the window's ACTUAL creation style
    /// (`surface::initial_visibility(GWL_STYLE)`, see its doc) — a decorated
    /// window is created hidden (`false`, and the creation-time
    /// `ShowWindow`'s `WM_SHOWWINDOW` edge flips it), while an undecorated
    /// window is created `WS_POPUP | WS_VISIBLE` (`true`, and no
    /// creation-time edge ever fires). A constant seed would desync the
    /// edge filter for one of the two and swallow that window's first real
    /// transition.
    pub(crate) visibility: Visibility,
    /// The window's subscription to cloak changes.
    pub(crate) cloak_watch: CloakWatch,
    /// Set between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`: a live resize,
    /// whose `WM_SIZE` renders at once.
    pub in_size_move: std::cell::Cell<bool>,
    /// The client size, in device pixels, and the scale the last resize
    /// callback delivered.
    pub last_resize: std::cell::Cell<Option<(Size<i32>, f64)>>,
    /// Delivers wake deadlines while a modal loop runs for the window.
    pub modal_pacing: crate::owner::windows::modal_loop::ModalPacing,
    /// Borrow ledger deferring this context's free past every live borrow
    /// on the owner thread — see [`ContextLedger`] for the reentrancy
    /// hazard (a framework callback closing the window from inside a
    /// dispatch) this exists to survive, and [`ContextGuard`] for the RAII
    /// half. `RefCell`, not a lock: only the owning thread ever touches it
    /// (the affinity gates enforce that), and every borrow is a short
    /// non-reentrant method call.
    pub ledger: std::cell::RefCell<ContextLedger>,
}

/// The compositor's last and next refresh around now, from the desktop window manager's timing
/// report and the performance counter it is measured on (ADR-0257). `None` when the window manager
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
            Some(timing) => self.callbacks.dispatch_frame(crate::frame_request_timed(
                timing.since_refresh,
                Some(timing.until_refresh),
            )),
            None => self.callbacks.dispatch_request_frame(),
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

    /// Tell the window's resize callback its client size, `device` pixels, at
    /// its current scale.
    pub(crate) fn dispatch_resize(&self, device: Size<i32>) {
        let scale = self.scale.scale();
        self.last_resize.set(Some((device, scale)));
        if let Ok(metrics) = flui_foundation::geometry::WindowMetrics::new(device, scale) {
            self.callbacks.dispatch_resize(metrics);
        }
    }

    /// Complete cache updates before callback-capable native/user code.
    pub(crate) fn update_window_state(&self, update: impl FnOnce(&mut WindowRecord)) {
        if let Some(state) = self.window_state.upgrade() {
            update(&mut state.lock());
        }
    }

    /// The window's published state, read from the OS again first when
    /// something invalidated it; `on_state_changed` is raised when that
    /// produced a new revision.
    pub(crate) fn window_state(&self, hwnd: HWND) -> flui_platform::WindowState {
        // While a change takes several steps the window is half-way between two
        // states; a read from inside a callback it raises sees the last whole one.
        if self.publish_hold.get() > 0 {
            return self.snapshot.published();
        }
        let Some(record) = self.window_state.upgrade() else {
            return self.snapshot.published();
        };
        let published = self
            .snapshot
            .state_with(|| sample_state(hwnd, &record, self.tracking(hwnd)));
        // With the fact callbacks the hold kept back, behind the announcement.
        self.callbacks.announce(published.changed);
        published.state
    }

    /// Read the window again for its state publisher, announcing nothing: the
    /// fact that asked is delivered behind the announcement, or, while
    /// publishing is held, behind the publication that ends the hold.
    pub(crate) fn read_window_state(&self, hwnd: HWND) -> Publication {
        self.snapshot.invalidate();
        if self.publish_hold.get() > 0 {
            return Publication::Held;
        }
        let Some(record) = self.window_state.upgrade() else {
            return Publication::Unchanged;
        };
        self.snapshot
            .state_with(|| sample_state(hwnd, &record, self.tracking(hwnd)))
            .outcome()
    }

    /// What this context knows of the window beyond what Win32 reports.
    pub(crate) fn tracking(&self, hwnd: HWND) -> OwnerTracking {
        OwnerTracking {
            mode: self.mode.get(),
            restore_style: self.restore_style.get(),
            constraints: self.constraints.get(),
            display: self.displays.display_of_window(hwnd),
            occluded: self.visibility.occluded(),
            appearance: self.appearance.get(),
            active: self.active.get(),
            hovered: self.callbacks.hovered(),
        }
    }

    /// The window may differ from its published state: read it again.
    pub(crate) fn publish_window_state(&self, hwnd: HWND) {
        self.snapshot.invalidate();
        // A change that takes several steps publishes once, when it is done:
        // a subscriber never sees the window half-changed.
        if self.publish_hold.get() == 0 {
            let _ = self.window_state(hwnd);
        }
    }

    /// Hold back publishing until the returned guard drops; the caller then
    /// publishes once.
    pub(crate) fn hold_publishing(&self) -> PublishHold<'_> {
        self.publish_hold
            .set(self.publish_hold.get().saturating_add(1));
        PublishHold(&self.publish_hold)
    }

    /// Dispatch a window event to the platform-level handler.
    #[inline]
    pub(crate) fn dispatch_event(&self, event: WindowEvent) {
        self.call_handler(
            |handlers| &mut handlers.window_event,
            |handler| handler(event),
        );
    }

    /// Runs one platform-level handler through the call point: out of its
    /// slot, so no `RefCell` borrow is held while it runs, then put back
    /// only if the slot is still empty. A handler registered while this one
    /// was out — including by this handler itself — is the newer
    /// registration and wins; the one that ran is then retired. Its panic
    /// unwinds to the window procedure's boundary, which aborts.
    ///
    /// The restore touches `self` AFTER the handler returns, and the handler
    /// may have closed this very window — that is sound only because every
    /// caller holds a [`ContextGuard`] over `self`, which defers the
    /// context's free past the borrow even when the handler's close runs a
    /// nested `WM_DESTROY` (which merely retires it).
    fn call_handler<H>(
        &self,
        project: fn(&mut PlatformHandlers) -> &mut Option<H>,
        body: impl FnOnce(&mut H),
    ) {
        call(&Field::new(&*self.handlers, project), body).or_resume(());
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
/// nested `WM_DESTROY` (a framework callback closing the window from inside
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
            // `Box::into_raw` allocation `WindowsWindow::new` installed —
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
/// This is the gate that discharges the HWND thread-affinity obligation on
/// `WindowsWindow`'s `Send`/`Sync` impls: the context box is freed exactly
/// once, by `WM_DESTROY` on the window's owning thread, so a dereference is
/// sound only when made **on that same thread** (where it is serialized with
/// the free by same-thread message dispatch) against a live window of our
/// own class with a non-null slot. Every decision is delegated to the pure,
/// Linux-tested rules in [`crate::owner::hwnd_affinity`]; this shell only
/// performs the OS queries.
///
/// The borrow is guarded, not fragile: `f` may make calls that re-enter
/// `window_proc` on this same thread (an `SetWindowPos` inside the
/// fullscreen toggle, a dispatched framework callback that closes the
/// window). If such reentrancy reaches `WM_DESTROY`, the context is only
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
/// dropped here, on the calling thread, untouched — which is what lets a
/// callback registration hand its callback back to the thread that offered
/// it instead of parking it where the owner would later run or free it.
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
            // pointer `WindowsWindow::new` installed, not a foreign
            // protocol's), and the slot is non-null (so `WM_DESTROY` has
            // not retired it). Between the slot read above and this
            // acquisition no message dispatch happens on this thread, so
            // the context cannot have been retired-and-freed in between;
            // once the guard is held, any reentrant `WM_DESTROY` inside
            // `f` defers the free past the guard's drop (see
            // `ContextGuard`).
            let guard = unsafe { ContextGuard::acquire(slot as *mut WindowContext) };
            Ok(f(guard.context()))
        }
        UserDataVerdict::Refuse(reason) => {
            tracing::debug!(
                op,
                hwnd = ?hwnd,
                ?reason,
                "refusing to touch this window's native context"
            );
            Err(reason)
        }
    }
}

/// Routes a teardown request (`close()`, last `Drop`) for `hwnd` by thread
/// identity — `DestroyWindow` directly on the owning thread, `PostMessageW
/// (WM_CLOSE)` from any other ("a thread cannot use `DestroyWindow` to
/// destroy a window created by a different thread" — MSDN), nothing for a
/// dead handle — **and by window identity**: `expected_context` is the
/// context pointer the calling wrapper installed at creation, compared (as
/// a value, never dereferenced, hence safe from any thread) against the
/// window's current class + `GWLP_USERDATA` slot, so a wrapper that
/// outlived its native window can never destroy or close whatever
/// unrelated window the OS recycled the handle value for. Pure rule in
/// [`crate::owner::hwnd_affinity::route_teardown`], which also states the
/// pointer-ABA residual; this shell only performs the OS queries.
pub(crate) fn teardown_route(
    hwnd: HWND,
    expected_context: *const WindowContext,
) -> crate::owner::hwnd_affinity::TeardownRoute {
    // SAFETY: see the identical queries in `with_window_context` — no
    // pointers dereferenced, no messages pumped.
    let (owner_thread, current_thread, slot) = unsafe {
        (
            GetWindowThreadProcessId(hwnd, None),
            GetCurrentThreadId(),
            GetWindowLongPtrW(hwnd, GWLP_USERDATA),
        )
    };
    crate::owner::hwnd_affinity::route_teardown(
        owner_thread,
        current_thread,
        hwnd_names_flui_class(hwnd),
        slot,
        expected_context as isize,
    )
}

/// Runs `handle` with the context of `hwnd`, borrowed for one message of its
/// window procedure: `None` before the context is installed and after
/// `WM_DESTROY` retired it. `handle` also gets the slot's value, against which
/// a hook can tell, after a callback, that the window closed and another took
/// its handle.
///
/// The borrow survives reentrant dispatch: a hook holds it across framework
/// callbacks, and a callback may close the window — running a nested
/// `WM_DESTROY` frame on this same thread that RETIRES the context (clears the
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

impl WindowMessages for WindowProc {
    /// The window's own keys (the fullscreen hotkey) follow a key the
    /// application let through, and a locked session hides it; every other
    /// step is the window's own.
    fn step(&self, window: WindowRef, step: WindowStep) {
        match step {
            WindowStep::KeyLetThrough {
                virtual_key,
                repeat,
            } => {
                window.with_context("window keys", |ctx| {
                    ctx.apply_window_hotkey(window.hwnd(), virtual_key, repeat);
                });
            }
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
            | WindowStep::Closed => {}
        }
    }

    /// The window's own messages: its creation, closing and destruction, its
    /// size, place, limits, DPI, focus, paint, cursor and title, the modal
    /// size-move and menu loops with their deadline timer, the session and
    /// setting changes it follows, the frame and fullscreen requests posted to
    /// it, and the keyboard's system-menu command.
    fn answers(&self, msg: u32, wparam: WPARAM) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_ENTERMENULOOP, WM_ENTERSIZEMOVE, WM_EXITMENULOOP, WM_EXITSIZEMOVE, WM_TIMER,
        };
        match msg {
            WM_CREATE | WM_CLOSE | WM_DESTROY | WM_ERASEBKGND | WM_ENTERSIZEMOVE
            | WM_EXITSIZEMOVE | WM_ENTERMENULOOP | WM_EXITMENULOOP | WM_PAINT | WM_SIZE
            | WM_SHOWWINDOW | WM_MOVE | WM_DPICHANGED | WM_WINDOWPOSCHANGING
            | WM_WINDOWPOSCHANGED | TOGGLE_FULLSCREEN | WM_SETTEXT | WM_STYLECHANGED
            | WM_GETMINMAXINFO | REQUEST_FRAME | WM_SETCURSOR | WM_SETFOCUS | WM_KILLFOCUS
            | WM_ACTIVATE | WM_SETTINGCHANGE => true,
            WM_TIMER => wparam.0 == MODAL_DEADLINE_TIMER,
            WM_SYSCOMMAND => (wparam.0 & 0xFFF0) == SC_KEYMENU as usize,
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
    /// so the `GWLP_USERDATA` writes below do not race `WM_DESTROY`'s
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
                    tracing::debug!("WM_CREATE for HWND {:?}", hwnd);
                    LRESULT(0)
                }

                WM_CLOSE => {
                    tracing::debug!("WM_CLOSE for HWND {:?}", hwnd);

                    if let Some(ctx) = ctx {
                        // Ask per-window callback if close should proceed
                        let should_close = ctx.callbacks.dispatch_should_close();

                        if should_close {
                            // Dispatch CloseRequested to global handlers
                            ctx.dispatch_event(WindowEvent::CloseRequested {
                                window_id: ctx.window_id,
                            });
                            if let Err(error) = DestroyWindow(hwnd) {
                                tracing::warn!(?hwnd, ?error, "DestroyWindow failed on WM_CLOSE");
                            }
                        }
                        // If !should_close, the close is vetoed
                    } else if let Err(error) = DestroyWindow(hwnd) {
                        tracing::warn!(
                            ?hwnd,
                            ?error,
                            "DestroyWindow failed on WM_CLOSE (no WindowContext)"
                        );
                    }

                    LRESULT(0)
                }

                WM_DESTROY => {
                    tracing::debug!("WM_DESTROY for HWND {:?}", hwnd);

                    if let Some(ctx) = ctx {
                        // While the window and its callbacks still exist: what
                        // the input holds of the window ends as cancelled and
                        // its touchpad viewport is released, a drag over it
                        // leaves it and OLE no longer offers it one, and the
                        // session no longer reports to it.
                        let hooks = Rc::clone(&ctx.hooks);
                        window_destroyed(&hooks, WindowRef::new(hwnd, ctx.identity));

                        // Fire per-window on_close callback (FnOnce)
                        ctx.callbacks.dispatch_close();

                        // Dispatch Closed event to global handlers
                        ctx.dispatch_event(WindowEvent::Closed(ctx.window_id));

                        // Release every remaining registered callback now,
                        // while the HWND is still valid (Win32 only frees it
                        // after this handler returns, at `WM_NCDESTROY`).
                        // Without this, the frame callback registered via
                        // `on_request_frame` — which in `flui-app`'s wiring
                        // owns this window's GPU renderer, whose
                        // `wgpu::Surface` is built from the
                        // `Arc<dyn PlatformWindow>` clone the renderer owns
                        // (ADR-0063) — stays pinned forever: window (through
                        // its callback slots) → frame closure → raster lane
                        // → renderer → surface → `Arc<WindowsWindow>`, a
                        // cycle nothing else in this arm breaks. Calling it
                        // here, before the swapchain-owning callback's
                        // native window is gone, releases the surface while
                        // it is still valid to destroy.
                        //
                        // The destruction trace rides on the clear: a close
                        // from inside one of this window's own callbacks
                        // only queues it behind that callback, and the
                        // surface is released when the queued clear runs,
                        // so the trace is ordered after the release however
                        // the close arrived.
                        let window_id = ctx.window_id.0;
                        ctx.callbacks.clear_then(move || {
                            tracing::debug!(
                                target: "flui.platform",
                                event = "native_window_destroyed",
                                window_id,
                            );
                        });

                        // The hooks hear that the close callbacks ran, while
                        // the HWND is still valid and before the context
                        // retires: the text input deactivates TSF for the
                        // window now that the presentation is closed and its
                        // field unfocused (ADR-0135 §3).
                        ctx.step(hwnd, WindowStep::Closed);

                        // Retire the context: clear the slot FIRST, so no
                        // new borrow can be minted (`with_window_context`
                        // and this function's own entry both refuse a null
                        // slot), then mark the ledger. The allocation is
                        // NOT freed here — this frame's own `guard` still
                        // borrows it, and so may outer frames when this
                        // `WM_DESTROY` arrived reentrantly (a framework
                        // callback closing the window from inside another
                        // arm's dispatch). The free happens exactly once,
                        // at the outermost `ContextGuard` release — in the
                        // common case, this frame's own guard dropping at
                        // the end of this function (see `ContextGuard`).
                        // `window_proc` never runs concurrently with itself
                        // for the same `hwnd` (Win32 dispatches on the one
                        // owning thread), so slot clear + retire need no
                        // further ordering.
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                        ctx.ledger.borrow_mut().retire();

                        // Retire tracking only after userdata: the removed Arc
                        // may be the last wrapper. Its Drop must see cleared userdata and
                        // cannot recursively destroy this in-flight HWND.
                        let removed = ctx
                            .windows
                            .upgrade()
                            .and_then(|windows| windows.remove(hwnd, ctx.identity));
                        let last_window_closed = removed.as_ref().is_some_and(|(_, empty)| *empty);
                        drop(removed);
                        // The last tracked window is gone: the owner decides,
                        // on its own turn, whether the loop ends. Not here —
                        // this `WM_DESTROY` may be nested inside embedder
                        // code that closed the window, and the hook
                        // re-enters the embedder.
                        if last_window_closed {
                            ctx.exit_policy.post();
                        }
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
                        ctx.exit_modal_loop(hwnd);
                        ctx.in_size_move.set(false);
                        ctx.step(hwnd, WindowStep::SizeMoveEnded);
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
                        ctx.exit_modal_loop(hwnd);
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
                            ctx.dispatch_event(WindowEvent::RedrawRequested {
                                window_id: ctx.window_id,
                            });
                        }
                        let _ = EndPaint(hwnd, &raw const ps);
                    }
                    LRESULT(0)
                }

                WM_SIZE => {
                    use super::surface::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

                    // WM_SIZE dimensions are unsigned16, unlike signed
                    // mouse and WM_MOVE coordinates packed in the same shape.
                    let width = (lparam.0 as u32 & 0xffff).max(1) as i32;
                    let height = hiword(lparam.0 as u32).max(1) as i32;
                    let size_type = wparam.0 as u32;

                    if let Some(ctx) = ctx {
                        let size = Size::new(width, height);
                        let prev_mode = ctx.mode.get();
                        if size_type != SIZE_MINIMIZED {
                            ctx.step(hwnd, WindowStep::Resized((width, height)));
                            ctx.update_window_state(|state| {
                                state.bounds.size = Size::new(
                                    width as f64 / ctx.scale.scale(),
                                    height as f64 / ctx.scale.scale(),
                                );
                            });
                        }

                        // Handle state transition and dispatch appropriate event
                        let (new_mode, event) = match size_type {
                            SIZE_MINIMIZED => {
                                tracing::info!("📦 Window Minimized");
                                // Validate transition
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
                                    (candidate, None)
                                } else {
                                    tracing::warn!(
                                        "⚠️  Invalid state transition: {:?} -> Minimized (transition ignored)",
                                        prev_mode
                                    );
                                    (prev_mode, None)
                                }
                            }
                            SIZE_MAXIMIZED => {
                                tracing::info!("📏 Window Maximized: {}x{}", width, height);
                                // Validate transition
                                let candidate = NativeMode::Maximized;
                                if prev_mode.is_fullscreen() {
                                    // A fullscreen window covers its monitor as a
                                    // popup; it does not become maximized.
                                    (prev_mode, None)
                                } else if prev_mode.can_transition_to(&candidate) {
                                    ctx.last_size.set(size);
                                    (
                                        candidate,
                                        Some(WindowEvent::Resized {
                                            window_id: ctx.window_id,
                                            size,
                                        }),
                                    )
                                } else {
                                    tracing::warn!(
                                        "⚠️  Invalid state transition: {:?} -> Maximized (transition ignored)",
                                        prev_mode
                                    );
                                    (prev_mode, None)
                                }
                            }
                            SIZE_RESTORED => {
                                // SIZE_RESTORED covers two cases: a genuine restore FROM
                                // minimized/maximized (a real state change), and a plain
                                // resize while already Normal (same state — NOT a transition).
                                // `can_transition_to` rejects same-state by design, so it must
                                // NOT gate this event: a normal-state resize is always a valid
                                // `Resized`. Gating on it dropped the event (and the last_size
                                // update) on every live-resize drag step.
                                ctx.last_size.set(size);
                                if prev_mode.is_minimized() || prev_mode.is_maximized() {
                                    tracing::info!("📐 Window Restored: {}x{}", width, height);
                                } else {
                                    tracing::debug!("📐 Window Resized: {}x{}", width, height);
                                }
                                let event = WindowEvent::Resized {
                                    window_id: ctx.window_id,
                                    size,
                                };
                                let restored = match prev_mode {
                                    // A resize does not end fullscreen.
                                    NativeMode::Fullscreen { .. } => prev_mode,
                                    // Restoring a minimized fullscreen window
                                    // brings fullscreen back.
                                    NativeMode::Minimized {
                                        from_fullscreen: Some(restore_bounds),
                                    } => NativeMode::Fullscreen { restore_bounds },
                                    _ => NativeMode::Normal,
                                };
                                (restored, Some(event))
                            }
                            _ => {
                                // Regular resize while in current state
                                tracing::info!("📐 Window Resized: {}x{}", width, height);
                                if prev_mode.is_normal() {
                                    ctx.last_size.set(size);
                                }
                                (
                                    prev_mode,
                                    Some(WindowEvent::Resized {
                                        window_id: ctx.window_id,
                                        size,
                                    }),
                                )
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

                        // Hidden-surface gating, minimize/restore half: it
                        // reaches `dispatch_visibility_status_change`, the wire
                        // the runtime's `FrameClock` gate and `AppLifecycleState`
                        // derivation hang off. Decided by the pure rule in
                        // `surface` (host-tested).
                        if !ctx.visibility_muted.get()
                            && let Some(visible) =
                                super::surface::size_visibility(size_type, prev_mode.is_minimized())
                        {
                            ctx.report_surface_visibility(visible);
                        }

                        // Fire per-window on_resize callback (for all size changes except minimize)
                        if size_type != SIZE_MINIMIZED {
                            ctx.dispatch_resize(size);
                        }

                        // Dispatch event to global handlers if any
                        if let Some(event) = event {
                            ctx.dispatch_event(event);
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
                    // Hidden-surface gating, hide/show half (the
                    // minimize/restore half lives in the WM_SIZE arm above).
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
                        ctx.update_window_state(|state| state.visible = shown);
                        ctx.publish_window_state(hwnd);
                        if !ctx.visibility_muted.get()
                            && let Some(visible) = super::surface::show_window_visibility(
                                shown,
                                ctx.mode.get().is_minimized(),
                            )
                        {
                            ctx.report_surface_visibility(visible);
                        }
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
                    tracing::debug!("Window Moved: ({}, {})", x, y);

                    if let Some(ctx) = ctx {
                        ctx.update_window_state(|state| {
                            state.bounds.origin = Point::new(
                                x as f64 / ctx.scale.scale(),
                                y as f64 / ctx.scale.scale(),
                            );
                        });
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
                        ctx.clamp_pending_size(hwnd, &mut *(lparam.0 as *mut WINDOWPOS));
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_WINDOWPOSCHANGED => {
                    // `WM_MOVE` and `WM_SIZE` come from the default procedure;
                    // the monitor is checked once the move has landed.
                    let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                    if let Some(ctx) = ctx {
                        let monitor = ctx.displays.nearest_monitor(hwnd);
                        if ctx.last_monitor.replace(monitor) != monitor {
                            ctx.callbacks.dispatch_display_changed();
                        }
                        // Position, size and z-order changes all land here.
                        ctx.publish_window_state(hwnd);
                        // A window shown on another virtual desktop is shown
                        // cloaked; later changes come through `CloakWatch`.
                        ctx.refresh_cloaked(hwnd);
                    }
                    result
                }

                // Toggled after the key's dispatch, on a turn of its own (see
                // `WM_KEYDOWN`).
                // A handle the window's close let another window reuse is not
                // the window whose key asked for it.
                TOGGLE_FULLSCREEN => {
                    if ctx.is_some_and(|ctx| ctx.identity.halves() == (wparam.0, lparam.0)) {
                        WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
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
                // window never receives `WM_PAINT`.
                REQUEST_FRAME => {
                    if let Some(ctx) = ctx {
                        ctx.request_frame();
                        ctx.dispatch_event(WindowEvent::RedrawRequested {
                            window_id: ctx.window_id,
                        });
                    }
                    LRESULT(0)
                }

                WM_SETCURSOR => {
                    if let Some(ctx) = ctx
                        && (lparam.0 as u32 & 0xffff) == HTCLIENT
                    {
                        match WindowsWindow::apply_native_cursor(ctx.cursor.get()) {
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
                        ctx.update_window_state(|state| state.focused = true);
                        ctx.publish_window_state(hwnd);
                        // Fire per-window on_active_status_change callback
                        ctx.callbacks.dispatch_active_status_change(true);

                        // Dispatch FocusChanged to global handlers
                        ctx.dispatch_event(WindowEvent::FocusChanged {
                            window_id: ctx.window_id,
                            focused: true,
                        });
                    }

                    LRESULT(0)
                }

                WM_KILLFOCUS => {
                    tracing::debug!("Window Unfocused");

                    if let Some(ctx) = ctx {
                        ctx.update_window_state(|state| state.focused = false);
                        ctx.step(hwnd, WindowStep::FocusLost);
                        ctx.publish_window_state(hwnd);
                        // Fire per-window on_active_status_change callback
                        ctx.callbacks.dispatch_active_status_change(false);

                        // Dispatch FocusChanged to global handlers
                        ctx.dispatch_event(WindowEvent::FocusChanged {
                            window_id: ctx.window_id,
                            focused: false,
                        });
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
                            ctx.callbacks.dispatch_appearance_changed();
                        }
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

/// Mutable window state
pub(crate) struct WindowRecord {
    /// Current window bounds (logical pixels)
    pub(crate) bounds: Bounds<f64>,

    /// Current scale factor (DPI / 96)
    pub(crate) scale_factor: f64,

    /// Is window visible?
    pub(crate) visible: bool,

    /// Is window focused?
    pub(crate) focused: bool,
}

/// What the owner context knows about a window that Win32 does not say: the
/// mode it tracks from `WM_SIZE` and its own fullscreen toggle, the style a
/// fullscreen window returns to, the limits the app set and the display the
/// window is on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OwnerTracking {
    pub(crate) mode: NativeMode,
    pub(crate) restore_style: u32,
    pub(crate) constraints: SizeConstraints,
    pub(crate) display: Option<flui_platform::DisplayId>,
    /// Whether a cloak or a locked session hides the window whatever it shows.
    pub(crate) occluded: bool,
    /// The user's theme as `WM_SETTINGCHANGE` last read it: never the
    /// dark-mode flag this host sets on the chrome to match (ADR-0222).
    pub(crate) appearance: WindowAppearance,
    /// Whether the last `WM_ACTIVATE` made the window the active one.
    pub(crate) active: bool,
    /// Whether the mouse is over the window, as its input last reported it.
    pub(crate) hovered: bool,
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
pub(crate) fn sample_state(
    hwnd: HWND,
    record: &Mutex<WindowRecord>,
    tracking: OwnerTracking,
) -> WindowState {
    let OwnerTracking {
        mode,
        restore_style,
        constraints,
        display,
        occluded,
        appearance,
        active,
        hovered,
    } = tracking;
    let (scale, visible, focused, cached) = {
        let record = record.lock();
        (
            record.scale_factor,
            record.visible && !occluded,
            record.focused,
            record.bounds.size,
        )
    };
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
    let content = if have_client {
        Size::new(
            f64::from(client.right - client.left) / scale,
            f64::from(client.bottom - client.top) / scale,
        )
    } else {
        cached
    };
    let outer = if have_window && !mode.is_minimized() {
        WindowGeometry::new(
            Some(Point::new(
                f64::from(window.left) / scale,
                f64::from(window.top) / scale,
            )),
            Size::new(
                f64::from(window.right - window.left) / scale,
                f64::from(window.bottom - window.top) / scale,
            ),
        )
    } else {
        WindowGeometry::new(None, if mode.is_minimized() { cached } else { content })
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
    let normal = restore.map(|bounds| {
        WindowGeometry::new(
            Some(Point::new(
                f64::from(bounds.origin.x) / scale,
                f64::from(bounds.origin.y) / scale,
            )),
            Size::new(
                f64::from(bounds.size.width) / scale,
                f64::from(bounds.size.height) / scale,
            ),
        )
    });
    let state = WindowState::new(outer, if mode.is_minimized() { cached } else { content })
        .with_mode(mode_of(mode.flags()))
        .with_normal_geometry(normal)
        .with_visible(visible)
        .with_focused(focused)
        .with_title(window_text(hwnd))
        .with_constraints(constraints)
        .with_resizable(style & WS_THICKFRAME.0 != 0)
        .with_decorated(style & WS_CAPTION.0 == WS_CAPTION.0)
        .with_display(display);
    ReportedFacts {
        // The client area in device pixels at the window's scale; a minimized
        // window's empty client area is not drawable and keeps the last.
        metrics: have_client
            .then(|| {
                flui_foundation::geometry::WindowMetrics::new(
                    Size::new(client.right - client.left, client.bottom - client.top),
                    scale,
                )
                .ok()
            })
            .flatten(),
        appearance,
        // A desktop window has no safe area, and Win32 never suspends one.
        safe_area: flui_foundation::geometry::EdgeInsets::ZERO,
        execution: flui_platform::WindowExecutionState::Running,
        hovered,
        active,
        capabilities: super::window::window_capabilities(),
    }
    .onto(state)
}

impl WindowsWindow {
    /// Toggle fullscreen mode for a window by HWND (static method for use from
    /// window_proc)
    ///
    /// This method implements borderless fullscreen by:
    /// 1. **Entering fullscreen**: Saves current window style and bounds,
    ///    removes window borders (WS_POPUP), and resizes to cover the entire
    ///    monitor
    /// 2. **Exiting fullscreen**: Restores saved window style and bounds
    ///
    /// # Implementation Details
    /// - Uses borderless fullscreen (WS_POPUP) rather than exclusive fullscreen
    ///   for better compatibility
    /// - Automatically detects the monitor containing the window and fills it
    ///   completely
    /// - Preserves window state (position, size, style) for proper restoration
    /// - Publishes the new window state (`on_state_changed`) once the window
    ///   has moved
    ///
    /// # Thread Safety
    /// Callable from any thread, but only effective on the window's owning
    /// thread (where `window_proc`'s hotkey arm calls it): the context gate
    /// (`with_window_context`, see `platform.rs`) refuses the toggle with a
    /// warning on any other thread instead of racing the owner's
    /// `WM_DESTROY` free of the window context.
    ///
    /// # Example
    /// ```ignore
    /// // Toggle fullscreen on F11 key press (from WM_KEYDOWN handler)
    /// WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
    /// ```
    pub(crate) fn toggle_fullscreen_for_hwnd(hwnd: HWND) {
        use windows::Win32::{
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
                MonitorFromWindow,
            },
            UI::WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, GetWindowRect, HWND_TOP, SWP_FRAMECHANGED,
                SWP_NOACTIVATE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_POPUP,
                WS_VISIBLE,
            },
        };

        let toggled = with_window_context(hwnd, "toggle_fullscreen", |ctx| {
            let current_mode = ctx.mode.get();
            let hold = ctx.hold_publishing();

            // SAFETY: `GetWindowRect`/`GetWindowLongPtrW`/`SetWindowLongPtrW`/
            // `SetWindowPos`/`MonitorFromWindow`/`GetMonitorInfoW` below all
            // take `hwnd` or a `&raw mut` to a stack-local out-parameter
            // whose size matches what each call expects, and are otherwise
            // ordinary Win32 calls whose failure mode against a dead `hwnd`
            // is an error return, not UB. The window CAN die mid-closure:
            // `SetWindowPos` synchronously re-enters `window_proc`
            // (`WM_SIZE`/`WM_MOVE`), whose arms dispatch framework
            // callbacks, and `ctx.dispatch_event` below does so directly —
            // a callback may close this window, running a nested
            // `WM_DESTROY` right here. What stays valid regardless is
            // `ctx` itself: the gate's `ContextGuard` defers the context
            // free past this closure (`WM_DESTROY` only retires it — see
            // `ContextLedger`), so post-destruction the remaining `Cell`
            // reads/writes and dead-`hwnd` Win32 calls are sound no-ops,
            // never dangling.
            unsafe {
                if let NativeMode::Fullscreen { restore_bounds } = current_mode {
                    // Exit fullscreen - restore previous style and bounds
                    tracing::info!("Exiting fullscreen mode");

                    // Validate transition
                    let candidate = NativeMode::Normal;
                    if !current_mode.can_transition_to(&candidate) {
                        tracing::warn!("Cannot exit fullscreen: invalid state transition");
                        return;
                    }

                    // Restore the style the window had, but not its visibility:
                    // a window entered hidden and shown since must stay shown.
                    let shown = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_VISIBLE.0;
                    let restore_style = (ctx.restore_style.get() & !WS_VISIBLE.0) | shown;
                    SetWindowLongPtrW(hwnd, GWL_STYLE, restore_style as isize);

                    // The window is normal from here on, so the size limits the app set
                    // (also while it was fullscreen) apply to the rectangle it returns to.
                    ctx.mode.set(NativeMode::Normal);

                    // Restore window position and size
                    if let Err(error) = SetWindowPos(
                        hwnd,
                        None,
                        restore_bounds.origin.x,
                        restore_bounds.origin.y,
                        restore_bounds.size.width,
                        restore_bounds.size.height,
                        SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
                    ) {
                        tracing::warn!(
                            ?hwnd,
                            ?error,
                            "SetWindowPos (exit fullscreen restore) failed"
                        );
                    }

                    drop(hold);
                    ctx.publish_window_state(hwnd);
                } else {
                    // Enter fullscreen - save current state and go borderless on monitor
                    tracing::info!("Entering fullscreen mode");

                    // Get current window rect
                    let mut rect = RECT::default();
                    if let Err(error) = GetWindowRect(hwnd, &raw mut rect) {
                        tracing::warn!(
                            ?hwnd,
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

                    // Validate transition
                    let candidate = NativeMode::Fullscreen { restore_bounds };
                    if !current_mode.can_transition_to(&candidate) {
                        tracing::warn!(
                            "Cannot enter fullscreen: invalid state transition from {:?}",
                            current_mode
                        );
                        return;
                    }

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
                    // or minimized window is restored first.
                    if (iconic || IsZoomed(hwnd).as_bool()) && IsWindowVisible(hwnd).as_bool() {
                        SendMessageW(
                            hwnd,
                            WM_SYSCOMMAND,
                            Some(WPARAM(SC_RESTORE as usize)),
                            Some(LPARAM(0)),
                        );
                    }
                    let mut monitor_info = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if let Err(error) = GetMonitorInfoW(monitor, &raw mut monitor_info).ok() {
                        tracing::warn!(
                            ?hwnd,
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
                        tracing::warn!(?hwnd, ?error, "SetWindowPos (enter fullscreen) failed");
                    }
                    ctx.limits_waived.set(false);

                    // Update state
                    ctx.mode.set(candidate);

                    drop(hold);
                    ctx.publish_window_state(hwnd);
                }
            }
        });
        if toggled.is_none() {
            tracing::warn!(?hwnd, "cannot toggle fullscreen: no usable window context");
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
            WindowsWindow::redraw_native(hwnd);
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
    pub(crate) fn change_dpi(&self, hwnd: HWND, dpi: u16, suggested: Option<RECT>) {
        use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

        let old_scale = self.scale.rescale(u32::from(dpi));
        let new_scale = self.scale.scale();
        tracing::info!(dpi, new_scale, "DPI changed");
        // The display's scale changed, so its snapshot entry did.
        self.displays.invalidate();
        self.step(hwnd, WindowStep::Rescaled);
        // The scale and the rectangle change together: a subscriber sees the
        // window once, at its new size.
        let hold = self.hold_publishing();
        self.update_window_state(|state| state.scale_factor = new_scale);
        self.dispatch_event(flui_platform::WindowEvent::ScaleFactorChanged {
            window_id: self.window_id,
            scale_factor: new_scale,
        });
        // The handler may have closed the window: its rectangle, resize and
        // frame are then not for whatever window has the handle now.
        if !self.still_owns(hwnd) {
            return;
        }
        self.apply_dpi_rectangle(hwnd, suggested, new_scale / old_scale);
        // So may the resize callback the rectangle's `WM_SIZE` ran.
        if !self.still_owns(hwnd) {
            return;
        }
        drop(hold);
        self.publish_window_state(hwnd);

        // The runtime takes a new scale from a resize. When the rectangle kept
        // the client size in device pixels, or the move failed, no `WM_SIZE`
        // brought it.
        let mut client = RECT::default();
        // SAFETY: `client` is a stack local out-parameter.
        let measured = unsafe { GetClientRect(hwnd, &raw mut client) }.is_ok();
        if measured && !self.mode.get().is_minimized() {
            let size = Size::new(
                (client.right - client.left).max(1),
                (client.bottom - client.top).max(1),
            );
            if self.last_resize.get() != Some((size, new_scale)) {
                self.update_window_state(|state| {
                    state.bounds.size = Size::new(
                        f64::from(size.width) / new_scale,
                        f64::from(size.height) / new_scale,
                    );
                });
                self.publish_window_state(hwnd);
                self.dispatch_resize(size);
                self.redraw_if_still_owned(hwnd);
            }
        }
        // The field's screen rectangles follow the new scale and rectangle.
        self.step(hwnd, WindowStep::ScaleApplied);
        // After the state above is current: the hint wakes the owner, which
        // samples the snapshot and re-derives.
        self.callbacks.dispatch_display_changed();
    }

    /// Give the window the rectangle its new DPI asks for; `ratio` is the new
    /// scale over the old.
    ///
    /// A window that is not fullscreen takes the rectangle the system
    /// suggests. A fullscreen window covers its monitor again instead, which
    /// the suggestion, scaled from its old size, need not do; the rectangle
    /// it returns to is rescaled with it, as the system would have scaled it.
    pub(crate) fn apply_dpi_rectangle(&self, hwnd: HWND, suggested: Option<RECT>, ratio: f64) {
        use windows::Win32::{
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            },
            UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos},
        };

        let mode = self.mode.get();
        if let Some(bounds) = mode.fullscreen_bounds()
            && let (Some(width), Some(height)) = (
                rescaled(bounds.size.width, ratio),
                rescaled(bounds.size.height, ratio),
            )
        {
            self.mode.set(mode.with_fullscreen_bounds(Bounds {
                origin: bounds.origin,
                size: Size::new(width, height),
            }));
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
            tracing::warn!(?hwnd, ?error, "SetWindowPos (DPI change) failed");
        }
    }
}

/// What hides a window whose surface is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Occlusion {
    /// The desktop window manager cloaks the window.
    pub(crate) cloaked: bool,
    /// The user's session is locked.
    pub(crate) session_locked: bool,
}

/// The parts of a window's visibility, and what the runtime was last told.
pub(crate) struct Visibility {
    /// What the surface rules last said: shown and not minimized.
    surface: Cell<bool>,
    occlusion: Cell<Occlusion>,
    /// Last value dispatched through
    /// `WindowCallbacks::dispatch_visibility_status_change`.
    reported: Cell<ReportedVisibility>,
}

impl Visibility {
    /// A window whose surface starts out `surface`, in a session that is
    /// locked now or not. The runtime's first reading
    /// (`PlatformWindow::is_visible`) leaves out the occlusion.
    pub(crate) fn new(surface: bool) -> Self {
        let occlusion = Occlusion {
            cloaked: false,
            session_locked: session_locked_now(),
        };
        Self {
            surface: Cell::new(surface),
            occlusion: Cell::new(occlusion),
            reported: Cell::new(ReportedVisibility::new(
                surface && !occlusion.session_locked,
            )),
        }
    }

    /// Whether something hides the window whatever its surface.
    pub(crate) fn occluded(&self) -> bool {
        let occlusion = self.occlusion.get();
        occlusion.cloaked || occlusion.session_locked
    }

    fn visible(&self) -> bool {
        let occlusion = self.occlusion.get();
        self.surface.get() && !occlusion.cloaked && !occlusion.session_locked
    }
}

impl WindowContext {
    /// The surface rules say the window's surface is (`true`) or is not
    /// shown; the runtime hears of a change in its visibility.
    pub(crate) fn report_surface_visibility(&self, surface: bool) {
        self.visibility.surface.set(surface);
        self.settle_visibility();
    }

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
        // A cloak or a lock hides a shown window in its snapshot too: read it
        // again, whether or not the report below changes.
        self.snapshot.invalidate();
        let visible = self.visibility.visible();
        if let Some(visible) = ReportedVisibility::report_in(&self.visibility.reported, visible) {
            tracing::debug!(visible, "window visibility changed");
            self.callbacks.dispatch_visibility_status_change(visible);
        }
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
            && unsafe { info.Data.WTSInfoExLevel1 }.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK
    });
    // SAFETY: the buffer the query allocated, freed once.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    locked.unwrap_or(false)
}

/// The window's subscription to the cloak changes of its thread's windows;
/// unsubscribed when dropped, which happens on the owner thread with the
/// window's context.
pub(crate) struct CloakWatch(Option<HWINEVENTHOOK>);

impl CloakWatch {
    /// Subscribe on the calling thread, the window's owner. Without a
    /// subscription the window is never seen as cloaked.
    pub(crate) fn start() -> Self {
        // Every source, not this thread's: the shell cloaks the windows of a
        // virtual desktop it switches away from, so the event comes from its
        // thread. `cloak_changed` keeps only this thread's FLUI windows.
        //
        // SAFETY: `cloak_changed` has the `WINEVENTPROC` signature and stays
        // valid for the process; an out-of-context hook runs it on the thread
        // that installed it, this one, while it takes messages.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_CLOAKED,
                EVENT_OBJECT_UNCLOAKED,
                None,
                Some(cloak_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            tracing::warn!("cloak changes unavailable: a cloaked window stays visible");
            return Self(None);
        }
        Self(Some(hook))
    }

    fn is(&self, hook: HWINEVENTHOOK) -> bool {
        self.0 == Some(hook)
    }
}

impl Drop for CloakWatch {
    fn drop(&mut self) {
        if let Some(hook) = self.0.take() {
            // SAFETY: the hook this watch installed on this thread, released
            // once.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}

/// Every window of the thread has its own hook, and each event reaches all of
/// them: a window takes only the events of its own hook about itself. The
/// context gate refuses another thread's or process's window.
unsafe extern "system" fn cloak_changed(
    hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    // The visibility callback is user code. As in `window_proc`, the state
    // behind a panic mid-dispatch may be torn: it is logged, then the process
    // aborts.
    os_entry("the cloak-change hook", hwnd, AfterPanic::Abort, || {
        if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
            return;
        }
        let _ = with_window_context(hwnd, "cloak change", |ctx| {
            if ctx.cloak_watch.is(hook) {
                ctx.refresh_cloaked(hwnd);
            }
        });
    });
}

/// Wake deadlines while the system runs a modal loop for the window (moving
/// or sizing it, its menus): a timer of the window delivers what is due while
/// the main loop waits (`owner::windows::modal_loop`).
impl WindowContext {
    /// A modal loop for the window starts.
    pub(crate) fn enter_modal_loop(&self, hwnd: HWND) {
        if self.modal_pacing.enter() {
            arm_modal_timer(hwnd, crate::owner::modal_pace::MODAL_FRAME_PACE_MS);
        }
    }

    /// A modal loop for the window ends.
    pub(crate) fn exit_modal_loop(&self, hwnd: HWND) {
        if self.modal_pacing.exit() {
            // SAFETY: a plain call on the window's handle; a timer that is
            // not set fails harmlessly.
            let _ = unsafe { KillTimer(Some(hwnd), MODAL_DEADLINE_TIMER) };
        }
    }

    /// The pacing timer fired: deliver what is due and arm it again.
    pub(crate) fn pace_modal_loop(&self, hwnd: HWND) {
        if !self.modal_pacing.running() {
            // SAFETY: as in `exit_modal_loop`.
            let _ = unsafe { KillTimer(Some(hwnd), MODAL_DEADLINE_TIMER) };
            return;
        }
        let wait = self.modal_pacing.frames().deliver();
        if self.modal_pacing.running() {
            arm_modal_timer(hwnd, wait);
        }
    }
}

fn arm_modal_timer(hwnd: HWND, millis: u32) {
    // SAFETY: a plain call on the window's handle with no callback: the
    // timer posts `WM_TIMER`, and setting it again replaces it.
    let _ = unsafe { SetTimer(Some(hwnd), MODAL_DEADLINE_TIMER, millis, None) };
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

    /// After a window was first shown. A window that came up minimized is not
    /// visible to the runtime, whatever it was told before (a deferred window
    /// is told it is visible so that its frames run). Whether it did is read
    /// from the window, not from the mode asked for: a launcher can choose the
    /// first show state.
    pub(crate) fn end_first_show(&self, minimized: bool) {
        if minimized {
            self.report_surface_visibility(false);
        }
    }

    /// Tell the runtime the window's real visibility if what it was told after
    /// a muted restore disagrees with it (a show that came in meanwhile).
    pub(crate) fn reconcile_visibility(&self, visible: bool) {
        self.report_surface_visibility(visible);
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
            super::set::outer_size(style, ex_style, dpi, (0, 0)).unwrap_or((0, 0));
        Some(track_limits(
            constraints.min,
            constraints.max,
            dpi,
            PixelSize::new(frame_width, frame_height),
        ))
    }

    /// Fill in `WM_GETMINMAXINFO` with the app's size limits at the window's
    /// current DPI. A fullscreen window keeps the system's rectangle.
    pub(super) fn fill_min_max_info(&self, hwnd: HWND, info: &mut MINMAXINFO) {
        use super::limits::{MinMaxInfo, PixelSize, constrain};

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

    /// The window's own keys, for a keydown the application let through: the
    /// fullscreen hotkey (configurable, default F11) toggles on a turn of its
    /// own, as a window command does, not inside the keydown, because the
    /// resize it causes renders. The request names this window, not just its
    /// handle.
    pub(crate) fn apply_window_hotkey(&self, hwnd: HWND, virtual_key: u16, is_repeat: bool) {
        if is_repeat || self.config.fullscreen_hotkey != Some(virtual_key) {
            return;
        }
        let (low, high) = self.identity.halves();
        // SAFETY: plain values posted to a window of this thread; a handle
        // that closed meanwhile only makes the call fail.
        let _ = unsafe { PostMessageW(Some(hwnd), TOGGLE_FULLSCREEN, WPARAM(low), LPARAM(high)) };
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
