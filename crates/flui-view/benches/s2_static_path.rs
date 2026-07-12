//! S2 static-path tuple-permutation algorithm sketch bench.
//!
//! Resolves the spec's Deferred S2 question against a synthetic mock: does a
//! compile-time-specialised reconciliation algorithm for the static-tuple
//! `ViewSeq` path (`(A, B, ..., P)`, fixed arity 16 per FR-013) beat the
//! linear keyed algorithm FR-016 commits both paths to?
//!
//! Phase 0 (this bench) does **not** modify production reconciliation code —
//! the spec FR-016 commits to "both paths share the same algorithm" absent a
//! strong S2 inversion. The companion [`docs/research/2026-05-22-s2-static-path-sketch.md`]
//! synthesises these numbers into a verdict; the gate report consumes that
//! verdict to decide whether to re-open FR-016 before a later phase lands the
//! production reconciler.
//!
//! # The static-path observation
//!
//! In a true static-tuple `(A, B, C, ..., P)` setting, "reordering" means a
//! literally different type signature — `(C, A, B)` is not a permutation of
//! `(A, B, C)` at the type level; it is a different generic type. The
//! keyed-state-preserving reorder problem the linear algorithm solves is
//! structurally absent at the static path. The S2 question is whether a
//! specialised algorithm produces meaningfully better perf at the fixed
//! 16-position grain, given that the linear algorithm is over-engineered for
//! this case.
//!
//! # Bench groups
//!
//! - **`s2_static_path/linear_keyed/full_reverse`** — placeholder linear
//!   keyed reconcile workload over the 16-tuple, full-reverse permutation.
//!   This is the FR-016 baseline shape applied at the 16-tuple scale.
//! - **`s2_static_path/positional_specialised/full_reverse`** — pure
//!   static-path shape: walk 16 positions, compare `TypeId` per slot, no
//!   cross-position lookup. The shape a real
//!   `const fn reconcile_tuple_16<A, B, ..., P>(...)` would compile to.
//! - **`s2_static_path/reorder_specialised/full_reverse`** — apples-to-apples
//!   comparison against the linear keyed algorithm: stack-allocated
//!   `[Option<u8>; 16]` index over `key_hash`-bucketed old positions, no
//!   HashMap, no heap allocation.
//!
//! # Why a placeholder reconciler
//!
//! Same justification as the S1 storage-shape bench: the production keyed reconciler
//! does not land until a later phase (and its callers don't get rewired until
//! still later). The S2 question is an *algorithm-shape* question at the static-tuple
//! grain, not a *production-reconciler* question. The minimal kernel that
//! isolates algorithm-shape cost is a per-position TypeId-comparison loop
//! (specialised) vs a HashMap-build-and-probe loop (linear). That is what we
//! measure.
//!
//! # Plan / spec references
//!
//! - [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`]
//! - [`specs/004-view-element-core/spec.md`] Deferred S2, FR-016

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.
#![allow(missing_docs)]

#[path = "shared/mock_tuple.rs"]
mod mock_tuple;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use crate::mock_tuple::{
    identity_slots, reconcile_linear_keyed, reconcile_positional_specialised,
    reconcile_reorder_specialised, reversed_slots,
};

// ----------------------------------------------------------------------------
// Reconcile latency benches — linear keyed (FR-016 baseline)
// ----------------------------------------------------------------------------

/// Reconcile workload over 16-tuple full-reverse permutation through the
/// linear keyed algorithm — the FR-016 baseline shape. Per iteration we
/// reconstruct the old + new slot arrays via `iter_batched` so the HashMap
/// allocation cost is included; that is the cost a frame actually pays.
fn bench_static_path_keyed_linear(c: &mut Criterion) {
    c.bench_function("s2_static_path/linear_keyed/full_reverse", |b| {
        b.iter_batched(
            || (identity_slots(), reversed_slots()),
            |(old, new)| {
                let result = reconcile_linear_keyed(&old, &new);
                black_box(result);
            },
            BatchSize::SmallInput,
        );
    });
}

// ----------------------------------------------------------------------------
// Reconcile latency benches — positional specialised (pure static-path shape)
// ----------------------------------------------------------------------------

/// Reconcile workload over 16-tuple full-reverse permutation through the
/// positional-only specialised algorithm — the pure static-path shape with
/// no cross-position lookup. Same `iter_batched` discipline so the per-frame
/// cost comparison is honest.
///
/// Note: at the full-reverse input, this algorithm reports every position as
/// `Replace` (since `old[i].type_id != new[i].type_id` at every position when
/// the array is reversed and arity > 1). That is **the correct static-path
/// answer** — at the static path, a reversed tuple is a different `ViewSeq`
/// type and the framework's positional reconciler is the right answer.
fn bench_static_path_specialised(c: &mut Criterion) {
    c.bench_function("s2_static_path/positional_specialised/full_reverse", |b| {
        b.iter_batched(
            || (identity_slots(), reversed_slots()),
            |(old, new)| {
                let result = reconcile_positional_specialised(&old, &new);
                black_box(result);
            },
            BatchSize::SmallInput,
        );
    });
}

// ----------------------------------------------------------------------------
// Reconcile latency benches — reorder-aware specialised
// ----------------------------------------------------------------------------

/// Reconcile workload over 16-tuple full-reverse permutation through the
/// reorder-aware specialised algorithm — same O(N) big-O as the linear
/// keyed algorithm, but stack-allocated `[Option<u8>; 16]` index instead of
/// `HashMap<u64, u8>`. This is the apples-to-apples comparison the verdict
/// cares about.
fn bench_static_path_specialised_reorder(c: &mut Criterion) {
    c.bench_function("s2_static_path/reorder_specialised/full_reverse", |b| {
        b.iter_batched(
            || (identity_slots(), reversed_slots()),
            |(old, new)| {
                let result = reconcile_reorder_specialised(&old, &new);
                black_box(result);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_static_path_keyed_linear,
    bench_static_path_specialised,
    bench_static_path_specialised_reorder,
);
criterion_main!(benches);
