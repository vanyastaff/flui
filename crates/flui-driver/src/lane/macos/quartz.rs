//! The Quartz calls the macOS lane makes: events posted to the app's process from a private,
//! tagged source, the window list, what the hardware holds, and the privacy permissions the
//! system grants the driver's process.
#![expect(
    unsafe_code,
    reason = "Quartz's C calls; each takes plain values or an object the call keeps alive"
)]

use std::ffi::{CStr, c_ulong};

use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventType,
    CGMouseButton, CGPreflightListenEventAccess, CGPreflightPostEventAccess,
    CGPreflightScreenCaptureAccess, CGRectMakeWithDictionaryRepresentation, CGScrollEventUnit,
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowAlpha,
    kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID,
};

use crate::error::{Error, Result};
use crate::lane::macos::activity::LANE_TAG;
use crate::lane::macos::held::{Keystroke, TEXT_KEY};
use crate::lane::macos::keys::{self, KeyCode, Modifier, SHIFT};
use crate::lane::macos::pointer::{MouseEvent, MouseKind, Point};
use crate::lane::macos::safety::{ButtonBits, LANE, Permission};
use crate::lane::macos::screen::{PointRect, WindowInfo};
use crate::lane::macos::version::MacosVersion;

/// A lane failure, for the lane's own errors.
pub(super) fn lane_error(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: message.into(),
    }
}

/// The release this process runs on; `None` when the system does not say (before 10.13.4, the
/// `kern.osproductversion` sysctl did not exist, which is below every gate the lane has).
pub(super) fn os_version() -> Option<MacosVersion> {
    let mut buffer = [0_u8; 32];
    let mut length = buffer.len();
    // SAFETY: a NUL-terminated name, a buffer of `length` bytes the call writes at most, and its
    // length, which the call updates; no new value.
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &raw mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    let text = CStr::from_bytes_until_nul(buffer.get(..length)?).ok()?;
    MacosVersion::parse(text.to_str().ok()?)
}

/// Whether the system grants this process `permission`, asked without a prompt: a refusal never
/// waits on the person.
pub(super) fn granted(permission: Permission) -> bool {
    if !os_version().is_some_and(MacosVersion::has_privacy_preflight) {
        // Before 10.15, posting events needed only the accessibility trust, and no permission
        // guarded listening or capture.
        return permission != Permission::Accessibility || super::ax::trusted();
    }
    match permission {
        Permission::Accessibility => CGPreflightPostEventAccess(),
        Permission::InputMonitoring => CGPreflightListenEventAccess(),
        Permission::ScreenRecording => CGPreflightScreenCaptureAccess(),
    }
}

/// A private event source carrying the lane's tag: what every event the lane posts comes from.
fn source() -> Result<CFRetained<CGEventSource>> {
    let source = CGEventSource::new(CGEventSourceStateID::Private)
        .ok_or_else(|| lane_error("Quartz made no private event source"))?;
    CGEventSource::set_user_data(Some(&*source), LANE_TAG);
    Ok(source)
}

fn made(event: Option<CFRetained<CGEvent>>, what: &str) -> Result<CFRetained<CGEvent>> {
    event.ok_or_else(|| lane_error(format!("Quartz made no event for {what}")))
}

/// Post `event` to the process `pid`.
fn post(pid: libc::pid_t, event: &CGEvent) {
    CGEvent::post_to_pid(pid, Some(event));
}

/// Post one keyboard event of `key`, pressed when `down`, with `flags`, carrying `text`.
fn key_event(
    pid: libc::pid_t,
    key: KeyCode,
    down: bool,
    flags: u64,
    text: Option<&[u16]>,
) -> Result<()> {
    let source = source()?;
    let event = made(
        CGEvent::new_keyboard_event(Some(&*source), key.0, down),
        "a key",
    )?;
    if Modifier::of(key).is_some() {
        // A modifier is a flags change, its state read from the flags it leaves.
        CGEvent::set_type(Some(&*event), CGEventType::FlagsChanged);
    }
    CGEvent::set_flags(Some(&*event), CGEventFlags(flags));
    if let Some(units) = text {
        let length = c_ulong::try_from(units.len())
            .map_err(|_| lane_error("a text run is longer than Quartz counts"))?;
        // SAFETY: `units` holds `length` UTF-16 units and outlives the call, which copies them.
        unsafe { CGEvent::keyboard_set_unicode_string(Some(&*event), length, units.as_ptr()) };
    }
    post(pid, &event);
    Ok(())
}

