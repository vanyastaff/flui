//! The windows of a Win32 platform: [`WindowSet`], which the loop asks for
//! the deadline redraws and the modal paints, the window position that opens
//! them into it, and where a window opens and returns to (its placement).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use flui_foundation::geometry::{Bounds, Point, Size};
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::UpdateWindow,
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowPlacement, IsWindowVisible, WINDOWPLACEMENT,
    },
};

use flui_platform::{
    CapabilityAbsence, PlatformWindow, Unsupported, WindowCapabilities, WindowHandler, WindowId,
    WindowIdMint, WindowOptions,
};

use super::frame_timing::{DisplayAdapter, presentation_absence};
use super::material::{self, SystemBackdrop};
use super::proc::{WindowRef, to_screen, with_window_context_checked};
use super::window::{WindowsWindow, window_capabilities};
use crate::host::WindowPositions;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::handlers::OpenWindows;
use crate::owner::windows::{ClassWindows, WindowsLoop};
use crate::version::gate::Since;
use crate::version::gate::windows::WindowsBuild;
use crate::window::config::WindowConfiguration;

/// The windows of FLUI's class the platform tracks, keyed by HWND, and the
/// configuration they open with (hotkeys, debouncing, fullscreen behavior).
/// The owner's message-only windows are never in it.
///
/// The host builds it and hands it to the loop, which holds it as
/// [`ClassWindows`]; each window's context and wrapper keep it too, to leave
/// it when the window closes. Public where it is defined, as the host's table
/// names it, and nameable only inside the crate.
pub struct WindowSet {
    /// The windows, which only the owner thread holds (ADR-0327 §5). The only
    /// way from an HWND to a [`WindowId`]: a handle not in it names no window
    /// of this platform.
    by_handle: RefCell<FxHashMap<isize, Rc<WindowsWindow>>>,
    /// Issues the id of every window the set opens,
    /// never one twice: Win32 recycles HWND values, so a handle is never an
    /// id.
    ids: RefCell<WindowIdMint>,
    pub(super) config: WindowConfiguration,
    /// What every window hands out of the window-level positions.
    pub(super) positions: WindowPositions<WindowRef>,
    /// Whether the build the platform runs on draws a material behind a
    /// window, read once with the platform.
    pub(super) material: Result<Since<SystemBackdrop>, Unsupported>,
    /// Why the machine's display tells no window when its frames reached the
    /// screen, read once with the platform; `None` where it tells.
    presentation: Option<CapabilityAbsence>,
}

/// The machine's DXGI adapters, each with the number of its outputs the
/// desktop is on; none where DXGI cannot be asked.
fn display_adapters() -> Vec<DisplayAdapter> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    // SAFETY: a plain factory creation, no arguments.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    // DXGI lists adapters from 0 until `DXGI_ERROR_NOT_FOUND`, and outputs of
    // each the same way.
    // SAFETY: each call takes an index and returns an owned COM interface.
    (0..)
        .map_while(|index| unsafe { factory.EnumAdapters1(index) }.ok())
        .filter_map(|adapter| {
            // SAFETY: a read of the live adapter's description, returned by
            // value.
            let description = unsafe { adapter.GetDesc1() }.ok()?;
            let desktop_outputs = (0..)
                // SAFETY: as above, an index into the live adapter's outputs.
                .map_while(|index| unsafe { adapter.EnumOutputs(index) }.ok())
                // SAFETY: a read of the live output's description, returned
                // by value.
                .filter(|output| unsafe { output.GetDesc() }.is_ok_and(|d| d.AttachedToDesktop.as_bool()))
                .count();
            Some(DisplayAdapter {
                vendor_id: description.VendorId,
                device_id: description.DeviceId,
                desktop_outputs: u32::try_from(desktop_outputs).unwrap_or(u32::MAX),
            })
        })
        .collect::<Vec<DisplayAdapter>>()
}

impl WindowSet {
    /// No window yet; every window opens with `config` and hands out
    /// `positions`, and draws a material as `build` allows.
    pub(crate) fn new(
        config: WindowConfiguration,
        positions: WindowPositions<WindowRef>,
        build: WindowsBuild,
    ) -> Self {
        Self {
            by_handle: RefCell::default(),
            ids: RefCell::default(),
            config,
            positions,
            material: material::gate(build),
            presentation: presentation_absence(&display_adapters()),
        }
    }

