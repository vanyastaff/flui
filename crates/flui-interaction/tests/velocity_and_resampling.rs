//! Numerical contracts of the input-processing layer: velocity estimates are
//! finite and bounded, the "pointer stopped" gate reads the sample timeline,
//! fling clamping keeps direction, and the resampler keeps every terminal
//! event, interpolates at the right factor and stamps events with their own
//! time.

use std::{cell::RefCell, rc::Rc, time::Duration};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerEvent, PointerInfo, PointerKind, PointerPosition, ScrollDelta, ScrollEvent,
    make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    pointer::ScrollUnit,
};
use flui_interaction::processing::{
    ImpulseVelocityTracker, IosFlingVelocityTracker, MacosFlingVelocityTracker,
    PointerEventResampler, SamplingClock, VelocityTracker,
};
use flui_interaction::{
    DEFAULT_MAX_FLING_VELOCITY, GestureBinding, GestureSettings, GestureSettingsError,
    HitTestResult, PointerId, PointerRouteHandler, Velocity, VelocityEstimate,
};
use proptest::prelude::*;
use web_time::Instant;

// ----------------------------------------------------------------------------
// Shared fixtures
// ----------------------------------------------------------------------------

/// Run each named case, collecting every failure (with its message) before
/// failing once, so one broken row does not hide the others.
fn run_rows(family: &str, rows: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for (name, row) in rows {
        if let Err(payload) = std::panic::catch_unwind(*row) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            failures.push(format!("{name}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{family} rows failed:\n{}",
        failures.join("\n")
    );
}

/// A fixed origin far enough from the process start that negative offsets
/// stay representable.
fn origin() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn ms(value: f64) -> Duration {
    Duration::from_secs_f64(value / 1000.0)
}

fn assert_bounded(estimate: Option<VelocityEstimate>, what: &str) {
    let Some(estimate) = estimate else { return };
    assert!(
        estimate.is_valid(),
        "{what}: estimate must be finite with confidence in [0, 1]: {estimate:?}"
    );
    assert!(
        estimate.magnitude() <= DEFAULT_MAX_FLING_VELOCITY * (1.0 + 1e-9),
        "{what}: speed {} exceeds the fling bound",
        estimate.magnitude()
    );
}

fn contact() -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(7).expect("non-zero pointer id"))
}

fn with_time(mut event: PointerEvent, nanos: u64) -> PointerEvent {
    match &mut event {
        PointerEvent::Down(button) => {
            button.sample.time = flui_platform_api::EventTime::from_nanos(nanos)
        }
        PointerEvent::Up(button) => {
            button.sample.time = flui_platform_api::EventTime::from_nanos(nanos)
        }
        PointerEvent::Move(update) => {
            let mut sample = *update.current();
            sample.time = flui_platform_api::EventTime::from_nanos(nanos);
            *update =
                flui_interaction::events::PointerMove::new(update.pointer, update.buttons, sample)
                    .with_modifiers(update.modifiers)
                    .with_coalesced(update.coalesced().to_vec())
                    .with_predicted(update.predicted().to_vec());
        }
        _ => {}
    }
    event
}

fn move_to(x: f64) -> PointerEvent {
    make_move_event_for_id(contact(), Offset::new(x, 0.0), PointerKind::Touch)
        .expect("valid fixture sample")
}

fn event_time(event: &PointerEvent) -> Option<u64> {
    match event {
        PointerEvent::Down(button) => Some(button.sample.time.as_nanos()),
        PointerEvent::Up(button) => Some(button.sample.time.as_nanos()),
        PointerEvent::Move(update) => Some(update.current().time.as_nanos()),
        _ => None,
    }
}

fn event_x(event: &PointerEvent) -> Option<f64> {
    match event {
        PointerEvent::Down(button) => Some(button.sample.position.get().x),
        PointerEvent::Up(button) => Some(button.sample.position.get().x),
        PointerEvent::Move(update) => Some(update.current().position.get().x),
        _ => None,
    }
}

fn sample_all(resampler: &PointerEventResampler, at: Instant) -> Vec<PointerEvent> {
    let mut out = Vec::new();
    resampler.sample(at, at + ms(16.0), |event| out.push(event));
    out
}

// ----------------------------------------------------------------------------
// Velocity estimation
// ----------------------------------------------------------------------------

