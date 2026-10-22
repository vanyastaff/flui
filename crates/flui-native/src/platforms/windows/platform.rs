//! A window's native context and the window's part of FLUI's window
//! procedure: the context each window keeps in its `GWLP_USERDATA` slot, the
//! gates that reach it on the window's owner thread, and [`WindowProc`], the
//! hook that handles the messages of the window, its input, text, drag and
//! lifecycle.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Point, Size};
use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, PAINTSTRUCT},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
        WindowsAndMessaging::{
            DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClassNameW, GetWindowLongPtrW,
            GetWindowThreadProcessId, HTCLIENT, ISMEX_NOSEND, InSendMessageEx, MINMAXINFO,
            PostMessageW, SC_KEYMENU, SetWindowLongPtrW, WINDOWPOS, WM_CAPTURECHANGED, WM_CHAR,
            WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO,
            WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE,
            WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SETTEXT,
            WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SIZE, WM_STYLECHANGED, WM_SYSCOMMAND,
            WM_SYSKEYDOWN, WM_SYSKEYUP, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING,
        },
    },
};

use super::{
    display::WindowsDisplays,
    events::{get_x_lparam, get_y_lparam},
    window::{NativeMode, WindowsWindow},
};
use crate::owner::call_point::{Field, call};
use crate::owner::windows::{
    WINDOW_CLASS_NAME, WindowIdentity, WindowMessages, modal_loop::MODAL_DEADLINE_TIMER,
};
use crate::text::windows::TextServices;
use crate::{
    config::WindowConfiguration, owner::handlers::PlatformHandlers,
    owner::handlers::WindowCallbacks, shared::hwnd_affinity::ContextLedger,
    shared::window_state::WindowStateCell,
};
use flui_platform::{WindowAppearance, WindowEvent, WindowId};

#[path = "platform/pointer_messages.rs"]
mod pointer_messages;

#[path = "platform/keystroke_queue.rs"]
mod keystroke_queue;
use keystroke_queue::{
    drain_dead_char, drain_translated_chars, restore_translated_sys_chars,
    take_translated_sys_chars,
};

#[cfg(test)]
#[path = "platform/preference_retry_tests.rs"]
mod preference_retry_tests;

#[cfg(test)]
#[path = "platform/display_message_tests.rs"]
mod display_message_tests;

/// One window as an object that outlives its borrow of the window names it:
/// the handle and the identity of the context created with it. The OS
/// recycles handles, so a handle alone can name a later window;
/// [`Self::with_context`] reaches only the context this window was created
/// with. Plain values, so an `Arc`-shared object can hold one on any thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WindowRef {
    hwnd: isize,
    identity: WindowIdentity,
}

impl WindowRef {
    pub(crate) fn new(hwnd: HWND, identity: WindowIdentity) -> Self {
        Self {
            hwnd: hwnd.0 as isize,
            identity,
        }
    }

    pub(crate) fn hwnd(self) -> HWND {
        HWND(self.hwnd as *mut core::ffi::c_void)
    }

    /// [`with_window_context`] for this window only: `None` off its owner
    /// thread, after it closed, or when its handle now names another window.
    pub(crate) fn with_context<R>(
        self,
        op: &'static str,
        f: impl FnOnce(&WindowContext) -> R,
    ) -> Option<R> {
        with_window_context(self.hwnd(), op, |context| {
            (context.identity == self.identity).then(|| f(context))
        })
        .flatten()
    }
}

