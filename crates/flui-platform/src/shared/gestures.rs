//! The cross-backend trackpad-gesture contract, in pure functions.
//!
//! `PointerGesture`'s conventions — a pinch delta as a magnification
//! FRACTION (`0.1` = grow 10%), a rotation delta in CLOCKWISE RADIANS —
//! are normalized here, once, so the winit and AppKit boundaries cannot
//! drift apart. Both platforms happen to report the same raw shapes
//! (AppKit `magnification` is the fraction winit's `PinchGesture.delta`
//! forwards; AppKit `rotation` is the counterclockwise degrees winit's
//! `RotationGesture.delta` forwards), so each conversion lives in exactly
//! one function with the callers' unit tests executing on Linux.

use ui_events::pointer::PointerGesture;

/// The synthetic pointer identity shared by every trackpad-gesture tick.
///
/// Gestures arrive without a pointer id from either platform; the stream
/// gets one stable identity distinct from the mouse (`PointerId::PRIMARY`
/// is 1) and from any touch contact (allocated upward from 2) — the
/// binding's ephemeral no-contact dispatch keys on the id alone.
pub const GESTURE_POINTER_ID: u64 = u64::MAX;

/// A pinch tick from a platform magnification delta (the fraction both
/// AppKit and winit report). `None` for a NaN delta — winit documents NaN
/// as possible, and a NaN scale tick is meaningless to every consumer.
pub fn pinch(magnification: f64) -> Option<PointerGesture> {
    if magnification.is_nan() {
        return None;
    }
    #[expect(clippy::cast_possible_truncation)] // a magnification fraction is far inside f32 range
    Some(PointerGesture::Pinch(magnification as f32))
}

/// A rotation tick from the platforms' counterclockwise-degrees delta,
/// converted to the contract's clockwise radians.
pub fn rotation_ccw_degrees(degrees: f32) -> PointerGesture {
    PointerGesture::Rotate(-degrees.to_radians())
}
