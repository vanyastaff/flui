//! Pointer-input processing: One Euro filtering + impulse velocity tracking.
//!
//! Feeds a synthetic noisy drag-then-decelerate gesture through:
//!
//! 1. [`OneEuroFilter2D`] — speed-adaptive jitter removal (Casiez CHI 2012);
//! 2. [`VelocityEstimator::Impulse`] next to the default least-squares
//!    [`VelocityTracker`], comparing their estimates after a sharp deceleration.
//!
//! Run with: `cargo run -p flui-interaction --example pointer_filtering`

use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::gesture::{OneEuroFilter2D, VelocityEstimator, VelocityTracker};
use flui_interaction::{MonotonicClock, SystemClock};
use flui_platform_api::pointer::PointerKind;

fn main() {
    println!("FLUI pointer filtering example\n");

    // ------------------------------------------------------------------
    // 1. One Euro Filter: jittery slow hover, then a fast stroke.
    // ------------------------------------------------------------------
    println!("1. One Euro Filter (slow jitter suppressed, fast motion tracked):");
    let mut filter = OneEuroFilter2D::default();
    let t0 = SystemClock.now();
    let dt = Duration::from_millis(8); // ~120 Hz input

    println!("   slow phase (true position 100, ±1 px sensor jitter):");
    let mut t = t0;
    for i in 0..24 {
        let jitter = if i % 2 == 0 { 1.0 } else { -1.0 };
        let raw = Offset::new(100.0 + jitter, 50.0);
        let smoothed = filter.filter(t, raw).expect("finite slow-motion fixture");
        if i % 8 == 7 {
            println!("     raw x={:7.2}  filtered x={:7.2}", raw.dx, smoothed.dx);
        }
        t += dt;
    }

    println!("   fast phase (2000 px/s stroke — lag stays small):");
    let mut x = 100.0_f64;
    for i in 0..24 {
        x += 2000.0 * dt.as_secs_f64();
        let raw = Offset::new(x, 50.0);
        let smoothed = filter.filter(t, raw).expect("finite fast-motion fixture");
        if i % 8 == 7 {
            println!(
                "     raw x={:7.2}  filtered x={:7.2}  (lag {:5.2} px)",
                raw.dx,
                smoothed.dx,
                raw.dx - smoothed.dx
            );
        }
        t += dt;
    }

    // ------------------------------------------------------------------
    // 2. Impulse vs least-squares velocity on a decelerating release.
    // ------------------------------------------------------------------
    println!("\n2. Fling velocity after a sharp deceleration:");
    println!("   gesture: 4 intervals at 2000 px/s, then 5 intervals at 200 px/s\n");

    let mut impulse =
        VelocityTracker::with_estimator(PointerKind::Touch, VelocityEstimator::Impulse);
    let mut lsq = VelocityTracker::new();

    let mut pos = 0.0_f64;
    let mut t = SystemClock.now();
    for _ in 0..4 {
        impulse.add_position(t, Offset::new(pos, 0.0));
        lsq.add_position(t, Offset::new(pos, 0.0));
        pos += 20.0; // 20 px / 10 ms = 2000 px/s
        t += Duration::from_millis(10);
    }
    for _ in 0..6 {
        impulse.add_position(t, Offset::new(pos, 0.0));
        lsq.add_position(t, Offset::new(pos, 0.0));
        pos += 2.0; // 2 px / 10 ms = 200 px/s
        t += Duration::from_millis(10);
    }

    let impulse_v = impulse.velocity_at(t).pixels_per_second.dx;
    let lsq_v = lsq.velocity_at(t).pixels_per_second.dx;
    println!("   impulse:                   {impulse_v:8.1} px/s");
    println!("   least-squares:             {lsq_v:8.1} px/s");
    println!(
        "\n   These are estimates from the recent history, not the known final\n   \
         segment velocity of 200 px/s. Sharp deceleration can make a quadratic\n   \
         fit extrapolate a reversal; compare each model against your input traces."
    );
}
