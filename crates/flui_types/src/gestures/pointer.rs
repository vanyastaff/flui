//! Pointer data types
//!
//! This module provides types for tracking pointer/touch device information.

use crate::geometry::Offset;
use std::time::Duration;

/// A pair of local and global offsets
///
/// Similar to Flutter's `OffsetPair`. Used to track both the position
/// relative to the target widget (local) and relative to the screen (global).
///
/// # Examples
///
/// ```
/// use flui_types::gestures::OffsetPair;
/// use flui_types::Offset;
///
/// let pair = OffsetPair::new(
///     Offset::new(10.0, 20.0),  // local
///     Offset::new(100.0, 200.0), // global
/// );
///
/// assert_eq!(pair.local, Offset::new(10.0, 20.0));
/// assert_eq!(pair.global, Offset::new(100.0, 200.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OffsetPair {
    /// The local offset (relative to the target widget)
    pub local: Offset,

    /// The global offset (relative to the screen/window)
    pub global: Offset,
}

impl OffsetPair {
    /// The zero offset pair
    pub const ZERO: Self = Self {
        local: Offset::ZERO,
        global: Offset::ZERO,
    };

    /// Creates a new offset pair
    pub const fn new(local: Offset, global: Offset) -> Self {
        Self { local, global }
    }

    /// Creates an offset pair with the same local and global offsets
    pub const fn from_offset(offset: Offset) -> Self {
        Self {
            local: offset,
            global: offset,
        }
    }
}

impl Default for OffsetPair {
    fn default() -> Self {
        Self::ZERO
    }
}

/// The kind of pointer device
///
/// Similar to Flutter's `PointerDeviceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub enum PointerDeviceKind {
    /// A touch-based pointer device (finger on touchscreen)
    #[default]
    Touch,

    /// A mouse pointer device
    Mouse,

    /// A stylus pointer device
    Stylus,

    /// An inverted stylus (eraser end)
    InvertedStylus,

    /// A trackpad pointer device
    Trackpad,

    /// An unknown pointer device
    Unknown,
}


/// Information about a pointer event
///
/// Similar to Flutter's `PointerData`. Contains all the data about
/// a pointer at a specific moment in time.
///
/// # Examples
///
/// ```
/// use flui_types::gestures::{PointerData, PointerDeviceKind};
/// use flui_types::Offset;
/// use std::time::Duration;
///
/// let data = PointerData::new(
///     Duration::from_millis(100),
///     Offset::new(100.0, 200.0),
///     0,
///     PointerDeviceKind::Touch,
/// );
///
/// assert_eq!(data.position, Offset::new(100.0, 200.0));
/// assert_eq!(data.pointer, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointerData {
    /// Time of the event
    pub time_stamp: Duration,

    /// The position of the pointer in global coordinates
    pub position: Offset,

    /// The delta since the last update
    pub delta: Offset,

    /// Unique identifier for the pointer
    pub pointer: i32,

    /// The kind of pointer device
    pub device_kind: PointerDeviceKind,

    /// The pressure of the touch (0.0 to 1.0)
    ///
    /// 0.0 means no pressure, 1.0 means maximum pressure.
    /// May be 0.0 if the device doesn't support pressure.
    pub pressure: f32,

    /// The minimum pressure the device can detect
    pub pressure_min: f32,

    /// The maximum pressure the device can detect
    pub pressure_max: f32,

    /// The distance of the pointer from the screen (hover distance)
    ///
    /// Only available for some devices like styluses.
    /// 0.0 means touching the screen.
    pub distance: f32,

    /// The maximum distance the device can detect
    pub distance_max: f32,

    /// The radius of the touch area (major axis)
    pub radius_major: f32,

    /// The radius of the touch area (minor axis)
    pub radius_minor: f32,

    /// The minimum radius the device can detect
    pub radius_min: f32,

    /// The maximum radius the device can detect
    pub radius_max: f32,

    /// The orientation of the touch ellipse
    ///
    /// In radians, from -π to π.
    pub orientation: f32,

    /// The tilt of the stylus
    ///
    /// In radians, from 0 (perpendicular) to π/2 (flat).
    pub tilt: f32,

    /// Opaque platform-specific data
    pub platform_data: i64,

    /// Number of mouse buttons currently pressed
    pub buttons: i32,

    /// Whether the pointer is obscured by another app
    pub obscured: bool,

    /// Whether this is a synthesized event
    pub synthesized: bool,
}

