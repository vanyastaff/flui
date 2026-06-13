//! Tests for the protocol-agnostic [`Virtualizer`].
//!
//! Two layers:
//! - **Unit tests** pin specific behaviours (dual-range query, anchor
//!   correction, Estimated→Exact, boundary/edge cases, `O(log n)` seek scaling).
//! - **Property tests** ([`prop`]) run random op sequences against a naive
//!   `Vec<ItemExtent>` oracle, asserting the windowing invariants hold and the
//!   backing tree stays balanced.

use super::*;

const EPS: f32 = 1e-3;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS
}

// ============================================================================
// Construction, len, total_extent, Estimated -> Exact
// ============================================================================

#[test]
fn empty_virtualizer() {
    let v = Virtualizer::new(0, 10.0);
    assert!(v.is_empty());
    assert_eq!(v.len(), 0);
    assert_eq!(v.total_extent(), Extent::Exact(0.0));
    assert_eq!(v.measured_count(), 0);
    assert_eq!(v.estimated_count(), 0);
    assert_eq!(v.anchor_item(), (0, 0.0));
    assert!(!v.is_measured(0));
}

#[test]
fn new_seeds_estimates() {
    let v = Virtualizer::new(5, 10.0);
    assert_eq!(v.len(), 5);
    assert!(!v.is_empty());
    // All estimated -> Estimated total of 5 * 10.
    assert_eq!(v.total_extent(), Extent::Estimated(50.0));
    assert_eq!(v.measured_count(), 0);
    assert_eq!(v.estimated_count(), 5);
    for i in 0..5 {
        assert!(!v.is_measured(i));
        assert!(approx(v.offset_of(i), i as f32 * 10.0));
    }
    assert!(approx(v.offset_of(5), 50.0));
}

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
        other => panic!("expected Estimated while a prefix is unmeasured, got {other:?}"),
    }

    // Measure the last — now Exact.
    assert_eq!(v.set_measured(2, 20.0, (0, 0.0)), None);
    assert_eq!(v.measured_count(), 3);
    assert_eq!(v.estimated_count(), 0);
    assert_eq!(v.total_extent(), Extent::Exact(12.0 + 8.0 + 20.0));
}

#[test]
fn remeasuring_does_not_double_count() {
    let mut v = Virtualizer::new(2, 10.0);
    v.set_measured(0, 15.0, (0, 0.0));
    v.set_measured(0, 25.0, (0, 0.0)); // re-measure same item
    assert_eq!(v.measured_count(), 1);
    // total = 25 (item 0) + 10 (item 1 still estimated)
    assert_eq!(v.total_extent(), Extent::Estimated(35.0));
}

// ============================================================================
// query: dual band + leading_offset
// ============================================================================

#[test]
fn query_empty_is_empty_range() {
    let v = Virtualizer::new(0, 10.0);
    let r = v.query(&ScrollWindow::new(0.0, 100.0));
    assert_eq!(r, VisibleRange::EMPTY);
}

#[test]
fn query_visible_band_uniform() {
    // 10 items x 20px = 200px total. Viewport [25, 75) -> items 1,2,3.
    let v = Virtualizer::new(10, 20.0);
    let r = v.query(&ScrollWindow::new(25.0, 50.0));
    assert_eq!(r.first, 1, "item 1 starts at 20, contains offset 25");
    assert_eq!(r.last, 4, "viewport ends at 75, inside item 3 (60..80)");
    // leading_offset = item1.start - offset = 20 - 25 = -5
    assert!(approx(r.leading_offset, -5.0), "got {}", r.leading_offset);
}

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

#[test]
fn query_clamps_at_ends() {
    let v = Virtualizer::new(5, 10.0); // total 50
    // Window past the end.
    let r = v.query(&ScrollWindow::new(45.0, 100.0));
    assert_eq!(r.first, 4);
    assert_eq!(r.last, 5);
    // Window at offset 0.
    let r0 = v.query(&ScrollWindow::new(0.0, 10.0));
    assert_eq!(r0.first, 0);
    assert_eq!(r0.last, 1);
    assert!(approx(r0.leading_offset, 0.0));
}

