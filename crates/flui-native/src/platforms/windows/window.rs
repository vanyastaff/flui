//! Windows window implementation

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, EdgeInsets, Point, Size, WindowMetrics};
use parking_lot::Mutex;
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, Win32WindowHandle,
    WindowsDisplayHandle,
};
use windows::{
    Win32::{
        Foundation::{FALSE, HWND, LPARAM, POINT, RECT, TRUE, WPARAM},
        Graphics::Gdi::{ClientToScreen, HRGN, InvalidateRect, ScreenToClient, UpdateWindow},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForSystem, GetDpiForWindow},
            WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, GCLP_HBRBACKGROUND, GWL_EXSTYLE,
                GWL_STYLE, GWLP_USERDATA, GetClientRect, GetCursorPos, GetForegroundWindow,
                GetWindowLongPtrW, GetWindowRect, IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND,
                IDC_IBEAM, IDC_NO, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE,
                IDC_WAIT, IsIconic, IsWindowVisible, IsZoomed, LoadCursorW, PostMessageW,
                SC_RESTORE, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
                SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetClassLongPtrW, SetCursor,
                SetForegroundWindow, SetWindowLongPtrW, SetWindowPlacement, SetWindowPos,
                SetWindowTextW, ShowWindow, WM_APP, WM_CLOSE, WM_SYSCOMMAND, WS_CAPTION,
                WS_EX_APPWINDOW, WS_MAXIMIZE, WS_MAXIMIZEBOX, WS_MINIMIZE, WS_THICKFRAME,
            },
        },
    },
    core::HSTRING,
};

use super::platform::{WindowRef, with_window_context_checked};
use super::window_frame::{
    FirstShow, first_show_command, frame_style, restyled, to_screen, to_workspace,
};
use super::window_placement::{
    USER_DEFAULT_SCREEN_DPI, checked_logical_to_device, logical_to_device, open_spot, outer_size,
    placement_of, restore_rectangle, workspace_origin,
};
use crate::owner::windows::{WINDOW_CLASS_NAME, WindowsLoop};

/// Posted to a window nothing paints, so its frame still runs: a hidden window
/// never receives `WM_PAINT`.
pub(super) const REQUEST_FRAME: u32 = WM_APP + 1;

/// Posted by the fullscreen hotkey once the app let its key through.
pub(super) const TOGGLE_FULLSCREEN: u32 = WM_APP + 3;
use crate::shared::window_state::WindowStateCell;
use crate::{
    owner::OpenWindowError, owner::call_point::install, owner::handlers::PlatformHandlers,
    owner::handlers::WindowCallbacks, shared::hwnd_affinity::UserDataRefusal,
};
use flui_platform::{
    CursorError, DispatchEventResult, PlatformInput, PlatformWindow, SizeConstraints,
    WindowAppearance, WindowBackgroundAppearance, WindowCapabilities, WindowCommand,
    WindowCommandError, WindowExecutionState, WindowGeometry, WindowId, WindowMode, WindowOptions,
    WindowReveal, WindowState,
};

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Arc<Mutex<WindowRecord>>,

    /// Which native window this wrapper was built for. The callbacks
    /// themselves live in that window's owner-thread context, not here;
    /// see [`Self::register`].
    pub(crate) identity: crate::owner::windows::WindowIdentity,

    /// Reference to platform's window map (for cleanup)
    windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,

    /// The platform's monitors, by which this window names its display.
    displays: Arc<super::display::WindowsDisplays>,

    /// The published window state (ADR-0237), shared with the window's
    /// owner-thread context, which refreshes it from the window messages.
    snapshot: Arc<WindowStateCell>,

    /// The context allocation this wrapper installed in `hwnd`'s
    /// `GWLP_USERDATA` slot at creation, kept ONLY as an identity token:
    /// teardown compares it (as a value, never dereferencing through this
    /// field) against the window's current slot so a wrapper that outlived
    /// its native window cannot destroy or close whatever unrelated window
    /// the OS recycled the handle value for — see
    /// [`teardown_route`](super::platform::teardown_route). Dangling as a
    /// VALUE once `WM_DESTROY` has run; that is fine for comparison and
    /// exactly why it must never be dereferenced here.
    context: *const super::platform::WindowContext,

    /// This window's UIA bridge, subclassed onto `hwnd` at construction so
    /// a UI Automation client that asks at any later point is answered.
    /// Inert until one does — see
    /// [`WindowsAccessibility::new`](super::accessibility::WindowsAccessibility::new).
    #[cfg(feature = "a11y")]
    accessibility: Arc<super::accessibility::WindowsAccessibility>,

    /// This window's haptic actuator: the pen in range of it.
    haptics: Arc<super::haptics::WindowsHaptics>,

    /// Starts this window's OS drag sessions through OLE (ADR-0211).
    drag_source: Arc<super::drag::Win32DragSource>,
}

// SAFETY, per field: `state` is an `Arc<Mutex<..>>` with its own
// synchronization; `windows_map` is likewise an `Arc<Mutex<..>>`;
// `haptics` and `drag_source` are `Send + Sync` themselves;
// `identity` is a plain integer. The struct carries no callback storage:
// callbacks live in the owner-thread `WindowContext` and are installed only
// through the context gate below, so a foreign thread can neither run nor
// drop one. The non-`Sync`/non-`Send` members are `hwnd: HWND` and
// `context: *const WindowContext`, both bare addresses that this type never
// dereferences through these fields (`context` is an identity token for
// value comparison only — see its field doc) — sending or sharing an
// address itself aliases nothing, so `Send` and `Sync` are sound for the
// struct.
//
// The HWND behind that address is thread-AFFINE, not "thread-safe by
// design": its message queue belongs to the thread that created it, Win32
// dispatches `window_proc` (including the `WM_DESTROY` arm that retires the
// `WindowContext` stored in `GWLP_USERDATA`) on that thread, and
// `DestroyWindow` refuses to run anywhere else. That obligation is
// DISCHARGED — not merely documented — by routing every affine touch on
// this type through two gates in `platform.rs`:
//
// - every dereference of the `GWLP_USERDATA` context goes through
//   `with_window_context`, which refuses (safe fallback, traced) unless the
//   calling thread is the window's owning thread and the window is live, of
//   our class, with a non-null slot — on the owning thread the deref cannot
//   race the free: the only retiring path (`WM_DESTROY`) is dispatched on
//   that same thread, and the borrow-ledger (`ContextGuard`/`ContextLedger`)
//   defers the actual free past every live borrow, including across
//   reentrant dispatch;
// - every teardown (`close()`, the last wrapper `Drop`) goes through
//   `teardown_route`, which verifies the handle still names THIS wrapper's
//   window (class + context identity, guarding against OS handle
//   recycling), calls `DestroyWindow` only on the owning thread, and
//   otherwise posts `WM_CLOSE` to the owner's queue (`PostMessageW`, the
//   documented cross-thread mechanism).
//
// The decision rules are pure and Linux-tested (`shared::hwnd_affinity`);
// the remaining Win32 calls on `&self` (`ShowWindow`, `SetWindowPos`,
// `SetWindowTextW`, DWM attributes, ...) are documented by Win32 as legal
// cross-thread and dereference nothing.
unsafe impl Send for WindowsWindow {}
// SAFETY: see `Send` above — shared access adds nothing beyond the same
// gated paths.
unsafe impl Sync for WindowsWindow {}

/// Mutable window state
pub(super) struct WindowRecord {
    /// Current window bounds (logical pixels)
    pub(super) bounds: Bounds<f64>,

    /// Current scale factor (DPI / 96)
    pub(super) scale_factor: f64,

    /// Is window visible?
    pub(super) visible: bool,

    /// Is window focused?
    pub(super) focused: bool,
}

/// How the window occupies its monitor, with what the way back needs: the
/// native tracking behind the public [`WindowMode`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum NativeMode {
    #[default]
    Normal,
    /// Iconified. A fullscreen window that was minimized keeps the rectangle it
    /// returns to, so restoring it brings fullscreen back.
    Minimized {
        from_fullscreen: Option<Bounds<i32>>,
    },
    /// Maximized.
    Maximized,
    /// Borderless over the whole monitor; `restore_bounds` is the outer
    /// rectangle in device pixels the window returns to.
    Fullscreen { restore_bounds: Bounds<i32> },
}

impl NativeMode {
    pub(super) fn is_fullscreen(&self) -> bool {
        matches!(self, Self::Fullscreen { .. })
    }

    /// The rectangle a fullscreen window returns to, also while it is minimized
    /// (a minimized fullscreen window is still the fullscreen window it comes
    /// back as); `None` for every other mode.
    pub(super) fn fullscreen_bounds(&self) -> Option<Bounds<i32>> {
        match *self {
            Self::Fullscreen { restore_bounds }
            | Self::Minimized {
                from_fullscreen: Some(restore_bounds),
            } => Some(restore_bounds),
            _ => None,
        }
    }

    /// Whether this is fullscreen, or minimized out of fullscreen.
    pub(super) fn holds_fullscreen(&self) -> bool {
        self.fullscreen_bounds().is_some()
    }

    /// The same mode with the rectangle a fullscreen window returns to
    /// replaced; any other mode is returned as it is.
    pub(super) fn with_fullscreen_bounds(self, bounds: Bounds<i32>) -> Self {
        match self {
            Self::Fullscreen { .. } => Self::Fullscreen {
                restore_bounds: bounds,
            },
            Self::Minimized {
                from_fullscreen: Some(_),
            } => Self::Minimized {
                from_fullscreen: Some(bounds),
            },
            other => other,
        }
    }

    pub(crate) fn is_minimized(&self) -> bool {
        matches!(self, Self::Minimized { .. })
    }

    pub(super) fn is_maximized(&self) -> bool {
        matches!(self, Self::Maximized)
    }

    pub(super) fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }

    /// Every transition is allowed except to the same mode.
    pub(super) fn can_transition_to(&self, next: &Self) -> bool {
        std::mem::discriminant(self) != std::mem::discriminant(next)
    }

    fn public(&self) -> WindowMode {
        match self {
            Self::Normal => WindowMode::Normal,
            Self::Minimized { .. } => WindowMode::Minimized,
            Self::Maximized => WindowMode::Maximized,
            Self::Fullscreen { .. } => WindowMode::Fullscreen,
        }
    }
}

/// What the owner context knows about a window that Win32 does not say: the
/// mode it tracks from `WM_SIZE` and its own fullscreen toggle, the style a
/// fullscreen window returns to, the limits the app set and the display the
/// window is on.
#[derive(Debug, Clone, Copy)]
pub(super) struct OwnerTracking {
    pub(super) mode: NativeMode,
    pub(super) restore_style: u32,
    pub(super) constraints: SizeConstraints,
    pub(super) display: Option<flui_platform::DisplayId>,
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
pub(super) fn sample_state(
    hwnd: HWND,
    record: &Mutex<WindowRecord>,
    tracking: OwnerTracking,
) -> WindowState {
    let OwnerTracking {
        mode,
        restore_style,
        constraints,
        display,
    } = tracking;
    let (scale, visible, focused, cached) = {
        let record = record.lock();
        (
            record.scale_factor,
            record.visible,
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
    WindowState::new(outer, if mode.is_minimized() { cached } else { content })
        .with_mode(mode.public())
        .with_normal_geometry(normal)
        .with_visible(visible)
        .with_focused(focused)
        .with_title(window_text(hwnd))
        .with_constraints(constraints)
        .with_resizable(style & WS_THICKFRAME.0 != 0)
        .with_decorated(style & WS_CAPTION.0 == WS_CAPTION.0)
        .with_display(display)
}

impl std::fmt::Debug for WindowsWindow {
    // Hand-written: `WindowCallbacks` is a callback payload with no meaningful
    // Debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsWindow")
            .field("hwnd", &self.hwnd.0)
            .finish_non_exhaustive()
    }
}

/// A Win32 call that failed, as the error of the command that made it.
fn backend_error(what: &str, error: windows::core::Error) -> WindowCommandError {
    WindowCommandError::Backend {
        message: format!("{what} failed: {error}"),
    }
}

/// A command's logical pair in device pixels, refused as invalid geometry
/// when either value is beyond what Win32 can represent.
fn device_pair(x: f64, y: f64, scale: f64) -> Result<(i32, i32), WindowCommandError> {
    checked_logical_to_device(x, scale)
        .zip(checked_logical_to_device(y, scale))
        .ok_or(WindowCommandError::InvalidGeometry)
}

/// What every Win32 window honours and observes, known before one exists
/// (`Platform::window_capabilities`).
pub(crate) const fn window_capabilities() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_title = true;
    capabilities.sets_position = true;
    capabilities.sets_size = true;
    capabilities.sets_constraints = true;
    capabilities.sets_resizable = true;
    capabilities.sets_decorated = true;
    capabilities.minimizes = true;
    capabilities.maximizes = true;
    capabilities.enters_fullscreen = true;
    capabilities.shows = true;
    capabilities.hides = true;
    capabilities.activates = true;
    capabilities.observes_position = true;
    capabilities.observes_minimized = true;
    capabilities.places_at_open = true;
    capabilities
}

impl WindowsWindow {
    /// Create a new Windows window. Crate-internal: windows are opened
    /// through [`Window::open`](crate::host::Window::open), which refuses a
    /// call off the owner thread and hands over the platform's own handler
    /// set.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when Win32 window creation fails.
    pub(super) fn new(
        options: WindowOptions,
        windows_map: Arc<super::platform::WindowMap>,
        shares: crate::owner::windows::control::OwnerShares,
        config: crate::config::WindowConfiguration,
        apartment: Option<Rc<crate::owner::windows::com_apartment::ComApartment>>,
        drag: Arc<super::drag::OleTransfer>,
    ) -> Result<Arc<Self>, OpenWindowError> {
        let crate::owner::windows::control::OwnerShares {
            handlers,
            frames,
            exit_policy,
            displays,
            notices,
            window_hooks,
            ..
        } = shares;
        // Admission refuses identity exhaustion before creating an HWND
        // whose ownership has not yet transferred to a context and wrapper.
        let (identity, created) = crate::owner::windows::WindowIdentity::admit(
            crate::owner::windows::WindowIdentity::source(),
            || Self::create_native(&options, &displays),
        );
        let (hwnd, requested, scale_factor, normal) = created?;
        Self::install_native(
            options,
            windows_map,
            handlers,
            frames,
            exit_policy,
            displays,
            notices,
            config,
            identity,
            hwnd,
            requested,
            scale_factor,
            normal,
            apartment,
            drag,
            window_hooks,
        )
    }

