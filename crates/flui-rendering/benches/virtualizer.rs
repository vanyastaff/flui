//! Virtualization core benchmark (ADR-0003 U1).
//!
//! Demonstrates the asymptotic win of the augmented B+-tree-backed
//! [`Virtualizer`] over a naive flat-array baseline on the three operations the
//! ADR rejected Fenwick for:
//!
//! - **offset → index** (scroll seek): tree is `O(log n)`, the naive linear scan
//!   is `O(n)`.
//! - **index → offset** (prefix sum): tree is `O(log n)`, the naive sum is
//!   `O(n)`.
//! - **structural growth** (dynamic-list build): the tree grows by `O(log n)`
//!   inserts (`O(n log n)` total); a Fenwick/BIT cannot insert mid-list at all —
//!   a structural edit shifts indices, forcing an `O(n)` rebuild of the
//!   cumulative structure (`O(n²)` total). This is the decisive reason the ADR
//!   rejected a Fenwick/BIT for a *dynamic* list, and what this bench isolates.
//!
//!   NB: the baseline here is the **Fenwick rebuild cost** (an `O(n)` arithmetic
//!   pass), *not* a plain `Vec::insert` shift. A `Vec::insert` is a single
//!   `memmove` of 4-byte floats — a SIMD/cache-optimal constant so small it
//!   *beats* the tree until ~5k items, which would understate the tree's win and
//!   measure the wrong alternative (a flat array is not the rejected Fenwick).
//!
//! Run with:
//!   cargo bench -p flui-rendering --bench virtualizer

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_rendering::virtualization::{ScrollWindow, Virtualizer};

/// Sizes spanning two orders of magnitude so the `log n` vs `n` gap is visible.
const SIZES: &[usize] = &[1_000, 10_000, 100_000];

/// Deterministic non-uniform extent for item `i` (so neither structure can
/// shortcut via uniform spacing).
#[inline]
fn extent_for(i: usize) -> f32 {
    (i % 17 + 1) as f32
}

/// Naive flat-array baseline: a `Vec` of extents. Seeks scan linearly; a
/// mid-list insert/remove shifts the tail. This is the `O(n)` structure the
/// tree beats (and the shape a Fenwick/BIT shares for structural edits).
struct NaiveExtents {
    extents: Vec<f32>,
}

impl NaiveExtents {
    fn new(count: usize) -> Self {
        Self {
            extents: (0..count).map(extent_for).collect(),
        }
    }

    /// `O(n)` prefix sum.
    fn offset_of(&self, index: usize) -> f32 {
        self.extents.iter().take(index).sum()
    }

    /// `O(n)` linear scan for the item containing `offset`.
    fn seek(&self, offset: f32) -> usize {
        let mut acc = 0.0;
        for (i, &e) in self.extents.iter().enumerate() {
            if acc + e > offset {
                return i;
            }
            acc += e;
        }
        self.extents.len().saturating_sub(1)
    }

    fn total(&self) -> f32 {
        self.extents.iter().sum()
    }
}

/// The ADR's rejected alternative: a Fenwick/BIT (modeled by a cumulative-sum
/// cache). It seeks in `O(log n)` — competitive with the tree — but **cannot
/// insert mid-list**: a structural edit shifts indices, so the cumulative
/// structure must be rebuilt in an `O(n)` arithmetic pass. That per-edit `O(n)`
/// (not the `O(log n)` of a tree rebalance) is exactly why a Fenwick is the
/// wrong tool for a *dynamic* list, and the cost this baseline isolates.
struct FenwickRebuildBaseline {
    extents: Vec<f32>,
    /// `cumulative[i] = sum(extents[0..=i])`; rebuilt `O(n)` after every edit.
    cumulative: Vec<f32>,
}

impl FenwickRebuildBaseline {
    fn new() -> Self {
        Self {
            extents: Vec::new(),
            cumulative: Vec::new(),
        }
    }

    /// Insert one item, then rebuild the cumulative array — an `O(n)` arithmetic
    /// pass, the structural-edit cost a Fenwick/BIT pays (it has no `O(log n)`
    /// insert; index shift invalidates the whole prefix structure).
    fn insert_and_rebuild(&mut self, index: usize, extent: f32) {
        self.extents.insert(index, extent);
        self.cumulative.clear();
        self.cumulative.reserve(self.extents.len());
        let mut acc = 0.0;
        for &e in &self.extents {
            acc += e;
            self.cumulative.push(acc);
        }
    }

    fn total(&self) -> f32 {
        self.cumulative.last().copied().unwrap_or(0.0)
    }
}

fn build_virtualizer(count: usize) -> Virtualizer {
    let mut v = Virtualizer::new(count, 1.0);
    for i in 0..count {
        v.set_measured(i, extent_for(i), (0, 0.0));
    }
    v
}

// ============================================================================
// offset -> index seek
// ============================================================================

fn bench_seek_offset_to_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("virtualizer/seek_offset_to_index");
    for &n in SIZES {
        let v = build_virtualizer(n);
        let naive = NaiveExtents::new(n);
        let probe = naive.total() / 2.0; // mid-content offset

        group.bench_with_input(BenchmarkId::new("tree_log_n", n), &n, |b, _| {
            // `query` takes `&self` — the returned range is a pure function of
            // the window and current extents, so the captured `v` is reused
            // across iterations with no per-iter clone or mutation.
            b.iter(|| {
                let r = v.query(&ScrollWindow::new(black_box(probe), 1.0));
                black_box(r.first)
            });
        });
        group.bench_with_input(BenchmarkId::new("naive_linear", n), &n, |b, _| {
            b.iter(|| black_box(naive.seek(black_box(probe))));
        });
    }
    group.finish();
}

