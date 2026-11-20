//! Where an app's window sits on a Windows screen, in the units the driver maps with, and the
//! normalized coordinates an absolute `SendInput` move takes.
//!
//! The lane runs per-monitor DPI aware, so every rectangle it reads is in device pixels and a
//! window's scale is its monitor's DPI over 96. The driver maps a surface-logical point with
//! `floor((origin + x) * scale)` (`coordinates::to_device`), whose origin is logical: a client
//! area at device column 101 on a 150 % monitor has the logical origin 67.33…, which `f64`
//! cannot hold, and `101 / 1.5 * 1.5` is just under 101. [`logical_origin`] lifts the origin by
//! a millionth of a device pixel so it maps back to the column it came from; a point the lift
//! moves lies within that millionth of a column edge.

use flui_protocol::geometry::{LogicalPoint, LogicalSize};

use crate::coordinates::{DevicePoint, WindowGeometry};

/// The DPI of a monitor at 100 %.
pub const BASE_DPI: u32 = 96;

/// How far past a device-pixel edge [`logical_origin`] lifts an origin, in device pixels.
const LIFT: f64 = 1e-6;

/// How far a client area's device size may be from the app's logical size times its scale: the
/// OS rounds a window to whole device pixels.
const SIZE_TOLERANCE: f64 = 1.0;

/// How far a monitor's scale may be from the app's.
const SCALE_TOLERANCE: f64 = 1e-3;

/// The scale of a monitor at `dpi`.
#[must_use]
pub fn scale_of(dpi: u32) -> f64 {
    f64::from(dpi) / f64::from(BASE_DPI)
}

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

/// Whether a client area of `width` by `height` device pixels on a monitor at `dpi` is the window
/// the app reports as `size` at `scale`.
#[must_use]
pub fn is_window(width: i32, height: i32, dpi: u32, size: LogicalSize, scale: f64) -> bool {
    let monitor = scale_of(dpi);
    (monitor - scale).abs() <= SCALE_TOLERANCE
        && (f64::from(width) - size.width * scale).abs() <= SIZE_TOLERANCE
        && (f64::from(height) - size.height * scale).abs() <= SIZE_TOLERANCE
}

/// The normalized coordinate (0 to 65535) an absolute move over the virtual desktop takes to land
/// on device coordinate `at`, the desktop spanning `extent` pixels from `start` on that axis;
/// `None` for a point off the desktop. Windows scales a normalized coordinate `n` to
/// `floor(n * extent / 65536)`, so the smallest `n` at or past the pixel's left edge lands on it.
#[must_use]
pub fn normalized(at: i32, start: i32, extent: i32) -> Option<i32> {
    let offset = u64::try_from(i64::from(at) - i64::from(start)).ok()?;
    let extent = u64::try_from(extent).ok()?;
    if offset >= extent {
        return None;
    }
    let scaled = (offset * 65_536).div_ceil(extent);
    i32::try_from(scaled.min(65_535)).ok()
}
