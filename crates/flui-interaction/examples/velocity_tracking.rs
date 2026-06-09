//! Velocity estimation with the least-squares tracker.
//!
//! Feeds a synthetic horizontal swipe into a [`VelocityTracker`] and reads the
//! estimated velocity — the same estimator the drag/fling recognizers use.
//!
//! Run with:
//! ```text
//! cargo run -p flui-interaction --example velocity_tracking
//! ```

use std::time::{Duration, Instant};

use flui_interaction::processing::VelocityTracker;
use flui_types::{
    geometry::{Offset, Pixels},
    gestures::PointerDeviceKind,
};

fn main() {
    let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
    let start = Instant::now();

    // Horizontal swipe: 10 px every 10 ms == 1000 px/s.
    for i in 0..10 {
        let t = start + Duration::from_millis(i * 10);
        let x = i as f32 * 10.0;
        tracker.add_position(t, Offset::new(Pixels(x), Pixels(0.0)));
    }

    let velocity = tracker.get_velocity();
    println!(
        "estimated velocity: {:.1} px/s (x), {:.1} px/s (y)",
        velocity.pixels_per_second.dx.get(),
        velocity.pixels_per_second.dy.get(),
    );
    println!("magnitude: {:.1} px/s", velocity.magnitude());

    // The fit should recover ~1000 px/s along x; allow generous slack for the
    // short sample window.
    let speed_x = velocity.pixels_per_second.dx.get();
    assert!(
        (800.0..=1200.0).contains(&speed_x),
        "expected ~1000 px/s, got {speed_x:.1}"
    );
    println!("velocity tracking demo OK");
}
