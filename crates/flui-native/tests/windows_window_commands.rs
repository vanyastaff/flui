//! The Win32 window against the real operating system: it opens hidden and is
//! shown in the mode it opened in, every command lands in the next snapshot,
//! the size limits hold, and a state the system changes under the window
//! reaches the snapshot.
//!
//! Each case opens its own platform and window. Under nextest the cases run
//! on the test run's own desktop, which nobody sees and no mouse or keyboard
//! reaches (docs/testing.md, "The Windows test desktop").

#![cfg(target_os = "windows")]
#![expect(
    unsafe_code,
    reason = "native Win32 calls against the real window system"
)]

use flui_platform::CommandRefusal;
use flui_platform::WindowFrame;
use flui_testing::window_commands::AdmitAndApply as _;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Point, Size, WindowMetrics};
use flui_native::{HostWindow, Platform as _, WindowsPlatform};

use crate::Opened;
use flui_platform::{
    ApplyError, DispatchEventResult, DisplayId, MAX_SURFACE_EXTENT, PlatformFailure,
    PlatformWindow, SizeConstraints, WindowCommand, WindowExtent, WindowInteraction, WindowMode,
    WindowOptions, WindowPosition, WindowReveal, WindowState,
};
use flui_testing::window_conformance::{self, Readback, WindowFixture};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_CLOAK, DWMWA_CLOAKED, DwmGetWindowAttribute, DwmSetWindowAttribute},
        Gdi::{
            ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromWindow,
        },
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            GetActiveWindow, GetFocus, SetFocus, VK_ESCAPE, VK_F11, VK_RETURN, VK_RIGHT,
        },
        WindowsAndMessaging::{
            DispatchMessageW, GUI_INMOVESIZE, GUITHREADINFO, GWL_EXSTYLE, GWL_STYLE, GetClientRect,
            GetGUIThreadInfo, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
            HWND_TOPMOST, IsIconic, IsWindowVisible, IsZoomed, MSG, PM_REMOVE, PeekMessageW,
            PostMessageW, SC_MOVE, SC_SIZE, SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos,
            SetWindowTextW, ShowWindow, TranslateMessage, WA_ACTIVE, WA_INACTIVE, WM_ACTIVATE,
            WM_CLOSE, WM_DPICHANGED, WM_GETOBJECT, WM_KEYDOWN, WM_SYSCOMMAND, WM_WTSSESSION_CHANGE,
            WS_CAPTION, WS_MAXIMIZEBOX, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
    },
};
use windows::core::{BOOL, w};

const CONTENT: Size<f64> = Size::new(640.0, 480.0);

fn platform() -> WindowsPlatform {
    WindowsPlatform::try_new().expect("native Windows platform")
}

/// The options a case opens its window with; [`open`] places the window at
/// (100, 80) of the primary display where they name no position.
fn options() -> WindowOptions {
    WindowOptions::default()
        .title("window commands")
        .size(extent(CONTENT.width, CONTENT.height))
}

fn extent(width: f64, height: f64) -> WindowExtent {
    WindowExtent::try_new(Size::new(width, height)).expect("an extent")
}

/// The primary display of `platform`, which these windows open on.
fn primary(platform: &WindowsPlatform) -> DisplayId {
    let snapshot = platform.display_snapshot().expect("a display snapshot");
    snapshot
        .primary()
        .or_else(|| snapshot.displays().first())
        .map(|display| display.id)
        .expect("the desktop has a display")
}

/// The point `(x, y)` of the display `window` is on.
fn on_its_display(window: &Rc<dyn HostWindow>, x: f64, y: f64) -> WindowPosition {
    let display = window
        .state()
        .display
        .expect("a Win32 window names its display");
    WindowPosition::try_new(display, Point::new(x, y)).expect("a finite point")
}

fn hidden() -> WindowOptions {
    options().visible(false)
}

fn deferred() -> WindowOptions {
    options()
        .visible(true)
        .reveal(WindowReveal::AfterFirstFrame)
}

fn open(platform: &WindowsPlatform, options: WindowOptions) -> Opened {
    let at = options.position.unwrap_or_else(|| {
        WindowPosition::try_new(primary(platform), Point::new(100.0, 80.0)).expect("a finite point")
    });
    Opened::on(platform, options.position(Some(at))).expect("a Win32 window opens")
}

fn hwnd_of(window: &Rc<dyn HostWindow>) -> HWND {
    crate::win32_handle(window.as_ref())
}

fn shown(window: &Rc<dyn HostWindow>) -> bool {
    // SAFETY: a plain query of the live window, on its creating thread.
    unsafe { IsWindowVisible(hwnd_of(window)) }.as_bool()
}

fn style(window: &Rc<dyn HostWindow>) -> u32 {
    // SAFETY: a plain query of the live window.
    unsafe { GetWindowLongPtrW(hwnd_of(window), GWL_STYLE) as u32 }
}

fn rect(window: &Rc<dyn HostWindow>) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a stack local out-parameter.
    unsafe { GetWindowRect(hwnd_of(window), &raw mut rect) }.expect("the window rectangle");
    rect
}

fn monitor_rect(window: &Rc<dyn HostWindow>) -> RECT {
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

fn covers_its_monitor(window: &Rc<dyn HostWindow>) -> bool {
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
    near(state.content_size.width(), width) && near(state.content_size.height(), height)
}

fn apply(window: &Rc<dyn HostWindow>, command: WindowCommand) {
    window
        .admit_and_apply(command.clone())
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
        window.state().visible,
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
    apply(&window, WindowCommand::Activate);
    assert!(shown(&window));
}

fn a_maximized_window_is_first_seen_maximized() {
    let platform = platform();
    let normal = geometry(
        Some(
            WindowPosition::try_new(primary(&platform), Point::new(120.0, 100.0)).expect("a point"),
        ),
        Size::new(700.0, 500.0),
    );
    let window = open(
        &platform,
        deferred()
            .mode(WindowMode::Maximized)
            .normal_geometry(Some(normal)),
    );
    assert!(!shown(&window));
    window.reveal_after_first_frame();
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Maximized);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let outer = window.state().outer;
    assert!(near(outer.size().width, 700.0) && near(outer.size().height, 500.0));
    let at = outer.position().expect("a position");
    assert!(
        near(at.at().x, 120.0) && near(at.at().y, 100.0),
        "restored at {at:?}"
    );
}

fn a_hidden_window_shown_later_is_shown_in_its_opening_mode() {
    let platform = platform();
    let window = open(&platform, hidden().mode(WindowMode::Maximized));
    assert!(!shown(&window));
    apply(&window, WindowCommand::Show);
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Maximized);
}

fn a_mode_set_on_a_hidden_window_decides_how_it_is_first_shown() {
    let platform = platform();
    let window = open(&platform, hidden().mode(WindowMode::Maximized));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(!shown(&window), "setting the normal mode shows nothing");
    apply(&window, WindowCommand::Show);
    // SAFETY: a plain query of the live window.
    assert!(!unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_eq!(window.state().mode, WindowMode::Normal);
}

fn a_fullscreen_window_covers_its_monitor_before_it_is_shown() {
    let platform = platform();
    let window = open(&platform, deferred().mode(WindowMode::Fullscreen));
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
    assert!(!window.state().visible);
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
    window.handler.react_to_frame(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
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
        WindowCommand::SetPosition(on_its_display(&window, 220.0, 160.0)),
    );
    let at = window.state().outer.position().expect("a position");
    assert!(
        near(at.at().x, 220.0) && near(at.at().y, 160.0),
        "moved to {at:?}"
    );
}

fn a_position_set_while_maximized_is_where_it_is_restored() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(
        &window,
        WindowCommand::SetPosition(on_its_display(&window, 300.0, 250.0)),
    );
    assert_eq!(
        window.state().mode,
        WindowMode::Maximized,
        "still maximized"
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let at = window.state().outer.position().expect("a position");
    assert!(
        near(at.at().x, 300.0) && near(at.at().y, 250.0),
        "restored at {at:?}"
    );
}

fn a_position_set_while_fullscreen_is_where_it_returns_to() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(
        &window,
        WindowCommand::SetPosition(on_its_display(&window, 310.0, 260.0)),
    );
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let at = window.state().outer.position().expect("a position");
    assert!(
        near(at.at().x, 310.0) && near(at.at().y, 260.0),
        "returned to {at:?}"
    );
}

fn the_frame_follows_resizable_and_the_system_frame_and_keeps_the_content() {
    let platform = platform();
    let window = open(&platform, hidden().resizable(false));
    assert_eq!(style(&window) & (WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0), 0);
    assert!(!window.state().resizable);
    apply(&window, WindowCommand::SetResizable(true));
    assert_ne!(style(&window) & WS_THICKFRAME.0, 0);
    let state = window.state();
    assert!(
        state.resizable && content_is(&state, 640.0, 480.0),
        "{state:?}"
    );

    apply(&window, WindowCommand::SetFrame(WindowFrame::None));
    let state = window.state();
    assert!(!state.frame.has_system_title_bar(), "no title bar");
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");
    assert!(
        !state.resizable,
        "a window without a frame has no sizing border"
    );

    apply(&window, WindowCommand::SetFrame(WindowFrame::System));
    let state = window.state();
    assert!(
        state.frame.has_system_title_bar() && state.resizable,
        "the wish is kept: {state:?}"
    );
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");

    apply(&window, WindowCommand::SetResizable(false));
    assert!(!window.state().resizable);
}

fn an_app_title_bar_drops_the_caption_and_keeps_the_content() {
    use flui_platform::{AppTitleBar, SystemControls};

    let platform = platform();
    let requested =
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden));
    let window = open(&platform, hidden().frame(requested));
    let opened = window.state();
    assert_eq!(opened.frame, requested);
    assert_eq!(
        style(&window) & WS_CAPTION.0,
        WS_CAPTION.0,
        "the system still resizes, snaps and animates the window"
    );
    assert!(content_is(&opened, 640.0, 480.0), "{opened:?}");
    assert_eq!(
        client_top_on_screen(&window),
        rect(&window).top,
        "nothing lies above the content"
    );

    apply(&window, WindowCommand::SetFrame(WindowFrame::System));
    let system = window.state();
    assert_eq!(system.frame, WindowFrame::System);
    assert!(content_is(&system, 640.0, 480.0), "{system:?}");
    assert!(
        client_top_on_screen(&window) > rect(&window).top,
        "the caption is back"
    );

    apply(&window, WindowCommand::SetFrame(requested));
    let again = window.state();
    assert_eq!(again.frame, requested);
    assert!(content_is(&again, 640.0, 480.0), "{again:?}");
    assert_eq!(client_top_on_screen(&window), rect(&window).top);
}

fn the_system_hit_test_answers_from_the_chrome_regions() {
    use flui_foundation::geometry::Rect;
    use flui_platform::input_regions::{CaptionButton, InputRegion, InputRegionKind};
    use flui_platform::{AppTitleBar, SystemControls};
    use windows::Win32::UI::WindowsAndMessaging::{
        HTCAPTION, HTCLIENT, HTMAXBUTTON, HTTOP, WM_NCHITTEST,
    };

    let platform = platform();
    let frame =
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden));
    let window = open(&platform, hidden().frame(frame));
    let logical = |left, top, right, bottom| Rect::from_ltrb(left, top, right, bottom);
    window
        .set_input_regions(&[
            InputRegion::new(InputRegionKind::WindowMove, logical(0.0, 0.0, 640.0, 32.0)),
            InputRegion::new(
                InputRegionKind::WindowContent,
                logical(100.0, 4.0, 200.0, 28.0),
            ),
            InputRegion::new(
                InputRegionKind::CaptionButton(CaptionButton::Maximize),
                logical(560.0, 0.0, 600.0, 32.0),
            ),
        ])
        .expect("a Win32 window honours the chrome regions");
    let scale = window.state().metrics.expect("drawable").scale().get();
    let hit = |x: f64, y: f64| {
        let origin = client_origin_on_screen(&window);
        let x = origin.0 + (x * scale).round() as i32;
        let y = origin.1 + (y * scale).round() as i32;
        let packed = ((y as u32 & 0xffff) << 16) | (x as u32 & 0xffff);
        // SAFETY: a message sent to the live window on its creating thread, as
        // the system sends it on every pointer move.
        let answer = unsafe {
            SendMessageW(
                hwnd_of(&window),
                WM_NCHITTEST,
                Some(WPARAM(0)),
                Some(LPARAM(packed as i32 as isize)),
            )
        };
        answer.0 as u32
    };
    assert_eq!(hit(50.0, 20.0), HTCAPTION, "the bar moves the window");
    assert_eq!(hit(150.0, 20.0), HTCLIENT, "a field in the bar is content");
    assert_eq!(
        hit(580.0, 20.0),
        HTMAXBUTTON,
        "the maximize button shows the snap layouts"
    );
    assert_eq!(hit(50.0, 1.0), HTTOP, "the top band resizes the window");
    assert_eq!(hit(50.0, 200.0), HTCLIENT, "below the bar is content");
}

