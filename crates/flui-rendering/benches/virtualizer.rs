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
//!   inserts (`O(n log n)` total); the flat array shifts every later element on
//!   each non-tail insert (`O(n)` per op, `O(n²)` total). This is the decisive
//!   reason for a tree over a Fenwick/BIT, which is *also* `O(n)` per structural
//!   edit.
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

    /// `O(n)` mid-list insert (tail shift).
    fn insert(&mut self, index: usize, extent: f32) {
        self.extents.insert(index, extent);
    }

    fn total(&self) -> f32 {
        self.extents.iter().sum()
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
        let mut v = build_virtualizer(n);
        let naive = NaiveExtents::new(n);
        let probe = naive.total() / 2.0; // mid-content offset

        group.bench_with_input(BenchmarkId::new("tree_log_n", n), &n, |b, _| {
            // `query` takes `&mut self` only to record the viewport extent; the
            // returned range is a pure function of the window, so the captured
            // `v` is reused across iterations with no per-iter clone.
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
// reorders, infinite feeds) the ADR rejected a flat-array Fenwick/BIT for. We
// build a list of `n` items by inserting one at a time at the *front*:
//
// - **tree**: each insert is `O(log n)`; building the list is `O(n log n)`.
//   (Driven through the public `set_count`, whose growth path is the same
//   `O(log n)` tree insert — the backing tree never shifts, it rebalances.)
// - **naive**: each `Vec::insert(0, _)` shifts every existing element, so it is
//   `O(n)`; building the list is `O(n²)` — exactly the cost a Fenwick/BIT also
//   pays on a structural edit.
//
// Sizes are smaller here than the seek benches because the naive baseline is
// quadratic; even so the asymptotic separation is unmistakable.

/// Sizes for the quadratic-baseline structural bench (kept modest so the naive
/// `O(n²)` build stays in a measurable range).
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
        group.bench_with_input(BenchmarkId::new("naive_quadratic", n), &n, |b, _| {
            b.iter(|| {
                // Build the same list by front-insertion; each step shifts the
                // whole tail, so the build is O(n^2).
                let mut naive = NaiveExtents {
                    extents: Vec::new(),
                };
                for _ in 0..n {
                    naive.insert(0, black_box(1.0));
                }
                black_box(naive.total())
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_seek_offset_to_index,
    bench_seek_index_to_offset,
    bench_structural_growth
);
criterion_main!(benches);
