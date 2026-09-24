//! `windows-input`: the generated counter driven by real OS input on a real
//! window — the pointer and the keyboard a user has, not the accessibility
//! channel `windows-a11y` uses.
//!
//! The probe is the same `a11y_probe`; UI Automation is only the eyes. It
//! locates the button's rectangle and reads the count, while every action is
//! a `SendInput` event entering the system input queue exactly as a device's
//! would, hit-tested and routed by the OS into the backend's window
//! procedure:
//!
//! 1. a click on the window outside the button leaves the count at "0" — the
//!    control that makes the next step a hit rather than any click;
//! 2. a click at the button's centre makes it "1";
//! 3. Tab, then Enter, makes it "2": focus traversal reaches the button and
//!    the keyboard activates it.
//!
//! Before every event the probe's window must be the foreground window, so
//! no keystroke can land in another application; if Windows refuses to bring
//! it forward the check cannot verify and exits 2. The cursor is put back
//! where it was.
//!
//! Exit 0 on PASS, 1 on FAIL (with the tree dumped), 2 when this host cannot
//! take the measurement.

use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_VSC, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, MapVirtualKeyW,
    SendInput, VIRTUAL_KEY, VK_RETURN, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, SetCursorPos, SetForegroundWindow,
};

use super::uia::{self, BUTTON, Session, Start, TEXT};
use super::windows_a11y::{ADVANCE_WITHIN, INCREMENT};

/// How long a click that must change nothing is watched for.
const UNCHANGED_FOR: Duration = Duration::from_secs(1);
/// Between a key's press and release, and between events, so the backend
/// sees a key held for a frame rather than a zero-length tap.
const HOLD: Duration = Duration::from_millis(40);

/// Runs the check against the built probe and returns its exit code.
pub(super) fn run(probe: &Path) -> anyhow::Result<u8> {
    // UIA reports physical pixels and `SetCursorPos` takes them only when
    // this process is per-monitor aware; unaware, both are scaled apart on
    // any display that is not at 100 %. Process-local, not a system setting.
    // SAFETY: called before this process creates any window.
    if let Err(error) =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
    {
        println!("CANNOT_VERIFY: per-monitor DPI awareness was refused: {error}");
        return Ok(2);
    }
    let mut session = match Session::start(probe) {
        Ok(session) => session,
        Err(Start::CannotVerify(why)) => {
            println!("CANNOT_VERIFY: {why}");
            return Ok(2);
        }
        Err(Start::Failed(error)) => return Err(error),
    };
    let cursor = Cursor::save();
    let verdict = drive(&mut session);
    drop(cursor);
    drop(session);
    let verdict = verdict?;
    let line = match verdict {
        Verdict::Pass => "PASS",
        Verdict::Fail => "FAIL",
        Verdict::CannotVerify => "CANNOT_VERIFY",
    };
    println!("INPUT={line}");
    Ok(match verdict {
        Verdict::Pass => 0,
        Verdict::Fail => 1,
        Verdict::CannotVerify => 2,
    })
}

enum Verdict {
    Pass,
    Fail,
    CannotVerify,
}

fn drive(session: &mut Session) -> anyhow::Result<Verdict> {
    let Some(window) = session.window_with_button(INCREMENT)? else {
        return Ok(Verdict::Fail);
    };
    let before = session.walk(&window);
    uia::dump(&before);
    if !uia::has(&before, TEXT, "0") {
        println!("FAIL: the count is not \"0\" before any input");
        return Ok(Verdict::Fail);
    }
    let button = uia::bounds(
        &uia::find(&before, BUTTON, INCREMENT)
            .expect("BUG: window_with_button returns only a window whose tree holds the button")
            .element,
    )?;
    let frame = uia::bounds(&window)?;
    // SAFETY: a plain client call on the session's COM thread.
    let hwnd = unsafe { window.CurrentNativeWindowHandle() }?;

    let Some(()) = foreground(hwnd) else {
        return Ok(Verdict::CannotVerify);
    };

    // 1. The control: inside the window, clear of the button.
    let miss = POINT {
        x: frame.left + (frame.right - frame.left) / 2,
        y: button.bottom + (frame.bottom - button.bottom) / 2,
    };
    println!("click outside the button at ({}, {})", miss.x, miss.y);
    if !click(hwnd, miss)? {
        return Ok(Verdict::CannotVerify);
    }
    std::thread::sleep(UNCHANGED_FOR);
    if !uia::has(&session.walk(&window), TEXT, "0") {
        println!("FAIL: a click that missed the button changed the count");
        uia::dump(&session.walk(&window));
        return Ok(Verdict::Fail);
    }

    // 2. The hit.
    let hit = centre(button);
    println!("click the button at ({}, {})", hit.x, hit.y);
    if !click(hwnd, hit)? {
        return Ok(Verdict::CannotVerify);
    }
    if session
        .wait_for_text(&window, "1", ADVANCE_WITHIN)?
        .is_none()
    {
        println!("FAIL: a click on the button did not press it");
        return Ok(Verdict::Fail);
    }
    println!("the click pressed the button: count 1");

    // 3. The keyboard: Tab must bring the focus to the button, as a screen
    // reader sees it, and Enter must then activate it.
    println!("press Tab");
    if !press(hwnd, VK_TAB)? {
        return Ok(Verdict::CannotVerify);
    }
    let focused = std::iter::repeat_with(|| {
        let nodes = session.walk(&window);
        let focused = uia::find(&nodes, BUTTON, INCREMENT)
            .is_some_and(|button| uia::has_keyboard_focus(&button.element));
        if !focused {
            std::thread::sleep(HOLD);
        }
        focused
    })
    .take(25)
    .any(|focused| focused);
    if !focused {
        println!("FAIL: Tab did not give the button the keyboard focus");
        uia::dump(&session.walk(&window));
        return Ok(Verdict::Fail);
    }
    println!("Tab focused the button; press Enter");
    if !press(hwnd, VK_RETURN)? {
        return Ok(Verdict::CannotVerify);
    }
    if session
        .wait_for_text(&window, "2", ADVANCE_WITHIN)?
        .is_none()
    {
        println!("FAIL: Tab then Enter did not activate the button");
        return Ok(Verdict::Fail);
    }
    println!("Tab then Enter activated the button: count 2");
    Ok(Verdict::Pass)
}