/// Where the window's client area starts on the screen, in device pixels.
fn client_origin_on_screen(window: &Rc<dyn HostWindow>) -> (i32, i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;

    let mut origin = POINT::default();
    // SAFETY: `origin` is a stack local in/out parameter of the live window.
    assert!(unsafe { ClientToScreen(hwnd_of(window), &raw mut origin) }.as_bool());
    (origin.x, origin.y)
}

fn client_top_on_screen(window: &Rc<dyn HostWindow>) -> i32 {
    client_origin_on_screen(window).1
}

fn the_limits_hold_against_resizing() {
    let platform = platform();
    let window = open(
        &platform,
        hidden()
            .min_size(Some(
                flui_platform::WindowExtent::try_new(Size::new(400.0, 300.0)).expect("a limit"),
            ))
            .max_size(Some(
                flui_platform::WindowExtent::try_new(Size::new(800.0, 600.0)).expect("a limit"),
            )),
    );
    apply(&window, WindowCommand::SetSize(extent(100.0, 100.0)));
    assert!(
        content_is(&window.state(), 400.0, 300.0),
        "{:?}",
        window.state()
    );
    apply(&window, WindowCommand::SetSize(extent(2000.0, 2000.0)));
    assert!(
        content_is(&window.state(), 800.0, 600.0),
        "{:?}",
        window.state()
    );
    assert_eq!(
        window.state().constraints,
        limits(Some(Size::new(400.0, 300.0)), Some(Size::new(800.0, 600.0)))
    );
}

fn limits_set_later_apply_to_the_size_the_window_has() {
    let platform = platform();
    let window = open(&platform, hidden());
    let maximum = limits(None, Some(Size::new(500.0, 400.0)));
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
    apply(&window, WindowCommand::SetSize(extent(900.0, 700.0)));
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
    apply(&window, WindowCommand::SetFrame(WindowFrame::None));
    apply(&window, WindowCommand::SetFrame(WindowFrame::System));
    apply(&window, WindowCommand::SetResizable(false));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");
    assert!(!state.resizable);
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetFrame(WindowFrame::None));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(
        !state.frame.has_system_title_bar() && content_is(&state, 640.0, 480.0),
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
    let window = open(&platform, options().frame(WindowFrame::None));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(
        &window,
        WindowCommand::SetConstraints(limits(None, Some(Size::new(400.0, 300.0)))),
    );
    apply(&window, WindowCommand::SetSize(extent(900.0, 700.0)));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let content = window.state().content_size.get();
    assert!(
        content.width <= 402.0 && content.height <= 302.0,
        "restored to {content:?} past the maximum"
    );
}

fn a_frame_changed_while_maximized_keeps_the_content_it_restores_to() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(&window, WindowCommand::SetFrame(WindowFrame::None));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(!state.frame.has_system_title_bar());
    assert!(content_is(&state, 640.0, 480.0), "{state:?}");
}