/// Moves drained in one frame share a timestamp: the samples carry only two
/// distinct times, so a quadratic is rank-deficient. A straight line still
/// fits, and a moving pointer must not report zero velocity.
fn two_distinct_timestamps_still_measure_motion() {
    let t0 = origin();
    let mut tracker = VelocityTracker::new();
    for i in 0..4 {
        tracker.add_position(t0, Offset::new(f64::from(i), 0.0));
    }
    for i in 4..20 {
        tracker.add_position(t0 + ms(16.0), Offset::new(f64::from(i), 0.0));
    }
    let velocity = tracker.velocity_at(t0 + ms(16.0));
    assert!(
        velocity.dx() > 100.0,
        "a pointer that moved 16 px over 16 ms must fling, got {velocity:?}"
    );
    assert_eq!(velocity.dy(), 0.0);
}

/// The stop gate measures the gap between the last sample and the query on
/// the samples' own clock, so a virtual pause stops the fling and a virtual
/// release right after the last move does not, whatever the wall clock did.
fn stop_gate_reads_the_sample_timeline() {
    let t0 = origin();
    let mut tracker = VelocityTracker::new();
    for i in 0..6 {
        tracker.add_position(
            t0 + ms(8.0 * f64::from(i)),
            Offset::new(8.0 * f64::from(i), 0.0),
        );
    }
    let last = t0 + ms(40.0);
    let moving = tracker.velocity_at(last + ms(5.0));
    assert!(
        (moving.dx() - 1000.0).abs() < 1.0,
        "a release 5 ms after the last move keeps the 1000 px/s swipe, got {moving:?}"
    );
    let paused = tracker.velocity_at(last + ms(100.0));
    assert_eq!(
        paused,
        Velocity::ZERO,
        "a 100 ms pause on the sample clock means the pointer stopped"
    );
}

/// Sub-millisecond spacing makes the slope astronomically large; the
/// published estimate keeps its direction and stops at the fling bound.
fn sub_millisecond_spacing_is_bounded() {
    let t0 = origin();
    let mut tracker = VelocityTracker::new();
    for i in 0..3 {
        tracker.add_position(
            t0 + ms(0.1 * f64::from(i)),
            Offset::new(10.0 * f64::from(i), 0.0),
        );
    }
    let velocity = tracker.velocity_at(t0 + ms(0.2));
    assert!(
        (velocity.magnitude() - DEFAULT_MAX_FLING_VELOCITY).abs() < 1e-6,
        "a 100 000 px/s slope is clamped to the bound, got {velocity:?}"
    );
    assert!(velocity.dx() > 0.0, "clamping keeps the direction");
}

/// Coordinates near the top of the f64 range must not overflow the fit.
fn huge_coordinates_stay_finite() {
    let t0 = origin();
    let mut lsq = VelocityTracker::new();
    let mut impulse = ImpulseVelocityTracker::default();
    let mut ios = IosFlingVelocityTracker::default();
    let mut macos = MacosFlingVelocityTracker::default();
    for i in 0..8 {
        let x = if i % 2 == 0 { 1e300 } else { -1e300 };
        let at = t0 + ms(f64::from(i));
        let position = Offset::new(x, -x);
        lsq.add_position(at, position);
        impulse.add_position(at, position);
        ios.add_position(at, position);
        macos.add_position(at, position);
    }
    assert_bounded(lsq.estimate_at(t0 + ms(7.0)), "lsq");
    assert_bounded(impulse.get_velocity_estimate(), "impulse");
    assert_bounded(ios.get_velocity_estimate(), "ios");
    assert_bounded(macos.get_velocity_estimate(), "macos");
}

#[test]
fn velocity_estimates_are_finite_bounded_and_on_the_sample_clock() {
    run_rows(
        "velocity",
        &[
            (
                "two distinct timestamps",
                two_distinct_timestamps_still_measure_motion,
            ),
            (
                "stop gate on the sample clock",
                stop_gate_reads_the_sample_timeline,
            ),
            (
                "sub-millisecond spacing",
                sub_millisecond_spacing_is_bounded,
            ),
            ("huge coordinates", huge_coordinates_stay_finite),
        ],
    );
}

fn time_offset() -> impl Strategy<Value = i64> {
    prop_oneof![
        // A handful of shared instants, so duplicates are common.
        (0_i64..4).prop_map(|k| k * 8_000_000),
        // Anywhere in a ±150 ms window, out of order included.
        -150_000_000_i64..150_000_000,
        // Sub-microsecond neighbours.
        (0_i64..1000),
    ]
}

