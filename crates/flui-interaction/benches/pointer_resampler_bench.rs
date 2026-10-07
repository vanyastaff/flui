//! PointerEventResampler benchmarks
//!
//! Hot path: `PointerEventResampler::add_event` is called once per raw
//! pointer event from the platform layer (winit, Win32, etc.). The
//! resampler is invoked by `GestureBinding` between the platform event
//! source and the recogniser set. Trackpads emit events at much higher
//! rates than touchscreens (240 Hz on modern Precision touchpads, vs
//! 60-120 Hz on touchscreens), so the resampler must not regress under
//! burst input.
//!
//! Performance targets:
//! - `add_event` per raw input event: < 200 ns. The work is a
//!   `RefCell` borrow of the resampler state + a `VecDeque::push_back`
//!   (moves coalesce beyond 100 queued events) + a state-machine
//!   transition for Down/Up/Cancel/Leave.
//! - `sample` flush at 60 Hz (16.67 ms): < 1 µs per drained event.
//!
//! The two scenarios below (60 Hz and 240 Hz) verify the resampler
//! does not blow up under trackpad-style input.
//!
//! Follows the workspace benchmark template at
//! `rust-studio/.../templates/benchmark-report.md`.
//!
//! Run with `cargo bench -p flui-interaction --bench pointer_resampler_bench`.

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use flui_foundation::geometry::Offset;
use flui_interaction::events::{PointerKind, make_move_event};
use flui_interaction::ids::PointerId;
use flui_interaction::processing::PointerEventResampler;

/// Build `count` move events, 1 px apart. The `duration_ms` argument is
/// documentary: the synthetic events carry timestamp 0, so `add_event`
/// places them at their arrival; `bench_sample_flush` places them
/// explicitly with `add_event_at`.
fn make_move_events(
    count: usize,
    _duration_ms: u64,
) -> Vec<flui_interaction::events::PointerEvent> {
    (0..count)
        .map(|i| {
            make_move_event(Offset::new(100.0 + i as f64, 100.0), PointerKind::Touch)
                .expect("valid fixture sample")
        })
        .collect()
}

/// 60 Hz benchmark — touchscreen rate. Pre-loads the queue once at
/// setup, then per-iteration clears and re-feeds. Measures per-event
/// push cost in the steady state.
fn bench_add_event_60hz(c: &mut Criterion) {
    let events = black_box(make_move_events(100, 100)); // 100 events / 100 ms ≈ 1 kHz; rate is the test, not the gate
    c.bench_function("PointerEventResampler::add_event (60 Hz workload)", |b| {
        b.iter(|| {
            let resampler = PointerEventResampler::new(PointerId::new(std::num::NonZeroU64::MIN));
            for event in &events {
                resampler.add_event(black_box(event.clone()));
            }
            black_box(resampler.has_pending_events());
        });
    });
}

/// 240 Hz benchmark — trackpad rate. 240 events over 1 second. The
/// resampler must keep up without dropping events (the
/// `MAX_BUFFERED_EVENTS = 100` cap is the back-pressure boundary;
/// drops are logged via `tracing::warn!`).
fn bench_add_event_240hz(c: &mut Criterion) {
    let events = black_box(make_move_events(240, 1000));
    c.bench_function("PointerEventResampler::add_event (240 Hz workload)", |b| {
        b.iter(|| {
            let resampler = PointerEventResampler::new(PointerId::new(std::num::NonZeroU64::MIN));
            for event in &events {
                resampler.add_event(black_box(event.clone()));
            }
            black_box(resampler.has_pending_events());
        });
    });
}

/// `sample` flush cost — the per-frame work done by the binding
/// (drains the queue, fires callbacks). One iteration drains a
/// 60-event queue, one event per 0.25 ms, followed by a 61st event after
/// the sample time so the interpolated move is part of the work, and
/// counts callbacks. The queue is built in (untimed) setup on a tracked
/// resampler, so the timed region is only `sample`, and it is asserted to
/// emit all 61 events (60 real + 1 interpolated).
fn bench_sample_flush(c: &mut Criterion) {
    let events = make_move_events(61, 16);
    let start = Instant::now();
    let sample_time = start + Duration::from_millis(15);
    let next = sample_time + Duration::from_millis(16);
    let setup = || {
        let resampler = PointerEventResampler::new(PointerId::new(std::num::NonZeroU64::MIN));
        resampler.start_tracking();
        for (i, event) in events.iter().enumerate() {
            let at = start + Duration::from_micros(250 * i as u64);
            let at = if i == 60 { next } else { at };
            resampler.add_event_at(event.clone(), at);
        }
        resampler
    };
    let mut probe = 0u32;
    setup().sample(sample_time, next, |_event| probe += 1);
    assert_eq!(probe, 61, "the bench must drain the whole frame");
    c.bench_function(
        "PointerEventResampler::sample (drain 60-event queue)",
        |b| {
            b.iter_batched(
                setup,
                |resampler| {
                    let mut count = 0u32;
                    resampler.sample(black_box(sample_time), black_box(next), |_event| {
                        count += 1;
                    });
                    black_box(count)
                },
                criterion::BatchSize::SmallInput,
            );
        },
    );
}

/// Steady-state push: the queue is at its 100-event cap, so each
/// `add_event` coalesces the oldest pair of adjacent moves (the older
/// one's samples join the newer one's `coalesced` history) before
/// queueing — the overflow path that replaced dropping the newest event.
fn bench_push_at_capacity(c: &mut Criterion) {
    let resampler = PointerEventResampler::new(PointerId::new(std::num::NonZeroU64::MIN));
    // Pre-fill to capacity.
    for event in make_move_events(100, 100) {
        resampler.add_event(event);
    }
    let event = black_box(
        make_move_event(Offset::new(200.0, 100.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    c.bench_function(
        "PointerEventResampler::add_event (queue at cap, overflow path)",
        |b| {
            b.iter(|| {
                resampler.add_event(black_box(event.clone()));
            });
        },
    );
}

criterion_group!(
    resampler_benches,
    bench_add_event_60hz,
    bench_add_event_240hz,
    bench_sample_flush,
    bench_push_at_capacity,
);
criterion_main!(resampler_benches);
