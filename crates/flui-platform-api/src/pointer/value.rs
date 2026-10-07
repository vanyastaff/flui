//! Validated pointer quantities: each newtype holds a value in its documented range, so an
//! event built from them cannot carry NaN, infinity or an out-of-range reading.
//!
//! The policy is the same for every type:
//!
//! - **NaN or an infinity** is refused with [`InputValueError::NonFinite`].
//! - **A finite value outside a bounded range** (pressure, tangential pressure, altitude, a
//!   negative contact side, a non-positive scale) is refused with
//!   [`InputValueError::OutOfRange`] by `try_new`. Where platforms are known to overshoot a
//!   normalized range by rounding, a `saturating` constructor clamps finite values instead.
//! - **A periodic angle** (azimuth, twist) has no range to violate: any finite value is
//!   wrapped into `[0, 2π)`.

use core::f64::consts::{FRAC_PI_2, TAU};

use flui_foundation::geometry::{Point, Size};

/// The quantity a rejected input value was meant to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Quantity {
    /// A pointer position.
    Position,
    /// A normalized contact pressure.
    Pressure,
    /// A normalized barrel-control pressure.
    TangentialPressure,
    /// A pen's altitude above the surface.
    Altitude,
    /// A pen's azimuth.
    Azimuth,
    /// A pen's rotation about its axis.
    Twist,
    /// A contact's extent.
    ContactSize,
    /// A scroll distance.
    ScrollDelta,
    /// A trackpad gesture's pan.
    Pan,
    /// A trackpad gesture's scale.
    Scale,
    /// A trackpad gesture's rotation.
    Rotation,
}

/// Why an input value was refused at the platform boundary.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum InputValueError {
    /// The value was NaN or an infinity.
    #[error("{quantity:?} is not finite")]
    NonFinite {
        /// What the value was meant to be.
        quantity: Quantity,
    },
    /// The value was finite but outside the quantity's range.
    #[error("{quantity:?} {value} is outside [{min}, {max}]")]
    OutOfRange {
        /// What the value was meant to be.
        quantity: Quantity,
        /// The refused value.
        value: f64,
        /// The smallest accepted value.
        min: f64,
        /// The largest accepted value.
        max: f64,
    },
}

pub(super) fn finite(quantity: Quantity, value: f64) -> Result<f64, InputValueError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(InputValueError::NonFinite { quantity })
    }
}

pub(super) fn within(
    quantity: Quantity,
    value: f64,
    min: f64,
    max: f64,
) -> Result<f64, InputValueError> {
    let value = finite(quantity, value)?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(InputValueError::OutOfRange {
            quantity,
            value,
            min,
            max,
        })
    }
}

/// `angle` wrapped into `[0, 2π)`.
fn wrap_turn(angle: f64) -> f64 {
    let wrapped = angle.rem_euclid(TAU);
    // `rem_euclid` rounds a tiny negative angle up to exactly 2π.
    if wrapped >= TAU { 0.0 } else { wrapped }
}

/// Where a pointer is, in the logical pixels of the window the event is delivered to
/// (ADR-0098): a point whose coordinates are finite.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Point;
/// use flui_platform_api::pointer::{InputValueError, PointerPosition, Quantity};
///
/// let position = PointerPosition::try_new(Point::new(10.5, 20.0))?;
/// assert_eq!(position.get(), Point::new(10.5, 20.0));
/// assert_eq!(
///     PointerPosition::try_new(Point::new(f64::NAN, 0.0)),
///     Err(InputValueError::NonFinite { quantity: Quantity::Position })
/// );
/// # Ok::<(), InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerPosition(Point<f64>);

impl PointerPosition {
    /// The position `point`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when a coordinate is NaN or infinite.
    pub fn try_new(point: Point<f64>) -> Result<Self, InputValueError> {
        finite(Quantity::Position, point.x)?;
        finite(Quantity::Position, point.y)?;
        Ok(Self(point))
    }

    /// The point, in logical pixels.
    #[must_use]
    pub const fn get(self) -> Point<f64> {
        self.0
    }
}

impl TryFrom<Point<f64>> for PointerPosition {
    type Error = InputValueError;