fn coordinate() -> impl Strategy<Value = f64> {
    prop_oneof![
        -1e300..1e300_f64,
        -2000.0..2000.0_f64,
        Just(0.0),
        (-1e3..1e3_f64).prop_map(|v| v * 1e297),
    ]
}

proptest! {
    #[test]
    fn any_finite_samples_give_a_finite_bounded_velocity(
        samples in prop::collection::vec((time_offset(), coordinate(), coordinate()), 1..40),
        query_after_ns in 0_u64..60_000_000,
    ) {
        let t0 = origin();
        let at = |offset: i64| {
            if offset >= 0 {
                t0 + Duration::from_nanos(offset.unsigned_abs())
            } else {
                t0.checked_sub(Duration::from_nanos(offset.unsigned_abs()))
                    .expect("the origin is a minute ahead of the process clock")
            }
        };
        let mut lsq = VelocityTracker::new();
        let mut impulse = ImpulseVelocityTracker::default();
        let mut ios = IosFlingVelocityTracker::default();
        let mut macos = MacosFlingVelocityTracker::default();
        let mut last = t0;
        for (offset, x, y) in &samples {
            last = at(*offset);
            let position = Offset::new(*x, *y);
            lsq.add_position(last, position);
            impulse.add_position(last, position);
            ios.add_position(last, position);
            macos.add_position(last, position);
        }
        let query = last + Duration::from_nanos(query_after_ns);
        assert_bounded(lsq.estimate_at(query), "lsq");
        let fling = lsq.velocity_at(query);
        prop_assert!(fling.is_finite());
        prop_assert!(fling.magnitude() <= DEFAULT_MAX_FLING_VELOCITY * (1.0 + 1e-9));
        assert_bounded(impulse.get_velocity_estimate(), "impulse");
        assert_bounded(ios.get_velocity_estimate(), "ios");
        assert_bounded(macos.get_velocity_estimate(), "macos");
    }
}

// ----------------------------------------------------------------------------
// Fling clamping and settings validation
// ----------------------------------------------------------------------------

fn fling_clamp_keeps_sign_and_rejects_nan() {
    let settings = GestureSettings::default();
    assert_eq!(
        settings.clamp_fling_velocity(-500.0),
        -500.0,
        "an in-range release keeps its sign"
    );
    assert_eq!(
        settings.clamp_fling_velocity(-1e9),
        -DEFAULT_MAX_FLING_VELOCITY
    );
    assert_eq!(
        settings.clamp_fling_velocity(1e9),
        DEFAULT_MAX_FLING_VELOCITY
    );
    assert_eq!(
        settings.clamp_fling_velocity(f64::NEG_INFINITY),
        -DEFAULT_MAX_FLING_VELOCITY
    );
    assert_eq!(
        settings.clamp_fling_velocity(f64::NAN),
        0.0,
        "NaN is no velocity"
    );
}

fn inverted_fling_range_is_rejected() {
    assert_eq!(
        GestureSettings::default().try_with_fling_velocity(9000.0, 8000.0),
        Err(GestureSettingsError::InvertedFlingRange {
            min: 9000.0,
            max: 8000.0
        }),
        "a minimum above the maximum cannot be configured"
    );
    let raised = GestureSettings::default()
        .try_with_fling_velocity(9000.0, 12_000.0)
        .expect("an ordered range is accepted");
    assert_eq!(raised.clamp_fling_velocity(-20_000.0), -12_000.0);
    let clamped = Velocity::from_components(30_000.0, 40_000.0).clamp_magnitude(9000.0, 8000.0);
    assert!(
        (clamped.magnitude() - 9000.0).abs() < 1e-6,
        "inverted bounds resolve to the larger"
    );
    let nan_bounds = Velocity::from_components(3.0, 4.0).clamp_magnitude(f64::NAN, f64::NAN);
    assert_eq!(nan_bounds.magnitude(), 5.0, "NaN bounds are absent bounds");
    let nan_velocity = Velocity::from_components(f64::NAN, 1.0).clamp_magnitude(0.0, 10.0);
    assert_eq!(nan_velocity, Velocity::ZERO);
    let infinite_min = Velocity::from_components(1.0, 0.0).clamp_magnitude(f64::INFINITY, 10.0);
    assert_eq!(
        infinite_min.pixels_per_second.dy, 0.0,
        "an infinite minimum leaves the zero axis zero, not NaN"
    );
    assert!(infinite_min.pixels_per_second.dx > 0.0);
}

