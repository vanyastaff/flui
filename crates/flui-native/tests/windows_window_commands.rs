//! The Win32 window against the real operating system: it opens hidden and is
//! shown in the mode it opened in, every command lands in the next snapshot,
//! the size limits hold, and a state the system changes under the window
//! reaches the snapshot.
//!
//! Each case opens its own platform and window. Windows that are shown appear
//! on the desktop for the length of a case.

#![cfg(target_os = "windows")]
#![expect(
    unsafe_code,
    reason = "native Win32 calls against the real window system"
)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Point, Size};
use flui_native::{HostWindow, Platform as _, WindowsPlatform, platforms::windows::WindowsWindow};
use flui_platform::{
    DispatchEventResult, SizeConstraints, WindowCommand, WindowCommandError, WindowGeometry,
    WindowMode, WindowOptions, WindowReveal, WindowState,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute},
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    },
    UI::{
        Input::KeyboardAndMouse::{VK_ESCAPE, VK_F11},
        WindowsAndMessaging::{
            DispatchMessageW, GWL_STYLE, GetWindowLongPtrW, GetWindowRect, IsIconic,
            IsWindowVisible, IsZoomed, MSG, PM_REMOVE, PeekMessageW, PostMessageW, SC_MOVE,
            SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SendMessageW,
            SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
            WM_DPICHANGED, WM_KEYDOWN, WM_SYSCOMMAND, WM_WTSSESSION_CHANGE, WS_CAPTION,
            WS_MAXIMIZEBOX, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
    },
};
use windows::core::{BOOL, w};

const CONTENT: Size<f64> = Size::new(640.0, 480.0);

fn platform() -> WindowsPlatform {
    WindowsPlatform::new().expect("native Windows platform")
}

fn options() -> WindowOptions {
    WindowOptions::default()
        .with_title("window commands")
        .with_size(CONTENT)
        .with_position(Some(Point::new(100.0, 80.0)))
}

fn hidden() -> WindowOptions {
    options().with_visible(false)
}

fn deferred() -> WindowOptions {
    options()
        .with_visible(true)
        .with_reveal(WindowReveal::AfterFirstFrame)
}

fn open(platform: &WindowsPlatform, options: WindowOptions) -> Arc<dyn HostWindow> {
    platform.open_window(options).expect("a Win32 window opens")
}

fn hwnd_of(window: &Arc<dyn HostWindow>) -> HWND {
    window
        .as_any()
        .downcast_ref::<WindowsWindow>()
        .expect("the Win32 backend")
        .hwnd()
}

fn shown(window: &Arc<dyn HostWindow>) -> bool {
    // SAFETY: a plain query of the live window, on its creating thread.
    unsafe { IsWindowVisible(hwnd_of(window)) }.as_bool()
}

fn style(window: &Arc<dyn HostWindow>) -> u32 {
    // SAFETY: a plain query of the live window.
    unsafe { GetWindowLongPtrW(hwnd_of(window), GWL_STYLE) as u32 }
}

fn rect(window: &Arc<dyn HostWindow>) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a stack local out-parameter.
    unsafe { GetWindowRect(hwnd_of(window), &raw mut rect) }.expect("the window rectangle");
    rect
}

fn monitor_rect(window: &Arc<dyn HostWindow>) -> RECT {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a stack local with `cbSize` set.
    unsafe {
        let monitor = MonitorFromWindow(hwnd_of(window), MONITOR_DEFAULTTONEAREST);
        assert!(GetMonitorInfoW(monitor, &raw mut info).as_bool());
    }
    info.rcMonitor
}

fn covers_its_monitor(window: &Arc<dyn HostWindow>) -> bool {
    let (window_rect, monitor) = (rect(window), monitor_rect(window));
    window_rect.left == monitor.left
        && window_rect.top == monitor.top
        && window_rect.right == monitor.right
        && window_rect.bottom == monitor.bottom
}

fn near(actual: f64, wanted: f64) -> bool {
    (actual - wanted).abs() <= 2.0
}

fn content_is(state: &WindowState, width: f64, height: f64) -> bool {
    near(state.content_size.width, width) && near(state.content_size.height, height)
}

fn apply(window: &Arc<dyn HostWindow>, command: WindowCommand) {
    window
        .apply(command.clone())
        .unwrap_or_else(|error| panic!("{command:?} is supported: {error}"));
}