fn limits_set_while_maximized_hold_when_it_is_restored() {
    let platform = platform();
    // Without a sizing border the system does not ask for the limits on its own.
    let window = open(&platform, options().frame(WindowFrame::None));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    apply(
        &window,
        WindowCommand::SetConstraints(limits(None, Some(Size::new(400.0, 300.0)))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(
        state.content_size.width() <= 402.0 && state.content_size.height() <= 302.0,
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
        WindowCommand::SetConstraints(limits(None, Some(Size::new(400.0, 300.0)))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert!(
        state.content_size.width() <= 402.0 && state.content_size.height() <= 302.0,
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
    on_changes(&window, move |window, previous, next| {
        if previous.metrics != next.metrics {
            log.lock().expect("log").push(window.state().revision);
        }
    });
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    let after = window.state().revision;
    let seen = seen.lock().expect("log").clone();
    assert!(!seen.is_empty(), "entering fullscreen resizes the window");
    assert!(after > before, "entering fullscreen publishes");
    // The resize the change raised waits behind the publication that ends it,
    // so the callback reads the whole new window.
    assert!(
        seen.iter().all(|revision| *revision == after),
        "a callback saw revisions {seen:?}, not the whole one after {after:?}"
    );
}

fn a_maximum_caps_maximizing() {
    let platform = platform();
    let window = open(&platform, options());
    apply(
        &window,
        WindowCommand::SetConstraints(limits(None, Some(Size::new(500.0, 400.0)))),
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    let content = window.state().content_size.get();
    assert!(
        content.width <= 502.0 && content.height <= 402.0,
        "maximized to {content:?} past the maximum"
    );
}

fn fullscreen_ignores_the_limits() {
    let platform = platform();
    let window = open(
        &platform,
        options().max_size(Some(
            flui_platform::WindowExtent::try_new(Size::new(500.0, 400.0)).expect("a limit"),
        )),
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
    assert!(near(state.outer.size().width, normal.size().width));
    assert!(near(state.outer.size().height, normal.size().height));
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
    apply(&window, WindowCommand::SetFrame(WindowFrame::None));
    apply(&window, WindowCommand::SetFrame(WindowFrame::System));
    // Asking for fullscreen again brings the window back, once.
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    assert!(covers_its_monitor(&window));
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    let state = window.state();
    assert_eq!(state.mode, WindowMode::Normal);
    assert!(state.frame.has_system_title_bar(), "the frame is back");
    assert!(
        near(state.outer.size().width, normal.size().width),
        "{state:?}"
    );
    assert!(
        near(state.outer.size().height, normal.size().height),
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

/// A window maximized, made fullscreen and maximized again comes back in one
/// step: no snapshot between shows it normal, it returns to the normal
/// rectangle it had before fullscreen, and no report of the round trip says
/// the user placed it.
fn a_fullscreen_window_maximized_again_never_reads_normal() {
    let platform = platform();
    let window = open(&platform, options());
    let normal = window.state().outer;
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    settle();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    on_changes(&window, move |_, _, next| {
        log.lock().expect("log").push((next.mode, next.interaction));
    });
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    settle();
    let seen = seen.lock().expect("log").clone();
    assert!(
        seen.iter().all(|(mode, _)| *mode != WindowMode::Normal),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .all(|(_, interaction)| *interaction == WindowInteraction::None),
        "{seen:?}"
    );
    let state = window.state();
    assert_eq!(state.mode, WindowMode::Maximized);
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsZoomed(hwnd_of(&window)) }.as_bool());
    assert_ne!(style(&window) & WS_CAPTION.0, 0, "it has its frame");
    let returns = state.normal_geometry.expect("a maximized window's normal");
    assert!(near(returns.size().width, normal.size().width), "{state:?}");
    assert!(
        near(returns.size().height, normal.size().height),
        "{state:?}"
    );
}

/// A fullscreen window that was minimized leaves fullscreen straight from
/// minimized: a subscriber sees it minimized, then in the mode asked, and
/// never fullscreen or (on its way to maximized) normal between.
fn a_minimized_fullscreen_window_leaves_in_one_step() {
    for (target, between) in [
        (
            WindowMode::Maximized,
            [WindowMode::Fullscreen, WindowMode::Normal],
        ),
        (
            WindowMode::Normal,
            [WindowMode::Fullscreen, WindowMode::Maximized],
        ),
    ] {
        let platform = platform();
        let window = open(&platform, options());
        let normal = window.state().outer;
        apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
        apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
        settle();
        assert_eq!(window.state().mode, WindowMode::Minimized);
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        on_changes(&window, move |_, _, next| {
            log.lock().expect("log").push(next.mode);
        });
        apply(&window, WindowCommand::SetMode(target));
        settle();
        let seen = seen.lock().expect("log").clone();
        assert!(
            seen.iter().all(|mode| !between.contains(mode)),
            "{target:?}: {seen:?}"
        );
        let state = window.state();
        assert_eq!(state.mode, target);
        assert!(shown(&window));
        let returns = state.normal_geometry.unwrap_or(state.outer);
        assert!(near(returns.size().width, normal.size().width), "{state:?}");
        assert!(
            near(returns.size().height, normal.size().height),
            "{state:?}"
        );
    }
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
    apply(&window, WindowCommand::SetFrame(WindowFrame::None));
    assert_eq!(window.state().revision.get(), before + 1);
}

fn a_window_without_a_sizing_border_holds_its_limits() {
    let platform = platform();
    let window = open(
        &platform,
        hidden().frame(WindowFrame::None).max_size(Some(
            flui_platform::WindowExtent::try_new(Size::new(500.0, 400.0)).expect("a limit"),
        )),
    );
    apply(&window, WindowCommand::SetSize(extent(900.0, 700.0)));
    let content = window.state().content_size.get();
    assert!(
        content.width <= 502.0 && content.height <= 402.0,
        "grew to {content:?} past the maximum"
    );
}

/// Send a command whose numbers a maximized Win32 window may hold or not: it
/// applies, or the window system refuses it, and nothing overflows.
fn beyond_win32(window: &Rc<dyn HostWindow>, command: WindowCommand) {
    match window.admit_and_apply(command.clone()) {
        Ok(()) | Err(CommandRefusal::Apply(ApplyError::Platform(PlatformFailure::OutOfRange))) => {}
        Err(other) => panic!("{command:?} answered {other:?}"),
    }
}

fn a_position_beyond_the_screens_does_not_overflow() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    beyond_win32(
        &window,
        WindowCommand::SetPosition(on_its_display(&window, 3.0e9, 3.0e9)),
    );
    beyond_win32(&window, WindowCommand::SetSize(extent(3.0e9, 3.0e9)));
    beyond_win32(
        &window,
        WindowCommand::SetPosition(on_its_display(&window, -3.0e9, -3.0e9)),
    );
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

fn a_hidden_windows_frame_request_reaches_its_frame_callback() {
    let platform = platform();
    let redraws = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&redraws);
    let window = open(&platform, deferred());
    window.handler.react_to_frame(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    window.request_redraw();
    assert!(pump_until(Duration::from_secs(2), || redraws
        .load(Ordering::SeqCst)
        > 0));
}

/// Win32 folds a painted window's invalidations into one paint; a hidden
/// window's frame requests fold the same way until it takes one, so a burst
/// of wakes from another thread is one frame, not a thousand posts.
fn a_burst_of_frame_requests_on_a_hidden_window_is_one_frame() {
    let platform = platform();
    let window = open(&platform, hidden());
    settle();
    let frames = count_frames(&window);
    let requester = window.redraw_requester();
    std::thread::spawn(move || {
        for _ in 0..1000 {
            requester.request_redraw();
        }
    })
    .join()
    .expect("the requesting thread");
    assert!(pump_until(Duration::from_secs(2), || frames
        .load(Ordering::SeqCst)
        > 0));
    settle();
    assert_eq!(frames.load(Ordering::SeqCst), 1);
    // Taken, the next request asks again.
    window.request_redraw();
    assert!(pump_until(Duration::from_secs(2), || frames
        .load(Ordering::SeqCst)
        > 1));
}

/// A window that is not seen, for each fact the host knows for certain
/// (hidden, minimized, cloaked, its session locked), with nothing asked of
/// it, runs no frame over half a second: nothing of the host wakes it on a
/// clock of its own. Red-check: arm a periodic timer on the window that
/// requests a frame, and the count grows.
fn an_unseen_window_with_nothing_asked_runs_no_frame() {
    let platform = platform();
    let hidden_window = open(&platform, hidden());
    let minimized = open(&platform, options());
    apply(&minimized, WindowCommand::SetMode(WindowMode::Minimized));
    let cloaked = open(&platform, options());
    let value = BOOL::from(true);
    // SAFETY: `value` is a live `BOOL`, the size `DWMWA_CLOAK` reads.
    unsafe {
        DwmSetWindowAttribute(
            hwnd_of(&cloaked),
            DWMWA_CLOAK,
            (&raw const value).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
    }
    .expect("the window cloaks");
    let locked = open(&platform, options());
    send(&locked, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 1);
    settle();
    let windows = [&hidden_window, &minimized, &cloaked, &locked];
    for window in windows {
        assert!(!window.state().is_seen(), "not seen: {:?}", window.state());
    }
    let counts: Vec<_> = windows.iter().map(|window| count_frames(window)).collect();
    let _ = pump_until(Duration::from_millis(500), || false);
    let frames: Vec<usize> = counts
        .iter()
        .map(|count| count.load(Ordering::SeqCst))
        .collect();
    assert_eq!(frames, [0, 0, 0, 0], "hidden, minimized, cloaked, locked");
    send(
        &locked,
        WM_WTSSESSION_CHANGE,
        WTS_SESSION_UNLOCK as usize,
        1,
    );
}

/// The window stays on its thread; another thread asks for a frame through
/// its redraw requester, and the window's frame callback runs (ADR-0254 §5).
fn a_redraw_asked_from_another_thread_reaches_the_window() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let redraws = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&redraws);
    window.handler.react_to_frame(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    pump_until(Duration::from_millis(200), || false);
    let before = redraws.load(Ordering::SeqCst);
    let requester = window.redraw_requester();
    std::thread::spawn(move || requester.request_redraw())
        .join()
        .expect("the requesting thread");
    assert!(
        pump_until(Duration::from_secs(2), || redraws.load(Ordering::SeqCst)
            > before),
        "a redraw asked from another thread never reached the frame callback"
    );
}

/// `a_surface_target_is_send_sync_and_unavailable_after_close`, on Win32: the
/// target hands out the window's handle on any thread while the window is
/// open, and `Unavailable` once it closed; it does not keep the window open,
/// and a redraw asked after the close reaches nothing.
fn a_surface_target_is_unavailable_once_its_window_closed() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let hwnd = hwnd_of(&window).0 as isize;
    let target = window.surface_target();
    let requester = window.redraw_requester();
    let (open_target, open_hwnd) = (Arc::clone(&target), hwnd);
    std::thread::spawn(move || {
        let handle = raw_window_handle::HasWindowHandle::window_handle(&*open_target)
            .expect("an open window's target has a handle on any thread");
        let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() else {
            panic!("not a Win32 handle");
        };
        assert_eq!(handle.hwnd.get(), open_hwnd, "the window's own handle");
        assert!(raw_window_handle::HasDisplayHandle::display_handle(&*open_target).is_ok());
    })
    .join()
    .expect("the target's thread");

    let frames = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&frames);
    window.handler.react_to_frame(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    window.close_now();
    pump_until(Duration::from_millis(100), || false);
    // The window is still held: its target answers for the close, not for
    // the last handle's drop.
    assert!(
        matches!(
            raw_window_handle::HasWindowHandle::window_handle(&*target),
            Err(raw_window_handle::HandleError::Unavailable)
        ),
        "a closed window's target hands out no handle"
    );
    assert!(matches!(
        raw_window_handle::HasDisplayHandle::display_handle(&*target),
        Err(raw_window_handle::HandleError::Unavailable)
    ));
    let before = frames.load(Ordering::SeqCst);
    std::thread::spawn(move || requester.request_redraw())
        .join()
        .expect("the requesting thread");
    window.request_redraw();
    pump_until(Duration::from_millis(200), || false);
    assert_eq!(
        frames.load(Ordering::SeqCst),
        before,
        "a redraw asked of a closed window runs no frame"
    );
    // Neither the target nor the requester keeps the window alive.
    let gone = Rc::downgrade(&window.window);
    drop(window);
    assert!(
        gone.upgrade().is_none(),
        "the target and the requester keep no handle on the window"
    );
}

/// Run `changed` with the snapshot before and after each `state_changed` of
/// `window`'s handler from here on, as a consumer compares them.
fn on_changes(
    window: &Opened,
    changed: impl FnMut(&Rc<dyn HostWindow>, &WindowState, &WindowState) + 'static,
) {
    let weak = Rc::downgrade(&window.window);
    let seen = std::cell::RefCell::new(window.state());
    let changed = std::cell::RefCell::new(changed);
    window.handler.react_to_state_change(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let next = window.state();
        let previous = seen.replace(next.clone());
        (changed.borrow_mut())(&window, &previous, &next);
    });
}

/// Whether `window`'s content can be seen, each time the snapshot changes it.
fn record_visibility(window: &Opened) -> Arc<std::sync::Mutex<Vec<bool>>> {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    on_changes(window, move |_, previous, next| {
        if previous.is_seen() != next.is_seen() {
            log.lock().expect("log").push(next.is_seen());
        }
    });
    events
}

fn a_show_asked_for_while_a_hidden_window_is_restored_is_kept() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Minimized));
    apply(&window, WindowCommand::Hide);
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    // The app answers the restore's resize by showing the window.
    on_changes(&window, move |window, previous, next| {
        if previous.is_seen() != next.is_seen() {
            log.lock().expect("log").push(next.is_seen());
        }
        if previous.metrics != next.metrics || previous.mode != next.mode {
            window
                .admit_and_apply(WindowCommand::Show)
                .expect("the restored window shows");
        }
    });
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert!(
        shown(&window),
        "the newer show request is not undone by the hide"
    );
    assert_eq!(*events.lock().expect("log"), [true]);
}

fn a_window_first_shown_minimized_is_never_reported_visible() {
    let platform = platform();
    let window = open(&platform, deferred().mode(WindowMode::Minimized));
    let events = record_visibility(&window);
    window.reveal_after_first_frame();
    // SAFETY: a plain query of the live window.
    assert!(unsafe { IsIconic(hwnd_of(&window)) }.as_bool());
    assert_eq!(
        *events.lock().expect("log"),
        [false],
        "told visible while it waited for a frame, it is then told it is not"
    );

    let window = open(&platform, hidden().mode(WindowMode::Minimized));
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
    window.handler.react_to_state_change(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
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
fn count_frames(window: &Opened) -> Arc<AtomicUsize> {
    let frames = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&frames);
    window.handler.react_to_frame(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    frames
}

/// Run what the thread has queued for a while.
fn settle() {
    let _ = pump_until(Duration::from_millis(150), || false);
}

fn send(window: &Rc<dyn HostWindow>, message: u32, wparam: usize, lparam: isize) {
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
fn dpi_changed(window: &Rc<dyn HostWindow>, dpi: usize, suggested: RECT) {
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
    on_changes(&window, move |_, previous, next| {
        let scale = |state: &WindowState| state.metrics.map(|metrics| metrics.scale().get());
        if scale(previous) != scale(next)
            && let Some(scale) = scale(next)
        {
            log.lock().expect("log").push(scale);
        }
    });
    // The window keeps the rectangle it has, so no `WM_SIZE` follows.
    dpi_changed(&window, 144, rect(&window));
    assert_eq!(*resizes.lock().expect("log"), [1.5]);
    let scale = window.state().metrics.expect("drawable").scale().get();
    assert!(near(scale, 1.5));
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
    assert_eq!(
        (
            restored.right - restored.left,
            restored.bottom - restored.top
        ),
        outer_at(&window, 144, (960, 720)),
        "returns with its 640×480 content at 1.5 and the frame of 144 DPI around it, \
         not the {}-wide rectangle it left scaled linearly",
        normal.right - normal.left
    );
}

/// The outer size `AdjustWindowRectExForDpi` gives `window`'s style around a
/// content of `content` device pixels at `dpi`; a window whose application
/// draws its title bar has nothing above its content.
fn outer_at(window: &Rc<dyn HostWindow>, dpi: u32, content: (i32, i32)) -> (i32, i32) {
    use windows::Win32::UI::{
        HiDpi::AdjustWindowRectExForDpi,
        WindowsAndMessaging::{GWL_EXSTYLE, WINDOW_EX_STYLE, WINDOW_STYLE},
    };

    let mut outer = RECT {
        left: 0,
        top: 0,
        right: content.0,
        bottom: content.1,
    };
    // SAFETY: plain queries of the live window and an in/out stack local.
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd_of(window), GWL_EXSTYLE) as u32;
        AdjustWindowRectExForDpi(
            &raw mut outer,
            WINDOW_STYLE(style(window)),
            false,
            WINDOW_EX_STYLE(ex_style),
            dpi,
        )
        .expect("the frame at the DPI");
    }
    if !window.state().frame.has_system_title_bar() {
        outer.top = 0;
    }
    (outer.right - outer.left, outer.bottom - outer.top)
}

/// What `window` answers `WM_GETDPISCALEDSIZE` for `dpi` with its pending
/// outer size `pending`: `None` when it leaves the system its linear scaling.
fn dpi_scaled_size(
    window: &Rc<dyn HostWindow>,
    dpi: u32,
    pending: (i32, i32),
) -> Option<(i32, i32)> {
    use windows::Win32::{Foundation::SIZE, UI::WindowsAndMessaging::WM_GETDPISCALEDSIZE};

    let mut size = SIZE {
        cx: pending.0,
        cy: pending.1,
    };
    // SAFETY: a message to the live window on its own thread; `size` outlives
    // the synchronous send, as the system's `SIZE` outlives its own.
    let answered = unsafe {
        SendMessageW(
            hwnd_of(window),
            WM_GETDPISCALEDSIZE,
            Some(WPARAM(dpi as usize)),
            Some(LPARAM((&raw mut size) as isize)),
        )
    };
    (answered.0 != 0).then_some((size.cx, size.cy))
}

/// Before a `WM_DPICHANGED`, the system asks the window what size it takes at
/// the new DPI. A normal window answers the one whose content keeps its
/// logical size under the frame of that DPI, for the system's frame and an
/// application's title bar alike, where the system would scale the whole
/// rectangle linearly; across a change and back it returns to the size it
/// left. A maximized window answers none.
fn a_scale_change_keeps_the_contents_logical_size() {
    use flui_platform::{AppTitleBar, SystemControls};

    let platform = platform();
    for frame in [
        WindowFrame::System,
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden)),
    ] {
        let window = open(&platform, hidden().frame(frame));
        // SAFETY: a plain query of the live window.
        let at = unsafe { GetDpiForWindow(hwnd_of(&window)) };
        let before = rect(&window);
        let outer = (before.right - before.left, before.bottom - before.top);
        for (dpi, content) in [
            (120, (800, 600)),
            (144, (960, 720)),
            (168, (1120, 840)),
            (192, (1280, 960)),
        ] {
            assert_eq!(
                dpi_scaled_size(&window, dpi, outer),
                Some(outer_at(&window, dpi, content)),
                "{frame:?} from {at} to {dpi} DPI"
            );
        }

        let there = dpi_scaled_size(&window, 144, outer).expect("a size at 144 DPI");
        dpi_changed(
            &window,
            144,
            RECT {
                right: before.left + there.0,
                bottom: before.top + there.1,
                ..before
            },
        );
        let moved = rect(&window);
        assert_eq!(
            dpi_scaled_size(
                &window,
                at as u32,
                (moved.right - moved.left, moved.bottom - moved.top)
            ),
            Some(outer),
            "{frame:?}: back at {at} DPI, the size it left"
        );

        apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
        assert_eq!(
            dpi_scaled_size(&window, 192, outer),
            None,
            "{frame:?}: a maximized window covers its monitor at any DPI"
        );
    }
}

/// Where the mouse is not a pointer, the legacy non-client mouse messages
/// over a caption button the application draws reach its button as the
/// client mouse messages they stand for, at the button's position, and the
/// default procedure does not track a classic button over it; the leave of the
/// non-client area ends the hover. Over the caption button the system draws,
/// they stay the system's.
fn caption_buttons_take_the_legacy_mouse_messages_over_them() {
    use flui_foundation::geometry::Rect;
    use flui_platform::input_regions::{CaptionButton, InputRegion, InputRegionKind};
    use flui_platform::pointer::PointerEvent;
    use flui_platform::{AppTitleBar, PlatformInput, SystemControls};
    use windows::Win32::UI::WindowsAndMessaging::{
        HTMAXBUTTON, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    };

    // The screen point of the logical point `(x, y)` of `window`'s client, as
    // a non-client mouse message packs it.
    let screen_point = |window: &Rc<dyn HostWindow>, x: f64, y: f64| {
        let scale = window.state().metrics.expect("drawable").scale().get();
        let origin = client_origin_on_screen(window);
        let x = origin.0 + (x * scale).round() as i32;
        let y = origin.1 + (y * scale).round() as i32;
        ((y as u32 & 0xffff) << 16 | (x as u32 & 0xffff)) as i32 as isize
    };
    let pointer_events = |window: &Opened| -> Vec<PointerEvent> {
        window
            .handler
            .inputs()
            .iter()
            .filter_map(PlatformInput::as_pointer)
            .cloned()
            .collect()
    };

    let platform = platform();
    let frame =
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden));
    let window = open(&platform, hidden().frame(frame));
    window
        .set_input_regions(&[InputRegion::new(
            InputRegionKind::CaptionButton(CaptionButton::Maximize),
            Rect::from_ltrb(560.0, 0.0, 600.0, 32.0),
        )])
        .expect("a Win32 window honours the chrome regions");
    window.handler.take_calls();
    let over_maximize = screen_point(&window, 580.0, 16.0);

    send(&window, WM_NCMOUSEMOVE, HTMAXBUTTON as usize, over_maximize);
    assert!(window.state().hovered, "a move over the button hovers");
    send(
        &window,
        WM_NCLBUTTONDOWN,
        HTMAXBUTTON as usize,
        over_maximize,
    );
    send(&window, WM_NCLBUTTONUP, HTMAXBUTTON as usize, over_maximize);
    let events = pointer_events(&window);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, PointerEvent::Move(_))),
        "the move reached the application: {events:?}"
    );
    let presses: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            PointerEvent::Down(press) => Some(("down", press.sample.position.get())),
            PointerEvent::Up(release) => Some(("up", release.sample.position.get())),
            _ => None,
        })
        .collect();
    assert_eq!(
        presses.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        ["down", "up"],
        "the press and the release reached the application's button: {events:?}"
    );
    for (kind, at) in &presses {
        assert!(
            near(at.x, 580.0) && near(at.y, 16.0),
            "the {kind} lands on the button, in client coordinates: {at:?}"
        );
    }
    // A hidden window is under no cursor: the mouse left it.
    send(&window, WM_NCMOUSELEAVE, 0, 0);
    assert!(!window.state().hovered, "leaving the button ends the hover");

    let system = open(&platform, hidden());
    system.handler.take_calls();
    let over_its_button = screen_point(&system, 600.0, -10.0);
    send(
        &system,
        WM_NCMOUSEMOVE,
        HTMAXBUTTON as usize,
        over_its_button,
    );
    assert_eq!(
        pointer_events(&system),
        [],
        "the system's own caption button keeps its messages"
    );
    assert!(!system.state().hovered);
}