fn invalid_settings_are_rejected() {
    let base = GestureSettings::mouse_defaults;
    let invalid = |field, value| Err(GestureSettingsError::InvalidValue { field, value });
    assert_eq!(
        base().try_with_touch_slop(-1.0),
        invalid("touch_slop", -1.0)
    );
    assert_eq!(base().try_with_pan_slop(-4.0), invalid("pan_slop", -4.0));
    assert_eq!(
        base().try_with_scale_slop(f64::INFINITY),
        invalid("scale_slop", f64::INFINITY)
    );
    assert!(base().try_with_double_tap_slop(f64::NAN).is_err());
    assert!(base().try_with_pan_slop_vertical(f64::NAN).is_err());
    assert!(base().try_with_pan_slop_horizontal(-0.5).is_err());
    assert_eq!(
        base().try_with_fling_velocity(-1.0, 10.0),
        invalid("min_fling_velocity", -1.0)
    );
    assert!(base().try_with_fling_velocity(50.0, f64::NAN).is_err());
    let built = GestureSettings::try_new(
        18.0,
        18.0,
        0.05,
        100.0,
        Duration::from_millis(300),
        Duration::from_millis(500),
        50.0,
        8000.0,
    );
    assert_eq!(built, Ok(GestureSettings::touch_defaults()));
    let rejected = GestureSettings::try_new(
        18.0,
        18.0,
        0.05,
        100.0,
        Duration::from_millis(300),
        Duration::from_millis(500),
        9000.0,
        8000.0,
    );
    assert!(matches!(
        rejected,
        Err(GestureSettingsError::InvertedFlingRange { .. })
    ));
}

#[test]
fn fling_clamp_and_settings_never_panic_or_flip_sign() {
    run_rows(
        "settings",
        &[
            ("clamp keeps sign", fling_clamp_keeps_sign_and_rejects_nan),
            ("inverted range", inverted_fling_range_is_rejected),
            ("invalid values", invalid_settings_are_rejected),
        ],
    );
}

// ----------------------------------------------------------------------------
// Resampler
// ----------------------------------------------------------------------------

/// Last real sample at 0 ms, next at 11 ms, frame at 10 ms: the resampled
/// position is 10/11 of the way, not the next event's position.
fn interpolation_uses_the_bracketing_samples() {
    let t0 = origin();
    let resampler = PointerEventResampler::new(contact());
    resampler.start_tracking();
    resampler.add_event_at(with_time(move_to(0.0), 1_000), t0);
    resampler.add_event_at(with_time(move_to(110.0), 1_000 + 11_000_000), t0 + ms(11.0));
    let emitted = sample_all(&resampler, t0 + ms(10.0));
    let xs: Vec<_> = emitted.iter().filter_map(event_x).collect();
    assert_eq!(
        xs.len(),
        2,
        "the real sample and one resampled move: {xs:?}"
    );
    assert_eq!(xs[0], 0.0);
    assert!(
        (xs[1] - 100.0).abs() < 1e-9,
        "resampled at 10/11 of the way, got {}",
        xs[1]
    );
    let times: Vec<_> = emitted.iter().filter_map(event_time).collect();
    assert_eq!(
        times[1],
        1_000 + 10_000_000,
        "the resampled move is stamped at the sample time"
    );
}

/// Events delivered in one burst keep their own spacing: the later event
/// lies in the future of a sample taken between them.
fn burst_keeps_event_time_spacing() {
    let resampler = PointerEventResampler::new(contact());
    resampler.start_tracking();
    let base = 5_000_000_000_u64;
    resampler.add_event(with_time(move_to(0.0), base));
    resampler.add_event(with_time(move_to(100.0), base + 1_000_000_000));
    let after = Instant::now();
    let sample_time = after
        .checked_sub(ms(500.0))
        .expect("the process clock has run for 500 ms");
    let emitted = sample_all(&resampler, sample_time);
    let xs: Vec<_> = emitted.iter().filter_map(event_x).collect();
    assert_eq!(xs.first(), Some(&0.0), "the earlier event is due: {xs:?}");
    assert!(
        xs.len() == 2 && xs[1] > 0.0 && xs[1] < 100.0,
        "the later event is a second in the future of the sample, so only an interpolated move is due: {xs:?}"
    );
    assert!(
        resampler.has_pending_events(),
        "the later event stays queued"
    );
}

