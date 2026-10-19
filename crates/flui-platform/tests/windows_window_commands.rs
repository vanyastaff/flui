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
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    HostWindow, Platform as _, SizeConstraints, WindowCommand, WindowGeometry, WindowMode,
    WindowOptions, WindowReveal, WindowState, WindowsPlatform, platforms::windows::WindowsWindow,
};
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    UI::WindowsAndMessaging::{
        DispatchMessageW, GWL_STYLE, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible,
        IsZoomed, MSG, PM_REMOVE, PeekMessageW, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
        TranslateMessage, WS_CAPTION, WS_MAXIMIZEBOX, WS_THICKFRAME,
    },
};
use windows::core::w;

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
    window.on_resize(Box::new(move |_, _| {
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
    ];
    crate::run_table("native_window_commands", cases);
}
