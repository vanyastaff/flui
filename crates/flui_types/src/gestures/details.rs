//! Gesture detail types
//!
//! This module provides detail types for various gestures like tap, drag,
//! scale, long press, and force press.

use super::pointer::{OffsetPair, PointerDeviceKind};
use super::velocity::Velocity;
use crate::geometry::{Offset, Pixels};
use std::time::Duration;

// ============================================================================
// Tap Gesture Details
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TapDownDetails {
    /// The global position where the tap occurred
    pub global_position: Offset<Pixels>,

    /// The local position where the tap occurred
    pub local_position: Offset<Pixels>,

    /// The kind of device that triggered the tap
    pub kind: PointerDeviceKind,
}

impl TapDownDetails {
    /// Creates new tap down details
    #[inline]
    pub const fn new(global_position: Offset<Pixels>, local_position: Offset<Pixels>) -> Self {
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
pub struct TapUpDetails {
    /// The global position where the tap ended
    pub global_position: Offset<Pixels>,

    /// The local position where the tap ended
    pub local_position: Offset<Pixels>,

    /// The kind of device that triggered the tap
    pub kind: PointerDeviceKind,
}

impl TapUpDetails {
    /// Creates new tap up details
    #[inline]
    pub const fn new(global_position: Offset<Pixels>, local_position: Offset<Pixels>) -> Self {
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
// Drag Gesture Details
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DragStartDetails {
    /// The time when the drag started
    pub source_time_stamp: Duration,

    /// The global position where the drag started
    pub global_position: Offset<Pixels>,

    /// The local position where the drag started
    pub local_position: Offset<Pixels>,

    /// The kind of device performing the drag
    pub kind: PointerDeviceKind,
}

impl DragStartDetails {
    /// Creates new drag start details
    #[inline]
    pub const fn new(
        source_time_stamp: Duration,
        global_position: Offset<Pixels>,
        local_position: Offset<Pixels>,
    ) -> Self {
        Self {
            source_time_stamp,
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
pub struct DragDownDetails {
    /// The global position where the pointer contacted
    pub global_position: Offset<Pixels>,

    /// The local position where the pointer contacted
    pub local_position: Offset<Pixels>,
}

impl DragDownDetails {
    /// Creates new drag down details
    #[inline]
    pub const fn new(global_position: Offset<Pixels>, local_position: Offset<Pixels>) -> Self {
        Self {
            global_position,
            local_position,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DragUpdateDetails {
    /// The time when the update occurred
    pub source_time_stamp: Duration,

    /// The amount the pointer has moved since the last update
    pub delta: Offset<Pixels>,

    /// The primary delta along the main axis
    ///
    /// For vertical drags, this is the y component.
    /// For horizontal drags, this is the x component.
    pub primary_delta: Option<f32>,

    /// The global position of the pointer
    pub global_position: Offset<Pixels>,

    /// The local position of the pointer
    pub local_position: Offset<Pixels>,
}

impl DragUpdateDetails {
    /// Creates new drag update details
    #[inline]
    pub const fn new(
        source_time_stamp: Duration,
        delta: Offset<Pixels>,
        global_position: Offset<Pixels>,
        local_position: Offset<Pixels>,
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
    #[inline]
    pub fn with_primary_delta(mut self, primary_delta: f32) -> Self {
        self.primary_delta = Some(primary_delta);
        self
    }
}

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
    #[inline]
    pub const fn new(velocity: Velocity) -> Self {
        Self {
            velocity,
            primary_velocity: None,
        }
    }

    /// Builder method to set the primary velocity
    #[inline]
    pub fn with_primary_velocity(mut self, primary_velocity: f32) -> Self {
        self.primary_velocity = Some(primary_velocity);
        self
    }
}

// ============================================================================
// Scale Gesture Details
// ============================================================================

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
    #[inline]
    pub const fn new(focal_point: OffsetPair, pointer_count: usize) -> Self {
        Self {
            focal_point,
            pointer_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScaleUpdateDetails {
    /// The focal point of the pointers in contact with the screen
    pub focal_point: OffsetPair,

    /// The focal point delta since the last update
    pub focal_point_delta: Offset<Pixels>,

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
    #[inline]
    pub const fn new(
        focal_point: OffsetPair,
        scale: f32,
        rotation: f32,
        pointer_count: usize,
    ) -> Self {
        Self {
            focal_point,
            focal_point_delta: Offset::<Pixels>::ZERO,
            scale,
            horizontal_scale: scale,
            vertical_scale: scale,
            rotation,
            pointer_count,
        }
    }

    /// Builder method to set the focal point delta
    #[inline]
    pub fn with_focal_point_delta(mut self, delta: Offset<Pixels>) -> Self {
        self.focal_point_delta = delta;
        self
    }

    /// Builder method to set individual scale factors
    #[inline]
    pub fn with_scale_factors(mut self, horizontal: f32, vertical: f32) -> Self {
        self.horizontal_scale = horizontal;
        self.vertical_scale = vertical;
        self
    }
}

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
    #[inline]
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

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressDownDetails {
    /// The global position where the pointer contacted
    pub global_position: Offset<Pixels>,

    /// The local position where the pointer contacted
    pub local_position: Offset<Pixels>,

    /// The kind of device
    pub kind: PointerDeviceKind,
}

impl LongPressDownDetails {
    /// Creates new long press down details
    #[inline]
    pub const fn new(global_position: Offset<Pixels>, local_position: Offset<Pixels>) -> Self {
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
pub struct LongPressStartDetails {
    /// The global position where the long press started
    pub global_position: Offset<Pixels>,

    /// The local position where the long press started
    pub local_position: Offset<Pixels>,
}

impl LongPressStartDetails {
    /// Creates new long press start details
    #[inline]
    pub const fn new(global_position: Offset<Pixels>, local_position: Offset<Pixels>) -> Self {
        Self {
            global_position,
            local_position,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressMoveUpdateDetails {
    /// The global position of the pointer
    pub global_position: Offset<Pixels>,

    /// The local position of the pointer
    pub local_position: Offset<Pixels>,

    /// The distance moved since the last update
    pub offset_from_origin: Offset<Pixels>,

    /// The total distance moved since the long press started
    pub local_offset_from_origin: Offset<Pixels>,
}

impl LongPressMoveUpdateDetails {
    /// Creates new long press move update details
    #[inline]
    pub const fn new(
        global_position: Offset<Pixels>,
        local_position: Offset<Pixels>,
        offset_from_origin: Offset<Pixels>,
        local_offset_from_origin: Offset<Pixels>,
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
pub struct LongPressEndDetails {
    /// The global position where the long press ended
    pub global_position: Offset<Pixels>,

    /// The local position where the long press ended
    pub local_position: Offset<Pixels>,

    /// The velocity when the long press ended
    pub velocity: Velocity,
}

impl LongPressEndDetails {
    /// Creates new long press end details
    #[inline]
    pub const fn new(
        global_position: Offset<Pixels>,
        local_position: Offset<Pixels>,
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
pub struct ForcePressDetails {
    /// The global position of the pointer
    pub global_position: Offset<Pixels>,

    /// The local position of the pointer
    pub local_position: Offset<Pixels>,

    /// The pressure of the touch (0.0 to 1.0)
    pub pressure: f32,

    /// The maximum pressure the device can detect
    pub max_pressure: f32,
}

impl ForcePressDetails {
    /// Creates new force press details
    #[inline]
    pub const fn new(
        global_position: Offset<Pixels>,
        local_position: Offset<Pixels>,
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
    #[inline]
    pub fn normalized_pressure(&self) -> f32 {
        if self.max_pressure > 0.0 {
            (self.pressure / self.max_pressure).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}
