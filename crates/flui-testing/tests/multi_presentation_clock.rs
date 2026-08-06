//! The deterministic multi-presentation clock (issue #556) — the
//! headline capability `HeadlessBinding`'s single-view `pump_frame` cannot
//! provide on its own: each registered [`PresentationId`] gets its OWN
//! [`FrameClock`] over its OWN virtual clock and its OWN `Vsync` registry,
//! so a script can pump one presentation while a sibling sits untouched, or
//! drive two presentations at independent scripted cadences in one
//! interleaved script — the test Flutter's single-view `WidgetTester.pump`
//! cannot write.

use std::num::NonZeroU32;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus, UpdateScheduler};
use flui_foundation::PresentationId;
use flui_scheduler::DemandKind;
use flui_testing::HeadlessBinding;

fn presentation(index: u32) -> PresentationId {
    PresentationId::new_gen(index, NonZeroU32::MIN)
}

/// A controller long enough that it never completes within the pump counts
/// these tests use — so `has_running()` stays true and `Animation` demand is
/// freshly re-marked every single pump, making "N pumps -> N produces" exact.
fn long_running_controller() -> AnimationController {
    AnimationController::new(Duration::from_secs(10), &UpdateScheduler::new())
}

/// Frame-exact multi-window pump: pump A three frames while B is untouched
/// ⇒ A.produces == 3, B.produces == 0, and B's pending demand is still
/// latched — kills "advance leaks across presentations" and "manual clock
/// double-ticks".
#[test]
fn pumping_one_presentation_three_times_leaves_an_untouched_sibling_at_zero_with_its_demand_latched()
 {
    let mut binding = HeadlessBinding::new();
    let a = presentation(0);
    let b = presentation(1);

    let a_vsync = binding.install_presentation_clock(a);
    let _b_vsync = binding.install_presentation_clock(b);

    let a_controller = long_running_controller();
    a_vsync.register(a_controller.clone());
    a_controller.forward().expect("fresh controller forwards");

    // B has demand marked but is never pumped -- it must neither produce nor
    // lose that demand while A is driven.
    binding.mark_presentation_demand(b, DemandKind::Host);

    for _ in 0..3 {
        binding.pump_presentation(a, Duration::from_millis(16));
    }

    assert_eq!(
        binding.presentation_produced_count(a),
        3,
        "three pumps of A, each with the controller still running, must grant three produces"
    );
    assert_eq!(
        binding.presentation_produced_count(b),
        0,
        "B was never pumped -- it must not have produced anything"
    );

    // B's latched demand survives untouched: pumping it now (its first ever
    // pump) must produce immediately from that retained mark, not need a
    // fresh mark.
    binding.pump_presentation(b, Duration::from_millis(16));
    assert_eq!(
        binding.presentation_produced_count(b),
        1,
        "B's demand, marked before A was ever pumped, must still have been latched"
    );

    a_controller.dispose();
}

/// The test Flutter cannot write: A on a scripted 144 Hz cadence and B on
/// 60 Hz, advanced in one interleaved script ⇒ per-presentation tick counts
/// and animation values match their own cadences exactly.
#[test]
fn two_presentations_at_independent_scripted_cadences_tick_and_advance_independently() {
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
    let a_controller = AnimationController::new(Duration::from_secs(5), &UpdateScheduler::new());
    let b_controller = AnimationController::new(Duration::from_secs(5), &UpdateScheduler::new());
    a_vsync.register(a_controller.clone());
    b_vsync.register(b_controller.clone());
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

    let a_expected = (a_elapsed / 5.0).min(1.0) as f32;
    let b_expected = (b_elapsed / 5.0).min(1.0) as f32;
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

    a_controller.dispose();
    b_controller.dispose();
}

/// Determinism, with `now` genuinely LOAD-BEARING in the decision: a
/// throttle is configured so the produce/skip choice depends on how far
/// apart consecutive polls actually are, and the expectation is an
/// ABSOLUTE, hand-computed sequence — not merely "the two runs agree with
/// each other".
///
/// A bare `run() == run()` self-comparison with no throttle configured is
/// NOT sufficient to kill a stray wall-clock read inside `poll`: a `now`
/// read from the real OS clock in a tight loop is *consistently* wrong in
/// both runs (each run's sequence of real-clock reads is internally
/// self-consistent, just not equal to the scripted virtual timeline), so
/// `first == second` can still hold even though `poll` never looked at the
/// caller-supplied `now` at all. This is exactly why the equivalent
/// `flui-scheduler` unit tests that configure a throttle catch that mutant
/// and a bare self-comparison does not — see
/// `frame_clock.rs::tests::the_same_script_replayed_twice_matches_an_
/// absolute_throttled_sequence`, this test's unit-level twin.
#[test]
fn the_same_throttled_script_replayed_twice_matches_an_absolute_sequence() {
    /// Whether each of 5 scripted pumps of `id` produced, derived from
    /// `presentation_produced_count`'s running total (the only per-pump
    /// observable `HeadlessBinding`'s public API exposes) rather than a raw
    /// decision -- a produce increments the counter, a skip does not, so
    /// the delta sequence is exactly isomorphic to the produce/skip
    /// sequence itself.
    fn produce_sequence(binding: &HeadlessBinding, id: PresentationId) -> Vec<bool> {
        let mut sequence = Vec::new();
        let mut previous = binding.presentation_produced_count(id);
        for _ in 0..5 {
            binding.mark_presentation_demand(id, DemandKind::Dirty);
            binding.pump_presentation(id, Duration::from_millis(16));
            let now = binding.presentation_produced_count(id);
            sequence.push(now > previous);
            previous = now;
        }
        sequence
    }

    fn run() -> Vec<bool> {
        let mut binding = HeadlessBinding::new();
        let a = presentation(0);
        let _vsync = binding.install_presentation_clock(a);
        binding.set_presentation_min_produce_interval(a, Some(Duration::from_millis(33)));
        produce_sequence(&binding, a)
    }

    // Hand-computed against a 33ms throttle and a fixed 16ms step, from a
    // fresh clock (first poll always has capacity) -- the identical
    // reasoning as the flui-scheduler unit twin cited above:
    //   t=16ms: no prior produce                     -> produce (true)
    //   t=32ms: 32-16=16ms  < 33ms                    -> skip   (false)
    //   t=48ms: 48-16=32ms  < 33ms                    -> skip   (false)
    //   t=64ms: 64-16=48ms >= 33ms                    -> produce (true), last=64
    //   t=80ms: 80-64=16ms  < 33ms                    -> skip   (false)
    let expected = vec![true, false, false, true, false];

    let first = run();
    assert_eq!(
        first, expected,
        "the exact produce/skip sequence must match the scripted throttle timing -- a clock \
         reading real wall-clock time instead of the caller-supplied `now` would not reliably \
         reproduce this exact pattern"
    );

    let second = run();
    assert_eq!(
        first, second,
        "identical scripts must also produce identical traces run to run"
    );
}

