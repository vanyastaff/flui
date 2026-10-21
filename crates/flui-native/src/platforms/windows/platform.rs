//! Windows platform implementation

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Weak},
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Point, Size};
use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{
            ERROR_CANCELLED, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_FAILED, WAIT_OBJECT_0,
            WAIT_TIMEOUT, WPARAM,
        },
        Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, HBRUSH, PAINTSTRUCT},
        System::{
            LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
            Threading::GetCurrentThreadId,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            WindowsAndMessaging::{
                CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GWLP_USERDATA, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW,
                GetWindowThreadProcessId, HICON, HTCLIENT, HWND_MESSAGE, IDC_ARROW, ISMEX_NOSEND,
                InSendMessageEx, IsWindowVisible, MINMAXINFO, MSG, MWMO_INPUTAVAILABLE,
                MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, PostMessageW,
                PostQuitMessage, QS_ALLINPUT, RegisterClassW, SC_KEYMENU, SW_SHOWNORMAL,
                SetForegroundWindow, SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_STYLE, WINDOWPOS, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_CREATE,
                WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_INPUTLANGCHANGE,
                WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
                WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE, WM_PAINT,
                WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SETTEXT,
                WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SIZE, WM_STYLECHANGED, WM_SYSCOMMAND,
                WM_SYSKEYDOWN, WM_SYSKEYUP, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WNDCLASSW,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    display::WindowsDisplays,
    modal_loop::MODAL_DEADLINE_TIMER,
    text_services::TextServices,
    util::{WINDOW_CLASS_NAME, get_x_lparam, get_y_lparam, hiword, load_cursor_style},
    window::{NativeMode, WindowsWindow},
};
use crate::{
    config::WindowConfiguration, error::PlatformError, executor::BackgroundExecutor,
    shared::PlatformHandlers, shared::WindowCallbacks, shared::hwnd_affinity::ContextLedger,
    shared::hwnd_affinity::UserDataRefusal, shared::window_state::WindowStateCell,
    traits::HostWindow, traits::OpenWindowError, traits::OwnerPlatform, traits::Platform,
    traits::PlatformExecutor, traits::PlatformReadyCallback, traits::owner::OwnerHooks,
};
use flui_platform::{
    Clipboard, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, SystemNotice, WindowAppearance,
    WindowEvent, WindowId, WindowOptions, data_transfer::DataTransferSource,
};

/// Ensures window class is registered exactly once (sound replacement for
/// `static mut bool`).
static REGISTER_WINDOW_CLASS: std::sync::Once = std::sync::Once::new();

#[path = "platform/wake_deadline.rs"]
mod wake_deadline;
use wake_deadline::{DeadlineArm, DeadlineDelivery, DeadlineQuery, WakeDeadlineHook};
pub(super) use wake_deadline::{FrameCount, ModalDelivery};

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

#[cfg(test)]
#[path = "platform/modal_frame_tests.rs"]
mod modal_frame_tests;

/// Identity of one native window's context, minted once per
/// [`WindowsWindow::new`] and held by both the context and every wrapper
/// clone. Unlike the context's address it is never reused, so a wrapper can
/// tell its own window from one the OS built behind a recycled handle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct WindowIdentity(std::num::NonZeroU64);

impl WindowIdentity {
    /// The process-wide identity source.
    pub(super) fn source() -> &'static std::sync::atomic::AtomicU64 {
        // A monotonic id source, not shared state: nothing reads it back.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        &NEXT
    }

    /// The admission boundary `WindowsWindow::new` runs: reserve an
    /// identity from `source`, then run `acquire` for the native window it
    /// will own. Exhaustion refuses before `acquire` runs, so a refusal
    /// leaves nothing to release.
    pub(super) fn admit<R>(
        source: &std::sync::atomic::AtomicU64,
        acquire: impl FnOnce() -> R,
    ) -> (Self, R) {
        let identity = Self::mint_from(source);
        (identity, acquire())
    }

    fn mint_from(next: &std::sync::atomic::AtomicU64) -> Self {
        use std::sync::atomic::Ordering;

        // Mint the final nonzero identity once, then leave the source at zero.
        // Refusal must not advance it: callers may catch a failed open and retry.
        let raw = next
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |raw| {
                (raw != 0).then(|| raw.checked_add(1).unwrap_or(0))
            })
            .expect("BUG: the window identity counter is exhausted");
        Self(std::num::NonZeroU64::new(raw).expect("BUG: an admitted window identity is nonzero"))
    }

    /// The identity as a posted message's two parameters, its low and high 32
    /// bits, whole on a 32-bit target too.
    pub(super) fn halves(self) -> (usize, isize) {
        let raw = self.0.get();
        ((raw & 0xFFFF_FFFF) as usize, (raw >> 32) as isize)
    }
}

/// One window as an object that outlives its borrow of the window names it:
/// the handle and the identity of the context created with it. The OS
/// recycles handles, so a handle alone can name a later window;
/// [`Self::with_context`] reaches only the context this window was created
/// with. Plain values, so an `Arc`-shared object can hold one on any thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct WindowRef {
    hwnd: isize,
    identity: WindowIdentity,
}

impl WindowRef {
    pub(super) fn new(hwnd: HWND, identity: WindowIdentity) -> Self {
        Self {
            hwnd: hwnd.0 as isize,
            identity,
        }
    }

    pub(super) fn hwnd(self) -> HWND {
        HWND(self.hwnd as *mut core::ffi::c_void)
    }

    /// [`with_window_context`] for this window only: `None` off its owner
    /// thread, after it closed, or when its handle now names another window.
    pub(super) fn with_context<R>(
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

#[cfg(test)]
impl WindowsPlatform {
    pub(crate) fn window_identity_exhaustion_permanently_refuses_retries() {
        use std::{
            cell::Cell,
            sync::atomic::{AtomicU64, Ordering},
        };

        // Exercise the actual allocator with local sources; never exhaust the
        // process-wide source used by other native window tests.
        for seed in [0, u64::MAX - 1] {
            let next = AtomicU64::new(seed);
            let acquisitions = Cell::new(0);
            // The admission `WindowsWindow::new` runs, with an acquisition
            // that records native creation instead of performing it.
            let admit =
                || WindowIdentity::admit(&next, || acquisitions.set(acquisitions.get() + 1)).0;
            if seed != 0 {
                let penultimate = admit();
                let final_identity = admit();
                assert_eq!(penultimate.0.get(), u64::MAX - 1);
                assert_eq!(final_identity.0.get(), u64::MAX);
                assert_ne!(penultimate, final_identity);
            }
            for attempt in 0..3 {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(admit)).is_err(),
                    "seed {seed}: exhausted allocation succeeded on retry {attempt}"
                );
            }
            assert_eq!(
                next.load(Ordering::Relaxed),
                0,
                "seed {seed}: refusal advanced the exhausted source"
            );
            assert_eq!(
                acquisitions.get(),
                if seed == 0 { 0 } else { 2 },
                "seed {seed}: exhausted admission acquired native resources"
            );
        }
    }
}

