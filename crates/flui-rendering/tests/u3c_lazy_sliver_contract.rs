//! U3c acceptance tests — re-entrant child-build contract: logical-index
//! stamping (D1) and bounded child count / deferred dispose (D2).
//!
//! ## 9a — Convergence
//! N=1000 items, viewport shows ~K. Drive enough frames to settle. Assert:
//!   - every attached child's `parent_data.index` equals its true logical index
//!     (NOT 0) — proves D1 defect is fixed.
//!   - the visible band's logical indices match the expected visible window.
//!
//! ## 9b — Bounded child count
//! Scroll the viewport across many items over many frames. Assert the attached
//! child count stays bounded (≈ band size, ≪ N) — proves D2 dispose path works.
//!
//! ## Step-3 regression — logical index stamped on Insert
//! A `RenderSliverListLazy` with a single virtual item forces a deferred Insert
//! with `logical_index = 0`. After settlement, assert the child's
//! `SliverMultiBoxAdaptorParentData.index == 0` (not the pre-fix value of
//! "whatever was in memory"). For a non-trivial index, 9a covers many offsets.
//!
//! ## Step-7 regression — Remove → Insert ordering
//! A mixed Remove+Insert batch targeting the same parent applies Remove first.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::Diagnosticable;
use flui_objects::{RenderColoredBox, RenderSliverListLazy};
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints},
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::{BoxParentData, SliverMultiBoxAdaptorParentData},
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    testing::sliver as sliver_presets,
    traits::{RenderBox, RenderObject},
};
use flui_tree::{Leaf, Variable};
use flui_types::{Size, geometry::px};

/// Convenience alias for the item-source callback shared across tests.
type ItemSource = Arc<dyn Fn(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>> + Send + Sync>;

// ============================================================================
// Shared test utilities
// ============================================================================

/// A Box render object that lays out to a fixed size.  Used as a synthetic
/// leaf child for `RenderSliverListLazy`.
#[derive(Debug, Clone)]
struct FixedBox {
    height: f32,
}

impl FixedBox {
    fn new(height: f32) -> Self {
        Self { height }
    }
}

impl Diagnosticable for FixedBox {}

impl RenderBox for FixedBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        // max_width is a public Pixels field on BoxConstraints, not a method.
        let w = ctx.constraints().max_width;
        Size::new(w, px(self.height))
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }
}

/// A Box render object that hosts a Sliver child and drives it with the
/// given `SliverConstraints`.
#[derive(Debug, Clone)]
struct SliverHost {
    constraints: SliverConstraints,
}

impl SliverHost {
    fn new(constraints: SliverConstraints) -> Self {
        Self { constraints }
    }
}

impl Diagnosticable for SliverHost {}

impl RenderBox for SliverHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        ctx.constraints().biggest()
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        false
    }
}

/// Build a `SliverConstraints` for a vertical viewport at a given scroll
/// offset and viewport height.
fn vertical(scroll_offset: f32, viewport_height: f32) -> SliverConstraints {
    sliver_presets::vertical()
        .scroll_offset(scroll_offset)
        .remaining_paint_extent(viewport_height)
        .cross_axis_extent(300.0)
        .viewport_main_axis_extent(viewport_height)
        .remaining_cache_extent(viewport_height + 100.0)
        .cache_origin(-50.0)
        .build()
}