/// The hover an application's caption button saw ends when the pointer moves
/// on over the bar or a border, which the application hears nothing of: one
/// leave, at the button, for the first such move, and none for the next.
fn a_move_off_a_caption_button_ends_the_hover_it_saw() {
    use flui_foundation::geometry::Rect;
    use flui_platform::input_regions::{CaptionButton, InputRegion, InputRegionKind};
    use flui_platform::pointer::PointerEvent;
    use flui_platform::{AppTitleBar, PlatformInput, SystemControls};
    use windows::Win32::UI::WindowsAndMessaging::{
        HTCAPTION, HTMINBUTTON, HTRIGHT, WM_NCMOUSEMOVE,
    };

    let platform = platform();
    let frame =
        WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(SystemControls::Hidden));
    let window = open(&platform, hidden().frame(frame));
    window
        .set_input_regions(&[InputRegion::new(
            InputRegionKind::CaptionButton(CaptionButton::Minimize),
            Rect::from_ltrb(520.0, 0.0, 560.0, 32.0),
        )])
        .expect("a Win32 window honours the chrome regions");
    let scale = window.state().metrics.expect("drawable").scale().get();
    let origin = client_origin_on_screen(&window);
    let screen = |x: f64, y: f64| {
        let x = origin.0 + (x * scale).round() as i32;
        let y = origin.1 + (y * scale).round() as i32;
        ((y as u32 & 0xffff) << 16 | (x as u32 & 0xffff)) as i32 as isize
    };
    let leaves = |window: &Opened| -> Vec<(f64, f64)> {
        window
            .handler
            .inputs()
            .iter()
            .filter_map(PlatformInput::as_pointer)
            .filter_map(|event| match event {
                PointerEvent::Leave(signal) => {
                    let at = signal.position.expect("where it left").get();
                    Some((at.x, at.y))
                }
                _ => None,
            })
            .collect()
    };
    window.handler.take_calls();
    send(
        &window,
        WM_NCMOUSEMOVE,
        HTMINBUTTON as usize,
        screen(540.0, 16.0),
    );
    assert_eq!(leaves(&window), [], "over the button, no leave");
    send(
        &window,
        WM_NCMOUSEMOVE,
        HTCAPTION as usize,
        screen(300.0, 16.0),
    );
    let left = leaves(&window);
    assert_eq!(
        left.len(),
        1,
        "one leave for the move over the bar: {left:?}"
    );
    assert!(
        near(left[0].0, 540.0) && near(left[0].1, 16.0),
        "it leaves where the button last saw it: {left:?}"
    );
    send(
        &window,
        WM_NCMOUSEMOVE,
        HTRIGHT as usize,
        screen(641.0, 100.0),
    );
    assert_eq!(
        leaves(&window).len(),
        1,
        "the next move has no hover to end"
    );
}

fn geometry_win32_cannot_hold_is_refused() {
    let platform = platform();
    let window = open(&platform, options());
    let before = window.state();
    for command in [
        WindowCommand::SetPosition(on_its_display(&window, 1.0e12, 0.0)),
        WindowCommand::SetPosition(on_its_display(&window, 0.0, -1.0e12)),
    ] {
        // Finite and positive, the numbers pass admission; the device range
        // is Win32's own limit, so the window system refuses them.
        assert!(
            matches!(
                window.admit_and_apply(command.clone()),
                Err(CommandRefusal::Apply(ApplyError::Platform(
                    PlatformFailure::OutOfRange
                )))
            ),
            "{command:?}"
        );
    }
    let after = window.state();
    assert_eq!(after.content_size, before.content_size);
    assert_eq!(after.outer, before.outer);
}

/// A size past the largest surface a renderer draws is admitted and applied,
/// where it used to be refused as past Win32's range: the window takes a size no
/// larger than the largest surface. The clamp itself is proven by the pure
/// conversion's table and the headless host's snapshot
/// (`a_size_past_the_largest_surface_is_held_at_it`); Win32 holds a window
/// within its own track size as well, so this run cannot tell the two apart.
fn a_size_past_the_largest_surface_is_admitted() {
    let platform = platform();
    let window = open(&platform, options());
    for (command, wide) in [
        (WindowCommand::SetSize(extent(1.0e10, 100.0)), true),
        (WindowCommand::SetSize(extent(100.0, 1.0e10)), false),
    ] {
        window
            .admit_and_apply(command.clone())
            .unwrap_or_else(|refusal| panic!("{command:?} refused: {refusal:?}"));
        settle();
        let state = window.state();
        let physical = state.metrics.expect("a drawable window").physical();
        let (long, short, device) = if wide {
            (
                state.content_size.width(),
                state.content_size.height(),
                physical.width,
            )
        } else {
            (
                state.content_size.height(),
                state.content_size.width(),
                physical.height,
            )
        };
        // Win32 holds a window within its own track size as well, so the
        // side is at most the largest surface, and the other side at most
        // what Win32 lets a window be.
        assert!(
            u32::try_from(device).is_ok_and(|device| device <= MAX_SURFACE_EXTENT),
            "{command:?}: {long} logical pixels, {device} device pixels"
        );
        assert!(long > short, "{command:?}: {long} grew past {short}");
    }
}

fn press_f11(window: &Rc<dyn HostWindow>) {
    // F11's scan code is 0x57.
    send(window, WM_KEYDOWN, usize::from(VK_F11.0), 0x0057_0001);
}

/// The host has no key of its own: an F11 the application answers without
/// preventing its default leaves the window as it was. Fullscreen is the
/// application's `SetMode`.
fn a_key_the_application_lets_through_leaves_the_mode_alone() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let before = window.state();
    window
        .handler
        .react_to_input(|_| DispatchEventResult::resolved(true, false));
    press_f11(&window);
    let changed = pump_until(Duration::from_millis(500), || {
        window.state().mode != WindowMode::Normal
    });
    assert!(!changed, "{:?}", window.state().mode);
    assert_eq!(window.state().outer, before.outer);
}

fn a_resize_outside_a_live_resize_asks_for_one_frame_after_it() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let frames = count_frames(&window);
    apply(&window, WindowCommand::SetSize(extent(600.0, 420.0)));
    apply(&window, WindowCommand::SetSize(extent(620.0, 440.0)));
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
    // The application's handler asks for a deadline due at every instant;
    // the move loop, run from the bootstrap, stands in for the platform's.
    let during = Arc::new(AtomicUsize::new(0));
    let measured = Arc::clone(&during);
    Box::new(platform())
        .run(crate::app::with_log(move |log, owner| {
            log.answer_deadline_with(|| Some(Instant::now()));
            let window = Opened::ready(&owner, options());
            settle();
            let frames = count_frames(&window);
            let address = hwnd_of(&window).0 as isize;
            let moved = Arc::new(AtomicBool::new(false));
            let done = Arc::clone(&moved);
            let ender = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                while !done.load(Ordering::SeqCst) {
                    // SAFETY: a post to a window of this process; one that is
                    // gone fails the call.
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
            measured.store(frames.load(Ordering::SeqCst), Ordering::SeqCst);
            window.close_for_cleanup();
            owner.proxy().request_quit().expect("the loop takes a quit");
            Ok(())
        }))
        .expect("the Win32 loop");
    let during = during.load(Ordering::SeqCst);
    assert!(
        during >= 5,
        "{during} frames in the 300 ms the move loop ran"
    );
}

/// Whether the thread `thread` runs Win32's modal size-move loop.
fn in_move_size(thread: u32) -> bool {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a live local with its size set, which the call fills.
    unsafe { GetGUIThreadInfo(thread, &raw mut info) }.is_ok()
        && (info.flags & GUI_INMOVESIZE) == GUI_INMOVESIZE
}

/// The span a keyboard move loop of the window behind `address` stood idle,
/// on the thread `thread`, as [`hold_an_idle_move_loop`] measured it.
#[derive(Clone, Copy, Debug)]
struct IdleSpan {
    from: Instant,
    to: Instant,
    /// The loop ran at both ends of the span.
    in_loop: bool,
}

/// From another thread: once the thread `thread` runs the move loop of the
/// window behind `address`, let it rest for `idle` (calling `midway` halfway),
/// end it with Escape and ask the run to quit. The span is written to `span`.
fn hold_an_idle_move_loop(
    address: isize,
    thread: u32,
    idle: Duration,
    midway: impl FnOnce() + Send + 'static,
    quit: flui_native::PlatformProxy,
    span: Arc<std::sync::Mutex<Option<IdleSpan>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let entered = Instant::now();
        while !in_move_size(thread) && entered.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        // The loop's first delivery, and what the move itself ran, settle.
        std::thread::sleep(Duration::from_millis(100));
        let from = Instant::now();
        let started = in_move_size(thread);
        std::thread::sleep(idle / 2);
        midway();
        std::thread::sleep(idle / 2);
        let to = Instant::now();
        let in_loop = started && in_move_size(thread);
        *span.lock().expect("span") = Some(IdleSpan { from, to, in_loop });
        let ending = Instant::now();
        while in_move_size(thread) && ending.elapsed() < Duration::from_secs(3) {
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
        let _ = quit.request_quit();
    })
}

/// Start the keyboard move loop of `window` from the main loop, outside any
/// callback of the bootstrap: the post is dispatched once the bootstrap
/// returned.
fn post_move_loop(window: &Rc<dyn HostWindow>) {
    // SAFETY: a post to the live window of this thread.
    unsafe {
        PostMessageW(
            Some(hwnd_of(window)),
            WM_SYSCOMMAND,
            WPARAM(SC_MOVE as usize),
            LPARAM(0),
        )
    }
    .expect("the move is posted");
}

/// A move loop left idle, with no deadline asked for, asks the application's
/// handler for none: it sets no timer until the platform's code runs again.
/// Red-check: answer "no deadline" with a timer at the frame pace, and the
/// handler is asked every 16 ms of the half second.
fn an_idle_modal_loop_with_no_deadline_asks_nothing() {
    let asks = Rc::new(std::cell::RefCell::new(Vec::new()));
    let span = Arc::new(std::sync::Mutex::new(None));
    let kept = Rc::new(std::cell::RefCell::new(None));
    let (heard, measured, holder) = (Rc::clone(&asks), Arc::clone(&span), Rc::clone(&kept));
    Box::new(platform())
        .run(crate::app::with_log(move |log, owner| {
            log.react(move |call, _| {
                if call == crate::app::AppCall::WakeDeadline {
                    heard.borrow_mut().push(Instant::now());
                }
            });
            let window = Opened::ready(&owner, options());
            settle();
            let hwnd = hwnd_of(&window);
            // SAFETY: a plain query of the live window.
            let thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
            let mover = hold_an_idle_move_loop(
                hwnd.0 as isize,
                thread,
                Duration::from_millis(500),
                || {},
                owner.proxy(),
                measured,
            );
            post_move_loop(&window);
            *holder.borrow_mut() = Some((window, mover));
            Ok(())
        }))
        .expect("the Win32 loop");
    if let Some((window, mover)) = kept.borrow_mut().take() {
        mover.join().expect("the moving thread");
        window.close_for_cleanup();
    }
    let span = span
        .lock()
        .expect("span")
        .expect("the move loop was held idle");
    assert!(span.in_loop, "the move loop ran through the span: {span:?}");
    let idle_asks = asks
        .borrow()
        .iter()
        .filter(|at| **at > span.from && **at < span.to)
        .count();
    assert!(
        idle_asks <= 2,
        "{idle_asks} questions in the {:?} the loop stood idle",
        span.to - span.from
    );
}