    fn try_from(point: Point<f64>) -> Result<Self, Self::Error> {
        Self::try_new(point)
    }
}

impl From<PointerPosition> for Point<f64> {
    fn from(position: PointerPosition) -> Self {
        position.0
    }
}

/// A normalized contact pressure in `[0, 1]`: 0 is no pressure, 1 the most the device
/// reports. A hovering pen with a sensor reads 0; a device without one has no `Pressure` at
/// all.
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::Pressure;
///
/// assert_eq!(Pressure::try_new(0.25).map(Pressure::get), Ok(0.25));
/// assert!(Pressure::try_new(1.2).is_err());
/// // A device that rounds slightly past full scale:
/// assert_eq!(Pressure::saturating(1.0001).map(Pressure::get), Ok(1.0));
/// assert!(Pressure::saturating(f32::NAN).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Pressure(f32);

impl Pressure {
    /// The pressure `value`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] for NaN or an infinity, and
    /// [`InputValueError::OutOfRange`] outside `[0, 1]`.
    pub fn try_new(value: f32) -> Result<Self, InputValueError> {
        within(Quantity::Pressure, f64::from(value), 0.0, 1.0)?;
        Ok(Self(value))
    }

    /// The pressure `value` clamped into `[0, 1]`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] for NaN or an infinity.
    pub fn saturating(value: f32) -> Result<Self, InputValueError> {
        finite(Quantity::Pressure, f64::from(value))?;
        Ok(Self(value.clamp(0.0, 1.0)))
    }

    /// The pressure, in `[0, 1]`.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A normalized barrel-control pressure in `[-1, 1]` (an airbrush wheel); 0 is the control's
/// neutral position.
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::TangentialPressure;
///
/// assert_eq!(TangentialPressure::try_new(-0.5).map(TangentialPressure::get), Ok(-0.5));
/// assert!(TangentialPressure::try_new(-1.5).is_err());
/// assert_eq!(TangentialPressure::saturating(-1.5).map(TangentialPressure::get), Ok(-1.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct TangentialPressure(f32);

impl TangentialPressure {
    /// The barrel pressure `value`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] for NaN or an infinity, and
    /// [`InputValueError::OutOfRange`] outside `[-1, 1]`.
    pub fn try_new(value: f32) -> Result<Self, InputValueError> {
        within(Quantity::TangentialPressure, f64::from(value), -1.0, 1.0)?;
        Ok(Self(value))
    }

    /// The barrel pressure `value` clamped into `[-1, 1]`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] for NaN or an infinity.
    pub fn saturating(value: f32) -> Result<Self, InputValueError> {
        finite(Quantity::TangentialPressure, f64::from(value))?;
        Ok(Self(value.clamp(-1.0, 1.0)))
    }

    /// The barrel pressure, in `[-1, 1]`.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// The angle of a pen relative to the surface: the W3C altitude and azimuth, in radians.
///
/// The altitude is between the surface (0) and perpendicular to it (π/2); the azimuth is the
/// direction the pen leans, clockwise from the window's positive x axis (x right, y down), in
/// `[0, 2π)`.
/// Each angle is optional because platforms can report altitude without azimuth or vice versa.
/// An orientation always contains at least one reported angle; an absent angle remains `None`.
///
/// # Examples
///
/// ```
/// use core::f64::consts::{FRAC_PI_2, FRAC_PI_4};
/// use flui_platform_api::pointer::PenOrientation;
///
/// let leaning = PenOrientation::try_new(FRAC_PI_4, -FRAC_PI_2)?;
/// assert_eq!(leaning.altitude(), Some(FRAC_PI_4));
/// assert_eq!(leaning.azimuth(), Some(3.0 * FRAC_PI_2)); // wrapped into [0, 2π)
/// let altitude_only = PenOrientation::try_altitude(0.0)?;
/// assert_eq!(altitude_only.altitude(), Some(0.0)); // a reported zero
/// assert_eq!(altitude_only.azimuth(), None); // no reading
/// assert!(PenOrientation::try_new(2.0, 0.0).is_err()); // past perpendicular
/// # Ok::<(), flui_platform_api::pointer::InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenOrientation {
    altitude: Option<f64>,
    azimuth: Option<f64>,
}

impl PenOrientation {
    /// The orientation with these angles, the azimuth wrapped into `[0, 2π)`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when either angle is NaN or infinite, and
    /// [`InputValueError::OutOfRange`] for an altitude outside `[0, π/2]`.
    pub fn try_new(altitude: f64, azimuth: f64) -> Result<Self, InputValueError> {
        let altitude = within(Quantity::Altitude, altitude, 0.0, FRAC_PI_2)?;
        let azimuth = wrap_turn(finite(Quantity::Azimuth, azimuth)?);
        Ok(Self {
            altitude: Some(altitude),
            azimuth: Some(azimuth),
        })
    }