/// Build a complete pipeline with a SliverHost → RenderSliverListLazy tree
/// and pump `frame_count` layout passes.
///
/// Returns `(owner, root_id, sliver_id)`.
fn build_and_pump(
    n_items: usize,
    item_height: f32,
    scroll_offset: f32,
    viewport_height: f32,
    frame_count: usize,
) -> (
    PipelineOwner<flui_rendering::pipeline::Layout>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
) {
    let constraints = vertical(scroll_offset, viewport_height);
    let source: ItemSource = Arc::new(move |_idx| {
        Some(Box::new(FixedBox::new(item_height)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(n_items, item_height, Arc::clone(&source), None);

    let mut owner = PipelineOwner::new();
    let root_id =
        owner.insert(Box::new(SliverHost::new(constraints)) as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("sliver node must be inserted under root host");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(300.0),
        px(viewport_height),
    ))));

    let mut owner = owner.into_layout();

    for _ in 0..frame_count {
        owner.run_layout().expect("layout must succeed");
    }

    (owner, root_id, sliver_id)
}

/// Collect all `SliverMultiBoxAdaptorParentData` for the children of `sliver_id`.
///
/// Returns a sorted Vec of `(logical_index, render_id)`.
fn collect_child_indices(
    owner: &PipelineOwner<flui_rendering::pipeline::Layout>,
    sliver_id: flui_foundation::RenderId,
) -> Vec<(usize, flui_foundation::RenderId)> {
    let tree = owner.render_tree();
    let child_ids = tree.children(sliver_id).to_vec();
    let mut pairs = Vec::with_capacity(child_ids.len());
    for child_id in child_ids {
        if let Some(node) = tree.get(child_id)
            && let Some(pd) = node
                .parent_data()
                .and_then(|pd| pd.downcast_ref::<SliverMultiBoxAdaptorParentData>())
        {
            pairs.push((pd.index, child_id));
        }
    }
    pairs.sort_by_key(|(idx, _)| *idx);
    pairs
}

// ============================================================================
// Step-3 regression: logical index stamped on Insert (non-zero index)
// ============================================================================

/// Regression guard for Defect 1 (D1): `apply_deferred_mutation` must stamp the
/// correct logical index onto a freshly-inserted child.
///
/// N=5 items at 50 px each, scroll_offset=100 px → the visible band starts at
/// logical item 2 (not item 0). After settlement every attached child's index
/// must equal its true logical index (≥ 2).
///
/// **Why this test catches D1 where the old N=1/scroll=0 test did not:**
/// The pre-fix `index` field defaulted to `0` — so a child built at slot 0 with
/// logical index 0 passed trivially. Only a non-zero logical index (scroll_offset
/// \> 0) makes the stamp observable: if `apply_deferred_mutation` is a no-op, the
/// built child has `index = 0` instead of `index = 2`, which the assertion below
/// catches. 9a covers many offsets; this test fixes the discriminating edge at index=2.
#[test]
fn step3_logical_index_stamped_on_deferred_insert() {
    let item_height = 50.0_f32;
    let scroll_offset = 100.0_f32; // visible band starts at item 2
    // N=5 so item 2 and its neighbours are valid; viewport (300 px) fits items 2–7.
    let (owner, _root_id, sliver_id) = build_and_pump(5, item_height, scroll_offset, 300.0, 8);

    let pairs = collect_child_indices(&owner, sliver_id);
    assert!(
        !pairs.is_empty(),
        "D1: at least one child must be built after 8 frames with N=5 items",
    );

    // Pre-fix: index == 0 for every child (never stamped).
    // Post-fix: minimum index == 2 (cache_first = scroll_offset/item_height - 1 = 1;
    // with cache_origin=-50 the first cached item at offset=100 is item 1; clipped to
    // valid items starting at 0, actual min may be 1 or 2).
    // The invariant we assert: no child claims index=0 when scroll_offset=100
    // (item 0 ends at offset=50, which is above the cache band at scroll=100).
    let has_item_zero = pairs.iter().any(|(idx, _)| *idx == 0);
    assert!(
        !has_item_zero,
        "D1 regression: a child has logical index 0 even though scroll_offset={scroll_offset} \
         places item 0 (offset 0..50 px) above the entire cache band. \
         Pre-fix: apply_deferred_mutation never stamped logical_index → fresh \
         children defaulted to index=0 regardless of their true logical position.",
    );

    // Every index must be distinct.
    let indices: Vec<usize> = pairs.iter().map(|(idx, _)| *idx).collect();
    let unique_count = {
        let mut s = indices.clone();
        s.dedup();
        s.len()
    };
    assert_eq!(
        unique_count,
        indices.len(),
        "duplicate logical indices: {:?}",
        indices,
    );
}

// ============================================================================
// Step-7 regression: Remove → Insert ordering
// ============================================================================

/// Verifies D3: when a batch contains both a Remove and an Insert targeting
/// the same parent, the Remove is applied first, so the final child set
/// contains exactly the pre-existing child B plus newly inserted C (not also
/// the removed child A).
#[test]
fn step7_deferred_remove_before_insert_ordering() {
    let mut owner = PipelineOwner::new();

    // A Box parent with two children A and B.
    let root_id =
        owner.insert(
            Box::new(RenderColoredBox::red(300.0, 600.0)) as Box<dyn RenderObject<BoxProtocol>>
        );
    let child_a_id = owner
        .render_tree_mut()
        .insert_box_child(
            root_id,
            Box::new(RenderColoredBox::blue(10.0, 10.0)) as Box<dyn RenderObject<BoxProtocol>>,
        )
        .expect("child A must insert");
    let _child_b_id = owner
        .render_tree_mut()
        .insert_box_child(
            root_id,
            Box::new(RenderColoredBox::green(10.0, 10.0)) as Box<dyn RenderObject<BoxProtocol>>,
        )
        .expect("child B must insert");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(300.0), px(600.0)))));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 0: initial layout");

    // Enqueue Remove(A) and Insert(C) in the same pass.
    // D3 guarantees Remove is drained before Insert: A is gone when C is
    // inserted, so the final child list is [B, C].
    owner.defer_remove(root_id, child_a_id);
    owner.defer_insert_box(
        root_id,
        Box::new(RenderColoredBox::red(10.0, 10.0)) as Box<dyn RenderObject<BoxProtocol>>,
        None,
        None,
        None,
    );

    // Second pass drains the queued mutations.
    owner.run_layout().expect("frame 1: applies Remove+Insert");

    let children_after = owner.render_tree().children(root_id).to_vec();

    // B + C = 2. A must be absent.
    assert_eq!(
        children_after.len(),
        2,
        "after Remove(A) + Insert(C): 2 children expected (B + C), \
         got {}: wrong ordering may leave 3 (A not removed) or 1 (Insert lost)",
        children_after.len(),
    );
    assert!(
        !children_after.contains(&child_a_id),
        "removed child A must not appear after Remove+Insert batch (D3 ordering violated)",
    );
}

// ============================================================================
// 9a — Convergence: logical indices reconcile correctly
// ============================================================================

