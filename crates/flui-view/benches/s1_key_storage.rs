//! U2 — S1 KeyId interning prototype bench.
//!
//! Resolves the spec's Deferred S1 question against a synthetic mock: does
//! `Option<KeyId>` (NonZeroU64 newtype, 8 bytes) beat the `Option<Box<dyn ViewKey>>`
//! (fat-pointer, 16 bytes + heap) baseline by a material margin on a 10K-element
//! 80% unkeyed-leaf / 20% keyed-branch distribution?
//!
//! Phase 0 (this bench) does **not** modify production `ElementNode` storage —
//! the spec FR-022 commits to the boxed-dyn baseline absent a strong S1 inversion.
//! The U4 gate-report uses these numbers to decide whether to re-open FR-022
//! before Phase 1 lands the storage shape.
//!
//! # Bench groups
//!
//! - **`s1_reconcile/baseline_box_dyn/{permutation}`** — placeholder keyed
//!   reconcile workload over the boxed-dyn shape, three permutations.
//! - **`s1_reconcile/interned_key_id/{permutation}`** — same workload, interned
//!   shape, three permutations.
//! - **`s1_hash_lookup/baseline_box_dyn`** + **`s1_hash_lookup/interned_key_id`**
//!   — narrow latency probe for the HashMap-hit path. Strips the
//!   per-iteration construction cost; measures only the inner lookup loop.
//! - **`s1_memory/baseline_box_dyn`** + **`s1_memory/interned_key_id`** —
//!   single-iteration deterministic memory accounting (printed via Criterion
//!   so the value lands in the report alongside the latency numbers).
//!
//! # Why a placeholder reconciler
//!
//! The production keyed reconciler does not land until Phase 2 U12 (and its
//! callers don't get rewired until U15). The S1 question is a *storage-shape*
//! question, not a *reconciler-algorithm* question. The minimal kernel that
//! isolates the storage-shape cost is a `HashMap<key_hash, idx>` build +
//! per-new-position lookup. That is what we measure.
//!
//! # Plan / spec references
//!
//! - [`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`] U2
//! - [`specs/004-view-element-core/spec.md`] Deferred S1, FR-022

#[path = "shared/mock_node.rs"]
mod mock_node;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use crate::mock_node::{
    MemoryAccounting, NODE_COUNT, Permutation, build_baseline_nodes, build_interned_nodes,
    build_keyed_map_baseline, build_keyed_map_interned, identity_order, lookup_only_baseline,
    lookup_only_interned, reconcile_baseline_keyed, reconcile_interned_keyed,
};

// ----------------------------------------------------------------------------
// Reconcile latency benches — baseline (boxed dyn) shape
// ----------------------------------------------------------------------------