    /// Create the native window: the resource an admitted identity owns.
    /// Returns the HWND with the requested outer device size, the scale factor
    /// the sizes were computed at and, for a window that opens maximized or
    /// fullscreen, the rectangle it returns to (screen coordinates).
    fn create_native(
        options: &WindowOptions,
        displays: &super::display::WindowsDisplays,
    ) -> Result<(HWND, Size<i32>, f64, Option<RECT>), OpenWindowError> {
        // SAFETY: `GetModuleHandleW(None)` queries the current process image
        // and takes no pointer arguments — always sound. `GetDpiForSystem`
        // reads global state, no preconditions. `CreateWindowExW` requires
        // `WINDOW_CLASS_NAME` to already be registered, which
        // `WindowsLoop::with_config` guarantees before any `open_window`
        // call can reach here; `&title` is a live `HSTRING` owned by this
        // frame. `hwnd.is_invalid()` is checked before the handle is returned.
        unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| OpenWindowError::Backend {
                message: format!("Failed to get module handle: {e}"),
            })?;

            let system_scale = GetDpiForSystem() as f64 / USER_DEFAULT_SCREEN_DPI as f64;

            // Determine window style
            // Always created hidden: the window is placed and put in its mode
            // first, and shown afterwards, so its first sight is the final one.
            let style = windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(frame_style(
                options.decorated,
                options.resizable,
            ));

            let ex_style = WS_EX_APPWINDOW;

            // The options' size is the content size: ask Win32 for the outer
            // window that holds it at the target display's DPI, and place it
            // on the display the options name.
            let snapshot = displays.snapshot().ok();
            let target = options
                .display
                .and_then(|id| snapshot.as_ref().and_then(|snapshot| snapshot.get(id)));
            let spot = open_spot(options, target, system_scale, |dpi, content| {
                outer_size(style.0, ex_style.0, dpi, content).unwrap_or(content)
            });
            let (width, height) = spot.outer;
            let scale_factor = spot.scale;

            let title = HSTRING::from(&options.title);
            let hwnd = CreateWindowExW(
                ex_style,
                WINDOW_CLASS_NAME,
                &title,
                style,
                spot.origin.map_or(CW_USEDEFAULT, |origin| origin.0),
                spot.origin.map_or(CW_USEDEFAULT, |origin| origin.1),
                width,
                height,
                None, // parent
                None, // menu
                Some(hinstance.into()),
                None, // lpParam
            )
            .map_err(|e| OpenWindowError::Backend {
                message: format!("Failed to create window: {e}"),
            })?;