/// A full queue coalesces moves; the terminal event and the newest position
/// survive.
fn overflow_keeps_terminals_and_the_newest_move() {
    let t0 = origin();
    let resampler = PointerEventResampler::new(contact());
    resampler.add_event_at(
        with_time(
            make_down_event_for_id(contact(), Offset::ZERO, PointerKind::Touch)
                .expect("valid fixture sample"),
            1,
        ),
        t0,
    );
    for i in 1..=150_u32 {
        resampler.add_event_at(
            with_time(move_to(f64::from(i)), u64::from(i) * 1_000_000),
            t0 + ms(f64::from(i)),
        );
    }
    resampler.add_event_at(
        with_time(
            make_up_event_for_id(contact(), Offset::new(150.0, 0.0), PointerKind::Touch)
                .expect("valid fixture sample"),
            151_000_000,
        ),
        t0 + ms(151.0),
    );
    let emitted = sample_all(&resampler, t0 + ms(200.0));
    assert!(matches!(emitted.first(), Some(PointerEvent::Down(_))));
    assert!(
        matches!(emitted.last(), Some(PointerEvent::Up(_))),
        "the Up survives a full queue"
    );
    let last_move = emitted
        .iter()
        .rev()
        .find(|event| matches!(event, PointerEvent::Move(_)))
        .and_then(event_x);
    assert_eq!(
        last_move,
        Some(150.0),
        "the newest move survives coalescing"
    );
    assert!(
        emitted.len() <= 102,
        "the queue stays bounded: {}",
        emitted.len()
    );
}