    /// An altitude reading without an azimuth reading.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when the angle is NaN or infinite, and
    /// [`InputValueError::OutOfRange`] for an altitude outside `[0, π/2]`.
    pub fn try_altitude(altitude: f64) -> Result<Self, InputValueError> {
        let altitude = within(Quantity::Altitude, altitude, 0.0, FRAC_PI_2)?;
        Ok(Self {
            altitude: Some(altitude),
            azimuth: None,
        })
    }

    /// An azimuth reading without an altitude reading, wrapped into `[0, 2π)`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when the angle is NaN or infinite.
    pub fn try_azimuth(azimuth: f64) -> Result<Self, InputValueError> {
        let azimuth = wrap_turn(finite(Quantity::Azimuth, azimuth)?);
        Ok(Self {
            altitude: None,
            azimuth: Some(azimuth),
        })
    }

    /// The angle from the surface, in `[0, π/2]` radians, or `None` without a reading.
    #[must_use]
    pub const fn altitude(self) -> Option<f64> {
        self.altitude
    }

    /// The direction the pen leans, in `[0, 2π)` radians clockwise from the x axis,
    /// or `None` without a reading.
    #[must_use]
    pub const fn azimuth(self) -> Option<f64> {
        self.azimuth
    }
}

/// A pen's rotation about its own axis, in `[0, 2π)` radians clockwise.
///
/// # Examples
///
/// ```
/// use core::f64::consts::PI;
/// use flui_platform_api::pointer::Twist;
///
/// assert_eq!(Twist::try_new(-PI).map(Twist::radians), Ok(PI));
/// assert!(Twist::try_new(f64::INFINITY).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Twist(f64);

impl Twist {
    /// The rotation `radians`, wrapped into `[0, 2π)`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] for NaN or an infinity.
    pub fn try_new(radians: f64) -> Result<Self, InputValueError> {
        Ok(Self(wrap_turn(finite(Quantity::Twist, radians)?)))
    }

    /// The rotation, in `[0, 2π)` radians.
    #[must_use]
    pub const fn radians(self) -> f64 {
        self.0
    }
}

/// The extent of a contact (a fingertip's ellipse), in logical pixels: finite and not
/// negative on either side.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Size;
/// use flui_platform_api::pointer::ContactSize;
///
/// let fingertip = ContactSize::try_new(Size::new(12.0, 9.5))?;
/// assert_eq!(fingertip.get(), Size::new(12.0, 9.5));
/// assert!(ContactSize::try_new(Size::new(-1.0, 4.0)).is_err());
/// # Ok::<(), flui_platform_api::pointer::InputValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactSize(Size<f64>);

impl ContactSize {
    /// The contact extent `size`.
    ///
    /// # Errors
    ///
    /// [`InputValueError::NonFinite`] when a side is NaN or infinite, and
    /// [`InputValueError::OutOfRange`] when a side is negative.
    pub fn try_new(size: Size<f64>) -> Result<Self, InputValueError> {
        within(Quantity::ContactSize, size.width, 0.0, f64::MAX)?;
        within(Quantity::ContactSize, size.height, 0.0, f64::MAX)?;
        Ok(Self(size))
    }

    /// The extent, in logical pixels.
    #[must_use]
    pub const fn get(self) -> Size<f64> {
        self.0
    }
}

impl TryFrom<Size<f64>> for ContactSize {
    type Error = InputValueError;

    fn try_from(size: Size<f64>) -> Result<Self, Self::Error> {
        Self::try_new(size)
    }
}