            if hwnd.is_invalid() {
                return Err(OpenWindowError::Backend {
                    message: windows::core::Error::from_thread().to_string(),
                });
            }
            let normal = spot.normal.map(|(left, top, right, bottom)| RECT {
                left,
                top,
                right,
                bottom,
            });
            Ok((hwnd, Size::new(width, height), scale_factor, normal))
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the admitted identity and the native window it owns, handed over together"
    )]
    fn install_native(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<isize, Arc<WindowsWindow>>>>,
        handlers: Rc<RefCell<PlatformHandlers>>,
        frames: Rc<crate::owner::windows::FrameCount>,
        exit_policy: crate::owner::windows::control::ExitPolicyRequest,
        displays: Arc<super::display::WindowsDisplays>,
        notices: Arc<crate::shared::system_notice::SystemNotices>,
        config: crate::config::WindowConfiguration,
        identity: crate::owner::windows::WindowIdentity,
        hwnd: HWND,
        requested: Size<i32>,
        scale_factor: f64,
        normal: Option<RECT>,
        apartment: Option<Rc<crate::owner::windows::com_apartment::ComApartment>>,
        drag: Arc<super::drag::OleTransfer>,
        window_hooks: crate::owner::windows::WindowHooks,
    ) -> Result<Arc<Self>, OpenWindowError> {
        let (width, height) = (requested.width, requested.height);
        // SAFETY: `hwnd` is the valid handle `create_native` just returned
        // on this thread. `SetClassLongPtrW` and `apply_windows_features`
        // operate on that same freshly created, still-valid `hwnd`.
        // `Box::into_raw(context)` intentionally leaks the allocation into
        // the `GWLP_USERDATA` slot — ownership transfers to the window and is
        // reclaimed by the `WM_DESTROY` arm of `WindowProc`, the hook every
        // Windows host registers on the class (`platform.rs`, `Box::from_raw`);
        // a window destroyed by any path that skips `WM_DESTROY`
        // (process-exit teardown) leaks the
        // allocation rather than double-frees or dangles it. `ShowWindow`/
        // `UpdateWindow` again only need a valid `hwnd`, which holds here.
        unsafe {
            // Remove background brush to allow Mica backdrop
            SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);

            // Apply Windows 11 features automatically
            Self::apply_windows_features(hwnd);
            // One sample seeds both the chrome and the change filter, so the
            // first theme change after creation is never swallowed.
            let appearance = super::appearance::system_appearance();
            super::appearance::apply_chrome(hwnd, appearance);

            tracing::info!(
                "Created window HWND {:?} - {}x{} - scale: {}",
                hwnd,
                width,
                height,
                scale_factor
            );

            // Seed observations from the native client, not requested outer bounds.
            let native_dpi = GetDpiForWindow(hwnd);
            let scale_factor = if native_dpi == 0 {
                scale_factor
            } else {
                native_dpi as f64 / USER_DEFAULT_SCREEN_DPI as f64
            };
            let mut client = RECT::default();
            let initial_size = if GetClientRect(hwnd, &raw mut client).is_ok() {
                Size::new(client.right - client.left, client.bottom - client.top)
            } else {
                Size::new(width, height)
            };
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &raw mut origin);

            let created_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let deferred = options.visible && options.reveal == WindowReveal::AfterFirstFrame;
            let state = Arc::new(Mutex::new(WindowRecord {
                bounds: Bounds {
                    origin: Point::new(
                        origin.x as f64 / scale_factor,
                        origin.y as f64 / scale_factor,
                    ),
                    size: Size::new(
                        initial_size.width as f64 / scale_factor,
                        initial_size.height as f64 / scale_factor,
                    ),
                },
                scale_factor,
                visible: options.visible,
                focused: false,
            }));

            let constraints = SizeConstraints::new(options.min_size, options.max_size);
            let snapshot = Arc::new(WindowStateCell::new(sample_state(
                hwnd,
                &state,
                OwnerTracking {
                    mode: NativeMode::Normal,
                    restore_style: 0,
                    constraints,
                    display: displays.display_of_window(hwnd),
                },
            )));

            // A wake marks the published state stale, so the first read after
            // the machine slept asks the system again.
            notices.mark_stale_on_wake(&snapshot);

            // Create and install the WindowContext for event dispatch
            // BEFORE building the wrapper: the wrapper keeps the pointer as
            // its teardown identity token (see the `context` field doc).
            use flui_foundation::geometry::Size;

            use super::platform::WindowContext;

            let window_id = WindowId(hwnd.0 as u64);
            // Seed the visibility edge filter from the window's ACTUAL
            // style, not a default: a visible undecorated window is created
            // `WS_POPUP | WS_VISIBLE` (already visible before this context
            // installs), so the creation-time `ShowWindow(SW_SHOW)` below
            // never delivers a `WM_SHOWWINDOW` edge for it — a `false`
            // seed would swallow that window's first minimize. See
            // `shared::visibility::win32_initial_visibility`'s doc.
            let message_clock = super::events::MessageClock::new();
            let pointer_registry = std::cell::RefCell::new(super::events::pointer_registry(
                message_clock.native(),
                scale_factor,
            ));
            let context = Box::new(WindowContext {
                window_id,
                windows: Arc::downgrade(&windows_map),
                window_state: Arc::downgrade(&state),
                snapshot: Arc::clone(&snapshot),
                handlers,
                exit_policy,
                callbacks: WindowCallbacks::counting_frames(frames.counter()),
                scale_factor: std::cell::Cell::new(scale_factor),
                message_clock,
                pointer_registry,
                touchpad: super::direct_manipulation::TouchpadSlot::default(),
                mode: std::cell::Cell::new(NativeMode::Normal),
                last_size: std::cell::Cell::new(initial_size),
                config,
                is_hovered: std::cell::Cell::new(false),
                modifiers: std::cell::Cell::new(flui_platform::Modifiers::NONE),
                cursor: std::cell::Cell::new(CursorIcon::default()),
                restore_style: std::cell::Cell::new(0),
                constraints: std::cell::Cell::new(constraints),
                resizable: std::cell::Cell::new(options.resizable),
                first_show: std::cell::Cell::new(if deferred {
                    FirstShow::AfterFirstFrame(options.mode)
                } else {
                    FirstShow::OnShow(options.mode)
                }),
                publish_hold: std::cell::Cell::new(0),
                limits_waived: std::cell::Cell::new(false),
                visibility_muted: std::cell::Cell::new(false),
                show_requests: std::cell::Cell::new(0),
                // A window whose reveal waits for its first frame is visible to
                // the runtime already: its frames must run to earn the reveal.
                visibility: super::window_visibility::Visibility::new(
                    deferred || crate::shared::visibility::win32_initial_visibility(created_style),
                ),
                cloak_watch: super::window_visibility::CloakWatch::start(),
                in_size_move: std::cell::Cell::new(false),
                last_resize: std::cell::Cell::new(None),
                modal_pacing: crate::owner::windows::modal_loop::ModalPacing::new(Rc::clone(
                    &frames,
                )),
                pending_high_surrogate: std::cell::Cell::new(None),
                text_services: std::cell::RefCell::new(None),
                drop_target: std::cell::RefCell::new(None),
                drag: Arc::clone(&drag),
                held_dead_keys: std::cell::RefCell::default(),
                ledger: std::cell::RefCell::new(crate::shared::hwnd_affinity::ContextLedger::new()),
                identity,
                displays: Arc::clone(&displays),
                last_monitor: std::cell::Cell::new(0),
                appearance: std::cell::Cell::new(appearance),
                chrome_follows_system: std::cell::Cell::new(true),
            });
            let context_ptr = Box::into_raw(context);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, context_ptr as isize);
            let this = super::platform::WindowRef::new(hwnd, identity);

            let window = Arc::new(Self {
                hwnd,
                state,
                identity,
                windows_map,
                displays,
                snapshot,
                context: context_ptr,
                // On the creating (owning) thread, as Win32 subclassing
                // requires; `hwnd` was validated just above.
                #[cfg(feature = "a11y")]
                accessibility: Arc::new(super::accessibility::WindowsAccessibility::new(hwnd)),
                haptics: super::haptics::WindowsHaptics::new(this),
                drag_source: super::drag::Win32DragSource::new(drag, this),
            });
            // The wrapper owns the window from here: a panic out of a hook
            // (or anything after it) drops it, which destroys the window and
            // retires its context.

            // The hooks of the window class learn of the window: its text
            // services activate, it takes drags from this process or another
            // (ADR-0211), and the precision touchpad's viewport is created.
            let mut client = windows::Win32::Foundation::RECT::default();
            let size = if GetClientRect(hwnd, &raw mut client).is_ok() {
                (client.right - client.left, client.bottom - client.top)
            } else {
                (width, height)
            };
            // A message a hook dispatched may have destroyed the window: no
            // later hook runs on the dead handle, and the open fails instead
            // of returning a wrapper around it.
            crate::owner::windows::window_created(
                window_hooks,
                &crate::owner::windows::CreatedWindow {
                    hwnd,
                    identity,
                    client_size: size,
                    apartment,
                },
                || this.with_context("window_created", |_| ()).is_some(),
            )
            .map_err(|_gone| OpenWindowError::Backend {
                message: "the window was destroyed while its class's hooks learned of it".into(),
            })?;

            // The window is still hidden: put it where and in the mode it
            // opens, then show it now, later (first frame) or never (hidden).
            Self::prepare_opening(hwnd, options.mode, normal);
            if constraints != SizeConstraints::NONE {
                window.enforce_constraints();
            }
            if options.visible && !deferred {
                window.show_first();
            }

            Ok(window)
        }
    }

    /// Put a window that is still hidden where and in the mode it opens, so
    /// its first sight is already the final one.
    ///
    /// `normal` is the rectangle (screen coordinates) a window that opens
    /// maximized, minimized or fullscreen returns to. It becomes the window's
    /// `rcNormalPosition` through a hidden `SetWindowPlacement`, so a restored
    /// window keeps its earlier bounds. Fullscreen is entered here, hidden.
    fn prepare_opening(hwnd: HWND, mode: WindowMode, normal: Option<RECT>) {
        if let Some(rect) = normal
            && mode != WindowMode::Normal
        {
            let placed = placement_of(hwnd).and_then(|mut placement| {
                placement.showCmd = SW_HIDE.0 as u32;
                placement.rcNormalPosition = to_workspace(rect, workspace_origin(hwnd));
                // SAFETY: `placement` is fully initialized; a stale `hwnd` only
                // makes the call fail.
                unsafe { SetWindowPlacement(hwnd, &raw const placement) }
            });
            if let Err(error) = placed {
                tracing::warn!(?hwnd, ?error, "the opening rectangle could not be set");
            }
        }
        if mode == WindowMode::Fullscreen {
            Self::toggle_fullscreen_for_hwnd(hwnd);
        }
    }

    /// Show a window that has never been shown, in the mode it opened in; does
    /// nothing for one that was. This settles a deferred reveal too.
    fn show_first(&self) {
        let mode = super::platform::with_window_context(self.hwnd, "show_first", |ctx| {
            let mode = ctx.first_show.replace(FirstShow::Done).mode();
            if mode.is_some() {
                ctx.note_show_request();
            }
            mode
        })
        .flatten();
        let Some(mode) = mode else {
            return;
        };
        super::platform::with_window_context(self.hwnd, "show_first", |ctx| {
            ctx.begin_first_show(mode);
        });
        self.show_native(first_show_command(mode));
        // The show can have run user callbacks that closed this window and
        // opened another on the same handle: what follows is for this one only.
        if !self.owns_handle() {
            return;
        }
        // SAFETY: plain queries and a plain call on this wrapper's live window.
        let minimized = unsafe { IsIconic(self.hwnd) }.as_bool();
        super::platform::with_window_context(self.hwnd, "show_first", |ctx| {
            ctx.end_first_show(minimized);
        });
        // SAFETY: a plain call on this wrapper's window handle.
        if let Err(error) = unsafe { UpdateWindow(self.hwnd) }.ok() {
            tracing::warn!(hwnd = ?self.hwnd, ?error, "UpdateWindow failed after the first show");
        }
    }

    /// Note that the app asked for the window to be shown.
    fn note_show_request(&self) {
        super::platform::with_window_context(self.hwnd, "note_show_request", |ctx| {
            ctx.note_show_request();
        });
    }

    /// Whether the handle still names this wrapper's window: after a call that
    /// can run user callbacks, the window may have closed and another taken its
    /// handle.
    pub(super) fn owns_handle(&self) -> bool {
        handle_available(super::platform::teardown_route(self.hwnd, self.context))
            && super::platform::with_window_context(self.hwnd, "owns_handle", |context| {
                context.identity == self.identity
            }) == Some(true)
    }

    /// Apply Windows 11 features automatically
    ///
    /// This applies modern Windows 11 visual features if running on Windows 11:
    /// - Mica backdrop for translucent background with blur
    /// - Rounded window corners
    /// - DWM frame extension for proper backdrop rendering
    fn apply_windows_features(hwnd: HWND) {
        // SAFETY: every call here (`DwmExtendFrameIntoClientArea`,
        // `DwmSetWindowAttribute` x2) takes `hwnd` and a pointer to a
        // stack-local value whose `size_of` matches the `u32` byte-count
        // argument passed alongside it (`MARGINS` by-reference for the first
        // call; `&raw const <i32>` cast to `c_void` for the rest, each with
        // `size_of::<i32>()`). Callers of this function (`WindowsWindow::new`)
        // pass the `hwnd` just returned by `CreateWindowExW`, so it is valid
        // for the duration of this call. Both `DwmSetWindowAttribute`
        // results are discarded (`let _ =`) because every attribute here is a
        // Windows-11-only cosmetic feature — failure on older Windows is
        // expected and non-fatal, not evidence swallowed silently for an
        // operation the caller depends on.
        unsafe {
            use windows::Win32::{
                Graphics::Dwm::{
                    DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
                },
                UI::Controls::MARGINS,
            };

            tracing::debug!("Applying Windows 11 features to HWND {:?}", hwnd);

            // 1. Extend frame into client area (required for Mica backdrop)
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);

            // 2. Enable Mica backdrop (Windows 11+)
            let mica_value: i32 = 2; // DWMSBT_MAINWINDOW
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
                (&raw const mica_value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            );

            // 3. Set rounded corners
            let corner_value: i32 = 2; // DWMWCP_ROUND
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
                (&raw const corner_value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            );

            tracing::debug!("Windows 11 features applied");
        }
    }

    /// Get the native HWND handle
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Installs a callback into this window's owner-thread context.
    ///
    /// `install` runs only on the window's owner thread, against the context
    /// of the very window this wrapper was built for. Otherwise the
    /// registration is refused and `install`, with the callback it carries,
    /// is dropped right here — on the calling thread for a foreign-thread
    /// registration, which is logged as an error because the caller has
    /// just lost a callback; on the owner for a window that is already gone,
    /// which is routine teardown ordering.
    fn register(&self, op: &'static str, install: impl FnOnce(&WindowCallbacks)) {
        let outcome = super::platform::with_window_context_checked(self.hwnd, op, |context| {
            (context.identity == self.identity && context.callbacks.accepts_registration())
                .then(|| install(&context.callbacks))
        });
        match outcome {
            Ok(Some(())) => {}
            Ok(None) => tracing::debug!(
                hwnd = ?self.hwnd,
                op,
                "callback registration refused: the handle now names a different window"
            ),
            Err(UserDataRefusal::ForeignThread) => tracing::error!(
                hwnd = ?self.hwnd,
                op,
                "callback registration refused: window callbacks are registered on the \
                 window's owner thread; the callback was dropped on the calling thread"
            ),
            Err(reason) => tracing::debug!(
                hwnd = ?self.hwnd,
                op,
                ?reason,
                "callback registration refused: the native window is gone"
            ),
        }
    }

    /// Get current window bounds
    pub fn bounds(&self) -> Bounds<f64> {
        let state = self.state.lock();
        Bounds {
            origin: state.bounds.origin,
            size: state.bounds.size,
        }
    }

    pub(super) fn show_native(
        &self,
        command: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD,
    ) {
        // SAFETY: `ShowWindow` takes `self.hwnd` and a command constant by
        // value, no pointer arguments. Its return value is the window's
        // PREVIOUS visibility state, not success/failure, so there is nothing
        // to check.
        unsafe {
            let _ = ShowWindow(self.hwnd, command);
        }
    }

    fn set_native_title(&self, title: &str) {
        // SAFETY: `title_str` is a live, locally-owned `HSTRING` for the
        // duration of the call; `SetWindowTextW` reads it by reference and
        // does not retain the pointer past the call.
        unsafe {
            let title_str = HSTRING::from(title);
            if let Err(error) = SetWindowTextW(self.hwnd, &title_str) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowTextW failed");
            }
        }
    }

    /// Resize the content area to `size`.
    ///
    /// The size is the window's **normal** size, whatever mode it is in, so the
    /// frame is measured against the normal rectangle and never against the
    /// maximized, minimized or fullscreen one:
    ///
    /// - a normal window resizes at once, keeping its position;
    /// - a maximized or minimized window keeps its mode and takes the size
    ///   when it is restored (Win32's `rcNormalPosition`);
    /// - a fullscreen window takes the size for the bounds it returns to.
    ///
    /// # Errors
    /// [`WindowCommandError::InvalidGeometry`] for a size beyond what Win32
    /// can represent, before any call; [`WindowCommandError::Backend`] when
    /// Win32 refuses or fails the resize.
    fn set_inner_size(&self, size: Size<f64>) -> Result<(), WindowCommandError> {
        let scale = self.state.lock().scale_factor;
        let content = device_pair(size.width, size.height, scale)?;
        let fullscreen = super::platform::with_window_context(self.hwnd, "set_inner_size", |ctx| {
            ctx.mode
                .get()
                .fullscreen_bounds()
                .map(|restore_bounds| (restore_bounds, ctx.restore_style.get()))
        })
        .flatten();
        // SAFETY: `self.hwnd` is this wrapper's live window handle. Every
        // out-parameter (`RECT`, `WINDOWPLACEMENT`) is a stack local passed as
        // `&raw mut`/`&raw const`, correctly sized for its call, with
        // `WINDOWPLACEMENT::length` set; `SetWindowPos` takes plain integer
        // and flag arguments with `None` as the documented no-op z-order
        // handle.
        unsafe {
            let dpi = GetDpiForWindow(self.hwnd).max(USER_DEFAULT_SCREEN_DPI);
            let ex_style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            if let Some((restore_bounds, restore_style)) = fullscreen {
                let (width, height) = outer_size(restore_style, ex_style, dpi, content)
                    .map_err(|e| backend_error("AdjustWindowRectExForDpi", e))?;
                super::platform::with_window_context(self.hwnd, "set_inner_size", |ctx| {
                    ctx.mode.set(ctx.mode.get().with_fullscreen_bounds(Bounds {
                        origin: restore_bounds.origin,
                        size: Size::new(width, height),
                    }));
                });
                self.clamp_restore_rectangle();
                return Ok(());
            }
            let mut placement =
                placement_of(self.hwnd).map_err(|e| backend_error("GetWindowPlacement", e))?;
            let away_from_normal = placement.showCmd == SW_SHOWMAXIMIZED.0 as u32
                || placement.showCmd == SW_SHOWMINIMIZED.0 as u32;
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            if away_from_normal {
                let (width, height) = outer_size(style, ex_style, dpi, content)
                    .map_err(|e| backend_error("AdjustWindowRectExForDpi", e))?;
                placement.rcNormalPosition.right =
                    placement.rcNormalPosition.left.saturating_add(width);
                placement.rcNormalPosition.bottom =
                    placement.rcNormalPosition.top.saturating_add(height);
                SetWindowPlacement(self.hwnd, &raw const placement)
                    .map_err(|e| backend_error("SetWindowPlacement", e))?;
                // A size command must not make the next restore break the limits.
                self.clamp_restore_rectangle();
                return Ok(());
            }
            let (width, height) = outer_size(style, ex_style, dpi, content)
                .map_err(|e| backend_error("AdjustWindowRectExForDpi", e))?;
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|e| backend_error("SetWindowPos", e))
        }
    }

    /// Move the window's outer top-left corner to `at`, global logical pixels.
    ///
    /// Like the size, the position is the window's **normal** one: a maximized
    /// or minimized window keeps its mode and takes the position when it is
    /// restored, and a fullscreen window takes it for the bounds it returns to.
    ///
    /// # Errors
    /// [`WindowCommandError::InvalidGeometry`] for a position beyond what
    /// Win32 can represent, before any call; [`WindowCommandError::Backend`]
    /// when Win32 refuses or fails the move.
    fn set_outer_position(&self, at: Point<f64>) -> Result<(), WindowCommandError> {
        let scale = self.state.lock().scale_factor;
        let (x, y) = device_pair(at.x, at.y, scale)?;
        let fullscreen =
            super::platform::with_window_context(self.hwnd, "set_outer_position", |ctx| {
                let mode = ctx.mode.get();
                mode.fullscreen_bounds().is_some_and(|restore_bounds| {
                    ctx.mode.set(mode.with_fullscreen_bounds(Bounds {
                        origin: Point::new(x, y),
                        size: restore_bounds.size,
                    }));
                    true
                })
            })
            .unwrap_or(false);
        if fullscreen {
            return Ok(());
        }
        // SAFETY: `self.hwnd` is this wrapper's live window handle; the
        // `WINDOWPLACEMENT` is a stack local with `length` set.
        unsafe {
            let mut placement =
                placement_of(self.hwnd).map_err(|e| backend_error("GetWindowPlacement", e))?;
            let away_from_normal = placement.showCmd == SW_SHOWMAXIMIZED.0 as u32
                || placement.showCmd == SW_SHOWMINIMIZED.0 as u32;
            if away_from_normal {
                let rect = to_screen(placement.rcNormalPosition, workspace_origin(self.hwnd));
                let moved = RECT {
                    left: x,
                    top: y,
                    right: x.saturating_add(rect.right.saturating_sub(rect.left)),
                    bottom: y.saturating_add(rect.bottom.saturating_sub(rect.top)),
                };
                placement.rcNormalPosition = to_workspace(moved, workspace_origin(self.hwnd));
                return SetWindowPlacement(self.hwnd, &raw const placement)
                    .map_err(|e| backend_error("SetWindowPlacement", e));
            }
            SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|e| backend_error("SetWindowPos", e))
        }
    }

    /// Set the inner-size limits and apply them to the size the window has.
    fn set_constraints(&self, constraints: SizeConstraints) {
        let kept = super::platform::with_window_context(self.hwnd, "set_constraints", |ctx| {
            ctx.constraints.set(constraints);
        });
        if kept.is_some() {
            self.enforce_constraints();
        }
    }

    /// Give the window a frame of this kind, keeping its content size.
    ///
    /// A fullscreen window has no frame to change: the style it returns to
    /// changes instead, and it is the one the next sample reports.
    ///
    /// # Errors
    /// [`WindowCommandError::Backend`] when Win32 refuses or fails the change.
    fn restyle_frame(&self, decorated: bool, resizable: bool) -> Result<(), WindowCommandError> {
        let fullscreen = super::platform::with_window_context(self.hwnd, "restyle_frame", |ctx| {
            ctx.resizable.set(resizable);
            let mode = ctx.mode.get();
            mode.fullscreen_bounds().map(|bounds| {
                let old = ctx.restore_style.get();
                let new = restyled(old, decorated, resizable);
                ctx.restore_style.set(new);
                // The frame it returns to changes size: the outer rectangle
                // follows, so the content it returns to is the one it had.
                // SAFETY: plain queries of this wrapper's live window.
                let (dpi, ex_style) = unsafe {
                    (
                        GetDpiForWindow(self.hwnd).max(USER_DEFAULT_SCREEN_DPI),
                        GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32,
                    )
                };
                if let (Ok(was), Ok(now)) = (
                    outer_size(old, ex_style, dpi, (0, 0)),
                    outer_size(new, ex_style, dpi, (0, 0)),
                ) {
                    ctx.mode.set(
                        mode.with_fullscreen_bounds(Bounds {
                            origin: bounds.origin,
                            size: Size::new(
                                bounds
                                    .size
                                    .width
                                    .saturating_sub(was.0)
                                    .saturating_add(now.0)
                                    .max(1),
                                bounds
                                    .size
                                    .height
                                    .saturating_sub(was.1)
                                    .saturating_add(now.1)
                                    .max(1),
                            ),
                        }),
                    );
                }
            })
        })
        .flatten()
        .is_some();
        if fullscreen {
            return Ok(());
        }
        // SAFETY: `self.hwnd` is this wrapper's live window handle; the RECT is
        // a stack local out-parameter; the style calls take plain integers.
        super::platform::with_window_context(
            self.hwnd,
            "restyle_frame",
            |ctx| -> Result<(), WindowCommandError> {
                // The frame changes in two steps; a subscriber sees the result only.
                let _hold = ctx.hold_publishing();
                unsafe {
                    let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
                    let changed = restyled(style, decorated, resizable);
                    if changed == style {
                        return Ok(());
                    }
                    let mut client = RECT::default();
                    GetClientRect(self.hwnd, &raw mut client)
                        .map_err(|e| backend_error("GetClientRect", e))?;
                    let content = (client.right - client.left, client.bottom - client.top);
                    let ex_style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
                    let dpi = GetDpiForWindow(self.hwnd).max(USER_DEFAULT_SCREEN_DPI);
                    let normal = !IsZoomed(self.hwnd).as_bool() && !IsIconic(self.hwnd).as_bool();
                    SetWindowLongPtrW(self.hwnd, GWL_STYLE, changed as isize);
                    let (width, height) = outer_size(changed, ex_style, dpi, content)
                        .map_err(|e| backend_error("AdjustWindowRectExForDpi", e))?;
                    let mut flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
                    if !normal {
                        flags |= SWP_NOSIZE;
                    }
                    SetWindowPos(self.hwnd, None, 0, 0, width, height, flags)
                        .map_err(|e| backend_error("SetWindowPos", e))?;
                    if !normal {
                        // The rectangle the window returns to grows or shrinks by
                        // the change of frame, so restoring it keeps its content.
                        let was = outer_size(style, ex_style, dpi, (0, 0));
                        let now = outer_size(changed, ex_style, dpi, (0, 0));
                        if let (Ok(was), Ok(now), Ok(mut placement)) =
                            (was, now, placement_of(self.hwnd))
                        {
                            let normal = &mut placement.rcNormalPosition;
                            normal.right = normal.right.saturating_add(now.0 - was.0);
                            normal.bottom = normal.bottom.saturating_add(now.1 - was.1);
                            SetWindowPlacement(self.hwnd, &raw const placement)
                                .map_err(|e| backend_error("SetWindowPlacement", e))?;
                        }
                    }
                    Ok(())
                }
            },
        )
        .unwrap_or(Err(WindowCommandError::Closed))
    }

    /// Whether the window waits for its first presented frame to be shown.
    fn awaiting_reveal(&self) -> bool {
        super::platform::with_window_context(self.hwnd, "awaiting_reveal", |ctx| {
            ctx.first_show.get().awaits_frame()
        })
        .unwrap_or(false)
    }

    /// Put the window in `mode`.
    ///
    /// A window that waits for its first frame takes the mode as the one it is
    /// revealed in: setting a mode must not show it early.
    ///
    /// # Errors
    /// [`WindowCommandError::Unsupported`] for a mode this backend does not know.
    fn set_mode(&self, mode: WindowMode) -> Result<(), WindowCommandError> {
        if !matches!(
            mode,
            WindowMode::Normal
                | WindowMode::Minimized
                | WindowMode::Maximized
                | WindowMode::Fullscreen
        ) {
            return Err(WindowCommandError::Unsupported);
        }
        if self.awaiting_reveal() {
            self.set_fullscreen(mode == WindowMode::Fullscreen);
            super::platform::with_window_context(self.hwnd, "stage_mode", |ctx| {
                // Fullscreen is entered hidden and shown like a normal window.
                let staged = match mode {
                    WindowMode::Fullscreen => WindowMode::Normal,
                    other => other,
                };
                ctx.first_show.set(ctx.first_show.get().staged(staged));
            });
            return Ok(());
        }
        match mode {
            WindowMode::Minimized => {
                self.note_show_request();
                self.show_native(SW_MINIMIZE);
            }
            WindowMode::Maximized => {
                self.note_show_request();
                // A fullscreen popup is not maximized: it leaves fullscreen first.
                self.set_fullscreen(false);
                self.show_native(SW_MAXIMIZE);
            }
            WindowMode::Fullscreen => self.set_fullscreen(true),
            _ => self.restore_normal(),
        }
        // A window that was never shown has had its first show if the mode
        // showed it (minimized, maximized); otherwise it is still to be shown,
        // and it is shown as the normal or fullscreen window it now is.
        super::platform::with_window_context(self.hwnd, "set_mode", |ctx| {
            if ctx.first_show.get().mode().is_some() {
                // SAFETY: a plain query of this wrapper's window handle.
                let shown = unsafe { IsWindowVisible(self.hwnd) }.as_bool();
                ctx.first_show.set(if shown {
                    FirstShow::Done
                } else {
                    FirstShow::OnShow(WindowMode::Normal)
                });
            }
        });
        Ok(())
    }

    /// Hide the window. A window whose first show was still to come stays
    /// hidden until it is shown explicitly.
    fn hide_native(&self) {
        self.show_native(SW_HIDE);
        self.state.lock().visible = false;
        super::platform::with_window_context(self.hwnd, "hide", |ctx| {
            // The first frame no longer shows it; an explicit show still does.
            ctx.first_show
                .set(ctx.first_show.get().without_frame_wait());
            // A window hidden before it was ever shown sends no `WM_SHOWWINDOW`;
            // the runtime still has to hear that it is no longer visible.
            ctx.report_surface_visibility(false);
        });
    }

    /// Get current scale factor
    pub fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

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
    pub fn toggle_fullscreen_for_hwnd(hwnd: HWND) {
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

        let toggled = super::platform::with_window_context(hwnd, "toggle_fullscreen", |ctx| {
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

    /// Toggle fullscreen mode for this window
    pub fn toggle_fullscreen(&self) {
        Self::toggle_fullscreen_for_hwnd(self.hwnd);
    }

    /// Check if the window is currently in fullscreen mode
    ///
    /// `false` off the owning thread or once the window is gone — the
    /// context gate refuses the read instead of racing `WM_DESTROY`'s free.
    pub fn is_fullscreen(&self) -> bool {
        super::platform::with_window_context(self.hwnd, "is_fullscreen", |ctx| {
            ctx.mode.get().holds_fullscreen()
        })
        .unwrap_or(false)
    }

    /// Set fullscreen mode
    ///
    /// # Arguments
    /// * `fullscreen` - true to enter fullscreen, false to exit fullscreen
    pub fn set_fullscreen(&self, fullscreen: bool) {
        // A fullscreen window that is minimized comes back fullscreen when it
        // is restored; leaving fullscreen starts from there.
        let minimized_from_fullscreen =
            super::platform::with_window_context(self.hwnd, "set_fullscreen", |ctx| {
                matches!(
                    ctx.mode.get(),
                    NativeMode::Minimized {
                        from_fullscreen: Some(_)
                    }
                )
            })
            .unwrap_or(false);
        if minimized_from_fullscreen {
            self.show_native(SW_RESTORE);
        }
        // Only toggle if state needs to change
        if fullscreen != self.is_fullscreen() {
            Self::toggle_fullscreen_for_hwnd(self.hwnd);
        }
    }

    /// Ask for a frame of `hwnd`, from any thread. A window that is shown and
    /// not minimized is invalidated and paints. Win32 paints neither a hidden
    /// window nor a minimized one, whose client area is empty, so those get
    /// the frame request posted: their frame is what earns a deferred window
    /// its reveal, and what polls the tasks woken while nobody sees it.
    pub(super) fn redraw_native(hwnd: HWND) {
        // SAFETY: plain calls on a window handle with no pointer arguments.
        // Each may fail for a window that is already gone, which leaves
        // nothing to draw; a redraw request is best-effort.
        unsafe {
            if IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
                let _ = InvalidateRect(Some(hwnd), None, false);
            } else {
                let _ = PostMessageW(Some(hwnd), REQUEST_FRAME, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub(super) fn apply_native_cursor(cursor: CursorIcon) -> Result<(), CursorError> {
        let resource = match cursor {
            CursorIcon::Pointer | CursorIcon::Copy | CursorIcon::Grab | CursorIcon::Grabbing => {
                IDC_HAND
            }
            CursorIcon::Progress => IDC_APPSTARTING,
            CursorIcon::Wait => IDC_WAIT,
            CursorIcon::Cell | CursorIcon::Crosshair => IDC_CROSS,
            CursorIcon::Text | CursorIcon::VerticalText => IDC_IBEAM,
            CursorIcon::Move | CursorIcon::AllScroll => IDC_SIZEALL,
            CursorIcon::NoDrop | CursorIcon::NotAllowed => IDC_NO,
            CursorIcon::EResize
            | CursorIcon::WResize
            | CursorIcon::EwResize
            | CursorIcon::ColResize => IDC_SIZEWE,
            CursorIcon::NResize
            | CursorIcon::SResize
            | CursorIcon::NsResize
            | CursorIcon::RowResize => IDC_SIZENS,
            CursorIcon::NeResize | CursorIcon::SwResize | CursorIcon::NeswResize => IDC_SIZENESW,
            CursorIcon::NwResize | CursorIcon::SeResize | CursorIcon::NwseResize => IDC_SIZENWSE,
            // Default, ContextMenu, Help, Alias, ZoomIn, ZoomOut, DndAsk and
            // every future variant fall back to the arrow cursor.
            _ => IDC_ARROW,
        };

        // SAFETY: `LoadCursorW(None, resource)` loads a built-in system
        // cursor by atom/ordinal (`resource` is always one of the `IDC_*`
        // constants from the match above, never a caller-supplied pointer),
        // and returns a handle owned by the system — no allocation to free.
        // `SetCursor` takes that handle by value; passing `None` on error is
        // not reachable here since `?` returns before it.
        unsafe {
            let handle = LoadCursorW(None, resource)
                .map_err(|error| CursorError::Backend(error.to_string()))?;
            SetCursor(Some(handle));
        }
        Ok(())
    }
}

impl crate::host::Window<WindowsLoop> for WindowsWindow {
    fn open(
        owner: &WindowsLoop,
        options: WindowOptions,
    ) -> Result<Arc<dyn crate::host_window::HostWindow>, OpenWindowError> {
        if !owner.owner_control.signal.accepting() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        // An HWND's message queue belongs to the creating thread, and the
        // window context shares the owner-only platform handlers, so a
        // window is opened on the owner thread or not at all (ADR-0039).
        let shares = owner
            .owner_control
            .shares("open_window")
            .map_err(|reason| OpenWindowError::Backend {
                message: format!(
                    "Win32 windows are opened on the platform's owner thread ({reason:?})"
                ),
            })?;
        tracing::info!("Opening window: {:?}", options.title);

        let window = WindowsWindow::new(
            options,
            owner.windows.clone(),
            shares,
            owner.config.clone(),
            owner.apartment.share(),
            Arc::clone(&owner.drag),
        )?;
        let hwnd_value = window.hwnd().0 as isize;
        let this = WindowRef::new(window.hwnd(), window.identity);
        let wake =
            crate::owner::signal::OwnerSignal::input_wake(&owner.owner_control.signal, move || {
                let _ = this.with_context("resume_input_delivery", |context| {
                    context.callbacks.resume_input_delivery();
                });
            });
        let _ =
            with_window_context_checked(window.hwnd(), "input_capabilities", |context| {
                context.callbacks.set_input_capabilities(
                    crate::shared::input_availability::windows(context.has_touchpad()),
                );
                context.callbacks.set_input_wake(wake);
            });

        // Store window
        let _prev = owner.windows.lock().insert(hwnd_value, window.clone());

        Ok(window)
    }

    fn active(owner: &WindowsLoop) -> Option<WindowId> {
        owner.affinity.debug_assert_owner("WindowsWindow::active");
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

    fn capabilities(_: &WindowsLoop) -> WindowCapabilities {
        window_capabilities()
    }

    fn frame_inset(_: &WindowsLoop, _: f64) -> Size<f64> {
        // The frame is not measured before a window exists, so the host
        // reports the contract's default: no inset.
        Size::new(0.0, 0.0)
    }

    fn on_event(owner: &WindowsLoop, callback: Box<dyn FnMut(flui_platform::WindowEvent) + Send>) {
        owner.register_handler("on_window_event", callback, |handlers| {
            &mut handlers.window_event
        });
    }
}

impl crate::host_window::CoreHostWindow for WindowsWindow {}

impl crate::host_window::HostWindow for WindowsWindow {
    /// The window's own UIA bridge — the capability the composition root's
    /// accessibility wire discovers. Without this override the trait
    /// default (`None`) leaves every real Windows window silently invisible
    /// to UI Automation clients.
    #[cfg(feature = "a11y")]
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility) as _)
    }

    /// The token proves the platform's owner thread, not this window's: an
    /// HWND belongs to the thread that created it, and its text services
    /// live in its `WindowContext` on that thread. So the host is read
    /// through `with_window_context`, which refuses any other thread
    /// (`shared::hwnd_affinity`), and a caller on one gets `None`. On the
    /// creating thread it is the window's TSF connection (ADR-0135 §3):
    /// `None` when TSF did not activate for it, or once the window is
    /// destroyed; a host read before that answers
    /// [`TextStoreHostError::Unavailable`] from then on.
    ///
    /// [`TextStoreHostError::Unavailable`]: flui_platform::text_store::TextStoreHostError::Unavailable
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform::text_store::TextStoreHost>> {
        super::platform::with_window_context(self.hwnd, "text_store_host", |context| {
            let services = context.text_services.borrow().clone();
            services.map(|services| services as Rc<dyn flui_platform::text_store::TextStoreHost>)
        })
        .flatten()
    }
}

impl PlatformWindow for WindowsWindow {
    fn id(&self) -> WindowId {
        WindowId(self.hwnd.0 as u64)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.haptics) as Arc<dyn flui_platform::PlatformHaptics>)
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        Some(Arc::clone(&self.drag_source) as Arc<dyn flui_platform::drag::PlatformDragSource>)
    }

    fn window_at(&self, position: Point<f64>) -> flui_platform::WindowAt {
        use flui_platform::WindowAt;
        use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};

        let scale = self.state.lock().scale_factor;
        let device = |logical: f64| {
            let device = (logical * scale).round();
            (device.is_finite() && device >= f64::from(i32::MIN) && device <= f64::from(i32::MAX))
                .then_some(device as i32)
        };
        let (Some(x), Some(y)) = (device(position.x), device(position.y)) else {
            // No display holds a point past the device coordinate range.
            return WindowAt::Outside;
        };
        let mut point = POINT { x, y };
        // SAFETY: `point` is a live, writable local; on a stale `hwnd` the call
        // fails and `point` is not read.
        if !unsafe { ClientToScreen(self.hwnd, &raw mut point) }.as_bool() {
            return WindowAt::Unknown;
        }
        // SAFETY: queries by value; a point over no window answers a null
        // handle, which `GetAncestor` and the context lookup refuse.
        let root = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
        // Only a window of this platform instance, on this thread, has a context
        // here; any other window under the point is another application's.
        let Some((window, target_scale)) =
            super::platform::with_window_context(root, "window_at", |context| {
                (context.window_id, context.scale_factor.get())
            })
        else {
            return WindowAt::Outside;
        };
        let mut client = RECT::default();
        // SAFETY: `root` is this thread's live window (its context answered);
        // both out-parameters are live locals.
        let placed = unsafe {
            ScreenToClient(root, &raw mut point).as_bool()
                && GetClientRect(root, &raw mut client).is_ok()
        };
        let inside = (client.left..client.right).contains(&point.x)
            && (client.top..client.bottom).contains(&point.y);
        if !(placed && inside) {
            // Over the window's frame, not its content.
            return WindowAt::Outside;
        }
        WindowAt::Window {
            window,
            position: Point::new(
                f64::from(point.x) / target_scale,
                f64::from(point.y) / target_scale,
            ),
        }
    }

    fn physical_size(&self) -> Size<i32> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor;
        Size::new(
            logical_to_device(logical.width, scale),
            logical_to_device(logical.height, scale),
        )
    }

    fn logical_size(&self) -> Size<f64> {
        self.state.lock().bounds.size
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        use crate::shared::hwnd_affinity::UserDataRefusal;
        use flui_platform::{GestureGeometry, PreferenceQueryError};

        super::platform::with_window_context_checked(self.hwnd, "gesture geometry", |_| {
            // SAFETY: the checked owner context proves this is the still-live
            // owned HWND. The scalar DPI getter does not dispatch user code.
            let dpi = unsafe { GetDpiForWindow(self.hwnd) };
            let native = super::preferences::mouse_geometry(dpi)?;
            Ok(Some(GestureGeometry::from_native_mouse(&native)?))
        })
        .map_err(|refusal| match refusal {
            UserDataRefusal::ForeignThread => PreferenceQueryError::WrongThread,
            _ => PreferenceQueryError::Unavailable,
        })?
    }

    fn request_redraw(&self) {
        // A handle the OS has reused for another window is not this one.
        if !handle_available(super::platform::teardown_route(self.hwnd, self.context)) {
            return;
        }
        Self::redraw_native(self.hwnd);
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        // A locked session or a cloak hides a shown window. Off the owner
        // thread only the shown state is known.
        self.state.lock().visible
            && !super::platform::with_window_context(self.hwnd, "is_visible", |ctx| {
                ctx.visibility.occluded()
            })
            .unwrap_or(false)
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        super::platform::with_window_context(self.hwnd, "set_cursor", |ctx| {
            ctx.cursor.set(cursor);
            if ctx.is_hovered.get() {
                Self::apply_native_cursor(cursor)?;
            }
            Ok(())
        })
        .ok_or_else(|| {
            CursorError::Backend(
                "the native window is closed, or set_cursor was called off its owning thread"
                    .to_string(),
            )
        })?
    }

    // ==================== State and commands (ADR-0237) ====================

    fn state(&self) -> WindowState {
        // The owner context holds the tracking the snapshot needs; off the
        // owner thread, or once the window is gone, the last published
        // snapshot is the answer.
        super::platform::with_window_context(self.hwnd, "state", |ctx| ctx.window_state(self.hwnd))
            .unwrap_or_else(|| self.snapshot.published())
    }

    fn capabilities(&self) -> WindowCapabilities {
        window_capabilities()
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if !handle_available(super::platform::teardown_route(self.hwnd, self.context)) {
            return Err(WindowCommandError::Closed);
        }
        command.admit(&PlatformWindow::capabilities(self))?;
        match command {
            WindowCommand::SetTitle(title) => self.set_native_title(&title),
            WindowCommand::SetPosition(at) => self.set_outer_position(at)?,
            WindowCommand::SetSize(size) => self.set_inner_size(size)?,
            WindowCommand::SetConstraints(constraints) => self.set_constraints(constraints),
            WindowCommand::SetResizable(on) => {
                let decorated =
                    super::platform::with_window_context(self.hwnd, "set_resizable", |ctx| {
                        // A fullscreen window has been stripped of its frame:
                        // the style it returns to says whether it has one.
                        let style = if ctx.mode.get().holds_fullscreen() {
                            ctx.restore_style.get()
                        } else {
                            // SAFETY: a plain style query on this window.
                            unsafe { GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32 }
                        };
                        style & WS_CAPTION.0 == WS_CAPTION.0
                    })
                    .unwrap_or(true);
                self.restyle_frame(decorated, on)?;
            }
            WindowCommand::SetDecorated(on) => {
                let resizable =
                    super::platform::with_window_context(self.hwnd, "set_decorated", |ctx| {
                        ctx.resizable.get()
                    })
                    .unwrap_or(true);
                self.restyle_frame(on, resizable)?;
            }
            WindowCommand::SetMode(mode) => self.set_mode(mode)?,
            WindowCommand::Hide => self.hide_native(),
            WindowCommand::Show => self.show().map_err(|error| match error {
                flui_platform::WindowShowError::Closed => WindowCommandError::Closed,
                flui_platform::WindowShowError::Native { message } => {
                    WindowCommandError::Backend { message }
                }
                _ => WindowCommandError::Unsupported,
            })?,
            WindowCommand::Activate => self.activate(),
            // Every other command is refused by `admit` through a capability
            // this backend leaves `false`.
            _ => return Err(WindowCommandError::Unsupported),
        }
        super::platform::with_window_context(self.hwnd, "apply", |ctx| {
            ctx.publish_window_state(self.hwnd);
        });
        Ok(())
    }

    fn is_active(&self) -> bool {
        // SAFETY: `GetForegroundWindow` takes no arguments; comparing its
        // result to `self.hwnd` is a plain integer/handle comparison.
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    fn is_hovered(&self) -> bool {
        super::platform::with_window_context(self.hwnd, "is_hovered", |ctx| ctx.is_hovered.get())
            .unwrap_or(false)
    }

    fn mouse_position(&self) -> Point<f64> {
        // SAFETY: `cursor_pos` is a stack-local `POINT`; `&raw mut
        // cursor_pos` gives both `GetCursorPos` and `ScreenToClient` a
        // valid, correctly-sized out-parameter. The `is_ok()`/`as_bool()`
        // short-circuit means `cursor_pos` is only read after both calls
        // reported success, so it is never read uninitialized.
        unsafe {
            let mut cursor_pos = POINT::default();
            if GetCursorPos(&raw mut cursor_pos).is_ok()
                && ScreenToClient(self.hwnd, &raw mut cursor_pos).as_bool()
            {
                let scale = self.state.lock().scale_factor;
                Point::new(cursor_pos.x as f64 / scale, cursor_pos.y as f64 / scale)
            } else {
                Point::default()
            }
        }
    }

    fn modifiers(&self) -> flui_platform::keyboard::Modifiers {
        super::platform::with_window_context(self.hwnd, "modifiers", |ctx| ctx.modifiers.get())
            .unwrap_or(flui_platform::Modifiers::NONE)
    }

    fn appearance(&self) -> WindowAppearance {
        // The user's theme, read from any thread without the window context.
        // Not the DWM dark-mode flag: this backend sets that flag to match,
        // so reading it back would report the backend's own choice.
        super::appearance::system_appearance()
    }

    fn display_id(&self) -> Option<flui_platform::DisplayId> {
        // A destroyed or recycled `HWND` names another window's monitor: refuse
        // it with the identity probe `window_handle` uses.
        if !handle_available(super::platform::teardown_route(self.hwnd, self.context)) {
            return None;
        }
        self.displays.display_of_window(self.hwnd)
    }

    fn show(&self) -> Result<(), flui_platform::WindowShowError> {
        use windows::Win32::UI::WindowsAndMessaging::{
            SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WPF_RESTORETOMAXIMIZED,
        };
        crate::shared::hwnd_affinity::show_owned_window(
            || {
                use crate::shared::hwnd_affinity::TeardownRoute;
                let route = super::platform::teardown_route(self.hwnd, self.context);
                if route != TeardownRoute::DestroyDirect {
                    return route;
                }
                // The wrapper and its live context carry the same identity,
                // minted once and never reused, so unlike the raw context
                // address it cannot be recycled. Only inspect it under the
                // owner guard.
                if super::platform::with_window_context(self.hwnd, "show identity", |context| {
                    context.identity == self.identity
                }) == Some(true)
                {
                    route
                } else {
                    TeardownRoute::StaleHandle
                }
            },
            || {
                super::platform::with_window_context(self.hwnd, "show", |ctx| {
                    ctx.note_show_request();
                    let first = ctx.first_show.replace(FirstShow::Done).mode();
                    // SAFETY: the identity check proves this wrapper's live HWND
                    // on its owner thread. The context guard pins userdata through
                    // ShowWindow reentry; placement is an initialized out pointer.
                    unsafe {
                        let placement = placement_of(self.hwnd).map_err(|error| {
                            flui_platform::WindowShowError::Native {
                                message: error.to_string(),
                            }
                        })?;
                        if let Some(mode) = first {
                            ctx.begin_first_show(mode);
                        }
                        let command = if let Some(mode) = first {
                            first_show_command(mode)
                        } else if placement.showCmd == SW_SHOWMINIMIZED.0 as u32 {
                            if placement.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0 {
                                SW_SHOWMAXIMIZED
                            } else {
                                SW_RESTORE
                            }
                        } else {
                            SW_SHOW
                        };
                        // Return value describes previous visibility, not success.
                        let _ = ShowWindow(self.hwnd, command);
                        if first.is_some() && self.owns_handle() {
                            ctx.end_first_show(IsIconic(self.hwnd).as_bool());
                        }
                        Ok(())
                    }
                })
                .unwrap_or(Err(flui_platform::WindowShowError::Closed))
            },
            || {
                // SAFETY: the adapter rechecked wrapper identity after ShowWindow,
                // on the same owner thread, before this callback-capable call.
                // Foreground policy may deny the request without failing show.
                unsafe {
                    let _ = SetForegroundWindow(self.hwnd);
                }
            },
        )
    }

    fn reveal_after_first_frame(&self) {
        // Only a window whose reveal was deferred waits for this call; one
        // opened hidden, or already shown, ignores it.
        let deferred = super::platform::with_window_context(self.hwnd, "reveal", |ctx| {
            ctx.first_show.get().awaits_frame()
        })
        .unwrap_or(false);
        if deferred {
            self.show_first();
            tracing::debug!("Windows: window revealed after its first presented frame");
        }
    }

    fn activate(&self) {
        // Activating a window that was never shown shows it.
        self.show_first();
        // SAFETY: `SetForegroundWindow` takes `self.hwnd` by value, no
        // pointer arguments.
        unsafe {
            if let Err(error) = SetForegroundWindow(self.hwnd).ok() {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetForegroundWindow failed");
            }
        }
    }

    fn close(&self) {
        use crate::shared::hwnd_affinity::TeardownRoute;
        match super::platform::teardown_route(self.hwnd, self.context) {
            TeardownRoute::DestroyDirect => {
                // SAFETY: `DestroyWindow` takes `self.hwnd` by value; the
                // route just established this is the owning thread, the only
                // one Win32 permits the call from, and where `WM_DESTROY`
                // (handled in `platform.rs`, reclaiming the `GWLP_USERDATA`
                // allocation) is dispatched synchronously before the call
                // returns.
                unsafe {
                    if let Err(error) = DestroyWindow(self.hwnd) {
                        tracing::warn!(hwnd = ?self.hwnd, ?error, "DestroyWindow (PlatformWindow::close) failed");
                    }
                }
            }
            TeardownRoute::PostClose => {
                // Cross-thread close cannot call `DestroyWindow` (Win32
                // forbids it off the creating thread), so it goes through
                // the documented mechanism instead: post `WM_CLOSE` to the
                // owner thread's queue. Note the behavioral nuance: the
                // owner-side `WM_CLOSE` arm consults the window's
                // should-close veto before destroying, so a vetoing window
                // treats a cross-thread `close()` as a close *request*.
                //
                // SAFETY: `PostMessageW` takes the handle and plain
                // integers by value — nothing dereferenced, delivery
                // marshalled by the OS to the owning thread.
                unsafe {
                    if let Err(error) =
                        PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
                    {
                        tracing::warn!(hwnd = ?self.hwnd, ?error, "PostMessageW(WM_CLOSE) (cross-thread PlatformWindow::close) failed");
                    }
                }
            }
            TeardownRoute::AlreadyGone => {
                tracing::debug!(hwnd = ?self.hwnd, "close: the native window is already gone");
            }
            TeardownRoute::StaleHandle => {
                tracing::debug!(
                    hwnd = ?self.hwnd,
                    "close: the handle no longer names this wrapper's window \
                     (recycled by the OS); leaving its new owner untouched"
                );
            }
        }
    }

    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance) {
        // SAFETY: `backdrop_value` is a stack-local `i32`; `DwmSetWindowAttribute`
        // receives it via `&raw const` cast to `c_void`, sized with
        // `size_of::<i32>()` matching the value pointed at.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

            let backdrop_value: i32 = match appearance {
                // DWMSBT_NONE — Windows has no native transparent backdrop,
                // so Transparent also maps to NONE.
                WindowBackgroundAppearance::Opaque | WindowBackgroundAppearance::Transparent => 1,
                WindowBackgroundAppearance::Blurred => 3, // DWMSBT_TRANSIENTWINDOW (Acrylic)
                WindowBackgroundAppearance::MicaBackdrop => 2, // DWMSBT_MAINWINDOW (Mica)
                WindowBackgroundAppearance::MicaAltBackdrop => 4, // DWMSBT_TABBEDWINDOW (Mica Alt)
            };

            if let Err(error) = DwmSetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
                (&raw const backdrop_value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<i32>() as u32,
            ) {
                tracing::warn!(
                    hwnd = ?self.hwnd,
                    ?error,
                    ?appearance,
                    "DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE) failed"
                );
            }
        }
    }

    // ==================== Per-Window Callbacks ====================
    //
    // Every setter goes through `Self::register`: it installs into the
    // owner-thread context, or refuses and drops the callback on the calling
    // thread. A replaced callback is retired through the call point after its
    // slot's lock is released, since its destructor may re-enter this window.

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.register("on_input", move |callbacks| {
            callbacks.set_input_callback(callback);
        });
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_request_frame", move |callbacks| {
            install(&callbacks.on_request_frame, callback);
        });
    }

    fn on_resize(&self, callback: Box<dyn FnMut(WindowMetrics) + Send>) {
        self.register("on_resize", move |callbacks| {
            install(&callbacks.on_resize, callback);
        });
    }

    fn on_state_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_state_changed", move |callbacks| {
            install(&callbacks.on_state_changed, callback);
        });
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        self.register("on_close", move |callbacks| {
            install(&callbacks.on_close, callback);
        });
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        self.register("on_should_close", move |callbacks| {
            install(&callbacks.on_should_close, callback);
        });
    }

    fn on_safe_area_change(&self, callback: Box<dyn FnMut(EdgeInsets) + Send>) {
        self.register("on_safe_area_change", move |callbacks| {
            callbacks.set_safe_area_callback(callback);
        });
    }

    fn on_execution_state_change(&self, callback: Box<dyn FnMut(WindowExecutionState) + Send>) {
        self.register("on_execution_state_change", move |callbacks| {
            callbacks.set_execution_state_callback(callback);
        });
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_active_status_change", move |callbacks| {
            install(&callbacks.on_active_status_change, callback);
        });
    }

    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_visibility_status_change", move |callbacks| {
            install(&callbacks.on_visibility_status_change, callback);
        });
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_hover_status_change", move |callbacks| {
            install(&callbacks.on_hover_status_change, callback);
        });
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_appearance_changed", move |callbacks| {
            install(&callbacks.on_appearance_changed, callback);
        });
    }

    fn on_display_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.register("on_display_changed", move |callbacks| {
            install(&callbacks.on_display_changed, callback);
        });
    }

    fn on_surface_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.register("on_surface_status_change", move |callbacks| {
            install(&callbacks.on_surface_status_change, callback);
        });
    }

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        HasWindowHandle::window_handle(self)
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        HasDisplayHandle::display_handle(self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Whether [`WindowsWindow`]'s [`HasWindowHandle::window_handle`] may hand
/// out a handle for a window whose teardown routes to `route`.
///
/// Pure so it is testable without a live `HWND` (see
/// [`crate::shared::hwnd_affinity::TeardownRoute`]): `DestroyDirect` and
/// `PostClose` both describe a live window this wrapper still owns (they
/// differ only in which thread may call `DestroyWindow` directly);
/// `AlreadyGone` and `StaleHandle` both describe a window with nothing left
/// to hand a caller — destroyed, or the OS recycled the `HWND` value for an
/// unrelated window.
#[must_use]
fn handle_available(route: crate::shared::hwnd_affinity::TeardownRoute) -> bool {
    use crate::shared::hwnd_affinity::TeardownRoute;
    matches!(
        route,
        TeardownRoute::DestroyDirect | TeardownRoute::PostClose
    )
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for WindowsWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::num::NonZeroIsize;

        // Refuse a destroyed or recycled `HWND` up front — the same
        // identity probe `close()` uses to route teardown, reused here so a
        // caller that re-queries this method (issue #1043's recovery path,
        // which holds an `Arc<dyn PlatformWindow>` rather than a saved
        // handle) never receives a handle whose pointee no longer exists.
        if !handle_available(super::platform::teardown_route(self.hwnd, self.context)) {
            return Err(raw_window_handle::HandleError::Unavailable);
        }

        let hwnd_value = self.hwnd.0 as isize;
        let mut handle = Win32WindowHandle::new(
            NonZeroIsize::new(hwnd_value).ok_or(raw_window_handle::HandleError::Unavailable)?,
        );

        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments — always sound.
        unsafe {
            let hinstance =
                GetModuleHandleW(None).map_err(|_| raw_window_handle::HandleError::Unavailable)?;
            let hinstance_value = hinstance.0 as isize;
            handle.hinstance = NonZeroIsize::new(hinstance_value);
        }

        // SAFETY: `raw_window_handle::WindowHandle::borrow_raw`'s contract
        // requires the wrapped handle to stay valid for the returned
        // `WindowHandle`'s lifetime, which this function ties only to
        // `&self` — i.e. to the `Arc<WindowsWindow>` staying alive — not to
        // the underlying native HWND staying valid for that whole span.
        //
        // What the `handle_available` check above closes: a caller that
        // re-acquires a handle from a retained `Arc<dyn PlatformWindow>`
        // (issue #1043's recovery path) never receives one for a window
        // that is already destroyed or whose `HWND` value the OS already
        // recycled for someone else's window — those routes are refused
        // before `Win32WindowHandle::new` runs. And `WM_DESTROY` now calls
        // `WindowCallbacks::clear()` (see `platform.rs`), which breaks the
        // frame-callback → renderer → surface → `Arc<WindowsWindow>` cycle
        // that used to orphan the whole chain forever after a native close
        // — a surface is no longer pinned alive past the window it was
        // created from.
        //
        // What stays open: the identity probe is a point-in-time check.
        // `close()` on the owning thread (`TeardownRoute::DestroyDirect`)
        // calls `DestroyWindow` synchronously, and Win32 dispatches
        // `WM_DESTROY` — with it, `callbacks.clear()` — before that call
        // returns. If that `close()` is itself invoked from inside a
        // callback currently out of its slot in `WindowCallbacks` for its
        // call (the restore hazard #919 identified), a surface built
        // from a handle this method returned earlier in the same call chain
        // can still be alive while `DestroyWindow` runs beneath it — rwh's
        // own contract puts the burden of not outliving the native window
        // on the caller holding the handle, not on this method. That is a
        // logic-level ordering gap this comment records, not a memory-safety
        // one: `WindowHandle::borrow_raw`'s own doc
        // (raw-window-handle-0.6.2/src/borrowed.rs) states its non-null
        // guarantee "only applies to *pointers*, and not any window ID
        // types in the handle", explaining that "it is possible for safe
        // code in the same process to delete the window" — `hwnd` here is
        // exactly such an ID (`Win32WindowHandle::hwnd: NonZeroIsize`, not a
        // pointer type), so a caller is required to handle `HandleError` on
        // every subsequent use rather than assume a handle it already holds
        // stays good.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for WindowsWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // wgpu 29.x fix: For multi-monitor support, ensure we return a valid display handle.
        // WindowsDisplayHandle::new() creates a valid default for Windows Display enumeration.
        // This helps wgpu locate the correct adapter/surface for the window's monitor.
        let handle = WindowsDisplayHandle::new();
        // SAFETY: `WindowsDisplayHandle` carries no fields (Windows has no
        // per-display native handle in this API) — there is nothing for
        // `borrow_raw` to invalidate; the call only exists to satisfy the
        // `raw-window-handle` trait's `unsafe fn` signature.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Windows(handle))
        })
    }
}

