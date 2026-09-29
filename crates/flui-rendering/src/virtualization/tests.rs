//! Tests for the protocol-agnostic [`Virtualizer`].
//!
//! Two layers:
//! - **Unit tests** pin specific behaviours (dual-range query, anchor
//!   correction, Estimated→Exact, boundary/edge cases, `O(log n)` seek scaling).
//! - **Property tests** ([`prop`]) run random op sequences against a naive
//!   `Vec<ItemExtent>` oracle, asserting the windowing invariants hold and the
//!   backing tree stays balanced.

use super::*;

const EPS: f64 = 1e-3;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS
}

// ============================================================================
// Construction, len, total_extent, Estimated -> Exact
// ============================================================================

#[test]
fn estimated_to_exact_transition() {
    let mut v = Virtualizer::new(3, 10.0);
    assert_eq!(v.total_extent(), Extent::Estimated(30.0));

    // Measure first two — still estimated (item 2 outstanding).
    assert_eq!(v.set_measured(0, 12.0, (0, 0.0)), None);
    assert_eq!(v.set_measured(1, 8.0, (0, 0.0)), None);
    assert_eq!(v.measured_count(), 2);
    assert_eq!(v.estimated_count(), 1);
    match v.total_extent() {
        Extent::Estimated(t) => assert!(approx(t, 12.0 + 8.0 + 10.0)),
        other @ Extent::Exact(_) => {
            panic!("expected Estimated while a prefix is unmeasured, got {other:?}")
        }
    }

    // Measure the last — now Exact.
    assert_eq!(v.set_measured(2, 20.0, (0, 0.0)), None);
    assert_eq!(v.measured_count(), 3);
    assert_eq!(v.estimated_count(), 0);
    assert_eq!(v.total_extent(), Extent::Exact(12.0 + 8.0 + 20.0));
}

// ============================================================================
// query: dual band + leading_offset
// ============================================================================

#[test]
fn query_dual_band_with_cache() {
    // 20 items x 10px. Viewport [50,80) + cache 20 before / 30 after.
    let v = Virtualizer::new(20, 10.0);
    let window = ScrollWindow {
        offset: 50.0,
        main_extent: 30.0,
        cache_before: 20.0,
        cache_after: 30.0,
    };
    let r = v.query(&window);
    // visible [50,80) -> items 5,6,7
    assert_eq!((r.first, r.last), (5, 8));
    // cache [30, 110) -> items 3..11
    assert_eq!((r.cache_first, r.cache_last), (3, 11));
    // Containment invariant.
    assert!(r.cache_first <= r.first && r.last <= r.cache_last);
}

// ============================================================================
// Anchor correction — the jitter killer
// ============================================================================

#[test]
fn anchor_correction_keeps_content_stationary() {
    // Concretely: anchor item's pixel position must move by exactly `delta`,
    // and adding `delta` to the scroll offset cancels it.
    let mut v = Virtualizer::new(10, 10.0);
    let anchor_idx = 5;
    let before = v.offset_of(anchor_idx); // 50
    let corr = v
        .set_measured(2, 17.0, (anchor_idx, 0.0))
        .expect("above-anchor re-measure must emit a correction");
    let after = v.offset_of(anchor_idx); // 57
    assert!(
        approx(after - before, corr.delta),
        "anchor moved by {} but correction was {}",
        after - before,
        corr.delta
    );
}

// ============================================================================
// set_count — O(log n) structural edits
// ============================================================================

#[test]
fn set_count_shrink_clamps_anchor() {
    let mut v = Virtualizer::new(10, 10.0);
    v.set_measured(8, 10.0, (8, 0.0)); // anchor at 8
    v.set_count(3);
    assert_eq!(
        v.anchor_item(),
        (2, 0.0),
        "anchor clamps to last valid index"
    );
}

// ============================================================================
// invalidate_from
// ============================================================================

// ============================================================================
// scroll_to_item
// ============================================================================

#[test]
fn scroll_to_item_alignment_uses_viewport() {
    let mut v = Virtualizer::new(10, 10.0); // total 100
    // Item 5 starts at 50, extent 10; viewport 40 supplied by the caller.
    // leading (a=0): 50
    assert!(approx(v.scroll_to_item(5, 0.0, 40.0), 50.0));
    // trailing (a=1): 50 - (40 - 10) = 20
    assert!(approx(v.scroll_to_item(5, 1.0, 40.0), 20.0));
    // center (a=0.5): 50 - 0.5*(40-10) = 35
    assert!(approx(v.scroll_to_item(5, 0.5, 40.0), 35.0));
}

// ============================================================================
// O(log n) seek both directions on 10k items
// ============================================================================