#[test]
fn query_zero_extent_viewport_is_empty_visible_band() {
    let v = Virtualizer::new(5, 10.0);
    let r = v.query(&ScrollWindow::new(20.0, 0.0));
    // Zero-height viewport: visible band empty, but first is well-defined.
    assert_eq!(r.first, 2);
    assert_eq!(r.last, 2, "zero-extent viewport intersects no item");
}

// ============================================================================
// Anchor correction — the jitter killer
// ============================================================================

#[test]
fn anchor_correction_above_anchor_returns_signed_delta() {
    let mut v = Virtualizer::new(10, 10.0);
    // Anchor on item 5. Re-measure item 2 (above anchor) from estimate 10 -> 18.
    let corr = v.set_measured(2, 18.0, (5, 0.0));
    assert_eq!(
        corr,
        Some(AnchorCorrection { delta: 8.0 }),
        "delta must equal new - old extent of the above-anchor item"
    );
}

#[test]
fn anchor_correction_below_anchor_is_none() {
    let mut v = Virtualizer::new(10, 10.0);
    // Anchor on item 5. Re-measure item 7 (below anchor): no correction.
    assert_eq!(v.set_measured(7, 30.0, (5, 0.0)), None);
}

#[test]
fn anchor_correction_at_anchor_is_none() {
    let mut v = Virtualizer::new(10, 10.0);
    // Re-measure the anchor item itself: content above it is unchanged.
    assert_eq!(v.set_measured(5, 30.0, (5, 0.0)), None);
}

#[test]
fn anchor_correction_equal_extent_is_none() {
    let mut v = Virtualizer::new(10, 10.0);
    // Above anchor but extent equals the estimate: nothing shifts.
    assert_eq!(v.set_measured(2, 10.0, (5, 0.0)), None);
}

#[test]
fn anchor_correction_negative_delta_when_smaller_than_estimate() {
    let mut v = Virtualizer::new(10, 10.0);
    let corr = v.set_measured(1, 4.0, (5, 0.0));
    assert_eq!(corr, Some(AnchorCorrection { delta: -6.0 }));
}

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

#[test]
fn set_measured_out_of_range_anchor_is_ignored_but_still_measures() {
    let mut v = Virtualizer::new(10, 10.0);
    // Establish a valid anchor at item 5.
    v.set_measured(5, 10.0, (5, 0.0));
    assert_eq!(v.anchor_item(), (5, 0.0));
    // Measure item 2 (above the anchor) but pass an out-of-range anchor index:
    // the anchor is refused (kept at 5, NOT silently clamped to 9), and no
    // correction is emitted because a fabricated anchor can't be reasoned about.
    let corr = v.set_measured(2, 18.0, (99, 0.0));
    assert_eq!(corr, None, "out-of-range anchor yields no correction");
    assert_eq!(
        v.anchor_item(),
        (5, 0.0),
        "out-of-range anchor is ignored, not clamped"
    );
    // The measurement itself still landed regardless of the bad anchor.
    assert!(v.is_measured(2));
    // items 0,1,3,4,6,7,8,9 estimated (8*10=80) + item 2 (18) + item 5 (10).
    assert_eq!(v.total_extent(), Extent::Estimated(108.0));
}

// ============================================================================
// set_count — O(log n) structural edits
// ============================================================================

#[test]
fn set_count_grow_appends_estimates() {
    let mut v = Virtualizer::new(3, 10.0);
    v.set_measured(0, 5.0, (0, 0.0));
    v.set_count(6);
    assert_eq!(v.len(), 6);
    assert_eq!(
        v.measured_count(),
        1,
        "growth must not touch measured count"
    );
    // Appended items are estimated.
    for i in 3..6 {
        assert!(!v.is_measured(i));
    }
    // total = 5 + 2*10 (items 1,2) + 3*10 (new) = 55
    assert_eq!(v.total_extent(), Extent::Estimated(55.0));
}

