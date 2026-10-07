//! Owned pointer resampling with deterministic source times and frame windows.
//!
//! Each fixture proves delivery before measurement. Construction and retirement
//! are outside sample/stop/overflow timings; frame traces include admission,
//! sampling, callback consumption and the terminal flush. These are workload
//! timings, not per-event bounds or native-device throughput measurements.

use std::hint::black_box;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::geometry::Point;
use flui_interaction::events::{PointerEvent, PointerEventExt};
use flui_interaction::processing::PointerEventResampler;
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{
    CancelReason, PointerButton, PointerButtons, PointerCancel, PointerId, PointerInfo,
    PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRole,
    PointerSample,
};

const SECOND: u64 = 1_000_000_000;
const FRAME_RATE: u64 = 60;
const LOOKBACK: u64 = 5_000_000;

#[derive(Clone, Copy)]
enum Terminal {
    Up,
    Cancel,
}

impl Terminal {
    fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Cancel => "cancel",
        }
    }
}

fn pointer() -> PointerInfo {
    PointerInfo::new(PointerId::new(NonZeroU64::MIN), PointerKind::Touch)
        .with_role(PointerRole::Primary)
}

fn sample(nanos: u64, x: f64) -> PointerSample {
    PointerSample::new(
        EventTime::from_nanos(nanos),
        PointerPosition::try_new(Point::new(x, 25.5)).expect("finite fixture position"),
    )
}

fn movement(nanos: u64) -> PointerEvent {
    PointerEvent::Move(PointerMove::new(
        pointer(),
        PointerButtons::only(PointerButton::PRIMARY),
        sample(nanos, nanos as f64 / 1_000_000.0),
    ))
}

fn down() -> PointerEvent {
    PointerEvent::Down(PointerPress::new(
        pointer(),
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(0, 0.0),
    ))
}

fn terminal(kind: Terminal, nanos: u64, x: f64) -> PointerEvent {
    match kind {
        Terminal::Up => PointerEvent::Up(PointerRelease::new(
            pointer(),
            PointerButton::PRIMARY,
            PointerButtons::only(PointerButton::PRIMARY),
            sample(nanos, x),
        )),
        Terminal::Cancel => PointerEvent::Cancel(PointerCancel::new(
            pointer(),
            EventTime::from_nanos(nanos),
            CancelReason::Platform,
        )),
    }
}

fn event_nanos(event: &PointerEvent) -> u64 {
    flui_interaction::events::get_event_time(event)
        .expect("fixture event time")
        .as_nanos()
}

/// Consume both the source clock and the delivered value, not merely callback count.
fn consume(event: PointerEvent) {
    black_box((event_nanos(&event), event.position()));
    if let PointerEvent::Move(event) = event {
        black_box(event.coalesced());
    }
}

struct Fixture {
    resampler: PointerEventResampler,
    base: Instant,
}

impl Fixture {
    fn new() -> Self {
        Self {
            resampler: PointerEventResampler::new(pointer().id),
            base: Instant::now(),
        }
    }

    fn add(&self, event: PointerEvent) {
        let at = self.base + Duration::from_nanos(event_nanos(&event));
        self.resampler.add_event_at(event, at);
    }
}

/// A preflight witness of the known 1000 px/s horizontal trajectory.
#[derive(Default)]
struct Witness {
    down: usize,
    up: usize,
    cancel: usize,
    moves: usize,
    readings: usize,
    last_time: u64,
    last_move: Option<(u64, f64)>,
    terminal: Option<&'static str>,
}

impl Witness {
    fn observe(&mut self, event: PointerEvent) {
        let nanos = event_nanos(&event);
        assert!(
            nanos >= self.last_time,
            "delivered timestamps are monotonic"
        );
        self.last_time = nanos;
        assert!(self.terminal.is_none(), "nothing follows the terminal");
        match event {
            PointerEvent::Down(event) => {
                self.down += 1;
                assert_eq!(event.sample.position.get(), Point::new(0.0, 25.5));
                assert_eq!(event.sample.time.as_nanos(), 0);
            }
            PointerEvent::Move(event) => {
                assert_eq!(self.down, 1, "movement follows admitted Down");
                self.moves += 1;
                for sample in event
                    .coalesced()
                    .iter()
                    .chain(std::iter::once(event.current()))
                {
                    self.readings += 1;
                    // Interpolated EventTime rounds down to integer nanos;
                    // at 1000 px/s that accounts for at most 0.000001 px.
                    assert!(
                        (sample.position.get().x - sample.time.as_nanos() as f64 / 1_000_000.0)
                            .abs()
                            < 1.1e-6,
                        "delivered values follow the independent linear trajectory"
                    );
                    assert_eq!(sample.position.get().y, 25.5);
                }
                self.last_move = Some((
                    event.current().time.as_nanos(),
                    event.current().position.get().x,
                ));
            }
            PointerEvent::Up(event) => {
                self.up += 1;
                assert!(!event.buttons().contains(PointerButton::PRIMARY));
                self.terminal = Some("up");
            }
            PointerEvent::Cancel(event) => {
                self.cancel += 1;
                assert_eq!(event.reason, CancelReason::Platform);
                self.terminal = Some("cancel");
            }
            _ => panic!("unexpected fixture event"),
        }
    }

    fn finished(&self, terminal: Terminal, time: u64, last_move: (u64, f64)) {
        assert_eq!(self.down, 1);
        assert_eq!(self.terminal, Some(terminal.name()));
        assert_eq!(
            (self.up, self.cancel),
            match terminal {
                Terminal::Up => (1, 0),
                Terminal::Cancel => (0, 1),
            }
        );
        assert_eq!(self.last_time, time);
        assert_eq!(self.last_move, Some(last_move));
    }
}