/// A Manual sampling clock installed on the binding still paces moves frame
/// by frame instead of holding them until the pointer lifts.
fn manual_clock_does_not_starve_moves() {
    let binding = Rc::new(GestureBinding::new());
    binding
        .set_resampling_enabled(true)
        .expect("no active pointers");
    binding.set_sampling_clock(SamplingClock::Manual {
        period: Duration::from_millis(16),
    });
    let moves = Rc::new(RefCell::new(0_u32));
    let seen = Rc::clone(&moves);
    let handler: PointerRouteHandler = Rc::new(move |event| {
        if matches!(event, PointerEvent::Move(_)) {
            *seen.borrow_mut() += 1;
        }
    });
    binding
        .pointer_router()
        .add_route(contact(), Rc::clone(&handler));
    let down = make_down_event_for_id(contact(), Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    binding.handle_pointer_event(&down, |_| HitTestResult::new());
    binding.handle_pointer_event(&move_to(10.0), |_| HitTestResult::new());
    assert_eq!(
        binding.flush_pending_moves(),
        1,
        "the frame flushes the queued move"
    );
    assert_eq!(*moves.borrow(), 1);
    let up = make_up_event_for_id(contact(), Offset::new(10.0, 0.0), PointerKind::Touch)
        .expect("valid fixture sample");
    binding.handle_pointer_event(&up, |_| HitTestResult::new());
    binding.pointer_router().remove_route(contact(), &handler);
}

/// Moves interleaved with scrolls never sit next to each other, so folding
/// moves cannot make room; the queue still stays bounded and keeps the
/// sequence boundaries.
fn interleaved_non_moves_stay_bounded() {
    let t0 = origin();
    let resampler = PointerEventResampler::new(contact());
    resampler.add_event_at(
        make_down_event_for_id(contact(), Offset::ZERO, PointerKind::Touch)
            .expect("valid fixture sample"),
        t0,
    );
    for i in 1..=500_u32 {
        let x = f64::from(i);
        resampler.add_event_at(
            PointerEvent::Scroll(ScrollEvent::new(
                PointerInfo::new(contact(), PointerKind::Mouse),
                flui_platform_api::EventTime::from_nanos(0),
                PointerPosition::try_new(flui_foundation::geometry::Point::new(x, 0.0))
                    .expect("finite scroll position"),
                ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, 1.0).expect("finite scroll delta"),
            )),
            t0 + ms(x),
        );
        resampler.add_event_at(move_to(x), t0 + ms(x));
    }
    resampler.add_event_at(
        make_up_event_for_id(contact(), Offset::new(500.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
        t0 + ms(501.0),
    );
    assert!(
        resampler.pending_event_count() <= 102,
        "an unsampled queue stays bounded: {}",
        resampler.pending_event_count()
    );
    let emitted = sample_all(&resampler, t0 + ms(600.0));
    assert!(matches!(emitted.first(), Some(PointerEvent::Down(_))));
    assert!(matches!(emitted.last(), Some(PointerEvent::Up(_))));
}

#[test]
fn resampler_interpolates_on_event_time_and_never_drops_terminals() {
    run_rows(
        "resampler",
        &[
            (
                "interpolation factor",
                interpolation_uses_the_bracketing_samples,
            ),
            ("burst keeps event spacing", burst_keeps_event_time_spacing),
            (
                "overflow keeps terminals",
                overflow_keeps_terminals_and_the_newest_move,
            ),
            ("manual clock", manual_clock_does_not_starve_moves),
            (
                "interleaved non-moves stay bounded",
                interleaved_non_moves_stay_bounded,
            ),
            (
                "queue diagnostic permits inspection",
                queue_diagnostic_permits_inspection,
            ),
        ],
    );
}

/// The subscriber reads the public handle on a worker with a bounded wait:
/// a regression fails instead of hanging the test process on its mutex.
fn queue_diagnostic_permits_inspection() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    struct InspectQueue {
        resampler: PointerEventResampler,
        observed: Arc<AtomicUsize>,
    }
    impl tracing::Subscriber for InspectQueue {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.target().ends_with("resampler") && *metadata.level() == tracing::Level::DEBUG
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            let resampler = self.resampler.clone();
            let (send, receive) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(resampler.pending_event_count());
            });
            let count = receive
                .recv_timeout(Duration::from_secs(5))
                .expect("queue diagnostic must release the resampler mutex");
            self.observed.store(count, Ordering::Relaxed);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    for explicit in [false, true] {
        let resampler = PointerEventResampler::new(contact());
        let t0 = origin();
        let enqueue = |event| {
            if explicit {
                resampler.add_event_at(event, t0);
            } else {
                resampler.add_event(event);
            }
        };
        for _ in 0..50 {
            enqueue(
                make_down_event_for_id(contact(), Offset::ZERO, PointerKind::Touch)
                    .expect("valid fixture sample"),
            );
            enqueue(
                make_up_event_for_id(contact(), Offset::ZERO, PointerKind::Touch)
                    .expect("valid fixture sample"),
            );
        }
        enqueue(
            make_down_event_for_id(contact(), Offset::ZERO, PointerKind::Touch)
                .expect("valid fixture sample"),
        );
        let observed = Arc::new(AtomicUsize::new(0));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::subscriber::with_default(
            InspectQueue {
                resampler: resampler.clone(),
                observed: Arc::clone(&observed),
            },
            || enqueue(move_to(1.0)),
        );
        assert_eq!(
            observed.load(Ordering::Relaxed),
            102,
            "the diagnostic can inspect the committed event, explicit={explicit}"
        );
    }
}

#[derive(Debug, Clone)]
enum Op {
    Down(i64),
    Move(i64, f64),
    Up(i64),
    Cancel,
    Sample(u64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        1 => (-20_i64..200).prop_map(Op::Down),
        8 => ((-20_i64..200), -1e6..1e6_f64).prop_map(|(t, x)| Op::Move(t, x)),
        1 => (-20_i64..200).prop_map(Op::Up),
        1 => Just(Op::Cancel),
        2 => (0_u64..40).prop_map(Op::Sample),
    ]
}

fn terminal_tag(event: &PointerEvent) -> Option<(char, u64)> {
    match event {
        PointerEvent::Down(button) => Some(('d', button.sample.position.get().y.to_bits())),
        PointerEvent::Up(button) => Some(('u', button.sample.position.get().y.to_bits())),
        PointerEvent::Cancel(_) => Some(('c', 0)),
        _ => None,
    }
}