/// Reconcile workload over `Option<Box<dyn ViewKey>>`-keyed nodes. Per iteration
/// we rebuild the node distribution from scratch via `iter_batched` so the
/// HashMap allocation cost is included; that is the cost a frame actually pays.
fn bench_reconcile_baseline_box_dyn(c: &mut Criterion) {
    let mut group = c.benchmark_group("s1_reconcile/baseline_box_dyn");
    for &perm in Permutation::ALL {
        group.bench_function(perm.name(), |b| {
            b.iter_batched(
                || {
                    let nodes = build_baseline_nodes();
                    let mut order = identity_order();
                    perm.apply(&mut order);
                    (nodes, order)
                },
                |(nodes, order)| {
                    let matches = reconcile_baseline_keyed(&nodes, &order);
                    black_box(matches);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------------------------
// Reconcile latency benches — interned (KeyId) shape
// ----------------------------------------------------------------------------

/// Reconcile workload over `Option<KeyId>`-keyed nodes. Each iteration builds a
/// fresh interner — we explicitly want construction-included cost so the gate
/// report compares apples-to-apples against the boxed-dyn shape.
fn bench_reconcile_interned_key_id(c: &mut Criterion) {
    let mut group = c.benchmark_group("s1_reconcile/interned_key_id");
    for &perm in Permutation::ALL {
        group.bench_function(perm.name(), |b| {
            b.iter_batched(
                || {
                    let (nodes, interner) = build_interned_nodes();
                    let mut order = identity_order();
                    perm.apply(&mut order);
                    (nodes, interner, order)
                },
                |(nodes, _interner, order)| {
                    // _interner is kept alive across the measurement so the
                    // KeyId values remain meaningful; the reconcile kernel does
                    // not dereference into it (the bench's whole point — KeyId
                    // is a free u64 read).
                    let matches = reconcile_interned_keyed(&nodes, &order);
                    black_box(matches);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------------------------
// Hash-lookup latency probes — narrow inner-loop measurement
// ----------------------------------------------------------------------------

/// Narrow probe: per-position `key_hash()` dispatch + HashMap `contains_key`.
/// Strips the per-iteration construction cost from the outer reconcile bench
/// (the construction overhead amortised across all 10K nodes in `iter_batched`'s
/// setup phase). What remains is the inner walk cost — the column the S1
/// verdict cares about.
fn bench_hash_lookup_baseline_box_dyn(c: &mut Criterion) {
    let nodes = build_baseline_nodes();
    let order = identity_order();
    // Pre-build the keyed map OUTSIDE `b.iter` per Codex review #5 — the
    // hash-lookup probe must measure lookup latency in isolation, not lookup
    // + per-iteration map construction. The map build cost was previously
    // contaminating the lookup column of the S1 verdict.
    let map = build_keyed_map_baseline(&nodes);
    c.bench_function("s1_hash_lookup/baseline_box_dyn", |b| {
        b.iter(|| {
            let matches =
                lookup_only_baseline(black_box(&nodes), black_box(&map), black_box(&order));
            black_box(matches);
        });
    });
}

fn bench_hash_lookup_interned_key_id(c: &mut Criterion) {
    let (nodes, _interner) = build_interned_nodes();
    let order = identity_order();
    // Same pre-built-map discipline as the baseline probe — see Codex #5.
    let map = build_keyed_map_interned(&nodes);
    c.bench_function("s1_hash_lookup/interned_key_id", |b| {
        b.iter(|| {
            let matches =
                lookup_only_interned(black_box(&nodes), black_box(&map), black_box(&order));
            black_box(matches);
        });
    });
}

// ----------------------------------------------------------------------------
// Memory accounting — deterministic per-shape snapshot
// ----------------------------------------------------------------------------

/// Per-shape resident-bytes accounting, exposed as a Criterion bench so the
/// numbers land in the same `target/criterion/` report as the latency groups.
/// The bench body recomputes [`MemoryAccounting::for_baseline`] per iteration
/// (a pure `const`-style arithmetic over `NODE_COUNT`) so the value is
/// reproducibly visible in the report without `println!` (forbidden by
/// Constitution Principle 6). The U4 gate report reads the canonical totals
/// from this source file's accounting struct, not from runtime output.
fn bench_memory_baseline_box_dyn(c: &mut Criterion) {
    c.bench_function("s1_memory/baseline_box_dyn", |b| {
        b.iter(|| {
            let a = black_box(MemoryAccounting::for_baseline(NODE_COUNT));
            black_box(a.total());
        });
    });
}

fn bench_memory_interned_key_id(c: &mut Criterion) {
    c.bench_function("s1_memory/interned_key_id", |b| {
        b.iter(|| {
            let a = black_box(MemoryAccounting::for_interned(NODE_COUNT));
            black_box(a.total());
        });
    });
}

criterion_group!(
    benches,
    bench_reconcile_baseline_box_dyn,
    bench_reconcile_interned_key_id,
    bench_hash_lookup_baseline_box_dyn,
    bench_hash_lookup_interned_key_id,
    bench_memory_baseline_box_dyn,
    bench_memory_interned_key_id,
);
criterion_main!(benches);