/// The platform's tracked top-level windows, keyed by HWND. The owner
/// message-only windows are never in it.
pub(crate) type WindowMap = Mutex<HashMap<isize, Arc<WindowsWindow>>>;

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
    /// Tracking and cache access do not keep wrappers or their state alive.
    pub windows: std::sync::Weak<WindowMap>,
    pub(super) window_state: std::sync::Weak<Mutex<super::window::WindowRecord>>,
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
    pub displays: Arc<WindowsDisplays>,
    /// The monitor handle `WM_WINDOWPOSCHANGED` last saw this window on, `0`
    /// before the first message; a change raises `on_display_changed`.
    pub last_monitor: std::cell::Cell<isize>,
    /// Per-window callbacks for event delivery, owned by this context and
    /// released by `WM_DESTROY` on the owner thread.
    pub callbacks: WindowCallbacks,
    /// Scale factor for coordinate conversion.
    ///
    /// `Cell`, not a plain `f64`: `window_proc` only ever holds a shared
    /// `&WindowContext` (see its `# Safety` section), and `WM_DPICHANGED` is
    /// the one message that updates this field — `Cell::set` lets it do so
    /// through that shared reference instead of forging a second, aliasing
    /// `&mut WindowContext` from the raw `GWLP_USERDATA` pointer while the
    /// shared one is still live.
    pub scale_factor: std::cell::Cell<f64>,
    pub(super) message_clock: super::events::MessageClock,
    pub pointer_registry: RefCell<crate::shared::pointer_assembly::ContactRegistry>,
    /// The precision touchpad's DirectManipulation viewport; `None` when it
    /// did not initialise (the window then has no precise scrolling or
    /// trackpad gestures) or after it failed.
    pub(super) touchpad: super::direct_manipulation::TouchpadSlot,
    /// Current window mode (replaces display_state + saved bounds)
    pub mode: std::cell::Cell<NativeMode>,
    /// Last known size (before minimization) for restore detection
    pub last_size: std::cell::Cell<Size<i32>>,
    /// Window configuration (hotkeys, debouncing, etc.)
    pub config: WindowConfiguration,
    /// Whether the mouse is over this window; see [`Self::set_hovered`].
    pub is_hovered: std::cell::Cell<bool>,
    /// The system appearance last applied to this window's chrome and
    /// reported to `on_appearance_changed`.
    pub appearance: std::cell::Cell<WindowAppearance>,
    /// Whether the title bar follows the system theme; `false` once the app
    /// chose it through `WindowsWindowExt::set_dark_mode` or `set_theme`.
    pub chrome_follows_system: std::cell::Cell<bool>,
    /// Current keyboard modifiers.
    pub modifiers: std::cell::Cell<flui_platform::Modifiers>,
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
    pub(super) first_show: std::cell::Cell<super::window_frame::FirstShow>,
    /// The window's visibility as the runtime sees it (hidden-surface
    /// gating): the surface, which `WM_SIZE` (minimize/restore) and
    /// `WM_SHOWWINDOW` (hide/show) report through the pure rules in
    /// `crate::shared::visibility`, less a cloak or a locked session. Seeded
    /// from the window's ACTUAL creation style
    /// (`win32_initial_visibility(GWL_STYLE)`, see its doc) — a decorated
    /// window is created hidden (`false`, and the creation-time
    /// `ShowWindow`'s `WM_SHOWWINDOW` edge flips it), while an undecorated
    /// window is created `WS_POPUP | WS_VISIBLE` (`true`, and no
    /// creation-time edge ever fires). A constant seed would desync the
    /// edge filter for one of the two and swallow that window's first real
    /// transition.
    pub(super) visibility: super::window_visibility::Visibility,
    /// The window's subscription to cloak changes.
    pub(super) cloak_watch: super::window_visibility::CloakWatch,
    /// Set between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`: a live resize,
    /// whose `WM_SIZE` renders at once.
    pub in_size_move: std::cell::Cell<bool>,
    /// The client size, in device pixels, and the scale the last resize
    /// callback delivered.
    pub last_resize: std::cell::Cell<Option<(Size<i32>, f64)>>,
    /// Delivers wake deadlines while a modal loop runs for the window.
    pub modal_pacing: crate::owner::windows::modal_loop::ModalPacing,
    /// High half of a UTF-16 surrogate pair from an out-of-band `WM_CHAR`,
    /// held until its low half arrives in the next message (Windows splits
    /// astral-plane characters across two `WM_CHAR`s). Only the stray-char
    /// path in `window_proc` uses this; the `WM_KEYDOWN` drain sees a whole
    /// burst at once and never needs cross-message state.
    pub pending_high_surrogate: std::cell::Cell<Option<u16>>,
    /// The window's TSF connection and text-store host (ADR-0135 §3),
    /// activated as the window is created, on its owner thread; `None` when
    /// activation failed, and text input takes the `WM_CHAR` path.
    /// Deactivated by `WM_DESTROY` after the close callbacks and before the
    /// context retires ([`TextServices::retire_with_window`]).
    pub text_services: RefCell<Option<Rc<TextServices>>>,
    /// The window's registered OLE drop target; `None` when registration
    /// failed (the window then takes no drags) or after `WM_DESTROY`
    /// revoked it.
    pub(crate) drop_target:
        RefCell<Option<windows_core::ComObject<crate::drag::windows::WindowDropTarget>>>,
    /// The platform's drag-and-drop transport, which the window's drop target
    /// serves; the window class's drop-target hook registers it from here.
    pub(crate) drag: Arc<dyn crate::owner::windows::HostDrag>,
    /// Physical keys held down that were pressed as dead keys, so their
    /// release reports `Dead` like the press.
    pub held_dead_keys: std::cell::RefCell<super::events::HeldDeadKeys>,
    /// Borrow ledger deferring this context's free past every live borrow
    /// on the owner thread — see [`ContextLedger`] for the reentrancy
    /// hazard (a framework callback closing the window from inside a
    /// dispatch) this exists to survive, and [`ContextGuard`] for the RAII
    /// half. `RefCell`, not a lock: only the owning thread ever touches it
    /// (the affinity gates enforce that), and every borrow is a short
    /// non-reentrant method call.
    pub ledger: std::cell::RefCell<ContextLedger>,
}

