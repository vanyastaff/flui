//! Win32 message conversion to W3C ui-events (0.3 API).
//!
//! `window_proc` unpacks every mouse and keyboard message through these
//! functions:
//!
//! - **Modifiers** are the queue-synchronized key state (`GetKeyState`), which
//!   Win32 advances as each input message is retrieved — the state that held
//!   when this message was generated. `GetAsyncKeyState` would report the
//!   keyboard at processing time instead, so a queued Shift+click whose Shift
//!   was released before the click was processed would lose its Shift.
//! - **Capture** belongs to a press sequence: [`capture_on_press`] captures
//!   the mouse on a button press and [`release_capture_after`] releases it
//!   once no button stays held, so a drag released outside the client area
//!   still delivers its release (at client coordinates outside the client
//!   rectangle, sign-extended by `get_x_lparam`). A capture taken away while
//!   a button is held ends the sequence with a cancel
//!   ([`capture_changed_event`]).

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::{Modifiers as KeyboardModifiers, NamedKey};
use ui_events::{
    keyboard::{Code, KeyState, KeyboardEvent, Location},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerOrientation,
        PointerState, PointerUpdate,
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::{
        Input::KeyboardAndMouse::{
            GetCapture, GetKeyState, GetKeyboardLayout, GetKeyboardState, ReleaseCapture,
            SetCapture, ToUnicodeEx, VIRTUAL_KEY, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MBUTTON,
            VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::{
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN,
            WM_RBUTTONUP,
        },
    },
};

use super::util::{get_x_lparam, get_y_lparam};
use crate::{
    shared::events::{event_timestamp_ns, primary_mouse_info},
    shared::keys,
    traits::{Key, PlatformInput, device_to_logical},
};

// ============================================================================
// Message-time modifiers
// ============================================================================
//
// The translation decisions for keys (vk -> Key fallback, scancode -> Code,
// the WM_CHAR merge) are pure functions in `crate::shared::keys`, where their
// tests execute on every host; this file only unpacks the Win32 message
// parameters and feeds them through. The WM_CHAR pairing model is documented
// on that module.

/// Whether `key` was down when the message being processed was generated.
fn key_down_in_queue(key: VIRTUAL_KEY) -> bool {
    // SAFETY: `GetKeyState` takes a plain virtual-key code and reads only the
    // calling thread's queue-synchronized key state; no pointers are involved.
    unsafe { GetKeyState(i32::from(key.0)) < 0 }
}

/// The keyboard modifiers held when the message being processed was
/// generated (see the module doc for why this is not `GetAsyncKeyState`).
pub(super) fn message_modifiers() -> KeyboardModifiers {
    let mut mods = KeyboardModifiers::empty();
    for (key, modifier) in [
        (VK_SHIFT, KeyboardModifiers::SHIFT),
        (VK_CONTROL, KeyboardModifiers::CONTROL),
        (VK_MENU, KeyboardModifiers::ALT),
        (VK_LWIN, KeyboardModifiers::META),
        (VK_RWIN, KeyboardModifiers::META),
    ] {
        if key_down_in_queue(key) {
            mods |= modifier;
        }
    }
    mods
}

// ============================================================================
// Pointer Event Conversion (W3C ui-events 0.3 API)
// ============================================================================

/// The set of mouse buttons Win32 reports as held in a message's `WPARAM`.
///
/// Every mouse message (`WM_MOUSEMOVE`, `WM_*BUTTONDOWN`, `WM_*BUTTONUP`,
/// `WM_MOUSEWHEEL`) carries the `MK_*` mask in the low word of `WPARAM`, and
/// Win32 already applies the W3C rule for us: the bit for a button is set on
/// its DOWN message and clear on its UP message, i.e. the set held *after*
/// the event. That makes this stateless — unlike the winit backend, which has
/// to track the transition itself because winit does not surface a mask.
///
/// This matters beyond fidelity: the framework tells a drag-move from a hover
/// by asking whether any button is held. A move that always reports an empty
/// set is delivered as a hover, so no gesture recognizer ever sees it and the
/// drag is silently re-interpreted as a tap on release.
///
/// `MK_LBUTTON`/`MK_RBUTTON`/`MK_MBUTTON` are `0x0001`/`0x0002`/`0x0010`
/// (`winuser.h`); the X buttons have no `PointerButton` mapping here.
#[inline]
fn held_buttons(wparam: WPARAM) -> PointerButtons {
    const MK_LBUTTON: usize = 0x0001;
    const MK_RBUTTON: usize = 0x0002;
    const MK_MBUTTON: usize = 0x0010;

    let mask = wparam.0 & 0xffff;
    let mut buttons = PointerButtons::default();
    if mask & MK_LBUTTON != 0 {
        buttons.insert(PointerButton::Primary);
    }
    if mask & MK_RBUTTON != 0 {
        buttons.insert(PointerButton::Secondary);
    }
    if mask & MK_MBUTTON != 0 {
        buttons.insert(PointerButton::Auxiliary);
    }
    buttons
}

/// The button a button message reports and whether it is a press, or `None`
/// for any other message.
pub(super) fn button_message(msg: u32) -> Option<(PointerButton, bool)> {
    match msg {
        WM_LBUTTONDOWN => Some((PointerButton::Primary, true)),
        WM_LBUTTONUP => Some((PointerButton::Primary, false)),
        WM_RBUTTONDOWN => Some((PointerButton::Secondary, true)),
        WM_RBUTTONUP => Some((PointerButton::Secondary, false)),
        WM_MBUTTONDOWN => Some((PointerButton::Auxiliary, true)),
        WM_MBUTTONUP => Some((PointerButton::Auxiliary, false)),
        _ => None,
    }
}

/// Captures the mouse for `hwnd` on a button press, so the rest of the press
/// sequence — its moves and its release — reaches this window even when the
/// cursor leaves the client area. A press while the window already holds the
/// capture (a second button) keeps it.
pub(super) fn capture_on_press(hwnd: HWND) {
    // SAFETY: plain handle calls on the window's owner thread, where
    // `window_proc` runs; `SetCapture` captures for this thread's own window.
    unsafe {
        if GetCapture() != hwnd {
            SetCapture(hwnd);
        }
    }
}

/// Releases `hwnd`'s mouse capture after a button release that leaves no
/// button held (`wparam` is the release's `MK_*` mask, which no longer has
/// the released button). The release itself sends `WM_CAPTURECHANGED`, which
/// [`capture_changed_event`] recognizes as the sequence's own end.
pub(super) fn release_capture_after(hwnd: HWND, wparam: WPARAM) {
    if held_buttons(wparam) != PointerButtons::default() {
        return;
    }
    // SAFETY: plain calls on the window's owner thread; the capture is only
    // released while this window holds it, never another window's.
    unsafe {
        if GetCapture() == hwnd
            && let Err(error) = ReleaseCapture()
        {
            tracing::warn!(%error, "ReleaseCapture failed after the last button release");
        }
    }
}

/// The pointer cancel for a `WM_CAPTURECHANGED` that took the capture from a
/// press sequence still in progress, or `None` when the sequence ended on its
/// own.
///
/// `lparam` is the window gaining the capture. The sequence is in progress
/// while a button is still down in the queue-synchronized key state. After
/// the last release — the one that makes [`release_capture_after`] let go —
/// every button there is up. Another window, a modal loop or `WM_CANCELMODE`
/// taking the capture mid-drag leaves a button down, and without the cancel
/// this window would never see that sequence's release.
pub(super) fn capture_changed_event(hwnd: HWND, lparam: LPARAM) -> Option<PlatformInput> {
    let gaining = HWND(lparam.0 as *mut core::ffi::c_void);
    let held = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        .any(key_down_in_queue);
    (gaining != hwnd && held)
        .then(|| PlatformInput::Pointer(PointerEvent::Cancel(primary_mouse_info())))
}

/// Build a `PointerState` from LPARAM coordinates and scale factor.
///
/// `buttons` is the held set from [`held_buttons`]; it is a required argument
/// rather than a defaulted field so a new message arm cannot silently ship an
/// empty set.
#[inline]
fn pointer_state(
    lparam: LPARAM,
    scale_factor: f64,
    pressure: f64,
    buttons: PointerButtons,
    count: u8,
) -> PointerState {
    pointer_state_at(
        get_x_lparam(lparam),
        get_y_lparam(lparam),
        scale_factor,
        pressure,
        buttons,
        count,
    )
}

/// [`pointer_state`] with the CLIENT-space device coordinates already in
/// hand — for the wheel messages, whose `lParam` needs a screen-to-client
/// conversion first (see [`wheel_pointer_state`]).
///
/// `count` is the W3C click count — `1` on Down/Up transitions, `0`
/// elsewhere (the cross-wire contract in flui-interaction's module doc).
#[inline]
fn pointer_state_at(
    x: i32,
    y: i32,
    scale_factor: f64,
    pressure: f64,
    buttons: PointerButtons,
    count: u8,
) -> PointerState {
    let logical_x = device_to_logical(x as f64, scale_factor);
    let logical_y = device_to_logical(y as f64, scale_factor);

    PointerState {
        time: event_timestamp_ns(),
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons,
        modifiers: message_modifiers(),
        count,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: PointerOrientation::default(),
        pressure: pressure as f32,
        tangential_pressure: 0.0,
        scale_factor,
    }
}

/// Convert a `WM_[LRM]BUTTONDOWN`/`UP` (see [`button_message`]) to a W3C
/// pointer Down/Up.
pub fn mouse_button_event(
    button: PointerButton,
    is_down: bool,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
) -> PlatformInput {
    let state = pointer_state(
        lparam,
        scale_factor,
        if is_down { 0.5 } else { 0.0 },
        held_buttons(wparam),
        1,
    );
    let event = PointerButtonEvent {
        pointer: primary_mouse_info(),
        state,
        button: Some(button),
    };
    PlatformInput::Pointer(if is_down {
        PointerEvent::Down(event)
    } else {
        PointerEvent::Up(event)
    })
}

/// Convert WM_MOUSEMOVE to a W3C pointer Move.
pub fn mouse_move_event(wparam: WPARAM, lparam: LPARAM, scale_factor: f64) -> PlatformInput {
    let held = held_buttons(wparam);
    // Sensor-less pressure rule: 0.5 while any button is held (a drag),
    // 0.0 on a hover.
    let pressure = if held == PointerButtons::default() {
        0.0
    } else {
        0.5
    };
    PlatformInput::Pointer(PointerEvent::Move(PointerUpdate {
        pointer: primary_mouse_info(),
        current: pointer_state(lparam, scale_factor, pressure, held, 0),
        coalesced: Vec::new(),
        predicted: Vec::new(),
    }))
}

/// The signed scroll distance both wheel messages carry in the high word of
/// `wParam` (`GET_WHEEL_DELTA_WPARAM`), in multiples of `WHEEL_DELTA` (120).
fn wheel_distance(wparam: WPARAM) -> i16 {
    ((wparam.0 as i32) >> 16) as i16
}

/// Build the pointer state for a wheel message.
///
/// Unlike every other client-area mouse message, `WM_MOUSEWHEEL` and
/// `WM_MOUSEHWHEEL` deliver the cursor position in SCREEN coordinates
/// (both messages' `lParam` docs:
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>),
/// so the point is converted to client space here before the shared
/// [`pointer_state_at`] path DPI-scales it — otherwise scroll hit-testing
/// targets the wrong child whenever the window's client origin is not the
/// desktop origin.
fn wheel_pointer_state(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
) -> PointerState {
    let mut point = POINT {
        x: get_x_lparam(lparam),
        y: get_y_lparam(lparam),
    };
    // SAFETY: `point` is a live, writable local; `ScreenToClient` writes
    // nothing else. On failure (invalid `hwnd`) it returns FALSE and leaves
    // `point` unchanged.
    let converted = unsafe { ScreenToClient(hwnd, &raw mut point) };
    if !converted.as_bool() {
        tracing::warn!(
            "ScreenToClient failed for a wheel message; scroll position stays in screen space"
        );
    }
    pointer_state_at(point.x, point.y, scale_factor, 0.0, held_buttons(wparam), 0)
}

