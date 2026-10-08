//! GestureArena benchmarks
//!
//! Hot path: `GestureArena::add` and `GestureArena::sweep` are called on
//! every pointer-down / pointer-up. `add` is the per-recogniser cost of
//! joining the arena; `sweep` is the per-pointer cleanup at the end of a
//! gesture sequence.
//!
//! Performance targets (per `docs/testing.md` and the constitution's 16 ms
//! frame budget):
//! - `add` of a single member into an empty arena: < 1 µs.
//! - `sweep` of a single-member arena: < 1 µs.
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

use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use flui_interaction::arena::{GestureArena, GestureArenaEntry, GestureArenaMember};
use flui_interaction::ids::PointerId;

/// Minimal arena member used as a recogniser stand-in.
///
/// Implements the public `GestureArenaMember` extension point so the bench can drive arena
/// dispatch without instantiating a real recogniser. The bench
/// measures arena cost, not the recogniser's per-event work, so
/// `accept_gesture` / `reject_gesture` are no-ops. The `id` field
/// is intentionally kept for debugging output (the bench fixture
/// could grow to per-id timings in a follow-up).
#[derive(Debug)]
struct BenchMember {
    #[expect(dead_code)] // retained for future per-id bench breakdown
    id: usize,
}

impl GestureArenaMember for BenchMember {
    fn accept_gesture(&self, _pointer: PointerId) {}
    fn reject_gesture(&self, _pointer: PointerId) {}
}

/// Pre-build a pool of recogniser handles so per-iteration setup is
/// just a borrowed member handle. Strong owners stay alive throughout every
/// measured interval because the arena retains only weak membership.
fn make_members(count: usize) -> Vec<Rc<BenchMember>> {
    (0..count).map(|i| Rc::new(BenchMember { id: i })).collect()
}

/// Benchmark `arena.add` against an empty arena. Each iteration
/// removes the previous entry so the next `add` is a fresh insert.
fn bench_add_empty(c: &mut Criterion) {
    let arena = GestureArena::new();
    let members = black_box(make_members(1));
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    c.bench_function("GestureArena::add (empty, 1 member)", |b| {
        b.iter(|| {
            arena.sweep(pointer);
            let entry = arena.add(black_box(pointer), black_box(&members[0]));
            black_box(entry.pointer());
        });
    });
}

/// Benchmark `arena.add` into a busy arena (4 pre-existing members for
/// the same pointer). The cost profile changes: SmallVec push from
/// inline (4) to heap-backed (5+). This is the realistic tap-vs-drag
/// case.
fn bench_add_busy(c: &mut Criterion) {
    let arena = GestureArena::new();
    let members = black_box(make_members(5));
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    // Pre-load 4 members so each `add` is into a 4-member arena.
    let _entries: Vec<GestureArenaEntry> =
        (0..4).map(|i| arena.add(pointer, &members[i])).collect();
    c.bench_function("GestureArena::add (busy, 4 prior members)", |b| {
        b.iter(|| {
            // The 5th member will trigger SmallVec heap growth on
            // subsequent iterations because we never sweep.
            let entry = arena.add(black_box(pointer), black_box(&members[4]));
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
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    c.bench_function("GestureArena::sweep (1-member arena)", |b| {
        b.iter(|| {
            let _entry = arena.add(pointer, &members[0]);
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
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    c.bench_function("GestureArena::add + accept (eager vs competitor)", |b| {
        b.iter(|| {
            let arena = GestureArena::new();
            let eager = arena.add(pointer, &members[0]);
            let competitor = arena.add(pointer, &members[1]);
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
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    c.bench_function("GestureArena::add+close+sweep (full lifecycle)", |b| {
        b.iter(|| {
            let arena = GestureArena::new();
            let entry = arena.add(pointer, &members[0]);
            arena.close(black_box(pointer));
            arena.sweep(black_box(pointer));
            black_box(entry.pointer());
        });
    });
}

#[derive(Default)]
struct ResolutionMember {
    accepted: Cell<u32>,
    rejected: Cell<u32>,
}

impl GestureArenaMember for ResolutionMember {
    fn accept_gesture(&self, _: PointerId) {
        self.accepted.set(self.accepted.get() + 1);
    }

    fn reject_gesture(&self, _: PointerId) {
        self.rejected.set(self.rejected.get() + 1);
    }
}

struct ResolutionFixture {
    arena: GestureArena,
    winner: Rc<ResolutionMember>,
    loser: Rc<ResolutionMember>,
    candidate: Rc<dyn GestureArenaMember>,
}

impl ResolutionFixture {
    fn new() -> Self {
        let arena = GestureArena::new();
        let winner = Rc::new(ResolutionMember::default());
        let loser = Rc::new(ResolutionMember::default());
        arena.add(PointerId::new(std::num::NonZeroU64::MIN), &winner);
        arena.add(PointerId::new(std::num::NonZeroU64::MIN), &loser);
        arena.close(PointerId::new(std::num::NonZeroU64::MIN));
        let candidate = winner.clone();
        Self {
            arena,
            winner,
            loser,
            candidate,
        }
    }

    fn resolve(&self) {
        self.arena.resolve(
            black_box(PointerId::new(std::num::NonZeroU64::MIN)),
            Some(black_box(&self.candidate)),
        );
        black_box(self.arena.is_empty());
    }
}

/// Resolution public-call cost: fixture setup and retirement are not timed.
/// The strong baseline's `resolve/strong` takes an owned Arc candidate, so
/// candidate cloning and argument retirement are timed; `resolve/weak` borrows
/// its Rc candidate and resolves weak membership. The comparison includes
/// these ownership contracts rather than isolating Rc versus Arc operations.
fn bench_weak_resolution(c: &mut Criterion) {
    let witness = ResolutionFixture::new();
    witness.resolve();
    assert!(witness.arena.is_empty(), "resolution settles its arena");
    assert_eq!(
        (witness.winner.accepted.get(), witness.winner.rejected.get()),
        (1, 0)
    );
    assert_eq!(
        (witness.loser.accepted.get(), witness.loser.rejected.get()),
        (0, 1)
    );
    c.bench_function("resolve/weak", |b| {
        b.iter_batched_ref(
            ResolutionFixture::new,
            |fixture| fixture.resolve(),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    arena_benches,
    bench_add_empty,
    bench_add_busy,
    bench_sweep_empty,
    bench_resolve_conflict,
    bench_full_lifecycle,
    bench_weak_resolution,
);
criterion_main!(arena_benches);