#[cfg(test)]
impl WindowsPlatform {
    #[expect(unsafe_code, reason = "COM apartment and message-only window queries")]
    pub(crate) fn platform_identity_exhaustion_acquires_nothing() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use windows::Win32::{
            System::Com::{APTTYPE, APTTYPEQUALIFIER, CoGetApartmentType},
            UI::WindowsAndMessaging::FindWindowExW,
        };

        // This thread's COM apartment, or the query's refusal when none.
        let apartment = || {
            let (mut kind, mut qualifier) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
            // SAFETY: both out-pointers are live, writable locals.
            unsafe { CoGetApartmentType(&raw mut kind, &raw mut qualifier) }
                .map(|()| (kind, qualifier))
                .map_err(|error| error.code())
        };
        // The platform message-only windows that exist in this process.
        let message_windows = || {
            let mut count = 0;
            let mut after = None;
            // SAFETY: enumerates message-only windows by a static class name;
            // every handle is only passed back as the next search start.
            while let Ok(hwnd) = unsafe {
                FindWindowExW(Some(HWND_MESSAGE), after, WINDOW_CLASS_NAME, PCWSTR::null())
            } {
                count += 1;
                after = Some(hwnd);
            }
            count
        };

        let (apartment_before, windows_before) = (apartment(), message_windows());
        let source = AtomicU64::new(0);
        for attempt in 0..3 {
            let refused = std::panic::catch_unwind(|| {
                Self::admit_with_config(WindowConfiguration::default(), &source)
            });
            assert!(
                refused.is_err(),
                "exhausted platform admission succeeded on retry {attempt}"
            );
        }
        assert_eq!(
            source.load(Ordering::Relaxed),
            0,
            "refusal advanced the exhausted source"
        );
        assert_eq!(
            apartment(),
            apartment_before,
            "refused admission initialized COM"
        );
        assert_eq!(
            message_windows(),
            windows_before,
            "refused admission created a message window"
        );
    }
}

/// The platform's tracked top-level windows, keyed by HWND. The owner
/// message-only windows are never in it.
pub(super) type WindowMap = Mutex<HashMap<isize, Arc<WindowsWindow>>>;