/// Run the thread's window messages for `limit` or until `done` holds.
fn pump_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        let mut message = MSG::default();
        // SAFETY: `message` is a stack local out-parameter; the messages are
        // this thread's own.
        unsafe {
            while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

// --- the first show -------------------------------------------------------

fn a_window_opened_at_open_is_shown() {
    let platform = platform();
    let window = open(&platform, options());
    assert!(shown(&window));
    assert!(window.state().visible);
}

fn a_deferred_window_is_hidden_until_its_first_frame() {
    let platform = platform();
    let window = open(&platform, deferred());
    assert!(!shown(&window), "no blank frame before the first present");
    assert!(
        window.is_visible() && window.state().visible,
        "it is visible to the runtime meanwhile, so its frames run"
    );
    window.reveal_after_first_frame();
    assert!(shown(&window));
    window.reveal_after_first_frame();
    assert!(shown(&window), "a second call changes nothing");
}

fn a_window_opened_hidden_stays_hidden_until_shown() {
    let platform = platform();
    let window = open(&platform, hidden());
    assert!(!shown(&window));
    window.reveal_after_first_frame();
    assert!(!shown(&window), "the reveal call is for deferred windows");
    apply(&window, WindowCommand::Show);
    assert!(shown(&window));
}

fn activating_an_unshown_window_shows_it() {
    let platform = platform();
    let window = open(&platform, deferred());
    window.activate();
    assert!(shown(&window));
}

fn a_maximized_window_is_first_seen_maximized() {
    let platform = platform();
    let normal = WindowGeometry::new(Some(Point::new(120.0, 100.0)), Size::new(700.0, 500.0));
    let window = open(
        &platform,
        deferred()
            .with_mode(WindowMode::Maximized)
            .with_normal_geometry(Some(normal)),
    );
    assert!(!shown(&window));
    window.reveal_after_first_frame();
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Maximized);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let outer = window.state().outer;
    assert!(near(outer.size.width, 700.0) && near(outer.size.height, 500.0));
    let at = outer.position.expect("a position");
    assert!(near(at.x, 120.0) && near(at.y, 100.0), "restored at {at:?}");
}

fn a_hidden_window_shown_later_is_shown_in_its_opening_mode() {
    let platform = platform();
    let window = open(&platform, hidden().with_mode(WindowMode::Maximized));
    assert!(!shown(&window));
    apply(&window, WindowCommand::Show);
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Maximized);
}

fn a_mode_set_on_a_hidden_window_decides_how_it_is_first_shown() {
    let platform = platform();
    let window = open(&platform, hidden().with_mode(WindowMode::Maximized));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(!shown(&window), "setting the normal mode shows nothing");
    apply(&window, WindowCommand::Show);
    // SAFETY: a plain query of the live window.
    assert!(!unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Normal);
}

fn a_fullscreen_window_covers_its_monitor_before_it_is_shown() {
    let platform = platform();
    let window = open(&platform, deferred().with_mode(WindowMode::Fullscreen));
    assert!(!shown(&window));
    assert!(covers_its_monitor(&window), "entered while hidden");
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    window.reveal_after_first_frame();
    assert!(shown(&window));
    assert!(covers_its_monitor(&window));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(shown(&window), "leaving fullscreen keeps the window shown");
    assert_eq!(window.state().mode, WindowMode::Normal);
    assert!(!covers_its_monitor(&window));
    assert_ne!(style(&window) & WS_CAPTION.0, 0, "the frame is back");
}

fn a_mode_set_before_the_first_frame_waits_for_it() {
    let platform = platform();
    let window = open(&platform, deferred());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    assert!(!shown(&window), "setting a mode does not reveal early");
    window.reveal_after_first_frame();
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
}

fn hiding_a_deferred_window_cancels_its_reveal() {
    let platform = platform();
    let window = open(&platform, deferred());
    apply(&window, WindowCommand::Hide);
    assert!(!window.is_visible() && !window.state().visible);
    window.reveal_after_first_frame();
    assert!(!shown(&window), "the first frame does not undo a hide");
    apply(&window, WindowCommand::Show);
    assert!(shown(&window));
}

fn a_hidden_window_still_runs_its_frame_requests() {
    let platform = platform();
    let window = open(&platform, deferred());
    let frames = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&frames);
    window.on_request_frame(Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    window.request_redraw();
    assert!(
        pump_until(Duration::from_secs(2), || frames.load(Ordering::SeqCst) > 0),
        "a hidden window is never painted, so the request must reach its frame another way"
    );
}

// --- commands -------------------------------------------------------------

fn hide_and_show_follow_the_native_window() {
    let platform = platform();
    let window = open(&platform, options());
    let before = window.state().revision;
    apply(&window, WindowCommand::Hide);
    assert!(!shown(&window));
    let hidden = window.state();
    assert!(!hidden.visible && hidden.revision > before);
    apply(&window, WindowCommand::Show);
    assert!(shown(&window) && window.state().visible);
}

fn a_position_set_on_a_normal_window_moves_it() {
    let platform = platform();
    let window = open(&platform, hidden());
    apply(
        &window,
        WindowCommand::SetPosition(Point::new(220.0, 160.0)),
    );
    let at = window.state().outer.position.expect("a position");
    assert!(near(at.x, 220.0) && near(at.y, 160.0), "moved to {at:?}");
}

fn a_position_set_while_maximized_is_where_it_is_restored() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(
        &window,
        WindowCommand::SetPosition(Point::new(300.0, 250.0)),
    );
    assert_eq!(
        window.state().mode,
        WindowMode::Maximized,
        "still maximized"
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let at = window.state().outer.position.expect("a position");
    assert!(near(at.x, 300.0) && near(at.y, 250.0), "restored at {at:?}");
}