/// Post `stroke` to the process `pid`, carrying `flags`.
pub(super) fn keystroke(pid: libc::pid_t, stroke: Keystroke, flags: u64) -> Result<()> {
    let text = |character: Option<char>, units: &mut [u16; 2]| -> Option<usize> {
        character.map(|c| c.encode_utf16(units).len())
    };
    let mut units = [0_u16; 2];
    match stroke {
        Keystroke::Press(key, character) => {
            let length = text(character, &mut units);
            key_event(pid, key, true, flags, length.map(|n| &units[..n]))
        }
        Keystroke::Release(key, character) => {
            let length = text(character, &mut units);
            key_event(pid, key, false, flags, length.map(|n| &units[..n]))
        }
        Keystroke::ShiftFor(_) => key_event(pid, SHIFT, true, flags, None),
        Keystroke::UnshiftFor(_) => key_event(pid, SHIFT, false, flags, None),
        Keystroke::Text(chunk) => {
            key_event(pid, TEXT_KEY, true, flags, Some(chunk.units()))?;
            key_event(pid, TEXT_KEY, false, flags, Some(chunk.units()))
        }
    }
}

/// The event type and button of a mouse event of `kind`.
fn mouse_type(kind: MouseKind) -> (CGEventType, CGMouseButton) {
    let by_button = |button: u32, left, right, other| {
        let event = match button {
            0 => left,
            1 => right,
            _ => other,
        };
        (event, CGMouseButton(button))
    };
    match kind {
        MouseKind::Moved => (CGEventType::MouseMoved, CGMouseButton::Left),
        MouseKind::Dragged(button) => by_button(
            button.0,
            CGEventType::LeftMouseDragged,
            CGEventType::RightMouseDragged,
            CGEventType::OtherMouseDragged,
        ),
        MouseKind::Down(button) => by_button(
            button.0,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::OtherMouseDown,
        ),
        MouseKind::Up(button) => by_button(
            button.0,
            CGEventType::LeftMouseUp,
            CGEventType::RightMouseUp,
            CGEventType::OtherMouseUp,
        ),
    }
}

/// Post `event` to the process `pid`, carrying `flags`.
pub(super) fn mouse(pid: libc::pid_t, event: &MouseEvent, flags: u64) -> Result<()> {
    let source = source()?;
    let at = CGPoint::new(event.at.x, event.at.y);
    let (event_type, button) = mouse_type(event.kind);
    let cg = made(
        CGEvent::new_mouse_event(Some(&*source), event_type, at, button),
        "a mouse event",
    )?;
    if matches!(event.kind, MouseKind::Down(_) | MouseKind::Up(_)) {
        CGEvent::set_integer_value_field(Some(&*cg), CGEventField::MouseEventClickState, 1);
    }
    CGEvent::set_integer_value_field(
        Some(&*cg),
        CGEventField::MouseEventButtonNumber,
        i64::from(button.0),
    );
    CGEvent::set_flags(Some(&*cg), CGEventFlags(flags));
    post(pid, &cg);
    Ok(())
}

/// Post a turn of the wheels `vertical` and `horizontal` points at `at` to the process `pid`:
/// Quartz counts a pixel-unit scroll in points (`pointer::Wheel`).
pub(super) fn scroll(
    pid: libc::pid_t,
    at: Point,
    vertical: i32,
    horizontal: i32,
    flags: u64,
) -> Result<()> {
    let source = source()?;
    let event = made(
        CGEvent::new_scroll_wheel_event2(
            Some(&*source),
            CGScrollEventUnit::Pixel,
            2,
            vertical,
            horizontal,
            0,
        ),
        "a turn of the wheel",
    )?;
    CGEvent::set_location(Some(&*event), CGPoint::new(at.x, at.y));
    CGEvent::set_flags(Some(&*event), CGEventFlags(flags));
    post(pid, &event);
    Ok(())
}

