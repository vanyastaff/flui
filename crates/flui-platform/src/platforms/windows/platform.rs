//! Windows platform implementation

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};
use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, HBRUSH, PAINTSTRUCT},
        System::{
            LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
            Threading::GetCurrentThreadId,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            WindowsAndMessaging::{
                CS_HREDRAW, CS_OWNDC, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GWLP_USERDATA, GetClassNameW, GetForegroundWindow, GetMessageW,
                GetWindowLongPtrW, GetWindowThreadProcessId, HICON, HTCLIENT, HWND_MESSAGE,
                IDC_ARROW, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassW,
                SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowPos, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
                WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
                WM_MOUSEWHEEL, WM_MOVE, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR,
                WM_SETFOCUS, WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP,
                WNDCLASSW,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    display::enumerate_displays,
    util::{WINDOW_CLASS_NAME, get_x_lparam, get_y_lparam, hiword, load_cursor_style},
    window::WindowsWindow,
};
use crate::{
    config::WindowConfiguration,
    data_transfer::{DataTransferSource, NullDataTransferSource},
    executor::BackgroundExecutor,
    shared::{PlatformHandlers, WindowCallbacks, hwnd_affinity::ContextLedger},
    traits::{
        Clipboard, DesktopCapabilities, OwnerPlatform, Platform, PlatformCapabilities,
        PlatformDisplay, PlatformExecutor, PlatformReadyCallback, PlatformWindow, WindowAppearance,
        WindowEvent, WindowId, WindowMode, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks},
    },
};

/// Ensures window class is registered exactly once (sound replacement for
/// `static mut bool`).
static REGISTER_WINDOW_CLASS: std::sync::Once = std::sync::Once::new();