/// Convert WM_MOUSEWHEEL to a W3C pointer Scroll.
///
/// Win32's vertical sign (positive = wheel rotated away from the user) is the
/// inverse of the cross-backend convention — positive = content scrolls down —
/// so `from_win32_wheel` negates it at this boundary; see
/// `crate::shared::scroll` for the sign/unit table and citations. The cursor
/// position arrives in screen coordinates; see [`wheel_pointer_state`].
pub fn mouse_wheel_event(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
) -> PlatformInput {
    PlatformInput::Pointer(PointerEvent::Scroll(
        ui_events::pointer::PointerScrollEvent {
            pointer: primary_mouse_info(),
            state: wheel_pointer_state(hwnd, wparam, lparam, scale_factor),
            delta: crate::shared::scroll::from_win32_wheel(wheel_distance(wparam)),
        },
    ))
}

/// Convert WM_MOUSEHWHEEL to a W3C pointer Scroll.
///
/// Win32's horizontal sign (positive = wheel tilted right) already matches
/// the cross-backend convention — positive = content scrolls right — so only
/// the `WHEEL_DELTA` division applies; see `crate::shared::scroll`. The
/// cursor position arrives in screen coordinates; see
/// [`wheel_pointer_state`].
pub fn mouse_hwheel_event(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
) -> PlatformInput {
    PlatformInput::Pointer(PointerEvent::Scroll(
        ui_events::pointer::PointerScrollEvent {
            pointer: primary_mouse_info(),
            state: wheel_pointer_state(hwnd, wparam, lparam, scale_factor),
            delta: crate::shared::scroll::from_win32_hwheel(wheel_distance(wparam)),
        },
    ))
}