/// A deadline an owner turn sets while a move loop stands idle is delivered
/// inside the loop: the turn ran the platform's code, after which the loop
/// asks the deadlines again. Red-check: leave the owner window's procedure
/// without telling the modal loops, and the frame comes only after the loop.
fn a_deadline_a_turn_sets_during_an_idle_modal_loop_is_delivered_in_it() {
    let span = Arc::new(std::sync::Mutex::new(None));
    let deadline = Rc::new(Cell::new(None::<Instant>));
    let frames = Rc::new(std::cell::RefCell::new(Vec::new()));
    let kept = Rc::new(std::cell::RefCell::new(None));
    let (measured, holder) = (Arc::clone(&span), Rc::clone(&kept));
    let (answered, armed, drawn) = (
        Rc::clone(&deadline),
        Rc::clone(&deadline),
        Rc::clone(&frames),
    );
    let cleared = Rc::clone(&deadline);
    Box::new(platform())
        .run(crate::app::with_log(move |log, owner| {
            log.answer_deadline_with(move || answered.get());
            let window = Opened::ready(&owner, options());
            settle();
            window.handler.react_to_frame(move |_| {
                if cleared.take().is_some() {
                    drawn.borrow_mut().push(Instant::now());
                }
            });
            owner
                .on_wake(Box::new(move || {
                    armed.set(Some(Instant::now() + Duration::from_millis(60)));
                }))
                .expect("the owner takes a wake callback");
            let hwnd = hwnd_of(&window);
            // SAFETY: a plain query of the live window.
            let thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
            let waker = owner.proxy();
            let mover = hold_an_idle_move_loop(
                hwnd.0 as isize,
                thread,
                Duration::from_millis(600),
                move || {
                    let _ = waker.wake();
                },
                owner.proxy(),
                measured,
            );
            post_move_loop(&window);
            *holder.borrow_mut() = Some((window, mover));
            Ok(())
        }))
        .expect("the Win32 loop");
    if let Some((window, mover)) = kept.borrow_mut().take() {
        mover.join().expect("the moving thread");
        window.close_for_cleanup();
    }
    let span = span
        .lock()
        .expect("span")
        .expect("the move loop was held idle");
    assert!(span.in_loop, "the move loop ran through the span: {span:?}");
    let frames = frames.borrow();
    assert!(
        frames.iter().any(|at| *at > span.from && *at < span.to),
        "the deadline's frame came inside the loop's idle span {span:?}: {frames:?}"
    );
}

/// A redraw another thread asks of a window while its move loop stands idle,
/// with no deadline asked for, is painted inside the loop: the system's loop
/// dispatches the paint itself, and the paint asks the pacing again.
fn a_redraw_from_another_thread_is_painted_in_an_idle_modal_loop() {
    let span = Arc::new(std::sync::Mutex::new(None));
    let frames = Rc::new(std::cell::RefCell::new(Vec::new()));
    let kept = Rc::new(std::cell::RefCell::new(None));
    let (measured, holder, drawn) = (Arc::clone(&span), Rc::clone(&kept), Rc::clone(&frames));
    Box::new(platform())
        .run(crate::app::with_log(move |_, owner| {
            let window = Opened::ready(&owner, options());
            settle();
            window.handler.react_to_frame(move |_| {
                drawn.borrow_mut().push(Instant::now());
            });
            let redraw = window.redraw_requester();
            let hwnd = hwnd_of(&window);
            // SAFETY: a plain query of the live window.
            let thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
            let mover = hold_an_idle_move_loop(
                hwnd.0 as isize,
                thread,
                Duration::from_millis(600),
                move || redraw.request_redraw(),
                owner.proxy(),
                measured,
            );
            post_move_loop(&window);
            *holder.borrow_mut() = Some((window, mover));
            Ok(())
        }))
        .expect("the Win32 loop");
    if let Some((window, mover)) = kept.borrow_mut().take() {
        mover.join().expect("the moving thread");
        window.close_for_cleanup();
    }
    let span = span
        .lock()
        .expect("span")
        .expect("the move loop was held idle");
    assert!(span.in_loop, "the move loop ran through the span: {span:?}");
    // Nothing else asks this window for a frame while the loop stands idle.
    let frames = frames.borrow();
    assert!(
        frames.iter().any(|at| *at > span.from && *at < span.to),
        "the redraw asked midway through {span:?} was painted inside it: {frames:?}"
    );
}

/// A screen reader's first question (`WM_GETOBJECT` for the UIA root) runs the
/// activation listener inside AccessKit's own handling of it. A listener that
/// publishes a whole tree there answers the question with it, and the window
/// reports an assistive technology attached. Red-check: hand the publish to
/// the adapter during the activation, which re-borrows the state AccessKit
/// holds across it: the panic is contained, and the window reports itself
/// inactive with AccessKit active.
fn a_tree_published_inside_the_activation_answers_it() {
    use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
    use windows::Win32::UI::Accessibility::UiaRootObjectId;

    let platform = platform();
    let window = open(&platform, hidden());
    let accessibility = window
        .window
        .accessibility()
        .expect("a Win32 window answers UI Automation");
    let publisher = Arc::downgrade(&accessibility);
    let published = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&published);
    accessibility.set_activation_listener(Arc::new(move |active| {
        let Some(publisher) = publisher.upgrade().filter(|_| active) else {
            return;
        };
        counted.fetch_add(1, Ordering::SeqCst);
        let root = NodeId(1);
        let mut node = Node::new(Role::Window);
        node.set_label("inside the activation".to_string());
        publisher.publish(TreeUpdate {
            nodes: vec![(root, node)],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        });
    }));
    // SAFETY: a message to the live window on its own thread, with no pointer.
    let answer = unsafe {
        SendMessageW(
            hwnd_of(&window),
            WM_GETOBJECT,
            Some(WPARAM(0)),
            Some(LPARAM(UiaRootObjectId as isize)),
        )
    };
    assert_eq!(
        published.load(Ordering::SeqCst),
        1,
        "the listener published"
    );
    assert!(
        accessibility.is_active(),
        "the window reports the screen reader that asked"
    );
    assert_ne!(answer.0, 0, "the question got the tree's root");
    window.close_for_cleanup();
}

// --- the snapshot against what Win32 says ----------------------------------

/// What Win32 says of the window, read with its own queries.
fn win32_readback(hwnd: HWND) -> Readback {
    let mut client = RECT::default();
    let mut cloaked = 0_u32;
    // SAFETY: plain queries of the live window on its creating thread; the
    // out-parameters are stack locals of the sizes the calls write.
    let (have_client, dpi, focus, active, shown, cloak_read) = unsafe {
        (
            GetClientRect(hwnd, &raw mut client).is_ok(),
            GetDpiForWindow(hwnd),
            GetFocus(),
            GetActiveWindow(),
            IsWindowVisible(hwnd).as_bool(),
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast(),
                std::mem::size_of::<u32>() as u32,
            )
            .is_ok(),
        )
    };
    Readback {
        metrics: have_client
            .then(|| {
                WindowMetrics::try_new(
                    Size::new(client.right - client.left, client.bottom - client.top),
                    f64::from(dpi) / 96.0,
                )
                .ok()
            })
            .flatten(),
        focused: focus == hwnd,
        active: active == hwnd,
        visible: shown,
        occluded: cloak_read.then_some(cloaked != 0),
        material: system_backdrop(hwnd),
    }
}

/// The contract's material the desktop window manager draws behind `hwnd`:
/// `DWMSBT_NONE` is `Opaque` and `DWMSBT_MAINWINDOW` (Mica) is `System`;
/// another backdrop, or a build without the attribute, has no name here.
fn system_backdrop(hwnd: HWND) -> Option<flui_platform::WindowMaterial> {
    use windows::Win32::Graphics::Dwm::{
        DWM_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
    };
    let mut backdrop = DWM_SYSTEMBACKDROP_TYPE::default();
    // SAFETY: a query of the live window; the out-parameter is a stack local
    // of the 4-byte enum the attribute holds, read only when the call worked.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw mut backdrop).cast(),
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )
    };
    match (read.is_ok(), backdrop) {
        (true, DWMSBT_NONE) => Some(flui_platform::WindowMaterial::Opaque),
        (true, DWMSBT_MAINWINDOW) => Some(flui_platform::WindowMaterial::System),
        _ => None,
    }
}

/// What one `state_changed` read: the snapshot's metrics with what Win32
/// read back at that moment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Heard {
    published: Option<WindowMetrics>,
    reported: Option<WindowMetrics>,
}

/// Record what each of `window`'s `state_changed` reads from here on.
fn listen_for_snapshots(window: &Opened) -> Arc<std::sync::Mutex<Vec<Heard>>> {
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&heard);
    let weak = Rc::downgrade(window);
    let address = hwnd_of(window).0 as isize;
    window.handler.react_to_state_change(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let hwnd = HWND(address as *mut std::ffi::c_void);
        log.lock().expect("log").push(Heard {
            published: window.state().metrics,
            reported: win32_readback(hwnd).metrics,
        });
    });
    heard
}

/// `heard` saw the metrics move from `before`, and every snapshot is what
/// Win32 reported then.
fn the_snapshots_carry_each_resize(heard: &[Heard], before: WindowMetrics) {
    assert!(
        heard.iter().any(|heard| heard.published != Some(before)),
        "no resize was heard: {heard:?}"
    );
    for heard in heard {
        assert_eq!(
            heard.published, heard.reported,
            "a snapshot is what Win32 reported"
        );
    }
}

/// A DPI change holds publishing while the window takes its new rectangle:
/// what `WindowHandler::state_changed` reads is the whole new window, never the new
/// rectangle at the old scale.
fn a_dpi_change_announces_one_whole_snapshot() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let heard = listen_for_snapshots(&window);
    let now = rect(&window);
    let larger = RECT {
        right: now.left + (now.right - now.left) * 3 / 2,
        bottom: now.top + (now.bottom - now.top) * 3 / 2,
        ..now
    };
    dpi_changed(&window, 144, larger);
    let seen = heard.lock().expect("log").clone();
    assert!(!seen.is_empty(), "the DPI change published");
    assert!(
        seen.iter()
            .all(|heard| heard.published.map(|metrics| metrics.scale().get()) == Some(1.5)),
        "a snapshot was announced half made: {seen:?}"
    );
    dpi_changed(&window, 96, now);
}

/// Counts the warnings a window sends its `tracing` subscriber.
struct Warnings(Arc<AtomicUsize>);

impl tracing::Subscriber for Warnings {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A `WM_DPICHANGED` naming a DPI of zero, which any process can send in a
/// loop, is reported once per run of them, not once per message: each report
/// is a call into the application's subscriber. A real DPI ends the run.
fn a_run_of_zero_dpi_messages_is_reported_once() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(Arc::clone(&warnings)), || {
        for _ in 0..5 {
            send(&window, WM_DPICHANGED, 0, 0);
        }
    });
    assert_eq!(warnings.load(Ordering::SeqCst), 1, "one run, one warning");
    dpi_changed(&window, 96, rect(&window));
    let after = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(Arc::clone(&after)), || {
        send(&window, WM_DPICHANGED, 0, 0);
    });
    assert_eq!(after.load(Ordering::SeqCst), 1, "a new run is reported");
}

/// A `WM_DPICHANGED` whose DPI is no ratio a screen can have (1 and 4000,
/// either side of `DevicePixelRatio`'s range), which any process can send, is
/// refused by the hosts' one scale rule: the window keeps its scale, reports
/// the run once, and takes the next DPI that is one.
fn a_dpi_no_screen_has_is_refused_and_the_window_keeps_its_scale() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let scale = |window: &Rc<dyn HostWindow>| {
        window
            .state()
            .metrics
            .expect("a drawable window")
            .scale()
            .get()
    };
    let before = scale(&window);
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(Arc::clone(&warnings)), || {
        for dpi in [1, 4000, 1, 4000] {
            dpi_changed(&window, dpi, rect(&window));
        }
    });
    assert_eq!(warnings.load(Ordering::SeqCst), 1, "one run, one warning");
    assert_eq!(scale(&window), before, "the window keeps its scale");
    window.request_redraw();
    settle();
    dpi_changed(&window, 144, rect(&window));
    settle();
    assert_eq!(scale(&window), 1.5, "and takes the next DPI that is one");
}

/// Activation is a fact of its own: a window whose keyboard focus went
/// elsewhere (an owned popup, here none) is still the active one.
fn activation_is_a_fact_apart_from_focus() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    send(&window, WM_ACTIVATE, WA_ACTIVE as usize, 0);
    assert!(
        window.state().focus.is_active(),
        "WM_ACTIVATE made it active"
    );
    // SAFETY: a plain call on this thread; no window keeps the focus.
    let _ = unsafe { SetFocus(None) };
    let state = window.state();
    assert!(
        state.focus == flui_platform::WindowFocus::Active,
        "active, not focused: {state:?}"
    );
    send(&window, WM_ACTIVATE, WA_INACTIVE as usize, 0);
    assert!(
        !window.state().focus.is_active(),
        "WM_ACTIVATE took it back"
    );
}

