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
//!   `Mutex` lock on `ResamplerInner` + a `VecDeque::push_back` (with
//!   bounded cap = 100) + a state-machine transition for
//!   Down/Up/Cancel/Leave. No allocations in the steady state once
//!   the queue is at capacity.
//! - `sample` flush at 60 Hz (16.67 ms): < 1 µs per drained event.
//!
//! The two scenarios below (60 Hz and 240 Hz) verify the resampler
//! does not blow up under trackpad-style input.
//!
//! Follows the workspace benchmark template at
//! `rust-studio/.../templates/benchmark-report.md`.
//!
//! Run with `cargo bench -p flui-interaction --bench pointer_resampler_bench`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use flui_interaction::events::{PointerType, make_move_event};
use flui_interaction::ids::PointerId;
use flui_interaction::processing::PointerEventResampler;
use flui_types::geometry::{Offset, Pixels};

/// Build `count` move events. Position is varied by 1 px per event so
/// the resampler's dedup logic does not collapse the queue to a
/// single entry — we need the queue to actually hold `count` distinct
/// events. The `duration_ms` argument is documentary (the synthetic
/// events all carry timestamp 0; the resampler reads `Instant::now()`
/// at push time, so this bench is not a timing-fidelity measurement).
fn make_move_events(
    count: usize,
    _duration_ms: u64,
) -> Vec<flui_interaction::events::PointerEvent> {
    (0..count)
        .map(|i| {
            make_move_event(
                Offset::new(Pixels(100.0 + i as f32), Pixels(100.0)),
                PointerType::Touch,
            )
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
            let resampler = PointerEventResampler::new(PointerId::PRIMARY);
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
            let resampler = PointerEventResampler::new(PointerId::PRIMARY);
            for event in &events {
                resampler.add_event(black_box(event.clone()));
            }
            black_box(resampler.has_pending_events());
        });
    });
}

/// `sample` flush cost — the per-frame work done by the binding
/// (drains the queue, fires callbacks). One iteration drains a
/// 60-event queue and counts callbacks.
fn bench_sample_flush(c: &mut Criterion) {
    let resampler = PointerEventResampler::new(PointerId::PRIMARY);
    // Pre-load 60 events so the queue is at the typical 60 Hz
    // frame's worth of work.
    for event in make_move_events(60, 16) {
        resampler.add_event(event);
    }
    // Push 100 ns past MIN_SAMPLE_INTERVAL so the per-sample
    // throttling gate does not early-return.
    let now = Instant::now() + Duration::from_micros(2000);
    let next = now + Duration::from_millis(16);
    c.bench_function(
        "PointerEventResampler::sample (drain 60-event queue)",
        |b| {
            b.iter(|| {
                // Re-add events so the queue is non-empty for the next
                // sample call (each `sample` pops the queue).
                for event in make_move_events(60, 16) {
                    resampler.add_event(event);
                }
                let mut count = 0u32;
                resampler.sample(black_box(now), black_box(next), |_event| {
                    count += 1;
                });
                black_box(count);
            });
        },
    );
}

/// Steady-state push: the queue is at its 100-event cap, so each
/// `add_event` should be a single atomic Mutex lock + a state
/// transition. The 101st-and-onward events are silently dropped (per
/// `MAX_BUFFERED_EVENTS`), so this also covers the drop path.
fn bench_push_at_capacity(c: &mut Criterion) {
    let resampler = PointerEventResampler::new(PointerId::PRIMARY);
    // Pre-fill to capacity.
    for event in make_move_events(100, 100) {
        resampler.add_event(event);
    }
    let event = black_box(make_move_event(
        Offset::new(Pixels(200.0), Pixels(100.0)),
        PointerType::Touch,
    ));
    c.bench_function(
        "PointerEventResampler::add_event (queue at cap, drop path)",
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
