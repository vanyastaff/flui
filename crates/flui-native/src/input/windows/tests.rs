//! The Win32 input's per-window state, on real windows of FLUI's class: kept
//! by the input's hook, keyed by the window's identity, from the window's
//! announcement to its destruction, and read by the window's other features
//! through `WindowInputs`.

use flui_testing::window_commands::AdmitAndApply as _;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;

use flui_foundation::geometry::Size;
use flui_platform::WindowOptions;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_MOUSEMOVE};

use super::{InputWindows, testing};
use crate::WindowsPlatform;
use crate::host_window::HostWindow;
use crate::owner::windows::{
    WindowIdentity, WindowInputs, WindowMessages, WindowRef, WindowStep, window_inputs,
};
use crate::platform::Platform as _;
use crate::window::windows::WindowsWindow;

fn open(platform: &WindowsPlatform, title: &str) -> Rc<dyn HostWindow> {
    platform
        .open_window(
            WindowOptions::default()
                .with_title(title)
                .with_size(Size::new(160.0, 120.0)),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("window")
}

fn window_ref(window: &Rc<dyn HostWindow>) -> WindowRef {
    let window = window
        .as_any()
        .downcast_ref::<WindowsWindow>()
        .expect("Win32 window");
    WindowRef::new(window.hwnd(), window.identity)
}

/// `(10, 10)` in a client area, as a mouse message's `LPARAM` packs it.
const CLIENT_POINT: LPARAM = LPARAM(0x000A_000A);

/// A mouse move to `(10, 10)` in `hwnd`'s client area, as Win32 delivers it.
fn move_mouse(hwnd: HWND) {
    // SAFETY: a message to a live window of this thread, with plain values.
    unsafe { SendMessageW(hwnd, WM_MOUSEMOVE, Some(WPARAM(0)), Some(CLIENT_POINT)) };
}

/// Two windows keep their own input: a mouse that comes over one leaves the
/// other's hover as it was, and leaving the first changes only the first's.
#[test]
fn two_windows_keep_their_own_input() {
    let platform = WindowsPlatform::try_new().expect("platform");
    let (first, second) = (
        open(&platform, "flui input a"),
        open(&platform, "flui input b"),
    );
    let (first, second) = (window_ref(&first), window_ref(&second));
    let inputs = window_inputs().expect("the class's input on this thread");

    move_mouse(first.hwnd());
    assert_eq!(inputs.hovered(first), Some(true));
    assert_eq!(inputs.hovered(second), Some(false));

    // SAFETY: as in `move_mouse`.
    unsafe { SendMessageW(first.hwnd(), WM_MOUSELEAVE, None, None) };
    move_mouse(second.hwnd());
    assert_eq!(inputs.hovered(first), Some(false));
    assert_eq!(inputs.hovered(second), Some(true));
}

/// A window's input goes with the window: once it is destroyed, nothing reads
/// it, and the state itself is dropped. Red-check: keep the entry in
/// `InputWindows::forget`.
#[test]
fn a_windows_input_is_gone_once_it_is_destroyed() {
    let platform = WindowsPlatform::try_new().expect("platform");
    let window = open(&platform, "flui input gone");
    let this = window_ref(&window);
    let inputs = window_inputs().expect("the class's input on this thread");
    assert_eq!(inputs.hovered(this), Some(false), "kept from its creation");
    let kept =
        std::rc::Rc::downgrade(&testing::window_input(this.hwnd()).expect("the window's input"));

    window.close_now();
    assert_eq!(inputs.hovered(this), None);
    assert!(kept.upgrade().is_none(), "the window's input is dropped");
}

/// An input that was never told of a window holds nothing for it: a read
/// answers `None`, and a message, a step or its destruction finds no input
/// and does nothing, without a panic. A reference whose identity is not the
/// window's reads nothing either.
#[test]
fn a_window_the_input_never_saw_has_no_input() {
    let platform = WindowsPlatform::try_new().expect("platform");
    let window = open(&platform, "flui input stranger");
    let this = window_ref(&window);
    let stranger = InputWindows::new();

    assert_eq!(stranger.hovered(this), None);
    assert_eq!(stranger.pen_in_range(this, None), None);
    // SAFETY: the arguments a window of this thread receives for the message.
    let answered = unsafe { stranger.message(this.hwnd(), WM_MOUSEMOVE, WPARAM(0), CLIENT_POINT) };
    assert_eq!(
        answered,
        Some(LRESULT(0)),
        "answered, with nothing to deliver"
    );
    stranger.step(this, WindowStep::FocusLost);
    stranger.destroyed(this);

    let (other, ()) = WindowIdentity::admit(&AtomicU64::new(u64::MAX - 1), || ());
    let inputs = window_inputs().expect("the class's input on this thread");
    assert_eq!(
        inputs.hovered(WindowRef::new(this.hwnd(), other)),
        None,
        "a reference to another window behind the handle reads nothing"
    );
    assert_eq!(inputs.hovered(this), Some(false));
}
