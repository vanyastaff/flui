//! Trackpad pan, pinch and rotate as one gesture stream.

use flui_foundation::geometry::Offset;

use super::PointerInfo;
use super::value::{InputValueError, PointerPosition, Quantity, finite, within};
use crate::EventTime;
use crate::keyboard::Modifiers;

/// The pan, scale and rotation a trackpad gesture has accumulated **since its start**.
///
/// The values are cumulative, not per-event steps: an update replaces the previous one, and a
/// consumer that wants the step subtracts (or, for scale, divides by) the previous update. A
/// backend whose platform reports steps (AppKit, winit) sums the pan and rotation and
/// multiplies the scale; one that reports a cumulative transform (Windows Direct
/// Manipulation) passes it on. Cumulative values let a recognizer that joins mid-gesture see
/// the whole gesture, and do not drift the way a product of many rounded steps does.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Offset;
/// use flui_platform_api::pointer::PanZoomTransform;
///
/// let zoomed = PanZoomTransform::try_new(Offset::new(12.0, -4.0), 1.5, 0.1)?;
/// assert_eq!(zoomed.scale(), 1.5);
/// assert!(PanZoomTransform::try_new(Offset::new(0.0, 0.0), 0.0, 0.0).is_err());
/// # Ok::<(), flui_platform_api::pointer::InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanZoomTransform {
    pan: Offset<f64>,
    scale: f64,
    rotation: f64,
}

impl PanZoomTransform {
    /// No pan, no zoom, no rotation: where every gesture starts.
    pub const IDENTITY: Self = Self {
        pan: Offset::new(0.0, 0.0),
        scale: 1.0,
        rotation: 0.0,
    };

    /// The transform with this cumulative pan (logical pixels), scale factor and clockwise
    /// rotation (radians, not wrapped: two full turns are 4π).
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when a value is NaN or infinite, and
    /// [`InputValueError::OutOfRange`] for a scale that is not positive.
    pub fn try_new(pan: Offset<f64>, scale: f64, rotation: f64) -> Result<Self, InputValueError> {
        finite(Quantity::Pan, pan.dx)?;
        finite(Quantity::Pan, pan.dy)?;
        // Every positive finite scale, subnormals included: a cumulative zoom built
        // from many positive steps can get that small and is still valid.
        let smallest_positive = f64::from_bits(1);
        let scale = within(Quantity::Scale, scale, smallest_positive, f64::MAX)?;
        let rotation = finite(Quantity::Rotation, rotation)?;
        Ok(Self {
            pan,
            scale,
            rotation,
        })
    }

    /// How far the fingers have moved since the start, in logical pixels.
    #[must_use]
    pub const fn pan(self) -> Offset<f64> {
        self.pan
    }

    /// The zoom factor since the start: `1.0` is none, `2.0` twice as large. Always positive.
    #[must_use]
    pub const fn scale(self) -> f64 {
        self.scale
    }

    /// The rotation since the start, in radians, clockwise on screen (y points down).
    #[must_use]
    pub const fn rotation(self) -> f64 {
        self.rotation
    }
}

impl Default for PanZoomTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Where a trackpad gesture is.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PanZoomPhase {
    /// The fingers touched down; the transform is [`PanZoomTransform::IDENTITY`].
    Start,
    /// The gesture moved; the transform is cumulative since [`Start`](Self::Start).
    Update(PanZoomTransform),
    /// The fingers lifted. A consumer may fling from the gesture's velocity.
    End,
    /// The platform took the gesture back. A consumer ends it without a fling.
    Cancelled,
}

/// One event of a trackpad pan/zoom gesture.
///
/// A gesture is `Start`, any number of `Update`s, then `End` or `Cancelled`, all with the same
/// [`PointerInfo::id`], which no contact uses while the gesture runs. A two-finger scroll the
/// platform reports only as scrolling arrives as [`ScrollEvent`](super::ScrollEvent)s instead.
///
/// Built with [`PanZoomEvent::new`] and [`with_modifiers`](Self::with_modifiers).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PanZoomEvent {
    pointer: PointerInfo,
    /// When the platform observed the event.
    pub time: EventTime,
    /// The cursor's position: the focal point a zoom scales about.
    pub position: PointerPosition,
    /// Where the gesture is, with its cumulative transform.
    pub phase: PanZoomPhase,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

impl PanZoomEvent {
    /// A gesture event at `position` with no modifiers. The pointer's kind is set to
    /// [`PointerKind::Trackpad`](super::PointerKind::Trackpad) whatever `pointer` carried, so
    /// the event cannot classify itself two ways.
    #[must_use]
    pub const fn new(
        pointer: PointerInfo,
        time: EventTime,
        position: PointerPosition,
        phase: PanZoomPhase,
    ) -> Self {
        let mut pointer = pointer;
        pointer.kind = super::PointerKind::Trackpad;
        Self {
            pointer,
            time,
            position,
            phase,
            modifiers: Modifiers::NONE,
        }
    }

    /// This event with `modifiers` held.
    #[must_use]
    pub const fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// The gesture's pointer; its kind is always
    /// [`PointerKind::Trackpad`](super::PointerKind::Trackpad).
    #[must_use]
    pub const fn pointer(&self) -> &PointerInfo {
        &self.pointer
    }
}