/// The windows the window server lists, front to back: those on screen when `on_screen`, every
/// one otherwise.
pub(super) fn windows(on_screen: bool) -> Result<Vec<WindowInfo>> {
    let option = if on_screen {
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements
    } else {
        CGWindowListOption::OptionAll
    };
    let list = CGWindowListCopyWindowInfo(option, kCGNullWindowID)
        .ok_or_else(|| lane_error("the window server listed no windows"))?;
    // SAFETY: `CGWindowListCopyWindowInfo` returns an array of dictionaries keyed by strings
    // (CGWindow.h).
    let list =
        unsafe { CFRetained::cast_unchecked::<CFArray<CFDictionary<CFString, CFType>>>(list) };
    // SAFETY: the keys are constant strings Quartz exports.
    let (number_key, pid_key, layer_key, bounds_key, alpha_key, owner_key) = unsafe {
        (
            kCGWindowNumber,
            kCGWindowOwnerPID,
            kCGWindowLayer,
            kCGWindowBounds,
            kCGWindowAlpha,
            kCGWindowOwnerName,
        )
    };
    let number = |dictionary: &CFDictionary<CFString, CFType>, key: &CFString| {
        dictionary
            .get(key)
            .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
    };
    let mut found = Vec::new();
    for window in &*list {
        let window: &CFDictionary<CFString, CFType> = &window;
        let (Some(id), Some(pid), Some(level)) = (
            number(window, number_key).and_then(|n| u32::try_from(n).ok()),
            number(window, pid_key).and_then(|n| i32::try_from(n).ok()),
            number(window, layer_key).and_then(|n| i32::try_from(n).ok()),
        ) else {
            continue;
        };
        let alpha = window
            .get(alpha_key)
            .and_then(|value| value.downcast_ref::<CFNumber>().and_then(CFNumber::as_f64))
            .unwrap_or(1.0);
        let Some(bounds) = window.get(bounds_key).and_then(|value| {
            let dictionary = value.downcast_ref::<CFDictionary>()?;
            let mut rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, 0.0));
            // SAFETY: a bounds dictionary, as `kCGWindowBounds` holds, and a local rectangle
            // the call writes.
            unsafe { CGRectMakeWithDictionaryRepresentation(Some(dictionary), &raw mut rect) }
                .then_some(PointRect::new(
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ))
        }) else {
            continue;
        };
        let owner = window
            .get(owner_key)
            .and_then(|value| value.downcast_ref::<CFString>().map(ToString::to_string))
            .unwrap_or_else(|| format!("process {pid}"));
        found.push(WindowInfo::new(id, pid, level, bounds, alpha, owner));
    }
    Ok(found)
}

/// The mouse buttons, modifiers and other keys the hardware holds: someone's hands, since the
/// lane's events never reach this state.
pub(super) fn physical() -> (ButtonBits, Vec<Modifier>, Option<u16>) {
    let state = CGEventSourceStateID::HIDSystemState;
    let mut buttons = 0;
    for (bit, button) in [
        (1, CGMouseButton::Left),
        (2, CGMouseButton::Right),
        (4, CGMouseButton::Center),
    ] {
        if CGEventSource::button_state(state, button) {
            buttons |= bit;
        }
    }
    let flags = CGEventSource::flags_state(state).0;
    let modifiers = Modifier::ALL
        .into_iter()
        .filter(|modifier| flags & modifier.flag() != 0)
        .collect::<Vec<Modifier>>();
    let other_key = (0..0x80_u16)
        .find(|&code| keys::scanned_for_held(code) && CGEventSource::key_state(state, code));
    (buttons, modifiers, other_key)
}