    /// What every window of this platform honours and observes.
    pub(super) const fn capabilities(&self) -> WindowCapabilities {
        window_capabilities(&self.material, self.presentation)
    }

    /// Stops tracking the window `hwnd` names, when it is the one with
    /// `identity` (the OS recycles handles): the wrapper the set held, to be
    /// dropped once the set's lock is released, and whether no window is left.
    pub(crate) fn remove(
        &self,
        hwnd: HWND,
        identity: crate::owner::windows::WindowIdentity,
    ) -> Option<(Rc<WindowsWindow>, bool)> {
        let key = hwnd.0 as isize;
        let mut windows = self.by_handle.borrow_mut();
        if windows
            .get(&key)
            .is_none_or(|window| window.identity != identity)
        {
            return None;
        }
        windows
            .remove(&key)
            .map(|window| (window, windows.is_empty()))
    }

    /// A snapshot of the windows: what each of them does next may run user
    /// code, which may open or close windows.
    fn snapshot(&self) -> Vec<Rc<WindowsWindow>> {
        self.by_handle
            .borrow()
            .values()
            .cloned()
            .collect::<Vec<Rc<WindowsWindow>>>()
    }
}

impl std::fmt::Debug for WindowSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The formatter's writer is code the set does not own, and it may
        // open or close a window: every borrow is released before it runs.
        // A set being changed is not there to count.
        let windows = self.by_handle.try_borrow().map(|windows| windows.len()).ok();
        let ids = self.ids.try_borrow().map(|ids| format!("{:?}", *ids)).ok();
        let borrowed = format_args!("<borrowed>");
        let mut out = f.debug_struct("WindowSet");
        match &windows {
            Some(count) => out.field("windows", count),
            None => out.field("windows", &borrowed),
        };
        match &ids {
            Some(ids) => out.field("ids", &format_args!("{ids}")),
            None => out.field("ids", &borrowed),
        };
        out
            .field("config", &self.config)
            .field("positions", &self.positions)
            .field("material", &self.material)
            .field("presentation", &self.presentation)
            .finish()
    }
}

impl OpenWindows for WindowSet {
    /// A set no window was ever stored into is empty.
    fn is_empty(&self) -> bool {
        self.by_handle.borrow().is_empty()
    }
}

impl ClassWindows for WindowSet {
    /// The main loop's delivery of a wake deadline, and the modal drag
    /// loop's.
    fn redraw_for_deadline(&self) {
        // Traverse a bounded identity snapshot without a per-frame Vec.
        // A minimized window's frame request runs user code, which may open
        // or close windows; each owning `Rc` is cloned out of the set and used
        // only after the set is let go.
        let Some(limit) = self
            .by_handle
            .borrow()
            .values()
            .map(|window| window.identity)
            .max()
        else {
            return;
        };
        let mut after = None;
        loop {
            let next = self
                .by_handle
                .borrow()
                .values()
                .filter(|window| {
                    after.is_none_or(|after| window.identity > after) && window.identity <= limit
                })
                .min_by_key(|window| window.identity)
                .cloned();
            let Some(window) = next else {
                break;
            };
            after = Some(window.identity);
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
                    PlatformWindow::request_redraw(window.as_ref());
                }
            });
        }
    }

    fn paint_invalid(&self) {
        // A snapshot: a paint runs user code, which may open or close windows.
        for window in self.snapshot() {
            // SAFETY: a window this thread created; a handle closed by an
            // earlier paint fails the call harmlessly. Nothing is borrowed.
            let _ = unsafe { UpdateWindow(window.hwnd()) };
        }
    }
}

impl crate::host::Window<WindowsLoop> for WindowsWindow {
    type Shared = WindowSet;
    /// A window by its handle and the identity of its context.
    type Ref = WindowRef;