/// Context data stored per window for event dispatch
pub(super) struct WindowContext {
    /// Window ID for event dispatch
    pub window_id: WindowId,
    /// Reference to platform handlers (global)
    pub handlers: Arc<Mutex<PlatformHandlers>>, // PORT-CHECK-OK-SP6: WindowsPlatform handlers Arc<Mutex<>>; mirrors PlatformHandlers callback storage; pre-existing SP-6
    /// Per-window callbacks for event delivery
    pub callbacks: Arc<WindowCallbacks>,
    /// Scale factor for coordinate conversion.
    ///
    /// `Cell`, not a plain `f32`: `window_proc` only ever holds a shared
    /// `&WindowContext` (see its `# Safety` section), and `WM_DPICHANGED` is
    /// the one message that updates this field — `Cell::set` lets it do so
    /// through that shared reference instead of forging a second, aliasing
    /// `&mut WindowContext` from the raw `GWLP_USERDATA` pointer while the
    /// shared one is still live.
    pub scale_factor: std::cell::Cell<f32>,
    /// Current window mode (replaces display_state + saved bounds)
    pub mode: std::cell::Cell<WindowMode>,
    /// Last known size (before minimization) for restore detection
    pub last_size: std::cell::Cell<Size<DevicePixels>>,
    /// Window configuration (hotkeys, debouncing, etc.)
    pub config: WindowConfiguration,
    /// Is mouse hovering over this window? (T034)
    pub is_hovered: std::cell::Cell<bool>,
    /// Current keyboard modifiers (T035)
    pub modifiers: std::cell::Cell<keyboard_types::Modifiers>,
    /// Cursor selected by this exact window's presentation.
    pub cursor: std::cell::Cell<CursorIcon>,
    /// Window style bits before fullscreen (Windows-specific: WS_OVERLAPPEDWINDOW, etc.)
    ///
    /// Stored here instead of in `WindowMode` to keep the cross-platform enum
    /// free of platform-specific fields.
    pub restore_style: std::cell::Cell<u32>,
    /// Last surface-visibility value dispatched through
    /// `WindowCallbacks::dispatch_visibility_status_change` (hidden-surface
    /// gating). Two message families feed it — `WM_SIZE`
    /// (minimize/restore) and `WM_SHOWWINDOW` (hide/show) — via the pure
    /// rules in `crate::shared::visibility`; this cell is the shared edge
    /// filter that keeps the two from double-dispatching the same state.
    /// Starts `false`: windows are created hidden and shown afterwards
    /// (`WindowsWindow::new`'s `ShowWindow` runs after the context is
    /// installed), so the creation-time `WM_SHOWWINDOW` dispatches the
    /// initial `true` through the same wire as every later change.
    pub last_visibility_dispatched: std::cell::Cell<bool>,
    /// High half of a UTF-16 surrogate pair from an out-of-band `WM_CHAR`,
    /// held until its low half arrives in the next message (Windows splits
    /// astral-plane characters across two `WM_CHAR`s). Only the stray-char
    /// path in `window_proc` uses this; the `WM_KEYDOWN` drain sees a whole
    /// burst at once and never needs cross-message state.
    pub pending_high_surrogate: std::cell::Cell<Option<u16>>,
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
    /// Dispatch a window event safely without holding locks
    ///
    /// This method extracts the handler, releases the lock, calls the handler,
    /// then re-acquires the lock to restore it. This prevents deadlocks when
    /// the handler tries to acquire the same lock.
    ///
    /// The restore touches `self` AFTER the handler returns, and the handler
    /// may have closed this very window — that is sound only because every
    /// caller holds a [`ContextGuard`] over `self`, which defers the
    /// context's free past the borrow even when the handler's close runs a
    /// nested `WM_DESTROY` (which merely retires it).
    #[inline]
    pub(super) fn dispatch_event(&self, event: WindowEvent) {
        // Take the handler out of the lock
        let handler = self.handlers.lock().window_event.take();

        // Release the lock before calling the handler
        if let Some(mut handler) = handler {
            handler(event);

            // Restore the handler after the call
            self.handlers.lock().window_event = Some(handler);
        }
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
            Some(f(guard.context()))
        }
        UserDataVerdict::Refuse(reason) => {
            tracing::debug!(
                op,
                hwnd = ?hwnd,
                ?reason,
                "refusing to touch this window's native context"
            );
            None
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
    /// Message-only window for platform messages
    message_window: HWND,

    /// All created windows (keyed by HWND)
    windows: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,

    /// Platform handlers (callbacks from platform to framework)
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// Background executor for async tasks
    background_executor: Arc<BackgroundExecutor>,

    /// Window configuration (shared across all windows)
    config: WindowConfiguration,

    /// Records the owner thread so the thread-affine Win32 operations below
    /// can `debug_assert` their caller (ADR-0039). Bound at construction —
    /// the message-only window's queue belongs to the constructing thread —
    /// and re-asserted by `run`. Pre-run window creation on that same
    /// thread (the Win32 examples) stays legal.
    affinity: flui_foundation::OwnerAffinity,
}

// SAFETY, per field: `windows` and `handlers` are `Arc<Mutex<..>>`, the
// executors are `Arc`-shared and internally synchronized, and `config` is plain
// data. The only non-`Sync` member is `message_window: HWND`, a bare address
// that is never dereferenced here.
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
// — see the event-loop affinity gap in
// `docs/audits/2026-07-25-upgrade-pack-audit.md`.
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
    pub fn new() -> Result<Self> {
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
    /// use flui_platform::{WindowsPlatform, WindowConfiguration, FullscreenMonitor};
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
    pub fn with_config(config: WindowConfiguration) -> Result<Self> {
        // SAFETY: `CoInitializeEx` takes no pointer arguments (`None` for
        // the reserved parameter) and its `HRESULT` is checked before
        // anything downstream assumes COM is initialized on this thread —
        // this call establishes the calling thread as an STA, which is also
        // the thread this platform's `affinity` binds to a few lines below.
        //
        // Initialize COM for drag-and-drop, clipboard, etc.
        unsafe {
            use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                return Err(anyhow::anyhow!("Failed to initialize COM: {hr:?}"));
            }
        }

        // SAFETY: `SetProcessDpiAwarenessContext` takes a constant by value,
        // no pointer arguments; failure (already set, or an OS predating
        // per-monitor-v2 awareness) is expected and non-fatal, hence the
        // discarded result.
        //
        // Set DPI awareness to per-monitor v2 (best quality)
        // Ignore errors - this can fail if already set or on older Windows
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        // SAFETY: `register_window_class`'s own `# Safety` contract (below)
        // is discharged by `REGISTER_WINDOW_CLASS: Once` making the call
        // race-free and idempotent regardless of caller thread.
        //
        // Register window class
        unsafe {
            Self::register_window_class()?;
        }

        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments. `CreateWindowExW` here creates the
        // message-only window with `HWND_MESSAGE` as parent and constant
        // Win32 arguments (no caller-supplied buffers); its class was just
        // registered above on this same thread, satisfying the ordering
        // Win32 requires (register-before-create).
        //
        // Create message-only window for platform messages
        let message_window = unsafe {
            let hinstance = GetModuleHandleW(None)
                .map_err(|e| anyhow::anyhow!("Failed to get module handle: {e:?}"))?;

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
            .map_err(|e| anyhow::anyhow!("Failed to create message window: {e:?}"))?
        };

        // Create executors
        let background_executor = Arc::new(BackgroundExecutor::new());

        tracing::info!("Windows platform initialized with Tokio executors");

        let platform = Self {
            message_window,
            windows: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(Mutex::new(PlatformHandlers::default())),
            background_executor,
            config,
            affinity: flui_foundation::OwnerAffinity::new(),
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
    unsafe fn register_window_class() -> Result<()> {
        // SAFETY: per the `# Safety` contract above, `Self::window_proc`'s
        // signature matches `WNDPROC`. `GetModuleHandleW(None)` takes no
        // pointer arguments. `wc.lpszClassName`/`wc.hCursor` reference
        // process-lifetime statics (`WINDOW_CLASS_NAME`, the loaded cursor
        // resource); `&raw const wc` gives `RegisterClassW` a valid pointer
        // to the fully-initialized, stack-local `WNDCLASSW`. `call_once`
        // guarantees this body executes at most once, so there is no
        // concurrent registration to race.
        unsafe {
            let mut result: Result<()> = Ok(());

            REGISTER_WINDOW_CLASS.call_once(|| {
                let reg = (|| -> Result<()> {
                    let hinstance =
                        GetModuleHandleW(None).context("Failed to get module handle")?;

                    let wc = WNDCLASSW {
                        style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
                        lpfnWndProc: Some(Self::window_proc),
                        cbClsExtra: 0,
                        cbWndExtra: 0,
                        hInstance: hinstance.into(),
                        hIcon: HICON::default(),
                        hCursor: load_cursor_style(IDC_ARROW)?,
                        hbrBackground: HBRUSH(std::ptr::null_mut()),
                        lpszMenuName: PCWSTR::null(),
                        lpszClassName: WINDOW_CLASS_NAME,
                    };

                    let atom = RegisterClassW(&raw const wc);
                    if atom == 0 {
                        return Err(windows::core::Error::from_thread().into());
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
                        // Fire per-window on_close callback (FnOnce)
                        ctx.callbacks.dispatch_close();

                        // Dispatch Closed event to global handlers
                        ctx.dispatch_event(WindowEvent::Closed(ctx.window_id));

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
                    }

                    LRESULT(0)
                }

                WM_ERASEBKGND => {
                    // Return 1 to prevent Windows from erasing background
                    // This allows Mica backdrop and other DWM effects to show through
                    tracing::debug!("WM_ERASEBKGND - preventing background erase");
                    LRESULT(1)
                }

                WM_PAINT => {
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
                        // Skip rendering for minimized windows to save CPU/GPU resources
                        let should_skip = WindowsWindow::should_skip_render(hwnd);
                        if should_skip {
                            tracing::trace!("Skipping render for minimized window");
                        } else {
                            // No GDI painting here. This HWND carries a D3D12 flip-model
                            // swapchain (wgpu-on-DX12 is always flip-model). Per Microsoft,
                            // GDI and flip model cannot share an HWND: after the first
                            // `Present1`, GDI updates to the window are dropped, and the
                            // contention with the compositor produces visible jitter while
                            // the window is being live-resized. The background comes from the
                            // wgpu clear pass + the scene, not from a GDI fill (the old
                            // FillRect was overwritten by the present in the same frame).
                            if let Some(ctx) = ctx {
                                // Fire per-window on_request_frame callback
                                ctx.callbacks.dispatch_request_frame();

                                // Also dispatch RedrawRequested to global handlers
                                ctx.dispatch_event(WindowEvent::RedrawRequested {
                                    window_id: ctx.window_id,
                                });
                            }
                        }
                        let _ = EndPaint(hwnd, &raw const ps);
                    }
                    LRESULT(0)
                }

                WM_SIZE => {
                    use super::util::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

                    let width = get_x_lparam(lparam).max(1);
                    let height = get_y_lparam(lparam).max(1);
                    let size_type = wparam.0 as u32;

                    if let Some(ctx) = ctx {
                        use flui_types::geometry::DevicePixels;
                        let size = Size::new(DevicePixels(width), DevicePixels(height));
                        let prev_mode = ctx.mode.get();

                        // Handle state transition and dispatch appropriate event
                        let (new_mode, event) = match size_type {
                            SIZE_MINIMIZED => {
                                tracing::info!("📦 Window Minimized");
                                // Validate transition
                                let candidate = WindowMode::Minimized {
                                    previous: Bounds {
                                        origin: Point::new(DevicePixels(0), DevicePixels(0)),
                                        size: ctx.last_size.get(),
                                    },
                                };
                                if prev_mode.can_transition_to(&candidate) {
                                    // Save current size before minimizing
                                    if !prev_mode.is_minimized() {
                                        ctx.last_size.set(size);
                                    }
                                    (
                                        candidate,
                                        Some(WindowEvent::Minimized {
                                            window_id: ctx.window_id,
                                        }),
                                    )
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
                                let candidate = WindowMode::Maximized {
                                    previous: Bounds {
                                        origin: Point::new(DevicePixels(0), DevicePixels(0)),
                                        size: ctx.last_size.get(),
                                    },
                                };
                                if prev_mode.can_transition_to(&candidate) {
                                    ctx.last_size.set(size);
                                    (
                                        candidate,
                                        Some(WindowEvent::Maximized {
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
                                let event = if prev_mode.is_minimized() || prev_mode.is_maximized()
                                {
                                    tracing::info!("📐 Window Restored: {}x{}", width, height);
                                    WindowEvent::Restored {
                                        window_id: ctx.window_id,
                                        size,
                                    }
                                } else {
                                    tracing::debug!("📐 Window Resized: {}x{}", width, height);
                                    WindowEvent::Resized {
                                        window_id: ctx.window_id,
                                        size,
                                    }
                                };
                                (WindowMode::Normal, Some(event))
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

                        // Hidden-surface gating: minimize/restore is the
                        // visibility signal Win32 actually delivers (no
                        // occlusion events exist here), so it must reach
                        // `dispatch_visibility_status_change` — the wire the
                        // runtime's `FrameClock` gate and `AppLifecycleState`
                        // derivation hang off. Decided by the pure rule in
                        // `shared::visibility` (host-tested); the cell is the
                        // edge filter shared with the WM_SHOWWINDOW arm.
                        if let Some(visible) = crate::shared::visibility::win32_size_visibility(
                            size_type,
                            prev_mode.is_minimized(),
                        ) && crate::shared::visibility::visibility_edge(
                            ctx.last_visibility_dispatched.get(),
                            visible,
                        )
                        .is_some()
                        {
                            ctx.last_visibility_dispatched.set(visible);
                            tracing::debug!(
                                ?visible,
                                "Window surface visibility changed (WM_SIZE)"
                            );
                            ctx.callbacks.dispatch_visibility_status_change(visible);
                        }

                        // Fire per-window on_resize callback (for all size changes except minimize)
                        if size_type != SIZE_MINIMIZED {
                            let logical_size = Size::new(
                                flui_types::geometry::px(super::util::device_to_logical(
                                    width,
                                    ctx.scale_factor.get(),
                                )),
                                flui_types::geometry::px(super::util::device_to_logical(
                                    height,
                                    ctx.scale_factor.get(),
                                )),
                            );
                            ctx.callbacks
                                .dispatch_resize(logical_size, ctx.scale_factor.get());
                        }

                        // Dispatch event to global handlers if any
                        if let Some(event) = event {
                            ctx.dispatch_event(event);
                        }

                        // Render synchronously at the new size, in the same WM_SIZE
                        // message that reconfigured the surface. Win32 does not post a
                        // WM_PAINT for every WM_SIZE during the modal resize loop, so
                        // without this the next rendered frame lags ≥1 step behind the
                        // window size: the compositor stretches the stale frame and
                        // fixed-position content appears to jitter while dragging the
                        // border. `dispatch_resize` above already released the renderer
                        // lock (its closure returned), so this re-locks cleanly; a
                        // minimized window has nothing to present.
                        if size_type != SIZE_MINIMIZED {
                            // Render synchronously, in the same WM_SIZE message that
                            // reconfigured the surface, so the new size is presented within
                            // the modal resize loop instead of waiting for the next WM_PAINT
                            // (which Windows does not reliably post per drag step).
                            // `dispatch_resize` above already released the renderer lock, so
                            // this re-locks cleanly; a minimized window has nothing to present.
                            ctx.callbacks.dispatch_request_frame();
                        }
                    }

                    LRESULT(0)
                }

                WM_SHOWWINDOW => {
                    // Hidden-surface gating, hide/show half (the
                    // minimize/restore half lives in the WM_SIZE arm above;
                    // both share `last_visibility_dispatched` as the edge
                    // filter). Only `lParam == 0` — a genuine
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
                        if let Some(visible) =
                            crate::shared::visibility::win32_show_window_visibility(
                                shown,
                                ctx.mode.get().is_minimized(),
                            )
                            && crate::shared::visibility::visibility_edge(
                                ctx.last_visibility_dispatched.get(),
                                visible,
                            )
                            .is_some()
                        {
                            ctx.last_visibility_dispatched.set(visible);
                            tracing::debug!(
                                ?visible,
                                "Window surface visibility changed (WM_SHOWWINDOW)"
                            );
                            ctx.callbacks.dispatch_visibility_status_change(visible);
                        }
                    }
                    // DefWindowProc continues the ordinary show/hide
                    // processing this message announces. (Already inside
                    // this function's one big SAFETY block, like the other
                    // pass-through arms.)
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                WM_MOVE => {
                    let x = get_x_lparam(lparam);
                    let y = get_y_lparam(lparam);
                    tracing::debug!("Window Moved: ({}, {})", x, y);

                    if let Some(ctx) = ctx {
                        // Fire per-window on_moved callback
                        ctx.callbacks.dispatch_moved();

                        // Dispatch Moved event to global handlers
                        use flui_types::geometry::{Point, px};
                        let position = Point::new(
                            px(x as f32 / ctx.scale_factor.get()),
                            px(y as f32 / ctx.scale_factor.get()),
                        );
                        ctx.dispatch_event(WindowEvent::Moved {
                            window_id: ctx.window_id,
                            position,
                        });
                    }

                    LRESULT(0)
                }

                WM_DPICHANGED => {
                    // Extract new DPI from wparam
                    let new_dpi = hiword(wparam.0 as u32) as f32;
                    let new_scale = new_dpi / 96.0; // 96 DPI = 1.0 scale
                    tracing::info!("🔍 DPI Changed: {} (scale: {:.2}x)", new_dpi, new_scale);

                    // Dispatch ScaleFactorChanged event
                    if let Some(ctx) = ctx {
                        ctx.dispatch_event(WindowEvent::ScaleFactorChanged {
                            window_id: ctx.window_id,
                            scale_factor: new_scale as f64,
                        });

                        // Update context scale factor through the shared
                        // reference already held above — see the field doc
                        // on `WindowContext::scale_factor` for why this must
                        // not go through a second `&mut` reborrow of
                        // `ctx_ptr`.
                        ctx.scale_factor.set(new_scale);

                        // Suggested rect for new DPI
                        //
                        // SAFETY: `lparam` carries a pointer to a `RECT`
                        // supplied by Win32 for `WM_DPICHANGED` specifically
                        // (documented behavior of this message); the
                        // null-check guards a caller that violates that
                        // documented contract, and `rect` is copied out
                        // (`RECT: Copy`) rather than referenced further.
                        let suggested_rect = lparam.0 as *const RECT;
                        if !suggested_rect.is_null() {
                            let rect = *suggested_rect;
                            if let Err(error) = SetWindowPos(
                                hwnd,
                                None,
                                rect.left,
                                rect.top,
                                rect.right - rect.left,
                                rect.bottom - rect.top,
                                SWP_NOZORDER | SWP_NOACTIVATE,
                            ) {
                                tracing::warn!(
                                    ?hwnd,
                                    ?error,
                                    "SetWindowPos (DPI-change suggested rect) failed"
                                );
                            }
                        }
                    }

                    LRESULT(0)
                }

                WM_MOUSEMOVE => {
                    if let Some(ctx) = ctx {
                        // Request WM_MOUSELEAVE notification for hover tracking
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&raw mut tme);

                        // Track hover state (T034)
                        ctx.is_hovered.set(true);

                        // Dispatch hover enter (will be cleared on WM_MOUSELEAVE)
                        ctx.callbacks.dispatch_hover_status_change(true);

                        use super::events::mouse_move_event;
                        let event = mouse_move_event(wparam, lparam, ctx.scale_factor.get());
                        ctx.callbacks.dispatch_input(event);
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

                WM_LBUTTONDOWN => {
                    if let Some(ctx) = ctx {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Primary,
                            true,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_RBUTTONDOWN => {
                    if let Some(ctx) = ctx {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Secondary,
                            true,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MBUTTONDOWN => {
                    if let Some(ctx) = ctx {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Auxiliary,
                            true,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_LBUTTONUP => {
                    if let Some(ctx) = ctx {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Primary,
                            false,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_RBUTTONUP => {
                    if let Some(ctx) = ctx {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Secondary,
                            false,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MBUTTONUP => {
                    if let Some(ctx) = ctx {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Auxiliary,
                            false,
                            wparam,
                            lparam,
                            ctx.scale_factor.get(),
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MOUSEWHEEL => {
                    if let Some(ctx) = ctx {
                        use super::events::mouse_wheel_event;
                        let event = mouse_wheel_event(hwnd, wparam, lparam, ctx.scale_factor.get());
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MOUSEHWHEEL => {
                    if let Some(ctx) = ctx {
                        use super::events::mouse_hwheel_event;
                        let event =
                            mouse_hwheel_event(hwnd, wparam, lparam, ctx.scale_factor.get());
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let vk = wparam.0 as u16;
                    let is_repeat = (lparam.0 & (1 << 30)) != 0;

                    // Check if fullscreen hotkey is pressed (configurable, default F11)
                    if let Some(ctx) = ctx {
                        if let Some(hotkey) = ctx.config.fullscreen_hotkey
                            && vk == hotkey
                            && !is_repeat
                        {
                            tracing::info!(
                                "Fullscreen hotkey (VK={:#04x}) pressed - toggling fullscreen",
                                hotkey
                            );
                            WindowsWindow::toggle_fullscreen_for_hwnd(hwnd);
                        }

                        // Track modifiers (T035)
                        ctx.modifiers.set(current_modifiers());

                        // Merge the fully-translated character(s) for this
                        // keystroke into the keydown. The message loop runs
                        // `TranslateMessage` before `DispatchMessageW` (see
                        // `run`), so the WM_CHAR burst for this keydown —
                        // shift/caps/AltGr applied, dead keys resolved — is
                        // already in the queue and is drained synchronously
                        // here. Pairing model and merge rules:
                        // `crate::shared::keys` module doc.
                        let translated = drain_translated_chars(hwnd);

                        // Dispatch keyboard event via per-window callback
                        use super::events::key_down_event;
                        let event = key_down_event(wparam, lparam, translated);
                        ctx.callbacks.dispatch_input(event);
                    }

                    LRESULT(0)
                }

                WM_KEYUP | WM_SYSKEYUP => {
                    if let Some(ctx) = ctx {
                        // Track modifiers (T035)
                        ctx.modifiers.set(current_modifiers());

                        use super::events::key_up_event;
                        let event = key_up_event(wparam, lparam);
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
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
                            ctx.callbacks.dispatch_input(stray_char_event(text));
                        }
                    }
                    LRESULT(0)
                }

                WM_SETFOCUS => {
                    tracing::debug!("Window Focused");

                    if let Some(ctx) = ctx {
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

                // T025: Mouse hover tracking — WM_MOUSELEAVE (0x02A3)
                0x02A3 => {
                    if let Some(ctx) = ctx {
                        // Track hover state (T034)
                        ctx.is_hovered.set(false);

                        ctx.callbacks.dispatch_hover_status_change(false);
                    }
                    LRESULT(0)
                }

                // T026: System theme/appearance change
                WM_SETTINGCHANGE => {
                    if let Some(ctx) = ctx {
                        ctx.callbacks.dispatch_appearance_changed();
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                // T046: Keyboard layout change
                WM_INPUTLANGCHANGE => {
                    if let Some(ctx) = ctx {
                        // Dispatch keyboard layout change via take/restore pattern
                        let handler = ctx.handlers.lock().keyboard_layout_changed.take();
                        if let Some(mut handler) = handler {
                            handler();
                            ctx.handlers.lock().keyboard_layout_changed = Some(handler);
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    /// Run the Windows message loop (internal implementation)
    fn run_message_loop() {
        tracing::info!("Starting Windows message loop");

        // SAFETY: `msg` is a stack-local `MSG`; `&raw mut msg`/`&raw const
        // msg` give `GetMessageW`/`TranslateMessage`/`DispatchMessageW`
        // valid, correctly-sized pointers to it, and `GetMessageW` only
        // returns `TRUE` after filling `msg` in, so it is never read
        // uninitialized. `DispatchMessageW` is what invokes `window_proc`
        // (registered per-class in `register_window_class`) on this thread.
        unsafe {
            let mut msg = MSG::default();

            while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }

            tracing::info!("Message loop exited with code: {}", msg.wParam.0);
        }
    }
}

impl Platform for WindowsPlatform {
    // ==================== Core System ====================

    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    // ==================== Lifecycle ====================

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> anyhow::Result<()> {
        tracing::info!("Running Windows platform");

        // Idempotent from the constructing thread; a `run` migrated to a
        // different thread is a bug the re-bind assertion surfaces (the
        // message-only window's queue is bound to the constructing thread).
        self.affinity.bind_current();

        // No owner lane on this backend: every `OwnerPlatform::open_window`
        // call creates directly and is always `Ready` (ADR-0039 slice 2).
        // `PlatformProxy` is permanently unsupported until slice 3 adopts a
        // lane here.
        let platform: Arc<dyn Platform> = Arc::new(*self);
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::new(Arc::clone(&platform)));
        // `on_ready` runs before the message pump starts: on `Err`, skip
        // the pump entirely and return rather than servicing messages for a
        // half-built app.
        on_ready(OwnerPlatform::new(platform, hooks))?;

        Self::run_message_loop();
        Ok(())
    }

    fn quit(&self) {
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

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        enumerate_displays()
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        enumerate_displays().into_iter().find(|d| d.is_primary())
    }

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn PlatformWindow>> {
        // An HWND's message queue belongs to the creating thread; a window
        // minted off the owner thread is silently mis-affined (ADR-0039).
        self.affinity
            .debug_assert_owner("WindowsPlatform::open_window");
        tracing::info!("Opening window: {:?}", options.title);

        let window = WindowsWindow::new(
            options,
            self.windows.clone(),
            self.handlers.clone(),
            self.config.clone(),
        )?;
        let hwnd_value = window.hwnd().0 as isize;

        // Store window
        self.windows.lock().insert(hwnd_value, window.clone());

        Ok(window)
    }

    // ==================== Input & Clipboard ====================

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        Arc::new(super::WindowsClipboard::new())
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // No Win32 transport yet (IDropTarget/IDataObject land with the
        // native slices of ADR-0038): inert and honest.
        Arc::new(NullDataTransferSource)
    }

    // ==================== Platform Capabilities ====================

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        // TODO: Return actual capabilities
        &WINDOWS_CAPABILITIES
    }

    fn name(&self) -> &'static str {
        "Windows"
    }

    // ==================== Callbacks ====================

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.handlers.lock().quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.handlers.lock().window_event = Some(callback);
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        self.handlers.lock().keyboard_layout_changed = Some(callback);
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

    // ==================== Appearance (US3 T040) ====================

    fn window_appearance(&self) -> WindowAppearance {
        // Read system theme from registry: AppsUseLightTheme
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
        };
        // SAFETY: `subkey`/`value_name` are `Vec<u16>` explicitly
        // NUL-terminated (`"...\0".encode_utf16()`), matching what
        // `PCWSTR` requires, and kept alive across the calls that borrow
        // their pointers. `hkey` is a stack-local `HKEY` written through
        // `&raw mut hkey`; `RegQueryValueExW` only reads through
        // `&raw mut data` after `status.is_err()` returned early on
        // failure to open the key, and `data_size` is seeded to
        // `size_of::<u32>()` so the registry API cannot write past `data`.
        // `RegCloseKey` runs unconditionally once `hkey` was successfully
        // opened, on every return path below (both the early `is_err()`
        // return and the final value).
        unsafe {
            let mut hkey = HKEY::default();
            let subkey: Vec<u16> =
                "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
                    .encode_utf16()
                    .collect();
            let value_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();

            let status = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                Some(0),
                KEY_READ,
                &raw mut hkey,
            );
            if status.is_err() {
                return WindowAppearance::Light;
            }

            let mut data: u32 = 1;
            let mut data_size = std::mem::size_of::<u32>() as u32;
            let status = RegQueryValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                None,
                None,
                Some((&raw mut data).cast::<u8>()),
                Some(&raw mut data_size),
            );
            let _ = RegCloseKey(hkey);

            if status.is_err() {
                return WindowAppearance::Light;
            }

            if data == 0 {
                WindowAppearance::Dark
            } else {
                WindowAppearance::Light
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
    ) -> crate::task::Task<Result<Option<Vec<std::path::PathBuf>>>> {
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            // COM file dialogs must run on an STA thread
            let result = std::thread::spawn(move || -> Result<Option<Vec<std::path::PathBuf>>> {
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
                            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                            CoTaskMemFree,
                        },
                        UI::Shell::{
                            FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                            FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
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
                        Err(e) => return Err(e.into()),
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
            })
            .join()
            .map_err(|_| anyhow::anyhow!("File dialog thread panicked"))??;
            Ok(result)
        })
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::task::Task<Result<Option<std::path::PathBuf>>> {
        let dir = directory.to_path_buf();
        let name = suggested_name.map(std::string::ToString::to_string);
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            let result = std::thread::spawn(move || -> Result<Option<std::path::PathBuf>> {
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
                            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                            CoTaskMemFree,
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
                        Err(e) => return Err(e.into()),
                    }

                    let result = dialog.GetResult()?;
                    let name = result.GetDisplayName(SIGDN_FILESYSPATH)?;
                    let path_str = name.to_string()?;
                    let path = std::path::PathBuf::from(path_str);
                    CoTaskMemFree(Some(name.as_ptr() as *const _));
                    Ok(Some(path))
                }
            })
            .join()
            .map_err(|_| anyhow::anyhow!("File dialog thread panicked"))??;
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

    fn app_path(&self) -> Result<std::path::PathBuf> {
        // SAFETY: `buffer` is a stack-local `[u16; MAX_PATH]`; `&mut buffer`
        // gives `GetModuleFileNameW` a valid, correctly-sized out-parameter,
        // and only the first `len` code units it reports writing are read
        // below — a `len == 0` failure returns before any read.
        unsafe {
            let mut buffer = [0u16; 260]; // MAX_PATH, stack-allocated
            let len = GetModuleFileNameW(None, &mut buffer);
            if len == 0 {
                return Err(windows::core::Error::from_thread().into());
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

        // SAFETY: `CoUninitialize` takes no arguments, so this call itself
        // cannot be memory-unsafe regardless of which thread runs it.
        //
        // NOT established: that this runs on the same thread that called
        // `CoInitializeEx` in `with_config`. COM's apartment state is
        // per-thread, and `CoUninitialize` is only the matching call for
        // the thread that initialized it — but `WindowsPlatform` is `Send`
        // (see its impl above), so nothing stops this value being moved to
        // and dropped on a different thread than the one that constructed
        // it. If that happens, this does not uninitialize the constructing
        // thread's COM apartment at all; at worst it leaves that thread's
        // COM reference count unbalanced (permanently initialized) and/or
        // calls `CoUninitialize` on a thread whose own apartment state this
        // struct never tracked — an accounting/leak concern, not memory
        // unsafety, and not fixed by this comment.
        //
        // Uninitialize COM
        unsafe {
            use windows::Win32::System::Com::CoUninitialize;
            CoUninitialize();
        }
    }
}

// PlatformHandlers is imported from crate::shared

// ==================== Helper Functions ====================

/// Read current keyboard modifier state from Win32 (T035)
fn current_modifiers() -> keyboard_types::Modifiers {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    // SAFETY: `GetKeyState` takes a plain `i32` virtual-key code and returns
    // a plain `i16`/`u16` bit pattern — no pointer arguments, no invariant
    // beyond the ordinary FFI call.
    unsafe {
        let mut mods = keyboard_types::Modifiers::empty();
        if (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 {
            mods |= keyboard_types::Modifiers::SHIFT;
        }
        if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
            mods |= keyboard_types::Modifiers::CONTROL;
        }
        if (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
            mods |= keyboard_types::Modifiers::ALT;
        }
        if (GetKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
        {
            mods |= keyboard_types::Modifiers::META;
        }
        mods
    }
}

/// Drain the `WM_CHAR` burst `TranslateMessage` queued for the keydown
/// currently being handled, and decode it into typeable text.
///
/// Called from `window_proc`'s `WM_KEYDOWN`/`WM_SYSKEYDOWN` arm. The burst
/// is complete at that point — the message loop runs `TranslateMessage`
/// before `DispatchMessageW`, so every `WM_CHAR` belonging to this keystroke
/// (two of them for an astral-plane character) is already posted. Draining
/// filters exactly `WM_CHAR` for this window: `WM_SYSCHAR` is deliberately
/// left queued so Alt+mnemonic accelerators still flow to `DefWindowProcW`,
/// and `WM_DEADCHAR` is left to expire so dead-key state stays Windows'
/// business. Returns `None` for keystrokes with no typeable translation
/// (navigation keys, Ctrl chords — see `shared::keys::wm_char_text`).
fn drain_translated_chars(hwnd: HWND) -> Option<String> {
    let mut units: Vec<u16> = Vec::new();
    let mut msg = MSG::default();

    // SAFETY: `msg` is a live, writable local and `PeekMessageW` writes
    // nothing else. `PM_REMOVE` only ever removes messages from this
    // thread's own queue (the wndproc runs on the queue's owning thread).
    // Note `PeekMessageW` may deliver pending nonqueued (sent) messages,
    // re-entering `window_proc` — the same re-entrancy any modal Win32 API
    // call permits, and `window_proc` holds no lock across this call.
    unsafe {
        while PeekMessageW(&raw mut msg, Some(hwnd), WM_CHAR, WM_CHAR, PM_REMOVE).as_bool() {
            units.push(msg.wParam.0 as u16);
        }
    }

    crate::shared::keys::wm_char_text(&units)
}

// Windows platform capabilities
static WINDOWS_CAPABILITIES: DesktopCapabilities = DesktopCapabilities;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_creation() {
        let result = WindowsPlatform::new();
        assert!(
            result.is_ok(),
            "Failed to create Windows platform: {:?}",
            result.err()
        );
    }
}
