//! Frame-rate-independent smoothing: `exp_decay` vs `SmoothDamp`.
//!
//! Simulates a "handle follows target" interaction at two different frame
//! rates and shows that both followers land in the same place after the same
//! wall-clock time — the property the naive `lerp(a, b, 0.1)`-per-frame
//! pattern does NOT have.
//!
//! Run with: `cargo run -p flui-animation --example smoothing_follow`

use flui_animation::smoothing::{SmoothDamp, Smoothed, exp_decay_half_life};

fn main() {
    println!("FLUI smoothing example\n");

    // 1. The frame-rate trap: same "lerp factor" at different frame rates
    //    diverges; exp_decay does not.
    println!("1. One second of following target=100 from 0:");
    for (label, fps) in [("30 fps", 30u32), ("120 fps", 120u32)] {
        let dt = 1.0 / fps as f32;

        // Broken pattern: x += (target - x) * 0.05 per FRAME.
        let mut broken = 0.0_f32;
        // Correct pattern: exponential decay with a 250 ms half-life.
        let mut correct = 0.0_f32;
        for _ in 0..fps {
            broken += (100.0 - broken) * 0.05;
            correct = exp_decay_half_life(correct, 100.0, 0.25, dt);
        }
        println!("   {label:>7}: per-frame lerp = {broken:6.2}   exp_decay = {correct:6.2}");
    }
    println!("   -> per-frame lerp depends on the frame count; exp_decay only on elapsed time.\n");

    // 2. Smoothed: stateful wrapper for a moving target.
    println!("2. Smoothed cursor with a retarget mid-flight (120 fps):");
    let mut cursor = Smoothed::new(0.0, 0.1); // 100 ms half-life
    cursor.set_target(80.0);
    for frame in 0..36 {
        if frame == 18 {
            cursor.set_target(20.0); // user changed direction
            println!("   -- retarget to 20 --");
        }
        let v = cursor.tick(1.0 / 120.0);
        if frame % 6 == 5 {
            println!(
                "   t={:>4.0} ms  value={v:6.2}",
                (frame + 1) as f32 * 1000.0 / 120.0
            );
        }
    }
    println!();

    // 3. SmoothDamp: carries velocity, so motion is C1-continuous and never
    //    overshoots — the right tool for camera/scroll-indicator follow.
    println!("3. SmoothDamp follow with a max speed of 300 units/s:");
    let mut damp = SmoothDamp::new(0.25).with_max_speed(300.0);
    let mut pos = 0.0_f32;
    for frame in 0..48 {
        pos = damp.step(pos, 100.0, 1.0 / 120.0);
        if frame % 12 == 11 {
            println!(
                "   t={:>4.0} ms  pos={pos:6.2}  velocity={:7.2}/s",
                (frame + 1) as f32 * 1000.0 / 120.0,
                damp.velocity()
            );
        }
    }
    println!("\nDone.");
}