/// A Win32 window for the conformance checks of `flui-testing`.
struct Win32Fixture {
    platform: WindowsPlatform,
    window: Rc<dyn HostWindow>,
    hwnd: HWND,
    /// The window that took the focus away, kept open until the fixture goes.
    focus_holder: Option<Opened>,
}

/// A flip-model DXGI swap chain on a window, presented the way wgpu's DX12
/// backend presents: its frame statistics read before each present and handed
/// to the window's surface target.
struct DxgiPresenter {
    swap_chain: windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    _device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
}

impl DxgiPresenter {
    /// A two-buffer swap chain on `hwnd`, on a hardware device or, where
    /// there is none, WARP.
    fn on(hwnd: HWND) -> Self {
        use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
        };
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
        };
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_DISCARD,
            DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2,
        };

        let device = [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP]
            .into_iter()
            .find_map(|driver| {
                let mut device = None;
                // SAFETY: the out-pointer is a local that outlives the call;
                // no adapter, module or feature-level list is passed.
                unsafe {
                    D3D11CreateDevice(
                        None,
                        driver,
                        windows::Win32::Foundation::HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&raw mut device),
                        None,
                        None,
                    )
                }
                .ok()?;
                device
            })
            .expect("a Direct3D 11 device");
        // SAFETY: a plain factory creation.
        let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory1() }.expect("a DXGI factory");
        let description = DXGI_SWAP_CHAIN_DESC1 {
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            ..Default::default()
        };
        // SAFETY: `hwnd` is the live test window, on its creating thread; the
        // description outlives the call.
        let swap_chain = unsafe {
            factory.CreateSwapChainForHwnd(&device, hwnd, &raw const description, None, None)
        }
        .expect("a flip-model swap chain on the window");
        Self {
            swap_chain,
            _device: device,
        }
    }

    /// The counters as the renderer reads them from wgpu's DX12 swap chain
    /// before a present: the presents made so far, and the statistics, which
    /// name nothing until the display has shown a present.
    fn statistics(&self) -> flui_platform::SwapChainStatistics {
        let mut statistics = windows::Win32::Graphics::Dxgi::DXGI_FRAME_STATISTICS::default();
        // SAFETY: reads of the swap chain's counters on its presenting
        // thread; `statistics` and `made` outlive the calls that fill them.
        let (read, made) = unsafe {
            (
                self.swap_chain.GetFrameStatistics(&raw mut statistics),
                self.swap_chain.GetLastPresentCount(),
            )
        };
        let shown = match read {
            Ok(()) => flui_platform::DxgiShown::Present(flui_platform::DxgiFrameStatistics::new(
                statistics.PresentCount,
                statistics.SyncRefreshCount,
                statistics.SyncQPCTime,
            )),
            Err(_) => flui_platform::DxgiShown::Nothing,
        };
        flui_platform::SwapChainStatistics::Dxgi(flui_platform::DxgiStatistics::new(
            made.expect("a swap chain counts its presents"),
            shown,
        ))
    }

    /// Present the back buffer at the next refresh.
    fn present(&self) {
        // SAFETY: a present on the thread that made the swap chain.
        unsafe {
            self.swap_chain
                .Present(1, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0))
        }
        .ok()
        .expect("a present");
    }
}

impl WindowFixture for Win32Fixture {
    fn window(&self) -> Rc<dyn PlatformWindow> {
        Rc::clone(&self.window) as Rc<dyn PlatformWindow>
    }

    /// The desktop's cursor clip: the client area is a confined pointer, one
    /// pixel of it a locked one, anything else none.
    fn held_grab(&self) -> Option<flui_platform::CursorGrab> {
        Some(cursor_clip_of(self.hwnd))
    }

    /// Another window of this thread takes the keyboard focus, as a click
    /// in it would; `false` where the window under test does not have it to
    /// lose (the test process is not in the foreground).
    fn take_focus_away(&mut self) -> bool {
        // SAFETY: a plain query of this thread's focus.
        if unsafe { GetFocus() } != self.hwnd {
            return false;
        }
        let other = open(&self.platform, options());
        // SAFETY: a window of this thread, live while `other` is held.
        let _ = unsafe { SetFocus(Some(hwnd_of(&other.window))) };
        settle();
        // SAFETY: as above.
        let moved = unsafe { GetFocus() } != self.hwnd;
        self.focus_holder = Some(other);
        moved
    }

    fn readback(&self) -> Readback {
        win32_readback(self.hwnd)
    }

    fn settle(&mut self) {
        settle();
    }

    fn close(&mut self) {
        self.window
            .admit_and_apply(flui_platform::WindowCommand::Close)
            .expect("a Win32 window closes");
        settle();
    }

    /// A `WM_DPICHANGED` that names a DPI of zero and no rectangle, as any
    /// process may send one: over a scale of zero the window's rectangles are
    /// no numbers.
    fn report_no_window(&mut self) -> bool {
        send(&self.window, WM_DPICHANGED, 0, 0);
        settle();
        true
    }

    /// The Win32 input table: whatever its touchpad, a Win32 window hands the
    /// system no input region.
    fn input_capabilities(&self) -> flui_platform::input_capabilities::InputCapabilities {
        flui_native::input::availability::windows(false)
    }

    /// A Win32 window's target hands out its `HWND` on any thread.
    fn has_native_surface(&self) -> bool {
        true
    }

    /// Frames presented through a DXGI swap chain on a window of its own, as
    /// a window an earlier check minimized shows none, each after handing the
    /// window's target the statistics read before it, as the renderer does,
    /// until the statistics have named a shown present twice (the second
    /// measures the refresh period); the swap chain is let go before the
    /// window's next frame, and the window closes after it. DXGI updates its
    /// statistics when the compositor reports a flip, which can take many
    /// presents.
    fn present_and_request_frame(&mut self) -> Option<flui_platform::FrameRequest> {
        let presenting = open(&self.platform, options());
        let hwnd = hwnd_of(&presenting.window);
        // The foreground rules open a test's window behind the window the user
        // works with, and DWM composes no flip of a window a maximized one
        // covers, so its statistics would name nothing shown: above every
        // window that is not topmost, without taking the foreground.
        // SAFETY: plain integers for this fixture's own live window.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .expect("a test window goes above the user's");
        settle();
        let target = presenting.window.surface_target();
        {
            let presenter = DxgiPresenter::on(hwnd);
            let mut shown = std::collections::BTreeSet::new();
            for present in 1..=300 {
                let statistics = presenter.statistics();
                if let flui_platform::SwapChainStatistics::Dxgi(counters) = statistics
                    && let flui_platform::DxgiShown::Present(read) = counters.shown
                    && read.sync_qpc_time > 0
                    && shown.insert(read.present_count)
                {
                    eprintln!(
                        "DXGI before present {}: present {} shown at refresh {}, QPC {}",
                        counters.last_present_count + 1,
                        read.present_count,
                        read.sync_refresh_count,
                        read.sync_qpc_time
                    );
                }
                target.pre_present_notify(
                    Some(flui_testing::window_conformance::fixture_frame(present)),
                    statistics,
                );
                presenter.present();
                let _ = pump_until(Duration::ZERO, || false);
                if shown.len() >= 2 {
                    break;
                }
            }
        }
        presenting.handler.take_calls();
        presenting.window.request_redraw();
        settle();
        let request =
            presenting
                .handler
                .take_calls()
                .into_iter()
                .rev()
                .find_map(|call| match call {
                    flui_testing::window_handler::HandlerCall::FrameRequested(request) => {
                        Some(request)
                    }
                    _ => None,
                });
        if let Some(request) = request {
            eprintln!(
                "the next frame request: vsync {:?}, target present {:?}, presented {:?}, \
                 now {:?}",
                request.vsync,
                request.target_present,
                request.presented,
                flui_platform::EventTime::now()
            );
            // DWM always composes on the hosts Win32 supports, and its report
            // names the next refresh, whichever side of the reading it lies.
            assert!(
                request.target_present.is_some(),
                "a Win32 frame request names the compositor's next refresh: {request:?}"
            );
        }
        presenting
            .window
            .admit_and_apply(flui_platform::WindowCommand::Close)
            .expect("a Win32 window closes");
        settle();
        request
    }
}

fn the_win32_window_conforms() {
    let platform = platform();
    let window = open(&platform, options());
    let hwnd = hwnd_of(&window);
    window_conformance::assert_conforms(&mut Win32Fixture {
        platform,
        window: window.window,
        hwnd,
        focus_holder: None,
    });
}

/// Whether `upper` is above `lower` in the desktop's stacking order.
fn is_above(upper: HWND, lower: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GW_HWNDNEXT, GetWindow};
    let mut next = upper;
    // SAFETY: plain queries of the desktop's window list; a window that
    // closes in between ends the walk.
    while let Ok(below) = unsafe { GetWindow(next, GW_HWNDNEXT) } {
        if below == lower {
            return true;
        }
        next = below;
    }
    false
}