/// Context data stored per window for event dispatch.
///
/// Lives in the window's `GWLP_USERDATA` slot and is only ever reached on
/// the window's owner thread, through [`with_window_context`] or
/// `window_proc`. That is what makes it the home of the window's callbacks:
/// a registration that cannot reach this context from the calling thread is
/// refused, so a callback never lands where a foreign thread could run or
/// drop it.
pub(super) struct WindowContext {
    /// Window ID for event dispatch
    pub window_id: WindowId,
    /// Which native window this context belongs to; see [`WindowIdentity`].
    pub identity: WindowIdentity,
    /// Tracking and cache access do not keep wrappers or their state alive.
    pub windows: std::sync::Weak<WindowMap>,
    pub window_state: std::sync::Weak<Mutex<super::window::WindowRecord>>,
    /// The published window state, shared with the wrapper; refreshed by
    /// [`Self::publish_window_state`].
    pub snapshot: Arc<WindowStateCell>,
    /// The platform-level handlers, shared with the owner control context.
    /// `Rc<RefCell<..>>`: every holder lives on the owner thread, and no
    /// borrow is held while a handler runs.
    pub handlers: Rc<RefCell<PlatformHandlers>>,
    /// Asks the owner to consult the exit policy once this window's
    /// `WM_DESTROY` leaves the platform tracking no window.
    pub exit_policy: super::owner_control::ExitPolicyRequest,
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
    pub message_clock: super::events::MessageClock,
    pub pointer_registry: RefCell<crate::shared::pointer_assembly::ContactRegistry>,
    /// The precision touchpad's DirectManipulation viewport; `None` when it
    /// did not initialise (the window then has no precise scrolling or
    /// trackpad gestures) or after it failed.
    pub touchpad: super::direct_manipulation::TouchpadSlot,
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
    pub first_show: std::cell::Cell<super::window_frame::FirstShow>,
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
    pub visibility: super::window_visibility::Visibility,
    /// The window's subscription to cloak changes.
    pub cloak_watch: super::window_visibility::CloakWatch,
    /// Set between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`: a live resize,
    /// whose `WM_SIZE` renders at once.
    pub in_size_move: std::cell::Cell<bool>,
    /// The client size, in device pixels, and the scale the last resize
    /// callback delivered.
    pub last_resize: std::cell::Cell<Option<(Size<i32>, f64)>>,
    /// Delivers wake deadlines while a modal loop runs for the window.
    pub modal_pacing: super::modal_loop::ModalPacing,
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
    pub drop_target: RefCell<Option<windows_core::ComObject<super::drag::WindowDropTarget>>>,
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
    pub(super) fn request_frame(&self) {
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

    /// The window moved on the screen or changed DPI: the focused field's
    /// input method follows it.
    pub(super) fn text_layout_moved(&self) {
        let services = self.text_services.borrow().clone();
        if let Some(services) = services {
            services.window_moved();
        }
    }

    /// Dispatch a window event to the platform-level handler.
    #[inline]
    pub(super) fn dispatch_event(&self, event: WindowEvent) {
        self.with_leased_handler(
            |handlers| &mut handlers.window_event,
            |handler| handler(event),
        );
    }

    /// Dispatch a keyboard-layout change to the platform-level hook.
    pub(super) fn dispatch_keyboard_layout_change(&self) {
        self.with_leased_handler(
            |handlers| &mut handlers.keyboard_layout_changed,
            |handler| handler(),
        );
    }

    /// Runs one platform-level handler leased out of its slot: taken out, so
    /// no `RefCell` borrow is held while it runs, then put back only if the
    /// slot is still empty. A handler registered while this one was out —
    /// including by this handler itself — is the newer registration and
    /// wins; the leased one is then dropped. The same lease rule as
    /// `OwnerSignal::drive`.
    ///
    /// The restore touches `self` AFTER the handler returns, and the handler
    /// may have closed this very window — that is sound only because every
    /// caller holds a [`ContextGuard`] over `self`, which defers the
    /// context's free past the borrow even when the handler's close runs a
    /// nested `WM_DESTROY` (which merely retires it).
    fn with_leased_handler<H>(
        &self,
        slot: impl Fn(&mut PlatformHandlers) -> &mut Option<H>,
        call: impl FnOnce(&mut H),
    ) {
        let handler = slot(&mut self.handlers.borrow_mut()).take();
        let Some(mut handler) = handler else {
            return;
        };
        call(&mut handler);
        let superseded = {
            let mut handlers = self.handlers.borrow_mut();
            let current = slot(&mut handlers);
            if current.is_none() {
                *current = Some(handler);
                None
            } else {
                Some(handler)
            }
        };
        drop(superseded);
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
pub(super) fn with_window_context<R>(
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
pub(super) fn with_window_context_checked<R>(
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

/// Windows platform state
pub struct WindowsPlatform {
    owner_control: super::owner_control::OwnerControl,
    /// OLE drag and drop: the platform's one data-transfer source.
    pub(super) drag: Arc<super::drag::OleTransfer>,
    /// Message-only window for platform messages
    message_window: HWND,

    /// All created windows (keyed by HWND)
    windows: Arc<WindowMap>,

    // The platform-level handlers (callbacks from platform to framework)
    // are not a field: they live in the owner control context, reachable
    // only on the owner thread through `owner_control.shares`.
    /// Background executor for async tasks
    background_executor: Arc<BackgroundExecutor>,

    /// Window configuration (shared across all windows)
    config: WindowConfiguration,

    /// The monitors this platform last sampled and the registry of their ids.
    displays: Arc<WindowsDisplays>,

    /// Records the owner thread so the thread-affine Win32 operations below
    /// can `debug_assert` their caller (ADR-0039). Bound at construction —
    /// the message-only window's queue belongs to the constructing thread —
    /// and re-asserted by `run`. Pre-run window creation on that same
    /// thread (the Win32 examples) stays legal.
    affinity: flui_foundation::OwnerAffinity,

    /// The owner thread's COM apartment. Declared last, so it is released
    /// after everything above; each window's text services hold their own
    /// clone, so COM outlives the platform while any of them is alive.
    apartment: super::com_apartment::ApartmentHold,
}

// SAFETY, per field: `windows` is an `Arc<Mutex<..>>`, `owner_control` holds
// only an `Arc`-shared signal and a locked address (its owner-thread state,
// callbacks included, sits behind the owner window's `GWLP_USERDATA` and is
// reached only through the owner-thread gate `OwnerControl::shares`), the
// executors are `Arc`-shared and internally synchronized, `displays` is a
// registry and a snapshot each behind a lock, and `config` is plain data. The
// only non-`Sync` member is `message_window: HWND`, a bare address that is
// never dereferenced here.
//
// NOT claimed — an earlier version of this comment claimed both, wrongly: that
// an HWND is "thread-safe by design" (it is thread-AFFINE; its message queue
// belongs to the creating thread, and `DestroyWindow` must run there), and that
// the struct is itself just a handle. Sending the struct is sound because the
// address alone aliases nothing; any Win32 call made through it still owes the
// thread-affinity obligation. The soundness-relevant slice of that obligation
// — dereferencing per-window `GWLP_USERDATA` contexts, and `DestroyWindow` —
// is discharged by the `with_window_context`/`teardown_route` gates above
// (see `WindowsWindow`'s `Send`/`Sync` docs); the remaining affine calls here
// (`quit`'s `PostQuitMessage`, `open_window`'s queue binding) are logic-level
// rather than memory-safety and stay guarded by `affinity.debug_assert_owner`
// — see ADR-0039 (event-loop affinity). `apartment` is `Send + Sync` by its
// own owner-thread gate, and `drag` keeps its COM objects in an owner slot.
unsafe impl Send for WindowsPlatform {}
// SAFETY: as for `Send` — `&WindowsPlatform` grants no more than shared access
// to already-synchronized members plus a never-dereferenced address.
unsafe impl Sync for WindowsPlatform {}

impl std::fmt::Debug for WindowsPlatform {
    // Hand-written: the remaining fields are raw platform handles and callback
    // payloads with no useful Debug form.
    //
    // `try_lock`, never `lock`: `parking_lot::Mutex` is not reentrant and
    // BLOCKS rather than panicking, so formatting this value while the same
    // thread already holds `windows` would deadlock silently — and a Debug
    // impl gets called from assertion messages and `tracing` fields, which is
    // exactly where a lock is likely to be held. Same pattern
    // `parking_lot::Mutex<T>: Debug` itself uses.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("WindowsPlatform");
        match self.windows.try_lock() {
            Some(windows) => out.field("windows", &windows.len()),
            None => out.field("windows", &format_args!("<locked>")),
        };
        out.finish_non_exhaustive()
    }
}

impl WindowsPlatform {
    /// Create a new Windows platform instance with default configuration
    pub fn new() -> Result<Self, PlatformError> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new Windows platform instance with custom configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Window configuration (hotkeys, debouncing, fullscreen
    ///   behavior)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_native::{WindowsPlatform, WindowConfiguration, FullscreenMonitor};
    ///
    /// // Disable F11 hotkey
    /// let config = WindowConfiguration::no_hotkey();
    /// let platform = WindowsPlatform::with_config(config)?;
    ///
    /// // Use primary monitor for fullscreen
    /// let config = WindowConfiguration {
    ///     fullscreen_monitor: FullscreenMonitor::Primary,
    ///     ..Default::default()
    /// };
    /// let platform = WindowsPlatform::with_config(config)?;
    /// ```
    pub fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
        Self::admit_with_config(config, WindowIdentity::source())
    }

    /// [`Self::with_config`] with the identity drawn from `source`. The
    /// owner control's identity is reserved first: exhaustion refuses before
    /// COM is initialized or any HWND exists, so a refusal (and every caught
    /// retry) leaves nothing to release.
    fn admit_with_config(
        config: WindowConfiguration,
        source: &std::sync::atomic::AtomicU64,
    ) -> Result<Self, PlatformError> {
        let owner_identity = WindowIdentity::mint_from(source);

        // Enter this thread's STA for drag-and-drop, clipboard and TSF. The
        // apartment is a value: an early return below releases it, and once
        // built it lives until the platform and every window's text services
        // are gone (see `com_apartment`). This thread is also the one the
        // platform's `affinity` binds to below.
        let apartment =
            super::com_apartment::ComApartment::enter().map_err(|hr| PlatformError::Init {
                message: format!("Failed to initialize COM: {hr:?}"),
            })?;

        // SAFETY: `SetProcessDpiAwarenessContext` takes a constant by value,
        // no pointer arguments; failure (already set, or an OS predating
        // per-monitor-v2 awareness) is expected and non-fatal, hence the
        // discarded result.
        //
        // Set DPI awareness to per-monitor v2 (best quality)
        // Ignore errors - this can fail if already set or on older Windows
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            // This native process setting is idempotent for the same value.
            // A host that already fixed it to false keeps the legacy mouse route.
            if let Err(error) = windows::Win32::UI::Input::Pointer::EnableMouseInPointer(true) {
                tracing::debug!(%error, "mouse pointer messages unavailable; retaining mouse messages");
            }
        }

        // SAFETY: `register_window_class`'s own `# Safety` contract (below)
        // is discharged by `REGISTER_WINDOW_CLASS: Once` making the call
        // race-free and idempotent regardless of caller thread.
        //
        // Register window class
        unsafe {
            Self::register_window_class()?;
        }

        // Build owner resources first so any later native creation failure
        // retires its preference subscriptions and receiver through the guard.
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let drag = super::drag::OleTransfer::new();
        let displays = Arc::new(WindowsDisplays::new());
        let notices = Arc::new(crate::shared::system_notice::SystemNotices::default());
        notices.mark_stale_on_wake(&displays);
        let owner_control = super::owner_control::OwnerControl::new(
            owner_identity,
            Arc::downgrade(&windows),
            Arc::clone(&drag),
            Arc::clone(&displays),
            notices,
        )?;

        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments. `CreateWindowExW` here creates the
        // message-only window with `HWND_MESSAGE` as parent and constant
        // Win32 arguments (no caller-supplied buffers); its class was just
        // registered above on this same thread, satisfying the ordering
        // Win32 requires (register-before-create).
        //
        // Create message-only window for platform messages
        let message_window = unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| PlatformError::Init {
                message: format!("Failed to get module handle: {e:?}"),
            })?;

            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WINDOW_CLASS_NAME,
                w!("Flui Platform Message Window"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE), // Message-only window
                None,
                Some(hinstance.into()),
                None,
            )
            .map_err(|e| PlatformError::Init {
                message: format!("Failed to create message window: {e:?}"),
            })?
        };

        // Create executors
        let background_executor = Arc::new(BackgroundExecutor::new());

        tracing::info!("Windows platform initialized with Tokio executors");

        let platform = Self {
            owner_control,
            drag,
            message_window,
            windows,
            background_executor,
            config,
            displays,
            affinity: flui_foundation::OwnerAffinity::new(),
            apartment: super::com_apartment::ApartmentHold::new(apartment),
        };
        // The message-only window above was just created on THIS thread, so
        // its message queue already belongs here — the owner is decided at
        // construction, not at `run`. A later `run` on another thread trips
        // the foreign re-bind assertion instead of silently accepting a
        // thread that cannot service the HWND.
        platform.affinity.bind_current();
        Ok(platform)
    }

