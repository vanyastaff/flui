//! Integration tests for the lazy-sliver backend (U4.3).
//!
//! Exercises the 7 correctness paths the single-node `LeafBox` harness missed.
//! Each test uses the headless frame driver (`pump_frame`) which, since U4.3,
//! calls `service_child_requests` after `run_frame` — so two `tick` calls
//! are enough to settle a visible window: the first dispatches the child-build
//! request; the second lays out the now-built children.
//!
//! # Frame sequence (per `pump_frame`)
//!
//! 1. `build_scope` — drains the element-level dirty heap.
//! 2. `run_frame`  — layout: the sliver emits pending child requests and a
//!    retain-band signal.
//! 3. `service_child_requests` — drains both buffers, calls each registered
//!    `ChildManager::service` (build new, evict off-band), runs a second
//!    `build_scope` for freshly-scheduled children, marks the sliver dirty,
//!    and finalizes any inactive elements (including sparse children pushed
//!    by `on_unmount` — F3).
//!
//! So: after `lay_out` the sliver has no children; after `tick1` children are
//! built and the sliver is marked dirty; after `tick2` the sliver lays out
//! its real children and reaches a stable state.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::common::{lay_out, tight};
use flui_view::ViewExt;
use flui_widgets::prelude::*;

// ============================================================================
// Test 1 — basic settle: all visible items built
// ============================================================================

/// After two ticks a `ListView::builder` over N items whose combined extent
/// fits within the viewport must have exactly N item render nodes in the tree,
/// plus 1 for the `RenderViewport` and 1 for the `RenderSliverList`.
///
/// Exercises the basic request→service→layout path (F1, F8 plan obligations).
#[test]
fn lazy_list_view_builder_builds_visible_items() {
    // 3 items × 48 px = 144 px total; viewport height = 200 px → all visible.
    let mut laid = lay_out(
        ListView::builder(3, 48.0, |i| {
            if i < 3 {
                Some(SizedBox::new(200.0, 48.0).boxed())
            } else {
                None
            }
        }),
        tight(200.0, 200.0),
    );

    // tick1: run_frame requests children → service builds them.
    laid.tick();
    // tick2: sliver dirty → laid out with real children.
    laid.tick();

    // Expected: 1 (RenderViewport) + 1 (RenderSliverList) + 3 (item nodes) = 5.
    let nodes_after_settle = laid.render_node_count();
    assert_eq!(
        nodes_after_settle, 5,
        "after settle, render tree should have 1 viewport + 1 sliver + 3 items = 5 nodes; \
         got {nodes_after_settle}"
    );
}

// ============================================================================
// Test 2 — None-at-K caps the build count
// ============================================================================

/// When the builder returns `None` for indices ≥ K, the list must stop building
/// at K items even if `item_count` is larger. The stricter bound wins.
#[test]
fn lazy_list_view_builder_none_at_k_caps_build_count() {
    const K: usize = 2;
    // item_count=10 but builder returns None for i >= K.
    let mut laid = lay_out(
        ListView::builder(10, 48.0, |i| {
            if i < K {
                Some(SizedBox::new(200.0, 48.0).boxed())
            } else {
                None
            }
        }),
        // Viewport tall enough to request all 10 items if they were all present.
        tight(200.0, 600.0),
    );

    laid.tick();
    laid.tick();

    // Expected: 1 (viewport) + 1 (sliver) + K (items capped by None-return) = 4.
    let nodes_after_settle = laid.render_node_count();
    let expected = 1 + 1 + K;
    assert_eq!(
        nodes_after_settle, expected,
        "None-at-K must cap build count: expected {expected} nodes, \
         got {nodes_after_settle}"
    );
}

// ============================================================================
// Test 3 — F2: multi-node child view (subtree build + subtree evict soundness)
// ============================================================================

