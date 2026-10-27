//! The window half of the precision touchpad, on a real window and a real
//! DirectManipulation viewport. No precision touchpad drives it: each case
//! appends the readings a manipulation reports to the viewport's queue (the
//! same queue its event handler fills during `Update`), then drives the
//! window's own messages and pump.

use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::RecordingWindowHandler;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use flui_foundation::geometry::Size;
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::pointer::PointerEvent;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{GetUpdateRect, ValidateRect};
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, SendMessageW, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_KILLFOCUS, WM_PAINT,
};

use super::PUMP_TIMER;
use crate::WindowsPlatform;
use crate::input::assembly::{ContentTransform, ViewportReading, ViewportStatus};
use crate::input::availability as input_availability;
use crate::input::windows::InputWindow;
use crate::window::windows::WindowsWindow;
use crate::{host_window::HostWindow, platform::Platform as _};
use flui_platform::{DispatchEventResult, PlatformInput, WindowOptions};

use ViewportStatus::{Ready, Running};

type Log = Arc<Mutex<Vec<String>>>;

struct Fixture {
    // Field order: the window closes before its platform goes.
    window: Rc<dyn HostWindow>,
    handler: Rc<RecordingWindowHandler>,
    _platform: WindowsPlatform,
    hwnd: HWND,
    log: Log,
}

fn content(y: f64) -> ViewportReading {
    ViewportReading::Content(ContentTransform::try_new(1.0, 0.0, y).expect("a finite transform"))
}

const fn status(status: ViewportStatus) -> ViewportReading {
    ViewportReading::Status(status)
}

/// A scroll as its phase and its vertical step in device pixels.
fn describe(input: &PlatformInput, scale: f64) -> Option<String> {
    match input.as_pointer()? {
        PointerEvent::Scroll(scroll) => Some(format!(
            "{:?} {:.0}",
            scroll.phase.expect("a touchpad scroll is phased"),
            scroll.delta.y() * scale
        )),
        _ => None,
    }
}

fn scale_of(hwnd: HWND) -> f64 {
    input(hwnd).scale_factor()
}

/// What the input keeps of the fixture's window.
fn input(hwnd: HWND) -> std::rc::Rc<InputWindow> {
    crate::input::windows::testing::window_input(hwnd).expect("the window's input")
}

fn push(hwnd: HWND, readings: &[ViewportReading]) {
    let input = input(hwnd);
    let host = input.touchpad.host.borrow();
    let host = host.as_ref().expect("the window's viewport initialised");
    host.viewport.queue.borrow_mut().extend_from_slice(readings);
}

fn pump(hwnd: HWND) {
    input(hwnd).pump_touchpad(hwnd);
}

/// Whether the pacing timer was set; clears it.
fn take_timer(hwnd: HWND) -> bool {
    // SAFETY: `KillTimer` takes the fixture's own window and a constant id.
    unsafe { KillTimer(Some(hwnd), PUMP_TIMER) }.is_ok()
}

/// Whether the window asked for a frame since `validate`.
fn invalidated(hwnd: HWND) -> bool {
    // SAFETY: the fixture's own window; no rectangle is requested.
    unsafe { GetUpdateRect(hwnd, None, false) }.as_bool()
}

fn validate(hwnd: HWND) {
    // SAFETY: the fixture's own window, all of its client area.
    let _ = unsafe { ValidateRect(Some(hwnd), None) };
}

fn send(hwnd: HWND, message: u32) {
    // SAFETY: an argument-free message to the fixture's own window on its
    // owner thread.
    unsafe { SendMessageW(hwnd, message, None, None) };
}