    /// Register the window class for all FLUI windows (idempotent via `Once`).
    ///
    /// # Safety
    ///
    /// `Self::window_proc` is registered as the class's `WNDPROC` — its
    /// `extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT` signature
    /// must exactly match what Win32 calls through that slot, or every
    /// dispatch to a window of this class is an ABI-mismatched call. The
    /// caller must not register a different, incompatible callback under
    /// the same `WINDOW_CLASS_NAME` afterward — `REGISTER_WINDOW_CLASS: Once`
    /// enforces that this body runs at most once per process, so subsequent
    /// calls are no-ops rather than a re-registration race.
    unsafe fn register_window_class() -> Result<(), PlatformError> {
        // SAFETY: per the `# Safety` contract above, `Self::window_proc`'s
        // signature matches `WNDPROC`. `GetModuleHandleW(None)` takes no
        // pointer arguments. `wc.lpszClassName`/`wc.hCursor` reference
        // process-lifetime statics (`WINDOW_CLASS_NAME`, the loaded cursor
        // resource); `&raw const wc` gives `RegisterClassW` a valid pointer
        // to the fully-initialized, stack-local `WNDCLASSW`. `call_once`
        // guarantees this body executes at most once, so there is no
        // concurrent registration to race.
        unsafe {
            let mut result: Result<(), PlatformError> = Ok(());

            REGISTER_WINDOW_CLASS.call_once(|| {
                let reg = (|| -> Result<(), PlatformError> {
                    let hinstance = GetModuleHandleW(None).map_err(|e| PlatformError::Init {
                        message: format!("Failed to get module handle: {e}"),
                    })?;

                    let wc = WNDCLASSW {
                        // No `CS_HREDRAW`/`CS_VREDRAW`: a flip-model surface is
                        // presented whole, and `WM_SIZE` asks for the frame itself.
                        style: CS_OWNDC,
                        lpfnWndProc: Some(Self::window_proc),
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: hinstance.into(),
                        hIcon: HICON::default(),
                        hCursor: load_cursor_style(IDC_ARROW).map_err(|e| PlatformError::Init {
                            message: format!("Failed to load the default cursor: {e}"),
                        })?,
                        hbrBackground: HBRUSH(std::ptr::null_mut()),
                        lpszMenuName: PCWSTR::null(),
                        lpszClassName: WINDOW_CLASS_NAME,
                    };

                    let atom = RegisterClassW(&raw const wc);
                    if atom == 0 {
                        return Err(PlatformError::Init {
                            message: format!(
                                "Failed to register the window class: {}",
                                windows::core::Error::from_thread()
                            ),
                        });
                    }

                    tracing::info!("Registered Windows window class");
                    Ok(())
                })();

                if let Err(e) = reg {
                    result = Err(e);
                }
            });

            result
        }
    }