fn a_position_set_while_fullscreen_is_where_it_returns_to() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(
        &window,
        WindowCommand::SetPosition(Point::new(310.0, 260.0)),
    );
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let at = window.state().outer.position.expect("a position");
    assert!(near(at.x, 310.0) && near(at.y, 260.0), "returned to {at:?}");
}

fn the_frame_follows_resizable_and_decorated_and_keeps_the_content() {
    let platform = platform();
    let window = open(&platform, hidden().with_resizable(false));
    assert_eq!(style(&window) & (WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0), 0);
    assert!(!window.state().resizable);
    apply(&window, WindowCommand::SetResizable(true));
    assert_ne!(style(&window) & WS_THICKFRAME.0, 0);
    let state = window.state();
    assert!(
        state.resizable && content_is(&state, 640.0, 480.0),
        "{state:?}"
    );

    apply(&window, WindowCommand::SetDecorated(false));
    let state = window.state();
    assert!(!state.decorated, "no title bar");
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");
    assert!(
        !state.resizable,
        "a window without a frame has no sizing border"
    );

    apply(&window, WindowCommand::SetDecorated(true));
    let state = window.state();
    assert!(
        state.decorated && state.resizable,
        "the wish is kept: {state:?}"
    );
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");

    apply(&window, WindowCommand::SetResizable(false));
    assert!(!window.state().resizable);
}

fn the_limits_hold_against_resizing() {
    let platform = platform();
    let window = open(
        &platform,
        hidden()
            .with_min_size(Some(Size::new(400.0, 300.0)))
            .with_max_size(Some(Size::new(800.0, 600.0))),
    );
    apply(&window, WindowCommand::SetSize(Size::new(100.0, 100.0)));
    assert!(
        content_is(&window.state(), 400.0, 300.0),
        "{:?}",
        window.state()
    );
    apply(&window, WindowCommand::SetSize(Size::new(2000.0, 2000.0)));
    assert!(
        content_is(&window.state(), 800.0, 600.0),
        "{:?}",
        window.state()
    );
    assert_eq!(
        window.state().constraints,
        SizeConstraints::new(Some(Size::new(400.0, 300.0)), Some(Size::new(800.0, 600.0)))
    );
}

fn limits_set_later_apply_to_the_size_the_window_has() {
    let platform = platform();
    let window = open(&platform, hidden());
    let maximum = SizeConstraints::new(None, Some(Size::new(500.0, 400.0)));
    apply(&window, WindowCommand::SetConstraints(maximum));
    assert!(
        content_is(&window.state(), 500.0, 400.0),
        "{:?}",
        window.state()
    );
    assert_eq!(window.state().constraints, maximum);

    apply(
        &window,
        WindowCommand::SetConstraints(SizeConstraints::NONE),
    );
    apply(&window, WindowCommand::SetSize(Size::new(900.0, 700.0)));
    assert!(
        content_is(&window.state(), 900.0, 700.0),
        "{:?}",
        window.state()
    );
}

fn a_frame_changed_in_fullscreen_keeps_the_content_it_returns_to() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetDecorated(false));
    apply(&window, WindowCommand::SetDecorated(true));
    apply(&window, WindowCommand::SetResizable(false));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");
    assert!(!state.resizable);
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetDecorated(false));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(
        !state.decorated && content_is(&state, 640.0, 480.0),
        "{state:?}"
    );
}

fn normal_on_a_hidden_maximized_window_restores_it_without_showing() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(&window, WindowCommand::Hide);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(!shown(&window), "restoring does not show it");
    assert_eq!(window.state().mode, WindowMode::Normal);
    apply(&window, WindowCommand::Show);
    // SAFETY: a plain query of the live window.
    assert!(!unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert!(shown(&window));
}

fn a_size_set_under_limits_while_maximized_still_restores_within_them() {
    let platform = platform();
    let window = open(&platform, options().with_decorated(false));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(
        &window,
        WindowCommand::SetConstraints(SizeConstraints::new(None, Some(Size::new(400.0, 300.0)))),
    );
    apply(&window, WindowCommand::SetSize(Size::new(900.0, 700.0)));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let content = window.state().content_size;
    assert!(
        content.width <= 402.0 && content.height <= 302.0,
        "restored to {content:?} past the maximum"
    );
}

fn a_frame_changed_while_maximized_keeps_the_content_it_restores_to() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(&window, WindowCommand::SetDecorated(false));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(!state.decorated);
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");
}

