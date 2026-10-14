//! Motion constraints shared by gesture recognizers.

/// Drag axis constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DragAxis {
    /// Vertical drag only (up/down).
    Vertical,
    /// Horizontal drag only (left/right).
    Horizontal,
    /// Free drag (any direction).
    #[default]
    Free,
}

/// Whether a free motion locks onto the axis it starts along.
///
/// A pan or a scale's focal motion that starts within 22.5° of an axis stays
/// on that axis for the rest of its gesture: each delta and the release
/// velocity are projected onto it. A diagonal start moves freely (UIKit
/// `isDirectionalLockEnabled`). The rails hold for touch and touchpad only; a
/// mouse or a pen follows the pointer exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum AxisLock {
    /// Motion follows the contact in every direction.
    #[default]
    Free,
    /// Motion that starts near an axis stays on it.
    Rails,
}

impl AxisLock {
    /// Whether a motion of `kind` may rail: touch and touchpad, under
    /// [`Rails`](Self::Rails).
    pub(crate) fn rails(self, kind: crate::events::PointerKind) -> bool {
        use crate::events::PointerKind;
        self == Self::Rails && matches!(kind, PointerKind::Touch | PointerKind::Trackpad)
    }

    /// The axis a motion of `kind` that started along `displacement` locks
    /// onto, if any.
    pub(crate) fn rail(
        self,
        kind: crate::events::PointerKind,
        displacement: flui_foundation::geometry::Offset<f64>,
    ) -> Option<flui_foundation::geometry::Axis> {
        use flui_foundation::geometry::Axis;
        // tan 22.5° = √2 − 1: within it of an axis, the motion rails.
        const RAIL: f64 = std::f64::consts::SQRT_2 - 1.0;
        if !self.rails(kind) {
            return None;
        }
        let (dx, dy) = (displacement.dx.abs(), displacement.dy.abs());
        if !(dx.is_finite() && dy.is_finite()) || (dx == 0.0 && dy == 0.0) {
            return None;
        }
        if dy <= dx * RAIL {
            Some(Axis::Horizontal)
        } else if dx <= dy * RAIL {
            Some(Axis::Vertical)
        } else {
            None
        }
    }
}

/// Project `offset` onto `rail`, or leave it whole without one.
pub(crate) fn on_rail(
    offset: flui_foundation::geometry::Offset<f64>,
    rail: Option<flui_foundation::geometry::Axis>,
) -> flui_foundation::geometry::Offset<f64> {
    use flui_foundation::geometry::{Axis, Offset};
    match rail {
        Some(Axis::Horizontal) => Offset::new(offset.dx, 0.0),
        Some(Axis::Vertical) => Offset::new(0.0, offset.dy),
        None => offset,
    }
}

/// Which transform a scale gesture that begins as a pan keeps at rest.
///
/// A scale begins as a pan when its focal point crosses the pan slop before
/// its span or its angle changes past theirs. The locked component then holds
/// at identity, rotation `0.0` or scale `1.0`, until every contact lifts; a
/// gesture that begins as a pinch or a turn transforms freely (Compose
/// `lockRotationOnZoomPan`). Rotation is part of the scale gesture: there is
/// no separate rotation recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TransformLock {
    /// Every component follows the contacts.
    #[default]
    None,
    /// A gesture that begins as a pan does not rotate.
    Rotation,
    /// A gesture that begins as a pan does not scale.
    Scale,
}