// ============================================================================
// Boundary edge cases: offset==0 and offset==total
// ============================================================================

// ============================================================================
// Property tests vs a naive Vec<ItemExtent> oracle
// ============================================================================

/// A float in `[lo, hi]` drawn WITHOUT proptest's uniform float sampler.
///
/// That sampler panics from inside its own strategy on some seeds (#889:
/// `assertion failed: self.low - result < self.intervals.step`,
/// `proptest-1.11.0/src/num/float_samplers.rs:466`), which fires while
/// *generating* a value, before any assertion here runs.
///
/// The grid is 2^24 steps, not a round decimal, so subpixel and
/// near-degenerate geometry stays reachable: across a 20,000 px range that is
/// ~0.0012 px between neighbours. A coarse grid would quietly narrow these
/// tests to whole-pixel cases, which is the opposite of what a geometry
/// property suite is for.
fn extent_in(lo: f64, hi: f64) -> impl proptest::strategy::Strategy<Value = f64> {
    const STEPS: u32 = 1 << 24;
    use proptest::strategy::Strategy as _;
    (0u32..=STEPS).prop_map(move |n| {
        let t = f64::from(n) / f64::from(STEPS);
        lo + t * (hi - lo)
    })
}

mod prop {
    use super::*;
    use proptest::prelude::*;

    /// Compares two extent sums that were accumulated in different orders (the
    /// tree folds leaf→summary→total, the oracle folds left→right), tolerating
    /// f64 round-off proportional to the magnitude. The invariant being checked
    /// is "same sum up to float accumulation", not bit-exact equality — exact
    /// equality across two summation orders is not a real property of f64.
    fn approx_sum(a: f64, b: f64) -> bool {
        let tol = 1e-3 + 1e-4 * a.abs().max(b.abs());
        (a - b).abs() <= tol
    }

    /// Naive reference model: a flat `Vec` of extents. Every invariant the
    /// `Virtualizer` claims is checked against this `O(n)` oracle.
    #[derive(Debug, Clone, Default)]
    struct Oracle {
        items: Vec<ItemExtent>,
        default_estimate: f64,
    }

    impl Oracle {
        fn new(count: usize, default_estimate: f64) -> Self {
            Self {
                items: vec![
                    ItemExtent::Unmeasured {
                        hint: default_estimate
                    };
                    count
                ],
                default_estimate,
            }
        }

        fn set_count(&mut self, n: usize) {
            let est = self.default_estimate;
            self.items.resize(n, ItemExtent::Unmeasured { hint: est });
        }

        fn set_measured(&mut self, index: usize, extent: f64) {
            if index < self.items.len() {
                self.items[index] = ItemExtent::Measured {
                    extent: extent.max(0.0),
                };
            }
        }

        fn invalidate_from(&mut self, index: usize) {
            let est = self.default_estimate;
            for it in self.items.iter_mut().skip(index) {
                *it = ItemExtent::Unmeasured { hint: est };
            }
        }

        fn total(&self) -> f64 {
            self.items.iter().map(ItemExtent::extent).sum()
        }

        fn offset_of(&self, index: usize) -> f64 {
            self.items.iter().take(index).map(ItemExtent::extent).sum()
        }

        fn measured_count(&self) -> usize {
            self.items.iter().filter(|i| i.is_measured()).count()
        }

        /// First item whose span `[start, start+extent)` contains `offset`
        /// (clamped to `[0, total]`), matching the tree's seek contract.
        fn seek(&self, offset: f64) -> usize {
            let n = self.items.len();
            if n == 0 {
                return 0;
            }
            let total = self.total();
            if offset <= 0.0 {
                return 0;
            }
            if offset >= total {
                return n - 1;
            }
            let mut acc = 0.0;
            for (i, it) in self.items.iter().enumerate() {
                let e = it.extent();
                if acc + e > offset {
                    return i;
                }
                acc += e;
            }
            n - 1
        }
    }

