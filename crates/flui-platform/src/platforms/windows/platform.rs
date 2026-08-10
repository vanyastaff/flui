//! Windows platform implementation

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use anyhow::{Context, Result};
use flui_types::geometry::{Bounds, Point, Size};
use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{
            ERROR_CANCELLED, ERROR_SUCCESS, GetLastError, HWND, LPARAM, LRESULT, RECT,
            SetLastError, WPARAM,
        },
        Graphics::Gdi::{BeginPaint, EndPaint, HBRUSH, PAINTSTRUCT},
        System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            WindowsAndMessaging::{
                CS_HREDRAW, CS_OWNDC, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GWLP_USERDATA, GetForegroundWindow, GetMessageW,
                GetWindowLongPtrW, HICON, HTCLIENT, HWND_MESSAGE, IDC_ARROW, MSG, PostQuitMessage,
                RegisterClassW, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowPos, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
                WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE,
                WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS,
                WM_SETTINGCHANGE, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    display::enumerate_displays,
    util::{WINDOW_CLASS_NAME, get_x_lparam, get_y_lparam, hiword, load_cursor_style},
    window::{WindowCommand, WindowState, WindowsWindow},
};
use crate::{
    config::WindowConfiguration,
    data_transfer::{DataTransferSource, NullDataTransferSource},
    executor::BackgroundExecutor,
    shared::{PlatformHandlers, WindowCallbacks},
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
    pub(super) window_id: WindowId,
    /// Reference to platform handlers (global)
    pub(super) handlers: Arc<Mutex<PlatformHandlers>>, // PORT-CHECK-OK-SP6: WindowsPlatform handlers Arc<Mutex<>>; mirrors PlatformHandlers callback storage; pre-existing SP-6
    /// Per-window callbacks for event delivery
    pub(super) callbacks: Arc<WindowCallbacks>,
    /// State shared with `WindowsWindow`; locks are released before callbacks.
    pub(super) state: Arc<Mutex<WindowState>>,
    /// Window configuration (hotkeys, debouncing, etc.)
    pub(super) config: WindowConfiguration,
    /// Non-owning access to the platform registry for owner-thread teardown.
    pub(super) windows: Weak<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
}

static_assertions::assert_impl_all!(WindowContext: Send, Sync);