/// A window kept below the others is held on top by fullscreen while it
/// lasts, keeps its level in the snapshot, and goes back under the others
/// when it leaves fullscreen.
fn fullscreen_wins_over_a_level_below_while_it_lasts() {
    use flui_platform::WindowLevel;

    let platform = platform();
    let other = open(&platform, options());
    let window = open(&platform, options());
    settle();
    let (hwnd, other_hwnd) = (hwnd_of(&window), hwnd_of(&other.window));
    apply(
        &window,
        WindowCommand::SetLevel(WindowLevel::AlwaysOnBottom),
    );
    settle();
    assert!(is_above(other_hwnd, hwnd), "kept below the other window");
    // A restack to the top, as an activation or a click asks for, is put
    // back under the others outside fullscreen and kept inside it.
    let raised = |hwnd: HWND| {
        use windows::Win32::UI::WindowsAndMessaging::{HWND_TOP, WINDOWPOS, WM_WINDOWPOSCHANGING};
        let mut pending = WINDOWPOS {
            hwnd,
            hwndInsertAfter: HWND_TOP,
            flags: SWP_NOMOVE | SWP_NOSIZE,
            ..Default::default()
        };
        // SAFETY: `WM_WINDOWPOSCHANGING`'s `lParam` is the address of the
        // pending `WINDOWPOS`, a live local the window reads and writes.
        unsafe {
            SendMessageW(
                hwnd,
                WM_WINDOWPOSCHANGING,
                Some(WPARAM(0)),
                Some(LPARAM((&raw mut pending) as isize)),
            )
        };
        pending.hwndInsertAfter == HWND_TOP
    };
    assert!(
        !raised(hwnd),
        "a normal window kept below is put back under"
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    settle();
    assert!(
        raised(hwnd),
        "fullscreen keeps the window on top while it lasts"
    );
    assert_eq!(
        window.state().level,
        WindowLevel::AlwaysOnBottom,
        "the level is kept"
    );
    apply(&window, WindowCommand::SetMode(WindowMode::Normal));
    settle();
    assert!(
        is_above(other_hwnd, hwnd),
        "under the others again once fullscreen ends"
    );
    assert_eq!(window.state().level, WindowLevel::AlwaysOnBottom);
}

/// Bring `hwnd` to the foreground as a user's click would, from a process the
/// foreground rules do not favour: join the input of the thread in front for
/// the call, then press and release Shift, which makes this process the one
/// that received the last input (the foreground suite's way). Whether `hwnd`
/// is in front.
fn take_foreground(hwnd: HWND) -> bool {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    // SAFETY: plain calls on this thread's own window and the thread in
    // front, whose input is joined only for the one call and let go after;
    // the input array is a stack local of the size named.
    unsafe {
        if !SetForegroundWindow(hwnd).as_bool() {
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let joined = theirs != 0
                && theirs != ours
                && windows::Win32::System::Threading::AttachThreadInput(ours, theirs, true)
                    .as_bool();
            let _ = SetForegroundWindow(hwnd);
            if joined {
                let _ = windows::Win32::System::Threading::AttachThreadInput(ours, theirs, false);
            }
        }
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_SHIFT,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let tap = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        let _ = SendInput(&tap, std::mem::size_of::<INPUT>() as i32);
        settle();
        GetForegroundWindow() == hwnd
    }
}

/// The grab the desktop's cursor clip holds for `hwnd`: its client area is a
/// confined pointer, one pixel of it a locked one, anything else none.
fn cursor_clip_of(hwnd: HWND) -> flui_platform::CursorGrab {
    use windows::Win32::UI::WindowsAndMessaging::GetClipCursor;
    let mut clip = RECT::default();
    let mut client = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: plain queries into live locals.
    let read = unsafe {
        GetClipCursor(&raw mut clip).is_ok()
            && GetClientRect(hwnd, &raw mut client).is_ok()
            && ClientToScreen(hwnd, &raw mut origin).as_bool()
    };
    if !read {
        return flui_platform::CursorGrab::None;
    }
    let on_screen = RECT {
        left: origin.x,
        top: origin.y,
        right: origin.x + client.right,
        bottom: origin.y + client.bottom,
    };
    if clip == on_screen {
        flui_platform::CursorGrab::Confined
    } else if clip.right - clip.left == 1
        && clip.bottom - clip.top == 1
        && (on_screen.left..on_screen.right).contains(&clip.left)
        && (on_screen.top..on_screen.bottom).contains(&clip.top)
    {
        flui_platform::CursorGrab::Locked
    } else {
        flui_platform::CursorGrab::None
    }
}

/// A focused window holds the pointer as asked, on the desktop's cursor clip
/// and in its snapshot, follows the client area while confined, and lets it
/// go with the focus; an unfocused window takes no grab.
fn a_grab_is_held_while_focused_and_let_go_with_the_focus() {
    use flui_platform::CursorGrab;

    let platform = platform();
    let window = open(&platform, options());
    settle();
    let hwnd = hwnd_of(&window);
    // A grab is taken only by the window with the keyboard: the test brings
    // the window forward as a click would (on a desktop of the test's own it
    // is in front already), gives it the focus, and cannot check anything
    // without it.
    let _ = take_foreground(hwnd);
    // SAFETY: this thread's window.
    let _ = unsafe { SetFocus(Some(hwnd)) };
    settle();
    // SAFETY: a plain query of this thread's focus.
    let focus = unsafe { GetFocus() };
    assert_eq!(
        focus, hwnd,
        "the test window must hold the keyboard to hold the pointer"
    );
    for grab in [CursorGrab::Confined, CursorGrab::Locked] {
        apply(&window, WindowCommand::SetCursorGrab(grab));
        settle();
        assert_eq!(window.state().cursor_grab, grab, "{grab:?} published");
        assert_eq!(cursor_clip_of(hwnd), grab, "{grab:?} held by the desktop");
    }
    // A lock keeps its pixel of the client area when the window moves and
    // resizes, never an old screen pixel over another window.
    apply(
        &window,
        WindowCommand::SetPosition(on_its_display(&window, 900.0, 700.0)),
    );
    settle();
    assert_eq!(
        cursor_clip_of(hwnd),
        CursorGrab::Locked,
        "the lock follows a move"
    );
    apply(&window, WindowCommand::SetSize(extent(200.0, 150.0)));
    settle();
    assert_eq!(
        cursor_clip_of(hwnd),
        CursorGrab::Locked,
        "the lock stays inside a smaller client"
    );
    assert_eq!(window.state().cursor_grab, CursorGrab::Locked);
    apply(&window, WindowCommand::SetCursorGrab(CursorGrab::Confined));
    apply(&window, WindowCommand::SetSize(extent(500.0, 300.0)));
    settle();
    assert_eq!(
        cursor_clip_of(hwnd),
        CursorGrab::Confined,
        "the clip follows the resized client area"
    );
    let other = open(&platform, options());
    // SAFETY: a window of this thread, live while `other` is held.
    let _ = unsafe { SetFocus(Some(hwnd_of(&other.window))) };
    settle();
    assert_eq!(
        window.state().cursor_grab,
        CursorGrab::None,
        "published let go"
    );
    assert_eq!(
        cursor_clip_of(hwnd),
        CursorGrab::None,
        "the clip went with the focus"
    );
    apply(&window, WindowCommand::SetCursorGrab(CursorGrab::Locked));
    settle();
    assert_eq!(
        window.state().cursor_grab,
        CursorGrab::None,
        "a window without the focus takes no grab"
    );
    assert_eq!(cursor_clip_of(hwnd), CursorGrab::None);
}

/// A resize by the frame is the user resizing the window from the loop's
/// first sizing step to its end; the snapshot after the loop says the user
/// no longer is.
fn a_resize_by_the_frame_is_a_live_resize_until_the_loop_ends() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let events = record_interactions(&window);
    // The first arrow picks the right edge, the rest move it; Enter keeps the
    // size.
    let sizer = post_keys(
        &window,
        vec![VK_RIGHT, VK_RIGHT, VK_RIGHT, VK_RIGHT, VK_RETURN],
    );
    send(&window, WM_SYSCOMMAND, SC_SIZE as usize, 0);
    sizer.join().expect("the key thread");
    let events = events.lock().expect("log").clone();
    assert!(
        events.iter().any(|(placed, _)| *placed),
        "the loop resized the window: {events:?}"
    );
    assert!(
        events
            .iter()
            .filter(|(placed, _)| *placed)
            .all(|(_, interaction)| *interaction == WindowInteraction::UserResizing),
        "every report that resized it says the user is resizing it: {events:?}"
    );
    assert_eq!(
        window.state().interaction,
        WindowInteraction::None,
        "the end of the loop ends the live resize: {events:?}"
    );
}

/// The rectangle a drag of the frame proposes keeps the steps and the
/// proportion the application asked for, the dragged edges giving way; with
/// neither asked, the default procedure keeps the proposal.
fn a_frame_drag_keeps_the_steps_and_the_proportion() {
    use std::num::NonZeroU32;
    use windows::Win32::UI::WindowsAndMessaging::{WM_SIZING, WMSZ_BOTTOMRIGHT};

    let platform = platform();
    let window = open(&platform, options());
    settle();
    let hwnd = hwnd_of(&window);
    let scale = window.state().metrics.expect("drawable").scale().get();
    let outer = rect(&window);
    let mut client = RECT::default();
    // SAFETY: a plain query into a live local.
    unsafe { GetClientRect(hwnd, &raw mut client) }.expect("a client area");
    let frame = (
        (outer.right - outer.left) - client.right,
        (outer.bottom - outer.top) - client.bottom,
    );
    let drag = |proposed: RECT| {
        let mut proposed = proposed;
        // SAFETY: `WM_SIZING`'s `lParam` is the address of the dragged
        // rectangle, a live local the window reads and writes back.
        unsafe {
            SendMessageW(
                hwnd,
                WM_SIZING,
                Some(WPARAM(WMSZ_BOTTOMRIGHT as usize)),
                Some(LPARAM((&raw mut proposed) as isize)),
            )
        };
        proposed
    };
    let proposal = RECT {
        left: outer.left,
        top: outer.top,
        right: outer.left + frame.0 + 333,
        bottom: outer.top + frame.1 + 251,
    };
    assert_eq!(drag(proposal), proposal, "nothing asked, nothing held");

    let step = (50.0 * scale).round() as i32;
    apply(
        &window,
        WindowCommand::SetResizeIncrements(Some(
            WindowExtent::try_new(Size::new(50.0, 50.0)).expect("a step"),
        )),
    );
    let held = drag(proposal);
    assert_eq!(
        (held.left, held.top),
        (proposal.left, proposal.top),
        "the anchor stays"
    );
    let content = (
        held.right - held.left - frame.0,
        held.bottom - held.top - frame.1,
    );
    assert_eq!(
        (content.0 % step, content.1 % step),
        (0, 0),
        "{content:?} in steps of {step}"
    );
    assert!(
        content.0 <= 333 && content.1 <= 251,
        "{content:?} rounded down"
    );

    apply(&window, WindowCommand::SetResizeIncrements(None));
    apply(
        &window,
        WindowCommand::SetAspectRatio(Some(flui_platform::AspectRatio::new(
            NonZeroU32::new(2).expect("nonzero"),
            NonZeroU32::new(1).expect("nonzero"),
        ))),
    );
    let held = drag(proposal);
    let content = (
        held.right - held.left - frame.0,
        held.bottom - held.top - frame.1,
    );
    assert_eq!(content, (333, 167), "the width decides at a corner");
}

/// The snapshot's level is what Win32 holds: kept above is the window's
/// `WS_EX_TOPMOST`, which Win32 grants a window of the foreground and may
/// leave unset for one in the background, and the snapshot says so either
/// way; kept below is the host's, and back to neither.
fn a_level_is_what_win32_holds() {
    use flui_platform::WindowLevel;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, WS_EX_TOPMOST};

    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::Activate);
    settle();
    let hwnd = hwnd_of(&window);
    // SAFETY: plain queries.
    let foreground = unsafe { GetForegroundWindow() } == hwnd;
    let topmost = || {
        // SAFETY: a plain style query of the live window.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        style & WS_EX_TOPMOST.0 != 0
    };
    for level in [
        WindowLevel::AlwaysOnTop,
        WindowLevel::AlwaysOnBottom,
        WindowLevel::Normal,
    ] {
        apply(&window, WindowCommand::SetLevel(level));
        settle();
        let held = match (topmost(), level) {
            (true, _) => WindowLevel::AlwaysOnTop,
            (false, WindowLevel::AlwaysOnBottom) => WindowLevel::AlwaysOnBottom,
            (false, _) => WindowLevel::Normal,
        };
        assert_eq!(
            window.state().level,
            held,
            "asked {level:?}: the snapshot says what Win32 holds"
        );
        if foreground {
            assert_eq!(held, level, "a foreground window takes {level:?}");
        }
    }
}

/// A live resize runs Win32's modal sizing loop: each `WM_SIZE` of it
/// publishes the snapshot, whose metrics are the size and scale Win32 reports
/// at that step, and announces it, all before the
/// loop returns.
fn a_live_resize_publishes_inside_its_modal_loop() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let hwnd = hwnd_of(&window);
    let address = hwnd.0 as isize;
    let before = window.state().metrics.expect("a shown window is drawable");
    let steps = listen_for_snapshots(&window);
    let done = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&done);
    let sizer = std::thread::spawn(move || {
        // The first arrow picks the right edge, the rest move it; Enter keeps
        // the size.
        let keys = std::iter::repeat_n(VK_RIGHT, 6).chain([VK_RETURN]);
        for key in keys {
            std::thread::sleep(Duration::from_millis(60));
            // SAFETY: a post to a window of this process; one that is gone
            // fails the call.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(address as *mut _)),
                    WM_KEYDOWN,
                    WPARAM(usize::from(key.0)),
                    LPARAM(0x0001_0001),
                )
            };
        }
        while !finished.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
            // SAFETY: as above.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(address as *mut _)),
                    WM_KEYDOWN,
                    WPARAM(usize::from(VK_ESCAPE.0)),
                    LPARAM(0x0001_0001),
                )
            };
        }
    });
    // The keyboard sizing loop, modal until the keys above end it.
    send(&window, WM_SYSCOMMAND, SC_SIZE as usize, 0);
    let inside = steps.lock().expect("log").clone();
    done.store(true, Ordering::SeqCst);
    sizer.join().expect("the sizing thread");
    the_snapshots_carry_each_resize(&inside, before);
    let grown = window.state().metrics.expect("drawable");
    assert!(
        grown.physical().width > before.physical().width,
        "the loop grew the window: {before:?} to {grown:?}"
    );
}

/// Each snapshot `window` publishes from here on: whether it moved or
/// changed mode from the one before, and the interaction it states.
fn record_interactions(window: &Opened) -> Arc<std::sync::Mutex<Vec<(bool, WindowInteraction)>>> {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    on_changes(window, move |_, previous, next| {
        let placed = previous.outer != next.outer || previous.mode != next.mode;
        log.lock().expect("log").push((placed, next.interaction));
    });
    events
}

/// Post `keys` to `window` from another thread, one every 60 ms, while the
/// window runs a modal loop on this one.
fn post_keys(
    window: &Rc<dyn HostWindow>,
    keys: Vec<windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY>,
) -> std::thread::JoinHandle<()> {
    let address = hwnd_of(window).0 as isize;
    std::thread::spawn(move || {
        for key in keys {
            std::thread::sleep(Duration::from_millis(60));
            // SAFETY: a post to a window of this process; one that is gone
            // fails the call.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(address as *mut _)),
                    WM_KEYDOWN,
                    WPARAM(usize::from(key.0)),
                    LPARAM(0x0001_0001),
                )
            };
        }
    })
}

/// A move in Win32's size-move loop (the system menu's move, by the
/// keyboard) is reported as the user placing the window on the reports that
/// move it, and the snapshot published as the loop ends says the user no
/// longer is.
fn the_size_move_loop_states_the_users_placement_and_ends_it() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let events = record_interactions(&window);
    let mover = post_keys(&window, vec![VK_RIGHT, VK_RIGHT, VK_RIGHT, VK_RETURN]);
    send(&window, WM_SYSCOMMAND, SC_MOVE as usize, 0);
    mover.join().expect("the key thread");
    let events = events.lock().expect("log").clone();
    assert!(
        events.iter().any(|(placed, _)| *placed),
        "the loop moved the window: {events:?}"
    );
    assert!(
        events
            .iter()
            .filter(|(placed, _)| *placed)
            .all(|(_, interaction)| *interaction == WindowInteraction::UserPlacing),
        "every report that moved it says the user: {events:?}"
    );
    assert_eq!(
        window.state().interaction,
        WindowInteraction::None,
        "the end of the loop ends the user's placement: {events:?}"
    );
}