fn limits_set_while_maximized_hold_when_it_is_restored() {
    let platform = platform();
    // Without a sizing border the system does not ask for the limits on its own.
    let window = open(&platform, options().with_decorated(false));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(
        &window,
        WindowCommand::SetConstraints(SizeConstraints::new(None, Some(Size::new(400.0, 300.0)))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(
        state.content_size.width <= 402.0 && state.content_size.height <= 302.0,
        "restored to {:?} past the maximum",
        state.content_size
    );
}

fn limits_set_in_fullscreen_hold_when_it_ends() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(
        &window,
        WindowCommand::SetConstraints(SizeConstraints::new(None, Some(Size::new(400.0, 300.0)))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(
        state.content_size.width <= 402.0 && state.content_size.height <= 302.0,
        "returned to {:?} past the maximum",
        state.content_size
    );
}

fn a_state_read_inside_a_callback_never_sees_a_half_made_change() {
    let platform = platform();
    let window = open(&platform, options());
    let before = window.state().revision;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let reader = Arc::clone(&window);
    window.on_resize(Box::new(move |_| {
        log.lock().expect("log").push(reader.state().revision);
    }));
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    let seen = seen.lock().expect("log").clone();
    assert!(!seen.is_empty(), "entering fullscreen resizes the window");
    assert!(
        seen.iter().all(|revision| *revision == before),
        "a callback saw revisions {seen:?}, not the whole one before {before:?}"
    );
}

fn a_maximum_caps_maximizing() {
    let platform = platform();
    let window = open(&platform, options());
    apply(
        &window,
        WindowCommand::SetConstraints(SizeConstraints::new(None, Some(Size::new(500.0, 400.0)))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    let content = window.state().content_size;
    assert!(
        content.width <= 502.0 && content.height <= 402.0,
        "maximized to {content:?} past the maximum"
    );
}

fn fullscreen_ignores_the_limits() {
    let platform = platform();
    let window = open(
        &platform,
        options().with_max_size(Some(Size::new(500.0, 400.0))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert!(covers_its_monitor(&window));
}

// --- fullscreen from another mode -----------------------------------------

fn fullscreen_entered_from_maximized_returns_to_normal() {
    let platform = platform();
    let window = open(&platform, options());
    let normal = window.state().outer;
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    let hwnd = hwnd_of(&window);
    // SAFETY: a plain query of the live window.
    assert!(!unsafe { IsZoomed(hwnd) }.as_bool(), "no longer maximized");
    assert!(covers_its_monitor(&window));
    assert_eq!(window.state().mode, WindowMode::Fullscreen);

    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert_eq!(state.mode, WindowMode::Normal);
    // SAFETY: a plain query of the live window.
    assert!(!unsafe { IsZoomed(hwnd) }.as_bool());
    assert!(near(state.outer.size.width, normal.size.width));
    assert!(near(state.outer.size.height, normal.size.height));
    assert!(shown(&window));
}

fn fullscreen_entered_from_minimized_returns_to_normal() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
    assert_eq!(window.state().mode, WindowMode::Minimized);
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    // SAFETY: a plain query of the live window.
    assert!(!unsafe { IsIconic(hwnd_of(&window)) }.as_bool());
    assert!(covers_its_monitor(&window));
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert_eq!(window.state().mode, WindowMode::Normal);
}

fn a_minimized_fullscreen_window_answers_commands_as_fullscreen() {
    let platform = platform();
    let window = open(&platform, options());
    let normal = window.state().outer;
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
    apply(&window, WindowCommand::SetDecorated(false));
    apply(&window, WindowCommand::SetDecorated(true));
    // Asking for fullscreen again brings the window back, once.
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    assert!(covers_its_monitor(&window));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert_eq!(state.mode, WindowMode::Normal);
    assert!(state.decorated, "the frame is back");
    assert!(near(state.outer.size.width, normal.size.width), "{state:?}");
    assert!(
        near(state.outer.size.height, normal.size.height),
        "{state:?}"
    );
}

fn maximizing_a_fullscreen_window_leaves_fullscreen() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Maximized);
    assert_ne!(style(&window) & WS_CAPTION.0, 0, "it has its frame");
}

fn entering_fullscreen_and_changing_the_frame_each_publish_one_snapshot() {
    let platform = platform();
    let window = open(&platform, options());
    let before = window.state().revision.get();
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert_eq!(
        window.state().revision.get(),
        before + 1,
        "a subscriber never sees the window half-way into fullscreen"
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let before = window.state().revision.get();
    apply(&window, WindowCommand::SetDecorated(false));
    assert_eq!(window.state().revision.get(), before + 1);
}

fn a_window_without_a_sizing_border_holds_its_limits() {
    let platform = platform();
    let window = open(
        &platform,
        hidden()
            .with_decorated(false)
            .with_max_size(Some(Size::new(500.0, 400.0))),
    );
    apply(&window, WindowCommand::SetSize(Size::new(900.0, 700.0)));
    let content = window.state().content_size;
    assert!(
        content.width <= 502.0 && content.height <= 402.0,
        "grew to {content:?} past the maximum"
    );
}

fn a_position_beyond_the_screens_does_not_overflow() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    let _ = window.apply(WindowCommand::SetPosition(Point::new(3.0e9, 3.0e9)));
    let _ = window.apply(WindowCommand::SetSize(Size::new(3.0e9, 3.0e9)));
    let _ = window.apply(WindowCommand::SetPosition(Point::new(-3.0e9, -3.0e9)));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
}

fn maximizing_a_hidden_window_shows_it_but_the_normal_mode_does_not() {
    let platform = platform();
    let window = open(&platform, hidden());
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(!shown(&window));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    assert!(shown(&window), "Win32 shows a window it maximizes");
}

fn a_hidden_windows_frame_request_reaches_the_global_redraw_event() {
    let platform = platform();
    let redraws = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&redraws);
    platform.on_window_event(Box::new(move |event| {
        if matches!(event, flui_platform::WindowEvent::RedrawRequested { .. }) {
            counted.fetch_add(1, Ordering::SeqCst);
        }
    }));
    let window = open(&platform, deferred());
    window.request_redraw();
    assert!(pump_until(Duration::from_secs(2), || redraws
        .load(Ordering::SeqCst)
        > 0));
}

fn record_visibility(window: &Arc<dyn HostWindow>) -> Arc<std::sync::Mutex<Vec<bool>>> {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    window.on_visibility_status_change(Box::new(move |visible| {
        log.lock().expect("log").push(visible);
    }));
    events
}

fn a_show_asked_for_while_a_hidden_window_is_restored_is_kept() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
    apply(&window, WindowCommand::Hide);
    let events = record_visibility(&window);
    // The app answers the restore's resize by showing the window.
    let inner = Arc::clone(&window);
    window.on_resize(Box::new(move |_| {
        let _ = inner.apply(WindowCommand::Show);
    }));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(
        shown(&window),
        "the newer show request is not undone by the hide"
    );
    assert_eq!(*events.lock().expect("log"), [true]);
}

fn a_window_first_shown_minimized_is_never_reported_visible() {
    let platform = platform();
    let window = open(&platform, deferred().with_mode(WindowMode::Minimized));
    let events = record_visibility(&window);
    window.reveal_after_first_frame();
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsIconic(hwnd_of(&window)) }.as_bool());
    assert_eq!(
        *events.lock().expect("log"),
        [false],
        "told visible while it waited for a frame, it is then told it is not"
    );

    let window = open(&platform, hidden().with_mode(WindowMode::Minimized));
    let events = record_visibility(&window);
    apply(&window, WindowCommand::Show);
    assert!(events.lock().expect("log").is_empty());
}