    /// Main window procedure for all FLUI windows: a panic boundary around
    /// [`Self::window_proc_body`], which holds the actual message handling.
    ///
    /// The body dispatches framework-supplied callbacks (`ctx.callbacks.*`,
    /// `ctx.dispatch_event`) that can panic. A panic must not unwind across
    /// this `extern "system"` frame: unwinding across a non-unwind ABI
    /// boundary is a guaranteed process abort since Rust 1.81, and was
    /// undefined behavior before that (Rust Reference, *Panic* chapter,
    /// "unwinding across FFI boundaries") — and that language-imposed abort
    /// reports nothing through this framework's diagnostics. So the panic
    /// is caught here, its payload logged via `tracing`, and the process
    /// terminated deliberately — the same outcome, made deterministic and
    /// observable on every toolchain. Deliberately NOT
    /// swallowed-and-continued (`DefWindowProcW` as a fallback answer): a
    /// panic mid-`WM_DESTROY` or mid-dispatch leaves framework state torn,
    /// and pumping further messages over torn state converts one crash into
    /// arbitrary later misbehavior — per `docs/PANIC-POLICY.md`, a panic is
    /// a bug report, never control flow.
    ///
    /// # Safety
    ///
    /// Called by Win32 as a `WNDPROC` for windows of `WINDOW_CLASS_NAME`,
    /// registered as this exact function in `register_window_class`; that
    /// provenance is exactly what [`Self::window_proc_body`]'s own `# Safety`
    /// contract requires, so forwarding the arguments verbatim discharges it.
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // `AssertUnwindSafe` is sound here: on `Err` the process aborts
        // before any code can observe whatever state the unwind tore.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: arguments forwarded verbatim from Win32's dispatch of
            // this registered `WNDPROC` — see `# Safety` above.
            unsafe { Self::window_proc_body(hwnd, msg, wparam, lparam) }
        }));
        match outcome {
            Ok(result) => result,
            Err(payload) => {
                tracing::error!(
                    msg,
                    hwnd = ?hwnd,
                    panic = crate::shared::panic_boundary::panic_payload_message(&*payload),
                    "panic in a window-procedure callback; aborting — it must not \
                     unwind across the extern \"system\" boundary, and the framework \
                     state behind this message may be torn"
                );
                std::process::abort();
            }
        }
    }

    /// The message handling behind [`Self::window_proc`]. Every dispatch of
    /// user callbacks stays inside that caller's panic boundary.
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
                        super::drag::revoke_target(hwnd, &ctx.drop_target);

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
                        ctx.text_layout_moved();
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
                    use super::util::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

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
                        ctx.text_layout_moved();
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

    /// Run the Windows message loop (internal implementation)
    fn run_message_loop(&self) -> Result<(), PlatformError> {
        tracing::info!("Starting Windows message loop");

        let mut armed: Option<DeadlineArm> = None;
        let mut delivered: Option<DeadlineDelivery> = None;
        // Shared with every window this platform opens; without an owner
        // there are no windows, so the count never moves.
        let shares = self.owner_control.shares("run message loop").ok();
        let frames = shares.as_ref().map(|shares| Rc::clone(&shares.frames));
        let preferences = shares.map(|shares| shares.preferences);
        let fallback = Arc::clone(&self.owner_control.wake);
        let wait_handles = [fallback.handle()];
        let frame_count = || frames.as_ref().map_or(0, |frames| frames.get());

        // SAFETY: `msg` is a stack-local, default-initialized `MSG` that
        // outlives every call here. `&raw mut msg` is the only pointer
        // `PeekMessageW` writes through, and `TranslateMessage`/
        // `DispatchMessageW` read `&raw const msg` only after the peek
        // reported a filled message. No pointer into `msg` is held across a
        // call, so the re-entrancy below cannot alias it.
        // `wait_handles` and its strong OwnedHandle owner outlive every native
        // wait, including reentrant shutdown; no handle closes while waiting.
        // The loop runs on the thread that created this
        // platform's windows, so every peek, wait and dispatch addresses that
        // thread's queue, and `DispatchMessageW` (like `PeekMessageW`, which
        // delivers sent messages) re-enters `window_proc`
        // on it. The deadline hook is safe code, called with no borrow of
        // `msg` and no platform lock held, so it may re-enter the platform
        // (close windows, replace itself, request quit) without violating
        // either invariant.
        unsafe {
            let mut msg = MSG::default();

            loop {
                if let Some(source) = &preferences {
                    source.retry_if_due();
                }
                // An admitted due wake is delivery debt. Actuate it before
                // querying user code, which may abandon a late deadline.
                if armed
                    .as_ref()
                    .is_some_and(|arm| arm.deadline <= web_time::Instant::now())
                {
                    let arm = armed.take().expect("BUG: a due deadline is armed");
                    if self
                        .deadline_hook()
                        .is_some_and(|hook| Weak::ptr_eq(&arm.hook, &Arc::downgrade(&hook)))
                    {
                        let before = frame_count();
                        Self::redraw_deadline_windows(&self.windows);
                        delivered = Some(DeadlineDelivery {
                            arm,
                            frames: before,
                        });
                    }
                }

                // Drain native input/paint/quit before another idle query.
                // Peek also services sent messages, so never follow a timed
                // wake with an unconditional GetMessage that might re-park.
                if PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break;
                    }
                    let _ = TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                    continue;
                }

                let hook = self.deadline_hook();
                let next = hook.map(|hook| {
                    let custody = DeadlineQuery(Some(hook));
                    let hook = custody
                        .0
                        .as_ref()
                        .expect("BUG: a queried deadline hook is owned");
                    let deadline = hook();
                    let identity = Arc::downgrade(hook);
                    // A replaced hook's final captures may themselves
                    // replace registration or quit. Retire before validation.
                    drop(custody);
                    (deadline, identity)
                });
                armed = match next {
                    Some((deadline, identity))
                        if self.deadline_hook().is_some_and(|hook| {
                            Weak::ptr_eq(&identity, &Arc::downgrade(&hook))
                        }) =>
                    {
                        match deadline {
                            Some(deadline)
                                if delivered.as_ref().is_none_or(|last| {
                                    last.admits(deadline, &identity, frame_count())
                                }) =>
                            {
                                Some(DeadlineArm {
                                    deadline,
                                    hook: identity,
                                })
                            }
                            Some(_) => None,
                            None => {
                                delivered = None;
                                None
                            }
                        }
                    }
                    _ => {
                        delivered = None;
                        None
                    }
                };

                let next_deadline = armed
                    .as_ref()
                    .map(|arm| arm.deadline)
                    .into_iter()
                    .chain(
                        preferences
                            .as_ref()
                            .and_then(|source| source.retry_deadline()),
                    )
                    .min();
                let timeout = next_deadline.map_or(u32::MAX, |deadline| {
                    let remaining = deadline.saturating_duration_since(web_time::Instant::now());
                    // Round upwards so a submillisecond deadline cannot spin;
                    // reserve Win32's INFINITE value for an actual idle wait.
                    let milliseconds = remaining.as_nanos().saturating_add(999_999) / 1_000_000;
                    u32::try_from(milliseconds.min(u128::from(u32::MAX - 1)))
                        .expect("BUG: a bounded native timeout fits u32")
                });
                let outcome = MsgWaitForMultipleObjectsEx(
                    Some(&wait_handles),
                    timeout,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if outcome == WAIT_FAILED {
                    return Err(PlatformError::EventLoop {
                        message: windows::core::Error::from_thread().to_string(),
                    });
                }
                if outcome == WAIT_OBJECT_0 {
                    self.owner_control.drive_fallback();
                } else if outcome != WAIT_TIMEOUT && outcome.0 != WAIT_OBJECT_0.0 + 1 {
                    return Err(PlatformError::EventLoop {
                        message: format!("unexpected Win32 message wait result: {}", outcome.0),
                    });
                }
            }

            tracing::info!("Message loop exited with code: {}", msg.wParam.0);
        }
        Ok(())
    }

    fn deadline_hook(&self) -> Option<Arc<WakeDeadlineHook>> {
        if !self.owner_control.signal.accepting() {
            return None;
        }
        let shares = self.owner_control.shares("query wake deadline").ok()?;
        shares.handlers.borrow().wake_deadline.clone()
    }

    /// Ask every window of `windows` for the frame a delivered wake deadline
    /// is due: the main loop's delivery, and the modal drag loop's.
    pub(super) fn redraw_deadline_windows(windows: &WindowMap) {
        // Traverse a bounded identity snapshot without a per-frame Vec.
        // A minimized window's frame request runs user code, which may open
        // or close windows; each owning Arc is cloned under the registry
        // lock and used only after releasing it.
        let limit = windows
            .lock()
            .values()
            .map(|window| window.identity.0.get())
            .max()
            .unwrap_or(0);
        let mut after = 0;
        loop {
            let next = windows
                .lock()
                .values()
                .filter(|window| {
                    let identity = window.identity.0.get();
                    identity > after && identity <= limit
                })
                .min_by_key(|window| window.identity.0.get())
                .cloned();
            let Some(window) = next else {
                break;
            };
            after = window.identity.0.get();
            let _ = with_window_context_checked(window.hwnd(), "deadline redraw", |context| {
                if context.identity != window.identity {
                    return;
                }
                // SAFETY: a query of the live HWND this context belongs to,
                // on its creating thread.
                let shown = unsafe { IsWindowVisible(window.hwnd()) }.as_bool();
                if context.mode.get().is_minimized() || !shown {
                    // WM_PAINT skips a minimized window's frame request,
                    // Windows rarely paints one at all, and never paints a
                    // hidden one, so an invalidation would strand the
                    // delivered deadline until unrelated input. Request the
                    // frame directly instead: its frame services timers and
                    // gestures for a window nothing can see.
                    context.request_frame();
                } else {
                    flui_platform::PlatformWindow::request_redraw(window.as_ref());
                }
            });
        }
    }

    /// What the main loop would deliver, while `DoDragDrop`'s modal loop
    /// stands in for it and dispatches neither deadlines nor paints: a due
    /// wake deadline is delivered as the main loop delivers it, once per
    /// frame generation, and every window with an invalid region is painted
    /// (`UpdateWindow` sends its `WM_PAINT` directly), so every runtime on
    /// the thread keeps getting frames. Returns how many milliseconds to wait
    /// before the next request. The hook is user code, run with no borrow
    /// held and in the main loop's custody.
    pub(super) fn deliver_modal_frames(
        handlers: &RefCell<PlatformHandlers>,
        windows: &Weak<WindowMap>,
        frames: &FrameCount,
        delivered: &ModalDelivery,
    ) -> u32 {
        let hook = handlers.borrow().wake_deadline.clone();
        let queried = hook.map(|hook| {
            let identity = Arc::downgrade(&hook);
            let custody = DeadlineQuery(Some(hook));
            let deadline = custody.0.as_ref().and_then(|hook| hook());
            // A replaced hook's final captures may replace the registration
            // again: retired before the answer is checked against it.
            drop(custody);
            (deadline, identity)
        });
        // The answer of a hook that is no longer installed is not its
        // replacement's: as in the main loop, the delivery record is keyed by
        // the hook as well as the instant.
        let current = queried.and_then(|(deadline, identity)| {
            let installed = handlers
                .borrow()
                .wake_deadline
                .as_ref()
                .is_some_and(|hook| Weak::ptr_eq(&identity, &Arc::downgrade(hook)));
            installed.then_some((deadline?, identity))
        });
        let now = web_time::Instant::now();
        let deadline = current.as_ref().map(|(deadline, _)| *deadline);
        match current {
            Some((deadline, identity))
                if deadline <= now && delivered.admits(deadline, &identity, frames.get()) =>
            {
                if let Some(windows) = windows.upgrade() {
                    delivered.record(deadline, identity, frames.get());
                    Self::redraw_deadline_windows(&windows);
                }
            }
            Some(_) => {}
            None => delivered.clear(),
        }
        // A snapshot: a paint runs user code, which may open or close windows.
        let painted: Vec<HWND> = windows
            .upgrade()
            .map(|windows| {
                windows
                    .lock()
                    .values()
                    .map(|window| window.hwnd())
                    .collect()
            })
            .unwrap_or_default();
        for hwnd in painted {
            // SAFETY: a window this thread created; a handle closed by an
            // earlier paint fails the call harmlessly. Nothing is borrowed.
            let _ = unsafe { windows::Win32::Graphics::Gdi::UpdateWindow(hwnd) };
        }
        crate::shared::ole_drag_drop::modal_frame_wait_ms(deadline, now)
    }

    /// Installs a platform-level handler into the owner-thread handler set.
    ///
    /// Off the owner thread, or once the platform has shut down, the
    /// registration is refused and `callback` is dropped here, on the
    /// calling thread. A replaced handler is dropped after the `RefCell`
    /// borrow ends, since its destructor may re-enter the platform.
    fn register_handler<H>(
        &self,
        op: &'static str,
        callback: H,
        slot: impl FnOnce(&mut PlatformHandlers) -> &mut Option<H>,
    ) {
        match self.owner_control.shares(op) {
            Ok(shares) => {
                let previous = slot(&mut shares.handlers.borrow_mut()).replace(callback);
                // The replaced callback is retired in a boundary of its own: a
                // capture that panics in `Drop` must not unwind into the host.
                crate::shared::panic_boundary::contain_owner_callback(|| drop(previous));
            }
            Err(UserDataRefusal::ForeignThread) => {
                tracing::error!(
                    op,
                    "platform callback registration refused: platform callbacks are registered \
                     on the owner thread; the callback was dropped on the calling thread"
                );
                drop(callback);
            }
            Err(reason) => {
                tracing::debug!(
                    op,
                    ?reason,
                    "platform callback registration refused: the platform has shut down"
                );
                drop(callback);
            }
        }
    }
}

