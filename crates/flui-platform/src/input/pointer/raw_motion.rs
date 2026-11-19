//! A locked pointer's motion.

use super::PointerSource;
use super::value::{InputValueError, PointerPosition, Quantity, finite};
use crate::EventTime;

/// How far a locked pointer's device moved since the last report: right and
/// down are positive.
///
/// # Examples
///
/// ```
/// use flui_platform::pointer::MotionDelta;
///
/// let nudge = MotionDelta::try_new(3.0, -1.5)?;
/// assert_eq!((nudge.x(), nudge.y()), (3.0, -1.5));
/// assert!(MotionDelta::try_new(f64::INFINITY, 0.0).is_err());
/// # Ok::<(), flui_platform::pointer::InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionDelta {
    x: f64,
    y: f64,
}

impl MotionDelta {
    /// A motion of `x` right and `y` down.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when either component is NaN or infinite.
    pub fn try_new(x: f64, y: f64) -> Result<Self, InputValueError> {
        Ok(Self {
            x: finite(Quantity::Motion, x)?,
            y: finite(Quantity::Motion, y)?,
        })
    }

    /// The motion to the right.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// The motion down.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }
}

/// Whether a [`RawMotion`]'s delta is the device's own motion or the one the
/// user's pointer speed and acceleration turned it into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a motion is reported before the system's pointer acceleration or after it; there is no third stage"
)]
pub enum MotionAcceleration {
    /// The device's motion before the system's acceleration, in the
    /// platform's own scale: the device's counts (Win32 raw input, X11's raw
    /// valuators) or unaccelerated pixels (a Wayland relative pointer, the
    /// web's unadjusted movement). What a 3D view that turns with the mouse
    /// wants.
    None,
    /// The motion after the user's pointer speed and acceleration, in logical
    /// pixels: what the platform reports where it has nothing rawer (AppKit's
    /// deltas, the web's movement where the browser cannot unadjust it).
    System,
}

/// The motion of a pointer the window holds in place
/// ([`CursorGrab::Locked`](crate::CursorGrab::Locked)): it arrives instead of
/// moves while the lock lasts, and only then.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RawMotion {
    /// The pointer that moved.
    pub pointer: PointerSource,
    /// When the platform observed the motion.
    pub time: EventTime,
    /// Where the locked pointer rests, in the window's logical pixels: what
    /// the motion is hit-tested at; `None` where the platform does not say.
    pub position: Option<PointerPosition>,
    /// How far the device moved.
    pub delta: MotionDelta,
    /// Whether that is before or after the system's pointer acceleration.
    pub acceleration: MotionAcceleration,
}

impl RawMotion {
    /// A motion of `delta` from `pointer` at `time`, with no resting position.
    #[must_use]
    pub const fn new(
        pointer: PointerSource,
        time: EventTime,
        delta: MotionDelta,
        acceleration: MotionAcceleration,
    ) -> Self {
        Self {
            pointer,
            time,
            position: None,
            delta,
            acceleration,
        }
    }

    /// This motion of a pointer resting at `position`.
    #[must_use]
    pub const fn with_position(self, position: PointerPosition) -> Self {
        Self {
            position: Some(position),
            ..self
        }
    }
}