fn restoring_a_hidden_minimized_window_reports_nothing_until_it_is_shown() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
    apply(&window, WindowCommand::Hide);
    let events = record_visibility(&window);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(
        events.lock().expect("log").is_empty(),
        "a hidden window did not become visible"
    );
    apply(&window, WindowCommand::Show);
    assert_eq!(*events.lock().expect("log"), [true]);
}

fn a_resize_does_not_end_fullscreen() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    // The system resizing a fullscreen window (a display change, a snap) is
    // not the user leaving fullscreen.
    // SAFETY: plain integers for this test's own live window.
    unsafe {
        SetWindowPos(
            hwnd_of(&window),
            None,
            0,
            0,
            900,
            700,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .expect("resize");
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
}

fn a_minimized_fullscreen_window_comes_back_fullscreen() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
    assert_eq!(window.state().mode, WindowMode::Minimized);
    // The task bar restoring it.
    // SAFETY: a plain call on this test's own live window.
    let _ = unsafe { ShowWindow(hwnd_of(&window), SW_RESTORE) };
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    assert!(covers_its_monitor(&window));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert_eq!(window.state().mode, WindowMode::Normal);
}

// --- what the system changes under the window ------------------------------

fn a_title_the_system_sets_reaches_the_snapshot() {
    let platform = platform();
    let window = open(&platform, hidden());
    let changed = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&changed);
    window.on_state_changed(Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    let before = window.state().revision;
    // SAFETY: a plain call on this test's own live window.
    unsafe { SetWindowTextW(hwnd_of(&window), w!("set from outside")) }.expect("title");
    assert!(
        changed.load(Ordering::SeqCst) > 0,
        "the change was announced"
    );
    let state = window.state();
    assert_eq!(state.title.as_deref(), Some("set from outside"));
    assert!(state.revision > before);
}

fn a_style_the_system_changes_reaches_the_snapshot() {
    let platform = platform();
    let window = open(&platform, hidden());
    assert!(window.state().resizable);
    let hwnd = hwnd_of(&window);
    // SAFETY: plain style calls on this test's own live window.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_THICKFRAME.0 as isize));
    }
    assert!(!window.state().resizable);
}

// --- frames, scale and visibility ----------------------------------------

/// Count the window's frame requests from here on.
fn count_frames(window: &Arc<dyn HostWindow>) -> Arc<AtomicUsize> {
    let frames = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&frames);
    window.on_request_frame(Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    frames
}

/// Run what the thread has queued for a while.
fn settle() {
    let _ = pump_until(Duration::from_millis(150), || false);
}

