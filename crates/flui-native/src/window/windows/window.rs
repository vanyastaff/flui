//! [`WindowsWindow`], the handle the Win32 host hands out: how a window
//! opens, what its commands ask of the OS, and its handles for the renderer.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, Point, Size};
use parking_lot::Mutex;
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, Win32WindowHandle,
    WindowsDisplayHandle,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::{ClientToScreen, InvalidateRect, ScreenToClient, UpdateWindow},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForSystem, GetDpiForWindow},
            WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, GCLP_HBRBACKGROUND, GWL_EXSTYLE,
                GWL_STYLE, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, GetWindowRect,
                IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_NO, IDC_SIZEALL,
                IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT, IsIconic,
                IsWindowVisible, IsZoomed, LoadCursorW, PostMessageW, SW_HIDE, SW_MAXIMIZE,
                SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetClassLongPtrW, SetCursor, SetForegroundWindow, SetWindowLongPtrW,
                SetWindowPlacement, SetWindowPos, SetWindowTextW, ShowWindow, WM_CLOSE, WS_CAPTION,
                WS_EX_APPWINDOW, WS_MAXIMIZE,
            },
        },
    },
    core::HSTRING,
};

use flui_platform::{
    AdmittedCommand, ApplyError, CursorError, PlatformFailure, PlatformWindow, SizeConstraints,
    Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand, WindowId, WindowMaterial,
    WindowMode, WindowOptions, WindowReveal, WindowState,
};

use flui_platform::os_options::WindowsCornerPreference;

use super::material::SystemBackdrop;
use super::mode::NativeMode;
use super::proc::{
    FirstShow, OwnerTracking, REQUEST_FRAME, WindowRecord, first_show_command, frame_style,
    restyled, sample_state, to_screen, to_workspace,
};
use super::set::{
    USER_DEFAULT_SCREEN_DPI, WindowSet, checked_logical_to_device, open_spot, outer_size,
    placement_of, restore_rectangle, workspace_origin,
};
use crate::host::WindowFacets;
use crate::owner::windows::WINDOW_CLASS_NAME;
use crate::text::WindowTextInput;
use crate::version::gate::Since;
use crate::window::state::WindowStateCell;
use crate::{
    owner::OpenWindowError, owner::window_delivery::WindowDelivery, owner::windows::AppHooks,
};

/// Windows window wrapper
pub struct WindowsWindow {
    /// Native window handle
    hwnd: HWND,

    /// Window state
    state: Arc<Mutex<WindowRecord>>,

    /// Which native window this wrapper was built for.
    pub(crate) identity: crate::owner::windows::WindowIdentity,

    /// The windows the platform tracks, which this one leaves when the last
    /// wrapper drops.
    windows: Arc<WindowSet>,

    /// The platform's monitors, by which this window names its display.
    displays: Arc<dyn crate::owner::windows::HostDisplays>,

    /// The published window state (ADR-0237), shared with the window's
    /// owner-thread context, which refreshes it from the window messages.
    snapshot: Arc<WindowStateCell>,

    /// The context allocation this wrapper installed in `hwnd`'s
    /// `GWLP_USERDATA` slot at creation, kept ONLY as an identity token:
    /// teardown compares it (as a value, never dereferencing through this
    /// field) against the window's current slot so a wrapper that outlived
    /// its native window cannot destroy or close whatever unrelated window
    /// the OS recycled the handle value for — see
    /// [`teardown_route`](super::proc::teardown_route). Dangling as a
    /// VALUE once `WM_DESTROY` has run; that is fine for comparison and
    /// exactly why it must never be dereferenced here.
    context: *const super::proc::WindowContext,

    /// What the table's positions made for this window at construction: its
    /// accessibility bridge (UIA subclasses `hwnd` then, so a client that
    /// asks at any later point is answered; detached before the window is
    /// destroyed), its actuator (the pen in range of it), its handle to its
    /// TSF connection, and its system chrome (none on Windows).
    facets: WindowFacets,

    /// Starts this window's OS drag sessions through OLE (ADR-0211).
    drag_source: Arc<dyn flui_platform::drag::PlatformDragSource>,

    /// What the window delivers to its handler, shared with its owner-thread
    /// context, which raises every delivery from the window procedure. The
    /// handler itself lives in an owner-thread slot of it, so this field
    /// reaches nothing off that thread.
    delivery: Arc<WindowDelivery>,
}

impl std::fmt::Debug for WindowsWindow {
    // Hand-written: the window's handler has no meaningful
    // Debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsWindow")
            .field("hwnd", &self.hwnd.0)
            .finish_non_exhaustive()
    }
}

/// A Win32 call that failed, as the error of the command that made it.
fn backend_error(what: &str, error: windows::core::Error) -> ApplyError {
    ApplyError::failed(format!("{what} failed: {error}"))
}

/// A command's logical pair in device pixels, refused before any call when
/// either value is beyond what Win32 can represent: admission held the numbers
/// to being finite and positive, and the device range is Win32's own limit.
fn device_pair(x: f64, y: f64, scale: f64) -> Result<(i32, i32), ApplyError> {
    checked_logical_to_device(x, scale)
        .zip(checked_logical_to_device(y, scale))
        .ok_or(ApplyError::Platform(PlatformFailure::OutOfRange))
}