impl WindowContext {
    /// Dispatch a window event safely without holding locks
    ///
    /// This method extracts the handler, releases the lock, calls the handler,
    /// then re-acquires the lock to restore it. This prevents deadlocks when
    /// the handler tries to acquire the same lock.
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

/// Installs the raw strong reference owned by an HWND's userdata slot.
pub(super) fn install_window_context(hwnd: HWND, context: Arc<WindowContext>) -> Result<()> {
    // Win32 initializes a fresh HWND's userdata to zero, and this runs
    // on its owner thread before the window is published. Preflighting before
    // `Arc::into_raw` means an unexpected occupied slot is never overwritten
    // or later mistaken for FLUI's Arc. After the conversion, a successful
    // write transfers that one strong reference to the slot; its address is
    // obtained with `expose_provenance` because Win32 stores only an integer.
    // A failed write reconstructs and drops the original pointer directly.
    // SAFETY: these inseparable calls inspect only `hwnd`'s pointer-sized
    // userdata value. `SetLastError` disambiguates a successful zero read.
    let (existing, preflight_error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let existing = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        (existing, GetLastError())
    };
    if existing != 0 {
        anyhow::bail!("refusing to overwrite occupied Win32 userdata slot");
    }
    if preflight_error != ERROR_SUCCESS {
        anyhow::bail!(
            "failed to inspect Win32 userdata slot: error={}",
            preflight_error.0
        );
    }

    let context_ptr = Arc::into_raw(context);
    let context_address = context_ptr.expose_provenance().cast_signed();
    // SAFETY: `context_address` is the exposed address of the raw Arc strong
    // reference above. These calls atomically install that integer value and
    // immediately capture the thread-local error needed to interpret zero.
    let (previous, install_error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let previous = SetWindowLongPtrW(hwnd, GWLP_USERDATA, context_address);
        (previous, GetLastError())
    };
    if previous != 0 {
        // A nonzero previous value means the write succeeded despite the
        // owner-serialized empty preflight. The slot now owns our Arc;
        // the caller's error cleanup destroys the HWND and consumes it.
        anyhow::bail!("Win32 userdata slot changed after empty preflight: previous={previous}");
    }
    if install_error != ERROR_SUCCESS {
        // SAFETY: the failed slot write did not consume the raw strong
        // reference produced immediately above.
        drop(unsafe { Arc::from_raw(context_ptr) });
        anyhow::bail!(
            "failed to install Win32 window context: error={}",
            install_error.0
        );
    }
    Ok(())
}

/// Pins the context for one WNDPROC invocation.
///
/// # Safety
///
/// The caller must be executing serialized WNDPROC dispatch on `hwnd`'s owner
/// thread. Every nonzero userdata value must be the exposed address of the raw
/// `Arc<WindowContext>` strong reference installed by `install_window_context`,
/// and no other thread may clear or replace the slot during this call.
unsafe fn acquire_window_context(hwnd: HWND) -> Option<Arc<WindowContext>> {
    // Only this module reads or clears this slot, and Win32 serializes
    // WNDPROC dispatch for an HWND on its owner thread. A non-null pointer is
    // the raw strong reference installed by `install_window_context`; it
    // remains owned by the slot while the increment is performed. The address
    // is reconstituted with `with_exposed_provenance` before any Arc operation.
    // The new strong reference pins the allocation across reentrant callbacks,
    // including a nested `WM_DESTROY` that clears the slot.
    // SAFETY: the caller's contract guarantees owner-thread serialization;
    // this reads the pointer-sized userdata value without dereferencing it.
    let context_address = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if context_address == 0 {
        return None;
    }
    let context_ptr =
        std::ptr::with_exposed_provenance::<WindowContext>(context_address.cast_unsigned());
    // SAFETY: the nonzero address is the slot-owned raw Arc strong reference.
    // The increment happens while that reference is still slot-owned, and
    // `from_raw` consumes exactly the newly added invocation-local reference.
    unsafe {
        Arc::increment_strong_count(context_ptr);
        Some(Arc::from_raw(context_ptr))
    }
}

/// Clears userdata and consumes the strong reference formerly owned by it.
///
/// # Safety
///
/// The caller must be executing serialized WNDPROC dispatch on `hwnd`'s owner
/// thread, and must be the sole code allowed to clear the slot. Every nonzero
/// value must be the exposed address installed by `install_window_context` and
/// must not have been previously consumed with `Arc::from_raw`.
unsafe fn take_window_context(hwnd: HWND) -> Result<Option<Arc<WindowContext>>> {
    // Owner-thread WNDPROC dispatch is the sole clearer of this slot.
    // A nonzero return is exactly the raw strong reference installed by
    // `install_window_context`; `with_exposed_provenance` reconstitutes the
    // pointer before `Arc::from_raw` consumes it once. On a failed zero return
    // the slot may still own the reference, so it is left untouched and
    // reported rather than guessed-at or double-consumed.
    // SAFETY: the caller's contract grants sole owner-thread clearing access.
    // These inseparable calls clear the pointer-sized value and immediately
    // capture the thread-local error needed to interpret a zero return.
    let (context_address, error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let context_address = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        (context_address, GetLastError())
    };
    if context_address != 0 {
        let context_ptr =
            std::ptr::with_exposed_provenance::<WindowContext>(context_address.cast_unsigned());
        // SAFETY: by contract this is the unique raw strong reference just
        // removed from the slot, reconstructed with exposed provenance.
        return Ok(Some(unsafe { Arc::from_raw(context_ptr) }));
    }
    if error == ERROR_SUCCESS {
        Ok(None)
    } else {
        anyhow::bail!("failed to clear Win32 window context: error={}", error.0)
    }
}

fn default_window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: this forwards the exact HWND and message parameters supplied by
    // Win32 to the registered WNDPROC; no Rust reference is derived from them.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
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
// thread-affinity obligation, which these impls do not discharge. See the
// event-loop affinity gap in `docs/audits/2026-07-25-upgrade-pack-audit.md`.
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

