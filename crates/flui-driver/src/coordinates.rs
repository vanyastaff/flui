//! From the app's surface-logical points to the screen's device pixels an OS lane injects at.
//!
//! The agent answers rects and points in the window's surface, in logical pixels from its top-left,
//! and a lane locates that surface on its screen (`Lane::locate_window`). The OS path maps each
//! point with [`to_device`], one pure function: arithmetic in `f64`, every intermediate checked
//! finite, the result checked to fit the `i32` an OS takes. A point outside the window's surface
//! is refused as [`Error::OutsideTarget`], never clamped into it: a clamped point lands on
//! something the caller did not aim at. Whether the point is shown (not clipped, not covered) is
//! the actionability check's, made by the app before the point is chosen.
//!
//! The surface's pixel grid is half-open: a point at `x` covers the device column
//! `floor((origin + x) * scale)`, so `0 <= x < width` maps inside the window's device columns and
//! `x == width` is past its right edge. Mapping is direction-free: an RTL window lays its content
//! out mirrored, but the points it publishes are already measured from the surface's physical
//! left, so the same function maps both.

use flui_protocol::geometry::{LogicalPoint, LogicalSize};

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

/// Where a window's surface sits on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct WindowGeometry {
    /// The surface's top-left on screen, in logical pixels; negative left of or above the
    /// primary monitor.
    pub origin: LogicalPoint,
    /// The surface's size in logical pixels.
    pub size: LogicalSize,
    /// Device pixels per logical pixel.
    pub scale: f64,
}

impl WindowGeometry {
    /// A window at `origin` of `size` at `scale`.
    #[must_use]
    pub const fn new(origin: LogicalPoint, size: LogicalSize, scale: f64) -> Self {
        Self {
            origin,
            size,
            scale,
        }
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
        return Err(Error::invalid_argument(format!(
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
        return Err(Error::invalid_argument(format!(
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
        return Err(Error::outside_target(format!(
            "({}, {}) is outside the window's surface of {} x {}",
            point.x, point.y, geometry.size.width, geometry.size.height
        )));
    }
    let x = axis(geometry.origin.x, point.x, scale)?;
    let y = axis(geometry.origin.y, point.y, scale)?;
    Ok(DevicePoint::new(x, y))
}

/// One axis: `floor((origin + offset) * scale)`, every step checked.
fn axis(origin: f64, offset: f64, scale: f64) -> Result<i32> {
    let device = ((origin + offset) * scale).floor();
    let fits = device.is_finite() && device >= f64::from(i32::MIN) && device <= f64::from(i32::MAX);
    if !fits {
        return Err(Error::outside_target(format!(
            "the point maps to device pixel {device}, past what the screen addresses"
        )));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a whole number checked inside i32's range"
    )]
    Ok(device as i32)
}
