//! GestureArena benchmarks
//!
//! Hot path: `GestureArena::add` and `GestureArena::sweep` are called on
//! every pointer-down / pointer-up. `add` is the per-recogniser cost of
//! joining the arena; `sweep` is the per-pointer cleanup at the end of a
//! gesture sequence.
//!
//! Performance targets (per `docs/testing.md` and the constitution's 16 ms
//! frame budget):
//! - `add` of a single member into an empty arena: < 1 µs (1 DashMap
//!   entry insert + 1 Mutex lock + 1 SmallVec push).
//! - `sweep` of a single-member arena: < 1 µs (1 Mutex lock + 1 DashMap
//!   remove).
//! - Conflict resolution (eager + competitor) should not regress the hot
//!   path beyond a constant factor (sharing the least-squares math
//!   primitives across recognisers made arena dispatch cheaper; we
//!   regression-guard against losing that win).
//!
//! Follows the workspace benchmark template at
//! `rust-studio/.../templates/benchmark-report.md`.
//!
//! Run with `cargo bench -p flui-interaction --bench gesture_arena_bench`.

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.
#![allow(missing_docs)]

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_interaction::arena::{GestureArena, GestureArenaEntry};
use flui_interaction::ids::PointerId;
use flui_interaction::sealed::CustomGestureRecognizer;

/// Minimal arena member used as a recogniser stand-in.
///
/// Implements `CustomGestureRecognizer` (the public extension point —
/// `GestureArenaMember` is sealed) so the bench can drive arena
/// dispatch without instantiating a real recogniser. The bench
/// measures arena cost, not the recogniser's per-event work, so
/// `on_arena_accept` / `on_arena_reject` are no-ops. The `id` field
/// is intentionally kept for debugging output (the bench fixture
/// could grow to per-id timings in a follow-up).
#[derive(Debug)]
struct BenchMember {
    #[allow(dead_code)] // retained for future per-id bench breakdown
    id: usize,
}

impl CustomGestureRecognizer for BenchMember {
    fn on_arena_accept(&self, _pointer: PointerId) {}
    fn on_arena_reject(&self, _pointer: PointerId) {}
}

/// Pre-build a pool of recogniser handles so per-iteration setup is
/// just an `Arc::clone` (which is cheap but not free — we want to
/// measure arena cost, not allocation cost).
fn make_members(count: usize) -> Vec<Arc<BenchMember>> {
    (0..count)
        .map(|i| Arc::new(BenchMember { id: i }))
        .collect()
}

/// Benchmark `arena.add` against an empty arena. Each iteration
/// removes the previous entry so the next `add` is a fresh insert.
fn bench_add_empty(c: &mut Criterion) {
    let arena = GestureArena::new();
    let members = black_box(make_members(1));
    let pointer = PointerId::PRIMARY;
    c.bench_function("GestureArena::add (empty, 1 member)", |b| {
        b.iter(|| {
            arena.sweep(pointer);
            let entry = arena.add(black_box(pointer), black_box(members[0].clone()));
            black_box(entry.pointer());
        });
    });
}

/// Benchmark `arena.add` into a busy arena (4 pre-existing members for
/// the same pointer). The cost profile changes: SmallVec push from
/// inline (4) to heap-backed (5+), plus extra contention on the inner
/// Mutex as `close()` is checked. This is the realistic tap-vs-drag
/// case.
fn bench_add_busy(c: &mut Criterion) {
    let arena = GestureArena::new();
    let members = black_box(make_members(5));
    let pointer = PointerId::PRIMARY;
    // Pre-load 4 members so each `add` is into a 4-member arena.
    let _entries: Vec<GestureArenaEntry> = (0..4)
        .map(|i| arena.add(pointer, members[i].clone()))
        .collect();
    c.bench_function("GestureArena::add (busy, 4 prior members)", |b| {
        b.iter(|| {
            // The 5th member will trigger SmallVec heap growth on
            // subsequent iterations because we never sweep.
            let entry = arena.add(black_box(pointer), black_box(members[4].clone()));
            // Drop the just-added member so the next iter starts
            // from the same 4-member baseline.
            entry.resolve(flui_interaction::arena::GestureDisposition::Rejected);
            black_box(entry.pointer());
        });
    });
}

/// Benchmark `arena.sweep` of a single-member arena. Sweep is called
/// on `pointerup` to remove resolved arenas. A 1-member arena is the
/// common case (tap recogniser alone, or a single-element gesture).
fn bench_sweep_empty(c: &mut Criterion) {
    let arena = GestureArena::new();
    let members = black_box(make_members(1));
    let pointer = PointerId::PRIMARY;
    c.bench_function("GestureArena::sweep (1-member arena)", |b| {
        b.iter(|| {
            let _entry = arena.add(pointer, members[0].clone());
            arena.sweep(black_box(pointer));
        });
    });
}

/// Conflict resolution: an `Eager` member (wins on accept) and a
/// `Competitor` member (rejects). Eager accept → arena resolves in
/// Eager's favour → Competitor gets `reject_gesture`. This is the
/// tap-vs-eager-platform-view race on Android (`AndroidView`).
fn bench_resolve_conflict(c: &mut Criterion) {
    let members = black_box(make_members(2));
    let pointer = PointerId::PRIMARY;
    c.bench_function("GestureArena::add + accept (eager vs competitor)", |b| {
        b.iter(|| {
            let arena = GestureArena::new();
            let eager = arena.add(pointer, members[0].clone());
            let competitor = arena.add(pointer, members[1].clone());
            arena.close(pointer);
            eager.resolve(flui_interaction::arena::GestureDisposition::Accepted);
            competitor.resolve(flui_interaction::arena::GestureDisposition::Rejected);
            black_box(arena.is_empty());
        });
    });
}

/// Combined add + close + sweep — the full lifecycle cost of one
/// pointer-down → pointer-up cycle. This is the end-to-end hot-path
/// measurement that downstream `GestureBinding` sees per pointer event.
fn bench_full_lifecycle(c: &mut Criterion) {
    let members = black_box(make_members(1));
    let pointer = PointerId::PRIMARY;
    c.bench_function("GestureArena::add+close+sweep (full lifecycle)", |b| {
        b.iter(|| {
            let arena = GestureArena::new();
            let entry = arena.add(pointer, members[0].clone());
            arena.close(black_box(pointer));
            arena.sweep(black_box(pointer));
            black_box(entry.pointer());
        });
    });
}

criterion_group!(
    arena_benches,
    bench_add_empty,
    bench_add_busy,
    bench_sweep_empty,
    bench_resolve_conflict,
    bench_full_lifecycle,
);
criterion_main!(arena_benches);