    fn open(
        owner: &WindowsLoop,
        windows: &Arc<WindowSet>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
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

        let window_id = windows.ids.borrow_mut().next()?;
        let window = WindowsWindow::try_new(
            window_id,
            options,
            handler,
            Arc::clone(windows),
            shares,
            owner.apartment.share(),
            Arc::clone(&owner.drag),
        )?;
        let hwnd_value = window.hwnd().0 as isize;

        // A window the OS gave the same handle before has closed; its
        // wrapper is dropped with the set's borrow released.
        let previous = windows
            .by_handle
            .borrow_mut()
            .insert(hwnd_value, Rc::clone(&window));
        drop(previous);

        Ok(window)
    }

    fn active(owner: &WindowsLoop, windows: &WindowSet) -> Option<WindowId> {
        owner.affinity.debug_assert_owner("WindowsWindow::active");
        // SAFETY: `GetForegroundWindow` takes no arguments; `hwnd` is just
        // an opaque value looked up, never dereferenced.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }
        // Another application's window, or one of ours that closed, is in
        // no entry: the foreground window is then none of this platform's.
        windows
            .by_handle
            .borrow()
            .get(&(hwnd.0 as isize))
            .map(|window| window.window_id)
    }

    fn capabilities(_: &WindowsLoop, windows: &WindowSet) -> WindowCapabilities {
        windows.capabilities()
    }

    fn frame_inset(_: &WindowsLoop, _: &WindowSet, _: f64) -> Size<f64> {
        // The frame is not measured before a window exists, so the host
        // reports the contract's default: no inset.
        Size::new(0.0, 0.0)
    }

    /// Win32's display handle, which carries no pointer.
    fn display_connection(
        _: &WindowsLoop,
        _: &WindowSet,
    ) -> std::sync::Arc<dyn flui_platform::DisplayConnection> {
        std::sync::Arc::new(crate::display_connection::PointerlessDisplay::Windows)
    }
}

/// DPI constants
pub(crate) const USER_DEFAULT_SCREEN_DPI: u32 = 96;

/// Convert logical pixels to device pixels
#[inline]
pub(crate) fn logical_to_device(logical: f64, scale_factor: f64) -> i32 {
    (logical * scale_factor).round() as i32
}

/// The largest device coordinate or extent a window command may name. It
/// leaves room below `i32::MAX` for the frame `AdjustWindowRectExForDpi` adds
/// and for an origin plus a size.
pub(crate) const DEVICE_COORDINATE_LIMIT: i32 = 0x3FFF_0000;

/// Convert logical pixels to device pixels, or `None` when the result is not
/// a finite device value within [`DEVICE_COORDINATE_LIMIT`]: geometry Win32
/// would otherwise receive saturated.
#[inline]
pub(crate) fn checked_logical_to_device(logical: f64, scale_factor: f64) -> Option<i32> {
    let device = (logical * scale_factor).round();
    let limit = f64::from(DEVICE_COORDINATE_LIMIT);
    (device.is_finite() && (-limit..=limit).contains(&device)).then_some(device as i32)
}

/// The window's placement: its show state and the rectangles it returns to.
pub(crate) fn placement_of(hwnd: HWND) -> windows::core::Result<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: `placement` is a stack local with `length` set; a stale `hwnd`
    // only makes the call fail.
    unsafe { GetWindowPlacement(hwnd, &raw mut placement) }?;
    Ok(placement)
}

/// How far the work area of `hwnd`'s monitor starts from the monitor's own
/// origin, in device pixels: a taskbar on the top or left edge moves it.
/// `WINDOWPLACEMENT::rcNormalPosition` is measured from the work area, screen
/// coordinates from the monitor.
pub(crate) fn workspace_origin(hwnd: HWND) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };

    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a stack local with `cbSize` set; a stale `hwnd` makes
    // the monitor lookup fall back to the nearest monitor and never faults.
    let found = unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &raw mut info).as_bool()
    };
    if found {
        (
            info.rcWork.left - info.rcMonitor.left,
            info.rcWork.top - info.rcMonitor.top,
        )
    } else {
        (0, 0)
    }
}

/// The outer rectangle `hwnd` returns to from a maximized or minimized state
/// (`rcNormalPosition`), in device pixels of the virtual screen. Unlike
/// `GetWindowRect` it is the normal rectangle whatever state the window is in
/// now.
pub(crate) fn restore_rectangle(hwnd: HWND) -> Option<Bounds<i32>> {
    let placement = placement_of(hwnd).ok()?;
    let screen = to_screen(placement.rcNormalPosition, workspace_origin(hwnd));
    Some(Bounds {
        origin: Point::new(screen.left, screen.top),
        size: Size::new(screen.right - screen.left, screen.bottom - screen.top),
    })
}