impl Clone for WindowsWindow {
    fn clone(&self) -> Self {
        Self {
            hwnd: self.hwnd,
            state: Arc::clone(&self.state),
            identity: self.identity,
            windows_map: Arc::clone(&self.windows_map),
            displays: Arc::clone(&self.displays),
            snapshot: Arc::clone(&self.snapshot),
            // Same window, same identity token.
            context: self.context,
            // The clone shares the same window, so it shares the same UIA
            // bridge — one subclass hook per HWND, never two.
            #[cfg(feature = "a11y")]
            accessibility: Arc::clone(&self.accessibility),
            // And its actuator, so a newer request replaces an older one
            // whichever wrapper made it.
            haptics: Arc::clone(&self.haptics),
            drag_source: Arc::clone(&self.drag_source),
        }
    }
}

impl WindowsWindow {
    /// Set DWM window attribute
    ///
    /// # Safety
    ///
    /// `value` must be a valid, initialized `T` whose size and layout match
    /// what `attribute` (a `DWMWINDOWATTRIBUTE` ordinal) expects DWM to read
    /// — the byte count passed to `DwmSetWindowAttribute` is derived from
    /// `size_of::<T>()`, so calling this with the wrong `T` for a given
    /// `attribute` has DWM read past `value`'s bytes.
    unsafe fn set_dwm_attribute<T>(&self, attribute: i32, value: &T) -> windows::core::Result<()> {
        // SAFETY: per the `# Safety` contract above, the caller guarantees
        // `T` matches `attribute`'s expected layout; `value` is a live `&T`
        // for the duration of this call, and `size_of::<T>()` is the
        // correct byte count for the pointer `std::ptr::from_ref` produces
        // from it.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

            DwmSetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(attribute),
                std::ptr::from_ref::<T>(value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<T>() as u32,
            )
        }
    }

    /// Get DWM window attribute
    ///
    /// # Safety
    ///
    /// `T` must match the layout DWM writes for `attribute` (a
    /// `DWMWINDOWATTRIBUTE` ordinal) — `size_of::<T>()` is the byte count
    /// passed to `DwmGetWindowAttribute`, so the wrong `T` for a given
    /// `attribute` has DWM write past `value`'s bytes.
    unsafe fn get_dwm_attribute<T: Default>(&self, attribute: i32) -> windows::core::Result<T> {
        // SAFETY: per the `# Safety` contract above, `T::default()` seeds a
        // valid, fully-initialized `value` before `&raw mut value` is handed
        // to DWM, so even if the call fails without writing anything, `value`
        // is never read uninitialized; the caller guarantees `T` matches
        // `attribute`'s expected layout and `size_of::<T>()`.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};

            let mut value = T::default();
            DwmGetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(attribute),
                (&raw mut value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<T>() as u32,
            )?;
            Ok(value)
        }
    }
}