    /// One randomized operation against both the `Virtualizer` and the oracle.
    #[derive(Debug, Clone)]
    enum Op {
        SetCount(usize),
        SetMeasured { index: usize, extent: f64 },
        InvalidateFrom(usize),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..200).prop_map(Op::SetCount),
            (0usize..200, extent_in(0.1, 100.0))
                .prop_map(|(index, extent)| Op::SetMeasured { index, extent }),
            (0usize..200).prop_map(Op::InvalidateFrom),
        ]
    }

    /// Applies `op` to both models. `index`-bearing ops are taken modulo the
    /// current length so they stay in range as the list resizes.
    fn apply(op: &Op, v: &mut Virtualizer, oracle: &mut Oracle) {
        match *op {
            Op::SetCount(n) => {
                v.set_count(n);
                oracle.set_count(n);
            }
            Op::SetMeasured { index, extent } => {
                if !oracle.items.is_empty() {
                    let i = index % oracle.items.len();
                    let anchor = v.anchor_item();
                    v.set_measured(i, extent, anchor);
                    oracle.set_measured(i, extent);
                }
            }
            Op::InvalidateFrom(index) => {
                let len = oracle.items.len();
                let i = if len == 0 { 0 } else { index % (len + 1) };
                v.invalidate_from(i);
                oracle.invalidate_from(i);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        /// (a) total_extent == naive sum; (b) offset_of == naive prefix-sum;
        /// (c) query round-trips; (d) tree stays balanced; (e) count matches.
        #[test]
        fn invariants_hold_under_random_ops(
            initial_count in 0usize..100,
            default_estimate in extent_in(0.5, 50.0),
            ops in proptest::collection::vec(op_strategy(), 0..120),
        ) {
            let mut v = Virtualizer::new(initial_count, default_estimate);
            let mut oracle = Oracle::new(initial_count, default_estimate);

            for op in &ops {
                apply(op, &mut v, &mut oracle);

                // (e) count.
                prop_assert_eq!(v.len(), oracle.items.len());

                // (d) balance + summary correctness (debug-only invariant check).
                v.tree
                    .check_invariants()
                    .map_err(|e| TestCaseError::fail(format!("tree invariant: {e}")))?;
                // Depth must be logarithmic: a balanced B-tree with branching
                // factor >= MIN(=B) over n items has depth <= log_B(n) + 1.
                let n = v.len();
                let max_depth = depth_bound(n);
                prop_assert!(
                    v.tree.depth() <= max_depth,
                    "depth {} exceeds log-bound {} for n={}",
                    v.tree.depth(), max_depth, n
                );

                // (a) total.
                prop_assert!(
                    approx_sum(v.total_extent().value(), oracle.total()),
                    "total {} != oracle {}", v.total_extent().value(), oracle.total()
                );

                // measured_count must agree, and the Exact/Estimated tag with it.
                prop_assert_eq!(v.measured_count(), oracle.measured_count());
                let all_measured = v.len() == v.measured_count();
                match v.total_extent() {
                    Extent::Exact(_) => prop_assert!(all_measured),
                    Extent::Estimated(_) => prop_assert!(!all_measured),
                }
            }

            // (b) offset_of == naive prefix-sum, at every index incl. len().
            for i in 0..=v.len() {
                prop_assert!(
                    approx_sum(v.offset_of(i), oracle.offset_of(i)),
                    "offset_of({}) {} != oracle {}", i, v.offset_of(i), oracle.offset_of(i)
                );
            }

            // (c) query round-trip. The robust law is tree-self-consistent span
            // containment: the item `query` returns for `off` has a span (per the
            // tree's own offset_of) that contains `off`. We also check agreement
            // with the oracle's seek, tolerating a ±1 index difference only when
            // `off` is within float tolerance of the shared item boundary (a
            // sampled offset can land exactly on an edge, where two summation
            // orders legitimately disagree on which side it falls).
            let total = oracle.total();
            if !v.is_empty() && total > 0.0 {
                for k in 0..=20u32 {
                    let off = total * (k as f64) / 20.0;
                    let r = v.query(&ScrollWindow::new(off, 1.0));

                    if off < total {
                        let start = v.offset_of(r.first);
                        let end = v.offset_of(r.first + 1);
                        let tol = 1e-3 + 1e-4 * total;
                        prop_assert!(
                            start <= off + tol && off < end + tol,
                            "offset {} not within item {}'s span [{}, {})",
                            off, r.first, start, end
                        );
                    }

                    let oracle_idx = oracle.seek(off);
                    let agree = r.first == oracle_idx
                        || (r.first.abs_diff(oracle_idx) == 1
                            && approx_sum(v.offset_of(r.first.max(oracle_idx)), off));
                    prop_assert!(
                        agree,
                        "query.first {} vs oracle {} for offset {} (not a boundary tie)",
                        r.first, oracle_idx, off
                    );
                }
            }
        }

    }

    /// A provably-safe upper bound on the depth of the balanced B-tree holding
    /// `n` items, independent of the exact branching factor.
    ///
    /// Every non-root internal node holds at least 2 children (the tree's `MIN`
    /// is well above 2), so each level at least doubles the item capacity:
    /// `capacity(depth) >= 2^(depth-1)`. Hence `depth <= log2(n) + 2`. This is a
    /// loose `O(log n)` envelope — its only job is to catch a tree that has gone
    /// linear (an unbalanced-shape regression), not to assert a tight constant.
    fn depth_bound(n: usize) -> usize {
        let mut bound = 1usize;
        let mut capacity = 2usize;
        while capacity < n.max(1) {
            capacity = capacity.saturating_mul(2);
            bound += 1;
        }
        bound + 1
    }
}

// ===========================================================================
// Direct ExtentTree edits — the mid-list insert/remove differentiator. A
// Fenwick/BIT pays O(n) for these; this B+-tree pays O(log n) and must stay
// rebalanced and drift-free. The Virtualizer surface above never inserts/removes
// in the interior, so these go straight at the tree against a Vec oracle.
// ===========================================================================
mod tree_edits {
    use super::super::sumtree::ExtentTree;
    use super::*;
    use proptest::prelude::*;

    fn measured(e: f64) -> ItemExtent {
        ItemExtent::Measured { extent: e }
    }

    #[derive(Clone, Default)]
    struct Vecf(Vec<f64>);
    impl Vecf {
        fn insert(&mut self, i: usize, e: f64) {
            self.0.insert(i, e);
        }
        fn remove(&mut self, i: usize) -> f64 {
            self.0.remove(i)
        }
        fn set(&mut self, i: usize, e: f64) {
            self.0[i] = e;
        }
        fn total(&self) -> f64 {
            self.0.iter().sum()
        }
        fn offset_of(&self, i: usize) -> f64 {
            self.0.iter().take(i).sum()
        }
        fn seek(&self, off: f64) -> (usize, f64) {
            let n = self.0.len();
            if n == 0 {
                return (0, 0.0);
            }
            let total = self.total();
            if off <= 0.0 {
                return (0, 0.0);
            }
            if off >= total {
                return (n - 1, off - self.offset_of(n - 1));
            }
            let mut acc = 0.0;
            for (i, &e) in self.0.iter().enumerate() {
                if acc + e > off {
                    return (i, off - acc);
                }
                acc += e;
            }
            (n - 1, off - self.offset_of(n - 1))
        }
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert { at: usize, e: f64 },
        Remove { at: usize },
        Set { at: usize, e: f64 },
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..300, extent_in(0.0, 50.0)).prop_map(|(at, e)| Op::Insert { at, e }),
            (0usize..300).prop_map(|at| Op::Remove { at }),
            (0usize..300, extent_in(0.0, 50.0)).prop_map(|(at, e)| Op::Set { at, e }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn mid_list_insert_remove_matches_oracle(
            init in proptest::collection::vec(extent_in(0.0, 50.0), 0..30),
            ops in proptest::collection::vec(op(), 0..400),
        ) {
            let mut t = ExtentTree::from_fn(init.len(), |i| measured(init[i]));
            let mut o = Vecf(init.clone());

            for op in &ops {
                match *op {
                    Op::Insert { at, e } => {
                        let at = at % (o.0.len() + 1);
                        t.insert(at, measured(e));
                        o.insert(at, e);
                    }
                    Op::Remove { at } => {
                        if o.0.is_empty() { continue; }
                        let at = at % o.0.len();
                        let r = t.remove(at);
                        let ro = o.remove(at);
                        prop_assert!((r.extent() - ro).abs() < 1e-6,
                            "removed wrong item: tree {} oracle {}", r.extent(), ro);
                    }
                    Op::Set { at, e } => {
                        if o.0.is_empty() { continue; }
                        let at = at % o.0.len();
                        t.set(at, measured(e));
                        o.set(at, e);
                    }
                }

                t.check_invariants()
                    .map_err(|m| TestCaseError::fail(format!("invariant after {op:?}: {m}")))?;
                prop_assert_eq!(t.len(), o.0.len(), "count mismatch after {:?}", op);
            }

            for i in 0..=o.0.len() {
                let got = t.offset_of(i);
                let exp = o.offset_of(i);
                prop_assert!((got - exp).abs() <= 1e-2 + 1e-4 * exp.abs(),
                    "offset_of({}) tree {} oracle {}", i, got, exp);
            }

            let total = o.total();
            if !o.0.is_empty() && total > 0.0 {
                for k in 0..=40u32 {
                    let off = total * (k as f64) / 40.0;
                    let (ti, _tinto) = t.seek_offset(off);
                    let (oi, _ointo) = o.seek(off);
                    let agree = ti == oi
                        || (ti.abs_diff(oi) == 1
                            && (t.offset_of(ti.max(oi)) - off).abs() <= 1e-2 + 1e-4 * total);
                    prop_assert!(agree, "seek({}) tree {} oracle {}", off, ti, oi);
                }
            }
        }

    }
}

// ============================================================================
// Adaptive estimate — re-hinting unmeasured items keeps the anchor stationary
// ============================================================================
mod adaptive_estimate {}

mod band_local_mean {}