#[test]
fn set_count_shrink_drops_tail() {
    let mut v = Virtualizer::new(6, 10.0);
    for i in 0..6 {
        v.set_measured(i, 10.0, (0, 0.0));
    }
    assert_eq!(v.measured_count(), 6);
    v.set_count(4);
    assert_eq!(v.len(), 4);
    assert_eq!(
        v.measured_count(),
        4,
        "dropped measured tail decrements count"
    );
    assert_eq!(v.total_extent(), Extent::Exact(40.0));
}

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

#[test]
fn invalidate_from_resets_to_estimates() {
    let mut v = Virtualizer::new(5, 10.0);
    for i in 0..5 {
        v.set_measured(i, 20.0, (0, 0.0));
    }
    assert_eq!(v.total_extent(), Extent::Exact(100.0));
    v.invalidate_from(2);
    assert_eq!(v.measured_count(), 2, "items 2,3,4 reset to estimates");
    assert_eq!(v.estimated_count(), 3);
    // total = 20+20 (measured 0,1) + 10*3 (re-estimated) = 70
    assert_eq!(v.total_extent(), Extent::Estimated(70.0));
}

// ============================================================================
// scroll_to_item
// ============================================================================

#[test]
fn scroll_to_item_zero_viewport_is_leading_flush() {
    let mut v = Virtualizer::new(10, 10.0);
    // viewport_extent 0: alignment 0 is leading-flush (item start at the
    // origin); alignment 1 puts the item's trailing edge at the origin
    // (item_start + item_extent = 40 + 10 = 50).
    assert!(approx(v.scroll_to_item(4, 0.0, 0.0), 40.0));
    assert!(approx(v.scroll_to_item(4, 1.0, 0.0), 50.0));
    assert_eq!(v.anchor_item(), (4, 0.0));
}

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

#[test]
fn scroll_to_item_clamps_to_scroll_range() {
    let mut v = Virtualizer::new(10, 10.0); // total 100, viewport 40 -> max_scroll 60
    // Trailing-align item 0: 0 - (40-10) = -30 -> clamps to 0.
    assert!(approx(v.scroll_to_item(0, 1.0, 40.0), 0.0));
    // Leading-align last item: starts at 90 -> clamps to 60.
    assert!(approx(v.scroll_to_item(9, 0.0, 40.0), 60.0));
}

#[test]
fn scroll_to_item_uses_caller_viewport_no_hidden_state() {
    // The viewport is a call argument, not recorded state, so the stale-viewport
    // ordering hazard cannot arise: two calls with different viewports disagree,
    // and an interleaved (pure, `&self`) query cannot influence a later call.
    let mut v = Virtualizer::new(10, 10.0); // total 100
    // center-align item 5 (start 50, ext 10).
    // viewport 40: 50 - 0.5*(40-10) = 35.
    // viewport 20: 50 - 0.5*(20-10) = 45.
    assert!(approx(v.scroll_to_item(5, 0.5, 40.0), 35.0));
    assert!(approx(v.scroll_to_item(5, 0.5, 20.0), 45.0));
    let _ = v.query(&ScrollWindow::new(0.0, 999.0));
    assert!(approx(v.scroll_to_item(5, 0.5, 20.0), 45.0));
}

// ============================================================================
// O(log n) seek both directions on 10k items
// ============================================================================

