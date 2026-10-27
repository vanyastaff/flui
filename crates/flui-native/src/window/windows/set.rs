//! The windows of a Win32 platform: [`WindowSet`], which the loop asks for
//! the deadline redraws and the modal paints, the window position that opens
//! them into it, and where a window opens and returns to (its placement).

use std::{collections::HashMap, sync::Arc};

use flui_foundation::geometry::{Bounds, Point, Size};
use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::UpdateWindow,
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowPlacement, IsWindowVisible, WINDOWPLACEMENT,
    },
};

use flui_platform::{PlatformWindow, WindowCapabilities, WindowId, WindowOptions};

use super::proc::{WindowRef, to_screen, with_window_context_checked};
use super::window::{WindowsWindow, window_capabilities};
use crate::host::WindowPositions;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::handlers::OpenWindows;
use crate::owner::windows::{ClassWindows, WindowsLoop};
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
    by_handle: Mutex<HashMap<isize, Arc<WindowsWindow>>>,
    pub(super) config: WindowConfiguration,
    /// What every window hands out of the window-level positions.
    pub(super) positions: WindowPositions<WindowRef>,
}

impl WindowSet {
    /// No window yet; every window opens with `config` and hands out
    /// `positions`.
    pub(crate) fn new(config: WindowConfiguration, positions: WindowPositions<WindowRef>) -> Self {
        Self {
            by_handle: Mutex::new(HashMap::new()),
            config,
            positions,
        }
    }

    /// Stops tracking the window `hwnd` names, when it is the one with
    /// `identity` (the OS recycles handles): the wrapper the set held, to be
    /// dropped once the set's lock is released, and whether no window is left.
    pub(crate) fn remove(
        &self,
        hwnd: HWND,
        identity: crate::owner::windows::WindowIdentity,
    ) -> Option<(Arc<WindowsWindow>, bool)> {
        let key = hwnd.0 as isize;
        let mut windows = self.by_handle.lock();
        if windows
            .get(&key)
            .is_some_and(|window| window.identity == identity)
        {
            windows
                .remove(&key)
                .map(|window| (window, windows.is_empty()))
        } else {
            None
        }
    }
}

impl std::fmt::Debug for WindowSet {
    // `try_lock`, never `lock`: `parking_lot::Mutex` is not reentrant and
    // BLOCKS rather than panicking, so formatting this value while the same
    // thread already holds the set would deadlock silently — and a Debug impl
    // gets called from assertion messages and `tracing` fields, which is
    // exactly where a lock is likely to be held. Same pattern
    // `parking_lot::Mutex<T>: Debug` itself uses.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("WindowSet");
        match self.by_handle.try_lock() {
            Some(windows) => out.field("windows", &windows.len()),
            None => out.field("windows", &format_args!("<locked>")),
        };
        out.field("config", &self.config)
            .field("positions", &self.positions)
            .finish()
    }
}

impl OpenWindows for WindowSet {
    fn is_empty(&self) -> bool {
        self.by_handle.lock().is_empty()
    }
}

impl ClassWindows for WindowSet {
    /// The main loop's delivery of a wake deadline, and the modal drag
    /// loop's.
    fn redraw_for_deadline(&self) {
        // Traverse a bounded identity snapshot without a per-frame Vec.
        // A minimized window's frame request runs user code, which may open
        // or close windows; each owning Arc is cloned under the set's lock and
        // used only after releasing it.
        let Some(limit) = self
            .by_handle
            .lock()
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
                .lock()
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
        let painted: Vec<HWND> = self
            .by_handle
            .lock()
            .values()
            .map(|window| window.hwnd())
            .collect();
        for hwnd in painted {
            // SAFETY: a window this thread created; a handle closed by an
            // earlier paint fails the call harmlessly. Nothing is borrowed.
            let _ = unsafe { UpdateWindow(hwnd) };
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
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
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
            Arc::clone(windows),
            shares,
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
        // The input set the window's capabilities when the class announced
        // it; the wake resumes the input held behind them.
        let _ = with_window_context_checked(window.hwnd(), "input_wake", |context| {
            context.callbacks.set_input_wake(wake);
        });

        // Store window
        let _prev = windows.by_handle.lock().insert(hwnd_value, window.clone());

        Ok(window)
    }

    fn active(owner: &WindowsLoop, _: &WindowSet) -> Option<WindowId> {
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

    fn capabilities(_: &WindowsLoop, _: &WindowSet) -> WindowCapabilities {
        window_capabilities()
    }

    fn frame_inset(_: &WindowsLoop, _: &WindowSet, _: f64) -> Size<f64> {
        // The frame is not measured before a window exists, so the host
        // reports the contract's default: no inset.
        Size::new(0.0, 0.0)
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
/// does not keep.
pub(crate) fn outer_size(
    style: u32,
    ex_style: u32,
    dpi: u32,
    content: (i32, i32),
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