impl Platform for WindowsPlatform {
    fn window_capabilities(&self) -> flui_platform::WindowCapabilities {
        super::window::window_capabilities()
    }

    // ==================== Core System ====================

    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    // ==================== Lifecycle ====================

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        tracing::info!("Running Windows platform");

        // Idempotent from the constructing thread; a `run` migrated to a
        // different thread is a bug the re-bind assertion surfaces (the
        // message-only window's queue is bound to the constructing thread).
        self.affinity.bind_current();

        let platform = Arc::new(*self);
        struct RunGuard(Arc<WindowsPlatform>);
        impl Drop for RunGuard {
            fn drop(&mut self) {
                // Take the quit callback while the owner context still
                // exists, close (which frees that context on this thread),
                // and only then run it: the callback still observes a
                // closed platform.
                let callback = match self.0.owner_control.shares("run teardown") {
                    Ok(shares) => shares.handlers.borrow_mut().quit.take(),
                    Err(_) => None,
                };
                self.0.owner_control.close();
                if let Some(callback) = callback {
                    crate::shared::panic_boundary::invoke_and_drop_owner_callback(callback);
                }
            }
        }
        let _guard = RunGuard(Arc::clone(&platform));
        let erased: Arc<dyn Platform> = platform.clone();
        let hooks: Arc<dyn OwnerHooks> = Arc::new(super::owner_control::WindowsOwnerHooks::new(
            Arc::clone(&erased),
            &platform.owner_control,
        ));
        on_ready(OwnerPlatform::new(erased, hooks)).map_err(PlatformError::bootstrap)?;
        platform
            .owner_control
            .signal
            .start()
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            })?;
        platform.run_message_loop()
    }

    fn quit(&self) {
        self.owner_control.close_signal();
        // PostQuitMessage posts to the CALLING thread's message queue — off
        // the owner thread it silently quits nothing (ADR-0039).
        self.affinity.debug_assert_owner("WindowsPlatform::quit");
        tracing::info!("Quitting Windows platform");
        // SAFETY: `PostQuitMessage` takes a plain `i32` exit code, no
        // pointer arguments; the `debug_assert_owner` above documents (in
        // debug builds) that this runs on the thread whose queue it posts
        // to — posting from elsewhere is a logic bug (silently posts to the
        // wrong queue), not memory-unsafety.
        unsafe {
            PostQuitMessage(0);
        }
    }

    fn set_wake_deadline_hook(&self, hook: Box<WakeDeadlineHook>) {
        if !self.owner_control.signal.accepting() {
            drop(hook);
            return;
        }
        let hook: Arc<WakeDeadlineHook> = Arc::from(hook);
        self.register_handler("set_wake_deadline_hook", hook, |handlers| {
            &mut handlers.wake_deadline
        });
        // Re-query after replacement even when the old query re-entered this
        // setter. The existing owner signal coalesces these notifications.
        if self.owner_control.signal.accepting()
            && let Err(error) = self.owner_control.signal.wake()
        {
            tracing::warn!(%error, "failed to wake Win32 after deadline hook registration");
        }
    }

    // ==================== Window Management ====================

    fn active_window(&self) -> Option<WindowId> {
        self.affinity
            .debug_assert_owner("WindowsPlatform::active_window");
        // SAFETY: `GetForegroundWindow` takes no arguments; `hwnd` is just
        // an opaque value compared/wrapped, never dereferenced.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                None
            } else {
                Some(WindowId(hwnd.0 as u64))
            }
        }
    }

    // ==================== Display Management ====================

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.displays.snapshot()
    }

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        if !self.owner_control.signal.accepting() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        // An HWND's message queue belongs to the creating thread, and the
        // window context shares the owner-only platform handlers, so a
        // window is opened on the owner thread or not at all (ADR-0039).
        let shares = self.owner_control.shares("open_window").map_err(|reason| {
            OpenWindowError::Backend {
                message: format!(
                    "Win32 windows are opened on the platform's owner thread ({reason:?})"
                ),
            }
        })?;
        tracing::info!("Opening window: {:?}", options.title);

        let window = WindowsWindow::new(
            options,
            self.windows.clone(),
            shares,
            self.config.clone(),
            self.apartment.share(),
            Arc::clone(&self.drag),
        )?;
        let hwnd_value = window.hwnd().0 as isize;
        let this = WindowRef::new(window.hwnd(), window.identity);
        let wake = crate::shared::owner_signal::OwnerSignal::input_wake(
            &self.owner_control.signal,
            move || {
                let _ = this.with_context("resume_input_delivery", |context| {
                    context.callbacks.resume_input_delivery();
                });
            },
        );
        let _ =
            with_window_context_checked(window.hwnd(), "input_capabilities", |context| {
                context.callbacks.set_input_capabilities(
                    crate::shared::input_availability::windows(context.has_touchpad()),
                );
                context.callbacks.set_input_wake(wake);
            });

        // Store window
        let _prev = self.windows.lock().insert(hwnd_value, window.clone());

        Ok(window)
    }

    // ==================== Input & Clipboard ====================

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        Arc::new(super::WindowsClipboard::new())
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        self.drag.clone()
    }

    fn name(&self) -> &'static str {
        "Windows"
    }

    fn preferences(&self) -> Result<flui_platform::SystemPreferences, PlatformError> {
        self.owner_control
            .shares("read system preferences")
            .map_err(|reason| PlatformError::Preferences {
                message: format!("platform owner is unavailable: {reason:?}"),
            })?
            .preferences
            .sample()
    }

    // ==================== Callbacks ====================

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.register_handler("on_quit", callback, |handlers| &mut handlers.quit);
    }

    fn on_session_end(&self, callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>) {
        self.register_handler("on_session_end", callback, |handlers| {
            &mut handlers.session_end
        });
    }

    fn on_host_notice(&self, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        match self.owner_control.shares("on_host_notice") {
            Ok(shares) => shares.notices.set_hook(callback),
            Err(reason) => {
                tracing::debug!(?reason, "host notice registration refused");
                drop(callback);
            }
        }
    }

    fn release_session_end(&self) {
        if let Ok(shares) = self.owner_control.shares("release_session_end") {
            shares.host.release_block();
        }
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        self.register_handler("set_exit_policy_hook", hook, |handlers| {
            &mut handlers.exit_policy
        });
    }

    fn request_exit_policy_reevaluation(&self) {
        self.owner_control.exit_policy_request().post();
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.register_handler("on_window_event", callback, |handlers| {
            &mut handlers.window_event
        });
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        self.register_handler("on_keyboard_layout_change", callback, |handlers| {
            &mut handlers.keyboard_layout_changed
        });
    }

    // ==================== App Activation (US3 T038) ====================

    fn activate(&self, _ignoring_other_apps: bool) {
        // SAFETY: `GetForegroundWindow`/`SetForegroundWindow` take no
        // pointer arguments — `hwnd` is an opaque value, never dereferenced.
        unsafe {
            // Bring the foreground window to front
            let hwnd = GetForegroundWindow();
            if !hwnd.is_invalid()
                && let Err(error) = SetForegroundWindow(hwnd).ok()
            {
                tracing::warn!(
                    ?hwnd,
                    ?error,
                    "SetForegroundWindow failed in Platform::activate"
                );
            }
        }
    }

    // ==================== File Operations (US3 T041) ====================

    fn open_url(&self, url: &str) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let wide_url: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide_url` is explicitly NUL-terminated
        // (`.chain(std::iter::once(0))`), matching what `PCWSTR` requires,
        // and stays alive for the duration of this call (it is not dropped
        // or reallocated between `.as_ptr()` and the call).
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide_url.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn reveal_path(&self, path: &std::path::Path) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        // Use "explorer /select,<path>" to reveal in Explorer
        let path_str = path.to_string_lossy();
        let arg = format!("/select,{path_str}");
        let wide_arg: Vec<u16> = arg.encode_utf16().chain(std::iter::once(0)).collect();
        let explorer: Vec<u16> = "explorer\0".encode_utf16().collect();
        // SAFETY: see `open_url` above — both `wide_arg` and `explorer` are
        // explicitly NUL-terminated and kept alive across the call.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(explorer.as_ptr()),
                PCWSTR(wide_arg.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn open_path(&self, path: &std::path::Path) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let wide_path: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: see `open_url` above — `wide_path` is explicitly
        // NUL-terminated and kept alive across the call.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide_path.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    // ==================== File Dialogs (US3 T042-T043) ====================

    fn prompt_for_paths(
        &self,
        options: crate::traits::PathPromptOptions,
    ) -> crate::task::Task<Result<Option<Vec<std::path::PathBuf>>, PlatformError>> {
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            // COM file dialogs must run on an STA thread. The closure keeps
            // the raw `windows::core::Result` so the COM `?`s inside stay
            // untouched; the typed mapping happens once, at the join below.
            let result = std::thread::spawn(
                move || -> windows::core::Result<Option<Vec<std::path::PathBuf>>> {
                    // SAFETY: this whole body runs on the freshly `std::thread::spawn`ed
                    // thread, which owns no other window state — `CoInitializeEx`
                    // makes it an STA for the `IFileOpenDialog` COM object, as
                    // that interface requires. Every `windows-rs` COM call
                    // (`CoCreateInstance`, `dialog.Show`/`SetOptions`, `results.*`,
                    // `item.GetDisplayName`) is a checked, typed FFI wrapper with
                    // no raw pointer of ours to validate. `CoTaskMemFree` below
                    // frees the `PWSTR` `name` owns from COM — it is only used
                    // (via `name.to_string()`, which copies the string out) before
                    // this free, and each loop iteration gets its own `name` from
                    // `GetDisplayName`, so there is no double-free or use-after-free
                    // across iterations.
                    unsafe {
                        use windows::Win32::{
                            System::Com::{
                                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance,
                                CoInitializeEx, CoTaskMemFree,
                            },
                            UI::Shell::{
                                FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                                FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
                                SIGDN_FILESYSPATH,
                            },
                        };

                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                        let dialog: IFileOpenDialog =
                            CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;

                        let mut flags = FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
                        if options.multiple {
                            flags |= FOS_ALLOWMULTISELECT;
                        }
                        if options.directories {
                            flags |= FOS_PICKFOLDERS;
                        }
                        dialog.SetOptions(flags)?;

                        match dialog.Show(None) {
                            Ok(()) => {}
                            Err(e)
                                if e.code()
                                    == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                            {
                                return Ok(None);
                            }
                            Err(e) => return Err(e),
                        }

                        let results = dialog.GetResults()?;
                        let count = results.GetCount()?;
                        let mut paths = Vec::with_capacity(count as usize);
                        for i in 0..count {
                            let item = results.GetItemAt(i)?;
                            let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                            let path_str = name.to_string()?;
                            paths.push(std::path::PathBuf::from(path_str));
                            CoTaskMemFree(Some(name.as_ptr() as *const _));
                        }
                        Ok(Some(paths))
                    }
                },
            )
            .join()
            .map_err(|_| PlatformError::Dialog {
                message: "File dialog thread panicked".to_string(),
            })?
            .map_err(|error| PlatformError::Dialog {
                message: error.to_string(),
            })?;
            Ok(result)
        })
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::task::Task<Result<Option<std::path::PathBuf>, PlatformError>> {
        let dir = directory.to_path_buf();
        let name = suggested_name.map(std::string::ToString::to_string);
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            // Same raw-`windows::core::Result` closure + single typed
            // mapping shape as `prompt_for_paths` above.
            let result = std::thread::spawn(
                move || -> windows::core::Result<Option<std::path::PathBuf>> {
                    // SAFETY: see `prompt_for_paths` above — same
                    // dedicated-STA-thread, checked-COM-wrapper reasoning.
                    // `dir_wide` is explicitly NUL-terminated and outlives the
                    // `SHCreateItemFromParsingName` call that borrows its
                    // pointer; the single `CoTaskMemFree` below frees the one
                    // `name` this function's single `IFileSaveDialog::GetResult`
                    // path produces, after `name.to_string()` has already copied
                    // the string out.
                    unsafe {
                        use windows::Win32::{
                            System::Com::{
                                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance,
                                CoInitializeEx, CoTaskMemFree,
                            },
                            UI::Shell::{
                                FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
                                FileSaveDialog, IFileSaveDialog, IShellItem,
                                SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
                            },
                        };

                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                        let dialog: IFileSaveDialog =
                            CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;

                        dialog.SetOptions(
                            FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT,
                        )?;

                        // Set initial directory
                        let dir_wide: Vec<u16> = dir
                            .to_string_lossy()
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        if let Ok(folder) = SHCreateItemFromParsingName::<PCWSTR, _, IShellItem>(
                            PCWSTR(dir_wide.as_ptr()),
                            None,
                        ) {
                            let _ = dialog.SetFolder(&folder);
                        }

                        // Set suggested file name
                        if let Some(ref name) = name {
                            let name_hstring = windows::core::HSTRING::from(name.as_str());
                            let _ = dialog.SetFileName(&name_hstring);
                        }

                        match dialog.Show(None) {
                            Ok(()) => {}
                            Err(e)
                                if e.code()
                                    == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                            {
                                return Ok(None);
                            }
                            Err(e) => return Err(e),
                        }

                        let result = dialog.GetResult()?;
                        let name = result.GetDisplayName(SIGDN_FILESYSPATH)?;
                        let path_str = name.to_string()?;
                        let path = std::path::PathBuf::from(path_str);
                        CoTaskMemFree(Some(name.as_ptr() as *const _));
                        Ok(Some(path))
                    }
                },
            )
            .join()
            .map_err(|_| PlatformError::Dialog {
                message: "File dialog thread panicked".to_string(),
            })?
            .map_err(|error| PlatformError::Dialog {
                message: error.to_string(),
            })?;
            Ok(result)
        })
    }

    // ==================== Keyboard (US3 T045) ====================

    fn keyboard_layout(&self) -> String {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayoutNameW;
        // SAFETY: `buffer` is a stack-local, zero-initialized `[u16; 9]`
        // sized to `KL_NAMELENGTH`, the exact size this API requires;
        // `&mut buffer` gives it a valid, correctly-sized out-parameter, and
        // it is only read (`is_ok()` branch) after the call reports success.
        unsafe {
            let mut buffer = [0u16; 9]; // KL_NAMELENGTH = 9
            if GetKeyboardLayoutNameW(&mut buffer).is_ok() {
                String::from_utf16_lossy(
                    &buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())],
                )
            } else {
                String::new()
            }
        }
    }

    // ==================== File System Integration ====================

    fn app_path(&self) -> Result<std::path::PathBuf, PlatformError> {
        // SAFETY: `buffer` is a stack-local `[u16; MAX_PATH]`; `&mut buffer`
        // gives `GetModuleFileNameW` a valid, correctly-sized out-parameter,
        // and only the first `len` code units it reports writing are read
        // below — a `len == 0` failure returns before any read.
        unsafe {
            let mut buffer = [0u16; 260]; // MAX_PATH, stack-allocated
            let len = GetModuleFileNameW(None, &mut buffer);
            if len == 0 {
                return Err(PlatformError::AppPath {
                    message: windows::core::Error::from_thread().to_string(),
                });
            }
            Ok(std::path::PathBuf::from(String::from_utf16_lossy(
                &buffer[..len as usize],
            )))
        }
    }
}

impl Drop for WindowsPlatform {
    fn drop(&mut self) {
        tracing::debug!("Dropping WindowsPlatform");

        // Destroy message window
        //
        // SAFETY: `DestroyWindow` takes `self.message_window` by value; the
        // `is_invalid()` guard skips the call for a handle that was never
        // successfully created. This window carries no `GWLP_USERDATA`
        // context (it is message-only and never routed through
        // `WindowsWindow::new`), so there is no allocation to reclaim here.
        if !self.message_window.is_invalid() {
            unsafe {
                if let Err(error) = DestroyWindow(self.message_window) {
                    tracing::warn!(
                        hwnd = ?self.message_window,
                        ?error,
                        "DestroyWindow failed for the message-only window"
                    );
                }
            }
        }

        // COM is left by the `apartment` field, after this body, or later by
        // the last window's text services if any still live.
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
