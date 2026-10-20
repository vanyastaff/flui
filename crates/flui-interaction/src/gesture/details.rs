//! Gesture detail types
//!
//! Callback payloads for tap, held-press and pressure recognition.
//!
//! Each phase names the payload its callback actually delivers. Root and local
//! positions use logical units; device kinds are frozen on contact admission.
//! Drag and scale payloads remain next to their recognizers.

use flui_foundation::geometry::Offset;

use crate::{PointerId, PointerKind, Velocity};

// ============================================================================
// Tap Gesture Details
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a tap-down event: the pointer has contacted the screen and
/// might begin a tap.
#[non_exhaustive]
pub struct TapDownDetails {
    /// The global position where the tap occurred
    pub global_position: Offset<f64>,

    /// The local position where the tap occurred
    pub local_position: Offset<f64>,

    /// The kind of device that triggered the tap
    pub kind: PointerKind,

    /// The contact that pressed: the one a later up or cancel of the same
    /// press names
    pub pointer: PointerId,
}

impl TapDownDetails {
    /// Creates new tap down details
    #[inline]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        pointer: PointerId,
    ) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerKind::Touch,
            pointer,
        }
    }

    /// Builder method to set the device kind
    #[inline]
    pub fn with_kind(mut self, kind: PointerKind) -> Self {
        self.kind = kind;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a tap-up event: the pointer that triggered a tap has
/// stopped contacting the screen.
#[non_exhaustive]
pub struct TapUpDetails {
    /// The global position where the tap ended
    pub global_position: Offset<f64>,

    /// The local position where the tap ended
    pub local_position: Offset<f64>,

    /// The kind of device that triggered the tap
    pub kind: PointerKind,

    /// The contact that lifted
    pub pointer: PointerId,
}

impl TapUpDetails {
    /// Creates new tap up details
    #[inline]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        pointer: PointerId,
    ) -> Self {
        Self {
            global_position,
            local_position,
            kind: PointerKind::Touch,
            pointer,
        }
    }

    /// Builder method to set the device kind
    #[inline]
    pub fn with_kind(mut self, kind: PointerKind) -> Self {
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
#[non_exhaustive]
pub struct LongPressMoveUpdateDetails {
    /// The global position of the pointer
    pub global_position: Offset<f64>,

    /// The local position of the pointer
    pub local_position: Offset<f64>,

    /// Root-space displacement from the admitted Down position.
    pub offset_from_origin: Offset<f64>,

    /// Recognizer-local displacement from the admitted Down position.
    pub local_offset_from_origin: Offset<f64>,

    /// Device kind frozen at admission.
    pub kind: PointerKind,
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
            kind: PointerKind::Touch,
        }
    }

    /// Set the admitted device kind for an authored payload.
    #[inline]
    pub const fn with_kind(mut self, kind: PointerKind) -> Self {
        self.kind = kind;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a long-press-end event: the pointer that held the long
/// press has stopped contacting the screen.
#[non_exhaustive]
pub struct LongPressEndDetails {
    /// The global position where the long press ended
    pub global_position: Offset<f64>,

    /// The local position where the long press ended
    pub local_position: Offset<f64>,

    /// Recognizer-local measured velocity at release, in logical units/second.
    pub velocity: Velocity,

    /// Device kind frozen at admission.
    pub kind: PointerKind,
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
            kind: PointerKind::Touch,
        }
    }

    /// Set the admitted device kind for an authored payload.
    #[inline]
    pub const fn with_kind(mut self, kind: PointerKind) -> Self {
        self.kind = kind;
        self
    }
}

// ============================================================================
// Force Press Gesture Details
// ============================================================================

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Details for a force-press event: the pointer's pressure on a
/// pressure-sensitive screen, along with its position.
#[non_exhaustive]
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