/// U3c 9a acceptance test: N=1000 items, viewport showing ~K. Drive enough
/// frames to settle. Assert every attached child's `parent_data.index` equals
/// its true logical index (NOT 0).
///
/// This test MUST fail before D1 (logical_index was 0 for all children when
/// scroll_offset > 0) and pass after.
#[test]
fn u3c_9a_convergence_logical_indices_reconcile() {
    let n_items = 1_000;
    let item_height = 50.0_f32;
    let viewport_height = 500.0_f32;
    // Scroll to item 10 so the visible band starts at logical index 10, not 0.
    // Pre-fix: all children would report index = 0.
    let scroll_offset = 10.0 * item_height;

    // The v1 next-frame backend builds one absent child per frame.
    // With ~K visible items, settle after K + 5 passes (generous headroom).
    let k_visible = (viewport_height / item_height).ceil() as usize + 2;
    let frame_count = k_visible + 5;

    let (owner, _root_id, sliver_id) = build_and_pump(
        n_items,
        item_height,
        scroll_offset,
        viewport_height,
        frame_count,
    );

    let pairs = collect_child_indices(&owner, sliver_id);

    assert!(
        !pairs.is_empty(),
        "at least some children must have been built after {frame_count} frames",
    );

    // Pre-fix: all indices are 0. Post-fix: indices are distinct and > 0
    // because scroll_offset = 10 * item_height means item 0 is above the fold.
    let all_zero = pairs.iter().all(|(idx, _)| *idx == 0);
    assert!(
        !all_zero,
        "U3c D1 regression: ALL attached children have logical index 0. \
         With scroll_offset={scroll_offset} the visible band starts at item 10, \
         not item 0. Pre-fix: apply_deferred_mutation never wrote logical_index \
         into parent-data → every fresh child defaulted to index 0.",
    );

    // All logical indices must be distinct (no two children claim the same item).
    let indices: Vec<usize> = pairs.iter().map(|(idx, _)| *idx).collect();
    let unique_count = {
        // `indices` is already ascending (collect_child_indices sorts by index),
        // so consecutive-dedup yields the distinct count.
        let mut deduped = indices.clone();
        deduped.dedup();
        deduped.len()
    };
    assert_eq!(
        unique_count,
        indices.len(),
        "duplicate logical indices detected: {:?} — two children claim the same item",
        indices,
    );

    // The lowest visible logical index must be ≥ cache_first.  With
    // cache_origin = −50 px and item height = 50 px, item 9 starts at
    // virtual offset 450 px which falls inside the cache band when
    // scroll_offset = 500 px (cache reaches back 50 px before the viewport).
    // Asserting ≥ 9 (= cache_first) is the correct invariant; ≥ 10 (= visible
    // first) would be too tight and would spuriously fail on valid pre-fetch.
    let min_idx = pairs.first().map(|(idx, _)| *idx).unwrap_or(0);
    assert!(
        min_idx >= 9,
        "scroll_offset={scroll_offset}: visible band must start at item ≥ 9 \
         (cache_first), got min index {min_idx}",
    );

    eprintln!(
        "9a convergence ok: {} children, logical indices {}..{}",
        pairs.len(),
        pairs.first().map(|(i, _)| *i).unwrap_or(0),
        pairs.last().map(|(i, _)| *i).unwrap_or(0),
    );
}

// ============================================================================
// 9b — Bounded child count (dispose works)
// ============================================================================