/// No policy divergence: pumping through the deterministic multi-
/// presentation registry decides produce/skip via the exact same
/// `DemandMask`/`FrameClock::poll` code path as `ClockSource::Platform` —
/// this test proves the SAME empty-mask-skips / nonzero-mask-produces
/// contract holds through `HeadlessBinding`'s own API, not a second, laxer
/// produce path.
#[test]
fn an_installed_presentation_with_no_demand_and_no_running_controller_never_produces() {
    let mut binding = HeadlessBinding::new();
    let a = presentation(0);
    let _vsync = binding.install_presentation_clock(a);

    // Positive control FIRST: this binding's own produce path genuinely
    // works for `a` (not an uninstalled id, a broken install, or a dead
    // `pump_presentation` -- any of which would ALSO leave the idle
    // assertion below vacuously true).
    binding.mark_presentation_demand(a, DemandKind::Host);
    binding.pump_presentation(a, Duration::from_millis(16));
    assert_eq!(
        binding.presentation_produced_count(a),
        1,
        "sanity: this binding's produce path must work at all for `a`"
    );

    // Now the actual claim: ten more pumps with nothing marked and no
    // controller registered must grant exactly zero ADDITIONAL produces.
    for _ in 0..10 {
        binding.pump_presentation(a, Duration::from_millis(16));
    }

    assert_eq!(
        binding.presentation_produced_count(a),
        1,
        "no demand was ever marked and no controller was ever registered after the sanity \
         produce above -- ten more pumps must grant zero additional produces"
    );
}

/// A settled controller (naturally reaching `Completed`) stops re-arming
/// `Animation` demand — produces stop within one frame of settle, the same
/// demand-driven-idle invariant a sibling-under-a-hidden-presentation
/// scenario relies on.
#[test]
fn a_settled_controller_stops_producing_new_frames_once_completed() {
    let mut binding = HeadlessBinding::new();
    let a = presentation(0);
    let vsync = binding.install_presentation_clock(a);

    let controller = AnimationController::new(Duration::from_millis(50), &UpdateScheduler::new());
    vsync.register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    // Pump well past the 50ms duration -- the controller settles partway
    // through, and produces must stop once it does.
    for _ in 0..20 {
        binding.pump_presentation(a, Duration::from_millis(10));
    }
    assert_eq!(controller.status(), AnimationStatus::Completed);
    let produced_at_settle = binding.presentation_produced_count(a);

    for _ in 0..10 {
        binding.pump_presentation(a, Duration::from_millis(10));
    }
    assert_eq!(
        binding.presentation_produced_count(a),
        produced_at_settle,
        "a settled controller must not keep re-arming Animation demand -- produces must stop \
         growing once it completes"
    );

    controller.dispose();
}

/// Re-registering an id replaces its pair with a fresh one (documented
/// contract): a stale handle from before the re-install no longer reaches
/// anything this binding drives.
#[test]
fn reinstalling_a_presentation_id_starts_a_fresh_pair() {
    let mut binding = HeadlessBinding::new();
    let a = presentation(0);

    let first_vsync = binding.install_presentation_clock(a);
    let controller = long_running_controller();
    first_vsync.register(controller.clone());
    controller.forward().expect("fresh controller forwards");
    binding.pump_presentation(a, Duration::from_millis(16));
    assert_eq!(binding.presentation_produced_count(a), 1);

    // Re-install: a fresh clock, produced_count reset to zero.
    let _second_vsync = binding.install_presentation_clock(a);
    assert_eq!(
        binding.presentation_produced_count(a),
        0,
        "re-registering an id must start a fresh clock, not accumulate onto the old one"
    );

    // The OLD vsync handle still exists (the test holds it) and still ticks
    // its own controller if driven directly, but the BINDING no longer
    // drives it through `pump_presentation` -- only the fresh pair.
    binding.pump_presentation(a, Duration::from_millis(16));
    assert_eq!(
        binding.presentation_produced_count(a),
        0,
        "the fresh pair has no demand and no controller registered on it yet"
    );

    controller.dispose();
}