// ============================================================================
// Windows Window Extension Trait Implementation
// ============================================================================

use super::window_ext::{
    TaskbarProgressState, WindowCornerPreference, WindowsBackdrop, WindowsTheme,
    WindowsWindowExt as WindowsWindowExtTrait, dwm_attributes,
};

impl WindowsWindowExtTrait for WindowsWindow {
    fn set_backdrop(&mut self, backdrop: WindowsBackdrop) {
        // SAFETY: `backdrop_value` is `i32`, matching `DWMWA_SYSTEMBACKDROP_TYPE`'s
        // expected layout, satisfying `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let backdrop_value = backdrop.to_dwm_value();
            if let Err(e) =
                self.set_dwm_attribute(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE, &backdrop_value)
            {
                tracing::warn!("Failed to set backdrop material: {:?}", e);
            } else {
                tracing::debug!("Set window backdrop to {:?}", backdrop);
            }
        }
    }

    fn clear_backdrop(&mut self) {
        self.set_backdrop(WindowsBackdrop::None);
    }

    fn backdrop(&self) -> WindowsBackdrop {
        // SAFETY: `i32` matches `DWMWA_SYSTEMBACKDROP_TYPE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE) {
                // Ok(1) is DWMSBT_NONE — covered by the fallback arm.
                Ok(2) => WindowsBackdrop::Mica,
                Ok(3) => WindowsBackdrop::Acrylic,
                Ok(4) => WindowsBackdrop::MicaAlt,
                _ => WindowsBackdrop::None,
            }
        }
    }

    fn enable_snap_layouts(&mut self) {
        // Snap Layouts are automatically enabled on Windows 11 if the window has
        // a standard maximize button. No explicit API call needed.
        // We just need to ensure WS_MAXIMIZEBOX is set
        //
        // SAFETY: see `PlatformWindow::is_resizable`/`set_resizable` above —
        // same `GetWindowLongPtrW`/`SetWindowLongPtrW`/`SetWindowPos` shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            style |= WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (enable snap layouts style refresh) failed");
            }

            tracing::debug!("Snap Layouts enabled (via WS_MAXIMIZEBOX)");
        }
    }

    fn disable_snap_layouts(&mut self) {
        // Disable by removing WS_MAXIMIZEBOX
        //
        // SAFETY: see `enable_snap_layouts` above — same call shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            style &= !WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd, ?error, "SetWindowPos (disable snap layouts style refresh) failed");
            }

            tracing::debug!("Snap Layouts disabled");
        }
    }

    fn is_snap_layouts_enabled(&self) -> bool {
        // SAFETY: see `PlatformWindow::is_resizable` above — same call shape.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            (style & WS_MAXIMIZEBOX.0) != 0
        }
    }

    fn set_corner_preference(&mut self, preference: WindowCornerPreference) {
        // SAFETY: `corner_value` is `i32`, matching
        // `DWMWA_WINDOW_CORNER_PREFERENCE`'s expected layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let corner_value = preference.to_dwm_value();
            if let Err(e) = self.set_dwm_attribute(
                dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_value,
            ) {
                tracing::warn!("Failed to set corner preference: {:?}", e);
            } else {
                tracing::debug!("Set corner preference to {:?}", preference);
            }
        }
    }

    fn corner_preference(&self) -> WindowCornerPreference {
        // SAFETY: `i32` matches `DWMWA_WINDOW_CORNER_PREFERENCE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE) {
                // Ok(0) is DWMCP_DEFAULT — covered by the fallback arm.
                Ok(1) => WindowCornerPreference::DoNotRound,
                Ok(2) => WindowCornerPreference::Round,
                Ok(3) => WindowCornerPreference::RoundSmall,
                _ => WindowCornerPreference::Default,
            }
        }
    }

    fn enable_blur_behind(&mut self, enable: bool) {
        use windows::Win32::Graphics::Dwm::{
            DWM_BB_ENABLE, DWM_BLURBEHIND, DwmEnableBlurBehindWindow,
        };

        // SAFETY: `bb` is a stack-local `DWM_BLURBEHIND`, fully initialized
        // above; `&raw const bb` gives `DwmEnableBlurBehindWindow` a valid
        // pointer to it — the API takes a typed struct pointer, not a
        // `(pointer, size)` pair, so there is no separate size argument to
        // get wrong.
        unsafe {
            let bb = DWM_BLURBEHIND {
                dwFlags: DWM_BB_ENABLE,
                fEnable: if enable { TRUE } else { FALSE },
                hRgnBlur: HRGN::default(),
                fTransitionOnMaximized: FALSE,
            };

            if let Err(e) = DwmEnableBlurBehindWindow(self.hwnd, &raw const bb) {
                tracing::warn!("Failed to enable blur behind: {:?}", e);
            } else {
                tracing::debug!("Blur behind: {}", enable);
            }
        }
    }

    fn set_taskbar_progress(&mut self, state: TaskbarProgressState, progress: u32) {
        // This requires ITaskbarList3 COM interface
        // For now, just log - full implementation would need COM integration
        tracing::debug!("Set taskbar progress: {:?} {}%", state, progress);

        // TODO: Implement ITaskbarList3::SetProgressState and SetProgressValue
        // This requires:
        // 1. CoCreateInstance for ITaskbarList3
        // 2. Call SetProgressState(hwnd, state)
        // 3. Call SetProgressValue(hwnd, progress, 100)
    }

    fn clear_taskbar_progress(&mut self) {
        self.set_taskbar_progress(TaskbarProgressState::NoProgress, 0);
    }

    fn set_dark_mode(&mut self, dark_mode: bool) {
        // An explicit choice outlives later system theme changes.
        super::platform::with_window_context(self.hwnd, "set_dark_mode", |ctx| {
            ctx.chrome_follows_system.set(false);
        });
        // SAFETY: `dark_mode_value` is `i32`, matching
        // `DWMWA_USE_IMMERSIVE_DARK_MODE`'s expected layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let dark_mode_value: i32 = i32::from(dark_mode);
            if let Err(e) = self.set_dwm_attribute(
                dwm_attributes::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode_value,
            ) {
                tracing::warn!("Failed to set dark mode: {:?}", e);
            } else {
                tracing::debug!("Set dark mode: {}", dark_mode);
            }
        }
    }

    fn is_dark_mode(&self) -> bool {
        // SAFETY: `i32` matches `DWMWA_USE_IMMERSIVE_DARK_MODE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_USE_IMMERSIVE_DARK_MODE)
                .unwrap_or(0)
                != 0
        }
    }

    fn set_theme(&mut self, theme: WindowsTheme) {
        if let Some(dark_mode) = theme.to_dark_mode_value() {
            self.set_dark_mode(dark_mode);
        } else {
            // Back to the system theme, now and on each later change.
            super::platform::with_window_context(self.hwnd, "set_theme", |ctx| {
                ctx.chrome_follows_system.set(true);
            });
            super::appearance::apply_chrome(self.hwnd, super::appearance::system_appearance());
        }
    }

    fn theme(&self) -> WindowsTheme {
        if self.is_dark_mode() {
            WindowsTheme::Dark
        } else {
            WindowsTheme::Light
        }
    }

    fn set_has_shadow(&mut self, has_shadow: bool) {
        // Windows doesn't have a direct API to disable shadows
        // Shadows are controlled by DWM composition
        // We can try extended window styles, but this is limited
        tracing::debug!("set_has_shadow: {} (limited support)", has_shadow);
    }

    fn set_title_bar_color(&mut self, color: Option<(u8, u8, u8)>) {
        // SAFETY: both `colorref` and `default_color` are `u32`, matching
        // `DWMWA_CAPTION_COLOR`'s expected `COLORREF` layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            if let Some((r, g, b)) = color {
                // Windows expects COLORREF format: 0x00BBGGRR
                let colorref: u32 = ((b as u32) << 16) | ((g as u32) << 8) | (r as u32);

                if let Err(e) =
                    self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &colorref)
                {
                    tracing::warn!("Failed to set title bar color: {:?}", e);
                } else {
                    tracing::debug!("Set title bar color: RGB({}, {}, {})", r, g, b);
                }
            } else {
                // Reset to default (0xFFFFFFFF means use default)
                let default_color: u32 = 0xFFFF_FFFF;
                if let Err(error) =
                    self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &default_color)
                {
                    tracing::warn!(
                        hwnd = ?self.hwnd,
                        ?error,
                        "failed to reset title bar color to default"
                    );
                }
            }
        }
    }

    fn set_caption_color(&mut self, color: Option<(u8, u8, u8)>) {
        // Caption color is the same as title bar color in Windows 11
        self.set_title_bar_color(color);
    }

    fn set_animations_enabled(&mut self, enabled: bool) {
        // Windows animations are typically controlled system-wide
        // Per-window animation control is limited
        tracing::debug!("set_animations_enabled: {} (system-wide setting)", enabled);
    }

    fn dpi(&self) -> u32 {
        // SAFETY: `GetDpiForWindow` takes `self.hwnd` by value, no pointer
        // arguments — a stale or invalid `hwnd` cannot cause UB here, it is
        // just an ordinary FFI call either way. NOT a "safe fallback",
        // though: per its documented contract, an invalid `hwnd` makes this
        // return a literal `0`, not some usable default DPI — a caller that
        // divides by this result (e.g. computing a scale factor) would get
        // infinity or NaN, not graceful degradation. No current caller in
        // this crate divides by `dpi()`'s result, but a future one should
        // not assume `0` means "use 96 instead".
        unsafe { GetDpiForWindow(self.hwnd) }
    }

    fn convert_point_from_device(&self, point: Point<i32>) -> Point<f64> {
        let scale = self.scale_factor();
        Point::new(point.x as f64 / scale, point.y as f64 / scale)
    }

    fn convert_point_to_device(&self, point: Point<f64>) -> Point<i32> {
        let scale = self.scale_factor();
        Point::new(
            (point.x * scale).round() as i32,
            (point.y * scale).round() as i32,
        )
    }
}