/// A placement system command the user works (here `SC_MAXIMIZE`, as a
/// caption button sends it) is reported as the user's on the report that
/// carries it, and the snapshot after it says the user no longer is.
fn a_system_command_states_the_users_placement_and_ends_it() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let events = record_interactions(&window);
    send(
        &window,
        WM_SYSCOMMAND,
        windows::Win32::UI::WindowsAndMessaging::SC_MAXIMIZE as usize,
        0,
    );
    let events = events.lock().expect("log").clone();
    assert_eq!(window.state().mode, WindowMode::Maximized);
    assert!(
        events
            .iter()
            .any(|(placed, interaction)| *placed && *interaction == WindowInteraction::UserPlacing),
        "the report that maximized it says the user: {events:?}"
    );
    assert_eq!(
        window.state().interaction,
        WindowInteraction::None,
        "the command's end ends the user's placement: {events:?}"
    );
}

/// The restore the window sends itself before entering fullscreen from
/// maximized is the application's command: no report of it says the user.
fn the_applications_own_restore_is_not_the_users() {
    let platform = platform();
    let window = open(&platform, options());
    apply(&window, WindowCommand::SetMode(WindowMode::Maximized));
    settle();
    let events = record_interactions(&window);
    apply(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    let events = events.lock().expect("log").clone();
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    assert!(
        events
            .iter()
            .all(|(_, interaction)| *interaction == WindowInteraction::None),
        "no report of the application's command says the user: {events:?}"
    );
}

fn a_locked_session_or_a_cloak_occludes_a_shown_window() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let events = record_visibility(&window);
    send(&window, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 1);
    assert_eq!(*events.lock().expect("log"), [false]);
    let locked = window.state();
    assert!(
        locked.visible && locked.occluded && !locked.is_seen(),
        "a runtime that first reads the window now must not see it seen: {locked:?}"
    );
    send(
        &window,
        WM_WTSSESSION_CHANGE,
        WTS_SESSION_UNLOCK as usize,
        1,
    );
    assert_eq!(*events.lock().expect("log"), [false, true]);
    assert!(window.state().is_seen());

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

/// The thread's one subscription to cloak changes serves each of its
/// windows: a cloak of either is that window's alone, heard once, and the
/// window opened first still hears one after the other closed.
fn each_window_hears_its_own_cloak() {
    let platform = platform();
    let (first, second) = (open(&platform, options()), open(&platform, options()));
    settle();
    let (first_events, second_events) = (record_visibility(&first), record_visibility(&second));
    let cloak = |window: &Opened, on: bool| {
        let value = BOOL::from(on);
        // SAFETY: `value` is a live `BOOL`, the size `DWMWA_CLOAK` reads.
        unsafe {
            DwmSetWindowAttribute(
                hwnd_of(window),
                DWMWA_CLOAK,
                (&raw const value).cast(),
                std::mem::size_of::<BOOL>() as u32,
            )
        }
        .expect("the window cloaks");
    };
    let heard = |events: &Arc<std::sync::Mutex<Vec<bool>>>, count: usize| {
        pump_until(Duration::from_secs(1), || {
            events.lock().expect("log").len() == count
        })
    };
    cloak(&second, true);
    assert!(
        heard(&second_events, 1),
        "the second window hears its cloak"
    );
    cloak(&second, false);
    assert!(heard(&second_events, 2));
    settle();
    assert_eq!(*second_events.lock().expect("log"), [false, true]);
    assert!(
        first_events.lock().expect("log").is_empty(),
        "the first window heard the second's cloak"
    );

    second.close_for_cleanup();
    settle();
    cloak(&first, true);
    assert!(
        heard(&first_events, 1),
        "the first window still hears its cloak once the second closed"
    );
    cloak(&first, false);
    assert!(heard(&first_events, 2));
    assert_eq!(*first_events.lock().expect("log"), [false, true]);
}

/// A close command closes the window without asking the should-close veto: a
/// command is not a close request.
fn a_close_command_asks_no_veto() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let asked = Arc::new(AtomicUsize::new(0));
    let veto = Arc::clone(&asked);
    window.handler.react_to_close_request(move || {
        veto.fetch_add(1, Ordering::SeqCst);
        flui_platform::CloseDecision::KeepOpen
    });
    let closed = Arc::new(AtomicBool::new(false));
    let heard = Arc::clone(&closed);
    window
        .handler
        .react_to_close(move || heard.store(true, Ordering::SeqCst));
    window
        .admit_and_apply(flui_platform::WindowCommand::Close)
        .expect("a close command is admitted");
    assert!(
        pump_until(Duration::from_secs(1), || closed.load(Ordering::SeqCst)),
        "the owner closed the window"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the veto was asked");
}

/// A window the user closes leaves the platform's map and is dropped: once the
/// test lets go of its own handle, nothing keeps the wrapper alive. Runs only on
/// a Windows host.
fn win32_close_drains_platform_map_entry() {
    let platform = platform();
    let window = open(&platform, options());
    settle();
    let closed = Arc::new(AtomicBool::new(false));
    let heard = Arc::clone(&closed);
    window
        .handler
        .react_to_close(move || heard.store(true, Ordering::SeqCst));
    let wrapper = Rc::downgrade(&window.window);
    let hwnd = hwnd_of(&window);
    // SAFETY: a message posted to the live window on its creating thread, as
    // the system's close button posts it.
    unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
        .expect("WM_CLOSE is posted");
    assert!(
        pump_until(Duration::from_secs(1), || closed.load(Ordering::SeqCst)),
        "the window closed"
    );
    drop(window);
    assert!(
        pump_until(Duration::from_secs(1), || wrapper.upgrade().is_none()),
        "the platform still holds the closed window's wrapper"
    );
}

#[test]
fn native_window_commands() {
    let cases: &[(&str, fn())] = &[
        (
            "a_window_opened_at_open_is_shown",
            a_window_opened_at_open_is_shown,
        ),
        ("a_close_command_asks_no_veto", a_close_command_asks_no_veto),
        (
            "win32_close_drains_platform_map_entry",
            win32_close_drains_platform_map_entry,
        ),
        (
            "an_app_title_bar_drops_the_caption_and_keeps_the_content",
            an_app_title_bar_drops_the_caption_and_keeps_the_content,
        ),
        (
            "the_system_hit_test_answers_from_the_chrome_regions",
            the_system_hit_test_answers_from_the_chrome_regions,
        ),
        (
            "caption_buttons_take_the_legacy_mouse_messages_over_them",
            caption_buttons_take_the_legacy_mouse_messages_over_them,
        ),
        (
            "a_scale_change_keeps_the_contents_logical_size",
            a_scale_change_keeps_the_contents_logical_size,
        ),
        (
            "a_move_off_a_caption_button_ends_the_hover_it_saw",
            a_move_off_a_caption_button_ends_the_hover_it_saw,
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
            "the_frame_follows_resizable_and_the_system_frame_and_keeps_the_content",
            the_frame_follows_resizable_and_the_system_frame_and_keeps_the_content,
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
            "a_fullscreen_window_maximized_again_never_reads_normal",
            a_fullscreen_window_maximized_again_never_reads_normal,
        ),
        (
            "a_minimized_fullscreen_window_leaves_in_one_step",
            a_minimized_fullscreen_window_leaves_in_one_step,
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
            "a_hidden_windows_frame_request_reaches_its_frame_callback",
            a_hidden_windows_frame_request_reaches_its_frame_callback,
        ),
        (
            "a_burst_of_frame_requests_on_a_hidden_window_is_one_frame",
            a_burst_of_frame_requests_on_a_hidden_window_is_one_frame,
        ),
        (
            "an_unseen_window_with_nothing_asked_runs_no_frame",
            an_unseen_window_with_nothing_asked_runs_no_frame,
        ),
        (
            "a_redraw_asked_from_another_thread_reaches_the_window",
            a_redraw_asked_from_another_thread_reaches_the_window,
        ),
        (
            "a_surface_target_is_unavailable_once_its_window_closed",
            a_surface_target_is_unavailable_once_its_window_closed,
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
            "a_size_past_the_largest_surface_is_admitted",
            a_size_past_the_largest_surface_is_admitted,
        ),
        (
            "a_key_the_application_lets_through_leaves_the_mode_alone",
            a_key_the_application_lets_through_leaves_the_mode_alone,
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
            "a_locked_session_or_a_cloak_occludes_a_shown_window",
            a_locked_session_or_a_cloak_occludes_a_shown_window,
        ),
        (
            "each_window_hears_its_own_cloak",
            each_window_hears_its_own_cloak,
        ),
        (
            "a_tree_published_inside_the_activation_answers_it",
            a_tree_published_inside_the_activation_answers_it,
        ),
        ("the_win32_window_conforms", the_win32_window_conforms),
        (
            "a_live_resize_publishes_inside_its_modal_loop",
            a_live_resize_publishes_inside_its_modal_loop,
        ),
        (
            "a_grab_is_held_while_focused_and_let_go_with_the_focus",
            a_grab_is_held_while_focused_and_let_go_with_the_focus,
        ),
        (
            "a_resize_by_the_frame_is_a_live_resize_until_the_loop_ends",
            a_resize_by_the_frame_is_a_live_resize_until_the_loop_ends,
        ),
        (
            "a_frame_drag_keeps_the_steps_and_the_proportion",
            a_frame_drag_keeps_the_steps_and_the_proportion,
        ),
        ("a_level_is_what_win32_holds", a_level_is_what_win32_holds),
        (
            "fullscreen_wins_over_a_level_below_while_it_lasts",
            fullscreen_wins_over_a_level_below_while_it_lasts,
        ),
        (
            "the_size_move_loop_states_the_users_placement_and_ends_it",
            the_size_move_loop_states_the_users_placement_and_ends_it,
        ),
        (
            "a_system_command_states_the_users_placement_and_ends_it",
            a_system_command_states_the_users_placement_and_ends_it,
        ),
        (
            "the_applications_own_restore_is_not_the_users",
            the_applications_own_restore_is_not_the_users,
        ),
        (
            "a_dpi_change_announces_one_whole_snapshot",
            a_dpi_change_announces_one_whole_snapshot,
        ),
        (
            "a_dpi_no_screen_has_is_refused_and_the_window_keeps_its_scale",
            a_dpi_no_screen_has_is_refused_and_the_window_keeps_its_scale,
        ),
        (
            "a_run_of_zero_dpi_messages_is_reported_once",
            a_run_of_zero_dpi_messages_is_reported_once,
        ),
        (
            "activation_is_a_fact_apart_from_focus",
            activation_is_a_fact_apart_from_focus,
        ),
    ];
    crate::run_table("native_window_commands", cases);
}

/// The cases that hold a modal loop idle and count what ran on the owner
/// thread meanwhile, in a process of their own: the cases above leave their
/// windows shown until the process ends, and on a desktop where no window is
/// in the foreground (the run's own, which takes no input) the system moves
/// the activation among those windows while the loop stands, so each window
/// is sent `WM_NCACTIVATE`, `WM_GETICON` and `WM_PAINT`, and the platform's
/// code runs for each as it must.
#[test]
fn native_idle_modal_loops() {
    let cases: &[(&str, fn())] = &[
        (
            "an_idle_modal_loop_with_no_deadline_asks_nothing",
            an_idle_modal_loop_with_no_deadline_asks_nothing,
        ),
        (
            "a_deadline_a_turn_sets_during_an_idle_modal_loop_is_delivered_in_it",
            a_deadline_a_turn_sets_during_an_idle_modal_loop_is_delivered_in_it,
        ),
        (
            "a_redraw_from_another_thread_is_painted_in_an_idle_modal_loop",
            a_redraw_from_another_thread_is_painted_in_an_idle_modal_loop,
        ),
    ];
    crate::run_table("native_idle_modal_loops", cases);
}

/// Limits with these bounds.
fn limits(
    min: Option<flui_foundation::geometry::Size<f64>>,
    max: Option<flui_foundation::geometry::Size<f64>>,
) -> flui_platform::SizeConstraints {
    flui_platform::SizeConstraints::try_new(min, max).expect("finite limits")
}

/// A rectangle with this position and size.
fn geometry(
    position: Option<WindowPosition>,
    size: flui_foundation::geometry::Size<f64>,
) -> flui_platform::WindowGeometry {
    flui_platform::WindowGeometry::try_new(position, size).expect("a finite rectangle")
}
