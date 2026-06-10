//! Pointer-input processing: One Euro filtering + impulse velocity tracking.
//!
//! Feeds a synthetic noisy drag-then-decelerate gesture through:
//!
//! 1. [`OneEuroFilter2D`] — speed-adaptive jitter removal (Casiez CHI 2012);
//! 2. [`ImpulseVelocityTracker`] (Android's default fling strategy) next to
//!    the least-squares [`VelocityTracker`] (Flutter's strategy), showing how
//!    the impulse model discounts stale samples after a sharp deceleration.
//!
//! Run with: `cargo run -p flui-interaction --example pointer_filtering`

use std::time::{Duration, Instant};

use flui_interaction::processing::{ImpulseVelocityTracker, OneEuroFilter2D, VelocityTracker};
use flui_types::geometry::{Offset, Pixels};

fn main() {
    println!("FLUI pointer filtering example\n");

    // ------------------------------------------------------------------
    // 1. One Euro Filter: jittery slow hover, then a fast stroke.
    // ------------------------------------------------------------------
    println!("1. One Euro Filter (slow jitter suppressed, fast motion tracked):");
    let mut filter = OneEuroFilter2D::default();
    let t0 = Instant::now();
    let dt = Duration::from_millis(8); // ~120 Hz input

    println!("   slow phase (true position 100, ±1 px sensor jitter):");
    let mut t = t0;
    for i in 0..24 {
        let jitter = if i % 2 == 0 { 1.0 } else { -1.0 };
        let raw = Offset::new(Pixels(100.0 + jitter), Pixels(50.0));
        let smoothed = filter.filter(t, raw);
        if i % 8 == 7 {
            println!(
                "     raw x={:7.2}  filtered x={:7.2}",
                raw.dx.get(),
                smoothed.dx.get()
            );
        }
        t += dt;
    }

    println!("   fast phase (2000 px/s stroke — lag stays small):");
    let mut x = 100.0_f32;
    for i in 0..24 {
        x += 2000.0 * dt.as_secs_f32();
        let raw = Offset::new(Pixels(x), Pixels(50.0));
        let smoothed = filter.filter(t, raw);
        if i % 8 == 7 {
            println!(
                "     raw x={:7.2}  filtered x={:7.2}  (lag {:5.2} px)",
                raw.dx.get(),
                smoothed.dx.get(),
                raw.dx.get() - smoothed.dx.get()
            );
        }
        t += dt;
    }

    // ------------------------------------------------------------------
    // 2. Impulse vs least-squares velocity on a decelerating release.
    // ------------------------------------------------------------------
    println!("\n2. Fling velocity after a sharp deceleration:");
    println!("   gesture: 4 intervals at 2000 px/s, then 5 intervals at 200 px/s\n");

    let mut impulse = ImpulseVelocityTracker::default();
    let mut lsq = VelocityTracker::new();

    let mut pos = 0.0_f32;
    let mut t = Instant::now();
    for _ in 0..4 {
        impulse.add_position(t, Offset::new(Pixels(pos), Pixels(0.0)));
        lsq.add_position(t, Offset::new(Pixels(pos), Pixels(0.0)));
        pos += 20.0; // 20 px / 10 ms = 2000 px/s
        t += Duration::from_millis(10);
    }
    for _ in 0..6 {
        impulse.add_position(t, Offset::new(Pixels(pos), Pixels(0.0)));
        lsq.add_position(t, Offset::new(Pixels(pos), Pixels(0.0)));
        pos += 2.0; // 2 px / 10 ms = 200 px/s
        t += Duration::from_millis(10);
    }

    let impulse_v = impulse.get_velocity().pixels_per_second.dx.get();
    let lsq_v = lsq.get_velocity().pixels_per_second.dx.get();
    println!("   impulse (Android default): {impulse_v:8.1} px/s");
    println!("   least-squares (Flutter):   {lsq_v:8.1} px/s");
    println!(
        "\n   The impulse model weights each interval by the velocity CHANGE it\n   \
         represents, so the release velocity tracks the finger's final intent\n   \
         instead of averaging the whole window."
    );
}