// ============================================================================
// Keyboard events (simple wrappers)
// ============================================================================

/// Convert WM_KEYDOWN to W3C KeyboardEvent.
///
/// `translated_text` is the drained `WM_CHAR` burst for this keydown (see
/// `window_proc`'s `WM_KEYDOWN` arm and `crate::shared::keys`'s module doc
/// for the pairing model); when present and typeable it becomes the event's
/// `Key::Character`, otherwise the layout-independent virtual-key fallback
/// applies.
pub fn key_down_event(
    wparam: WPARAM,
    lparam: LPARAM,
    translated_text: Option<String>,
    held_dead: &mut HeldDeadKeys,
) -> PlatformInput {
    let vk = wparam.0 as u16;
    let (scan_code, extended, is_repeat) = keys::parse_key_lparam(lparam.0);
    let dead = translated_text.is_none() && is_dead_keystroke(vk, scan_code);
    if dead {
        held_dead.press(scan_code, extended);
    }

    let modifiers = message_modifiers();
    let fallback = keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT));
    let code = keys::scancode_to_code(scan_code, extended);
    // A dead key with nothing typed yet is reported as one, not as its
    // unshifted fallback character: otherwise a shortcut bound to that
    // character fires while the user composes an accented letter. Its second
    // press (or a space after it) types the accent itself and arrives as text.
    let key = if dead {
        Key::Named(NamedKey::Dead)
    } else {
        keys::key_for_keydown(
            fallback,
            translated_text,
            modifiers.contains(KeyboardModifiers::ALT),
            modifiers.contains(KeyboardModifiers::CONTROL),
            code,
        )
    };

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key,
        code,
        location: keys::location_for_code(code),
        modifiers,
        repeat: is_repeat,
        is_composing: false,
    })
}