fn send(window: &Arc<dyn HostWindow>, message: u32, wparam: usize, lparam: isize) {
    // SAFETY: a message to the live window on its own thread; a pointer in
    // `lparam` is the caller's live local.
    unsafe {
        SendMessageW(
            hwnd_of(window),
            message,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
    };
}

/// A `WM_DPICHANGED` for `dpi` that suggests `suggested`.
fn dpi_changed(window: &Arc<dyn HostWindow>, dpi: usize, suggested: RECT) {
    send(
        window,
        WM_DPICHANGED,
        (dpi << 16) | dpi,
        (&raw const suggested) as isize,
    );
}

fn a_task_woken_while_minimized_gets_its_frame() {
    let platform = platform();
    let window = open(&platform, options());
    // SAFETY: a plain call on the live window.
    let _ = unsafe { ShowWindow(hwnd_of(&window), SW_MINIMIZE) };
    settle();
    let frames = count_frames(&window);
    window.request_redraw();
    assert!(
        pump_until(Duration::from_secs(1), || frames.load(Ordering::SeqCst) > 0),
        "Win32 paints no minimized window: the wake must reach its frame another way"
    );
}

fn a_dpi_change_that_keeps_the_client_size_still_delivers_the_scale() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let resizes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&resizes);
    window.on_resize(Box::new(move |metrics| {
        let scale = metrics.scale().get();
        log.lock().expect("log").push(scale);
    }));
    // The window keeps the rectangle it has, so no `WM_SIZE` follows.
    dpi_changed(&window, 144, rect(&window));
    assert_eq!(*resizes.lock().expect("log"), [1.5]);
    assert!(near(window.scale_factor(), 1.5));
    dpi_changed(&window, 96, rect(&window));
    assert_eq!(
        *resizes.lock().expect("log"),
        [1.5, 1.0],
        "delivered once each"
    );
}

fn a_fullscreen_window_covers_its_monitor_through_a_dpi_change() {
    let platform = platform();
    let window = open(&platform, options());
    let normal = rect(&window);
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert!(covers_its_monitor(&window));
    let monitor = monitor_rect(&window);
    let half = RECT {
        right: monitor.left + (monitor.right - monitor.left) / 2,
        bottom: monitor.top + (monitor.bottom - monitor.top) / 2,
        ..monitor
    };
    dpi_changed(&window, 144, half);
    assert!(
        covers_its_monitor(&window),
        "the suggestion is for a window, not for a monitor"
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let restored = rect(&window);
    let width = restored.right - restored.left;
    let wanted = f64::from(normal.right - normal.left) * 1.5;
    assert!(
        (f64::from(width) - wanted).abs() <= 3.0,
        "returns {width} wide, not the {wanted} the new scale makes of it"
    );
}

fn geometry_win32_cannot_hold_is_refused() {
    let platform = platform();
    let window = open(&platform, options());
    let before = window.state();
    for command in [
        WindowCommand::SetSize(Size::new(1.0e10, 100.0)),
        WindowCommand::SetSize(Size::new(100.0, 1.0e10)),
        WindowCommand::SetPosition(Point::new(1.0e12, 0.0)),
        WindowCommand::SetPosition(Point::new(0.0, -1.0e12)),
    ] {
        assert_eq!(
            window.apply(command.clone()),
            Err(WindowCommandError::InvalidGeometry),
            "{command:?}"
        );
    }
    let after = window.state();
    assert_eq!(after.content_size, before.content_size);
    assert_eq!(after.outer, before.outer);
}

fn press_f11(window: &Arc<dyn HostWindow>) {
    // F11's scan code is 0x57.
    send(window, WM_KEYDOWN, usize::from(VK_F11.0), 0x0057_0001);
}

fn the_fullscreen_hotkey_waits_for_the_app_to_let_its_key_through() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    // A toggle request that names another window (one whose handle this
    // window reuses) is not this window's.
    // SAFETY: a post to the live window; the parameters are plain values.
    let _ = unsafe {
        PostMessageW(
            Some(hwnd_of(&window)),
            windows::Win32::UI::WindowsAndMessaging::WM_APP + 3,
            WPARAM(0),
            LPARAM(0),
        )
    };
    settle();
    assert_ne!(window.state().mode, WindowMode::Fullscreen);

    window.on_input(Box::new(|_| DispatchEventResult::resolved(false, true)));
    press_f11(&window);
    settle();
    assert_ne!(
        window.state().mode,
        WindowMode::Fullscreen,
        "the app prevented it"
    );

    window.on_input(Box::new(|_| DispatchEventResult::resolved(true, false)));
    press_f11(&window);
    assert_ne!(
        window.state().mode,
        WindowMode::Fullscreen,
        "not inside the key's own message"
    );
    assert!(pump_until(Duration::from_secs(1), || {
        window.state().mode == WindowMode::Fullscreen
    }));
}

fn a_resize_outside_a_live_resize_asks_for_one_frame_after_it() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let frames = count_frames(&window);
    apply(&window, WindowCommand::SetSize(Size::new(600.0, 420.0)));
    apply(&window, WindowCommand::SetSize(Size::new(620.0, 440.0)));
    assert_eq!(
        frames.load(Ordering::SeqCst),
        0,
        "no frame at an intermediate size"
    );
    assert!(pump_until(Duration::from_secs(1), || frames
        .load(Ordering::SeqCst)
        > 0));
    settle();
    assert_eq!(frames.load(Ordering::SeqCst), 1);
}