fn frame_trace(fixture: &Fixture, events: &[PointerEvent], mut emit: impl FnMut(PointerEvent)) {
    let mut next = 0;
    for frame in 1..=FRAME_RATE {
        let frame_time = frame * SECOND / FRAME_RATE;
        while next < events.len() && event_nanos(&events[next]) <= frame_time {
            fixture.add(events[next].clone());
            next += 1;
        }
        let target = frame_time - LOOKBACK;
        fixture.resampler.sample(
            fixture.base + Duration::from_nanos(target),
            fixture.base + Duration::from_nanos(target + SECOND / FRAME_RATE),
            &mut emit,
        );
    }
    for event in &events[next..] {
        fixture.add(event.clone());
    }
    fixture.resampler.stop(emit);
}

fn bench_frame_traces(c: &mut Criterion) {
    let mut group = c.benchmark_group("resampler/frame_trace");
    for rate in [60_u64, 240] {
        for kind in [Terminal::Up, Terminal::Cancel] {
            let mut events = vec![down()];
            events.extend((1..=rate).map(|i| movement(i * SECOND / rate)));
            events.push(terminal(kind, SECOND + 1, 1000.0));
            let mut witness = Witness::default();
            frame_trace(&Fixture::new(), &events, |event| witness.observe(event));
            // One measured packet per source tick, one interpolated value per
            // display frame, followed by the genuine terminal.
            assert_eq!(witness.moves, rate as usize + FRAME_RATE as usize);
            witness.finished(kind, SECOND + 1, (SECOND, 1000.0));
            group.bench_function(
                BenchmarkId::new(format!("{rate}_to_60"), kind.name()),
                |b| {
                    b.iter_batched_ref(
                        Fixture::new,
                        |fixture| frame_trace(fixture, black_box(&events), consume),
                        BatchSize::SmallInput,
                    );
                },
            );
        }
    }
    group.finish();
}

fn sample_fixture(kind: Terminal) -> Fixture {
    let fixture = Fixture::new();
    fixture.add(down());
    for i in 1..=60 {
        fixture.add(movement(i * 250_000));
    }
    fixture.add(movement(16_000_000));
    fixture.add(terminal(kind, 17_000_000, 16.0));
    fixture
}

fn sample_frame(fixture: &Fixture, emit: impl FnMut(PointerEvent)) {
    fixture.resampler.sample(
        fixture.base + Duration::from_nanos(15_500_000),
        fixture.base + Duration::from_nanos(32_166_667),
        emit,
    );
}

fn verify_sample_and_tail(kind: Terminal) {
    let fixture = sample_fixture(kind);
    let mut witness = Witness::default();
    sample_frame(&fixture, |event| witness.observe(event));
    assert_eq!(
        (witness.down, witness.moves, witness.up, witness.cancel),
        (1, 61, 0, 0)
    );
    assert_eq!(
        witness.last_move,
        Some((15_500_000, 15.5)),
        "the 61st movement is the interpolated frame reading"
    );
    fixture.resampler.stop(|event| witness.observe(event));
    assert_eq!(
        witness.moves, 62,
        "the future measured packet follows interpolation"
    );
    witness.finished(kind, 17_000_000, (16_000_000, 16.0));
    assert!(
        !fixture.resampler.has_pending_events(),
        "terminal flush drains accepted debt"
    );
}

fn bench_sample_and_stop(c: &mut Criterion) {
    for kind in [Terminal::Up, Terminal::Cancel] {
        verify_sample_and_tail(kind);
    }
    c.bench_function("resampler/sample/measured_and_interpolated", |b| {
        b.iter_batched_ref(
            || sample_fixture(Terminal::Up),
            |fixture| sample_frame(fixture, consume),
            BatchSize::SmallInput,
        );
    });
    let mut group = c.benchmark_group("resampler/stop");
    for kind in [Terminal::Up, Terminal::Cancel] {
        group.bench_function(kind.name(), |b| {
            b.iter_batched_ref(
                || {
                    let fixture = sample_fixture(kind);
                    sample_frame(&fixture, consume);
                    fixture
                },
                |fixture| fixture.resampler.stop(consume),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn capacity_fixture(moves: u64) -> Fixture {
    let fixture = Fixture::new();
    fixture.add(down());
    for i in 1..=moves {
        fixture.add(movement(i * 1_000_000));
    }
    assert_eq!(
        fixture.resampler.pending_event_count(),
        100,
        "documented capacity fixture"
    );
    fixture
}

fn bench_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("resampler/overflow");
    for (name, moves, expected_readings) in [
        ("scalar_history", 99_u64, 100_usize),
        ("saturated_history", 300, 199),
    ] {
        let event = movement((moves + 1) * 1_000_000);
        let witness_fixture = capacity_fixture(moves);
        witness_fixture.add(event.clone());
        witness_fixture.add(terminal(
            Terminal::Up,
            (moves + 2) * 1_000_000,
            (moves + 1) as f64,
        ));
        let mut witness = Witness::default();
        witness_fixture
            .resampler
            .stop(|event| witness.observe(event));
        assert_eq!(witness.moves, 99, "overflow folds one adjacent pair");
        assert_eq!(
            witness.readings, expected_readings,
            "bounded history is explicit, not lossless beyond its cap"
        );
        witness.finished(
            Terminal::Up,
            (moves + 2) * 1_000_000,
            ((moves + 1) * 1_000_000, (moves + 1) as f64),
        );
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                || capacity_fixture(moves),
                |fixture| fixture.add(black_box(event.clone())),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    resampler_benches,
    bench_frame_traces,
    bench_sample_and_stop,
    bench_capacity
);
criterion_main!(resampler_benches);