/// Each item is a `Padding` wrapping a `SizedBox` — two render nodes per item.
///
/// Exercises F2: `SparseChildren::ensure` must schedule a full second
/// `build_scope` pass so the Padding element builds its SizedBox child, and
/// `SparseChildren::evict` must remove the child's **whole subtree** (both
/// render nodes), not just the root node.
///
/// With 3 items × 2 render nodes each, plus 1 viewport + 1 sliver = 8 total.
#[test]
fn lazy_list_view_builder_multi_node_child() {
    let mut laid = lay_out(
        ListView::builder(3, 64.0, |i| {
            if i < 3 {
                // Padding(all=8) wraps SizedBox(184×48):
                //   total item width  = 184 + 8 + 8 = 200  (fills viewport cross-axis)
                //   total item height =  48 + 8 + 8 = 64   (matches the extent estimate)
                // Two render nodes: RenderPadding + RenderConstrainedBox.
                Some(Padding::all(8.0).child(SizedBox::new(184.0, 48.0)).boxed())
            } else {
                None
            }
        }),
        tight(200.0, 250.0),
    );

    laid.tick();
    laid.tick();

    // 1 (RenderViewport) + 1 (RenderSliverList) + 3 × 2 (Padding + SizedBox) = 8.
    let nodes_after_settle = laid.render_node_count();
    assert_eq!(
        nodes_after_settle, 8,
        "each 2-node item must contribute exactly 2 render nodes; \
         got {nodes_after_settle} (expected 8 = 1 viewport + 1 sliver + 3 × 2 items)"
    );
}

// ============================================================================
// Test 4 — third tick is idempotent (build-count invariant)
// ============================================================================

/// After the list has settled (two ticks), a third tick must NOT add or remove
/// any render nodes, and the builder closure must NOT be called again. The stable
/// state is a fixed point: no new children are built and no existing ones are
/// evicted on an un-driven frame.
///
/// The `Arc<AtomicUsize>` build counter gives a precise quiescence signal: if any
/// new build occurs on tick 3, `builds_at_third_tick > builds_at_settle` and the
/// test fails — proving the `ChildManager::service` bool-gate is working correctly.
#[test]
fn lazy_list_view_builder_third_tick_is_idempotent() {
    let items_built = Arc::new(AtomicUsize::new(0));

    let mut laid = lay_out(
        ListView::builder(3, 48.0, {
            let items_built = Arc::clone(&items_built);
            move |i| {
                if i < 3 {
                    items_built.fetch_add(1, Ordering::Relaxed);
                    Some(SizedBox::new(200.0, 48.0).boxed())
                } else {
                    None
                }
            }
        }),
        tight(200.0, 200.0),
    );

    laid.tick(); // tick1: service builds children, build counter increments
    laid.tick(); // tick2: sliver lays out built children (no new builds needed)

    let nodes_at_settle = laid.render_node_count();
    let builds_at_settle = items_built.load(Ordering::Relaxed);

    // tick3: no-op — neither the element tree nor the sliver is dirty after settle.
    laid.tick();

    let nodes_at_third_tick = laid.render_node_count();
    let builds_at_third_tick = items_built.load(Ordering::Relaxed);

    assert_eq!(
        nodes_at_settle, nodes_at_third_tick,
        "a third tick must not change the render node count: \
         settled at {nodes_at_settle}, after tick3: {nodes_at_third_tick}"
    );
    assert_eq!(
        builds_at_settle, builds_at_third_tick,
        "a third tick must trigger zero new item builds (quiescence invariant); \
         settled after {builds_at_settle} builds, \
         tick3 raised the count to {builds_at_third_tick}"
    );
}

// ============================================================================
// Test 5 — F3: host unmount cleans up all lazy children
// ============================================================================

/// A `StatefulView` that starts as a `ListView::builder` then switches to a
/// plain `SizedBox`. After the switch is pumped, all lazy children and their
/// render nodes must be gone — F3: `on_unmount` pushes sparse children to the
/// inactive queue; `service_child_requests`'s unconditional `finalize_tree`
/// pre-pass then drains the queue even when no layout requests are pending.
///
/// Without the F3 fix, `finalize_tree` skipped sparse children because they
/// never appear in the host's `child_ids` (F4 invariant) and
/// `service_child_requests` would early-return before reaching `finalize_tree`.
#[derive(Clone, StatefulView)]
struct MaybeListView {
    show_list: Arc<AtomicBool>,
}