/// Convert WM_KEYUP to W3C KeyboardEvent.
///
/// No `WM_CHAR` pairs with a keyup, so the key is always the virtual-key
/// fallback — for a letter that is its shift-respecting character, for OEM
/// punctuation the US-layout position. Consumers that type text act on
/// `KeyState::Down` only, so this asymmetry never reaches a text field.
pub fn key_up_event(wparam: WPARAM, lparam: LPARAM, held_dead: &mut HeldDeadKeys) -> PlatformInput {
    let vk = wparam.0 as u16;
    let (scan_code, extended, _) = keys::parse_key_lparam(lparam.0);

    let modifiers = message_modifiers();
    // The release of a dead key is `Dead` too (W3C UI Events dead-key
    // sequence), so a pressed-key set sees the same identity go down and up.
    // The press decided it; the layout may have changed while the key was held.
    let key = if held_dead.release(scan_code, extended) {
        Key::Named(NamedKey::Dead)
    } else {
        keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT))
    };
    let code = keys::scancode_to_code(scan_code, extended);

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Up,
        key,
        code,
        location: keys::location_for_code(code),
        modifiers,
        repeat: false,
        is_composing: false,
    })
}

/// Build the KeyboardEvent for an out-of-band `WM_CHAR` — one that reached
/// `window_proc` instead of being drained by a `WM_KEYDOWN` (Alt+numpad
/// composition, or a directly-sent message). There is no owning physical
/// key, so it is dispatched as a key-down with `Code::Unidentified` and no
/// paired key-up; the text lane consumes `Key::Character` on key-down only.
pub fn stray_char_event(text: String) -> PlatformInput {
    let modifiers = message_modifiers();

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key: Key::Character(text),
        code: Code::Unidentified,
        location: Location::Standard,
        modifiers,
        repeat: false,
        is_composing: false,
    })
}