impl Drop for WindowsWindow {
    fn drop(&mut self) {
        // Only destroy if this is the last reference
        if Arc::strong_count(&self.state) == 1 {
            tracing::debug!("Destroying window HWND {:?}", self.hwnd);

            use crate::shared::hwnd_affinity::TeardownRoute;
            match super::platform::teardown_route(self.hwnd, self.context) {
                TeardownRoute::DestroyDirect => {
                    // Unhook the UIA subclass BEFORE destroying the window —
                    // Win32 wants subclasses removed while the window still
                    // exists, and this is the owner-thread teardown path
                    // subclassing requires. Any capability `Arc` still held
                    // elsewhere degrades to a no-op.
                    #[cfg(feature = "a11y")]
                    self.accessibility.shutdown();

                    // SAFETY: `DestroyWindow` takes `self.hwnd` by value;
                    // the route just established this is the owning thread
                    // and that the handle still names THIS wrapper's window
                    // (a never-created or already-destroyed handle routes
                    // `AlreadyGone`, a recycled one `StaleHandle`), where
                    // `WM_DESTROY` (handled in `platform.rs`) retires the
                    // `GWLP_USERDATA` allocation synchronously within this
                    // call, the ledger freeing it at the outermost borrow
                    // release.
                    unsafe {
                        if let Err(error) = DestroyWindow(self.hwnd) {
                            tracing::warn!(hwnd = ?self.hwnd, ?error, "DestroyWindow failed in Drop");
                        }
                    }
                }
                TeardownRoute::PostClose => {
                    // Off the owning thread `DestroyWindow` is forbidden and
                    // unhooking the UIA subclass would race the owner's
                    // message dispatch, so neither runs here: the close is
                    // posted to the owner's queue, and the subclass adapter
                    // is left for `WindowsAccessibility`'s own drop guard
                    // (which leaks it, with a warning, rather than unhook
                    // cross-thread).
                    //
                    // SAFETY: `PostMessageW` takes the handle and plain
                    // integers by value — nothing dereferenced.
                    unsafe {
                        if let Err(error) =
                            PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
                        {
                            tracing::warn!(hwnd = ?self.hwnd, ?error, "PostMessageW(WM_CLOSE) (cross-thread Drop) failed");
                        }
                    }
                }
                TeardownRoute::AlreadyGone => {
                    tracing::debug!(hwnd = ?self.hwnd, "Drop: the native window is already gone");
                }
                TeardownRoute::StaleHandle => {
                    tracing::debug!(
                        hwnd = ?self.hwnd,
                        "Drop: the handle no longer names this wrapper's window \
                         (recycled by the OS); leaving its new owner untouched"
                    );
                }
            }

            // Remove from windows map
            let hwnd_key = self.hwnd.0 as isize;
            let _prev = {
                let mut windows = self.windows_map.lock();
                if windows
                    .get(&hwnd_key)
                    .is_some_and(|window| window.identity == self.identity)
                {
                    windows.remove(&hwnd_key)
                } else {
                    None
                }
            };
        }
    }
}