fn deadlines_are_delivered_while_the_window_is_moved() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    platform.set_wake_deadline_hook(Box::new(|| Some(Instant::now())));
    let frames = count_frames(&window);
    let address = hwnd_of(&window).0 as isize;
    let moved = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&moved);
    let ender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        while !done.load(Ordering::SeqCst) {
            // SAFETY: a post to a window of this process; one that is gone
            // fails the call.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(address as *mut _)),
                    WM_KEYDOWN,
                    WPARAM(usize::from(VK_ESCAPE.0)),
                    LPARAM(0x0001_0001),
                )
            };
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    // The keyboard move loop, modal until the escape above ends it.
    send(&window, WM_SYSCOMMAND, SC_MOVE as usize, 0);
    moved.store(true, Ordering::SeqCst);
    ender.join().expect("the ending thread");
    let during = frames.load(Ordering::SeqCst);
    assert!(
        during >= 5,
        "{during} frames in the 300 ms the move loop ran"
    );
}

fn a_locked_session_or_a_cloak_hides_a_shown_window() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let events = record_visibility(&window);
    send(&window, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 1);
    assert_eq!(*events.lock().expect("log"), [false]);
    assert!(
        !window.is_visible(),
        "a runtime that first reads the window now must not see it visible"
    );
    send(
        &window,
        WM_WTSSESSION_CHANGE,
        WTS_SESSION_UNLOCK as usize,
        1,
    );
    assert_eq!(*events.lock().expect("log"), [false, true]);
    assert!(window.is_visible());

    let cloak = |on: bool| {
        let value = BOOL::from(on);
        // SAFETY: `value` is a live `BOOL`, the size `DWMWA_CLOAK` reads.
        unsafe {
            DwmSetWindowAttribute(
                hwnd_of(&window),
                DWMWA_CLOAK,
                (&raw const value).cast(),
                std::mem::size_of::<BOOL>() as u32,
            )
        }
        .expect("the window cloaks");
    };
    cloak(true);
    assert!(pump_until(Duration::from_secs(1), || {
        events.lock().expect("log").len() == 3
    }));
    cloak(false);
    assert!(pump_until(Duration::from_secs(1), || {
        events.lock().expect("log").len() == 4
    }));
    assert_eq!(*events.lock().expect("log"), [false, true, false, true]);
}