impl PointerData {
    /// Creates new pointer data with default values
    pub fn new(
        time_stamp: Duration,
        position: Offset,
        pointer: i32,
        device_kind: PointerDeviceKind,
    ) -> Self {
        Self {
            time_stamp,
            position,
            delta: Offset::ZERO,
            pointer,
            device_kind,
            pressure: 0.0,
            pressure_min: 0.0,
            pressure_max: 1.0,
            distance: 0.0,
            distance_max: 0.0,
            radius_major: 0.0,
            radius_minor: 0.0,
            radius_min: 0.0,
            radius_max: 0.0,
            orientation: 0.0,
            tilt: 0.0,
            platform_data: 0,
            buttons: 0,
            obscured: false,
            synthesized: false,
        }
    }

    /// Builder method to set pressure
    pub fn with_pressure(mut self, pressure: f32, min: f32, max: f32) -> Self {
        self.pressure = pressure;
        self.pressure_min = min;
        self.pressure_max = max;
        self
    }

    /// Builder method to set distance
    pub fn with_distance(mut self, distance: f32, max: f32) -> Self {
        self.distance = distance;
        self.distance_max = max;
        self
    }

    /// Builder method to set radius
    pub fn with_radius(mut self, major: f32, minor: f32, min: f32, max: f32) -> Self {
        self.radius_major = major;
        self.radius_minor = minor;
        self.radius_min = min;
        self.radius_max = max;
        self
    }

    /// Builder method to set orientation
    pub fn with_orientation(mut self, orientation: f32) -> Self {
        self.orientation = orientation;
        self
    }

    /// Builder method to set tilt
    pub fn with_tilt(mut self, tilt: f32) -> Self {
        self.tilt = tilt;
        self
    }

    /// Builder method to set delta
    pub fn with_delta(mut self, delta: Offset) -> Self {
        self.delta = delta;
        self
    }

    /// Builder method to set buttons
    pub fn with_buttons(mut self, buttons: i32) -> Self {
        self.buttons = buttons;
        self
    }

    /// Returns whether the pointer is down (touching)
    pub fn is_down(&self) -> bool {
        self.distance == 0.0 || self.pressure > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_pair_new() {
        let pair = OffsetPair::new(Offset::new(10.0, 20.0), Offset::new(100.0, 200.0));
        assert_eq!(pair.local, Offset::new(10.0, 20.0));
        assert_eq!(pair.global, Offset::new(100.0, 200.0));
    }

    #[test]
    fn test_offset_pair_zero() {
        let pair = OffsetPair::ZERO;
        assert_eq!(pair.local, Offset::ZERO);
        assert_eq!(pair.global, Offset::ZERO);
    }

    #[test]
    fn test_offset_pair_from_offset() {
        let offset = Offset::new(50.0, 75.0);
        let pair = OffsetPair::from_offset(offset);
        assert_eq!(pair.local, offset);
        assert_eq!(pair.global, offset);
    }

    #[test]
    fn test_pointer_device_kind_default() {
        assert_eq!(PointerDeviceKind::default(), PointerDeviceKind::Touch);
    }

    #[test]
    fn test_pointer_data_new() {
        let data = PointerData::new(
            Duration::from_millis(100),
            Offset::new(100.0, 200.0),
            0,
            PointerDeviceKind::Touch,
        );

        assert_eq!(data.time_stamp, Duration::from_millis(100));
        assert_eq!(data.position, Offset::new(100.0, 200.0));
        assert_eq!(data.pointer, 0);
        assert_eq!(data.device_kind, PointerDeviceKind::Touch);
        assert_eq!(data.delta, Offset::ZERO);
    }

    #[test]
    fn test_pointer_data_builder() {
        let data = PointerData::new(
            Duration::from_millis(100),
            Offset::new(100.0, 200.0),
            0,
            PointerDeviceKind::Stylus,
        )
        .with_pressure(0.8, 0.0, 1.0)
        .with_distance(5.0, 10.0)
        .with_radius(10.0, 5.0, 0.0, 20.0)
        .with_orientation(0.5)
        .with_tilt(0.3)
        .with_delta(Offset::new(2.0, 3.0))
        .with_buttons(1);

        assert_eq!(data.pressure, 0.8);
        assert_eq!(data.distance, 5.0);
        assert_eq!(data.radius_major, 10.0);
        assert_eq!(data.orientation, 0.5);
        assert_eq!(data.tilt, 0.3);
        assert_eq!(data.delta, Offset::new(2.0, 3.0));
        assert_eq!(data.buttons, 1);
    }

    #[test]
    fn test_pointer_data_is_down() {
        let down = PointerData::new(
            Duration::from_millis(100),
            Offset::ZERO,
            0,
            PointerDeviceKind::Touch,
        )
        .with_pressure(0.5, 0.0, 1.0);
        assert!(down.is_down());

        let hover = PointerData::new(
            Duration::from_millis(100),
            Offset::ZERO,
            0,
            PointerDeviceKind::Stylus,
        )
        .with_distance(5.0, 10.0);
        assert!(!hover.is_down());
    }
}