#[cfg(test)]
mod open_tests {
    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{
        DisplayInfo, DisplayKey, DisplayRegistry, WindowCommand, WindowGeometry, WindowMode,
        WindowOptions,
    };

    use super::open_spot;
    use crate::platform::Platform as _;

    const FRAME: (i32, i32) = (16, 39);

    fn with_frame(_dpi: u32, content: (i32, i32)) -> (i32, i32) {
        (content.0 + FRAME.0, content.1 + FRAME.1)
    }

    fn display(scale: f64, origin: (i32, i32)) -> DisplayInfo {
        let registry = DisplayRegistry::new();
        let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
        DisplayInfo::new(
            id,
            "d",
            Bounds::new(Point::new(origin.0, origin.1), Size::new(3840, 2160)),
            scale,
        )
    }

    fn the_size_is_the_content_and_the_frame_is_added() {
        let options = WindowOptions::default().with_size(Size::new(800.0, 600.0));
        let spot = open_spot(&options, None, 1.0, with_frame);
        assert_eq!(spot.outer, (816, 639));
        assert_eq!(spot.origin, None);
    }

    fn a_position_converts_through_the_named_displays_own_scale() {
        // A 2x display that starts at device x 1920: 100 logical is 200 device
        // pixels from its origin, whatever the system scale is.
        let target = display(2.0, (1920, 0));
        let options = WindowOptions::default()
            .with_size(Size::new(800.0, 600.0))
            .with_position(Some(Point::new(100.0, 50.0)));
        let spot = open_spot(&options, Some(&target), 1.0, with_frame);
        assert_eq!(spot.scale, 2.0);
        assert_eq!(spot.outer, (1600 + 16, 1200 + 39));
        assert_eq!(spot.origin, Some((1920 + 200, 100)));
    }

