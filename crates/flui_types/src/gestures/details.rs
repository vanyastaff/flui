//! Gesture detail types
//!
//! This module provides detail types for various gestures like tap, drag,
//! scale, long press, and force press.

use super::pointer::{OffsetPair, PointerDeviceKind};
use super::velocity::Velocity;
use crate::geometry::Offset;
use std::time::Duration;

// ============================================================================
// Tap Gesture Details
// ============================================================================

/// Details for a tap down event
///
/// Similar to Flutter's `TapDownDetails`.
///
/// # Examples
///
/// ```
/// use flui_types::gestures::TapDownDetails;
/// use flui_types::Offset;
///
/// let details = TapDownDetails::new(
///     Offset::<f32>::new(100.0, 200.0),
///     Offset::<f32>::new(10.0, 20.0),
/// );
///
/// assert_eq!(details.global_position, Offset::<f32>::new(100.0, 200.0));
/// assert_eq!(details.local_position, Offset::<f32>::new(10.0, 20.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TapDownDetails {
    /// The global position where the tap occurred
    pub global_position: Offset<f32>,

    /// The local position where the tap occurred
    pub local_position: Offset<f32>,

    /// The kind of device that triggered the tap
    pub kind: PointerDeviceKind,
}

impl TapDownDetails {
    /// Creates new tap down details
    pub const fn new(global_position: Offset<f32>, local_position: Offset<f32>) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerDeviceKind::Touch,
        }
    }

    /// Builder method to set the device kind
    pub fn with_kind(mut self, kind: PointerDeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Details for a tap up event
///
/// Similar to Flutter's `TapUpDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TapUpDetails {
    /// The global position where the tap ended
    pub global_position: Offset<f32>,

    /// The local position where the tap ended
    pub local_position: Offset<f32>,

    /// The kind of device that triggered the tap
    pub kind: PointerDeviceKind,
}

impl TapUpDetails {
    /// Creates new tap up details
    pub const fn new(global_position: Offset<f32>, local_position: Offset<f32>) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerDeviceKind::Touch,
        }
    }

    /// Builder method to set the device kind
    pub fn with_kind(mut self, kind: PointerDeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

// ============================================================================
// Drag Gesture Details
// ============================================================================

/// Details for when a drag gesture starts
///
/// Similar to Flutter's `DragStartDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DragStartDetails {
    /// The time when the drag started
    pub source_time_stamp: Duration,

    /// The global position where the drag started
    pub global_position: Offset<f32>,

    /// The local position where the drag started
    pub local_position: Offset<f32>,

    /// The kind of device performing the drag
    pub kind: PointerDeviceKind,
}

impl DragStartDetails {
    /// Creates new drag start details
    pub const fn new(
        source_time_stamp: Duration,
        global_position: Offset<f32>,
        local_position: Offset<f32>,
    ) -> Self {
        Self {
            source_time_stamp,
            global_position,
            local_position,
            kind: PointerDeviceKind::Touch,
        }
    }

    /// Builder method to set the device kind
    pub fn with_kind(mut self, kind: PointerDeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Details for when a pointer starts contacting the screen (before drag starts)
///
/// Similar to Flutter's `DragDownDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DragDownDetails {
    /// The global position where the pointer contacted
    pub global_position: Offset<f32>,

    /// The local position where the pointer contacted
    pub local_position: Offset<f32>,
}

impl DragDownDetails {
    /// Creates new drag down details
    pub const fn new(global_position: Offset<f32>, local_position: Offset<f32>) -> Self {
        Self {
            global_position,
            local_position,
        }
    }
}

/// Details for when a drag gesture updates
///
/// Similar to Flutter's `DragUpdateDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DragUpdateDetails {
    /// The time when the update occurred
    pub source_time_stamp: Duration,

    /// The amount the pointer has moved since the last update
    pub delta: Offset<f32>,

    /// The primary delta along the main axis
    ///
    /// For vertical drags, this is the y component.
    /// For horizontal drags, this is the x component.
    pub primary_delta: Option<f32>,

    /// The global position of the pointer
    pub global_position: Offset<f32>,

    /// The local position of the pointer
    pub local_position: Offset<f32>,
}

impl DragUpdateDetails {
    /// Creates new drag update details
    pub const fn new(
        source_time_stamp: Duration,
        delta: Offset<f32>,
        global_position: Offset<f32>,
        local_position: Offset<f32>,
    ) -> Self {
        Self {
            source_time_stamp,
            delta,
            primary_delta: None,
            global_position,
            local_position,
        }
    }

    /// Builder method to set the primary delta
    pub fn with_primary_delta(mut self, primary_delta: f32) -> Self {
        self.primary_delta = Some(primary_delta);
        self
    }
}

/// Details for when a drag gesture ends
///
/// Similar to Flutter's `DragEndDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DragEndDetails {
    /// The velocity of the pointer when the drag ended
    pub velocity: Velocity,

    /// The primary velocity along the main axis
    pub primary_velocity: Option<f32>,
}