struct MaybeListViewState {
    show_list: Arc<AtomicBool>,
}

impl StatefulView for MaybeListView {
    type State = MaybeListViewState;

    fn create_state(&self) -> MaybeListViewState {
        MaybeListViewState {
            show_list: Arc::clone(&self.show_list),
        }
    }
}

impl ViewState<MaybeListView> for MaybeListViewState {
    fn build(&self, _view: &MaybeListView, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show_list.load(Ordering::Relaxed) {
            // Lazy list: viewport(1) + sliver(1) + 3 items(3) = 5 render nodes.
            ListView::builder(3, 48.0, |i| {
                if i < 3 {
                    Some(SizedBox::new(200.0, 48.0).boxed())
                } else {
                    None
                }
            })
            .boxed()
        } else {
            // Single SizedBox: 1 render node.
            SizedBox::square(100.0).boxed()
        }
    }
}

#[test]
fn lazy_list_view_builder_host_unmount_cleans_render_nodes() {
    let show_list = Arc::new(AtomicBool::new(true));

    let mut laid = lay_out(
        MaybeListView {
            show_list: Arc::clone(&show_list),
        },
        tight(200.0, 300.0),
    );

    // Settle the lazy list: tick1 builds children, tick2 lays them out.
    laid.tick();
    laid.tick();

    let nodes_with_list_mounted = laid.render_node_count();
    // Should be 5: viewport(1) + sliver(1) + 3 items. Assert ≥ 5 as a sanity check
    // (caching may add extra items, but all must be cleaned up on unmount).
    assert!(
        nodes_with_list_mounted >= 5,
        "render tree should have ≥5 nodes while list is mounted; \
         got {nodes_with_list_mounted}"
    );

    // Element tree includes StatefulView/StatelessView wrapper elements that own
    // no render nodes (e.g. MaybeListView element, ListView element, Viewport element)
    // on top of the render-bearing ones — so the element count must be ≥ the render count.
    let elements_with_list_mounted = laid.element_node_count();
    assert!(
        elements_with_list_mounted >= nodes_with_list_mounted,
        "element tree must have at least as many nodes as the render tree while the list \
         is mounted (stateless/stateful wrappers add element-only nodes); \
         render: {nodes_with_list_mounted}, element: {elements_with_list_mounted}"
    );

    // Switch to SizedBox — triggers a StatefulView rebuild that unmounts the list.
    show_list.store(false, Ordering::Relaxed);
    // `pump` marks root dirty and drives one frame. `service_child_requests`
    // unconditionally finalizes inactive_elements (F3 fix), so lazy children
    // pushed by `on_unmount` are cleaned up in the same frame.
    laid.pump();

    // After unmount the lazy children must have been cleaned up (F3).
    // Only the SizedBox render node remains.
    let nodes_after_unmount = laid.render_node_count();
    assert_eq!(
        nodes_after_unmount, 1,
        "after unmounting the ListView all lazy children must be cleaned up (F3); \
         got {nodes_after_unmount} render nodes (expected 1 for the SizedBox)"
    );

    // Both the element tree and the render tree must shrink on unmount.
    let elements_after_unmount = laid.element_node_count();
    assert!(
        elements_after_unmount < elements_with_list_mounted,
        "element tree must shrink after unmounting the ListView: \
         was {elements_with_list_mounted}, now {elements_after_unmount}"
    );
}

// ============================================================================
// Test 6 — convergence: items taller than estimate reach a fixed point
// ============================================================================