/// What every Win32 window honours and observes, known before one exists
/// (`Platform::window_capabilities`), on a build that passed or failed the
/// material's `gate`. Win32 has no fullscreen of its own: the host makes one
/// of a borderless window sized to its monitor. It takes no refresh-rate
/// request: the display's rate is the user's setting. The material is
/// `DWMWA_SYSTEMBACKDROP_TYPE`; a Win32 window opens with
/// [`WindowMaterial::System`] where the build draws it.
pub(crate) const fn window_capabilities(
    gate: &Result<Since<SystemBackdrop>, Unsupported>,
) -> WindowCapabilities {
    use flui_platform::Availability::{Emulated, Native};
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_title = Native;
    capabilities.sets_position = Native;
    capabilities.sets_size = Native;
    capabilities.sets_constraints = Native;
    capabilities.sets_resizable = Native;
    capabilities.sets_decorated = Native;
    capabilities.minimizes = Native;
    capabilities.maximizes = Native;
    capabilities.enters_fullscreen = Emulated;
    capabilities.shows = Native;
    capabilities.hides = Native;
    capabilities.activates = Native;
    capabilities.closes = Native;
    capabilities.observes_position = Native;
    capabilities.observes_minimized = Native;
    capabilities.places_at_open = Native;
    capabilities.material = Native;
    super::material::below_the_gate(capabilities, gate)
}

impl WindowsWindow {
    /// Create a new Windows window. Crate-internal: windows are opened
    /// through [`Window::open`](crate::host::Window::open), which refuses a
    /// call off the owner thread and hands over the platform's own handler
    /// set.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when Win32 window creation fails.
    pub(crate) fn new(
        options: WindowOptions,
        handler: Rc<dyn flui_platform::WindowHandler>,
        windows: Arc<WindowSet>,
        shares: crate::owner::windows::control::OwnerShares,
        apartment: Option<Rc<crate::owner::windows::com_apartment::ComApartment>>,
        drag: Arc<dyn crate::owner::windows::HostDrag>,
    ) -> Result<Rc<Self>, OpenWindowError> {
        let crate::owner::windows::control::OwnerShares {
            app_hooks,
            frames,
            exit_policy,
            displays,
            notices,
            window_hooks,
            preferences,
            ..
        } = shares;
        // Admission refuses identity exhaustion before creating an HWND
        // whose ownership has not yet transferred to a context and wrapper.
        let (identity, created) = crate::owner::windows::WindowIdentity::admit(
            crate::owner::windows::WindowIdentity::source(),
            || Self::create_native(&options, displays.as_ref()),
        );
        let (hwnd, requested, scale_factor, normal) = created?;
        Self::install_native(
            options,
            handler,
            windows,
            app_hooks,
            frames,
            exit_policy,
            displays,
            notices,
            identity,
            hwnd,
            requested,
            scale_factor,
            normal,
            apartment,
            drag,
            window_hooks,
            preferences,
        )
    }