impl DragEndDetails {
    /// Creates new drag end details
    pub const fn new(velocity: Velocity) -> Self {
        Self {
            velocity,
            primary_velocity: None,
        }
    }

    /// Builder method to set the primary velocity
    pub fn with_primary_velocity(mut self, primary_velocity: f32) -> Self {
        self.primary_velocity = Some(primary_velocity);
        self
    }
}

// ============================================================================
// Scale Gesture Details
// ============================================================================

/// Details for when a scale gesture starts
///
/// Similar to Flutter's `ScaleStartDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScaleStartDetails {
    /// The focal point of the pointers in contact with the screen
    pub focal_point: OffsetPair,

    /// The number of pointers in contact with the screen
    pub pointer_count: usize,
}

impl ScaleStartDetails {
    /// Creates new scale start details
    pub const fn new(focal_point: OffsetPair, pointer_count: usize) -> Self {
        Self {
            focal_point,
            pointer_count,
        }
    }
}

/// Details for when a scale gesture updates
///
/// Similar to Flutter's `ScaleUpdateDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScaleUpdateDetails {
    /// The focal point of the pointers in contact with the screen
    pub focal_point: OffsetPair,

    /// The focal point delta since the last update
    pub focal_point_delta: Offset<f32>,

    /// The scale factor
    ///
    /// 1.0 means no scale, >1.0 means zoom in, <1.0 means zoom out.
    pub scale: f32,

    /// The horizontal scale factor
    pub horizontal_scale: f32,

    /// The vertical scale factor
    pub vertical_scale: f32,

    /// The rotation in radians
    pub rotation: f32,

    /// The number of pointers in contact with the screen
    pub pointer_count: usize,
}

impl ScaleUpdateDetails {
    /// Creates new scale update details
    pub const fn new(
        focal_point: OffsetPair,
        scale: f32,
        rotation: f32,
        pointer_count: usize,
    ) -> Self {
        Self {
            focal_point,
            focal_point_delta: Offset::<f32>::ZERO,
            scale,
            horizontal_scale: scale,
            vertical_scale: scale,
            rotation,
            pointer_count,
        }
    }

    /// Builder method to set the focal point delta
    pub fn with_focal_point_delta(mut self, delta: Offset<f32>) -> Self {
        self.focal_point_delta = delta;
        self
    }

    /// Builder method to set individual scale factors
    pub fn with_scale_factors(mut self, horizontal: f32, vertical: f32) -> Self {
        self.horizontal_scale = horizontal;
        self.vertical_scale = vertical;
        self
    }
}

/// Details for when a scale gesture ends
///
/// Similar to Flutter's `ScaleEndDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScaleEndDetails {
    /// The velocity of the gesture
    pub velocity: Velocity,

    /// The number of pointers that were in contact when the gesture ended
    pub pointer_count: usize,
}

impl ScaleEndDetails {
    /// Creates new scale end details
    pub const fn new(velocity: Velocity, pointer_count: usize) -> Self {
        Self {
            velocity,
            pointer_count,
        }
    }
}

// ============================================================================
// Long Press Gesture Details
// ============================================================================

/// Details for when a pointer contacts the screen (before long press)
///
/// Similar to Flutter's `LongPressDownDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressDownDetails {
    /// The global position where the pointer contacted
    pub global_position: Offset<f32>,

    /// The local position where the pointer contacted
    pub local_position: Offset<f32>,

    /// The kind of device
    pub kind: PointerDeviceKind,
}

impl LongPressDownDetails {
    /// Creates new long press down details
    pub const fn new(global_position: Offset<f32>, local_position: Offset<f32>) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerDeviceKind::Touch,
        }
    }

    /// Builder method to set the device kind
    pub fn with_kind(mut self, kind: PointerDeviceKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Details for when a long press gesture starts
///
/// Similar to Flutter's `LongPressStartDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressStartDetails {
    /// The global position where the long press started
    pub global_position: Offset<f32>,

    /// The local position where the long press started
    pub local_position: Offset<f32>,
}

impl LongPressStartDetails {
    /// Creates new long press start details
    pub const fn new(global_position: Offset<f32>, local_position: Offset<f32>) -> Self {
        Self {
            global_position,
            local_position,
        }
    }
}

/// Details for when a long press gesture updates (moves)
///
/// Similar to Flutter's `LongPressMoveUpdateDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressMoveUpdateDetails {
    /// The global position of the pointer
    pub global_position: Offset<f32>,

    /// The local position of the pointer
    pub local_position: Offset<f32>,

    /// The distance moved since the last update
    pub offset_from_origin: Offset<f32>,

    /// The total distance moved since the long press started
    pub local_offset_from_origin: Offset<f32>,
}