fn centre(rect: RECT) -> POINT {
    POINT {
        x: rect.left + (rect.right - rect.left) / 2,
        y: rect.top + (rect.bottom - rect.top) / 2,
    }
}

/// Brings `hwnd` forward, or prints why the host refused.
fn foreground(hwnd: HWND) -> Option<()> {
    // SAFETY: `hwnd` is the probe's live top-level window; both calls only
    // read or request foreground state.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        std::thread::sleep(HOLD);
        if GetForegroundWindow() == hwnd {
            return Some(());
        }
    }
    println!(
        "CANNOT_VERIFY: Windows would not bring the probe's window to the foreground (the foreground lock), so input sent now could reach another application"
    );
    None
}

/// Whether `hwnd` is still the foreground window; printed when not.
fn still_foreground(hwnd: HWND) -> bool {
    // SAFETY: reads foreground state only.
    let current = unsafe { GetForegroundWindow() };
    if current != hwnd {
        println!(
            "CANNOT_VERIFY: another window took the foreground during the check; no further input was sent"
        );
    }
    current == hwnd
}

/// A left click at `point`, in physical screen pixels. `false` when the
/// probe lost the foreground and no press was sent.
///
/// The foreground is checked again immediately before the press, after the
/// cursor move's settle, since a window can take it in between. A press that
/// went out is always released, even if the foreground moved meanwhile: a
/// release carries no action of its own, and withholding it would leave the
/// button held for whatever the user does next.
fn click(hwnd: HWND, point: POINT) -> anyhow::Result<bool> {
    if !still_foreground(hwnd) {
        return Ok(false);
    }
    // SAFETY: moves the cursor; `point` is on the probe's window.
    unsafe { SetCursorPos(point.x, point.y) }?;
    std::thread::sleep(HOLD);
    if !still_foreground(hwnd) {
        return Ok(false);
    }
    send(&[mouse(MOUSEEVENTF_LEFTDOWN)])?;
    std::thread::sleep(HOLD);
    send(&[mouse(MOUSEEVENTF_LEFTUP)])?;
    std::thread::sleep(HOLD);
    Ok(true)
}

/// A press and release of `key`. `false` when the probe lost the foreground
/// and no press was sent; a press that went out is always released, as in
/// [`click`].
fn press(hwnd: HWND, key: VIRTUAL_KEY) -> anyhow::Result<bool> {
    if !still_foreground(hwnd) {
        return Ok(false);
    }
    send(&[keyboard(key, KEYBD_EVENT_FLAGS(0))])?;
    std::thread::sleep(HOLD);
    send(&[keyboard(key, KEYEVENTF_KEYUP)])?;
    std::thread::sleep(HOLD);
    Ok(true)
}

fn mouse(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..MOUSEINPUT::default()
            },
        },
    }
}

/// A key event carrying both the virtual key and its scan code, as a real
/// keyboard's does.
fn keyboard(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    // SAFETY: a pure table lookup.
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: u16::try_from(scan).unwrap_or_default(),
                dwFlags: flags,
                ..KEYBDINPUT::default()
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> anyhow::Result<()> {
    let size = i32::try_from(size_of::<INPUT>()).expect("BUG: INPUT is a few dozen bytes");
    // SAFETY: `inputs` is a valid slice of initialised `INPUT`s and `size`
    // is the size of one, as the API requires.
    let sent = unsafe { SendInput(inputs, size) };
    anyhow::ensure!(
        sent as usize == inputs.len(),
        "SendInput injected {sent} of {} events (blocked by UIPI or another desktop)",
        inputs.len()
    );
    Ok(())
}

/// The user's cursor position, restored when dropped.
struct Cursor(Option<POINT>);

impl Cursor {
    fn save() -> Self {
        let mut point = POINT::default();
        // SAFETY: `point` is a valid out-parameter.
        Self(unsafe { GetCursorPos(&raw mut point) }.ok().map(|()| point))
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        if let Some(point) = self.0 {
            // SAFETY: moves the cursor back to where the user left it.
            let _ = unsafe { SetCursorPos(point.x, point.y) };
        }
    }
}