// ============================================================================
// Realistic windowed query (the production hot path: dual visible + cache band)
// ============================================================================
//
// The single-point seek above uses a 1px window, collapsing all four band edges
// to one point — the best case for the shared descent. This models a real
// viewport instead: an 800px visible band with a 250px cache buffer each side.
// The four edges (cache/visible start/end) still cluster within ~1300px, tiny
// against a 100k-item list, so `query`'s `seek_sorted` resolves them in one
// shared-prefix descent. The naive baseline must linear-scan once per edge.

/// Realistic viewport main-axis extent for the windowed-query bench.
const VIEWPORT: f32 = 800.0;
/// Realistic cache buffer kept on each side of the viewport.
const CACHE_SIDE: f32 = 250.0;

fn bench_query_window(c: &mut Criterion) {
    let mut group = c.benchmark_group("virtualizer/query_window");
    for &n in SIZES {
        let v = build_virtualizer(n);
        let naive = NaiveExtents::new(n);
        // Centre the viewport in the content (all SIZES have total >> VIEWPORT).
        let offset = (naive.total() - VIEWPORT) / 2.0;
        let window = ScrollWindow {
            offset,
            main_extent: VIEWPORT,
            cache_before: CACHE_SIDE,
            cache_after: CACHE_SIDE,
        };

        group.bench_with_input(BenchmarkId::new("tree_shared_descent", n), &n, |b, _| {
            b.iter(|| {
                let r = v.query(black_box(&window));
                black_box((r.first, r.last, r.cache_first, r.cache_last))
            });
        });
        group.bench_with_input(BenchmarkId::new("naive_linear_x4", n), &n, |b, _| {
            // A flat array has no shared descent: one O(n) scan per band edge.
            b.iter(|| {
                let e0 = naive.seek(black_box(offset - CACHE_SIDE));
                let e1 = naive.seek(black_box(offset));
                let e2 = naive.seek(black_box(offset + VIEWPORT));
                let e3 = naive.seek(black_box(offset + VIEWPORT + CACHE_SIDE));
                black_box((e0, e1, e2, e3))
            });
        });
    }
    group.finish();
}

// ============================================================================
// index -> offset seek
// ============================================================================

fn bench_seek_index_to_offset(c: &mut Criterion) {
    let mut group = c.benchmark_group("virtualizer/seek_index_to_offset");
    for &n in SIZES {
        let v = build_virtualizer(n);
        let naive = NaiveExtents::new(n);
        let probe = n / 2;

        group.bench_with_input(BenchmarkId::new("tree_log_n", n), &n, |b, _| {
            b.iter(|| black_box(v.offset_of(black_box(probe))));
        });
        group.bench_with_input(BenchmarkId::new("naive_linear", n), &n, |b, _| {
            b.iter(|| black_box(naive.offset_of(black_box(probe))));
        });
    }
    group.finish();
}

// ============================================================================
// Dynamic-list structural growth (the Fenwick-vs-SumTree decider)
// ============================================================================
//
// The decisive structural difference is not a single edit but *repeated*
// structural edits at a non-tail position — the dynamic-list workload (inserts,
// reorders, infinite feeds) the ADR rejected a Fenwick/BIT for. We build a list
// of `n` items by inserting one at a time at the *front*:
//
// - **tree**: each insert is `O(log n)`; building the list is `O(n log n)`.
//   (Driven through the public `set_count`, whose growth path is the same
//   `O(log n)` tree insert — the backing tree never shifts, it rebalances.)
// - **Fenwick rebuild**: each insert shifts indices and forces an `O(n)`
//   arithmetic rebuild of the cumulative structure, so building the list is
//   `O(n²)` — the structural-edit cost that disqualified a Fenwick/BIT. (A plain
//   `Vec::insert` memmove would be the wrong baseline: its constant is so small
//   it beats the tree until ~5k items and it isn't the rejected alternative.)
//
// Sizes are kept modest because the Fenwick baseline is `O(n²)`; the asymptotic
// separation is visible from the smallest size up.

/// Sizes for the `O(n²)` Fenwick-rebuild structural bench (kept modest so the
/// quadratic baseline stays in a measurable range).
const GROWTH_SIZES: &[usize] = &[256, 1_024, 4_096];

fn bench_structural_growth(c: &mut Criterion) {
    let mut group = c.benchmark_group("virtualizer/structural_growth");
    for &n in GROWTH_SIZES {
        group.bench_with_input(BenchmarkId::new("tree_n_log_n", n), &n, |b, _| {
            b.iter(|| {
                // Grow 0 -> n one item at a time; each step is an O(log n) tree
                // insert (no element shift).
                let mut v = Virtualizer::new(0, 1.0);
                for k in 1..=n {
                    v.set_count(black_box(k));
                }
                black_box(v.len())
            });
        });
        group.bench_with_input(BenchmarkId::new("fenwick_rebuild", n), &n, |b, _| {
            b.iter(|| {
                // Build the same list by front-insertion; each step rebuilds the
                // cumulative array O(n), so the build is O(n^2) — the Fenwick
                // structural-edit cost.
                let mut fenwick = FenwickRebuildBaseline::new();
                for _ in 0..n {
                    fenwick.insert_and_rebuild(0, black_box(1.0));
                }
                black_box(fenwick.total())
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_seek_offset_to_index,
    bench_query_window,
    bench_seek_index_to_offset,
    bench_structural_growth
);
criterion_main!(benches);