#[test]
fn seek_both_directions_on_10k_items() {
    let mut v = Virtualizer::new(10_000, 7.0);
    // Make extents non-uniform so the tree can't shortcut via uniformity.
    for i in 0..10_000 {
        v.set_measured(i, (i % 13 + 1) as f32, (0, 0.0));
    }
    let total = v.total_extent().value();

    // index -> offset is monotonic and consistent with offset -> index.
    let mut prev = 0.0;
    for i in (0..10_000).step_by(257) {
        let off = v.offset_of(i);
        assert!(off >= prev, "offsets must be non-decreasing");
        prev = off;
        // Seeking the item's own start returns the item (sub-offset 0).
        let r = v.query(&ScrollWindow::new(off, 1.0));
        assert_eq!(r.first, i, "offset {off} of item {i} must seek back to {i}");
    }

    // A mid-content offset lands on the item whose span contains it.
    let mid = total / 2.0;
    let r = v.query(&ScrollWindow::new(mid, 1.0));
    let start = v.offset_of(r.first);
    let end = v.offset_of(r.first + 1);
    assert!(
        start <= mid && mid < end,
        "offset {mid} must fall within item {}'s span [{start}, {end})",
        r.first
    );
}

// ============================================================================
// Boundary edge cases: offset==0 and offset==total
// ============================================================================

#[test]
fn boundary_offset_zero_and_total() {
    let v = Virtualizer::new(4, 10.0); // total 40
    // offset 0 -> item 0
    let r0 = v.query(&ScrollWindow::new(0.0, 5.0));
    assert_eq!(r0.first, 0);
    assert!(approx(r0.leading_offset, 0.0));
    // offset == total -> clamps to last item
    let rt = v.query(&ScrollWindow::new(40.0, 5.0));
    assert_eq!(rt.first, 3, "offset==total clamps to the last item");
}

#[test]
fn single_item_virtualizer() {
    let v = Virtualizer::new(1, 42.0);
    assert_eq!(v.len(), 1);
    assert!(approx(v.offset_of(0), 0.0));
    assert!(approx(v.offset_of(1), 42.0));
    let r = v.query(&ScrollWindow::new(0.0, 100.0));
    assert_eq!((r.first, r.last), (0, 1));
}

// ============================================================================
// Property tests vs a naive Vec<ItemExtent> oracle
// ============================================================================

mod prop {
    use super::*;
    use proptest::prelude::*;

    /// Compares two extent sums that were accumulated in different orders (the
    /// tree folds leaf→summary→total, the oracle folds left→right), tolerating
    /// f32 round-off proportional to the magnitude. The invariant being checked
    /// is "same sum up to float accumulation", not bit-exact equality — exact
    /// equality across two summation orders is not a real property of f32.
    fn approx_sum(a: f32, b: f32) -> bool {
        let tol = 1e-3 + 1e-4 * a.abs().max(b.abs());
        (a - b).abs() <= tol
    }

    /// Naive reference model: a flat `Vec` of extents. Every invariant the
    /// `Virtualizer` claims is checked against this `O(n)` oracle.
    #[derive(Debug, Clone, Default)]
    struct Oracle {
        items: Vec<ItemExtent>,
        default_estimate: f32,
    }

