//! One reading of a pointer's position and sensors.

use super::value::{
    ContactSize, PenOrientation, PointerPosition, Pressure, TangentialPressure, Twist,
};
use crate::EventTime;

/// One reading of a pointer: where it was, when, and what its sensors reported.
///
/// A sensor a device does not have reads `None`, never a stand-in value: a mouse has no
/// pressure, so `pressure` is `None` for it, not the W3C's 0.5. A consumer that wants the W3C
/// default chooses it explicitly. Every field is a validated value ([`PointerPosition`],
/// [`Pressure`], …), so a sample holds no NaN, infinity or out-of-range reading.
///
/// Built with [`PointerSample::new`] and the `with_*` methods.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Point;
/// use flui_platform_api::EventTime;
/// use flui_platform_api::pointer::{PointerPosition, PointerSample, Pressure};
///
/// let position = PointerPosition::try_new(Point::new(4.0, 8.0))?;
/// let mouse = PointerSample::new(EventTime::from_nanos(10), position);
/// assert_eq!(mouse.pressure, None); // no sensor, not 0.5
///
/// let pen = mouse.with_pressure(Pressure::try_new(0.0)?);
/// assert_eq!(pen.pressure.map(Pressure::get), Some(0.0)); // a sensor reading zero
/// # Ok::<(), flui_platform_api::pointer::InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PointerSample {
    /// When the platform observed this reading.
    pub time: EventTime,
    /// Where the pointer was.
    pub position: PointerPosition,
    /// The contact pressure, or `None` for a device without a pressure sensor.
    pub pressure: Option<Pressure>,
    /// The barrel control's pressure, or `None` without one.
    pub tangential_pressure: Option<TangentialPressure>,
    /// The pen's reported altitude and/or azimuth, or `None` when neither is reported.
    pub orientation: Option<PenOrientation>,
    /// The pen's rotation about its own axis, or `None` when the device does not report it.
    pub twist: Option<Twist>,
    /// The contact's extent, or `None` when the device does not report it.
    pub contact_size: Option<ContactSize>,
}

impl PointerSample {
    /// A reading at `position` with no sensor values.
    #[must_use]
    pub const fn new(time: EventTime, position: PointerPosition) -> Self {
        Self {
            time,
            position,
            pressure: None,
            tangential_pressure: None,
            orientation: None,
            twist: None,
            contact_size: None,
        }
    }

    /// This reading with the contact pressure `pressure`.
    #[must_use]
    pub const fn with_pressure(self, pressure: Pressure) -> Self {
        Self {
            pressure: Some(pressure),
            ..self
        }
    }

    /// This reading with the barrel control's pressure `pressure`.
    #[must_use]
    pub const fn with_tangential_pressure(self, pressure: TangentialPressure) -> Self {
        Self {
            tangential_pressure: Some(pressure),
            ..self
        }
    }

    /// This reading with the pen at `orientation`.
    #[must_use]
    pub const fn with_orientation(self, orientation: PenOrientation) -> Self {
        Self {
            orientation: Some(orientation),
            ..self
        }
    }

    /// This reading with the pen rotated by `twist` about its own axis.
    #[must_use]
    pub const fn with_twist(self, twist: Twist) -> Self {
        Self {
            twist: Some(twist),
            ..self
        }
    }

    /// This reading with a contact of `size`.
    #[must_use]
    pub const fn with_contact_size(self, size: ContactSize) -> Self {
        Self {
            contact_size: Some(size),
            ..self
        }
    }
}