/// Where Win32 is asked to put a window that opens, in physical pixels of the
/// virtual screen.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpenSpot {
    /// The scale the sizes were computed at: the target display's, else the
    /// system's.
    pub(crate) scale: f64,
    /// The outer size the window is created with.
    pub(crate) outer: (i32, i32),
    /// The outer top-left; `None` leaves the place to the system.
    pub(crate) origin: Option<(i32, i32)>,
    /// The outer rectangle (left, top, right, bottom) a window that opens
    /// maximized or fullscreen returns to; `None` leaves it to the system.
    pub(crate) normal: Option<(i32, i32, i32, i32)>,
}

/// Turn the options of an opening window into physical pixels.
///
/// `options.size` is the content size, so `outer_of` (the window frame at a
/// DPI) turns it into the outer size. `options.position` and
/// `options.normal_geometry` are relative to the origin of `target`'s usable
/// area in that display's logical pixels and convert through that display's own
/// scale; with no `target` they are global logical pixels at `system_scale`.
pub(crate) fn open_spot(
    options: &WindowOptions,
    target: Option<&flui_platform::DisplayInfo>,
    system_scale: f64,
    outer_of: impl Fn(u32, (i32, i32)) -> (i32, i32),
) -> OpenSpot {
    let scale = target
        .map(|display| display.scale_factor)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(system_scale);
    let dpi = ((scale * f64::from(USER_DEFAULT_SCREEN_DPI)).round() as u32).max(1);
    let content = (
        logical_to_device(options.size.width, scale),
        logical_to_device(options.size.height, scale),
    );
    let outer = outer_of(dpi, content);
    let anchor = target.map_or((0, 0), |display| {
        (
            display.usable_bounds.origin.x,
            display.usable_bounds.origin.y,
        )
    });
    let place = |at: Point<f64>| {
        (
            anchor.0.saturating_add(logical_to_device(at.x, scale)),
            anchor.1.saturating_add(logical_to_device(at.y, scale)),
        )
    };
    let origin = options.position.map(place);
    let normal = options.normal_geometry.and_then(|geometry| {
        let (left, top) = geometry.position.map(place).or(origin)?;
        Some((
            left,
            top,
            left.saturating_add(logical_to_device(geometry.size.width, scale)),
            top.saturating_add(logical_to_device(geometry.size.height, scale)),
        ))
    });
    OpenSpot {
        scale,
        outer,
        origin,
        normal,
    }
}

/// The outer size, in physical pixels, of a window of `style` that has a
/// content area of `content` at `dpi`: the content plus its title bar and
/// borders (`AdjustWindowRectExForDpi`), measured for the style given and not
/// for the window's current rectangle, which a maximized or minimized window
/// does not keep. A window whose application draws its title bar
/// (`app_title_bar`) has neither caption nor top border above its content
/// (`chrome::outer_extent`).
pub(crate) fn outer_size(
    style: u32,
    ex_style: u32,
    dpi: u32,
    content: (i32, i32),
    app_title_bar: bool,
) -> windows::core::Result<(i32, i32)> {
    use windows::Win32::UI::{
        HiDpi::AdjustWindowRectExForDpi,
        WindowsAndMessaging::{WINDOW_EX_STYLE, WINDOW_STYLE},
    };

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: content.0,
        bottom: content.1,
    };
    // SAFETY: `rect` is a stack local passed as the documented in/out
    // parameter; the style and DPI are plain values.
    unsafe {
        AdjustWindowRectExForDpi(
            &raw mut rect,
            WINDOW_STYLE(style),
            false,
            WINDOW_EX_STYLE(ex_style),
            dpi,
        )?;
    }
    let rect = super::chrome::outer_extent(
        super::chrome::Edges {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        },
        app_title_bar,
    );
    rect.right
        .checked_sub(rect.left)
        .zip(rect.bottom.checked_sub(rect.top))
        .ok_or_else(|| windows::Win32::Foundation::E_INVALIDARG.into())
}