impl WindowContext {
    /// Runs this window's frame callback; its callbacks count it on the
    /// platform only if one is registered to run.
    pub(crate) fn request_frame(&self) {
        self.callbacks.dispatch_request_frame();
    }

    /// Tell the window's resize callback its client size, `device` pixels, at
    /// its current scale.
    pub(super) fn dispatch_resize(&self, device: Size<i32>) {
        let scale = self.scale_factor.get();
        self.last_resize.set(Some((device, scale)));
        if let Ok(metrics) = flui_foundation::geometry::WindowMetrics::new(device, scale) {
            self.callbacks.dispatch_resize(metrics);
        }
    }

    /// Complete cache updates before callback-capable native/user code.
    pub(super) fn update_window_state(
        &self,
        update: impl FnOnce(&mut super::window::WindowRecord),
    ) {
        if let Some(state) = self.window_state.upgrade() {
            update(&mut state.lock());
        }
    }

    /// The window's published state, read from the OS again first when
    /// something invalidated it; `on_state_changed` is raised when that
    /// produced a new revision.
    pub(super) fn window_state(&self, hwnd: HWND) -> flui_platform::WindowState {
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
            .state_with(|| super::window::sample_state(hwnd, &record, self.tracking(hwnd)));
        if published.changed {
            self.callbacks.dispatch_state_changed();
        }
        published.state
    }

    /// What this context knows of the window beyond what Win32 reports.
    pub(super) fn tracking(&self, hwnd: HWND) -> super::window::OwnerTracking {
        super::window::OwnerTracking {
            mode: self.mode.get(),
            restore_style: self.restore_style.get(),
            constraints: self.constraints.get(),
            display: self.displays.display_of_window(hwnd),
        }
    }

    /// The window may differ from its published state: read it again.
    pub(super) fn publish_window_state(&self, hwnd: HWND) {
        self.snapshot.invalidate();
        // A change that takes several steps publishes once, when it is done:
        // a subscriber never sees the window half-changed.
        if self.publish_hold.get() == 0 {
            let _ = self.window_state(hwnd);
        }
    }