/// A window whose input callback logs scrolls, and calls `nested`
/// once, from inside the callback, on the first scroll it receives.
fn fixture(nested: Option<fn(HWND)>) -> Fixture {
    let platform = WindowsPlatform::new().expect("platform");
    let handler = RecordingWindowHandler::new();
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui touchpad")
                .with_size(Size::new(200.0, 120.0))
                // Shown: a hidden window keeps no update region.
                .with_visible(true),
            handler.clone(),
        )
        .expect("window");
    let hwnd = window
        .as_any()
        .downcast_ref::<WindowsWindow>()
        .expect("Win32 window")
        .hwnd();
    let scale = scale_of(hwnd);
    let log = Log::default();
    let inputs = Arc::clone(&log);
    let nested = Cell::new(nested);
    let raw = hwnd.0 as isize;
    handler.react_to_input(move |input| {
        if let Some(line) = describe(&input, scale) {
            inputs.lock().expect("log").push(line);
            if let Some(nested) = nested.take() {
                nested(HWND(raw as *mut _));
            }
        }
        DispatchEventResult::default()
    });
    let frames = Arc::clone(&log);
    handler.react_to_frame(move |_| {
        frames.lock().expect("log").push("frame".to_owned());
    });
    // What the viewport reported while it was built is not a manipulation.
    pump(hwnd);
    let _ = take_timer(hwnd);
    log.lock().expect("log").clear();
    Fixture {
        window,
        handler,
        _platform: platform,
        hwnd,
        log,
    }
}

impl Fixture {
    fn lines(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().expect("log"))
    }
}