#[test]
fn native_window_commands() {
    let cases: &[(&str, fn())] = &[
        (
            "a_window_opened_at_open_is_shown",
            a_window_opened_at_open_is_shown,
        ),
        (
            "a_deferred_window_is_hidden_until_its_first_frame",
            a_deferred_window_is_hidden_until_its_first_frame,
        ),
        (
            "a_window_opened_hidden_stays_hidden_until_shown",
            a_window_opened_hidden_stays_hidden_until_shown,
        ),
        (
            "activating_an_unshown_window_shows_it",
            activating_an_unshown_window_shows_it,
        ),
        (
            "a_maximized_window_is_first_seen_maximized",
            a_maximized_window_is_first_seen_maximized,
        ),
        (
            "a_hidden_window_shown_later_is_shown_in_its_opening_mode",
            a_hidden_window_shown_later_is_shown_in_its_opening_mode,
        ),
        (
            "a_mode_set_on_a_hidden_window_decides_how_it_is_first_shown",
            a_mode_set_on_a_hidden_window_decides_how_it_is_first_shown,
        ),
        (
            "a_fullscreen_window_covers_its_monitor_before_it_is_shown",
            a_fullscreen_window_covers_its_monitor_before_it_is_shown,
        ),
        (
            "a_mode_set_before_the_first_frame_waits_for_it",
            a_mode_set_before_the_first_frame_waits_for_it,
        ),
        (
            "hiding_a_deferred_window_cancels_its_reveal",
            hiding_a_deferred_window_cancels_its_reveal,
        ),
        (
            "a_hidden_window_still_runs_its_frame_requests",
            a_hidden_window_still_runs_its_frame_requests,
        ),
        (
            "hide_and_show_follow_the_native_window",
            hide_and_show_follow_the_native_window,
        ),
        (
            "a_position_set_on_a_normal_window_moves_it",
            a_position_set_on_a_normal_window_moves_it,
        ),
        (
            "a_position_set_while_maximized_is_where_it_is_restored",
            a_position_set_while_maximized_is_where_it_is_restored,
        ),
        (
            "a_position_set_while_fullscreen_is_where_it_returns_to",
            a_position_set_while_fullscreen_is_where_it_returns_to,
        ),
        (
            "the_frame_follows_resizable_and_decorated_and_keeps_the_content",
            the_frame_follows_resizable_and_decorated_and_keeps_the_content,
        ),
        (
            "the_limits_hold_against_resizing",
            the_limits_hold_against_resizing,
        ),
        (
            "limits_set_later_apply_to_the_size_the_window_has",
            limits_set_later_apply_to_the_size_the_window_has,
        ),
        (
            "a_frame_changed_in_fullscreen_keeps_the_content_it_returns_to",
            a_frame_changed_in_fullscreen_keeps_the_content_it_returns_to,
        ),
        (
            "normal_on_a_hidden_maximized_window_restores_it_without_showing",
            normal_on_a_hidden_maximized_window_restores_it_without_showing,
        ),
        (
            "a_size_set_under_limits_while_maximized_still_restores_within_them",
            a_size_set_under_limits_while_maximized_still_restores_within_them,
        ),
        (
            "a_frame_changed_while_maximized_keeps_the_content_it_restores_to",
            a_frame_changed_while_maximized_keeps_the_content_it_restores_to,
        ),
        (
            "limits_set_while_maximized_hold_when_it_is_restored",
            limits_set_while_maximized_hold_when_it_is_restored,
        ),
        (
            "limits_set_in_fullscreen_hold_when_it_ends",
            limits_set_in_fullscreen_hold_when_it_ends,
        ),
        (
            "a_state_read_inside_a_callback_never_sees_a_half_made_change",
            a_state_read_inside_a_callback_never_sees_a_half_made_change,
        ),
        ("a_maximum_caps_maximizing", a_maximum_caps_maximizing),
        (
            "fullscreen_ignores_the_limits",
            fullscreen_ignores_the_limits,
        ),
        (
            "fullscreen_entered_from_maximized_returns_to_normal",
            fullscreen_entered_from_maximized_returns_to_normal,
        ),
        (
            "fullscreen_entered_from_minimized_returns_to_normal",
            fullscreen_entered_from_minimized_returns_to_normal,
        ),
        (
            "a_minimized_fullscreen_window_answers_commands_as_fullscreen",
            a_minimized_fullscreen_window_answers_commands_as_fullscreen,
        ),
        (
            "maximizing_a_fullscreen_window_leaves_fullscreen",
            maximizing_a_fullscreen_window_leaves_fullscreen,
        ),
        (
            "entering_fullscreen_and_changing_the_frame_each_publish_one_snapshot",
            entering_fullscreen_and_changing_the_frame_each_publish_one_snapshot,
        ),
        (
            "a_window_without_a_sizing_border_holds_its_limits",
            a_window_without_a_sizing_border_holds_its_limits,
        ),
        (
            "a_position_beyond_the_screens_does_not_overflow",
            a_position_beyond_the_screens_does_not_overflow,
        ),
        (
            "maximizing_a_hidden_window_shows_it_but_the_normal_mode_does_not",
            maximizing_a_hidden_window_shows_it_but_the_normal_mode_does_not,
        ),
        (
            "a_hidden_windows_frame_request_reaches_the_global_redraw_event",
            a_hidden_windows_frame_request_reaches_the_global_redraw_event,
        ),
        (
            "a_show_asked_for_while_a_hidden_window_is_restored_is_kept",
            a_show_asked_for_while_a_hidden_window_is_restored_is_kept,
        ),
        (
            "a_window_first_shown_minimized_is_never_reported_visible",
            a_window_first_shown_minimized_is_never_reported_visible,
        ),
        (
            "restoring_a_hidden_minimized_window_reports_nothing_until_it_is_shown",
            restoring_a_hidden_minimized_window_reports_nothing_until_it_is_shown,
        ),
        (
            "a_resize_does_not_end_fullscreen",
            a_resize_does_not_end_fullscreen,
        ),
        (
            "a_minimized_fullscreen_window_comes_back_fullscreen",
            a_minimized_fullscreen_window_comes_back_fullscreen,
        ),
        (
            "a_title_the_system_sets_reaches_the_snapshot",
            a_title_the_system_sets_reaches_the_snapshot,
        ),
        (
            "a_style_the_system_changes_reaches_the_snapshot",
            a_style_the_system_changes_reaches_the_snapshot,
        ),
        (
            "a_task_woken_while_minimized_gets_its_frame",
            a_task_woken_while_minimized_gets_its_frame,
        ),
        (
            "a_dpi_change_that_keeps_the_client_size_still_delivers_the_scale",
            a_dpi_change_that_keeps_the_client_size_still_delivers_the_scale,
        ),
        (
            "a_fullscreen_window_covers_its_monitor_through_a_dpi_change",
            a_fullscreen_window_covers_its_monitor_through_a_dpi_change,
        ),
        (
            "geometry_win32_cannot_hold_is_refused",
            geometry_win32_cannot_hold_is_refused,
        ),
        (
            "the_fullscreen_hotkey_waits_for_the_app_to_let_its_key_through",
            the_fullscreen_hotkey_waits_for_the_app_to_let_its_key_through,
        ),
        (
            "a_resize_outside_a_live_resize_asks_for_one_frame_after_it",
            a_resize_outside_a_live_resize_asks_for_one_frame_after_it,
        ),
        (
            "deadlines_are_delivered_while_the_window_is_moved",
            deadlines_are_delivered_while_the_window_is_moved,
        ),
        (
            "a_locked_session_or_a_cloak_hides_a_shown_window",
            a_locked_session_or_a_cloak_hides_a_shown_window,
        ),
    ];
    crate::run_table("native_window_commands", cases);
}
