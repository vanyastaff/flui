//! Wheel normalization at the Web and Win32 translation boundaries.
//!
//! Positive deltas increase the consumer's scroll offset (down/right).
//! Logical pixels, lines and pages retain their own units; the consumer
//! resolves lines/pages against its settings and viewport.

use flui_platform_api::pointer::{InputValueError, ScrollDelta, ScrollUnit};

/// A Win32 vertical wheel detent points away from the user, toward earlier
/// content: invert its sign and divide by WHEEL_DELTA (120).
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>
pub fn from_win32_wheel(raw_distance: i16) -> ScrollDelta {
    ScrollDelta::try_new(ScrollUnit::Lines, 0.0, -f64::from(raw_distance) / 120.0)
        .expect("BUG: finite Win32 wheel distance divided by a nonzero detent")
}

/// A Win32 horizontal wheel detent points right, matching the consumer sign.
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousehwheel>
pub fn from_win32_hwheel(raw_distance: i16) -> ScrollDelta {
    ScrollDelta::try_new(ScrollUnit::Lines, f64::from(raw_distance) / 120.0, 0.0)
        .expect("BUG: finite Win32 wheel distance divided by a nonzero detent")
}

/// DOM wheel values already use down/right-positive logical CSS units.
/// Unknown modes follow DOM_DELTA_PIXEL; nonfinite deltas are refused.
/// <https://w3c.github.io/uievents/#events-wheelevents>
pub fn from_web(
    delta_mode: u32,
    delta_x: f64,
    delta_y: f64,
) -> Result<ScrollDelta, InputValueError> {
    let unit = match delta_mode {
        1 => ScrollUnit::Lines,
        2 => ScrollUnit::Pages,
        _ => ScrollUnit::Pixels,
    };
    ScrollDelta::try_new(unit, delta_x, delta_y)
}