    fn without_a_display_the_position_is_global_at_the_system_scale() {
        let options = WindowOptions::default().with_position(Some(Point::new(100.0, 50.0)));
        let spot = open_spot(&options, None, 1.5, with_frame);
        assert_eq!(spot.origin, Some((150, 75)));
    }

    fn the_normal_rectangle_is_the_outer_geometry_it_returns_to() {
        let target = display(1.0, (0, 0));
        let options = WindowOptions::default()
            .with_mode(WindowMode::Maximized)
            .with_normal_geometry(Some(WindowGeometry::new(
                Some(Point::new(100.0, 80.0)),
                Size::new(700.0, 500.0),
            )));
        let spot = open_spot(&options, Some(&target), 1.0, with_frame);
        assert_eq!(spot.normal, Some((100, 80, 800, 580)));
    }

    fn a_normal_rectangle_without_a_position_follows_the_windows_own() {
        let options = WindowOptions::default()
            .with_position(Some(Point::new(30.0, 40.0)))
            .with_normal_geometry(Some(WindowGeometry::new(None, Size::new(300.0, 200.0))));
        let spot = open_spot(&options, None, 1.0, with_frame);
        assert_eq!(spot.normal, Some((30, 40, 330, 240)));
        let unplaced = WindowOptions::default()
            .with_normal_geometry(Some(WindowGeometry::new(None, Size::new(300.0, 200.0))));
        assert_eq!(open_spot(&unplaced, None, 1.0, with_frame).normal, None);
    }

    fn an_opening_rectangle_at_the_edge_of_the_range_saturates() {
        let options = WindowOptions::default()
            .with_mode(WindowMode::Maximized)
            .with_normal_geometry(Some(WindowGeometry::new(
                Some(Point::new(3.0e9, 3.0e9)),
                Size::new(700.0, 500.0),
            )));
        let spot = open_spot(&options, None, 1.0, with_frame);
        assert_eq!(
            spot.normal,
            Some((i32::MAX, i32::MAX, i32::MAX, i32::MAX)),
            "no wrap into a rectangle on the other side of the desktop"
        );
    }

    #[test]
    fn open_geometry() {
        crate::table_test::run_table(
            "open_geometry",
            &[
                (
                    "the_size_is_the_content_and_the_frame_is_added",
                    the_size_is_the_content_and_the_frame_is_added as fn(),
                ),
                (
                    "a_position_converts_through_the_named_displays_own_scale",
                    a_position_converts_through_the_named_displays_own_scale as fn(),
                ),
                (
                    "without_a_display_the_position_is_global_at_the_system_scale",
                    without_a_display_the_position_is_global_at_the_system_scale as fn(),
                ),
                (
                    "the_normal_rectangle_is_the_outer_geometry_it_returns_to",
                    the_normal_rectangle_is_the_outer_geometry_it_returns_to as fn(),
                ),
                (
                    "an_opening_rectangle_at_the_edge_of_the_range_saturates",
                    an_opening_rectangle_at_the_edge_of_the_range_saturates as fn(),
                ),
                (
                    "a_normal_rectangle_without_a_position_follows_the_windows_own",
                    a_normal_rectangle_without_a_position_follows_the_windows_own as fn(),
                ),
            ],
        );
    }

    fn near(actual: f64, wanted: f64) -> bool {
        (actual - wanted).abs() <= 2.0
    }

    /// A real window opened with a content size reports that content size:
    /// the frame is added to the outer window, not taken out of the content.
    fn a_real_window_opens_with_the_content_size_it_asked_for() {
        let platform = crate::WindowsPlatform::new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .with_size(Size::new(640.0, 480.0))
                    .with_visible(false),
            )
            .expect("window");
        let state = window.state();
        assert!(
            near(state.content_size.width, 640.0) && near(state.content_size.height, 480.0),
            "content {:?}",
            state.content_size
        );
        assert!(
            state.outer.size.width > state.content_size.width
                && state.outer.size.height > state.content_size.height,
            "the outer window carries a frame"
        );
    }

    /// A size set on a maximized window is the size it returns to: the window
    /// stays maximized and restores to the content size asked for.
    fn a_size_set_while_maximized_is_the_size_restored_to() {
        let platform = crate::WindowsPlatform::new().expect("platform");
        let window = platform
            .open_window(WindowOptions::default().with_visible(false))
            .expect("window");
        window
            .apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("maximize");
        window
            .apply(WindowCommand::SetSize(Size::new(500.0, 400.0)))
            .expect("resize while maximized");
        assert_eq!(
            window.state().mode,
            WindowMode::Maximized,
            "still maximized"
        );
        window
            .apply(WindowCommand::SetMode(WindowMode::Normal))
            .expect("restore");
        let state = window.state();
        assert!(
            near(state.content_size.width, 500.0) && near(state.content_size.height, 400.0),
            "restored to {:?}",
            state.content_size
        );
    }

    /// A window that opens maximized returns to the rectangle it was given.
    fn a_window_opened_maximized_returns_to_its_normal_rectangle() {
        let platform = crate::WindowsPlatform::new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .with_mode(WindowMode::Maximized)
                    .with_normal_geometry(Some(WindowGeometry::new(
                        Some(Point::new(120.0, 100.0)),
                        Size::new(700.0, 500.0),
                    ))),
            )
            .expect("window");
        assert_eq!(window.state().mode, WindowMode::Maximized);
        window
            .apply(WindowCommand::SetMode(WindowMode::Normal))
            .expect("restore");
        let outer = window.state().outer;
        assert!(
            near(outer.size.width, 700.0) && near(outer.size.height, 500.0),
            "returned to {outer:?}"
        );
        let at = outer.position.expect("a Win32 window reports its position");
        assert!(near(at.x, 120.0) && near(at.y, 100.0), "at {at:?}");
    }

    /// Maximized and fullscreen windows both report the rectangle they return
    /// to, taken from Win32's own `rcNormalPosition`: entering fullscreen from
    /// a maximized window does not make the maximized rectangle the one to
    /// return to.
    fn maximized_and_fullscreen_windows_report_their_normal_rectangle() {
        let platform = crate::WindowsPlatform::new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .with_size(Size::new(640.0, 480.0))
                    .with_position(Some(Point::new(150.0, 120.0)))
                    .with_visible(false),
            )
            .expect("window");
        let normal = window.state().outer;
        let same_as_normal = |state: flui_platform::WindowState, what: &str| {
            let reported = state
                .normal_geometry
                .unwrap_or_else(|| panic!("{what} reports the rectangle it returns to"));
            assert!(
                near(reported.size.width, normal.size.width)
                    && near(reported.size.height, normal.size.height),
                "{what}: {reported:?} is not {normal:?}"
            );
            let (at, was) = (
                reported.position.expect("a position"),
                normal.position.expect("a position"),
            );
            assert!(
                near(at.x, was.x) && near(at.y, was.y),
                "{what}: at {at:?}, was {was:?}"
            );
        };

        window
            .apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("maximize");
        same_as_normal(window.state(), "a maximized window");
        window
            .apply(WindowCommand::SetMode(WindowMode::Fullscreen))
            .expect("fullscreen");
        same_as_normal(window.state(), "a fullscreen window entered from maximized");
    }

    #[test]
    fn open_real_window() {
        crate::table_test::run_table(
            "open_real_window",
            &[
                (
                    "maximized_and_fullscreen_windows_report_their_normal_rectangle",
                    maximized_and_fullscreen_windows_report_their_normal_rectangle as fn(),
                ),
                (
                    "a_real_window_opens_with_the_content_size_it_asked_for",
                    a_real_window_opens_with_the_content_size_it_asked_for as fn(),
                ),
                (
                    "a_size_set_while_maximized_is_the_size_restored_to",
                    a_size_set_while_maximized_is_the_size_restored_to as fn(),
                ),
                (
                    "a_window_opened_maximized_returns_to_its_normal_rectangle",
                    a_window_opened_maximized_returns_to_its_normal_rectangle as fn(),
                ),
            ],
        );
    }
}
