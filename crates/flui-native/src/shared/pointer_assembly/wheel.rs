//! A mouse wheel turn as plain data.
//!
//! A wheel turn belongs to no contact sequence: it scrolls under the primary
//! mouse without pressing anything. The backend copies its message into a
//! [`WheelReading`] and [`ContactRegistry::wheel`] turns it into the scroll,
//! so the registry's native recording holds the turn as one row and replays
//! it on any host.

use flui_foundation::geometry::Point;
use flui_platform::EventTime;
use flui_platform::keyboard::Modifiers;
use flui_platform::pointer::{PointerEvent, PointerPosition, ScrollEvent, ScrollPrecision};

use super::ContactRegistry;
use crate::shared::events::primary_mouse_info;
use crate::shared::scroll::{from_win32_hwheel, from_win32_wheel};
use flui_platform::PlatformInput;

/// The distance of one wheel detent (Win32's `WHEEL_DELTA`).
pub const WHEEL_DETENT: i16 = 120;

/// Which wheel turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WheelAxis {
    /// The vertical wheel (`WM_MOUSEWHEEL`).
    Vertical,
    /// The horizontal wheel or a tilt (`WM_MOUSEHWHEEL`).
    Horizontal,
}

/// One wheel message: its distance as the OS signs it, the cursor and when.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelReading {
    /// Which wheel.
    pub axis: WheelAxis,
    /// The signed distance in [`WHEEL_DETENT`] units, as Win32 signs it:
    /// positive is the vertical wheel turned away from the user, or the
    /// horizontal wheel tilted right.
    pub distance: i16,
    /// The cursor, in device pixels of the window's client area.
    pub position: (f64, f64),
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// When the message was generated, already on the event timeline.
    pub time: EventTime,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

impl ContactRegistry {
    /// The scroll `reading` delivers under the primary mouse, or `None` when
    /// its position or scale is not a finite point.
    ///
    /// A distance that is not a whole number of detents is
    /// [`ScrollPrecision::Precise`]: it shows a finer device. A whole one is
    /// [`ScrollPrecision::Unknown`], since a precise device can also send
    /// whole detents and the message carries no capability flag.
    #[must_use]
    pub fn wheel(&self, reading: &WheelReading) -> Option<PlatformInput> {
        journal!(self.journal, super::capture::Captured::Wheel(reading));
        let scale = reading.scale;
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let (x, y) = reading.position;
        let position = PointerPosition::try_new(Point::new(x / scale, y / scale)).ok()?;
        let delta = match reading.axis {
            WheelAxis::Vertical => from_win32_wheel(reading.distance),
            WheelAxis::Horizontal => from_win32_hwheel(reading.distance),
        };
        let precision = if reading.distance % WHEEL_DETENT == 0 {
            ScrollPrecision::Unknown
        } else {
            ScrollPrecision::Precise
        };
        Some(PlatformInput::Pointer(PointerEvent::Scroll(
            ScrollEvent::new(primary_mouse_info(), reading.time, position, delta)
                .with_precision(precision)
                .with_modifiers(reading.modifiers),
        )))
    }
}
