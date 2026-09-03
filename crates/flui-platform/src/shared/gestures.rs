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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinch_passes_the_fraction_through_and_drops_nan() {
        assert!(matches!(pinch(0.1), Some(PointerGesture::Pinch(d)) if d == 0.1));
        assert!(matches!(pinch(-0.25), Some(PointerGesture::Pinch(d)) if d == -0.25));
        assert!(pinch(f64::NAN).is_none(), "a NaN magnification is dropped");
    }

    #[test]
    fn rotation_converts_ccw_degrees_to_cw_radians() {
        // A 90° counterclockwise platform delta is a -π/2 clockwise delta.
        let PointerGesture::Rotate(radians) = rotation_ccw_degrees(90.0) else {
            panic!("expected Rotate");
        };
        assert!((radians + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }
}