/// Red-checks: drop the `InvalidateRect` (no frame is requested), keep the
/// timer when the viewport rests, or skip `keep_pumping`.
fn a_moving_viewport_requests_its_own_frames_and_a_resting_one_none(f: &Fixture) {
    validate(f.hwnd);
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Began 8"]);
    assert!(invalidated(f.hwnd), "a running viewport asks for its frame");
    assert!(take_timer(f.hwnd), "and paces itself while no frame runs");
    // The next step sets the pacing timer again; the stop must clear it.
    push(f.hwnd, &[content(-12.0)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Changed 4"]);

    validate(f.hwnd);
    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Ended 0"]);
    assert!(!invalidated(f.hwnd), "a resting viewport asks for nothing");
    assert!(!take_timer(f.hwnd), "and has no timer");
}

/// Red-check: move the pump after the frame callback in `WM_PAINT`, or
/// pump a resting viewport there.
fn a_frame_pumps_a_moving_viewport_before_its_callback(f: &Fixture) {
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    push(f.hwnd, &[content(-12.0)]);
    send(f.hwnd, WM_PAINT);
    assert_eq!(f.lines(), ["Began 8", "Changed 4", "frame"]);

    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    let _ = f.lines();
    // At rest a frame does not run the viewport: what is queued waits.
    push(f.hwnd, &[status(Running), content(-8.0)]);
    send(f.hwnd, WM_PAINT);
    assert_eq!(f.lines(), ["frame"]);
    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Began 8", "Ended 0"]);
}

/// Red-check: dispatch each pump's input directly instead of through the
/// outbox (the nested step overtakes the outer one).
fn a_pump_nested_in_an_input_callback_delivers_in_order(f: &Fixture) {
    push(f.hwnd, &[status(Running), content(-8.0), content(-12.0)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Began 8", "Changed 4", "Changed 8"]);
}

fn nested_pump(hwnd: HWND) {
    push(hwnd, &[content(-20.0)]);
    pump(hwnd);
}

/// Red-check: leave `WM_ENTERSIZEMOVE` or `WM_EXITSIZEMOVE` unhandled.
fn the_size_and_move_loop_cancels_and_pauses_until_it_ends(f: &Fixture) {
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    send(f.hwnd, WM_ENTERSIZEMOVE);
    assert_eq!(f.lines(), ["Began 8", "Cancelled 0"]);
    assert!(!take_timer(f.hwnd), "a paused viewport is not paced");

    push(f.hwnd, &[status(Running), content(-3.0)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), Vec::<String>::new(), "paused: nothing runs");

    send(f.hwnd, WM_EXITSIZEMOVE);
    pump(f.hwnd);
    assert_eq!(
        f.lines(),
        ["Began 3"],
        "resumed, measured from the origin the pause reset to"
    );
    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    let _ = f.lines();
}

/// Red-checks: leave `WM_KILLFOCUS` without `cancel_pointers`, or have the
/// cancellation unpause a viewport the size-and-move loop paused.
fn focus_loss_cancels_without_pausing(f: &Fixture) {
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    send(f.hwnd, WM_KILLFOCUS);
    assert_eq!(f.lines(), ["Began 8", "Cancelled 0"]);
    assert!(!take_timer(f.hwnd), "a cancelled viewport is not paced");

    // An inactive window still scrolls under the cursor.
    push(f.hwnd, &[status(Running), content(-3.0)]);
    pump(f.hwnd);
    assert_eq!(
        f.lines(),
        ["Began 3"],
        "measured from the origin it reset to"
    );
    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    let _ = f.lines();

    // Losing focus inside the size-and-move loop keeps the pause.
    send(f.hwnd, WM_ENTERSIZEMOVE);
    send(f.hwnd, WM_KILLFOCUS);
    push(f.hwnd, &[status(Running), content(-5.0)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), Vec::<String>::new(), "still paused");
    send(f.hwnd, WM_EXITSIZEMOVE);
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Began 5"]);
    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    let _ = f.lines();
}

/// The cases sharing one window each, by name.
type Row = (&'static str, Option<fn(HWND)>, fn(&Fixture));

#[test]
fn the_window_pumps_paces_and_pauses_its_viewport() {
    let rows: &[Row] = &[
        (
            "a_moving_viewport_requests_its_own_frames_and_a_resting_one_none",
            None,
            a_moving_viewport_requests_its_own_frames_and_a_resting_one_none,
        ),
        (
            "a_frame_pumps_a_moving_viewport_before_its_callback",
            None,
            a_frame_pumps_a_moving_viewport_before_its_callback,
        ),
        (
            "a_pump_nested_in_an_input_callback_delivers_in_order",
            Some(nested_pump),
            a_pump_nested_in_an_input_callback_delivers_in_order,
        ),
        (
            "the_size_and_move_loop_cancels_and_pauses_until_it_ends",
            None,
            the_size_and_move_loop_cancels_and_pauses_until_it_ends,
        ),
        (
            "focus_loss_cancels_without_pausing",
            None,
            focus_loss_cancels_without_pausing,
        ),
    ];
    for (name, nested, case) in rows {
        eprintln!("case: {name}");
        let f = fixture(*nested);
        case(&f);
        f.window.close_now();
    }
}

/// A focus loss with a touchpad pan and a mouse press open ends both, in
/// time order: the pan's cancellation is stamped from the performance
/// counter, the press's from the message time, and the press's is not
/// placed before it.
///
/// Red-check: deliver the touchpad's input without raising the window
/// clock's floor.
#[test]
fn a_focus_loss_stamps_its_cancellations_in_order() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN;

    let f = fixture(None);
    let timeline = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&timeline);
    f.handler.react_to_input(move |input| {
        let entry = match input.as_pointer() {
            Some(PointerEvent::Scroll(scroll)) => Some(("scroll", scroll.time)),
            Some(PointerEvent::Down(press)) => Some(("down", press.sample.time)),
            Some(PointerEvent::Cancel(cancel)) => Some(("cancel", cancel.time)),
            _ => None,
        };
        if let Some(entry) = entry {
            recorded.lock().expect("timeline").push(entry);
        }
        DispatchEventResult::default()
    });
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    // SAFETY: integer message data for the fixture's own window on its
    // owner thread: a primary press at client (10, 10).
    unsafe {
        SendMessageW(
            f.hwnd,
            WM_LBUTTONDOWN,
            Some(WPARAM(0x0001)),
            Some(LPARAM((0x000a << 16) | 0x000a)),
        )
    };
    send(f.hwnd, WM_KILLFOCUS);
    let timeline = timeline.lock().expect("timeline").clone();
    let kinds: Vec<_> = timeline.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(
        kinds,
        ["scroll", "down", "scroll", "cancel"],
        "{timeline:?}"
    );
    assert!(
        timeline.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "every stamp follows the one delivered before it: {timeline:?}"
    );
    f.window.close_now();
}

/// Red-check: drop `release_touchpad` from `WM_DESTROY` (the open pan is
/// never cancelled).
#[test]
fn destroying_the_window_cancels_what_is_open_before_it_closes() {
    let f = fixture(None);
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    let closes = Arc::clone(&f.log);
    f.handler.react_to_close(move || {
        closes.lock().expect("log").push("close".to_owned());
    });
    f.window.close_now();
    assert_eq!(f.lines(), ["Began 8", "Cancelled 0", "close"]);
}

fn capabilities(hwnd: HWND) -> InputCapabilities {
    input(hwnd).delivery.input_capabilities()
}

/// A viewport whose `Update` fails is released, what it had open is
/// cancelled, and the window stops claiming precise scrolling and trackpad
/// gestures it now reads as wheel ticks. `Update` survives a deactivated
/// manager and an abandoned viewport, so the viewport is told to fail its
/// next `Update`. Red-check: have `pump_touchpad` release the viewport
/// without republishing the capabilities.
#[test]
fn a_failed_viewport_withdraws_the_touchpad_capabilities() {
    let f = fixture(None);
    assert_eq!(capabilities(f.hwnd), input_availability::windows(true));
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    {
        let input = input(f.hwnd);
        let host = input.touchpad.host.borrow();
        let host = host.as_ref().expect("the window's viewport initialised");
        host.viewport.fail_update.set(true);
    }
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Began 8", "Cancelled 0"]);
    assert!(
        !input(f.hwnd).has_touchpad(),
        "the failed viewport is released"
    );
    assert_eq!(capabilities(f.hwnd), input_availability::windows(false));
    f.window.close_now();
}

/// Make the window's cursor unreadable, and forget where it was last read
/// when `forget` is set.
fn unreadable_cursor(hwnd: HWND, forget: bool) {
    let input = input(hwnd);
    let mut host = input.touchpad.host.borrow_mut();
    let host = host.as_mut().expect("the window's viewport initialised");
    host.cursor_unreadable = true;
    if forget {
        host.cursor = None;
    }
}

fn queued(hwnd: HWND) -> usize {
    let input = input(hwnd);
    let host = input.touchpad.host.borrow();
    let host = host.as_ref().expect("the window's viewport initialised");
    host.viewport.queue.borrow().len()
}

/// While the cursor cannot be read a manipulation is placed where it was
/// last read; one the cursor was never read for is dropped, with no timer
/// left pumping it and nothing kept queued. Red-check: re-queue the
/// readings and keep pumping when no frame can be made.
#[test]
fn an_unreadable_cursor_never_places_a_manipulation_at_the_origin() {
    let f = fixture(None);
    unreadable_cursor(f.hwnd, false);
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    assert_eq!(
        f.lines(),
        ["Began 8"],
        "placed where the cursor was last read"
    );
    push(f.hwnd, &[status(Ready)]);
    pump(f.hwnd);
    let _ = f.lines();
    let _ = take_timer(f.hwnd);

    unreadable_cursor(f.hwnd, true);
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    assert_eq!(f.lines(), Vec::<String>::new(), "nothing to place it at");
    assert!(!take_timer(f.hwnd), "nothing is left pumping");
    assert_eq!(queued(f.hwnd), 0, "nothing is left queued");
    f.window.close_now();
}

/// The viewport's event handler holds the window's `Rc` queue, so it does
/// not claim to be agile: another apartment reaches it through a proxy.
#[test]
fn the_viewport_handler_is_bound_to_its_apartment() {
    use windows::Win32::Graphics::DirectManipulation::IDirectManipulationViewportEventHandler;
    use windows::Win32::System::Com::IAgileObject;
    use windows_core::Interface as _;

    let handler: IDirectManipulationViewportEventHandler =
        super::handler::Handler::new(std::rc::Rc::default()).into();
    assert!(handler.cast::<IAgileObject>().is_err());
}