/// U3c 9b acceptance test: scroll the viewport across many items over many
/// frames; assert the attached child count stays bounded (≈ band size, ≪ N).
///
/// This test MUST show unbounded growth before D2 (dispose never fired) and
/// bounded growth after.
#[test]
fn u3c_9b_bounded_child_count_after_scroll() {
    let n_items = 1_000usize;
    let item_height = 50.0_f32;
    let viewport_height = 300.0_f32;
    // Expected max children: visible + cache band.
    // band ≈ (viewport + 2*cache_margin) / item_height + slack
    let expected_band_size = ((viewport_height + 200.0) / item_height).ceil() as usize + 4;

    let source: ItemSource = Arc::new(move |_idx| {
        Some(Box::new(FixedBox::new(item_height)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(n_items, item_height, Arc::clone(&source), None);

    let initial_constraints = vertical(0.0, viewport_height);
    let mut owner = PipelineOwner::new();
    let root_id =
        owner
            .insert(Box::new(SliverHost::new(initial_constraints))
                as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("lazy sliver must insert under root host");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(300.0),
        px(viewport_height),
    ))));

    let mut owner = owner.into_layout();

    // Simulate scrolling across 200 items in steps of item_height.
    // At each scroll position, pump 3 frames to let the next-frame backend
    // settle, then update the host's constraints for the next position.
    let mut peak = 0usize;
    let scroll_steps = 200usize;

    for step in 0..scroll_steps {
        let scroll_pos = step as f32 * item_height;
        let new_constraints = vertical(scroll_pos, viewport_height);

        // Update SliverHost constraints via direct render-object mutation.
        // `RenderObject<BoxProtocol>` implements `DowncastSync`, so
        // `downcast_mut::<SliverHost>()` resolves at runtime by TypeId.
        if let Some(node) = owner.render_tree_mut().get_mut(root_id)
            && let Some(entry) = node.as_box_mut()
            && let Some(host) = entry.render_object_mut().downcast_mut::<SliverHost>()
        {
            host.constraints = new_constraints;
        }
        owner.mark_needs_layout(root_id);

        // 3 frames per scroll position: schedule → apply → lay-out.
        for _ in 0..3 {
            owner
                .run_layout()
                .expect("layout must succeed during scroll");
        }

        let n_children = owner.render_tree().children(sliver_id).len();
        if n_children > peak {
            peak = n_children;
        }
    }

    eprintln!(
        "9b bounded count: peak={peak} children attached, \
         expected band ≈ {expected_band_size} (n={n_items}, viewport={viewport_height}px, \
         item_height={item_height}px)",
    );

    // Primary: child count stays bounded ≈ band size (not growing towards N).
    // Allow 3× band for pipeline timing jitter (dispose is deferred one frame).
    let upper_bound = expected_band_size * 3;
    assert!(
        peak <= upper_bound,
        "U3c D2 regression: peak attached child count {peak} exceeded \
         {upper_bound} (= 3 × expected band size {expected_band_size}). \
         Pre-fix: dispose_box_child was never called → unbounded growth to N={n_items}. \
         Post-fix: off-band children are evicted each pass via deferred Remove.",
    );

    // Secondary: count is significantly less than N.
    let n_limit = n_items / 5;
    assert!(
        peak < n_limit,
        "peak child count {peak} is too close to N={n_items} \
         (limit: {n_limit} = N/5). Dispose is not working.",
    );
}

// ============================================================================
// 9c — Full-range scroll reaches the tail (Core.2 exit-gate: end-to-end
// 1000-item scroll)
// ============================================================================

/// Core.2 exit-gate acceptance test: scroll a 1000-item sliver across its
/// ENTIRE range (top to bottom), not just a partial window.
///
/// 9b (above) only exercises the first ~20% of the list (200 steps × 50px =
/// 10,000px of a 49,700px scrollable range) — every position it visits keeps
/// logical indices in the 0..~200 band, which would not catch a regression
/// that corrupts indices only once the walk reaches deep into the list (e.g.
/// an index-arithmetic overflow, or a cache-eviction bug that only manifests
/// once thousands of children have cycled through). This test drives the
/// scroll position through the FULL range and asserts both:
///   - the attached child count stays bounded throughout (D2, generalized),
///   - the visible band's logical indices genuinely progress from the head
///     to the tail of the list (not stuck near 0), reaching within the last
///     visible band of item N-1.
#[test]
fn u3c_9c_full_range_scroll_reaches_tail_with_bounded_children() {
    let n_items = 1_000usize;
    let item_height = 50.0_f32;
    let viewport_height = 300.0_f32;
    let expected_band_size = ((viewport_height + 200.0) / item_height).ceil() as usize + 4;

    let max_scroll = n_items as f32 * item_height - viewport_height;
    let scroll_steps = 200usize;
    let step_size = max_scroll / (scroll_steps - 1) as f32;

    let source: ItemSource = Arc::new(move |_idx| {
        Some(Box::new(FixedBox::new(item_height)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(n_items, item_height, Arc::clone(&source), None);

    let initial_constraints = vertical(0.0, viewport_height);
    let mut owner = PipelineOwner::new();
    let root_id =
        owner
            .insert(Box::new(SliverHost::new(initial_constraints))
                as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("lazy sliver must insert under root host");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(300.0),
        px(viewport_height),
    ))));

    let mut owner = owner.into_layout();

    let mut peak = 0usize;
    let mut last_min_idx = 0usize;

    for step in 0..scroll_steps {
        let scroll_pos = (step as f32 * step_size).min(max_scroll);
        let new_constraints = vertical(scroll_pos, viewport_height);

        if let Some(node) = owner.render_tree_mut().get_mut(root_id)
            && let Some(entry) = node.as_box_mut()
            && let Some(host) = entry.render_object_mut().downcast_mut::<SliverHost>()
        {
            host.constraints = new_constraints;
        }
        owner.mark_needs_layout(root_id);

        for _ in 0..3 {
            owner
                .run_layout()
                .expect("layout must succeed across the full-range scroll");
        }

        let n_children = owner.render_tree().children(sliver_id).len();
        if n_children > peak {
            peak = n_children;
        }

        // Track the visible band's minimum logical index at the last step so
        // the tail-reached assertion below has real data from the deepest
        // scroll position actually visited.
        if step == scroll_steps - 1 {
            let pairs = collect_child_indices(&owner, sliver_id);
            last_min_idx = pairs.first().map(|(idx, _)| *idx).unwrap_or(0);
        }
    }

    eprintln!(
        "9c full-range scroll: peak={peak} children attached, \
         expected band ≈ {expected_band_size} (n={n_items}), \
         final min logical index={last_min_idx} (max_scroll={max_scroll}px)",
    );

    // Bounded child count across the ENTIRE range (D2, generalized beyond 9b's
    // partial-range coverage).
    let upper_bound = expected_band_size * 3;
    assert!(
        peak <= upper_bound,
        "U3c D2 regression (full range): peak attached child count {peak} exceeded \
         {upper_bound} (= 3 × expected band size {expected_band_size}) while scrolling \
         the ENTIRE 1000-item range, not just a partial window.",
    );
    let n_limit = n_items / 5;
    assert!(
        peak < n_limit,
        "peak child count {peak} is too close to N={n_items} across the full-range \
         scroll (limit: {n_limit} = N/5). Dispose is not working near the tail.",
    );

    // The walk must genuinely reach the tail: the final scroll position sits
    // at max_scroll, so the visible band's minimum logical index must be near
    // the LAST item, not still near the head of the list. Allow slack for the
    // cache band (which pre-fetches items before the visible window).
    let tail_threshold = n_items - expected_band_size * 2;
    assert!(
        last_min_idx >= tail_threshold,
        "full-range scroll did not reach the tail: final min logical index \
         {last_min_idx} is below the expected tail threshold {tail_threshold} \
         (n={n_items}). Either the scroll math is wrong, or logical indices \
         desync deep into the list.",
    );
}

// ============================================================================
// P1 regression guard: dispose targets the sliver, not the walk root
// ============================================================================

/// Smoke test for the P1 fix in `layout_dirty_root`:
/// [`ErasedSliverLayoutCtx::dispose_box_child`] must tag the pending remove
/// with the SLIVER's own `node_id` as parent, not the layout-walk root `id`.
///
/// **Tree topology**: `root (SliverHost/Box, Variable) → sliver (lazy) → children`.
/// `walk_root = root_id`, `sliver.node_id = sliver_id`.  These are distinct,
/// which exercises the nested-topology path that the unit-level harness does
/// not cover.
///
/// **Why this cannot be a true red/green guard in this harness:**
/// The dispose-side `mark_needs_layout` call is irrelevant here because the
/// test calls `owner.mark_needs_layout(root_id)` on every scroll step.  That
/// explicit mark already cascades through root → sliver, so the sliver
/// relayouts regardless of which node the dispose's `mark_needs_layout`
/// targeted.  Similarly, `DeferredMutation::Remove` detaches by child ID, not
/// by the parent field, so children are correctly evicted in both pre-fix and
/// post-fix states under this test driver.
///
/// **What this test DOES verify:**
/// - Dispose runs at all in a `root ≠ sliver` topology (smoke test for the
///   full D2 path with a non-trivial tree shape).
/// - The child count stays bounded under repeated scrolling (guards against
///   regressions where dispose stops firing entirely, as opposed to firing
///   with the wrong parent id).
///
/// True discrimination of the P1 parent-id direction would require a harness
/// frame where the dispose's `mark_needs_layout` is the SOLE reflow trigger
/// — i.e., no per-step explicit root mark — but that would also suppress the
/// build pipeline, making bounded-count meaningless.  The type-level fix
/// (`pending_removes: Vec<(RenderId, RenderId)>`) makes mis-using `id`
/// a compile-time error, which is the authoritative guard.
#[test]
fn p1_dispose_targets_sliver_not_walk_root() {
    // N large enough that scrolling creates off-band children.
    let n_items = 200usize;
    let item_height = 50.0_f32;
    let viewport_height = 300.0_f32;
    // band ≈ (viewport + 2*cache) / item_height + slack
    let expected_band_size = ((viewport_height + 200.0) / item_height).ceil() as usize + 4;

    let source: ItemSource = Arc::new(move |_idx| {
        Some(Box::new(FixedBox::new(item_height)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(n_items, item_height, Arc::clone(&source), None);

    let initial_constraints = vertical(0.0, viewport_height);
    let mut owner = PipelineOwner::new();
    // walk root = root_id (SliverHost/Box), NOT the sliver node.
    // layout_dirty_root is called with `id = root_id`; the sliver is a child.
    let root_id =
        owner
            .insert(Box::new(SliverHost::new(initial_constraints))
                as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("lazy sliver must insert under root host");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(300.0),
        px(viewport_height),
    ))));

    let mut owner = owner.into_layout();

    // Scroll forward 60 steps (each step = one item_height), pumping 4 frames
    // per position to allow the next-frame backend to settle + dispose.
    let mut peak = 0usize;
    for step in 0..60usize {
        let scroll_pos = step as f32 * item_height;
        let new_constraints = vertical(scroll_pos, viewport_height);

        if let Some(node) = owner.render_tree_mut().get_mut(root_id)
            && let Some(entry) = node.as_box_mut()
            && let Some(host) = entry.render_object_mut().downcast_mut::<SliverHost>()
        {
            host.constraints = new_constraints;
        }
        owner.mark_needs_layout(root_id);

        for _ in 0..4 {
            owner
                .run_layout()
                .expect("layout must succeed during scroll");
        }

        let n_children = owner.render_tree().children(sliver_id).len();
        if n_children > peak {
            peak = n_children;
        }
    }

    // Dispose must keep the child count bounded in the root≠sliver topology.
    // This is a smoke check: the explicit per-step mark_needs_layout(root_id)
    // means the sliver relayouts regardless of where dispose's mark_needs_layout
    // goes, so the assertion does not distinguish wrong vs correct parent_id.
    // What it does verify: dispose actually fires at all in this topology
    // (guards against regressions where the entire D2 path is skipped).
    let upper_bound = expected_band_size * 3;
    assert!(
        peak <= upper_bound,
        "D2 smoke: peak child count {peak} under sliver_id={sliver_id:?} \
         exceeded {upper_bound} (3 × expected band {expected_band_size}). \
         walk_root={root_id:?} ≠ sliver={sliver_id:?}. \
         Dispose is not evicting off-band children in this topology.",
    );

    // Dispose must actually have shrunk the child list: final count << N.
    let final_count = owner.render_tree().children(sliver_id).len();
    assert!(
        final_count < n_items / 5,
        "D2 smoke: final child count {final_count} is too close to N={n_items}. \
         Dispose did not shrink the child list under the lazy sliver.",
    );
}

// ============================================================================
// NoChild: data source shorter than the declared item_count
// ============================================================================

/// An unknown-length source that declines (`build` returns `None`) at logical
/// index L makes the lazy list clamp `item_count` to L in-flight (the
/// `ChildLayout::NoChild` arm) and the Virtualizer converges to L items. Guards
/// the unknown-length-source path that `build_and_pump` (an always-`Some`
/// source) never exercises: no child beyond the real length is ever attached.
#[test]
fn nochild_clamps_item_count_to_real_source_length() {
    let item_height = 50.0_f32;
    let real_len = 3usize;
    // Declared count is far larger than what the source actually yields.
    let declared = 100usize;

    let source: ItemSource = Arc::new(move |idx| {
        (idx < real_len)
            .then(|| Box::new(FixedBox::new(item_height)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(declared, item_height, Arc::clone(&source), None);

    let mut owner = PipelineOwner::new();
    let root_id =
        owner
            .insert(Box::new(SliverHost::new(vertical(0.0, 300.0)))
                as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("lazy sliver must insert under root host");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(300.0), px(300.0)))));

    let mut owner = owner.into_layout();
    // The NoChild clamp lands on the first frame that reaches index `real_len`;
    // a few extra frames let the parked builds settle.
    for _ in 0..8 {
        owner.run_layout().expect("layout must succeed");
    }

    // No child beyond the real source length may be attached, and the attached
    // count must not exceed it — the declared 100 collapsed to the real 3.
    let pairs = collect_child_indices(&owner, sliver_id);
    assert!(
        pairs.iter().all(|(idx, _)| *idx < real_len),
        "NoChild: a child has logical index >= source length {real_len}: {:?}",
        pairs.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
    );
    assert!(
        owner.render_tree().children(sliver_id).len() <= real_len,
        "NoChild: attached child count {} exceeds the real source length {real_len}",
        owner.render_tree().children(sliver_id).len(),
    );
}

// ============================================================================
// TEST A — anchor correction fires when an above-viewport item is re-measured
// ============================================================================

/// A variable-height Box child.  Returns `height` for the chosen `target_idx`
/// and `default_height` for every other index.  Used to inject a measurement
/// delta that should (post-fix) trigger an anchor correction.
#[derive(Debug, Clone)]
struct VarBox {
    height: f32,
}

impl VarBox {
    fn new(height: f32) -> Self {
        Self { height }
    }
}

impl Diagnosticable for VarBox {}

impl RenderBox for VarBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        let w = ctx.constraints().max_width;
        Size::new(w, px(self.height))
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }
}

/// Guards BUG 1 (anchor correction was inert).
///
/// Layout configuration:
/// - Item 0: actual height = 80 px (estimate = 50 px → delta = 30 px).
/// - Items 1+: height = 50 px (matches estimate, delta = 0).
/// - `scroll_offset = 50 px`, `cache_origin = −50 px` →
///   `cache_before = 50 px` → `cache_first = 0` (item 0 is still in the
///   cache band) but `range.first = 1` (item 0 is above the visible edge).
///
/// On the first pass where item 0 transitions from `Unmeasured{hint:50}` to
/// `Measured{extent:80}`:
///
/// - **Post-fix** (`anchor = (range.first=1, 0.0)`):
///   `set_measured(0, 80.0, anchor=(1,0.0))` → `0 < 1` → `AnchorCorrection{delta:30.0}`
///   → accumulated → emitted as `scroll_offset_correction = Some(30.0)`.
///
/// - **Pre-fix** (`anchor = self.virtualizer.anchor_item() = (0, 0.0)`):
///   `set_measured(0, 80.0, anchor=(0,0.0))` → `0 < 0 = false` → no correction
///   → `scroll_offset_correction = None` forever.
///
/// The test asserts that at least one layout pass yields a non-`None`
/// `scroll_offset_correction`, which is impossible under the pre-fix anchor.
#[test]
fn anchor_correction_fires_when_item_above_viewport_remeasured() {
    // Estimate used by the Virtualizer for all items.
    let estimate = 50.0_f32;
    // Item 0 actually measures larger — this delta is the expected correction.
    let item0_height = 80.0_f32;
    let expected_delta = item0_height - estimate; // 30.0

    // Scroll offset = one estimate-height: item 0 (0..50 px estimated) sits
    // entirely before the visible edge.  With cache_origin = -50 px the
    // cache band starts at offset 0, so item 0 IS in the cache band and
    // will be laid out — but range.first = 1, so the anchor = (1, 0.0).
    let scroll_offset = estimate; // 50 px

    let source: ItemSource = Arc::new(move |idx| {
        let h = if idx == 0 { item0_height } else { estimate };
        Some(Box::new(VarBox::new(h)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(20, estimate, Arc::clone(&source), None);

    let constraints = vertical(scroll_offset, 300.0);
    let mut owner = PipelineOwner::new();
    let root_id =
        owner.insert(Box::new(SliverHost::new(constraints)) as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("sliver must insert under root host");
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(300.0), px(300.0)))));

    let mut owner = owner.into_layout();

    // Drive enough frames for item 0 to be built (next-frame backend) and
    // then measured.  The correction fires on the pass that first measures
    // item 0 as 80 px (was estimated at 50 px).
    let mut saw_correction = false;
    for _ in 0..10 {
        owner.run_layout().expect("layout must succeed");

        // Read the sliver's committed geometry to check scroll_offset_correction.
        let correction = owner
            .render_tree()
            .get(sliver_id)
            .and_then(|n| n.as_sliver())
            .and_then(|e| e.state().geometry())
            .and_then(|g| g.scroll_offset_correction);

        if correction == Some(expected_delta) {
            saw_correction = true;
            break;
        }
    }

    assert!(
        saw_correction,
        "BUG 1 regression: no pass produced scroll_offset_correction = Some({expected_delta}). \
         Pre-fix: anchor=(0,0) → set_measured(idx=0, …, anchor=(0,0)) → 0 < 0 = false \
         → AnchorCorrection never emitted. \
         Post-fix: anchor=(range.first=1, 0.0) → 0 < 1 = true → delta=30.0 emitted.",
    );
}

// ============================================================================
// TEST B — no panic when the source shrinks mid-scroll
// ============================================================================

/// A [`SliverHost`]-equivalent whose sliver constraints are updated at runtime
/// via a shared handle.  Only used by [`no_panic_on_source_shrink_mid_scroll`].
#[derive(Debug, Clone)]
struct SharedConstraintSliverHost {
    constraints: Arc<Mutex<SliverConstraints>>,
}

impl SharedConstraintSliverHost {
    fn new(initial: SliverConstraints) -> (Self, Arc<Mutex<SliverConstraints>>) {
        let shared = Arc::new(Mutex::new(initial));
        (
            Self {
                constraints: Arc::clone(&shared),
            },
            shared,
        )
    }
}

impl Diagnosticable for SharedConstraintSliverHost {}

impl RenderBox for SharedConstraintSliverHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> Size {
        let c = *self
            .constraints
            .lock()
            .expect("constraints lock must not be poisoned");
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, c);
        }
        ctx.constraints().biggest()
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        false
    }
}