    /// Hold back publishing until the returned guard drops; the caller then
    /// publishes once.
    pub(super) fn hold_publishing(&self) -> PublishHold<'_> {
        self.publish_hold
            .set(self.publish_hold.get().saturating_add(1));
        PublishHold(&self.publish_hold)
    }

    /// The window changed DPI: once the new scale is applied, the focused
    /// field's input method follows it (a move reaches TSF through its class hook).
    pub(super) fn text_layout_moved(&self) {
        let services = self.text_services.borrow().clone();
        if let Some(services) = services {
            services.window_moved();
        }
    }

    /// Dispatch a window event to the platform-level handler.
    #[inline]
    pub(super) fn dispatch_event(&self, event: WindowEvent) {
        self.call_handler(
            |handlers| &mut handlers.window_event,
            |handler| handler(event),
        );
    }

    /// Dispatch a keyboard-layout change to the platform-level hook.
    pub(super) fn dispatch_keyboard_layout_change(&self) {
        self.call_handler(
            |handlers| &mut handlers.keyboard_layout_changed,
            |handler| handler(),
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
/// [`WindowContext`] pointer. Guards against OS handle recycling: a dead
/// handle value can be reissued for an unrelated window whose user-data
/// slot follows someone else's protocol entirely.
fn hwnd_names_flui_class(hwnd: HWND) -> bool {
    let mut name = [0_u16; 256];
    // SAFETY: `GetClassNameW` writes at most `name.len()` UTF-16 units
    // (including the terminator) into the live stack buffer the slice
    // points at, and returns the count copied EXCLUDING the terminator (0
    // on failure — a dead or foreign handle simply reports "not our class"
    // here). `WINDOW_CLASS_NAME.as_wide()` walks our own `w!(..)` static
    // literal, which is NUL-terminated by construction, and measures it
    // with `wcslen` — so it also EXCLUDES the terminator, matching the
    // length convention `class_name_matches` documents and its Linux tests
    // pin.
    let (copied, expected) =
        unsafe { (GetClassNameW(hwnd, &mut name), WINDOW_CLASS_NAME.as_wide()) };
    crate::shared::hwnd_affinity::class_name_matches(&name, copied, expected)
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
/// Linux-tested rules in [`crate::shared::hwnd_affinity`]; this shell only
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
) -> Result<R, crate::shared::hwnd_affinity::UserDataRefusal> {
    use crate::shared::hwnd_affinity::{UserDataVerdict, classify_user_data_access};

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
/// [`crate::shared::hwnd_affinity::route_teardown`], which also states the
/// pointer-ABA residual; this shell only performs the OS queries.
pub(super) fn teardown_route(
    hwnd: HWND,
    expected_context: *const WindowContext,
) -> crate::shared::hwnd_affinity::TeardownRoute {
    // SAFETY: see the identical queries in `with_window_context` — no
    // pointers dereferenced, no messages pumped.
    let (owner_thread, current_thread, slot) = unsafe {
        (
            GetWindowThreadProcessId(hwnd, None),
            GetCurrentThreadId(),
            GetWindowLongPtrW(hwnd, GWLP_USERDATA),
        )
    };
    crate::shared::hwnd_affinity::route_teardown(
        owner_thread,
        current_thread,
        hwnd_names_flui_class(hwnd),
        slot,
        expected_context as isize,
    )
}

/// The window's hook on FLUI's window class: it answers every message of the
/// class's windows — the window's own, and its input, text, drag and
/// lifecycle messages — passing what it does not handle to `DefWindowProcW`
/// itself.
pub(crate) struct WindowProc;

impl WindowMessages for WindowProc {
    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        // SAFETY: the arguments as Win32 delivered them to the class's
        // procedure, which is `window_proc_body`'s contract too.
        Some(unsafe { Self::window_proc_body(hwnd, msg, wparam, lparam) })
    }
}

impl WindowProc {
    /// The message handling behind FLUI's window procedure
    /// ([`WindowMessages::message`]). Every dispatch of user callbacks stays
    /// inside that procedure's panic boundary.
    ///
    /// # Safety
    ///
    /// Must only be called with arguments Win32 delivered to this class's
    /// `WNDPROC` for the message being handled — Win32 guarantees
    /// `hwnd`/`msg`/`wparam`/`lparam` are well-formed for that message, and
    /// always dispatches on the thread that owns the window's message queue,
    /// so the `GWLP_USERDATA` read below does not race `WM_DESTROY`'s
    /// clear+free precisely because both run here, serialized by that
    /// same-thread dispatch guarantee.
    unsafe fn window_proc_body(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: see the `# Safety` section above for the `GWLP_USERDATA`
        // contract this read relies on. The rest of this single `unsafe`
        // block covers every Win32 call in the `match` below: `GetWindowLongPtrW`/
        // `SetWindowLongPtrW` take `hwnd` and plain integers, no pointers;
        // `TrackMouseEvent` (`WM_MOUSEMOVE`) and `BeginPaint`/`EndPaint`
        // (`WM_PAINT`, its own SAFETY note below) take `&raw` pointers to
        // stack-local, correctly-sized structs; `DestroyWindow` calls are
        // individually commented where they appear; `DefWindowProcW`
        // forwards unhandled messages verbatim with no pointer arguments of
        // its own.
        unsafe {
            // Get window context from GWLP_USERDATA. The guard makes `ctx`
            // survive reentrant dispatch: arms below hold it across
            // framework callbacks, and a callback may close this window —
            // running a nested `WM_DESTROY` frame on this same thread that
            // RETIRES the context (clears the slot, marks the ledger)
            // without freeing it; the free happens when the outermost
            // guard, possibly this frame's, releases (see `ContextGuard`).
            //
            // SAFETY(acquire): this frame is Win32's dispatch for `hwnd` on
            // its owning thread, and a non-null slot means the context is
            // not yet retired — exactly `ContextGuard::acquire`'s contract.
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowContext;
            let guard = if ctx_ptr.is_null() {
                None
            } else {
                Some(ContextGuard::acquire(ctx_ptr))
            };
            let ctx = guard.as_ref().map(ContextGuard::context);

            match msg {
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
                    // The window hears its session lock and unlock itself;
                    // `WM_DESTROY` unsubscribes.
                    if let Err(error) =
                        windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(
                            hwnd,
                            windows::Win32::System::RemoteDesktop::NOTIFY_FOR_THIS_SESSION,
                        )
                    {
                        tracing::warn!(%error, "session notifications unavailable: a locked session leaves the window visible");
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
                    let _ = windows::Win32::System::RemoteDesktop::WTSUnRegisterSessionNotification(
                        hwnd,
                    );

                    if let Some(ctx) = ctx {
                        // Whatever the precision touchpad has open ends as
                        // cancelled, and its viewport is released, while the
                        // window and its callbacks still exist.
                        ctx.release_touchpad(hwnd);

                        // A drag over the window leaves it, and OLE no
                        // longer offers it one.
                        crate::drag::windows::revoke_target(hwnd, &ctx.drop_target);

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

                        // Deactivate TSF for the window while the HWND is
                        // still valid: after the close callbacks, which
                        // closed the presentation and so unfocused its
                        // field, and before the context retires (ADR-0135
                        // §3). Taken out in a statement of its own: the
                        // shutdown reaches application code, which must
                        // find no borrow held.
                        let services = ctx.text_services.borrow_mut().take();
                        if let Some(services) = services {
                            TextServices::retire_with_window(services);
                        }

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
                        let removed = ctx.windows.upgrade().and_then(|windows| {
                            let mut windows = windows.lock();
                            if windows
                                .get(&(hwnd.0 as isize))
                                .is_some_and(|window| window.identity == ctx.identity)
                            {
                                windows
                                    .remove(&(hwnd.0 as isize))
                                    .map(|window| (window, windows.is_empty()))
                            } else {
                                None
                            }
                        });
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

                // The system offers a contact to the window under it; a
                // touchpad's goes to the DirectManipulation viewport.
                windows::Win32::UI::WindowsAndMessaging::DM_POINTERHITTEST => {
                    if let Some(ctx) = ctx {
                        ctx.offer_touchpad_contact(hwnd, (wparam.0 & 0xffff) as u32);
                    }
                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_ENTERSIZEMOVE => {
                    if let Some(ctx) = ctx {
                        ctx.pause_touchpad(hwnd);
                        ctx.in_size_move.set(true);
                        ctx.enter_modal_loop(hwnd);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_EXITSIZEMOVE => {
                    if let Some(ctx) = ctx {
                        ctx.exit_modal_loop(hwnd);
                        ctx.in_size_move.set(false);
                        ctx.resume_touchpad();
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

                windows::Win32::UI::WindowsAndMessaging::WM_TIMER
                    if WindowContext::is_touchpad_timer(wparam.0) =>
                {
                    if let Some(ctx) = ctx {
                        ctx.pump_touchpad(hwnd);
                    }
                    LRESULT(0)
                }

                WM_PAINT => {
                    // The frame follows the scrolling it shows.
                    if let Some(ctx) = ctx {
                        ctx.pump_touchpad_for_frame(hwnd);
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
                    use crate::shared::visibility::{
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
                            ctx.resize_touchpad((width, height));
                            ctx.update_window_state(|state| {
                                state.bounds.size = Size::new(
                                    width as f64 / ctx.scale_factor.get(),
                                    height as f64 / ctx.scale_factor.get(),
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
                            ctx.cancel_pointers(hwnd);
                        }

                        // Hidden-surface gating, minimize/restore half: it
                        // reaches `dispatch_visibility_status_change`, the wire
                        // the runtime's `FrameClock` gate and `AppLifecycleState`
                        // derivation hang off. Decided by the pure rule in
                        // `shared::visibility` (host-tested).
                        if !ctx.visibility_muted.get()
                            && let Some(visible) = crate::shared::visibility::win32_size_visibility(
                                size_type,
                                prev_mode.is_minimized(),
                            )
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
                            && let Some(visible) =
                                crate::shared::visibility::win32_show_window_visibility(
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
                                x as f64 / ctx.scale_factor.get(),
                                y as f64 / ctx.scale_factor.get(),
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
                        let monitor = super::display::nearest_monitor(hwnd);
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

                // The window's own subscription (`WM_CREATE`): a locked
                // session hides every window, whatever its surface.
                crate::shared::windows_host_messages::WM_WTSSESSION_CHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.session_changed(wparam.0, lparam.0);
                    }
                    LRESULT(0)
                }

                // Toggled after the key's dispatch, on a turn of its own (see
                // `WM_KEYDOWN`).
                // A handle the window's close let another window reuse is not
                // the window whose key asked for it.
                super::window::TOGGLE_FULLSCREEN => {
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
                super::window::REQUEST_FRAME => {
                    if let Some(ctx) = ctx {
                        ctx.request_frame();
                        ctx.dispatch_event(WindowEvent::RedrawRequested {
                            window_id: ctx.window_id,
                        });
                    }
                    LRESULT(0)
                }

                WM_MOUSEMOVE => {
                    if let Some(ctx) = ctx {
                        let time = ctx.message_clock.message_time();
                        // Request WM_MOUSELEAVE notification for hover tracking
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&raw mut tme);
                        // Cleared by WM_MOUSELEAVE.
                        ctx.set_hovered(true);

                        let events = super::events::mouse_move_input(
                            &ctx.pointer_registry,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                            time,
                        );
                        for event in events {
                            if ctx.pointer_registry.borrow().delivers(&event) {
                                ctx.callbacks.dispatch_input(event);
                            }
                        }
                    }
                    LRESULT(0)
                }

                WM_SETCURSOR => {
                    if let Some(ctx) = ctx
                        && (lparam.0 as u32 & 0xffff) == HTCLIENT
                    {
                        match WindowsWindow::apply_native_cursor(ctx.cursor.get()) {
                            Ok(()) => return LRESULT(1),
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

                WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_RBUTTONDOWN
                | WM_RBUTTONUP
                | WM_MBUTTONDOWN
                | WM_MBUTTONUP
                | windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONDOWN
                | windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONUP => {
                    use super::events::{
                        button_message, capture_on_press, mouse_button_input, release_capture_after,
                    };
                    let Some((button, is_down)) = button_message(msg, wparam) else {
                        return LRESULT(0);
                    };
                    let timed_context = ctx.map(|ctx| (ctx, ctx.message_clock.message_time()));
                    // The capture follows the native button state even with
                    // no context to deliver to, and is settled before user
                    // code runs: a release outside the client area must
                    // still reach this window, and the sequence's own
                    // release must not read as a stolen capture.
                    if is_down {
                        capture_on_press(hwnd);
                    } else {
                        release_capture_after(hwnd, wparam);
                    }
                    if let Some((ctx, time)) = timed_context {
                        let events = mouse_button_input(
                            &ctx.pointer_registry,
                            button,
                            is_down,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                            time,
                        );
                        for event in events {
                            if ctx.pointer_registry.borrow().delivers(&event) {
                                ctx.callbacks.dispatch_input(event);
                            }
                        }
                    }
                    LRESULT(isize::from(matches!(
                        msg,
                        windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONDOWN
                            | windows::Win32::UI::WindowsAndMessaging::WM_XBUTTONUP
                    )))
                }

                windows::Win32::UI::WindowsAndMessaging::WM_POINTERDOWN
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERUP
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERUPDATE
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERENTER
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERLEAVE
                | windows::Win32::UI::WindowsAndMessaging::WM_POINTERCAPTURECHANGED => {
                    if let Some(ctx) = ctx {
                        ctx.pointer_message(hwnd, msg, wparam);
                    }
                    // Consume the native route: DefWindowProc would promote it
                    // to a second legacy mouse event.
                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_POINTERDEVICECHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.pointer_device_changed(wparam, lparam);
                    }
                    LRESULT(0)
                }

                WM_CAPTURECHANGED => {
                    // Another window, a modal loop or `WM_CANCELMODE` took
                    // the capture mid-press: this window will never see the
                    // sequence's release, so end it here.
                    if let Some(ctx) = ctx {
                        let cancels = super::events::capture_changed_input(
                            hwnd,
                            lparam,
                            ctx.message_clock.message_time(),
                            &ctx.pointer_registry,
                        );
                        for cancel in cancels {
                            ctx.callbacks.dispatch_input(cancel);
                        }
                    }
                    LRESULT(0)
                }

                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    if let Some(ctx) = ctx {
                        use crate::shared::pointer_assembly::WheelAxis;
                        let axis = if msg == WM_MOUSEWHEEL {
                            WheelAxis::Vertical
                        } else {
                            WheelAxis::Horizontal
                        };
                        let time = ctx.message_clock.message_time();
                        let event = super::events::mouse_wheel_input(
                            &ctx.pointer_registry,
                            hwnd,
                            axis,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                            time,
                        );
                        if let Some(event) = event {
                            ctx.callbacks.dispatch_input(event);
                        }
                    }
                    LRESULT(0)
                }

                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let vk = wparam.0 as u16;
                    let is_repeat = (lparam.0 & (1 << 30)) != 0;

                    if let Some(ctx) = ctx {
                        // Read before the character drain below retrieves
                        // other messages.
                        let time = ctx.message_clock.message_time();

                        // Track modifiers.
                        ctx.modifiers.set(super::events::message_modifiers());

                        // Merge the fully-translated character(s) for this
                        // keystroke into the keydown. The message loop runs
                        // `TranslateMessage` before `DispatchMessageW` (see
                        // `run`), so the WM_CHAR burst for this keydown —
                        // shift/caps/AltGr applied, dead keys resolved — is
                        // already in the queue and is drained synchronously
                        // here. Pairing model and merge rules:
                        // `crate::shared::keys` module doc.
                        //
                        // A keydown sent from another thread never passed
                        // through this thread's `TranslateMessage`, so nothing
                        // queued here is its own. It takes nothing: such a
                        // keydown is what a drain below can dispatch (`PeekMessageW`
                        // delivers pending cross-thread sent messages first), and
                        // draining from there would take the outer keystroke's
                        // characters.
                        let owns_burst = InSendMessageEx(None) == ISMEX_NOSEND;
                        let translated = if owns_burst {
                            drain_translated_chars(hwnd)
                        } else {
                            None
                        };
                        // A dead key queues `WM_DEADCHAR` (`WM_SYSDEADCHAR` under
                        // Alt) instead of a character, on the layer the held
                        // modifiers select. Taking it now leaves nothing for a
                        // nested keydown during the callback to mistake for its own.
                        let stroke =
                            if owns_burst && translated.is_none() && drain_dead_char(hwnd, msg) {
                                super::events::Keystroke::Dead
                            } else {
                                super::events::Keystroke::Text(translated)
                            };
                        // The default a system keydown may skip also lives in
                        // the WM_SYSCHAR `TranslateMessage` queued for it:
                        // `DefWindowProcW` turns Alt+Space's into the system
                        // menu. Hold it before user code runs, since a
                        // handler that pumps messages (a modal API) would
                        // otherwise dispatch it before the result is known.
                        let held = if owns_burst && msg == WM_SYSKEYDOWN {
                            take_translated_sys_chars(hwnd)
                        } else {
                            Vec::new()
                        };

                        // Dispatch keyboard event via per-window callback
                        use super::events::key_down_event;
                        let event = {
                            let mut held = ctx.held_dead_keys.borrow_mut();
                            key_down_event(wparam, lparam, stroke, &mut held, time)
                        };
                        let result = ctx.callbacks.dispatch_input(event);
                        // A callback can close this window and create another
                        // with a recycled HWND. The entry guard pins the old
                        // context, so matching userdata proves the native
                        // default still targets this exact live window.
                        if result.default_prevented
                            || GetWindowLongPtrW(hwnd, GWLP_USERDATA) != ctx_ptr as isize
                        {
                            // Withdrawn with its keydown: the held
                            // characters are dropped here.
                            return LRESULT(0);
                        }
                        // The fullscreen hotkey (configurable, default F11) is
                        // a default the app can prevent. It toggles on a turn
                        // of its own, as a window command does, not inside
                        // this message: the resize it causes renders. The
                        // request names this window, not just its handle.
                        if ctx.config.fullscreen_hotkey == Some(vk) && !is_repeat {
                            let (low, high) = ctx.identity.halves();
                            let _ = PostMessageW(
                                Some(hwnd),
                                super::window::TOGGLE_FULLSCREEN,
                                WPARAM(low),
                                LPARAM(high),
                            );
                        }
                        let handled = DefWindowProcW(hwnd, msg, wparam, lparam);
                        restore_translated_sys_chars(hwnd, &held);
                        return handled;
                    }

                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_KEYUP | WM_SYSKEYUP => {
                    if let Some(ctx) = ctx {
                        let time = ctx.message_clock.message_time();
                        // Track modifiers.
                        ctx.modifiers.set(super::events::message_modifiers());

                        use super::events::key_up_event;
                        let event = {
                            let mut held = ctx.held_dead_keys.borrow_mut();
                            key_up_event(wparam, lparam, &mut held, time)
                        };
                        let result = ctx.callbacks.dispatch_input(event);
                        if result.default_prevented
                            || GetWindowLongPtrW(hwnd, GWLP_USERDATA) != ctx_ptr as isize
                        {
                            return LRESULT(0);
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_CHAR => {
                    // Normally unreachable: the WM_KEYDOWN arm drains the
                    // paired WM_CHAR burst before it would be dispatched
                    // here. What remains is an out-of-band character with no
                    // owning keydown — Alt+numpad composition (queued by
                    // TranslateMessage of the Alt KEYUP) or a directly-sent
                    // message — dispatched as its own key-down instead of
                    // being discarded, so the text lane still receives it.
                    // Astral-plane characters arrive as two WM_CHARs; the
                    // high surrogate is parked in the context until its low
                    // half arrives.
                    if let Some(ctx) = ctx {
                        let (pending, text) = crate::shared::keys::assemble_stray_wm_char(
                            ctx.pending_high_surrogate.get(),
                            wparam.0 as u16,
                        );
                        ctx.pending_high_surrogate.set(pending);
                        if let Some(text) = text {
                            use super::events::stray_char_event;
                            let time = ctx.message_clock.message_time();
                            ctx.callbacks.dispatch_input(stray_char_event(text, time));
                        }
                    }
                    LRESULT(0)
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
                        ctx.cancel_pointers(hwnd);
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

                // WM_MOUSELEAVE (0x02A3), requested by WM_MOUSEMOVE.
                0x02A3 => {
                    if let Some(ctx) = ctx {
                        ctx.set_hovered(false);
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
                        ctx.pointer_registry.borrow_mut().set_click_settings(
                            super::events::click_settings(ctx.scale_factor.get()),
                        );
                        let appearance = super::appearance::system_appearance();
                        if ctx.appearance.replace(appearance) != appearance {
                            if ctx.chrome_follows_system.get() {
                                super::appearance::apply_chrome(hwnd, appearance);
                            }
                            ctx.callbacks.dispatch_appearance_changed();
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // Keyboard layout change
                WM_INPUTLANGCHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.dispatch_keyboard_layout_change();
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

                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }
}

/// While it lives, the window context does not sample and publish its state;
/// see [`WindowContext::hold_publishing`].
#[must_use = "publishing resumes when the guard drops"]
pub(super) struct PublishHold<'a>(&'a std::cell::Cell<u32>);

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