#[cfg(test)]
#[cfg(test)]
mod open_tests {
    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{
        DisplayInfo, DisplayKey, DisplayRegistry, WindowCommand, WindowGeometry, WindowMode,
        WindowOptions,
    };

    use flui_testing::window_commands::AdmitAndApply as _;

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
        let options = WindowOptions::default().size(Size::new(800.0, 600.0));
        let spot = open_spot(&options, None, 1.0, with_frame);
        assert_eq!(spot.outer, (816, 639));
        assert_eq!(spot.origin, None);
    }

    fn a_position_converts_through_the_named_displays_own_scale() {
        // A 2x display that starts at device x 1920: 100 logical is 200 device
        // pixels from its origin, whatever the system scale is.
        let target = display(2.0, (1920, 0));
        let options = WindowOptions::default()
            .size(Size::new(800.0, 600.0))
            .position(Some(Point::new(100.0, 50.0)));
        let spot = open_spot(&options, Some(&target), 1.0, with_frame);
        assert_eq!(spot.scale, 2.0);
        assert_eq!(spot.outer, (1600 + 16, 1200 + 39));
        assert_eq!(spot.origin, Some((1920 + 200, 100)));
    }

    fn without_a_display_the_position_is_global_at_the_system_scale() {
        let options = WindowOptions::default().position(Some(Point::new(100.0, 50.0)));
        let spot = open_spot(&options, None, 1.5, with_frame);
        assert_eq!(spot.origin, Some((150, 75)));
    }

    fn the_normal_rectangle_is_the_outer_geometry_it_returns_to() {
        let target = display(1.0, (0, 0));
        let options = WindowOptions::default()
            .mode(WindowMode::Maximized)
            .normal_geometry(Some(WindowGeometry::new(
                Some(Point::new(100.0, 80.0)),
                Size::new(700.0, 500.0),
            )));
        let spot = open_spot(&options, Some(&target), 1.0, with_frame);
        assert_eq!(spot.normal, Some((100, 80, 800, 580)));
    }

    fn a_normal_rectangle_without_a_position_follows_the_windows_own() {
        let options = WindowOptions::default()
            .position(Some(Point::new(30.0, 40.0)))
            .normal_geometry(Some(WindowGeometry::new(None, Size::new(300.0, 200.0))));
        let spot = open_spot(&options, None, 1.0, with_frame);
        assert_eq!(spot.normal, Some((30, 40, 330, 240)));
        let unplaced = WindowOptions::default()
            .normal_geometry(Some(WindowGeometry::new(None, Size::new(300.0, 200.0))));
        assert_eq!(open_spot(&unplaced, None, 1.0, with_frame).normal, None);
    }

    fn an_opening_rectangle_at_the_edge_of_the_range_saturates() {
        let options = WindowOptions::default()
            .mode(WindowMode::Maximized)
            .normal_geometry(Some(WindowGeometry::new(
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
        let platform = crate::WindowsPlatform::try_new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .size(Size::new(640.0, 480.0))
                    .visible(false),
                flui_testing::window_handler::RecordingWindowHandler::new(),
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
        let platform = crate::WindowsPlatform::try_new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default().visible(false),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("window");
        window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("maximize");
        window
            .admit_and_apply(WindowCommand::SetSize(Size::new(500.0, 400.0)))
            .expect("resize while maximized");
        assert_eq!(
            window.state().mode,
            WindowMode::Maximized,
            "still maximized"
        );
        window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Normal))
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
        let platform = crate::WindowsPlatform::try_new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .mode(WindowMode::Maximized)
                    .normal_geometry(Some(WindowGeometry::new(
                        Some(Point::new(120.0, 100.0)),
                        Size::new(700.0, 500.0),
                    ))),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("window");
        assert_eq!(window.state().mode, WindowMode::Maximized);
        window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Normal))
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
        let platform = crate::WindowsPlatform::try_new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .size(Size::new(640.0, 480.0))
                    .position(Some(Point::new(150.0, 120.0)))
                    .visible(false),
                flui_testing::window_handler::RecordingWindowHandler::new(),
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
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("maximize");
        same_as_normal(window.state(), "a maximized window");
        window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Fullscreen))
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
