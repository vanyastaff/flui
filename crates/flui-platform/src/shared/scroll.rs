//! Cross-backend wheel/scroll delta normalization.
//!
//! Every backend hands its raw platform wheel data to one of these helpers at
//! its translation boundary, so the shared consumers downstream
//! (`flui_interaction::events::ScrollEventData::delta_to_offset` states the
//! full contract) always receive the same convention:
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(delta: ScrollDelta) -> (f32, f32) {
        match delta {
            ScrollDelta::LineDelta(x, y) => (x, y),
            other => panic!("expected LineDelta, got {other:?}"),
        }
    }

    fn pixels(delta: ScrollDelta) -> (f64, f64) {
        match delta {
            ScrollDelta::PixelDelta(pos) => (pos.x, pos.y),
            other => panic!("expected PixelDelta, got {other:?}"),
        }
    }

    /// The cross-backend contract table: for each backend, the raw value a
    /// physical "scroll one step toward later content" (wheel toward the
    /// user / swipe down / tilt right) produces, and the single normalized
    /// delta every one of them must land on. A same-direction gesture that
    /// scrolls opposite ways on two backends is exactly the bug this table
    /// pins.
    #[test]
    fn one_physical_gesture_lands_on_one_normalized_delta() {
        // Wheel toward the user (scroll down), one notch:
        // winit reports line-y = -1.0 → contract +1.0.
        assert_eq!(lines(from_winit_lines(0.0, -1.0)), (0.0, 1.0));
        // Win32 reports distance -120 → contract +1.0 line.
        assert_eq!(lines(from_win32_wheel(-120)), (0.0, 1.0));
        // AppKit (non-precise) reports scrollingDeltaY = +1.0 for scroll UP,
        // so scroll down is -1.0 → contract +1.0.
        assert_eq!(lines(from_appkit(0.0, -1.0, false)), (0.0, 1.0));
        // DOM (mode 1) already reports +1.0 for scroll down → passthrough.
        assert_eq!(lines(from_web(1, 0.0, 1.0)), (0.0, 1.0));
    }

    #[test]
    fn horizontal_right_is_positive_on_every_backend() {
        // winit's positive line-x means content moves right (scroll LEFT):
        // a scroll-right gesture is x = -1.0 → contract +1.0.
        assert_eq!(lines(from_winit_lines(-1.0, 0.0)), (1.0, 0.0));
        // Win32 horizontal wheel: tilt right is ALREADY positive → +1.0.
        assert_eq!(lines(from_win32_hwheel(120)), (1.0, 0.0));
        // AppKit: swipe left (scroll right) is scrollingDeltaX = -1.0.
        assert_eq!(lines(from_appkit(-1.0, 0.0, false)), (1.0, 0.0));
        // DOM: scroll right is already deltaX = +1.0.
        assert_eq!(lines(from_web(1, 1.0, 0.0)), (1.0, 0.0));
    }

    /// Pixel-mode deltas must come out in LOGICAL pixels: winit's physical
    /// pixels are divided by the scale factor; AppKit points and CSS pixels
    /// already are logical and pass through unscaled. A 2x-DPI trackpad tick
    /// must not scroll twice as far on one backend as another.
    #[test]
    fn pixel_deltas_are_logical_on_every_backend() {
        assert_eq!(pixels(from_winit_pixels(0.0, -100.0, 2.0)), (0.0, 50.0));
        assert_eq!(pixels(from_appkit(0.0, -50.0, true)), (0.0, 50.0));
        assert_eq!(pixels(from_web(0, 0.0, 50.0)), (0.0, 50.0));
    }

    /// Fractional Win32 distances (fine-resolution wheels report fractions
    /// of `WHEEL_DELTA`) survive the division instead of truncating.
    #[test]
    fn win32_fractional_detents_stay_fractional() {
        assert_eq!(lines(from_win32_wheel(-60)), (0.0, 0.5));
        assert_eq!(lines(from_win32_hwheel(-30)), (-0.25, 0.0));
    }

    /// DOM page-mode deltas stay pages (the shared consumer owns the
    /// page-to-pixel conversion), and an unknown `deltaMode` falls back to
    /// pixels rather than dropping the event.
    #[test]
    fn web_pages_pass_through_and_unknown_modes_fall_back_to_pixels() {
        assert!(matches!(
            from_web(2, 0.0, 1.0),
            ScrollDelta::PageDelta(x, y) if x == 0.0 && y == 1.0
        ));
        assert_eq!(pixels(from_web(7, 3.0, 4.0)), (3.0, 4.0));
    }
}
