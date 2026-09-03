//! Criterion benchmark: the per-presentation segment gate, before vs. after
//! issue #556's `FrameClock` replacement.
//!
//! Before: `UiRealm::draw_frame_entered` read
//! `take_redraw_pending() || has_pending_work()` directly. After: the same
//! union is marked as `Dirty` demand and `FrameClock::poll` decides. The
//! gate runs once per presentation per pump, so its marginal cost matters at
//! scale; this measures that mechanism cost in isolation from the app-level
//! checks feeding it (identical either way, and not modeled here).
//!
//! Run with `cargo bench -p flui-scheduler --bench frame_clock_gate`.

// Benchmark harness functions are internal measurement scaffolding, not a
// public API surface — exempt from the crate's missing-docs lint, matching
// flui-animation's own bench target.

use std::cell::Cell;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_scheduler::{DemandKind, FrameClock};

/// Stand-in for the two `Cell`-backed dirty signals the OLD gate read
/// directly on `PresentationState`: `redraw_pending` (`take_redraw_pending`'s
/// read-and-clear) and `has_pending_work` (a stable boolean at the mechanism
/// level — the nested `WidgetsBinding`/`PipelineOwner` checks it wraps in
/// production are identical whichever gate calls them, so they are
/// deliberately NOT modeled here: this bench isolates the GATE mechanism's
/// own marginal cost, not the app-level checks feeding it).
struct OldGate {
    redraw_pending: Cell<bool>,
    has_pending_work: Cell<bool>,
}

impl OldGate {
    fn new(has_pending_work: bool) -> Self {
        Self {
            redraw_pending: Cell::new(false),
            has_pending_work: Cell::new(has_pending_work),
        }
    }

    /// The exact shape `UiRealm::draw_frame_entered` read before #556:
    /// `let woken = presentation.take_redraw_pending(); if !(woken ||
    /// presentation.has_pending_work()) { continue; }`.
    fn should_run_segment(&self) -> bool {
        let woken = self.redraw_pending.replace(false);
        woken || self.has_pending_work.get()
    }
}

/// The regime where every pump has real work: the old predicate's
/// `has_pending_work` stand-in stays `true`; the new gate has `Dirty`
/// demand marked before every poll, mirroring `draw_frame_entered`'s
/// `if woken || presentation.has_pending_work() { clock.mark_demand(...) }`
/// guard firing every time.
fn segment_gate_dirty(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment_gate_dirty");

    let old_gate = OldGate::new(true);
    group.bench_function("old_predicate", |b| {
        b.iter(|| black_box(old_gate.should_run_segment()));
    });

    let clock = FrameClock::new();
    group.bench_function("clock_poll", |b| {
        b.iter(|| {
            clock.mark_demand(black_box(DemandKind::Dirty));
            black_box(clock.poll(clock.now()).is_produce())
        });
    });

    group.finish();
}

/// The regime where a presentation is genuinely settled: nothing marks
/// demand and the old predicate's `has_pending_work` stand-in stays
/// `false` — the far more common case in a demand-driven engine, where
/// an idle presentation burns zero frames while staying instantly
/// responsive (issue #556).
fn segment_gate_idle(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment_gate_idle");

    let old_gate = OldGate::new(false);
    group.bench_function("old_predicate", |b| {
        b.iter(|| black_box(old_gate.should_run_segment()));
    });

    let clock = FrameClock::new();
    group.bench_function("clock_poll", |b| {
        // Nothing marks demand this pump -- mirrors the realm's own guard
        // never firing when neither input is true.
        b.iter(|| black_box(clock.poll(clock.now()).is_produce()));
    });

    group.finish();
}

/// Attribution: how much of `clock_poll`'s cost above is
/// `FrameClock::now()` (a `web_time::Instant::now()` read, on native a thin
/// wrapper over `clock_gettime`) versus `poll` itself. Isolates each so a
/// reader is not left guessing which side the difference from
/// `segment_gate_*` above actually lives on.
fn poll_cost_attribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("poll_cost_attribution");

    group.bench_function("now_only", |b| {
        let clock = FrameClock::new();
        b.iter(|| black_box(clock.now()));
    });

    group.bench_function("poll_only_precomputed_now", |b| {
        let clock = FrameClock::new();
        let now = clock.now();
        b.iter(|| black_box(clock.poll(black_box(now)).is_produce()));
    });

    group.finish();
}

criterion_group!(
    benches,
    segment_gate_dirty,
    segment_gate_idle,
    poll_cost_attribution
);
criterion_main!(benches);