    impl Oracle {
        fn new(count: usize, default_estimate: f32) -> Self {
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

        fn set_measured(&mut self, index: usize, extent: f32) {
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

        fn total(&self) -> f32 {
            self.items.iter().map(ItemExtent::extent).sum()
        }

        fn offset_of(&self, index: usize) -> f32 {
            self.items.iter().take(index).map(ItemExtent::extent).sum()
        }

        fn measured_count(&self) -> usize {
            self.items.iter().filter(|i| i.is_measured()).count()
        }

        /// First item whose span `[start, start+extent)` contains `offset`
        /// (clamped to `[0, total]`), matching the tree's seek contract.
        fn seek(&self, offset: f32) -> usize {
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
        SetMeasured { index: usize, extent: f32 },
        InvalidateFrom(usize),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..200).prop_map(Op::SetCount),
            (0usize..200, 0.1f32..100.0)
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
            default_estimate in 0.5f32..50.0,
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
                    let off = total * (k as f32) / 20.0;
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

        /// Mid-list insert/delete via `set_count` growth/shrink keeps order and
        /// sums consistent with the oracle through many resizes.
        #[test]
        fn set_count_resizes_preserve_sums(
            sizes in proptest::collection::vec(0usize..150, 1..40),
            est in 1.0f32..20.0,
        ) {
            let mut v = Virtualizer::new(0, est);
            let mut oracle = Oracle::new(0, est);
            for &s in &sizes {
                v.set_count(s);
                oracle.set_count(s);
                prop_assert_eq!(v.len(), oracle.items.len());
                v.tree
                    .check_invariants()
                    .map_err(|e| TestCaseError::fail(format!("tree invariant: {e}")))?;
                prop_assert!(approx_sum(v.total_extent().value(), oracle.total()));
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

    fn measured(e: f32) -> ItemExtent {
        ItemExtent::Measured { extent: e }
    }

    #[derive(Clone, Default)]
    struct Vecf(Vec<f32>);
    impl Vecf {
        fn insert(&mut self, i: usize, e: f32) {
            self.0.insert(i, e);
        }
        fn remove(&mut self, i: usize) -> f32 {
            self.0.remove(i)
        }
        fn set(&mut self, i: usize, e: f32) {
            self.0[i] = e;
        }
        fn total(&self) -> f32 {
            self.0.iter().sum()
        }
        fn offset_of(&self, i: usize) -> f32 {
            self.0.iter().take(i).sum()
        }
        fn seek(&self, off: f32) -> (usize, f32) {
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
        Insert { at: usize, e: f32 },
        Remove { at: usize },
        Set { at: usize, e: f32 },
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..300, 0.0f32..50.0).prop_map(|(at, e)| Op::Insert { at, e }),
            (0usize..300).prop_map(|at| Op::Remove { at }),
            (0usize..300, 0.0f32..50.0).prop_map(|(at, e)| Op::Set { at, e }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn mid_list_insert_remove_matches_oracle(
            init in proptest::collection::vec(0.0f32..50.0, 0..30),
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
                    let off = total * (k as f32) / 40.0;
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

    /// 200k random point-updates must not let the tree's summarized total drift
    /// from a fresh sum: summaries recompute from children (not an incremental
    /// `+= delta`), so error is bounded by f32 round-off, not by accumulation —
    /// the result is history-independent. A growing drift here would mean
    /// someone reintroduced incremental-delta summary maintenance.
    #[test]
    fn drift_long_edit_session() {
        let n = 4000usize;
        let mut t = ExtentTree::from_fn(n, |_| measured(0.1));
        let mut o = Vecf(vec![0.1; n]);
        let mut x = 0u64;
        for _ in 0..200_000usize {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let i = (x >> 33) as usize % t.len();
            let e = ((x & 0xffff) as f32) / 6553.6;
            t.set(i, measured(e));
            o.set(i, e);
        }
        let drift = (t.total_extent() - o.total()).abs();
        assert!(drift < 5.0, "drift {drift} too large — incremental delta?");
    }

    #[test]
    fn seek_exact_boundaries_roundtrip() {
        let t = ExtentTree::from_fn(5, |i| measured((i as f32 + 1.0) * 10.0));
        assert_eq!(t.seek_offset(0.0), (0, 0.0));
        assert_eq!(t.seek_offset(10.0), (1, 0.0));
        assert_eq!(t.seek_offset(30.0), (2, 0.0));
        assert_eq!(t.seek_offset(60.0), (3, 0.0));
        assert_eq!(t.seek_offset(100.0), (4, 0.0));
        assert_eq!(t.seek_offset(150.0), (4, 50.0));
        assert_eq!(t.seek_offset(200.0), (4, 100.0));
        for i in 0..5 {
            let start = t.offset_of(i);
            let (idx, into) = t.seek_offset(start);
            assert_eq!((idx, into), (i, 0.0), "boundary {start} for item {i}");
        }
    }

    #[test]
    fn zero_extent_middle_after_edits() {
        let mut t = ExtentTree::from_fn(4, |_| measured(10.0));
        t.insert(2, measured(0.0));
        assert_eq!(t.total_extent(), 40.0);
        assert_eq!(t.seek_offset(20.0), (3, 0.0));
    }
}