impl LongPressMoveUpdateDetails {
    /// Creates new long press move update details
    pub const fn new(
        global_position: Offset<f32>,
        local_position: Offset<f32>,
        offset_from_origin: Offset<f32>,
        local_offset_from_origin: Offset<f32>,
    ) -> Self {
        Self {
            global_position,
            local_position,
            offset_from_origin,
            local_offset_from_origin,
        }
    }
}

/// Details for when a long press gesture ends
///
/// Similar to Flutter's `LongPressEndDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressEndDetails {
    /// The global position where the long press ended
    pub global_position: Offset<f32>,

    /// The local position where the long press ended
    pub local_position: Offset<f32>,

    /// The velocity when the long press ended
    pub velocity: Velocity,
}

impl LongPressEndDetails {
    /// Creates new long press end details
    pub const fn new(global_position: Offset<f32>, local_position: Offset<f32>, velocity: Velocity) -> Self {
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

/// Details for a force press gesture
///
/// Similar to Flutter's `ForcePressDetails`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ForcePressDetails {
    /// The global position of the pointer
    pub global_position: Offset<f32>,

    /// The local position of the pointer
    pub local_position: Offset<f32>,

    /// The pressure of the touch (0.0 to 1.0)
    pub pressure: f32,

    /// The maximum pressure the device can detect
    pub max_pressure: f32,
}

impl ForcePressDetails {
    /// Creates new force press details
    pub const fn new(
        global_position: Offset<f32>,
        local_position: Offset<f32>,
        pressure: f32,
        max_pressure: f32,
    ) -> Self {
        Self {
            global_position,
            local_position,
            pressure,
            max_pressure,
        }
    }

    /// Returns the normalized pressure (0.0 to 1.0)
    pub fn normalized_pressure(&self) -> f32 {
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

    #[test]
    fn test_tap_down_details() {
        let details = TapDownDetails::new(Offset::<f32>::new(100.0, 200.0), Offset::<f32>::new(10.0, 20.0))
            .with_kind(PointerDeviceKind::Mouse);

        assert_eq!(details.global_position, Offset::<f32>::new(100.0, 200.0));
        assert_eq!(details.local_position, Offset::<f32>::new(10.0, 20.0));
        assert_eq!(details.kind, PointerDeviceKind::Mouse);
    }

    #[test]
    fn test_drag_start_details() {
        let details = DragStartDetails::new(
            Duration::from_millis(100),
            Offset::<f32>::new(100.0, 200.0),
            Offset::<f32>::new(10.0, 20.0),
        );

        assert_eq!(details.source_time_stamp, Duration::from_millis(100));
        assert_eq!(details.global_position, Offset::<f32>::new(100.0, 200.0));
    }

    #[test]
    fn test_drag_update_details() {
        let details = DragUpdateDetails::new(
            Duration::from_millis(100),
            Offset::<f32>::new(5.0, 10.0),
            Offset::<f32>::new(100.0, 200.0),
            Offset::<f32>::new(10.0, 20.0),
        )
        .with_primary_delta(10.0);

        assert_eq!(details.delta, Offset::<f32>::new(5.0, 10.0));
        assert_eq!(details.primary_delta, Some(10.0));
    }

    #[test]
    fn test_scale_update_details() {
        let focal_point = OffsetPair::new(Offset::<f32>::new(10.0, 20.0), Offset::<f32>::new(100.0, 200.0));
        let details = ScaleUpdateDetails::new(focal_point, 1.5, 0.5, 2)
            .with_focal_point_delta(Offset::<f32>::new(2.0, 3.0))
            .with_scale_factors(1.6, 1.4);

        assert_eq!(details.scale, 1.5);
        assert_eq!(details.rotation, 0.5);
        assert_eq!(details.pointer_count, 2);
        assert_eq!(details.focal_point_delta, Offset::<f32>::new(2.0, 3.0));
        assert_eq!(details.horizontal_scale, 1.6);
        assert_eq!(details.vertical_scale, 1.4);
    }

    #[test]
    fn test_force_press_details() {
        let details =
            ForcePressDetails::new(Offset::<f32>::new(100.0, 200.0), Offset::<f32>::new(10.0, 20.0), 0.8, 1.0);

        assert_eq!(details.pressure, 0.8);
        assert_eq!(details.max_pressure, 1.0);
        assert_eq!(details.normalized_pressure(), 0.8);
    }

    #[test]
    fn test_force_press_normalized_pressure() {
        let details = ForcePressDetails::new(Offset::<f32>::ZERO, Offset::<f32>::ZERO, 50.0, 100.0);
        assert_eq!(details.normalized_pressure(), 0.5);

        let no_max = ForcePressDetails::new(Offset::<f32>::ZERO, Offset::<f32>::ZERO, 50.0, 0.0);
        assert_eq!(no_max.normalized_pressure(), 0.0);
    }
}
