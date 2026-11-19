//! From the app's surface-logical points to the screen's device pixels an OS lane injects at.
//!
//! The agent answers rects in the window's surface, in logical pixels from its top-left, and
//! `window.placement` says where that surface sits on screen and at what scale. A lane maps a
//! point with [`to_device`], one pure function: arithmetic in `f64`, every intermediate checked
//! finite, the result checked to fit the `i32` an OS takes. A point outside the window's surface
//! is refused as [`Error::OutsideTarget`], never clamped into it: a clamped point lands on
//! something the caller did not aim at.
//!
//! The surface's pixel grid is half-open: a point at `x` covers the device column
//! `floor((origin + x) * scale)`, so `0 <= x < width` maps inside the window's device columns and
//! `x == width` is past its right edge. Mapping is direction-free: an RTL window lays its content
//! out mirrored, but the rects it publishes are already measured from the surface's physical left,
//! so the same function maps both.

use flui_protocol::geometry::{LogicalPoint, LogicalRect, LogicalSize};

use crate::error::{Error, Result};

/// A point on screen in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DevicePoint {
    /// Rightwards.
    pub x: i32,
    /// Downwards.
    pub y: i32,
}

impl DevicePoint {
    /// The point `(x, y)`.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// The monitor a window is on, where the OS gives one: its top-left in the screen's logical space
/// and in device pixels. Monitors of different scales meet at device pixels, not logical ones, so
/// a window on a secondary monitor maps through its monitor's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    /// The monitor's top-left in screen logical pixels.
    pub logical_origin: LogicalPoint,
    /// The monitor's top-left in screen device pixels.
    pub device_origin: DevicePoint,
}

/// Where a window's surface sits on screen: what `window.placement` answers, with the monitor
/// where the OS reports one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// The surface's top-left on screen, in logical pixels; negative left of or above the
    /// primary monitor.
    pub origin: LogicalPoint,
    /// The surface's size in logical pixels.
    pub size: LogicalSize,
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// The monitor the window is on.
    pub monitor: Option<Monitor>,
}

impl WindowGeometry {
    /// A window at `origin` of `size` at `scale`, its monitor unknown.
    #[must_use]
    pub const fn new(origin: LogicalPoint, size: LogicalSize, scale: f64) -> Self {
        Self {
            origin,
            size,
            scale,
            monitor: None,
        }
    }

    /// On `monitor`.
    #[must_use]
    pub const fn on_monitor(mut self, monitor: Monitor) -> Self {
        self.monitor = Some(monitor);
        self
    }
}

/// The screen device pixel of the surface-logical `point` of the window `geometry` describes.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a non-finite input or a scale that is not positive and finite;
/// [`Error::OutsideTarget`] for a point outside the window's surface, or one whose device pixel
/// does not fit an `i32`.
pub fn to_device(geometry: &WindowGeometry, point: LogicalPoint) -> Result<DevicePoint> {
    let scale = geometry.scale;
    if !(scale.is_finite() && scale > 0.0) {
        return Err(invalid(format!(
            "the window's scale {scale} is not a positive number"
        )));
    }
    let inputs = [
        point.x,
        point.y,
        geometry.origin.x,
        geometry.origin.y,
        geometry.size.width,
        geometry.size.height,
    ];
    if inputs.iter().any(|value| !value.is_finite()) {
        return Err(invalid(format!(
            "a coordinate is not finite: point ({}, {}) in a window at ({}, {}) of {} x {}",
            point.x,
            point.y,
            geometry.origin.x,
            geometry.origin.y,
            geometry.size.width,
            geometry.size.height
        )));
    }
    let inside = (0.0..geometry.size.width).contains(&point.x)
        && (0.0..geometry.size.height).contains(&point.y);
    if !inside {
        return Err(Error::OutsideTarget {
            message: format!(
                "({}, {}) is outside the window's surface of {} x {}",
                point.x, point.y, geometry.size.width, geometry.size.height
            ),
        });
    }
    let (base, logical_origin) = match geometry.monitor {
        Some(monitor) => (monitor.device_origin, monitor.logical_origin),
        None => (DevicePoint::new(0, 0), LogicalPoint::new(0.0, 0.0)),
    };
    let x = axis(base.x, geometry.origin.x - logical_origin.x, point.x, scale)?;
    let y = axis(base.y, geometry.origin.y - logical_origin.y, point.y, scale)?;
    Ok(DevicePoint::new(x, y))
}

/// The device pixel of the centre of `rect`'s part inside the window's surface: where a lane aims
/// a tap at an element.
///
/// # Errors
///
/// [`Error::OutsideTarget`] when no part of `rect` is inside the surface; otherwise as
/// [`to_device`].
pub fn visible_centre_to_device(
    geometry: &WindowGeometry,
    rect: LogicalRect,
) -> Result<DevicePoint> {
    let left = rect.x.max(0.0);
    let top = rect.y.max(0.0);
    let right = (rect.x + rect.width).min(geometry.size.width);
    let bottom = (rect.y + rect.height).min(geometry.size.height);
    if !(left < right && top < bottom) {
        return Err(Error::OutsideTarget {
            message: format!(
                "the rect at ({}, {}) of {} x {} shows no part inside the window's surface",
                rect.x, rect.y, rect.width, rect.height
            ),
        });
    }
    to_device(
        geometry,
        LogicalPoint::new(left + (right - left) / 2.0, top + (bottom - top) / 2.0),
    )
}

/// One axis: `base + floor((origin + offset) * scale)`, every step checked.
fn axis(base: i32, origin: f64, offset: f64, scale: f64) -> Result<i32> {
    let scaled = ((origin + offset) * scale).floor();
    let device = f64::from(base) + scaled;
    let fits = device.is_finite() && device >= f64::from(i32::MIN) && device <= f64::from(i32::MAX);
    if !fits {
        return Err(Error::OutsideTarget {
            message: format!(
                "the point maps to device pixel {device}, past what the screen addresses"
            ),
        });
    }
    // `device` is a whole number checked inside `i32`'s range above.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a whole number checked inside i32's range"
    )]
    Ok(device as i32)
}

fn invalid(message: String) -> Error {
    Error::InvalidArgument { message }
}
