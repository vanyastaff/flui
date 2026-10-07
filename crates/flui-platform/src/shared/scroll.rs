//! Cross-backend wheel/scroll delta normalization.
//!
//! Every backend hands its raw platform wheel data to one of these helpers at
//! its translation boundary, so the shared consumers downstream
//! (`flui_platform_api::pointer::ScrollDelta` states the unit and sign
//! contract) always receive the same convention:
//!
//! - **Sign**: positive = content scrolls down / right (the scroll offset
//!   increases), exactly the W3C `WheelEvent.deltaX/deltaY` convention
//!   (<https://w3c.github.io/uievents/#events-wheelevents>).
//! - **Units**: `PixelDelta` is LOGICAL pixels (scale-factor independent);
//!   `LineDelta` is unit-less lines, converted by the consumer (not here) at
//!   its own line height; `PageDelta` likewise stays in pages.
//!
//! This module is deliberately free of `cfg` gates and platform types so the
//! per-backend sign/unit decisions compile — and their tests execute — on any
//! host, even though the Win32/AppKit/web callers themselves only compile on
//! their own targets.

use dpi::PhysicalPosition;
use ui_events::ScrollDelta;

/// The Win32 wheel detent, from `winuser.h` (`WHEEL_DELTA`): one notch of a
/// conventional wheel reports ±120 so finer-resolution wheels can report
/// fractions of a notch.
const WIN32_WHEEL_DELTA: f32 = 120.0;

/// Normalize a winit `MouseScrollDelta::LineDelta`.
///
/// winit documents positive values as "the content that is being scrolled
/// should move right and down (revealing more content left and up)" — i.e.
/// the scroll offset *decreases*: the inverse of this contract on both axes,
/// so both are negated.
pub fn from_winit_lines(x: f32, y: f32) -> ScrollDelta {
    ScrollDelta::LineDelta(-x, -y)
}

/// Normalize a winit `MouseScrollDelta::PixelDelta`.
///
/// Same sign convention (and therefore the same negation) as
/// [`from_winit_lines`], and winit's pixel deltas are PHYSICAL pixels while
/// this contract — like pointer positions — is logical, so both axes are
/// divided by the window scale factor. Doing either transform in a
/// platform-neutral widget instead would double-apply it under winit.
pub fn from_winit_pixels(x: f64, y: f64, scale_factor: f64) -> ScrollDelta {
    ScrollDelta::PixelDelta(PhysicalPosition::new(-x / scale_factor, -y / scale_factor))
}

/// Normalize a Win32 `WM_MOUSEWHEEL` distance (the signed high word of
/// `wParam`).
///
/// Per the `WM_MOUSEWHEEL` docs, "a positive value indicates that the wheel
/// was rotated forward, away from the user" — which scrolls toward *earlier*
/// content, the inverse of this contract's down-positive y — so the value is
/// negated as well as divided by `WHEEL_DELTA`.
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>
pub fn from_win32_wheel(raw_distance: i16) -> ScrollDelta {
    ScrollDelta::LineDelta(0.0, -(f32::from(raw_distance)) / WIN32_WHEEL_DELTA)
}

/// Normalize a Win32 `WM_MOUSEHWHEEL` distance (the signed high word of
/// `wParam`).
///
/// Per the `WM_MOUSEHWHEEL` docs, "a positive value indicates that the wheel
/// was rotated to the right" — which scrolls content right, already this
/// contract's right-positive x (and W3C `deltaX`) — so unlike the vertical
/// wheel the sign passes through; only the `WHEEL_DELTA` division applies.
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousehwheel>
pub fn from_win32_hwheel(raw_distance: i16) -> ScrollDelta {
    ScrollDelta::LineDelta(f32::from(raw_distance) / WIN32_WHEEL_DELTA, 0.0)
}

/// Normalize an AppKit `NSEvent` scroll (`scrollingDeltaX/Y` +
/// `hasPreciseScrollingDeltas`).
///
/// AppKit's scrolling deltas keep the legacy `deltaX/deltaY` sign, which is
/// positive for a swipe/scroll *up* (content moves down, revealing earlier
/// content) — the system applies the user's natural-scrolling preference
/// before delivery (`isDirectionInvertedFromDevice` reports the flip), so
/// that semantic is stable either way. That is the inverse of this contract
/// on both axes, so both are negated.
/// <https://developer.apple.com/documentation/appkit/nsevent/scrollingdeltay>
///
/// Precise deltas (trackpads) are in points, and macOS points ARE logical
/// pixels, so no scale-factor conversion applies; non-precise wheels report
/// whole lines.
pub fn from_appkit(delta_x: f64, delta_y: f64, has_precise_deltas: bool) -> ScrollDelta {
    if has_precise_deltas {
        ScrollDelta::PixelDelta(PhysicalPosition::new(-delta_x, -delta_y))
    } else {
        ScrollDelta::LineDelta(-delta_x as f32, -delta_y as f32)
    }
}

/// Normalize a W3C DOM `WheelEvent` (`deltaMode` + `deltaX/deltaY`).
///
/// The DOM already speaks this contract's sign convention — this contract
/// *is* the W3C one — and `DOM_DELTA_PIXEL` deltas are CSS pixels, which are
/// logical pixels, so every mode passes through unscaled and unflipped.
/// `DOM_DELTA_PAGE` stays in pages: the shared consumer converts pages to
/// pixels itself, next to its line height.
/// <https://w3c.github.io/uievents/#events-wheelevents>
pub fn from_web(delta_mode: u32, delta_x: f64, delta_y: f64) -> ScrollDelta {
    match delta_mode {
        // DOM_DELTA_LINE = 1
        1 => ScrollDelta::LineDelta(delta_x as f32, delta_y as f32),
        // DOM_DELTA_PAGE = 2
        2 => ScrollDelta::PageDelta(delta_x as f32, delta_y as f32),
        // DOM_DELTA_PIXEL = 0, and anything unknown, is pixels
        _ => ScrollDelta::PixelDelta(PhysicalPosition::new(delta_x, delta_y)),
    }
}
