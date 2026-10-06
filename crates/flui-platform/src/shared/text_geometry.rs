//! A text range's logical rect as the physical screen rect an input method
//! places its windows against (TSF `ITextStoreACP::GetTextExt`), and a
//! screen point as the logical point it asks about (`GetACPFromPoint`).
//!
//! Cfg-free so its table runs on every host; the Win32 text services are its
//! one consumer. The store answers in window-root logical pixels
//! (ADR-0030 §7); the screen wants whole physical pixels. Three rules decide
//! the conversion:
//!
//! - the scale is the one the caller passes in (the window's own scale
//!   factor, the same source pointer input uses), never a monitor query;
//! - the result covers the logical rect: the left and top edges take the
//!   floor and the right and bottom edges the ceiling, so a candidate window
//!   placed against the rect never overlaps a partly covered pixel of the
//!   text. `floor`, not truncation, so a negative coordinate (a field
//!   scrolled past the client origin, a window on a monitor left of the
//!   primary) moves outwards too;
//! - an edge closer than [`SNAP_TOLERANCE`] device pixels to a whole pixel is
//!   that pixel, so float noise from the multiplication (`0.1 × 3.0`) does
//!   not grow the rect by one.

use flui_foundation::geometry::{Bounds, DevicePixelRatio, DevicePoint, Point};

/// How far, in device pixels, an edge may sit from a whole pixel and still be
/// taken as that pixel.
pub const SNAP_TOLERANCE: f64 = 1e-6;

/// A rectangle in physical screen pixels, edges as Win32's `RECT` holds them:
/// `right` and `bottom` are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    /// The left edge.
    pub left: i32,
    /// The top edge.
    pub top: i32,
    /// The right edge, exclusive.
    pub right: i32,
    /// The bottom edge, exclusive.
    pub bottom: i32,
}

/// Why a logical rect has no screen rect. The text services answer it as
/// "no layout" (`TS_E_NOLAYOUT`), so an input method waits for the next
/// layout change instead of placing its window at a wrong position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScreenRectError {
    /// A coordinate or extent is NaN or infinite.
    #[error("the text rect is not finite")]
    NotFinite,
    /// An edge lies outside the `i32` range screen coordinates use.
    #[error("the text rect lies outside the screen coordinate range")]
    OutOfRange,
}

/// The physical screen rect covering `rect` (window-root logical pixels) in
/// a window whose client area starts at `client_origin` (physical screen
/// pixels) and whose scale is `ratio`.
///
/// A rect of negative size is taken by its edges, so the answer is never
/// inverted.
///
/// # Errors
///
/// [`ScreenRectError::NotFinite`] for a NaN or infinite input, and
/// [`ScreenRectError::OutOfRange`] when an edge does not fit an `i32`.
pub fn range_rect_to_screen(
    rect: Bounds<f64>,
    client_origin: DevicePoint,
    ratio: DevicePixelRatio,
) -> Result<ScreenRect, ScreenRectError> {
    let edges = [
        rect.origin.x,
        rect.origin.y,
        rect.origin.x + rect.size.width,
        rect.origin.y + rect.size.height,
    ];
    if edges.iter().any(|edge| !edge.is_finite()) {
        return Err(ScreenRectError::NotFinite);
    }
    let [x0, y0, x1, y1] = edges.map(|edge| ratio.to_device(edge));
    let left = outward(x0.min(x1), f64::floor) + f64::from(client_origin.x);
    let top = outward(y0.min(y1), f64::floor) + f64::from(client_origin.y);
    let right = outward(x0.max(x1), f64::ceil) + f64::from(client_origin.x);
    let bottom = outward(y0.max(y1), f64::ceil) + f64::from(client_origin.y);
    Ok(ScreenRect {
        left: whole_i32(left)?,
        top: whole_i32(top)?,
        right: whole_i32(right)?,
        bottom: whole_i32(bottom)?,
    })
}

/// The window-root logical point at `screen` (physical screen pixels) in a
/// window whose client area starts at `client_origin` and whose scale is
/// `ratio`: the inverse of [`range_rect_to_screen`]'s offset and scale, for
/// the point an input method asks about (TSF `GetACPFromPoint`).
///
/// Every pair of `i32` coordinates has a finite answer: their difference
/// is taken in `f64`, where it is exact, since it need not fit an `i32`.
#[must_use]
pub fn screen_point_to_client(
    screen: DevicePoint,
    client_origin: DevicePoint,
    ratio: DevicePixelRatio,
) -> Point<f64> {
    Point::new(
        ratio.to_logical(f64::from(screen.x) - f64::from(client_origin.x)),
        ratio.to_logical(f64::from(screen.y) - f64::from(client_origin.y)),
    )
}

/// `device` rounded outwards by `round`, unless it is within
/// [`SNAP_TOLERANCE`] of a whole pixel, which it then is.
fn outward(device: f64, round: fn(f64) -> f64) -> f64 {
    let nearest = device.round();
    if (device - nearest).abs() < SNAP_TOLERANCE {
        nearest
    } else {
        round(device)
    }
}

/// `value`, already whole, as an `i32`; `OutOfRange` outside it (and for an
/// infinity a huge scale produced).
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is whole and checked against the i32 range first"
)]
fn whole_i32(value: f64) -> Result<i32, ScreenRectError> {
    if !value.is_finite() {
        return Err(ScreenRectError::OutOfRange);
    }
    if value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(ScreenRectError::OutOfRange);
    }
    Ok(value as i32)
}
