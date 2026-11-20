//! Where an app's window sits on a desktop whose pixels the lane maps its points with.
//!
//! The driver maps a surface-logical point with `floor((origin + x) * scale)`
//! (`coordinates::to_device`), whose origin is logical: a client area at device column 101 on a
//! 150 % monitor has the logical origin 67.33…, which `f64` cannot hold, and `101 / 1.5 * 1.5` is
//! just under 101. [`logical_origin`] lifts the origin by a millionth of a device pixel so it maps
//! back to the column it came from; a point the lift moves lies within that millionth of a column
//! edge.

use flui_protocol::geometry::{LogicalPoint, LogicalSize};

use crate::coordinates::{DevicePoint, WindowGeometry};

/// How far past a device-pixel edge [`logical_origin`] lifts an origin, in device pixels.
const LIFT: f64 = 1e-6;

/// The logical coordinate that maps back to device coordinate `device` at `scale`: see the
/// [module docs](self).
#[must_use]
pub fn logical_origin(device: i32, scale: f64) -> f64 {
    (f64::from(device) + LIFT) / scale
}

/// The geometry the driver maps a window's points with: its client area's top-left at
/// `client_origin` in device pixels, the app's logical `size`, at `scale`.
#[must_use]
pub fn window_geometry(
    client_origin: DevicePoint,
    size: LogicalSize,
    scale: f64,
) -> WindowGeometry {
    WindowGeometry::new(
        LogicalPoint::new(
            logical_origin(client_origin.x, scale),
            logical_origin(client_origin.y, scale),
        ),
        size,
        scale,
    )
}