    /// Main window procedure for all FLUI windows
    ///
    /// # Safety
    ///
    /// Called by Win32 as a `WNDPROC` for windows of `WINDOW_CLASS_NAME`,
    /// registered as this exact function in `register_window_class`. Win32
    /// guarantees `hwnd`/`msg`/`wparam`/`lparam` are well-formed for the
    /// message being delivered, and always dispatches on the thread that
    /// owns the window's message queue. `acquire_window_context` turns the
    /// slot-owned raw Arc reference into an invocation-local strong reference
    /// before any callback can reentrantly destroy the HWND.
    ///
    /// Known gap (not fixed by this comment, not UB — flagged for the
    /// audit): none of the callback dispatches below (`ctx.callbacks.*`,
    /// `ctx.dispatch_event`) are wrapped in `catch_unwind`. A panic inside a
    /// framework-supplied callback unwinds into this `extern "system"`
    /// frame; stable Rust aborts the process rather than invoking UB when
    /// that happens (FFI boundaries are implicitly `nounwind`), but that is
    /// still whole-process termination from a single window's callback,
    /// unlike the `winit` backend's `window_proc`-equivalent path, which
    /// does guard its callback boundary with `catch_unwind`.
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Ordinary scope only: unsafe authority is granted separately at each
        // FFI/raw-Arc operation below, never to the dispatch state machine.
        {
            // SAFETY: WNDPROC runs on the HWND owner thread. The slot-owned Arc is
            // live until the only clearing path, WM_DESTROY, so the helper can pin
            // an invocation-local reference before any callback runs.
            let ctx = unsafe { acquire_window_context(hwnd) };

            if let Some(command) = WindowCommand::from_message(msg) {
                if let Some(ctx) = ctx.as_deref() {
                    WindowsWindow::execute_window_command(hwnd, ctx, command);
                }
                return LRESULT(0);
            }

            match msg {
                WM_CREATE => {
                    tracing::debug!("WM_CREATE for HWND {:?}", hwnd);
                    LRESULT(0)
                }

                WM_CLOSE => {
                    tracing::debug!("WM_CLOSE for HWND {:?}", hwnd);

                    if let Some(ctx) = ctx.as_deref() {
                        // Ask per-window callback if close should proceed
                        let should_close = ctx.callbacks.dispatch_should_close();

                        if should_close {
                            // Dispatch CloseRequested to global handlers
                            ctx.dispatch_event(WindowEvent::CloseRequested {
                                window_id: ctx.window_id,
                            });
                            // SAFETY: WNDPROC runs on this HWND's owner thread.
                            if let Err(error) = unsafe { DestroyWindow(hwnd) } {
                                tracing::warn!(?hwnd, ?error, "DestroyWindow failed on WM_CLOSE");
                            }
                        }
                        // If !should_close, the close is vetoed
                    } else {
                        // SAFETY: WNDPROC runs on this HWND's owner thread even
                        // before or after a context is installed.
                        if let Err(error) = unsafe { DestroyWindow(hwnd) } {
                            tracing::warn!(
                                ?hwnd,
                                ?error,
                                "DestroyWindow failed on WM_CLOSE (no WindowContext)"
                            );
                        }
                    }

                    LRESULT(0)
                }

                WM_DESTROY => {
                    tracing::debug!("WM_DESTROY for HWND {:?}", hwnd);

                    if let Some(ctx) = ctx.as_deref() {
                        let should_dispatch_close = {
                            let mut state = ctx.state.lock();
                            let was_destroyed = state.is_destroyed;
                            state.is_destroyed = true;
                            state.visible = false;
                            state.focused = false;
                            state.is_hovered = false;
                            !was_destroyed
                        };

                        if should_dispatch_close {
                            // Fire per-window on_close callback (FnOnce)
                            ctx.callbacks.dispatch_close();

                            // Dispatch Closed event to global handlers
                            ctx.dispatch_event(WindowEvent::Closed(ctx.window_id));

                            if let Some(windows) = ctx.windows.upgrade() {
                                windows.lock().remove(&(hwnd.0 as isize));
                            }
                        }
                    }

                    // SAFETY: this owner-thread WNDPROC is the only userdata
                    // clearer. The invocation-local `ctx: Arc<_>` above stays
                    // alive until this function returns, so consuming the
                    // slot-owned strong reference cannot invalidate callback
                    // stack frames, including after reentrant destruction.
                    if let Err(error) = unsafe { take_window_context(hwnd) } {
                        tracing::warn!(?hwnd, ?error, "failed to release HWND userdata context");
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
                    let hdc = unsafe { BeginPaint(hwnd, &raw mut ps) };
                    if !hdc.is_invalid() {
                        // Skip rendering for minimized windows to save CPU/GPU resources
                        let should_skip = ctx
                            .as_deref()
                            .is_some_and(WindowsWindow::should_skip_render);
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
                            if let Some(ctx) = ctx.as_deref() {
                                // Fire per-window on_request_frame callback
                                ctx.callbacks.dispatch_request_frame();

                                // Also dispatch RedrawRequested to global handlers
                                ctx.dispatch_event(WindowEvent::RedrawRequested {
                                    window_id: ctx.window_id,
                                });
                            }
                        }
                        let _ = unsafe { EndPaint(hwnd, &raw const ps) };
                    }
                    LRESULT(0)
                }

                WM_SIZE => {
                    use super::util::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};

                    let width = get_x_lparam(lparam).max(1);
                    let height = get_y_lparam(lparam).max(1);
                    let size_type = wparam.0 as u32;

                    if let Some(ctx) = ctx.as_deref() {
                        use flui_types::geometry::DevicePixels;
                        let size = Size::new(DevicePixels(width), DevicePixels(height));
                        let (prev_mode, last_size, scale_factor) = {
                            let state = ctx.state.lock();
                            (state.mode, state.last_size, state.scale_factor)
                        };

                        // Handle state transition and dispatch appropriate event
                        let (new_mode, event) = match size_type {
                            SIZE_MINIMIZED => {
                                tracing::info!("📦 Window Minimized");
                                // Validate transition
                                let candidate = WindowMode::Minimized {
                                    previous: Bounds {
                                        origin: Point::new(DevicePixels(0), DevicePixels(0)),
                                        size: last_size,
                                    },
                                };
                                if prev_mode.can_transition_to(&candidate) {
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
                                        size: last_size,
                                    },
                                };
                                if prev_mode.can_transition_to(&candidate) {
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
                                (
                                    prev_mode,
                                    Some(WindowEvent::Resized {
                                        window_id: ctx.window_id,
                                        size,
                                    }),
                                )
                            }
                        };

                        // Update cached state under one short lock, then release it before
                        // any callback can synchronously re-enter WNDPROC.
                        {
                            let mut state = ctx.state.lock();
                            state.mode = new_mode;
                            if size_type != SIZE_MINIMIZED || !prev_mode.is_minimized() {
                                state.last_size = size;
                            }
                            if size_type != SIZE_MINIMIZED {
                                state.bounds.size = Size::new(
                                    flui_types::geometry::px(super::util::device_to_logical(
                                        width,
                                        scale_factor,
                                    )),
                                    flui_types::geometry::px(super::util::device_to_logical(
                                        height,
                                        scale_factor,
                                    )),
                                );
                            }
                        }

                        // Fire per-window on_resize callback (for all size changes except minimize)
                        if size_type != SIZE_MINIMIZED {
                            let logical_size = Size::new(
                                flui_types::geometry::px(super::util::device_to_logical(
                                    width,
                                    scale_factor,
                                )),
                                flui_types::geometry::px(super::util::device_to_logical(
                                    height,
                                    scale_factor,
                                )),
                            );
                            ctx.callbacks.dispatch_resize(logical_size, scale_factor);
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

                WM_MOVE => {
                    let x = get_x_lparam(lparam);
                    let y = get_y_lparam(lparam);
                    tracing::debug!("Window Moved: ({}, {})", x, y);

                    if let Some(ctx) = ctx.as_deref() {
                        let scale_factor = {
                            let mut state = ctx.state.lock();
                            state.bounds.origin = Point::new(
                                flui_types::geometry::px(x as f32 / state.scale_factor),
                                flui_types::geometry::px(y as f32 / state.scale_factor),
                            );
                            state.scale_factor
                        };
                        // Fire per-window on_moved callback
                        ctx.callbacks.dispatch_moved();

                        // Dispatch Moved event to global handlers
                        use flui_types::geometry::{Point, px};
                        let position =
                            Point::new(px(x as f32 / scale_factor), px(y as f32 / scale_factor));
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
                    if let Some(ctx) = ctx.as_deref() {
                        ctx.state.lock().scale_factor = new_scale;
                        ctx.dispatch_event(WindowEvent::ScaleFactorChanged {
                            window_id: ctx.window_id,
                            scale_factor: new_scale as f64,
                        });

                        // Suggested rect for new DPI
                        //
                        // SAFETY: `lparam` carries a pointer to a `RECT`
                        // supplied by Win32 for `WM_DPICHANGED` specifically
                        // (documented behavior of this message); the
                        // null-check guards a caller that violates that
                        // documented contract, and `rect` is copied out
                        // (`RECT: Copy`) rather than referenced further.
                        let suggested_rect =
                            std::ptr::with_exposed_provenance::<RECT>(lparam.0.cast_unsigned());
                        if !suggested_rect.is_null() {
                            let rect = unsafe { *suggested_rect };
                            let reposition_result = unsafe {
                                SetWindowPos(
                                    hwnd,
                                    None,
                                    rect.left,
                                    rect.top,
                                    rect.right - rect.left,
                                    rect.bottom - rect.top,
                                    SWP_NOZORDER | SWP_NOACTIVATE,
                                )
                            };
                            if let Err(error) = reposition_result {
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
                    if let Some(ctx) = ctx.as_deref() {
                        // Request WM_MOUSELEAVE notification for hover tracking
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        // SAFETY: `tme` is initialized with its exact size and
                        // the current live HWND; Win32 does not retain it.
                        let _ = unsafe { TrackMouseEvent(&raw mut tme) };

                        // Track hover state (T034)
                        let scale_factor = {
                            let mut state = ctx.state.lock();
                            state.is_hovered = true;
                            state.scale_factor
                        };

                        // Dispatch hover enter (will be cleared on WM_MOUSELEAVE)
                        ctx.callbacks.dispatch_hover_status_change(true);

                        use super::events::mouse_move_event;
                        let event = mouse_move_event(lparam, scale_factor);
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_SETCURSOR => {
                    if let Some(ctx) = ctx.as_deref()
                        && (lparam.0 as u32 & 0xffff) == HTCLIENT
                    {
                        let cursor = ctx.state.lock().cursor;
                        match WindowsWindow::apply_native_cursor(cursor) {
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
                    default_window_proc(hwnd, msg, wparam, lparam)
                }

                WM_LBUTTONDOWN => {
                    if let Some(ctx) = ctx.as_deref() {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Primary,
                            true,
                            lparam,
                            ctx.state.lock().scale_factor,
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_RBUTTONDOWN => {
                    if let Some(ctx) = ctx.as_deref() {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Secondary,
                            true,
                            lparam,
                            ctx.state.lock().scale_factor,
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MBUTTONDOWN => {
                    if let Some(ctx) = ctx.as_deref() {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Auxiliary,
                            true,
                            lparam,
                            ctx.state.lock().scale_factor,
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_LBUTTONUP => {
                    if let Some(ctx) = ctx.as_deref() {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Primary,
                            false,
                            lparam,
                            ctx.state.lock().scale_factor,
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_RBUTTONUP => {
                    if let Some(ctx) = ctx.as_deref() {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Secondary,
                            false,
                            lparam,
                            ctx.state.lock().scale_factor,
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MBUTTONUP => {
                    if let Some(ctx) = ctx.as_deref() {
                        use ui_events::pointer::PointerButton;

                        use super::events::mouse_button_event;
                        let event = mouse_button_event(
                            PointerButton::Auxiliary,
                            false,
                            lparam,
                            ctx.state.lock().scale_factor,
                        );
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_MOUSEWHEEL => {
                    if let Some(ctx) = ctx.as_deref() {
                        use super::events::mouse_wheel_event;
                        let event =
                            mouse_wheel_event(wparam, lparam, ctx.state.lock().scale_factor);
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let vk = wparam.0 as u16;
                    let is_repeat = (lparam.0 & (1 << 30)) != 0;

                    // Check if fullscreen hotkey is pressed (configurable, default F11)
                    if let Some(ctx) = ctx.as_deref() {
                        if let Some(hotkey) = ctx.config.fullscreen_hotkey
                            && vk == hotkey
                            && !is_repeat
                        {
                            tracing::info!(
                                "Fullscreen hotkey (VK={:#04x}) pressed - toggling fullscreen",
                                hotkey
                            );
                            let enter_fullscreen = !ctx.state.lock().mode.is_fullscreen();
                            WindowsWindow::set_fullscreen_for_context(hwnd, ctx, enter_fullscreen);
                        }

                        // Track modifiers (T035)
                        ctx.state.lock().modifiers = current_modifiers();

                        // Dispatch keyboard event via per-window callback
                        use super::events::key_down_event;
                        let event = key_down_event(wparam, lparam);
                        ctx.callbacks.dispatch_input(event);
                    }

                    LRESULT(0)
                }

                WM_KEYUP | WM_SYSKEYUP => {
                    if let Some(ctx) = ctx.as_deref() {
                        // Track modifiers (T035)
                        ctx.state.lock().modifiers = current_modifiers();

                        use super::events::key_up_event;
                        let event = key_up_event(wparam, lparam);
                        ctx.callbacks.dispatch_input(event);
                    }
                    LRESULT(0)
                }

                WM_CHAR => {
                    // WM_CHAR is handled by the framework via KeyboardEvent
                    // No per-window callback dispatch needed here
                    LRESULT(0)
                }

                WM_SETFOCUS => {
                    tracing::debug!("Window Focused");

                    if let Some(ctx) = ctx.as_deref() {
                        ctx.state.lock().focused = true;
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

                    if let Some(ctx) = ctx.as_deref() {
                        ctx.state.lock().focused = false;
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
                    if let Some(ctx) = ctx.as_deref() {
                        // Track hover state (T034)
                        ctx.state.lock().is_hovered = false;

                        ctx.callbacks.dispatch_hover_status_change(false);
                    }
                    LRESULT(0)
                }

                // T026: System theme/appearance change
                WM_SETTINGCHANGE => {
                    if let Some(ctx) = ctx.as_deref() {
                        ctx.callbacks.dispatch_appearance_changed();
                    }
                    default_window_proc(hwnd, msg, wparam, lparam)
                }

                // T046: Keyboard layout change
                WM_INPUTLANGCHANGE => {
                    if let Some(ctx) = ctx.as_deref() {
                        // Dispatch keyboard layout change via take/restore pattern
                        let handler = ctx.handlers.lock().keyboard_layout_changed.take();
                        if let Some(mut handler) = handler {
                            handler();
                            ctx.handlers.lock().keyboard_layout_changed = Some(handler);
                        }
                    }
                    default_window_proc(hwnd, msg, wparam, lparam)
                }

                _ => default_window_proc(hwnd, msg, wparam, lparam),
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