    /// Create the native window: the resource an admitted identity owns.
    /// Returns the HWND with the requested outer device size, the scale factor
    /// the sizes were computed at and, for a window that opens maximized or
    /// fullscreen, the rectangle it returns to (screen coordinates).
    fn create_native(
        options: &WindowOptions,
        displays: &dyn crate::owner::windows::HostDisplays,
    ) -> Result<(HWND, Size<i32>, f64, Option<RECT>), OpenWindowError> {
        // SAFETY: `GetModuleHandleW(None)` queries the current process image
        // and takes no pointer arguments — always sound. `GetDpiForSystem`
        // reads global state, no preconditions. `CreateWindowExW` requires
        // `WINDOW_CLASS_NAME` to already be registered, which
        // `WindowsLoop::new` guarantees before any `open_window`
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
        handler: Rc<dyn flui_platform::WindowHandler>,
        windows: Arc<WindowSet>,
        app_hooks: Rc<RefCell<AppHooks>>,
        frames: Rc<crate::owner::windows::FrameCount>,
        exit_policy: crate::owner::windows::control::ExitPolicyRequest,
        displays: Arc<dyn crate::owner::windows::HostDisplays>,
        notices: Arc<dyn crate::owner::notices::NoticeChannel>,
        identity: crate::owner::windows::WindowIdentity,
        hwnd: HWND,
        requested: Size<i32>,
        scale_factor: f64,
        normal: Option<RECT>,
        apartment: Option<Rc<crate::owner::windows::com_apartment::ComApartment>>,
        drag: Arc<dyn crate::owner::windows::HostDrag>,
        window_hooks: crate::owner::windows::WindowHooks,
        preferences: Rc<dyn crate::owner::windows::control::OwnerPreferences>,
    ) -> Result<Rc<Self>, OpenWindowError> {
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

            // What the window opens with on Windows: its own options, or
            // what a window gets without them.
            let os = options.os_options.windows.clone().unwrap_or_default();
            Self::apply_windows_features(hwnd, windows.material, os.corner_preference);
            // One sample seeds both the chrome and the change filter, so the
            // first theme change after creation is never swallowed.
            let appearance = super::backdrop::system_appearance();
            let (chrome, chrome_follows_system) = super::backdrop::opening_chrome(&os, appearance);
            // A build before Windows 10 20H1 refuses the attribute and draws
            // its own chrome; the window opens either way.
            let _ = super::backdrop::apply_chrome(hwnd, chrome);

            tracing::info!(
                "Created window HWND {:?} - {}x{} - scale: {}",
                hwnd,
                width,
                height,
                scale_factor
            );

            // Seed observations from the native client, not requested outer
            // bounds. The window, and every hook of its class, reads its
            // scale from this one value.
            let scale =
                crate::owner::windows::WindowScale::at_open(GetDpiForWindow(hwnd), scale_factor);
            let scale_factor = scale.scale();
            let mut client = RECT::default();
            let initial_size = if GetClientRect(hwnd, &raw mut client).is_ok() {
                Size::new(client.right - client.left, client.bottom - client.top)
            } else {
                Size::new(width, height)
            };
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &raw mut origin);

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
                    appearance,
                    active: false,
                    hovered: false,
                    occluded: false,
                    capabilities: windows.capabilities(),
                },
            )));

            // A wake marks the published state stale, so the first read after
            // the machine slept asks the system again.
            notices.mark_stale_on_wake(&snapshot);

            // Create and install the WindowContext for event dispatch
            // BEFORE building the wrapper: the wrapper keeps the pointer as
            // its teardown identity token (see the `context` field doc).
            use flui_foundation::geometry::Size;

            use super::proc::WindowContext;

            let window_id = WindowId(hwnd.0 as u64);
            let config = windows.config.clone();
            // The window class's input hook gives the capabilities of the
            // devices attached before the handler hears its snapshot.
            let delivery = Arc::new(WindowDelivery::counting_frames(
                handler,
                flui_platform::input_capabilities::InputCapabilities::UNAVAILABLE,
                frames.counter(),
            ));
            // What another thread raises for the window is delivered in a
            // turn of the window's own message queue.
            delivery.resume_through(super::proc::deliver_queued_turn(hwnd, identity));
            // Each fact the window reports, its input's hover included, is
            // published from the owner context of this very window before its
            // own callback runs: the context must carry this window's identity,
            // so a handle Win32 recycled for another window publishes nothing
            // of it. Every fact is dispatched from the window procedure, on the
            // owner thread; a call the context refuses (another thread, a
            // window already gone) still leaves the snapshot stale, so the
            // owner's next read publishes what changed.
            delivery.set_state_publisher({
                let address = hwnd.0 as isize;
                let cell = Arc::downgrade(&snapshot);
                Arc::new(move || {
                    let hwnd = HWND(address as *mut std::ffi::c_void);
                    let read = super::proc::with_window_context(hwnd, "publish state", |ctx| {
                        (ctx.identity == identity).then(|| ctx.read_window_state(hwnd))
                    });
                    match read {
                        Some(Some(outcome)) => outcome,
                        Some(None) => crate::owner::window_delivery::Publication::Unchanged,
                        None => {
                            if let Some(cell) = cell.upgrade() {
                                cell.invalidate();
                            }
                            crate::owner::window_delivery::Publication::Unchanged
                        }
                    }
                })
            });
            let context = Box::new(WindowContext {
                window_id,
                capabilities: windows.capabilities(),
                windows: Arc::downgrade(&windows),
                window_state: Arc::downgrade(&state),
                snapshot: Arc::clone(&snapshot),
                exit_policy,
                delivery: Arc::clone(&delivery),
                hooks: Rc::clone(&window_hooks),
                preferences: Rc::clone(&preferences),
                scale: scale.clone(),
                mode: std::cell::Cell::new(NativeMode::Normal),
                last_size: std::cell::Cell::new(initial_size),
                config,
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
                show_requests: std::cell::Cell::new(0),
                visibility: super::proc::Visibility::new(),
                cloak_watch: super::proc::CloakWatch::start(),
                in_size_move: std::cell::Cell::new(false),
                last_resize: std::cell::Cell::new(None),
                modal_pacing: crate::owner::windows::modal_loop::ModalPacing::new(
                    crate::owner::windows::modal_loop::ModalFrames::new(
                        std::sync::Arc::clone(&app_hooks.borrow().app),
                        Arc::downgrade(&windows)
                            as std::sync::Weak<dyn crate::owner::windows::ClassWindows>,
                        Rc::clone(&frames),
                    ),
                ),
                ledger: std::cell::RefCell::new(crate::owner::hwnd_affinity::ContextLedger::new()),
                identity,
                displays: Arc::clone(&displays),
                appearance: std::cell::Cell::new(appearance),
                active: std::cell::Cell::new(false),
                chrome_follows_system: std::cell::Cell::new(chrome_follows_system),
            });
            let context_ptr = Box::into_raw(context);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, context_ptr as isize);
            let this = super::proc::WindowRef::new(hwnd, identity);
            let positions = windows.positions;

            let window = Rc::new(Self {
                hwnd,
                state,
                identity,
                windows,
                displays,
                snapshot,
                context: context_ptr,
                // On the creating (owning) thread, as Win32 subclassing
                // requires; `hwnd` was validated just above.
                facets: positions.for_window(&this),
                drag_source: Arc::clone(&drag).drag_source(this),
                delivery: Arc::clone(&delivery),
            });
            // The wrapper owns the window from here: a panic out of a hook
            // (or anything after it) drops it, which destroys the window and
            // retires its context.

            // The hooks of the window class learn of the window: its text
            // services activate, its input is kept, it takes drags from this
            // process or another (ADR-0211), and the precision touchpad's
            // viewport is created.
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
                &window_hooks,
                &crate::owner::windows::CreatedWindow {
                    hwnd,
                    identity,
                    client_size: size,
                    scale,
                    delivery,
                    app_hooks: Rc::clone(&app_hooks),
                    preferences,
                    hooks: Rc::clone(&window_hooks),
                    apartment,
                    drag,
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
        let mode = super::proc::with_window_context(self.hwnd, "show_first", |ctx| {
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
        super::proc::with_window_context(self.hwnd, "show_first", |ctx| {
            ctx.begin_first_show(mode);
        });
        self.show_native(first_show_command(mode));
        // The show can have run user callbacks that closed this window and
        // opened another on the same handle: what follows is for this one only.
        if !self.owns_handle() {
            return;
        }
        super::proc::with_window_context(self.hwnd, "show_first", |ctx| {
            ctx.settle_shown();
        });
        // SAFETY: a plain call on this wrapper's window handle.
        if let Err(error) = unsafe { UpdateWindow(self.hwnd) }.ok() {
            tracing::warn!(hwnd = ?self.hwnd, ?error, "UpdateWindow failed after the first show");
        }
    }

    /// Note that the app asked for the window to be shown.
    fn note_show_request(&self) {
        super::proc::with_window_context(self.hwnd, "note_show_request", |ctx| {
            ctx.note_show_request();
        });
    }

    /// Whether the handle still names this wrapper's window: after a call that
    /// can run user callbacks, the window may have closed and another taken its
    /// handle.
    pub(crate) fn owns_handle(&self) -> bool {
        handle_available(super::proc::teardown_route(self.hwnd, self.context))
            && super::proc::with_window_context(self.hwnd, "owns_handle", |context| {
                context.identity == self.identity
            }) == Some(true)
    }

    /// What the desktop window manager draws for a window as it opens: the
    /// frame extended into the client area, so a material shows wherever the
    /// content is transparent; [`WindowMaterial::System`] where the build
    /// draws one (`material`), the material a Win32 window opens with; and the
    /// corners `corners` asks for.
    ///
    /// Each is cosmetic and the window opens without it: a build that
    /// refuses one keeps its own drawing, and the window's capabilities
    /// already say whether it draws a material.
    fn apply_windows_features(
        hwnd: HWND,
        material: Result<Since<SystemBackdrop>, Unsupported>,
        corners: WindowsCornerPreference,
    ) {
        use windows::Win32::{Graphics::Dwm::DwmExtendFrameIntoClientArea, UI::Controls::MARGINS};

        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        // SAFETY: `hwnd` is the window `CreateWindowExW` just returned on this
        // thread (the caller, `WindowsWindow::new`); the call reads the
        // stack-local `margins` through the typed pointer for its duration.
        let _ = unsafe { DwmExtendFrameIntoClientArea(hwnd, &raw const margins) };
        if let Ok(gate) = material {
            let _ = super::backdrop::apply_material(hwnd, gate, WindowMaterial::System);
        }
        let _ = super::backdrop::apply_corners(hwnd, corners);
    }

    /// Get the native HWND handle
    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// What other threads hold of this window: its surface and its redraw
    /// requests.
    fn surface(&self) -> Win32Surface {
        Win32Surface {
            hwnd: self.hwnd.0 as isize,
            context: self.context as isize,
            retirement: self.delivery.retirement(),
        }
    }

    /// Whether the build the platform runs on draws a system backdrop: the
    /// one gate every write of `DWMWA_SYSTEMBACKDROP_TYPE` takes.
    pub(super) fn backdrop_gate(&self) -> Result<Since<SystemBackdrop>, Unsupported> {
        self.windows.material
    }

    /// Whether the mouse is over this window, as its input read it: `None`
    /// off its owner thread, or once it closed.
    fn hovered(&self) -> Option<bool> {
        crate::owner::windows::window_inputs()?
            .hovered(super::proc::WindowRef::new(self.hwnd, self.identity))
    }

    /// Get current window bounds
    pub fn bounds(&self) -> Bounds<f64> {
        let state = self.state.lock();
        Bounds {
            origin: state.bounds.origin,
            size: state.bounds.size,
        }
    }

    pub(crate) fn show_native(
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
    /// [`ApplyError::Platform`] for a size beyond what Win32
    /// can represent, before any call; [`ApplyError::Platform`] when
    /// Win32 refuses or fails the resize.
    fn set_inner_size(&self, size: Size<f64>) -> Result<(), ApplyError> {
        let scale = self.state.lock().scale_factor;
        let content = device_pair(size.width, size.height, scale)?;
        let fullscreen = super::proc::with_window_context(self.hwnd, "set_inner_size", |ctx| {
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
                super::proc::with_window_context(self.hwnd, "set_inner_size", |ctx| {
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
    /// [`ApplyError::Platform`] for a position beyond what
    /// Win32 can represent, before any call; [`ApplyError::Platform`]
    /// when Win32 refuses or fails the move.
    fn set_outer_position(&self, at: Point<f64>) -> Result<(), ApplyError> {
        let scale = self.state.lock().scale_factor;
        let (x, y) = device_pair(at.x, at.y, scale)?;
        let fullscreen = super::proc::with_window_context(self.hwnd, "set_outer_position", |ctx| {
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
        let kept = super::proc::with_window_context(self.hwnd, "set_constraints", |ctx| {
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
    /// [`ApplyError::Platform`] when Win32 refuses or fails the change.
    fn restyle_frame(&self, decorated: bool, resizable: bool) -> Result<(), ApplyError> {
        let fullscreen = super::proc::with_window_context(self.hwnd, "restyle_frame", |ctx| {
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
        super::proc::with_window_context(
            self.hwnd,
            "restyle_frame",
            |ctx| -> Result<(), ApplyError> {
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
        .unwrap_or(Err(ApplyError::Closed))
    }

    /// Whether the window waits for its first presented frame to be shown.
    fn awaiting_reveal(&self) -> bool {
        super::proc::with_window_context(self.hwnd, "awaiting_reveal", |ctx| {
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
    /// [`ApplyError::Platform`] for a mode this backend does not know.
    fn set_mode(&self, mode: WindowMode) -> Result<(), ApplyError> {
        if !matches!(
            mode,
            WindowMode::Normal
                | WindowMode::Minimized
                | WindowMode::Maximized
                | WindowMode::Fullscreen
        ) {
            return Err(crate::window::state::not_carried_out(
                &WindowCommand::SetMode(mode),
            ));
        }
        if self.awaiting_reveal() {
            self.set_fullscreen(mode == WindowMode::Fullscreen);
            super::proc::with_window_context(self.hwnd, "stage_mode", |ctx| {
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
        super::proc::with_window_context(self.hwnd, "set_mode", |ctx| {
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
        super::proc::with_window_context(self.hwnd, "hide", |ctx| {
            // The first frame no longer shows it; an explicit show still does.
            ctx.first_show
                .set(ctx.first_show.get().without_frame_wait());
        });
        // A window hidden before it was ever shown sends no `WM_SHOWWINDOW`:
        // the record above is what its snapshot reads.
    }

    /// Get current scale factor
    pub fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
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
        super::proc::with_window_context(self.hwnd, "is_fullscreen", |ctx| {
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
            super::proc::with_window_context(self.hwnd, "set_fullscreen", |ctx| {
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
    pub(crate) fn redraw_native(hwnd: HWND) {
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

    pub(crate) fn apply_native_cursor(cursor: CursorIcon) -> Result<(), CursorError> {
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

impl crate::host_window::CoreHostWindow for WindowsWindow {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl crate::host_window::HostWindow for WindowsWindow {
    /// The window's own bridge — the capability the composition root's
    /// accessibility wire discovers.
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        self.facets.text.capture_sizing()
    }

    /// The token proves the platform's owner thread, not this window's: an
    /// HWND belongs to the thread that created it, and its text services live
    /// on that thread, so the text position answers `None` on any other. On
    /// the creating thread it is the window's TSF connection (ADR-0135 §3):
    /// `None` when TSF did not activate for it, or once the window is
    /// destroyed; a host read before that answers
    /// [`TextStoreHostError::Unsupported`] from then on.
    ///
    /// [`TextStoreHostError::Unsupported`]: flui_platform::text_store::TextStoreHostError::Unsupported
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

impl PlatformWindow for WindowsWindow {
    fn id(&self) -> WindowId {
        WindowId(self.hwnd.0 as u64)
    }

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        Some(Arc::clone(&self.drag_source))
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
            super::proc::with_window_context(root, "window_at", |context| {
                (context.window_id, context.scale.scale())
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

    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        use crate::owner::hwnd_affinity::UserDataRefusal;
        use flui_platform::{GestureGeometry, PreferenceQueryError};

        super::proc::with_window_context_checked(self.hwnd, "gesture geometry", |ctx| {
            // SAFETY: the checked owner context proves this is the still-live
            // owned HWND. The scalar DPI getter does not dispatch user code.
            let dpi = unsafe { GetDpiForWindow(self.hwnd) };
            let native = ctx
                .preferences
                .mouse_gestures(dpi)?
                .native_mouse_geometry()
                .ok_or(PreferenceQueryError::Unsupported(Unsupported::new(
                    UnsupportedReason::Unavailable,
                )))?;
            Ok(Some(GestureGeometry::from_native_mouse(&native)?))
        })
        .map_err(|refusal| match refusal {
            UserDataRefusal::ForeignThread => PreferenceQueryError::WrongThread,
            _ => Unsupported::new(UnsupportedReason::NoWindow).into(),
        })?
    }

    fn request_redraw(&self) {
        // Retired, destroyed, or a handle the OS reused for another window:
        // not this window, which asks for nothing.
        flui_platform::RedrawRequester::request_redraw(&self.surface());
    }

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(self.surface())
    }

    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(self.surface())
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        super::proc::with_window_context(self.hwnd, "set_cursor", |ctx| {
            ctx.cursor.set(cursor);
            if self.hovered() == Some(true) {
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
        super::proc::with_window_context(self.hwnd, "state", |ctx| ctx.window_state(self.hwnd))
            .unwrap_or_else(|| self.snapshot.published())
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if !handle_available(super::proc::teardown_route(self.hwnd, self.context)) {
            return Err(ApplyError::Closed);
        }
        let command = command.against(&self.snapshot.published_capabilities())?;
        match command {
            WindowCommand::SetTitle(title) => self.set_native_title(&title),
            WindowCommand::SetPosition(at) => self.set_outer_position(at)?,
            WindowCommand::SetSize(size) => self.set_inner_size(size)?,
            WindowCommand::SetConstraints(constraints) => self.set_constraints(constraints),
            WindowCommand::SetResizable(on) => {
                let decorated =
                    super::proc::with_window_context(self.hwnd, "set_resizable", |ctx| {
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
                    super::proc::with_window_context(self.hwnd, "set_decorated", |ctx| {
                        ctx.resizable.get()
                    })
                    .unwrap_or(true);
                self.restyle_frame(on, resizable)?;
            }
            WindowCommand::SetMode(mode) => self.set_mode(mode)?,
            WindowCommand::Hide => self.hide_native(),
            WindowCommand::Show => self.show_window()?,
            WindowCommand::Activate => self.activate_window(),
            // A destroyed window has no state left to publish.
            WindowCommand::Close => {
                self.close_window();
                return Ok(());
            }
            // The material is no fact of the snapshot: nothing to publish. A
            // build below the gate has the capability `Unavailable`, so
            // admission refused the command before it came here.
            WindowCommand::SetMaterial(material) => {
                let Ok(gate) = self.windows.material else {
                    return Err(crate::window::state::not_carried_out(
                        &WindowCommand::SetMaterial(material),
                    ));
                };
                return super::backdrop::apply_material(self.hwnd, gate, material);
            }
            other => return Err(crate::window::state::not_carried_out(&other)),
        }
        super::proc::with_window_context(self.hwnd, "apply", |ctx| {
            ctx.publish_window_state(self.hwnd);
        });
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        // Only a window whose reveal was deferred waits for this call; one
        // opened hidden, or already shown, ignores it.
        let deferred = super::proc::with_window_context(self.hwnd, "reveal", |ctx| {
            ctx.first_show.get().awaits_frame()
        })
        .unwrap_or(false);
        if deferred {
            self.show_first();
            tracing::debug!("Windows: window revealed after its first presented frame");
        }
    }

    /// The display's rate is the user's setting: a Win32 window asks for none.
    fn set_frame_rate_hint(&self, _hint: flui_platform::FrameRateHint) -> Result<(), Unsupported> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    }

    /// The input regions keep the system's edge gestures out of a region
    /// (`InputRegionKind::SystemGestureExclusion`). Windows starts its edge
    /// swipes whatever a window says: `SetWindowRgn` and `HTTRANSPARENT` decide
    /// where a window takes input, which is not that. The input capabilities
    /// say `Unavailable`.
    fn set_input_regions(
        &self,
        _regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), Unsupported> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    }
}

/// The bodies of the show, activate and close commands.
impl WindowsWindow {
    /// Show the window, unminimizing it and requesting foreground focus; the
    /// maximized or fullscreen mode is kept.
    fn show_window(&self) -> Result<(), ApplyError> {
        use windows::Win32::UI::WindowsAndMessaging::{
            SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WPF_RESTORETOMAXIMIZED,
        };
        crate::owner::hwnd_affinity::show_owned_window(
            || {
                use crate::owner::hwnd_affinity::TeardownRoute;
                let route = super::proc::teardown_route(self.hwnd, self.context);
                if route != TeardownRoute::DestroyDirect {
                    return route;
                }
                // The wrapper and its live context carry the same identity,
                // minted once and never reused, so unlike the raw context
                // address it cannot be recycled. Only inspect it under the
                // owner guard.
                if super::proc::with_window_context(self.hwnd, "show identity", |context| {
                    context.identity == self.identity
                }) == Some(true)
                {
                    route
                } else {
                    TeardownRoute::StaleHandle
                }
            },
            || {
                super::proc::with_window_context(self.hwnd, "show", |ctx| {
                    ctx.note_show_request();
                    let first = ctx.first_show.replace(FirstShow::Done).mode();
                    // SAFETY: the identity check proves this wrapper's live HWND
                    // on its owner thread. The context guard pins userdata through
                    // ShowWindow reentry; placement is an initialized out pointer.
                    unsafe {
                        let placement = placement_of(self.hwnd)
                            .map_err(|error| ApplyError::failed(error.to_string()))?;
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
                            ctx.settle_shown();
                        }
                        Ok(())
                    }
                })
                .unwrap_or(Err(ApplyError::Closed))
            },
            || {
                // The adapter rechecked wrapper identity after ShowWindow, on
                // the same owner thread, before this callback-capable call.
                // A show the foreground rules refuse is still a show: it asks
                // for no attention.
                let _ = request_foreground(self.hwnd);
            },
        )
    }

    /// Bring the window to the foreground where Windows' foreground rules let
    /// this process, and ask for the user's attention where they do not: the
    /// window never takes the focus from the application the user works
    /// with.
    fn activate_window(&self) {
        // Activating a window that was never shown shows it.
        self.show_first();
        // The show can have run user callbacks that closed this window and
        // opened another on the same handle.
        if !self.owns_handle() {
            return;
        }
        if request_foreground(self.hwnd) == Foreground::Refused {
            ask_attention(self.hwnd);
        }
    }

    fn close_window(&self) {
        use crate::owner::hwnd_affinity::TeardownRoute;
        match super::proc::teardown_route(self.hwnd, self.context) {
            TeardownRoute::DestroyDirect => {
                // SAFETY: `DestroyWindow` takes `self.hwnd` by value; the
                // route just established this is the owning thread, the only
                // one Win32 permits the call from, and where `WM_DESTROY`
                // (handled in `platform.rs`, reclaiming the `GWLP_USERDATA`
                // allocation) is dispatched synchronously before the call
                // returns.
                unsafe {
                    if let Err(error) = DestroyWindow(self.hwnd) {
                        tracing::warn!(hwnd = ?self.hwnd, ?error, "DestroyWindow (WindowCommand::Close) failed");
                    }
                }
            }
            TeardownRoute::PostClose => {
                // Cross-thread close cannot call `DestroyWindow` (Win32
                // forbids it off the creating thread), so the owner thread
                // destroys the window when it takes the posted message. Not
                // `WM_CLOSE`: that asks the should-close veto, and a close
                // command is not a request. The message names this window,
                // so a handle reused before it arrives is left alone.
                let (low, high) = self.identity.halves();
                // SAFETY: `PostMessageW` takes the handle and plain
                // integers by value — nothing dereferenced, delivery
                // marshalled by the OS to the owning thread.
                unsafe {
                    if let Err(error) = PostMessageW(
                        Some(self.hwnd),
                        super::proc::CLOSE_WINDOW,
                        WPARAM(low),
                        LPARAM(high),
                    ) {
                        tracing::warn!(hwnd = ?self.hwnd, ?error, "PostMessageW(WM_CLOSE) (cross-thread WindowCommand::Close) failed");
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
}

/// Whether [`WindowsWindow`]'s [`HasWindowHandle::window_handle`] may hand
/// out a handle for a window whose teardown routes to `route`.
///
/// Pure so it is testable without a live `HWND` (see
/// [`crate::owner::hwnd_affinity::TeardownRoute`]): `DestroyDirect` and
/// `PostClose` both describe a live window this wrapper still owns (they
/// differ only in which thread may call `DestroyWindow` directly);
/// `AlreadyGone` and `StaleHandle` both describe a window with nothing left
/// to hand a caller — destroyed, or the OS recycled the `HWND` value for an
/// unrelated window.
#[must_use]
fn handle_available(route: crate::owner::hwnd_affinity::TeardownRoute) -> bool {
    use crate::owner::hwnd_affinity::TeardownRoute;
    matches!(
        route,
        TeardownRoute::DestroyDirect | TeardownRoute::PostClose
    )
}

/// What Windows answered a request for the foreground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Foreground {
    /// The window is in the foreground.
    Granted,
    /// The foreground stays where it is.
    Refused,
}

/// Ask Windows to bring `hwnd` to the foreground. Windows grants it only to
/// the process the user works with: the one in the foreground, one it
/// started, the one that received the last input, or one the foreground
/// process handed the right to (`AllowSetForegroundWindow`, which a second
/// launch calls before it hands its arguments to the running instance).
/// Every other request is refused and the foreground stays where it is.
/// This is the only place a window asks for the foreground: nothing forces
/// it past those rules (no attached input queue, no injected keystroke).
fn request_foreground(hwnd: HWND) -> Foreground {
    // SAFETY: an `HWND` is an opaque handle Win32 validates on every call;
    // a stale one is refused, so no window this process does not own can be
    // reached through it.
    if unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        Foreground::Granted
    } else {
        Foreground::Refused
    }
}

/// Ask for the user's attention to `hwnd`, which the foreground rules kept
/// in the background: its taskbar button flashes until the window comes to
/// the foreground.
fn ask_attention(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx,
    };

    let flash = FLASHWINFO {
        cbSize: u32::try_from(std::mem::size_of::<FLASHWINFO>())
            .expect("BUG: FLASHWINFO is a few bytes"),
        hwnd,
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    // SAFETY: the pointer is to a live, initialised `FLASHWINFO` whose
    // `cbSize` matches its type, which Win32 reads only for the call. The
    // return value is the caption's state before the call, not a failure.
    let _ = unsafe { FlashWindowEx(&raw const flash) };
}

/// A Win32 window's surface target and redraw requester (ADR-0254 §5): what
/// the thread presenting its frames and any thread that wants a frame hold
/// of the window.
///
/// It holds the window's handle and the address of its context as plain
/// integers, never dereferenced, and the window's [`Retirement`], so it is
/// `Send + Sync` with no `unsafe impl` and keeps nothing of the window alive.
/// The window is there while its retirement has not started (`WM_DESTROY`
/// starts it) and the identity probe still finds this window behind the
/// handle: a destroyed handle, or one the OS recycled for another window,
/// is not this window.
///
/// [`Retirement`]: crate::owner::window_delivery::Retirement
#[derive(Debug)]
pub(crate) struct Win32Surface {
    /// The window handle, only probed, wrapped and posted to.
    hwnd: isize,
    /// The address of the window's context, compared by the probe against
    /// what the handle's `GWLP_USERDATA` holds now.
    context: isize,
    retirement: crate::owner::window_delivery::Retirement,
}

impl Win32Surface {
    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut std::ffi::c_void)
    }

    /// Whether the window this surface was made for is still there.
    fn is_live(&self) -> bool {
        !self.retirement.is_retired()
            && handle_available(super::proc::teardown_route(
                self.hwnd(),
                self.context as *const super::proc::WindowContext,
            ))
    }
}

impl HasWindowHandle for Win32Surface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::num::NonZeroIsize;

        if !self.is_live() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let mut handle = Win32WindowHandle::new(
            NonZeroIsize::new(self.hwnd).ok_or(raw_window_handle::HandleError::Unavailable)?,
        );
        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments.
        let hinstance = unsafe { GetModuleHandleW(None) }
            .map_err(|_| raw_window_handle::HandleError::Unavailable)?;
        handle.hinstance = NonZeroIsize::new(hinstance.0 as isize);
        // SAFETY: the handle names a window this target was made for, live
        // when `is_live` probed it just above. An `HWND` is an identifier, not
        // a pointer (`borrow_raw`'s guarantee covers only pointers): a window
        // destroyed after the probe makes later calls with it fail, and the
        // window's handler hears `surface_changed(Release)` before its
        // `WM_DESTROY` runs.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for Win32Surface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        if !self.is_live() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        // SAFETY: `WindowsDisplayHandle` carries no fields: there is nothing
        // for `borrow_raw` to keep valid.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Windows(
                WindowsDisplayHandle::new(),
            ))
        })
    }
}

impl flui_platform::SurfaceTarget for Win32Surface {
    /// Win32 has no frame callback to arm before a present: it returns at
    /// once.
    fn pre_present_notify(&self) {}
}

impl flui_platform::RedrawRequester for Win32Surface {
    /// `InvalidateRect` and `PostMessageW` take a window handle from any
    /// thread, so the request goes to the window system directly.
    fn request_redraw(&self) {
        if self.is_live() {
            WindowsWindow::redraw_native(self.hwnd());
        }
    }
}

impl Clone for WindowsWindow {
    fn clone(&self) -> Self {
        Self {
            hwnd: self.hwnd,
            state: Arc::clone(&self.state),
            identity: self.identity,
            windows: Arc::clone(&self.windows),
            displays: Arc::clone(&self.displays),
            snapshot: Arc::clone(&self.snapshot),
            // Same window, same identity token.
            context: self.context,
            // The clone shares the same window, so it shares what was made
            // for it: one bridge (one subclass hook per HWND, never two), and
            // one actuator, so a newer request replaces an older one
            // whichever wrapper made it.
            facets: self.facets.clone(),
            drag_source: Arc::clone(&self.drag_source),
            delivery: Arc::clone(&self.delivery),
        }
    }
}

impl Drop for WindowsWindow {
    fn drop(&mut self) {
        // Only destroy if this is the last reference
        if Arc::strong_count(&self.state) == 1 {
            tracing::debug!("Destroying window HWND {:?}", self.hwnd);

            use crate::owner::hwnd_affinity::TeardownRoute;
            match super::proc::teardown_route(self.hwnd, self.context) {
                TeardownRoute::DestroyDirect => {
                    // Detach the bridge (UIA's subclass) BEFORE destroying
                    // the window — Win32 wants subclasses removed while the
                    // window still exists, and this is the owner-thread
                    // teardown path subclassing requires. Any capability
                    // `Arc` still held elsewhere degrades to a no-op.
                    self.facets.accessibility.detach();

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
                    // is left for the bridge's own drop guard
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

            // Leave the tracked windows; the entry drops after the set's lock
            // is released.
            let _removed = self.windows.remove(self.hwnd, self.identity);
        }
    }
}

impl WindowsWindow {
    /// Bring the window back to its normal mode from any other: a minimized or
    /// maximized window is restored, and a fullscreen one gets its frame and
    /// bounds back. A hidden window stays hidden.
    pub(crate) fn restore_normal(&self) {
        // SAFETY: plain state queries on this wrapper's window handle.
        let (away, shown) = unsafe {
            (
                IsIconic(self.hwnd()).as_bool() || IsZoomed(self.hwnd()).as_bool(),
                IsWindowVisible(self.hwnd()).as_bool(),
            )
        };
        if away && shown {
            self.show_native(SW_RESTORE);
        } else if away {
            self.restore_hidden();
        }
        // A fullscreen window that was minimized comes back fullscreen from the
        // restore above; leaving fullscreen is the second step.
        self.set_fullscreen(false);
    }

    /// Take a hidden maximized or minimized window back to its normal rectangle
    /// without showing it.
    ///
    /// A maximized window loses its state bit and is moved to the rectangle it
    /// returns to. The system will not drop a minimized window's state bit, so
    /// that one is restored and hidden again; the show and the hide are not the
    /// window appearing, and the runtime hears neither.
    fn restore_hidden(&self) {
        let iconic = unsafe { IsIconic(self.hwnd()) }.as_bool();
        if iconic {
            super::proc::with_window_context(self.hwnd(), "restore_hidden", |ctx| {
                let _hold = ctx.hold_publishing();
                let requests = ctx.show_requests.get();
                self.show_native(SW_RESTORE);
                // The restore runs user callbacks. If one closed this window and
                // opened another on the handle, or asked for this one to be shown,
                // the hide below is not wanted.
                let overtaken = !self.owns_handle() || ctx.show_requests.get() != requests;
                if !overtaken {
                    self.show_native(SW_HIDE);
                }
                // The publishing held above goes out once, with what the
                // window is after both: the command that asked publishes it.
            });
            return;
        }
        let Some(normal) = restore_rectangle(self.hwnd()) else {
            return;
        };
        // The mode it ends in is set first: the `WM_SIZE` the move sends then
        // sees a window that was not minimized, so it reports no visibility for
        // a window that stays hidden.
        super::proc::with_window_context(self.hwnd(), "restore_hidden", |ctx| {
            ctx.mode.set(NativeMode::Normal);
        });
        // SAFETY: plain style and position calls on this wrapper's window; a
        // stale handle only makes them fail.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd(), GWL_STYLE) as u32;
            SetWindowLongPtrW(self.hwnd(), GWL_STYLE, (style & !WS_MAXIMIZE.0) as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd(),
                None,
                normal.origin.x,
                normal.origin.y,
                normal.size.width,
                normal.size.height,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
            ) {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "SetWindowPos (restore hidden) failed");
            }
        }
    }
}

impl WindowsWindow {
    /// Hold the window to its size limits: the size a normal window has, and
    /// the rectangle any other window returns to.
    pub(super) fn enforce_constraints(&self) {
        let mode = super::proc::with_window_context(self.hwnd(), "enforce_constraints", |ctx| {
            ctx.mode.get()
        });
        match mode {
            Some(mode) if mode.is_normal() => self.ask_for_limits_on_current_size(),
            Some(_) => self.clamp_restore_rectangle(),
            None => {}
        }
    }

    /// A size change makes the system ask for the limits: ask with the size the
    /// window already has.
    pub(super) fn ask_for_limits_on_current_size(&self) {
        let mut window = RECT::default();
        // SAFETY: `window` is a stack local out-parameter; `SetWindowPos` takes
        // plain integers and flags for this wrapper's live window.
        unsafe {
            if GetWindowRect(self.hwnd(), &raw mut window).is_err() {
                return;
            }
            if let Err(error) = SetWindowPos(
                self.hwnd(),
                None,
                0,
                0,
                window.right - window.left,
                window.bottom - window.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "SetWindowPos (size limits) failed");
            }
        }
    }

    /// Keep the rectangle a maximized, minimized or fullscreen window returns
    /// to within the limits, so restoring it does not undo what the app set.
    pub(super) fn clamp_restore_rectangle(&self) {
        use super::limits::PixelSize;

        super::proc::with_window_context(self.hwnd(), "clamp_restore_rectangle", |ctx| {
            let mode = ctx.mode.get();
            if let Some(bounds) = mode.fullscreen_bounds() {
                let Some(limits) = ctx.limits_with_frame_of(self.hwnd(), ctx.restore_style.get())
                else {
                    return;
                };
                let size = limits.clamp(PixelSize::new(bounds.size.width, bounds.size.height));
                ctx.mode.set(mode.with_fullscreen_bounds(Bounds {
                    origin: bounds.origin,
                    size: Size::new(size.width, size.height),
                }));
                return;
            }
            // SAFETY: a plain style query of this wrapper's window handle.
            let style = unsafe { GetWindowLongPtrW(self.hwnd(), GWL_STYLE) } as u32;
            let Some(limits) = ctx.limits_with_frame_of(self.hwnd(), style) else {
                return;
            };
            let Ok(mut placement) = super::set::placement_of(self.hwnd()) else {
                return;
            };
            let normal = &mut placement.rcNormalPosition;
            let size = limits.clamp(PixelSize::new(
                normal.right.saturating_sub(normal.left),
                normal.bottom.saturating_sub(normal.top),
            ));
            normal.right = normal.left.saturating_add(size.width);
            normal.bottom = normal.top.saturating_add(size.height);
            // SAFETY: `placement` is fully initialized; a stale handle only makes
            // the call fail.
            if let Err(error) = unsafe { SetWindowPlacement(self.hwnd(), &raw const placement) } {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "the restore rectangle could not be limited");
            }
        });
    }
}
