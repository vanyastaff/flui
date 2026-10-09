//! The deterministic multi-presentation clock (issue #556) — the
//! headline capability `HeadlessBinding`'s single-view `pump_frame` cannot
//! provide on its own: each registered [`PresentationId`] gets its OWN
//! [`FrameClock`] over its OWN virtual clock and its OWN `Vsync` registry,
//! so a script can pump one presentation while a sibling sits untouched, or
//! drive two presentations at independent scripted cadences in one
//! interleaved script.

use std::num::NonZeroU32;
use std::time::Duration;

use flui_animation::{Animation, AnimationController};
use flui_foundation::PresentationId;
use flui_testing::HeadlessBinding;

fn presentation(index: u32) -> PresentationId {
    PresentationId::new_gen(index, NonZeroU32::MIN)
}

/// A on a scripted 144 Hz cadence and B on
/// 60 Hz, advanced in one interleaved script ⇒ per-presentation tick counts
/// and animation values match their own cadences exactly.
pub(crate) fn two_presentations_at_independent_scripted_cadences_tick_and_advance_independently() {
    let mut binding = HeadlessBinding::new();
    let a = presentation(0); // 144 Hz -> ~6.94ms/frame
    let b = presentation(1); // 60 Hz -> ~16.67ms/frame

    let a_vsync = binding.install_presentation_clock(a);
    let b_vsync = binding.install_presentation_clock(b);

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

    // Interleave: every "tick" of the script pumps BOTH, but each at its OWN
    // frame duration -- 100 ticks means A has advanced 100 * (1/144)s and B
    // has advanced 100 * (1/60)s, on their own independent virtual clocks.
    for _ in 0..100 {
        binding.pump_presentation(a, frame_144hz);
        binding.pump_presentation(b, frame_60hz);
    }

    assert_eq!(binding.presentation_produced_count(a), 100);
    assert_eq!(binding.presentation_produced_count(b), 100);

    let a_elapsed = frame_144hz.as_secs_f64() * 100.0;
    let b_elapsed = frame_60hz.as_secs_f64() * 100.0;
    assert!(
        a_elapsed < b_elapsed,
        "sanity: 100 frames at 144Hz must cover less virtual time than 100 at 60Hz"
    );

    let a_expected = (a_elapsed / 5.0).min(1.0);
    let b_expected = (b_elapsed / 5.0).min(1.0);
    // Tolerance wider than a bare rounding epsilon: the detection tick anchors
    // `t = 0` on the FIRST observed instant rather than true zero (one frame's
    // worth of slack), and `Duration::from_nanos(1_000_000_000 / rate)` itself
    // truncates a repeating fraction each frame -- both are integer/anchoring
    // artifacts of this test's own script, not an imprecise clock.
    let tolerance = 5e-3;
    assert!(
        (a_controller.value() - a_expected).abs() < tolerance,
        "A's value must match its OWN 144Hz-paced elapsed time (expected {a_expected}, got {})",
        a_controller.value()
    );
    assert!(
        (b_controller.value() - b_expected).abs() < tolerance,
        "B's value must match its OWN 60Hz-paced elapsed time (expected {b_expected}, got {})",
        b_controller.value()
    );
    assert!(
        b_controller.value() > a_controller.value(),
        "B (60Hz, more virtual time per tick) must be further along than A (144Hz) after the \
         same tick count -- a single shared clock could not produce this asymmetry"
    );

    drop(a_owner);
    drop(b_owner);
}

/// The binding's motion clock sets the rate its `Vsync` runs at against the
/// virtual clock: at double rate a 1 s run is half done after 250 ms, a
/// rate change mid-run continues from the current value, and a step on a
/// paused clock moves the run by exactly the step.
pub(crate) fn presentation_rates_pause_and_step_are_independent() {
    use flui_animation::PlaybackRate;
    let mut binding = HeadlessBinding::new();
    let a = presentation(0);
    let b = presentation(1);
    let a_vsync = binding.install_presentation_clock(a);
    let b_vsync = binding.install_presentation_clock(b);
    let a_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&a_vsync));
    let b_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&b_vsync));
    let a_run = a_owner.controller();
    let b_run = b_owner.controller();
    a_run.forward().expect("A starts");
    b_run.forward().expect("B starts");
    binding.pump_all(Duration::ZERO);
    binding
        .with_presentation_motion_clock(a, |clock| {
            clock.set_rate(PlaybackRate::new(2.0).expect("valid rate"));
        })
        .expect("A exists");
    binding
        .with_presentation_motion_clock(b, |clock| {
            clock.set_rate(PlaybackRate::new(0.5).expect("valid rate"));
        })
        .expect("B exists");
    binding.pump_all(Duration::from_millis(250));
    assert!((a_run.value() - 0.5).abs() < 1e-9);
    assert!((b_run.value() - 0.125).abs() < 1e-9);
    binding
        .with_presentation_motion_clock(a, |clock| clock.set_rate(PlaybackRate::PAUSED))
        .expect("A exists");
    let frames = binding.presentation_produced_count(a);
    binding.pump_presentation(a, Duration::from_secs(10));
    assert_eq!(a_run.value(), 0.5);
    assert_eq!(
        binding.presentation_produced_count(a),
        frames,
        "paused animation creates no frame demand"
    );
    binding.pump_presentation(b, Duration::from_millis(100));
    assert!(
        (b_run.value() - 0.175).abs() < 1e-9,
        "B continues at its own rate"
    );
    binding
        .with_presentation_motion_clock(a, |clock| clock.step(Duration::from_millis(100)))
        .expect("A exists");
    binding.pump_presentation(a, Duration::ZERO);
    assert!((a_run.value() - 0.6).abs() < 1e-9);
    assert_eq!(
        binding.presentation_produced_count(a),
        frames + 1,
        "step produces one paused frame"
    );
    binding.pump_presentation(a, Duration::ZERO);
    assert_eq!(
        binding.presentation_produced_count(a),
        frames + 1,
        "step does not restart continuous demand"
    );
    assert_eq!(
        binding.with_presentation_motion_clock(presentation(99), |_| ()),
        None
    );
}

pub(crate) fn the_motion_clock_rate_and_step_drive_the_binding_vsync() {
    use flui_animation::PlaybackRate;

    let mut binding = HeadlessBinding::new();
    let owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(binding.vsync()));
    let controller = owner.controller();
    controller.forward().expect("fresh controller forwards");
    binding.pump_frame(Duration::ZERO);

    binding
        .motion_clock_mut()
        .set_rate(PlaybackRate::new(2.0).expect("2 is a valid rate"));
    binding.pump_frame(Duration::from_millis(250));
    assert!(
        (controller.value() - 0.5).abs() < 1e-9,
        "double rate: {}",
        controller.value()
    );

    binding.motion_clock_mut().set_rate(PlaybackRate::PAUSED);
    binding.pump_frame(Duration::from_secs(10));
    assert!(
        (controller.value() - 0.5).abs() < 1e-9,
        "paused: {}",
        controller.value()
    );
    assert!(controller.is_animating(), "a paused run is still running");

    binding.motion_clock_mut().step(Duration::from_millis(100));
    binding.pump_frame(Duration::ZERO);
    assert!(
        (controller.value() - 0.6).abs() < 1e-9,
        "stepped: {}",
        controller.value()
    );

    binding.motion_clock_mut().set_rate(PlaybackRate::NORMAL);
    binding.pump_frame(Duration::from_millis(100));
    assert!(
        (controller.value() - 0.7).abs() < 1e-9,
        "resumed: {}",
        controller.value()
    );
}