proptest! {
    #[test]
    fn resampler_keeps_terminals_in_order_with_monotonic_time(
        ops in prop::collection::vec(op(), 1..260),
        explicit_time in any::<bool>(),
    ) {
        let t0 = origin();
        let resampler = PointerEventResampler::new(contact());
        resampler.start_tracking();
        let mut expected = Vec::new();
        let mut emitted = Vec::new();
        let mut frame = t0;
        let mut serial = 0_u32;
        let stamp = |ms_offset: i64| {
            let nanos = (1_000_i64 + ms_offset) * 1_000_000;
            let at = (t0 + Duration::from_millis(u64::try_from(1_000 + ms_offset).expect("positive")))
                .checked_sub(Duration::from_secs(1))
                .expect("the origin is a minute ahead of the process clock");
            (u64::try_from(nanos).expect("positive"), at)
        };
        for op in &ops {
            serial += 1;
            // The y coordinate tags each terminal event uniquely.
            let tag = f64::from(serial);
            let (event, at) = match *op {
                Op::Down(t) => {
                    let (ns, at) = stamp(t);
                    (with_time(make_down_event_for_id(contact(), Offset::new(0.0, tag), PointerKind::Touch).expect("finite fixture"), ns), at)
                }
                Op::Move(t, x) => {
                    let (ns, at) = stamp(t);
                    (with_time(make_move_event_for_id(contact(), Offset::new(x, 0.0), PointerKind::Touch).expect("finite fixture"), ns), at)
                }
                Op::Up(t) => {
                    let (ns, at) = stamp(t);
                    (with_time(make_up_event_for_id(contact(), Offset::new(0.0, tag), PointerKind::Touch).expect("finite fixture"), ns), at)
                }
                Op::Cancel => (make_cancel_event_for_id(contact(), PointerKind::Touch), t0),
                Op::Sample(step) => {
                    frame += Duration::from_millis(step);
                    resampler.sample(frame, frame + ms(16.0), |event| emitted.push(event));
                    continue;
                }
            };
            if let Some(tag) = terminal_tag(&event) {
                expected.push(tag);
            }
            if explicit_time {
                resampler.add_event_at(event, at);
            } else {
                resampler.add_event(event);
            }
        }
        resampler.stop(|event| emitted.push(event));

        let terminals: Vec<_> = emitted.iter().filter_map(terminal_tag).collect();
        prop_assert_eq!(terminals, expected, "every Down/Up/Cancel exactly once, in order");
        let times: Vec<_> = emitted.iter().filter_map(event_time).collect();
        prop_assert!(times.windows(2).all(|w| w[0] <= w[1]), "emitted times are monotonic: {:?}", times);
        prop_assert!(!resampler.has_pending_events());
    }
}

// ----------------------------------------------------------------------------
// Checked pan/zoom transport
// ----------------------------------------------------------------------------

fn pinch_scale_is_finite_and_positive() {
    use flui_interaction::events::{PanZoomEvent, PanZoomPhase, PanZoomTransform};
    use flui_interaction::{EventPropagation, HitTestEntry, routing::InteractionLane};

    // The old upstream pinch fractions NaN/-1/-3/inf imply these invalid
    // cumulative scales. Checked transport refuses them before routing.
    for scale in [f64::NAN, 0.0, -2.0, f64::INFINITY] {
        assert!(
            PanZoomTransform::try_new(Offset::ZERO, scale, 0.0).is_err(),
            "invalid cumulative scale {scale}"
        );
    }
    let lane = InteractionLane::try_new().expect("panzoom owner");
    let handle = lane.dispatch_handle();
    let observed = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let sink = observed.clone();
        let target = handle
            .register_pan_zoom(move |event| {
                if let PanZoomPhase::Update(transform) = event.phase {
                    sink.borrow_mut()
                        .push((transform.scale(), transform.rotation()));
                }
                EventPropagation::Continue
            })
            .expect("panzoom target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(flui_foundation::RenderId::new(1)).pan_zoom_target(target));
        let event = PointerEvent::PanZoom(PanZoomEvent::new(
            PointerInfo::new(contact(), PointerKind::Trackpad),
            flui_platform_api::EventTime::from_nanos(1_000),
            PointerPosition::try_new(flui_foundation::geometry::Point::ZERO)
                .expect("finite focal position"),
            PanZoomPhase::Update(
                PanZoomTransform::try_new(Offset::ZERO, 1.25, 0.0).expect("valid zoom"),
            ),
        ));
        GestureBinding::new().handle_pointer_event_with_result(&event, &path);
    });
    assert_eq!(
        &*observed.borrow(),
        &[(1.25, 0.0)],
        "checked cumulative pinch reaches its actual target"
    );
}

#[test]
fn checked_pan_zoom_reaches_its_actual_target() {
    run_rows(
        "processing",
        &[("pinch scale", pinch_scale_is_finite_and_positive)],
    );
}