/// The physical keys currently held down that were pressed as dead keys, so
/// each one's release reports `Dead` like its press, even if the keyboard
/// layout or the modifiers changed while it was held. Owned per window.
#[derive(Debug, Default)]
pub struct HeldDeadKeys(Vec<(u16, bool)>);

impl HeldDeadKeys {
    fn press(&mut self, scan_code: u16, extended: bool) {
        if !self.0.contains(&(scan_code, extended)) {
            self.0.push((scan_code, extended));
        }
    }

    /// Whether the released key was pressed as a dead key; forgets it.
    fn release(&mut self, scan_code: u16, extended: bool) -> bool {
        let before = self.0.len();
        self.0.retain(|held| *held != (scan_code, extended));
        self.0.len() != before
    }
}

/// Whether this keystroke, with the modifiers held now (AltGr, Shift), is a
/// dead key in the calling thread's active layout.
///
/// `ToUnicodeEx` answers `-1` for a dead key on the layer the current keyboard
/// state selects, so a dead key that exists only under AltGr or Shift (French
/// AltGr+2) is found too. Flag `0x4` leaves the kernel's dead-key state
/// untouched, so the composition `TranslateMessage` already started is not
/// disturbed. No queued message is consulted.
fn is_dead_keystroke(vk: u16, scan_code: u16) -> bool {
    let mut state = [0u8; 256];
    let mut buffer = [0u16; 8];
    // SAFETY: `state` and `buffer` are live, writable locals of the sizes the
    // calls require; the layout handle is the calling thread's own.
    unsafe {
        if GetKeyboardState(&mut state).is_err() {
            return false;
        }
        ToUnicodeEx(
            u32::from(vk),
            u32::from(scan_code),
            &state,
            &mut buffer,
            0x4,
            Some(GetKeyboardLayout(0)),
        ) == -1
    }
}
