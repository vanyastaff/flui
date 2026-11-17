//! Presentations at independent cadences: each is its own UI runtime, with
//! its own virtual clock and its own animation registry (ADR-0083, migration
//! move 6b), so one script can pump one while a sibling sits untouched, or
//! drive two at independent scripted cadences.

use std::time::Duration;

use flui_animation::{Animation, AnimationController, Vsync};
use flui_testing::widgets::{LaidOut, lay_out_animated, tight};
use flui_widgets::SizedBox;

/// A UI runtime that ticks `vsync` at every frame of its own clock.
fn presentation(vsync: &Vsync) -> LaidOut {
    lay_out_animated(SizedBox::new(10.0, 10.0), tight(10.0, 10.0), vsync.clone())
}

/// A on a scripted 144 Hz cadence and B on 60 Hz, advanced in one
/// interleaved script ⇒ each animation's value matches its own cadence, and
/// pumping one presentation leaves the other where it was.
pub(crate) fn two_presentations_at_independent_scripted_cadences_tick_and_advance_independently() {
    let a_vsync = Vsync::new();
    let b_vsync = Vsync::new();
    let mut a = presentation(&a_vsync); // 144 Hz -> ~6.94ms/frame
    let mut b = presentation(&b_vsync); // 60 Hz -> ~16.67ms/frame

    // Same nominal duration on both, long enough that NEITHER cadence
    // completes it within 100 pumps (B's slower 60Hz tick covers ~1.67s of
    // virtual time over 100 pumps -- 5s clears that with room to spare) --
    // any value difference below then traces purely to each presentation's
    // own advance cadence, not one of them settling early.
    let a_owner = AnimationController::builder(Duration::from_secs(5)).build_on(Some(&a_vsync));
    let b_owner = AnimationController::builder(Duration::from_secs(5)).build_on(Some(&b_vsync));
    let a_controller = a_owner.controller();
    let b_controller = b_owner.controller();
    a_controller.forward().expect("fresh controller forwards");
    b_controller.forward().expect("fresh controller forwards");

    let frame_144hz = Duration::from_nanos(1_000_000_000 / 144);
    let frame_60hz = Duration::from_nanos(1_000_000_000 / 60);
    let a_frames = a.scheduler().frame_count();
    let b_frames = b.scheduler().frame_count();

    // Interleave: every "tick" of the script pumps BOTH, but each at its OWN
    // frame duration -- 100 ticks means A has advanced 100 * (1/144)s and B
    // has advanced 100 * (1/60)s, on their own independent virtual clocks.
    for _ in 0..100 {
        a.pump_for(frame_144hz);
        b.pump_for(frame_60hz);
    }

    assert_eq!(
        a.scheduler().frame_count() - a_frames,
        100,
        "each of A's pumps is one frame of A's runtime"
    );
    assert_eq!(
        b.scheduler().frame_count() - b_frames,
        100,
        "each of B's pumps is one frame of B's runtime"
    );

    // The first frame after `forward` anchors the run at its own instant, so
    // 100 frames advance a run by 99 frame durations. The durations are whole
    // nanoseconds, so the expected progress is exact up to the controller's
    // f64 arithmetic.
    let progress = |frame: Duration| (frame * 99).as_secs_f64() / 5.0;
    let a_expected = progress(frame_144hz);
    let b_expected = progress(frame_60hz);
    assert!(
        (a_controller.value() - a_expected).abs() < 1e-9,
        "A's value must match its OWN 144Hz-paced elapsed time (expected {a_expected}, got {})",
        a_controller.value()
    );
    assert!(
        (b_controller.value() - b_expected).abs() < 1e-9,
        "B's value must match its OWN 60Hz-paced elapsed time (expected {b_expected}, got {})",
        b_controller.value()
    );
    assert!(
        b_controller.value() > a_controller.value(),
        "B (60Hz, more virtual time per tick) must be further along than A (144Hz) after the \
         same tick count -- a single shared clock could not produce this asymmetry"
    );

    // A sibling's frames move neither B's clock nor B's animation.
    let b_value = b_controller.value();
    let b_frames = b.scheduler().frame_count();
    let a_frames = a.scheduler().frame_count();
    for _ in 0..10 {
        a.pump_for(frame_144hz);
    }
    assert_eq!(a.scheduler().frame_count() - a_frames, 10);
    assert_eq!(
        b.scheduler().frame_count(),
        b_frames,
        "B runs no frame while only A is pumped"
    );
    assert_eq!(
        b_controller.value(),
        b_value,
        "B sits untouched while only A is pumped"
    );

    drop(a_owner);
    drop(b_owner);
}
