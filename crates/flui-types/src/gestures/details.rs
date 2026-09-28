//! Gesture detail types
//!
//! Recognizer-independent gesture detail payloads: tap, long-press
//! move-update/end, and force press. The drag, scale, and long-press
//! down/start payloads are defined next to their recognizers in
//! `flui-interaction` instead — they carry the W3C pointer vocabulary
//! (`ui_events::pointer::PointerType`) and the recognizer clock
//! (`Instant`), neither of which this dependency-free vocabulary crate
//! knows about.

use super::{pointer::PointerDeviceKind, velocity::Velocity};
use crate::geometry::Offset;

// ============================================================================
// Tap Gesture Details
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a tap-down event: the pointer has contacted the screen and
/// might begin a tap.
pub struct TapDownDetails {
    /// The global position where the tap occurred
    pub global_position: Offset<f64>,

    /// The local position where the tap occurred
    pub local_position: Offset<f64>,

    /// The kind of device that triggered the tap
    pub kind: PointerDeviceKind,
}

impl TapDownDetails {
    /// Creates new tap down details
    #[inline]
    pub const fn new(global_position: Offset<f64>, local_position: Offset<f64>) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerDeviceKind::Touch,
        }
    }

    /// Builder method to set the device kind
    #[inline]
    pub fn with_kind(mut self, kind: PointerDeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a tap-up event: the pointer that triggered a tap has
/// stopped contacting the screen.
pub struct TapUpDetails {
    /// The global position where the tap ended
    pub global_position: Offset<f64>,

    /// The local position where the tap ended
    pub local_position: Offset<f64>,

    /// The kind of device that triggered the tap
    pub kind: PointerDeviceKind,
}

impl TapUpDetails {
    /// Creates new tap up details
    #[inline]
    pub const fn new(global_position: Offset<f64>, local_position: Offset<f64>) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerDeviceKind::Touch,
        }
    }

    /// Builder method to set the device kind
    #[inline]
    pub fn with_kind(mut self, kind: PointerDeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

// ============================================================================
// Long Press Gesture Details
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a long-press-move-update event: the pointer has moved
/// while the long press is held, carrying offsets from the press origin.
pub struct LongPressMoveUpdateDetails {
    /// The global position of the pointer
    pub global_position: Offset<f64>,

    /// The local position of the pointer
    pub local_position: Offset<f64>,

    /// The distance moved since the last update
    pub offset_from_origin: Offset<f64>,

    /// The total distance moved since the long press started
    pub local_offset_from_origin: Offset<f64>,
}

impl LongPressMoveUpdateDetails {
    /// Creates new long press move update details
    #[inline]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        offset_from_origin: Offset<f64>,
        local_offset_from_origin: Offset<f64>,
    ) -> Self {
        Self {
            global_position,
            local_position,
            offset_from_origin,
            local_offset_from_origin,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a long-press-end event: the pointer that held the long
/// press has stopped contacting the screen.
pub struct LongPressEndDetails {
    /// The global position where the long press ended
    pub global_position: Offset<f64>,

    /// The local position where the long press ended
    pub local_position: Offset<f64>,

    /// The velocity when the long press ended
    pub velocity: Velocity,
}

impl LongPressEndDetails {
    /// Creates new long press end details
    #[inline]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        velocity: Velocity,
    ) -> Self {
        Self {
            global_position,
            local_position,
            velocity,
        }
    }
}

// ============================================================================
// Force Press Gesture Details
// ============================================================================

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a force-press event: the pointer's pressure on a
/// pressure-sensitive screen, along with its position.
pub struct ForcePressDetails {
    /// The global position of the pointer
    pub global_position: Offset<f64>,

    /// The local position of the pointer
    pub local_position: Offset<f64>,

    /// The pressure of the touch (0.0 to 1.0)
    pub pressure: f64,

    /// The maximum pressure the device can detect
    pub max_pressure: f64,
}

impl ForcePressDetails {
    /// Creates new force press details
    #[inline]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        pressure: f64,
        max_pressure: f64,
    ) -> Self {
        Self {
            global_position,
            local_position,
            pressure,
            max_pressure,
        }
    }

    /// Returns the normalized pressure (0.0 to 1.0)
    #[inline]
    pub fn normalized_pressure(&self) -> f64 {
        if self.max_pressure > 0.0 {
            (self.pressure / self.max_pressure).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Normalized against `max_pressure` and clamped; no maximum reads as 0.
    #[test]
    fn force_press_normalized_pressure() {
        for ((pressure, max), expected) in
            [((2.5, 10.0), 0.25), ((20.0, 10.0), 1.0), ((3.0, 0.0), 0.0)]
        {
            let details = ForcePressDetails::new(Offset::ZERO, Offset::ZERO, pressure, max);
            assert_eq!(
                details.normalized_pressure(),
                expected,
                "{pressure} of {max}"
            );
        }
    }
}