/// When the actual item extent differs from the estimate the virtualizer
/// corrects its band on each layout pass. The correction must terminate
/// (no oscillation) within a small number of frames — a fixed point must
/// be reached and held, with only the visible + cache-band items built (not all).
///
/// Here actual extent (64 px) > estimate (24 px). After 6 frames the
/// render-node count must be stable and must be far fewer than the total
/// item count (only the visible+cached window is built). We use 50 items so
/// off-band eviction is guaranteed: 50 × 64 px = 3 200 px  >>  192 px
/// viewport + 250 px-per-side cache margin = 692 px cache window.
#[test]
fn lazy_list_view_builder_convergence_stabilizes() {
    // 50 items, estimate 24 px, actual 64 px → virtualizer corrects each frame.
    // Only ~10-11 items fit in the 192 px viewport + 250 px cache margin on
    // each side; the rest are evicted as the band converges.
    let mut laid = lay_out(
        ListView::builder(50, 24.0, |i| {
            if i < 50 {
                Some(SizedBox::new(200.0, 64.0).boxed())
            } else {
                None
            }
        }),
        // 200×192 viewport: fits exactly 3 items at 64 px each.
        tight(200.0, 192.0),
    );

    // Drive 6 frames — a converging virtualizer must settle well within this.
    for _ in 0..6 {
        laid.tick();
    }

    let nodes_before_stability_check = laid.render_node_count();

    // One more frame: must not change the count (fixed point reached).
    laid.tick();
    let nodes_after_stability_check = laid.render_node_count();

    assert_eq!(
        nodes_before_stability_check, nodes_after_stability_check,
        "convergence must be a fixed point by frame 6; \
         before={nodes_before_stability_check}, after={nodes_after_stability_check} \
         (oscillation detected)"
    );

    // Subtract 2 structural nodes (viewport + sliver) to get item render-node count.
    let item_render_nodes = nodes_after_stability_check.saturating_sub(2);

    // Lower bound: the 192 px viewport fits exactly 3 items at 64 px each, so all
    // 3 visible items must be built at convergence.
    assert!(
        item_render_nodes >= 3,
        "192 px viewport / 64 px per item = 3 visible items must all be built at \
         convergence; got {item_render_nodes} item render nodes"
    );

    // Upper bound: only visible + cached items built, never all 50.
    // The 192 px viewport + 250 px cache margins on each side ≈ 692 px cache
    // window; at 64 px/item that fits at most 11 items — far fewer than 50.
    assert!(
        item_render_nodes < 50,
        "convergence must build only the visible+cached window, not all 50 items; \
         item render nodes built: {item_render_nodes}"
    );
}

// ============================================================================
// Test 7 — F5: off-band eviction is bounded (no ABA double-remove)
// ============================================================================

/// A large list where the viewport shows only a few items. After settling,
/// the render tree must contain only the visible + cache-band items, not all
/// N — confirming that off-band children are evicted correctly via the
/// retain-band channel (not `dispose_box_child`, which would double-remove
/// render nodes owned by the element tree, the F5 ABA bug).
///
/// A post-settle relayout tick must not grow the node count (no leak) and
/// must not panic (the ABA would surface as a slab-index panic).
#[test]
fn lazy_list_view_builder_off_band_eviction_bounded() {
    const ITEM_COUNT: usize = 50;
    // Viewport 96px fits exactly 2 items at 48px each.
    let mut laid = lay_out(
        ListView::builder(ITEM_COUNT, 48.0, |i| {
            if i < ITEM_COUNT {
                Some(SizedBox::new(200.0, 48.0).boxed())
            } else {
                None
            }
        }),
        tight(200.0, 96.0),
    );

    // Settle over multiple ticks (more than 2 in case the cache band takes a
    // pass to stabilize after the first layout).
    for _ in 0..4 {
        laid.tick();
    }

    let nodes_after_settle = laid.render_node_count();

    // Off-band eviction: only ~2 visible + cache-band items should be built.
    // The exact cache margin depends on the virtualizer, but must be far fewer
    // than ITEM_COUNT. Allow up to 20 for a generous cache band, ensuring
    // at least 30 items of the 50 were NOT built (the eviction is real).
    assert!(
        nodes_after_settle <= 20,
        "off-band eviction must limit built items to the viewport + cache band; \
         got {nodes_after_settle} render nodes for {ITEM_COUNT} items \
         in a 96 px viewport (expected ≤20)"
    );

    // A further relayout tick must not panic (no ABA double-remove, F5) and
    // must not grow the node count.
    laid.tick();
    let nodes_after_relayout = laid.render_node_count();
    assert!(
        nodes_after_relayout <= nodes_after_settle,
        "a post-settle relayout tick must not leak render nodes; \
         count went from {nodes_after_settle} to {nodes_after_relayout}"
    );
}