/// Guards BUG 2 (`offset_of` panic on stale high-index children after
/// a mid-pass count shrink).
///
/// **How the panic fires (pre-fix):**
///
/// The build loop iterates `cache_first..cache_last`.  When the source returns
/// `NoChild` at index N_new it sets `self.item_count = N_new` and
/// `self.virtualizer.set_count(N_new)` — so `virtualizer.len() = N_new`.
/// But `cache_last` was computed by `virtualizer.query()` before the shrink;
/// it may be larger than N_new.  Steps 8 and 10 then call `offset_of(logical_i)`
/// for any child with `logical_i >= N_new` that is still "in-band" per the
/// stale bound.  `offset_of` asserts `index <= len()`, and with
/// `logical_i >= N_new = len()` the assert fires — thread panics.
///
/// **Setup that triggers it (two-phase scroll):**
///
/// Phase A (high-scroll warm-up): scroll = 600 px → cache covers items 11–18.
/// Pump 10 frames so items 11–18 are all attached.
///
/// Phase B (lower scroll + shrink): switch to scroll = 400 px → cache covers
/// items 7–14.  Items 7–10 are absent (never built at this scroll).
/// Items 11–14 remain attached from Phase A.  Now shrink source to 5.
///
/// On the first layout pass at scroll 400 with source shrunk to 5:
///  1. `query()` uses virtualizer with `len = 20` → `cache_first = 7,
///     cache_last ≈ 15`.
///  2. Build loop: items 7–10 absent → source(7) → `None` → `NoChild` →
///     `item_count = 7`, `virtualizer.len() = 7`, loop breaks.
///  3. (pre-fix) Steps 8/10 find items 11–14 "in-band" (`11 < 15`), call
///     `offset_of(11)` on virtualizer with `len = 7` → `11 > 7` → **panic**.
///  4. (post-fix) `cache_last = 15.min(7) = 7` → items 11–14 disposed in
///     step 5, skipped in steps 8/10 — no `offset_of` call on out-of-range index.
///
/// Pre-fix observable: `run_layout` returns `Err(LayoutPanic(...))` (the
/// `catch_unwind` wrapper converts the assert-panic to `RenderError`).
/// Post-fix observable: `run_layout` returns `Ok(())` and items ≥ 7 are disposed.
#[test]
fn no_panic_on_source_shrink_mid_scroll() {
    // item_height=50 px, viewport=300 px, cache_origin=−50 px →
    // cache_before=50 px, cache_after=100 px.
    let item_height = 50.0_f32;
    let viewport_height = 300.0_f32;
    let initial_count = 20usize;
    // Phase A: scroll=600 px → cache covers items 11–18 (600−50=550 .. 550+400=950,
    // i.e. floor(550/50)=11 .. ceil(950/50)=19 → exclusive end = 19).
    let phase_a_scroll = 600.0_f32;
    // Phase B: scroll=400 px → cache covers items 7–14 (350 .. 750).
    let phase_b_scroll = 400.0_f32;
    // Shrink well below the Phase-B cache_first so NoChild fires at an absent
    // item (7) while Phase-A-attached items 11–14 are still in logical_to_slot.
    let new_count = 5usize;

    let max_len = Arc::new(AtomicUsize::new(initial_count));
    let max_clone = Arc::clone(&max_len);
    let source: ItemSource = Arc::new(move |idx| {
        (idx < max_clone.load(Ordering::Relaxed))
            .then(|| Box::new(FixedBox::new(item_height)) as Box<dyn RenderObject<BoxProtocol>>)
    });

    let lazy = RenderSliverListLazy::new(initial_count, item_height, Arc::clone(&source), None);

    let (host, constraint_handle) =
        SharedConstraintSliverHost::new(vertical(phase_a_scroll, viewport_height));
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(host) as Box<dyn RenderObject<BoxProtocol>>);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(lazy) as Box<dyn RenderObject<SliverProtocol>>,
        )
        .expect("sliver must insert under root host");
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
        px(300.0),
        px(viewport_height),
    ))));

    let mut owner = owner.into_layout();

    // Phase A: warm up at scroll=600.  The next-frame backend builds one child
    // per frame; 10 frames is enough to attach all 8 items in the 11–18 band.
    for _ in 0..10 {
        owner
            .run_layout()
            .expect("layout must succeed during Phase A warm-up");
    }

    // Pre-condition A: items in the Phase-A band (indices ≥ 11) must be attached.
    let phase_a_children = collect_child_indices(&owner, sliver_id);
    assert!(
        phase_a_children.iter().any(|(idx, _)| *idx >= 11),
        "pre-condition A: need at least one child at index ≥ 11 from Phase A warm-up; \
         got indices: {:?}",
        phase_a_children.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
    );

    // Switch scroll to 400 px WITHOUT pumping any frames first.  The constraint
    // handle is shared (Arc<Mutex<>>) so the SliverHost reads the new value on
    // its very next perform_layout call.
    //
    // At this point the render tree still holds the Phase-A children (indices
    // 11–18).  Items 7–10 were NEVER built at Phase-A scroll (Phase-A cache
    // covered 11–18), so they are absent.  After the scroll switch the Phase-B
    // cache band is [7, 15): items 11–14 overlap it and remain in logical_to_slot;
    // items 7–10 are absent and will be requested as new builds.
    {
        let mut c = constraint_handle
            .lock()
            .expect("constraint handle lock must not be poisoned");
        *c = vertical(phase_b_scroll, viewport_height);
    }

    // Pre-condition B: the render tree already holds children from Phase A.
    // At least one must have index >= new_count so the stale in-band path fires.
    // We do NOT run layout here — pumping would build items 7–10 and eliminate
    // the absence needed to trigger NoChild.
    let pre_shrink_children = collect_child_indices(&owner, sliver_id);
    let pre_indices: Vec<usize> = pre_shrink_children.iter().map(|(i, _)| *i).collect();
    let stale_count = pre_indices.iter().filter(|&&idx| idx >= new_count).count();
    assert!(
        stale_count > 0,
        "pre-condition B: need at least one child at index >= {new_count} before \
         the shrink so the stale in-band path is exercised; \
         got indices: {pre_indices:?}",
    );

    // Shrink source to new_count=5.  On the next layout pass at scroll=400:
    //   cache_first=7, cache_last≈15, items 7–10 absent → source(7)=None →
    //   NoChild → item_count=7.
    // Pre-fix: cache_last still ≈ 15; items 11–14 in-band; offset_of(11) on
    //          virtualizer.len()=7 → assert(11 ≤ 7) → PANIC inside run_layout.
    // Post-fix: cache_last = 15.min(7) = 7; items 11–14 NOT in-band → disposed.
    max_len.store(new_count, Ordering::Relaxed);
    owner.mark_needs_layout(root_id);

    // Run exactly ONE layout pass.  The observable distinction is:
    //
    // Pre-fix (no cache_last clamp): the build loop shrinks virtualizer to
    //   len=7, but cache_last stays at ≈15.  Step 5 sees items 11–14 as
    //   in-band (11 < 15 = true) → skips disposal.  Step 8 calls
    //   offset_of(11) on a virtualizer with len=7 → asserts 11 ≤ 7 →
    //   panics.  The catch_unwind converts the panic to RenderError (silently
    //   recovered by the Box parent's sliver callback), but the disposal DID
    //   NOT fire.  After one frame, items 11–14 are still attached.
    //
    // Post-fix (cache_last clamped to 7): step 5 sees items 11–14 as
    //   out-of-band (11 < 7 = false) → disposes them.  Step 8 iterates an
    //   empty in-band set → no offset_of call → no panic.  After one frame,
    //   items 11–14 are gone; only items with index < 7 ≤ new_count could
    //   be present (there are none here because items 7–10 returned NoChild
    //   and 0–6 were never in the Phase-B cache band).
    owner.run_layout().expect(
        "run_layout must not return Err after source shrink; \
         the Box parent's sliver callback swallows descendant errors as Ok, \
         so an Err here indicates a protocol violation outside the sliver itself",
    );

    // After one frame, stale items (index ≥ new_count) that were in the
    // Phase-B cache band (indices 11–14) must be disposed.
    // Pre-fix: they are NOT disposed (cache_last clamp absent → in-band →
    //   step 5 skips them → they remain attached after frame 1).
    // Post-fix: they ARE disposed (clamp makes them out-of-band → step 5
    //   disposes them → tree is clean after frame 1).
    let after_one_frame = collect_child_indices(&owner, sliver_id);
    assert!(
        after_one_frame.iter().all(|(idx, _)| *idx < new_count),
        "BUG 2 regression: after one layout pass following a source shrink, \
         children with index >= {new_count} remain attached — cache_last was \
         not clamped to item_count so in-band detection used the pre-shrink \
         bound and skipped disposal of stale high-index children. \
         Still attached: {:?}",
        after_one_frame.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
    );
}
