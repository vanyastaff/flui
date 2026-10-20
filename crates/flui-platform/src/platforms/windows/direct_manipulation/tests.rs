//! The window half of the precision touchpad, on a real window and a real
//! DirectManipulation viewport. No precision touchpad drives it: each case
//! appends the readings a manipulation reports to the viewport's queue (the
//! same queue its event handler fills during `Update`), then drives the
//! window's own messages and pump.

use std::sync::{Arc, Mutex};

use flui_foundation::geometry::Size;
use flui_platform_api::input_capabilities::InputCapabilities;
use flui_platform_api::pointer::PointerEvent;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{GetUpdateRect, ValidateRect};
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, SendMessageW, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_KILLFOCUS, WM_PAINT,
};

use super::PUMP_TIMER;
use crate::platforms::windows::platform::{WindowContext, with_window_context};
use crate::platforms::windows::{WindowsPlatform, WindowsWindow};
use crate::shared::direct_manipulation::{ContentTransform, ViewportReading, ViewportStatus};
use crate::shared::input_availability;
use crate::traits::{DispatchEventResult, HostWindow, Platform as _, PlatformInput, WindowOptions};

use ViewportStatus::{Ready, Running};

type Log = Arc<Mutex<Vec<String>>>;

struct Fixture {
    // Field order: the window closes before its platform goes.
    window: Arc<dyn HostWindow>,
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
    with_window_context(hwnd, "test", |context| context.scale_factor.get()).expect("a context")
}

fn push(hwnd: HWND, readings: &[ViewportReading]) {
    with_window_context(hwnd, "test", |context| {
        let host = context.touchpad.host.borrow();
        let host = host.as_ref().expect("the window's viewport initialised");
        host.viewport.queue.borrow_mut().extend_from_slice(readings);
    })
    .expect("a context");
}

fn pump(hwnd: HWND) {
    with_window_context(hwnd, "test", |context| context.pump_touchpad(hwnd)).expect("a context");
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
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui touchpad")
                .with_size(Size::new(200.0, 120.0))
                // Shown: a hidden window keeps no update region.
                .with_visible(true),
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
    let mut nested = nested;
    let raw = hwnd.0 as isize;
    window.on_input(Box::new(move |input| {
        if let Some(line) = describe(&input, scale) {
            inputs.lock().expect("log").push(line);
            if let Some(nested) = nested.take() {
                nested(HWND(raw as *mut _));
            }
        }
        DispatchEventResult::default()
    }));
    let frames = Arc::clone(&log);
    window.on_request_frame(Box::new(move || {
        frames.lock().expect("log").push("frame".to_owned());
    }));
    // What the viewport reported while it was built is not a manipulation.
    pump(hwnd);
    let _ = take_timer(hwnd);
    log.lock().expect("log").clear();
    Fixture {
        window,
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
        f.window.close();
    }
}

/// Red-check: drop `release_touchpad` from `WM_DESTROY` (the open pan is
/// never cancelled).
#[test]
fn destroying_the_window_cancels_what_is_open_before_it_closes() {
    let f = fixture(None);
    push(f.hwnd, &[status(Running), content(-8.0)]);
    pump(f.hwnd);
    let closes = Arc::clone(&f.log);
    f.window.on_close(Box::new(move || {
        closes.lock().expect("log").push("close".to_owned());
    }));
    f.window.close();
    assert_eq!(f.lines(), ["Began 8", "Cancelled 0", "close"]);
}

fn capabilities(hwnd: HWND) -> InputCapabilities {
    with_window_context(hwnd, "test", |context| {
        context.callbacks.input_capabilities()
    })
    .expect("a context")
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
    with_window_context(f.hwnd, "test", |context| {
        let host = context.touchpad.host.borrow();
        let host = host.as_ref().expect("the window's viewport initialised");
        host.viewport.fail_update.set(true);
    })
    .expect("a context");
    pump(f.hwnd);
    assert_eq!(f.lines(), ["Began 8", "Cancelled 0"]);
    assert!(
        !with_window_context(f.hwnd, "test", WindowContext::has_touchpad).expect("a context"),
        "the failed viewport is released"
    );
    assert_eq!(capabilities(f.hwnd), input_availability::windows(false));
    f.window.close();
}

/// Make the window's cursor unreadable, and forget where it was last read
/// when `forget` is set.
fn unreadable_cursor(hwnd: HWND, forget: bool) {
    with_window_context(hwnd, "test", |context| {
        let mut host = context.touchpad.host.borrow_mut();
        let host = host.as_mut().expect("the window's viewport initialised");
        host.cursor_unreadable = true;
        if forget {
            host.cursor = None;
        }
    })
    .expect("a context");
}

fn queued(hwnd: HWND) -> usize {
    with_window_context(hwnd, "test", |context| {
        let host = context.touchpad.host.borrow();
        let host = host.as_ref().expect("the window's viewport initialised");
        host.viewport.queue.borrow().len()
    })
    .expect("a context")
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
    f.window.close();
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
