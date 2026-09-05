//! Integration tests for the lazy-sliver backend.
//!
//! Exercises the 7 correctness paths the single-node `LeafBox` harness missed.
//! Each test uses the headless frame driver (`pump_frame`) which, since the
//! child-manager wiring landed, calls `service_child_requests` after `run_frame` — so two `tick` calls
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
//!    by `on_unmount` because the host's own `child_ids` stays empty).
//!
//! So: after `lay_out` the sliver has no children; after `tick1` children are
//! built and the sliver is marked dirty; after `tick2` the sliver lays out
//! its real children and reaches a stable state.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::common::{LaidOut, lay_out, tight};
use flui_view::ViewExt;
use flui_widgets::prelude::*;

// ============================================================================
// Test 1 — basic settle: all visible items built
// ============================================================================

/// After two ticks a `ListView::builder` over N items whose combined extent
/// fits within the viewport must have exactly N item render nodes in the tree,
/// plus 1 for the `RenderViewport` and 1 for the `RenderSliverList`.
///
/// Exercises the basic request→service→layout path end to end.
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

    // Expected: 1 (RenderViewport) + 1 (RenderSliverList) + 3 (per-item
    // RenderRepaintBoundary) + 3 (item nodes) = 8.
    //
    // The per-item boundary is Flutter's: `SliverChildBuilderDelegate` defaults
    // `addRepaintBoundaries` to `true` and wraps each child
    // (`widgets/scroll_delegate.dart:560`), so a scrolling list does not
    // repaint items that did not change.
    let nodes_after_settle = laid.render_node_count();
    assert_eq!(
        nodes_after_settle, 8,
        "after settle, render tree should have 1 viewport + 1 sliver + 3 boundaries \
         + 3 items = 8 nodes; got {nodes_after_settle}"
    );
}

// ============================================================================
// Test 1b — composite (non-render) children settle and carry their index
// ============================================================================

/// A composite top-level child: a `StatelessView` that builds into a
/// `SizedBox` one level down. This is the shape `ListView::builder` callers
/// write constantly (a bare `Text`, a small extracted widget) and the one
/// every other test here misses, because they all hand back a `SizedBox`,
/// which owns a render node the moment `insert` returns.
#[derive(Clone, StatelessView)]
struct CompositeItem {
    height: f32,
}

impl StatelessView for CompositeItem {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, self.height)
    }
}

/// KNOWN GAP — a lazy list cannot yet take a composite child when the
/// per-item repaint boundary is switched off.
///
/// The sliver maps `logical -> dense slot` from parent data alone, so every
/// child's render node is stamped with its logical index at
/// `SparseChildren::ensure` time. A *composite* child (one whose top-level
/// view owns no render object — a bare `Text`, a `StatefulView`, an extracted
/// widget) has no render node at that moment: its first render descendant
/// only appears after the follow-up build pass expands the subtree. The stamp
/// therefore has nowhere to land, and `stamp_logical_index` reports it with a
/// `debug_assert!(false)`.
///
/// The default path hides this, which is why no other test here sees it:
/// `addRepaintBoundaries` defaults to `true`, and the boundary owns a render
/// node, so the composite view is never itself the top-level sparse child.
/// Turning the boundary off is what exposes the gap.
///
/// This test is `#[ignore]`d rather than deleted so the gap stays
/// reproducible in-tree instead of living in a branch. Deferring the stamp
/// until after the build pass removes the assertion but is NOT sufficient on
/// its own — measured: the items still do not materialise (2 render nodes
/// instead of 5), so the sliver's own layout path needs the matching work
/// before this can be un-ignored.
#[test]
fn lazy_list_view_builder_settles_composite_children() {
    let mut laid = lay_out(
        ListView::builder(3, 48.0, |i| {
            if i < 3 {
                Some(CompositeItem { height: 48.0 }.boxed())
            } else {
                None
            }
        })
        // Without the per-item boundary the COMPOSITE view is itself the
        // top-level sparse child, so nothing owns a render node when the
        // logical index is stamped. With the boundary on (the default) the
        // boundary supplies one and the composite path is never reached.
        .repaint_boundaries(false),
        tight(200.0, 200.0),
    );

    laid.tick();
    laid.tick();

    // 1 viewport + 1 sliver + 3 item nodes; no per-item boundary here.
    let nodes_after_settle = laid.render_node_count();
    assert_eq!(
        nodes_after_settle, 5,
        "a composite child must settle like a render child; got {nodes_after_settle}"
    );
}

// ============================================================================
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
    // `+ 2 * K`, not `+ K`: each item carries its own `RenderRepaintBoundary`,
    // which `SliverChildBuilderDelegate` adds by default exactly as Flutter's
    // does (`widgets/scroll_delegate.dart:560`).
    let expected = 1 + 1 + 2 * K;
    assert_eq!(
        nodes_after_settle, expected,
        "None-at-K must cap build count: expected {expected} nodes, \
         got {nodes_after_settle}"
    );
}

// ============================================================================
// Test 3 — multi-node child view (subtree build + subtree evict soundness)
// ============================================================================

/// Each item is a `Padding` wrapping a `SizedBox` — two render nodes per item.
///
/// Exercises how `SparseChildren::ensure` must schedule a full second
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
        nodes_after_settle, 11,
        "each 2-node item must contribute exactly 2 render nodes, plus its own \
         repaint boundary; got {nodes_after_settle} (expected 11 = 1 viewport \
         + 1 sliver + 3 boundaries + 3 × 2 items)"
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
// Test 5 — host unmount cleans up all lazy children
// ============================================================================

/// A `StatefulView` that starts as a `ListView::builder` then switches to a
/// plain `SizedBox`. After the switch is pumped, all lazy children and their
/// render nodes must be gone: `on_unmount` pushes sparse children to the
/// inactive queue; `service_child_requests`'s unconditional `finalize_tree`
/// pre-pass then drains the queue even when no layout requests are pending.
///
/// Before that fix, `finalize_tree` skipped sparse children because they
/// never appear in the host's `child_ids` (the host stays empty by
/// invariant) and `service_child_requests` would early-return before
/// reaching `finalize_tree`.
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
    // unconditionally finalizes inactive_elements, so lazy children pushed
    // by `on_unmount` are cleaned up in the same frame.
    laid.pump();

    // After unmount the lazy children must have been cleaned up.
    // Only the SizedBox render node remains.
    let nodes_after_unmount = laid.render_node_count();
    assert_eq!(
        nodes_after_unmount, 1,
        "after unmounting the ListView all lazy children must be cleaned up; \
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
// Test 7 — off-band eviction is bounded (no ABA double-remove)
// ============================================================================

/// A large list where the viewport shows only a few items. After settling,
/// the render tree must contain only the visible + cache-band items, not all
/// N — confirming that off-band children are evicted correctly via the
/// retain-band channel. The render side never disposes a child itself, which
/// is what avoids an ABA double-remove of nodes owned by the element tree.
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

    // A further relayout tick must not panic (no ABA double-remove) and
    // must not grow the node count.
    laid.tick();
    let nodes_after_relayout = laid.render_node_count();
    assert!(
        nodes_after_relayout <= nodes_after_settle,
        "a post-settle relayout tick must not leak render nodes; \
         count went from {nodes_after_settle} to {nodes_after_relayout}"
    );
}

// ============================================================================
// Repaint boundaries per item (Flutter parity)
// ============================================================================

/// Every list item sits under its own `RenderRepaintBoundary`.
///
/// Flutter parity: `SliverChildBuilderDelegate` and `SliverChildListDelegate`
/// both default `addRepaintBoundaries` to `true` and wrap each child
/// (`widgets/scroll_delegate.dart:560` and `:774`). Their doc gives the reason
/// — children in a scrolling container "do not need to be repainted as the
/// list scrolls".
///
/// This is the structural precondition for
/// `PipelineOwner::retained_boundaries` to do anything in a list: without a
/// boundary per item, the paint walk descends into every visible item on every
/// frame and there is nothing to reuse. The behaviour that follows from it is
/// pinned in `flui-rendering`'s `retained_boundary_layers` tests.
#[test]
fn lazy_list_view_builder_wraps_each_item_in_a_repaint_boundary() {
    const ITEMS: usize = 3;

    let mut laid = lay_out(
        ListView::builder(ITEMS, 100.0, |i| {
            (i < ITEMS).then(|| SizedBox::square(100.0).boxed())
        }),
        tight(200.0, 400.0),
    );
    laid.tick();
    laid.tick();

    let boundaries = laid.find_all_by_render_type("RenderRepaintBoundary");
    let items = laid.find_all_by_render_type("RenderConstrainedBox");

    assert_eq!(
        items.len(),
        ITEMS,
        "precondition: all {ITEMS} items are built and attached; got {items:?}"
    );
    assert_eq!(
        boundaries.len(),
        ITEMS,
        "each item must carry its own repaint boundary — one per item, not one \
         for the list; got {} boundaries for {ITEMS} items",
        boundaries.len()
    );
}

/// `repaint_boundaries(false)` actually reaches the tree.
///
/// The knob exists because Flutter's does (`addRepaintBoundaries`), for the
/// case its doc names: items cheaper to repaint than to composite. An earlier
/// version of this work put the opt-out on the delegate, where no widget could
/// reach it — the flag was public API that nothing could call. This pins that
/// it is wired end to end.
#[test]
fn lazy_list_view_builder_repaint_boundaries_false_drops_the_wrappers() {
    const ITEMS: usize = 3;

    let build = |add: bool| {
        let mut laid = lay_out(
            ListView::builder(ITEMS, 100.0, |i| {
                (i < ITEMS).then(|| SizedBox::square(100.0).boxed())
            })
            .repaint_boundaries(add),
            tight(200.0, 400.0),
        );
        laid.tick();
        laid.tick();
        (
            laid.find_all_by_render_type("RenderRepaintBoundary").len(),
            laid.find_all_by_render_type("RenderConstrainedBox").len(),
        )
    };

    let (with_boundaries, items_with) = build(true);
    let (without_boundaries, items_without) = build(false);

    assert_eq!(
        (items_with, items_without),
        (ITEMS, ITEMS),
        "precondition: the items themselves are built either way"
    );
    assert_eq!(
        with_boundaries, ITEMS,
        "the default wraps every item; got {with_boundaries}"
    );
    assert_eq!(
        without_boundaries, 0,
        "repaint_boundaries(false) must drop them; got {without_boundaries}"
    );
}

// ============================================================================
// Same-frame materialisation — the band a layout pass requests is built,
// laid out, and painted in that same frame
// ============================================================================

/// Lazy child requests are serviced inside the frame's layout↔build fixpoint,
/// not after paint: the bootstrap frame alone materialises every in-band item,
/// and a root swap that jumps the offset shows the new band — with the old
/// one evicted — after the single frame `pump_widget` drives. Before this the
/// list needed a second tick for every band change, so a scroll painted one
/// frame behind its position.
#[test]
fn lazy_list_view_builder_materialises_the_band_in_the_same_frame() {
    const ITEM_COUNT: usize = 100;
    const ITEM_EXTENT: f32 = 48.0;
    let list = |offset: f32| {
        ListView::builder(ITEM_COUNT, ITEM_EXTENT, |i| {
            // Exactly `ITEM_EXTENT` tall, so a jump to `50 * ITEM_EXTENT`
            // lands on item 50 whatever the estimate has adapted to.
            (i < ITEM_COUNT).then(|| {
                SizedBox::new(200.0, ITEM_EXTENT)
                    .child(Text::new(format!("item{i}")))
                    .boxed()
            })
        })
        // No per-item boundary: the harder case, where the item's own
        // render object is what must carry the index.
        .repaint_boundaries(false)
        .offset(offset)
    };
    let mut laid = lay_out(list(0.0), tight(200.0, 200.0));

    // The oracle is a LAID-OUT size, not mere presence: the old post-paint
    // service pass also created the paragraph nodes, but left them unsized
    // until the next frame. A non-zero height proves the fixpoint laid the
    // fresh band out before this frame painted.
    let laid_out_height = |laid: &LaidOut, text: &str| -> Option<f32> {
        laid.find_text(text).map(|id| laid.size(id).height.get())
    };

    // No tick: the bootstrap frame is the whole story.
    for text in ["item0", "item4"] {
        let height = laid_out_height(&laid, text);
        assert!(
            height.is_some_and(|h| h > 0.0),
            "{text} must be built AND laid out by the frame that requested it; height={height:?}"
        );
    }
    assert!(
        laid.find_text("item50").is_none(),
        "an item far outside the band must not be built"
    );

    // Jump the offset to item 50 through a root swap: one frame.
    laid.pump_widget(list(50.0 * ITEM_EXTENT));
    for text in ["item50", "item54"] {
        let height = laid_out_height(&laid, text);
        assert!(
            height.is_some_and(|h| h > 0.0),
            "{text} must be built AND laid out in the frame that moved the offset; height={height:?}"
        );
    }
    assert!(
        laid.find_text("item0").is_none(),
        "the old band must be evicted in that same frame"
    );
}

// ============================================================================
// Stateful items — init on entering the band, dispose on leaving it
// ============================================================================

#[derive(Clone, StatefulView)]
struct ProbeItem {
    index: usize,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
}

struct ProbeItemState {
    index: usize,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
}

impl StatefulView for ProbeItem {
    type State = ProbeItemState;
    fn create_state(&self) -> ProbeItemState {
        ProbeItemState {
            index: self.index,
            log: Arc::clone(&self.log),
        }
    }
}

impl ViewState<ProbeItem> for ProbeItemState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        self.log.lock().push((self.index, "init"));
    }
    fn build(&self, _view: &ProbeItem, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, 48.0)
    }
    fn dispose(&mut self) {
        self.log.lock().push((self.index, "dispose"));
    }
}

/// A `StatefulView` item — a composite top-level child — mounts (`init_state`)
/// when its index enters the band and is disposed exactly once when the band
/// moves away. Every disposed index was initialised first, and no index is
/// initialised twice while resident. (Same-frame timing is pinned by the test
/// above; this one pins the lifecycle pairing.)
#[test]
fn lazy_list_view_builder_stateful_items_init_and_dispose_with_the_band() {
    const ITEM_COUNT: usize = 100;
    const ITEM_EXTENT: f32 = 48.0;
    let log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let list = {
        let log = Arc::clone(&log);
        move |offset: f32| {
            let log = Arc::clone(&log);
            ListView::builder(ITEM_COUNT, ITEM_EXTENT, move |i| {
                (i < ITEM_COUNT).then(|| {
                    ProbeItem {
                        index: i,
                        log: Arc::clone(&log),
                    }
                    .boxed()
                })
            })
            .repaint_boundaries(false)
            .offset(offset)
        }
    };
    let mut laid = lay_out(list(0.0), tight(200.0, 200.0));

    let inits_at_start: Vec<usize> = log
        .lock()
        .iter()
        .filter(|(_, what)| *what == "init")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        inits_at_start.contains(&0) && inits_at_start.contains(&4),
        "visible items must have initialised state in the bootstrap frame; got {inits_at_start:?}"
    );
    assert!(
        !inits_at_start.contains(&50),
        "an off-band item must not have been created; got {inits_at_start:?}"
    );
    assert!(
        log.lock().iter().all(|(_, what)| *what != "dispose"),
        "nothing is disposed before the band moves"
    );

    laid.pump_widget(list(50.0 * ITEM_EXTENT));
    let entries = log.lock().clone();
    let disposed: Vec<usize> = entries
        .iter()
        .filter(|(_, what)| *what == "dispose")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        disposed.contains(&0) && disposed.contains(&4),
        "items that left the band must be disposed in the frame that moved it; got {disposed:?}"
    );
    let inits_after: Vec<usize> = entries
        .iter()
        .filter(|(_, what)| *what == "init")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        inits_after.contains(&50) && inits_after.contains(&54),
        "items of the new band must initialise in that same frame; got {inits_after:?}"
    );
    for index in &disposed {
        assert!(
            inits_after.contains(index),
            "index {index} was disposed without ever being initialised"
        );
    }
    // Resident set discipline: no index is initialised twice without a
    // dispose in between.
    let mut live = std::collections::HashSet::new();
    for (i, what) in &entries {
        match *what {
            "init" => assert!(
                live.insert(*i),
                "index {i} initialised twice while resident"
            ),
            "dispose" => assert!(live.remove(i), "index {i} disposed while not resident"),
            _ => unreachable!(),
        }
    }
}

// ============================================================================
// Estimate adaptation — a wrong seed estimate must not cost a frame per band
// generation, nor trip the frame's pass bound
// ============================================================================

/// The geometry of the tree's single `RenderSliverList`.
fn sliver_list_geometry(laid: &LaidOut) -> flui_rendering::constraints::SliverGeometry {
    let slivers = laid.find_all_by_render_type("RenderSliverList");
    assert_eq!(slivers.len(), 1, "expected exactly one RenderSliverList");
    laid.sliver_geometry(slivers[0])
}

/// A 20× over-estimate (200 px seed, 10 px items) once converged geometrically:
/// each pass requested only the handful of items the stale hint said still
/// fit, so a 600 px viewport took a dozen generations — one frame each on the
/// old post-paint service path, a `BUG:` overrun once servicing moved inside
/// the bounded fixpoint. The hint now follows the running mean of measured
/// extents, so the bootstrap frame alone lays out the whole visible band.
#[test]
fn lazy_list_view_builder_overestimated_extent_settles_in_the_bootstrap_frame() {
    const ITEM_COUNT: usize = 1000;
    const SEED_ESTIMATE: f32 = 200.0;
    const ACTUAL: f32 = 10.0;
    const VIEWPORT_HEIGHT: f32 = 600.0;
    let laid = lay_out(
        ListView::builder(ITEM_COUNT, SEED_ESTIMATE, |i| {
            (i < ITEM_COUNT).then(|| {
                SizedBox::new(200.0, ACTUAL)
                    .child(Text::new(format!("item{i}")))
                    .boxed()
            })
        })
        .repaint_boundaries(false),
        tight(200.0, VIEWPORT_HEIGHT),
    );
    // Every item that intersects the viewport is built AND laid out — not
    // just the six the seed estimate would have requested first.
    let visible_items = (VIEWPORT_HEIGHT / ACTUAL) as usize;
    for i in [0, visible_items / 2, visible_items - 1] {
        let text = format!("item{i}");
        let height = laid.find_text(&text).map(|id| laid.size(id).height.get());
        assert!(
            height.is_some_and(|h| h > 0.0),
            "{text} must be laid out by the bootstrap frame under a 20× over-estimate; height={height:?}"
        );
    }
    let geometry = sliver_list_geometry(&laid);
    assert!(
        geometry.paint_extent >= VIEWPORT_HEIGHT - 1.0,
        "the sliver must fill the viewport once its band settled; paint_extent={}",
        geometry.paint_extent
    );
}

/// Content whose measured extents keep falling faster than the mean can
/// follow — every item past the entry point is half the height of the one
/// before — cannot settle inside the frame's lazy-band budget. That is the
/// deferral path, and it must stay a deferral: no `BUG:` panic in a debug
/// build, and the band completes over the following frames exactly as the
/// old post-paint service path would have.
#[test]
fn lazy_list_view_builder_pathological_extents_defer_instead_of_panicking() {
    const ITEM_COUNT: usize = 400;
    const ENTRY: usize = 25;
    const SEED: f32 = 200.0;
    let height_of = |i: usize| -> f32 {
        if i < ENTRY {
            SEED
        } else {
            (SEED / 2f32.powi((i - ENTRY) as i32 + 1)).max(0.25)
        }
    };
    let mut laid = lay_out(
        ListView::builder(ITEM_COUNT, SEED, move |i| {
            (i < ITEM_COUNT).then(|| SizedBox::new(200.0, height_of(i)).boxed())
        })
        .offset(ENTRY as f32 * SEED),
        tight(200.0, 600.0),
    );
    // Whatever the first frame managed, the following frames finish the band.
    for _ in 0..12 {
        laid.tick();
    }
    // Past the entry point the remaining 375 items sum to under 200 px, so a
    // settled band reaches the list's end: every one of them is resident
    // (one boundary + one box each, plus the viewport and the sliver). The
    // 25 items above the entry are never measured — they stay hinted, as in
    // Flutter — so the total extent is deliberately not asserted here.
    let resident_items = laid.render_node_count().saturating_sub(2) / 2;
    assert!(
        resident_items >= ITEM_COUNT - ENTRY,
        "the band must reach the list's end over a few frames; resident items={resident_items}"
    );
    let geometry = sliver_list_geometry(&laid);
    assert!(
        geometry.scroll_extent.is_finite() && geometry.scroll_extent > 0.0,
        "geometry must stay sane while the band settles; {geometry:?}"
    );
}

/// The lazy-band pass budget is a deferral, never a panic. With the budget
/// forced to a single pass, the frame that mounts a 20× over-estimated list
/// services exactly one band generation inside its fixpoint; the widened
/// band is picked up by the frame's post-paint safety net — built, but not
/// laid out until the next frame — and that next frame completes it. The
/// default budget settles the same scene in one frame (the test above), so
/// the knob is what makes this path observable.
#[test]
fn lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame() {
    const ITEM_COUNT: usize = 1000;
    const SEED_ESTIMATE: f32 = 200.0;
    const ACTUAL: f32 = 10.0;
    let list = || {
        ListView::builder(ITEM_COUNT, SEED_ESTIMATE, |i| {
            (i < ITEM_COUNT).then(|| {
                SizedBox::new(200.0, ACTUAL)
                    .child(Text::new(format!("item{i}")))
                    .boxed()
            })
        })
        .repaint_boundaries(false)
    };
    let mut laid = lay_out(SizedBox::square(10.0), tight(200.0, 600.0));
    laid.build_owner_mut().set_lazy_band_pass_budget_for_test(1);

    laid.pump_widget(list());
    // `try_size`: a deferred item exists in the render tree (the safety net
    // built it) but has no geometry until the next frame lays it out.
    let laid_out = |laid: &LaidOut, i: usize| -> Option<f32> {
        laid.find_text(&format!("item{i}"))
            .and_then(|id| laid.try_size(id))
            .map(|size| size.height.get())
    };
    assert!(
        laid_out(&laid, 0).is_some_and(|h| h > 0.0),
        "the first band generation is serviced and laid out within the budget"
    );
    let deferred = laid_out(&laid, 30);
    assert!(
        deferred.is_none_or(|h| h == 0.0),
        "an item the widened band requested past the budget must not be laid out \
         this frame (deferred, not panicked); item30 height={deferred:?}"
    );

    laid.tick();
    assert!(
        laid_out(&laid, 30).is_some_and(|h| h > 0.0),
        "the deferred generation completes on the next frame"
    );
}

/// A frame whose lazy budget trips evicts before it paints.
///
/// The band is settled at the head, then the pass budget is forced to zero
/// and the list jumps 500 rows down. That frame cannot build the new band
/// (the budget is gone) — the deferral path — but the residents of the
/// head band are outside the band the jump's layout retained, so they must
/// be gone before the frame paints: the band walk positions in-band
/// children only, and a stale resident would be painted at whatever offset
/// it last had. The oracle is the frame's own display lists: no head row's
/// colour is drawn after the jump; the deferred band is drawn on the next
/// frame. Red without the evict-before-paint step of the fixpoint.
#[test]
fn lazy_list_view_builder_exhausted_budget_evicts_stale_residents_before_paint() {
    const ITEM_COUNT: usize = 1000;
    const EXTENT: f32 = 10.0;
    const JUMP_ROW: usize = 500;
    fn row_color(i: usize) -> Color {
        Color::rgb((i % 256) as u8, ((i / 256) % 256) as u8, 7)
    }
    let list = |offset: f32| {
        ListView::builder(ITEM_COUNT, EXTENT, |i| {
            (i < ITEM_COUNT).then(|| {
                SizedBox::new(200.0, EXTENT)
                    .child(ColoredBox::new(row_color(i)))
                    .boxed()
            })
        })
        .repaint_boundaries(false)
        .offset(offset)
    };
    let mut laid = lay_out(list(0.0), tight(200.0, 200.0));
    laid.pump();
    let head = painted_rect_colors(&laid);
    assert!(
        head.contains(&row_color(0)) && head.contains(&row_color(10)),
        "the head band is painted before the jump"
    );

    laid.build_owner_mut().set_lazy_band_pass_budget_for_test(0);
    laid.pump_widget(list(JUMP_ROW as f32 * EXTENT));
    let after_jump = painted_rect_colors(&laid);
    let stale: Vec<usize> = (0..JUMP_ROW)
        .filter(|&i| after_jump.contains(&row_color(i)))
        .collect();
    assert!(
        stale.is_empty(),
        "rows the jump's band no longer covers must not be painted in the frame \
         that deferred the new band; painted head rows: {stale:?}"
    );

    laid.tick();
    let next = painted_rect_colors(&laid);
    assert!(
        next.contains(&row_color(JUMP_ROW)),
        "the deferred band is built after the frame and painted on the next one"
    );
}

/// Every `DrawRect` colour in the most recent frame's composited layer tree,
/// in paint order.
fn painted_rect_colors(laid: &LaidOut) -> Vec<Color> {
    use flui_painting::DrawCommand;
    use flui_rendering::layer::Layer;
    let mut colors = Vec::new();
    let Some(tree) = laid.layer_tree() else {
        return colors;
    };
    let Some(root) = tree.root() else {
        return colors;
    };
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(layer) = tree.get_layer(id) else {
            continue;
        };
        let commands = match layer {
            Layer::Picture(picture) => Some(picture.picture()),
            Layer::Canvas(canvas) => Some(canvas.display_list()),
            _ => None,
        };
        if let Some(commands) = commands {
            colors.extend(commands.iter().filter_map(|command| match command {
                DrawCommand::DrawRect { paint, .. } => Some(paint.color),
                _ => None,
            }));
        }
        if let Some(children) = tree.children(id) {
            stack.extend(children.iter().rev().copied());
        }
    }
    colors
}

/// `ListView::new` over a thousand static children materialises only its
/// window: the children are a delegate served by index, not dense children.
/// Flutter's `SliverChildListDelegate` gives `ListView(children:)` the same
/// bound; before this the fixed-extent list attached every child eagerly.
#[test]
fn list_view_new_materialises_only_the_window() {
    const ITEM_COUNT: usize = 1000;
    const EXTENT: f32 = 10.0;
    let children: Vec<BoxedView> = (0..ITEM_COUNT)
        .map(|i| {
            SizedBox::new(200.0, EXTENT)
                .child(Text::new(format!("row{i}")))
                .boxed()
        })
        .collect();
    let laid = lay_out(
        ListView::new(EXTENT, children).repaint_boundaries(false),
        tight(200.0, 200.0),
    );
    // 200 px viewport + 250 px cache below at 10 px rows: 45 rows, and a few
    // render nodes each; a thousand rows would be thousands of nodes.
    let nodes = laid.render_node_count();
    assert!(
        nodes < 400,
        "only the window is built: {nodes} render nodes for {ITEM_COUNT} rows"
    );
    assert!(laid.find_text("row0").is_some(), "the head row is resident");
    assert!(
        laid.find_text("row500").is_none(),
        "a row far below the window is never built"
    );
}

/// The static delegate's key map: a keyed row of `ListView::new` whose data
/// moved out of the resident band while the viewport jumped to its new
/// place keeps its state — the same contract `ListView::builder` has with a
/// `find_index_by_key` callback, here with no callback at all (Flutter's
/// `SliverChildListDelegate` derives `findIndexByKey` from its children).
#[test]
fn list_view_new_keyed_row_moving_with_the_viewport_keeps_state() {
    const EXTENT: f32 = 10.0;
    let inits = Arc::new(parking_lot::Mutex::new(Vec::<u32>::new()));
    let rows = |order: &[u32]| -> Vec<BoxedView> {
        order
            .iter()
            .map(|&id| {
                KeyedRow {
                    id,
                    key: flui_foundation::ValueKey::new(id),
                    inits: Arc::clone(&inits),
                }
                .boxed()
            })
            .collect()
    };
    let initial: Vec<u32> = (1..=100).collect();
    let mut laid = lay_out(
        ListView::new(EXTENT, rows(&initial)).repaint_boundaries(false),
        tight(200.0, 200.0),
    );
    assert!(
        laid.find_text("row1").is_some(),
        "row 1 is resident at the head"
    );

    // Row 1 moves to index 60 and the viewport jumps there in the same frame.
    let mut moved: Vec<u32> = (2..=100).collect();
    moved.insert(59, 1);
    laid.pump_widget(
        ListView::new(EXTENT, rows(&moved))
            .repaint_boundaries(false)
            .offset(58.0 * EXTENT),
    );
    let row1 = laid
        .find_text("row1")
        .expect("row 1 is resident at its new index");
    let top = laid.absolute_offset(row1).dy.get();
    assert!(
        (0.0..200.0).contains(&top),
        "row 1 is on screen at its new index; top={top}"
    );
    let states_for_1 = inits.lock().iter().filter(|&&id| id == 1).count();
    assert_eq!(
        states_for_1, 1,
        "row 1's state must be created once and carried to its new index"
    );
}

/// The reviewer's scenario for the in-frame fixpoint: a 1000 px seed over
/// 1 px items in a 200 px viewport. The first pass requests one item; that
/// single measurement must be enough for the same frame to request, build,
/// and lay out the whole viewport — the loop only re-runs when a manager did
/// work, so the band the measurement moved has to be requested in the pass
/// that measured it, not discovered by the next frame.
#[test]
fn lazy_list_view_builder_thousandfold_overestimate_fills_the_viewport_in_the_bootstrap_frame() {
    const ITEM_COUNT: usize = 100_000;
    const SEED_ESTIMATE: f32 = 1000.0;
    const ACTUAL: f32 = 1.0;
    const VIEWPORT_HEIGHT: f32 = 200.0;
    let laid = lay_out(
        ListView::builder(ITEM_COUNT, SEED_ESTIMATE, |i| {
            (i < ITEM_COUNT).then(|| SizedBox::new(200.0, ACTUAL).boxed())
        })
        .repaint_boundaries(false),
        tight(200.0, VIEWPORT_HEIGHT),
    );
    let geometry = sliver_list_geometry(&laid);
    assert!(
        geometry.paint_extent >= VIEWPORT_HEIGHT - 1.0,
        "the viewport must be filled by the bootstrap frame; paint_extent={}",
        geometry.paint_extent
    );
    let resident_items = laid.render_node_count().saturating_sub(2);
    assert!(
        resident_items >= VIEWPORT_HEIGHT as usize,
        "every 1 px item across the 200 px viewport must be resident; got {resident_items}"
    );
}

// ============================================================================
// GlobalKey'd items — the per-item repaint boundary must not claim the key
// ============================================================================

#[derive(Clone)]
struct GlobalKeyedItem {
    key: flui_view::GlobalKey<GlobalKeyedItemState>,
    height: f32,
}

struct GlobalKeyedItemState {
    height: f32,
}

impl StatefulView for GlobalKeyedItem {
    type State = GlobalKeyedItemState;
    fn create_state(&self) -> GlobalKeyedItemState {
        GlobalKeyedItemState {
            height: self.height,
        }
    }
}

impl ViewState<GlobalKeyedItem> for GlobalKeyedItemState {
    fn build(&self, _view: &GlobalKeyedItem, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, self.height)
    }
}

impl flui_view::View for GlobalKeyedItem {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

/// An item carrying a `GlobalKey` mounts under the default per-item
/// `RepaintBoundary`. The boundary forwards the item's key so the sliver can
/// reconcile by it, but it must forward a *salted* key (Flutter's
/// `_SaltedValueKey`): forwarding the raw `GlobalKey` made the boundary and
/// the item both register it — a debug panic in
/// `register_global_key_with_collision_check` on the item's own mount, and a
/// duplicate report in release — so no lazy item could carry a `GlobalKey`
/// at all.
#[test]
fn lazy_list_view_builder_mounts_a_global_keyed_item_under_the_repaint_boundary() {
    let keys: Vec<flui_view::GlobalKey<GlobalKeyedItemState>> =
        (0..3).map(|_| flui_view::GlobalKey::new()).collect();
    let laid = lay_out(
        ListView::builder(3, 48.0, move |i| {
            (i < 3).then(|| {
                GlobalKeyedItem {
                    key: keys[i].clone(),
                    height: 48.0,
                }
                .boxed()
            })
        }),
        tight(200.0, 200.0),
    );
    // viewport + sliver + 3 × (boundary + item box)
    assert_eq!(laid.render_node_count(), 2 + 3 * 2);
}

// ============================================================================
// Keyed identity — insert, reorder, duplicate keys, and a GlobalKey graft
// ============================================================================

/// A keyed item whose state records which data id it was created for and
/// how many times `init_state` ran across the whole test.
#[derive(Clone)]
struct KeyedRow {
    id: u32,
    key: flui_foundation::ValueKey<u32>,
    inits: Arc<parking_lot::Mutex<Vec<u32>>>,
}

struct KeyedRowState {
    born_as: u32,
    log: Arc<parking_lot::Mutex<Vec<u32>>>,
}

impl StatefulView for KeyedRow {
    type State = KeyedRowState;
    fn create_state(&self) -> KeyedRowState {
        KeyedRowState {
            born_as: self.id,
            log: Arc::clone(&self.inits),
        }
    }
}

impl ViewState<KeyedRow> for KeyedRowState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        // One entry per STATE created: a preserved element never adds a
        // second entry for its id, a remounted one does.
        self.log.lock().push(self.born_as);
    }
    fn build(&self, _view: &KeyedRow, _ctx: &dyn BuildContext) -> impl IntoView {
        // The paragraph carries the STATE's id (what the element was born as),
        // so a remounted element reads differently from a preserved one.
        SizedBox::new(200.0, 48.0).child(Text::new(format!("row{}", self.born_as)))
    }
}

impl flui_view::View for KeyedRow {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

type Data = Arc<parking_lot::Mutex<Vec<u32>>>;

fn keyed_list(
    data: &Data,
    inits: &Arc<parking_lot::Mutex<Vec<u32>>>,
    with_callback: bool,
) -> ListView {
    let snapshot: Vec<u32> = data.lock().clone();
    let count = snapshot.len();
    let inits = Arc::clone(inits);
    let rows = snapshot.clone();
    let list = ListView::builder(count, 48.0, move |i| {
        rows.get(i).map(|&id| {
            KeyedRow {
                id,
                key: flui_foundation::ValueKey::new(id),
                inits: Arc::clone(&inits),
            }
            .boxed()
        })
    });
    if with_callback {
        let rows = snapshot;
        list.find_index_by_key(move |key| {
            key.as_any()
                .downcast_ref::<flui_foundation::ValueKey<u32>>()
                .and_then(|k| rows.iter().position(|id| id == k.value()))
        })
    } else {
        list
    }
}

/// The id every on-stage row's STATE was born as, in list order.
fn born_ids(laid: &LaidOut, ids: &[u32]) -> Vec<u32> {
    let mut found: Vec<(f32, u32)> = ids
        .iter()
        .filter_map(|&id| {
            laid.find_text(&format!("row{id}"))
                .map(|node| (laid.absolute_offset(node).dy.get(), id))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, id)| id).collect()
}

/// Insert at the head with `find_index_by_key`: every resident keyed row
/// keeps its element (its state still reports the id it was born as), only
/// the new row is created. Flutter needs `findChildIndexCallback` for the
/// same guarantee; FLUI's callback only *widens* the set of indices to
/// rebuild — the match itself is by key, so the rows that stayed in the
/// band would have been preserved even without it (the next test).
#[test]
fn lazy_list_view_builder_keyed_insert_at_head_preserves_resident_state() {
    let data: Data = Arc::new(parking_lot::Mutex::new((10..16).collect()));
    let inits = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut laid = lay_out(keyed_list(&data, &inits, true), tight(200.0, 200.0));
    assert_eq!(born_ids(&laid, &[10, 11, 12, 13]), vec![10, 11, 12, 13]);
    let nodes_before = laid.render_node_count();

    data.lock().insert(0, 99);
    laid.pump_widget(keyed_list(&data, &inits, true));
    assert_eq!(
        born_ids(&laid, &[99, 10, 11, 12]),
        vec![99, 10, 11, 12],
        "rows shift down by one and keep the state they were born with"
    );
    // No row was remounted: every resident element is still the one it was
    // born as (asserted above), and there is exactly one element per data
    // row that is resident — no leaked duplicates from a remount.
    let resident_rows = laid.count_elements_by_view_type::<KeyedRow>();
    assert!(
        resident_rows <= data.lock().len(),
        "an insert must not leave duplicate row elements behind; got {resident_rows}"
    );
    let _ = nodes_before;
}

/// A keyed row whose data moves far away while the viewport jumps to its
/// new place in the same frame keeps its state: the reconcile relocates it
/// before the band eviction judges it by its NEW index. Evicting first
/// destroyed it and mounted a fresh row at the destination.
#[test]
fn lazy_list_view_builder_keyed_row_moving_with_the_viewport_keeps_state() {
    const EXTENT: f32 = 48.0;
    let data: Data = Arc::new(parking_lot::Mutex::new((0..100).collect()));
    let inits = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut laid = lay_out(keyed_list(&data, &inits, true), tight(200.0, 200.0));
    assert_eq!(born_ids(&laid, &[0, 1, 2]), vec![0, 1, 2]);

    // Row 1 moves to index 60; the viewport jumps there in the same frame.
    {
        let mut d = data.lock();
        let row = d.remove(1);
        d.insert(60, row);
    }
    laid.pump_widget(keyed_list(&data, &inits, true).offset(58.0 * EXTENT));
    let top = laid
        .absolute_offset(
            laid.find_text("row1")
                .expect("row 1 must be resident at its new index"),
        )
        .dy
        .get();
    assert!(
        (0.0..200.0).contains(&top),
        "row 1 is on screen at its new index; top={top}"
    );
    // Its state is the one it was born with: the element moved, it was not
    // remounted — a remount would read `row1` too (the id is the data), so
    // the oracle is the state log: exactly one state ever created for id 1.
    let states_for_1 = inits.lock().iter().filter(|&&id| id == 1).count();
    assert_eq!(
        states_for_1, 1,
        "row 1's state must be created once and carried to its new index"
    );
}

/// A swap inside the band without any callback: the two rows keep their
/// elements (states born as their ids) and paint at each other's former
/// offsets, and the render parent-data stamps follow — the sliver lays the
/// list out with no duplicate-index assertion and evicts cleanly afterwards.
#[test]
fn lazy_list_view_builder_keyed_swap_within_the_band_preserves_state_without_a_callback() {
    let data: Data = Arc::new(parking_lot::Mutex::new((10..16).collect()));
    let inits = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut laid = lay_out(keyed_list(&data, &inits, false), tight(200.0, 200.0));
    let top_of = |laid: &LaidOut, id: u32| {
        laid.absolute_offset(laid.find_text(&format!("row{id}")).expect("resident"))
            .dy
            .get()
    };
    let (y11, y13) = (top_of(&laid, 11), top_of(&laid, 13));

    data.lock().swap(1, 3);
    laid.pump_widget(keyed_list(&data, &inits, false));
    assert_eq!(born_ids(&laid, &[10, 13, 12, 11]), vec![10, 13, 12, 11]);
    assert_eq!(top_of(&laid, 13), y11, "row 13 now paints where row 11 was");
    assert_eq!(top_of(&laid, 11), y13, "row 11 now paints where row 13 was");
    // A `set_state` inside a moved row replaces its render child: the fresh
    // node must be stamped with the NEW index, or it paints at the old one.
    laid.tick();
    assert_eq!(top_of(&laid, 13), y11);

    // Everything still evicts: jump far away and back.
    laid.pump_widget(keyed_list(&data, &inits, false).offset(48.0 * 100.0));
    laid.pump_widget(keyed_list(&data, &inits, false));
    assert_eq!(born_ids(&laid, &[10, 13, 12, 11]).len(), 4);
}

/// Two items answering to the same local key in one band: the first claims
/// the resident, the second is mounted fresh — no panic, no teleport.
#[test]
fn lazy_list_view_builder_duplicate_local_keys_first_wins_second_remounts() {
    let inits = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let make = {
        let inits = Arc::clone(&inits);
        move || {
            let inits = Arc::clone(&inits);
            ListView::builder(4, 48.0, move |i| {
                (i < 4).then(|| {
                    KeyedRow {
                        id: i as u32,
                        // indices 1 and 2 share a key
                        key: flui_foundation::ValueKey::new(if i == 2 { 1 } else { i as u32 }),
                        inits: Arc::clone(&inits),
                    }
                    .boxed()
                })
            })
        }
    };
    let mut laid = lay_out(make(), tight(200.0, 200.0));
    assert_eq!(born_ids(&laid, &[0, 1, 2, 3]), vec![0, 1, 2, 3]);
    laid.pump_widget(make());
    assert_eq!(
        born_ids(&laid, &[0, 1, 2, 3]),
        vec![0, 1, 2, 3],
        "a duplicate key remounts the second item in place instead of moving the first"
    );
}

/// A `GlobalKey`'d item that one lazy list stops building and another starts
/// building in the same frame moves between them — the second list's mount
/// retakes it (from the first list's still-active resident, or from the
/// inactive queue if the first list already let it go) — and the first list
/// drops its own bookkeeping entry for it (Flutter's `forgetChild` — the
/// list forgets the child, the child keeps its state), so the first list's
/// later band eviction never reaches into the other list's subtree. Observable: exactly one element,
/// its state intact, and both lists still evicting cleanly afterwards.
///
/// The lists keep the per-item repaint boundary — the default, and what an
/// app would actually write — so the resident root is the boundary (carrying
/// a salted, non-global key) and the keyed item is a *descendant* of the
/// subtree an eviction removes. Reaching it therefore depends on the removal
/// walk stopping at a `GlobalKey`'d descendant and deactivating it rather
/// than unmounting the subtree wholesale, which is the behaviour this file's
/// fixture exists to pin.
#[derive(Clone)]
struct TwoLists {
    keyed_in_second: Arc<AtomicBool>,
    key: flui_view::GlobalKey<CountingItemState>,
    log: Arc<parking_lot::Mutex<Vec<&'static str>>>,
}

struct TwoListsState {
    keyed_in_second: Arc<AtomicBool>,
    key: flui_view::GlobalKey<CountingItemState>,
    log: Arc<parking_lot::Mutex<Vec<&'static str>>>,
}

#[derive(Clone)]
struct CountingItem {
    key: flui_view::GlobalKey<CountingItemState>,
    log: Arc<parking_lot::Mutex<Vec<&'static str>>>,
}

struct CountingItemState {
    log: Arc<parking_lot::Mutex<Vec<&'static str>>>,
}

impl StatefulView for CountingItem {
    type State = CountingItemState;
    fn create_state(&self) -> CountingItemState {
        CountingItemState {
            log: Arc::clone(&self.log),
        }
    }
}

impl ViewState<CountingItem> for CountingItemState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        self.log.lock().push("init");
    }
    fn build(&self, _view: &CountingItem, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, 48.0).child(Text::new("keyed"))
    }
    fn dispose(&mut self) {
        self.log.lock().push("dispose");
    }
}

impl flui_view::View for CountingItem {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

impl StatefulView for TwoLists {
    type State = TwoListsState;
    fn create_state(&self) -> TwoListsState {
        TwoListsState {
            keyed_in_second: Arc::clone(&self.keyed_in_second),
            key: self.key.clone(),
            log: Arc::clone(&self.log),
        }
    }
}

impl ViewState<TwoLists> for TwoListsState {
    fn build(&self, _view: &TwoLists, _ctx: &dyn BuildContext) -> impl IntoView {
        let in_second = self.keyed_in_second.load(Ordering::SeqCst);
        let list = |holds_keyed: bool,
                    key: flui_view::GlobalKey<CountingItemState>,
                    log: Arc<parking_lot::Mutex<Vec<&'static str>>>| {
            ListView::builder(3, 48.0, move |i| {
                (i < 3).then(|| {
                    if i == 1 && holds_keyed {
                        CountingItem {
                            key: key.clone(),
                            log: Arc::clone(&log),
                        }
                        .boxed()
                    } else {
                        SizedBox::new(200.0, 48.0).boxed()
                    }
                })
            })
        };
        Column::new((
            SizedBox::new(200.0, 100.0).child(list(
                !in_second,
                self.key.clone(),
                Arc::clone(&self.log),
            )),
            SizedBox::new(200.0, 100.0).child(list(
                in_second,
                self.key.clone(),
                Arc::clone(&self.log),
            )),
        ))
    }
}

impl flui_view::View for TwoLists {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// The default per-item `RepaintBoundary` stays ON here, and that is the
/// point: it used to have to be turned off.
///
/// The boundary is an UNKEYED wrapper, and `remove_subtree` hard-removed every
/// descendant of one — including a `GlobalKey`'d element, which lost its
/// identity the moment the boundary was evicted. Issue #838. With the keyed
/// descendant deactivated instead, a lazy item can carry a `GlobalKey` under
/// the default configuration, which is what an app would actually write.
#[test]
fn lazy_list_view_builder_preserves_a_global_keyed_item_grafted_to_another_list() {
    let keyed_in_second = Arc::new(AtomicBool::new(false));
    let log: Arc<parking_lot::Mutex<Vec<&'static str>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut laid = lay_out(
        TwoLists {
            keyed_in_second: Arc::clone(&keyed_in_second),
            key: flui_view::GlobalKey::new(),
            log: Arc::clone(&log),
        },
        tight(200.0, 200.0),
    );
    assert_eq!(laid.count_elements_by_view_type::<CountingItem>(), 1);
    assert_eq!(log.lock().as_slice(), ["init"]);
    let first_top = laid
        .absolute_offset(laid.find_text("keyed").expect("keyed item"))
        .dy
        .get();

    // Move the keyed item from the first list to the second.
    keyed_in_second.store(true, Ordering::SeqCst);
    laid.pump();
    laid.tick();
    assert_eq!(
        laid.count_elements_by_view_type::<CountingItem>(),
        1,
        "one element: grafted, not duplicated"
    );
    assert_eq!(
        log.lock().as_slice(),
        ["init"],
        "the element moved with its state: no dispose, no second init"
    );
    let second_top = laid
        .absolute_offset(laid.find_text("keyed").expect("keyed item"))
        .dy
        .get();
    assert!(
        second_top > first_top,
        "the item now sits in the second list"
    );

    // Both lists still own only what they built: a later frame that
    // rebuilds and evicts must not reach the grafted element from the first
    // list's stale bookkeeping.
    laid.pump();
    laid.tick();
    assert_eq!(laid.count_elements_by_view_type::<CountingItem>(), 1);
    assert_eq!(log.lock().as_slice(), ["init"]);
}

// ---------------------------------------------------------------------------
// Keep-alive (#835)
// ---------------------------------------------------------------------------

/// An item that takes a keep-alive lease in `init_state` when `keep` is set.
///
/// The lease lives in the state, so it is released exactly when the state is
/// dropped — there is no `dispose` bookkeeping to forget.
#[derive(Clone, StatefulView)]
struct KeptItem {
    index: usize,
    keep: bool,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
}

struct KeptItemState {
    index: usize,
    keep: bool,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
    lease: Option<flui_view::owner::KeepAliveLease>,
}

impl StatefulView for KeptItem {
    type State = KeptItemState;
    fn create_state(&self) -> KeptItemState {
        KeptItemState {
            index: self.index,
            keep: self.keep,
            log: Arc::clone(&self.log),
            lease: None,
        }
    }
}

impl ViewState<KeptItem> for KeptItemState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.log.lock().push((self.index, "init"));
        if self.keep {
            self.lease = ctx.keep_alive_lease();
            self.log.lock().push((
                self.index,
                if self.lease.is_some() {
                    "leased"
                } else {
                    "no-lease"
                },
            ));
        }
    }
    fn build(&self, _view: &KeptItem, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, 48.0)
    }
    fn dispose(&mut self) {
        self.log.lock().push((self.index, "dispose"));
    }
}

/// A held item survives the band moving away; an unheld one does not.
///
/// The control is the point: without an unheld neighbour proving the band
/// really did move and really does evict, "the held item is still alive" would
/// also pass on a build where eviction never ran at all.
#[test]
fn a_kept_alive_item_survives_the_band_moving_away() {
    const ITEM_COUNT: usize = 100;
    const ITEM_EXTENT: f32 = 48.0;
    const KEPT: usize = 1;

    let log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let list = {
        let log = Arc::clone(&log);
        move |offset: f32| {
            let log = Arc::clone(&log);
            ListView::builder(ITEM_COUNT, ITEM_EXTENT, move |i| {
                (i < ITEM_COUNT).then(|| {
                    KeptItem {
                        index: i,
                        keep: i == KEPT,
                        log: Arc::clone(&log),
                    }
                    .boxed()
                })
            })
            .repaint_boundaries(false)
            .offset(offset)
        }
    };

    let mut laid = lay_out(list(0.0), tight(200.0, 200.0));
    let inits: Vec<usize> = log
        .lock()
        .iter()
        .filter(|(_, what)| *what == "init")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        inits.contains(&KEPT) && inits.contains(&0),
        "both the kept item and its control must start resident; got {inits:?}"
    );
    let lease_state: Vec<&str> = log
        .lock()
        .iter()
        .filter(|(_, what)| *what == "leased" || *what == "no-lease")
        .map(|(_, what)| *what)
        .collect();
    assert_eq!(lease_state, vec!["leased"], "the lease must be acquired");

    // Scroll far past both.
    laid.pump_widget(list(50.0 * ITEM_EXTENT));

    let disposed: Vec<usize> = log
        .lock()
        .iter()
        .filter(|(_, what)| *what == "dispose")
        .map(|(i, _)| *i)
        .collect();

    // The control proves the band moved and eviction ran.
    assert!(
        disposed.contains(&0),
        "the unheld control must be evicted when the band leaves it; got {disposed:?}"
    );
    // And the held item rode it out.
    assert!(
        !disposed.contains(&KEPT),
        "the held item must survive the band moving away; got {disposed:?}"
    );
}

/// Releasing the lease lets the next band eviction take the item.
///
/// This is the half a `#[must_use]` guard cannot enforce: that a *released*
/// hold actually stops holding. Without it, a lease that never released would
/// pass the test above forever.
#[test]
fn releasing_the_lease_lets_the_item_be_evicted() {
    const ITEM_COUNT: usize = 100;
    const ITEM_EXTENT: f32 = 48.0;
    const KEPT: usize = 1;

    let log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    // Flipped between frames: the item re-reads it on rebuild and drops the
    // lease when it goes false.
    let keeping = Arc::new(std::sync::atomic::AtomicBool::new(true));

    let list = {
        let log = Arc::clone(&log);
        let keeping = Arc::clone(&keeping);
        move |offset: f32| {
            let log = Arc::clone(&log);
            let keeping = Arc::clone(&keeping);
            ListView::builder(ITEM_COUNT, ITEM_EXTENT, move |i| {
                (i < ITEM_COUNT).then(|| {
                    ReleasableItem {
                        index: i,
                        keep: i == KEPT && keeping.load(std::sync::atomic::Ordering::SeqCst),
                        log: Arc::clone(&log),
                    }
                    .boxed()
                })
            })
            .repaint_boundaries(false)
            .offset(offset)
        }
    };

    let mut laid = lay_out(list(0.0), tight(200.0, 200.0));
    laid.pump_widget(list(50.0 * ITEM_EXTENT));
    assert!(
        !log.lock()
            .iter()
            .any(|(i, what)| *i == KEPT && *what == "dispose"),
        "held item survives the first scroll"
    );

    // Release, then scroll again.
    keeping.store(false, std::sync::atomic::Ordering::SeqCst);
    laid.pump_widget(list(60.0 * ITEM_EXTENT));
    laid.pump_widget(list(70.0 * ITEM_EXTENT));

    let kept_events: Vec<&str> = log
        .lock()
        .iter()
        .filter(|(i, _)| *i == KEPT)
        .map(|(_, what)| *what)
        .collect();
    assert!(
        log.lock()
            .iter()
            .any(|(i, what)| *i == KEPT && *what == "dispose"),
        "a released item must be evicted by the next band move; kept item saw {kept_events:?}"
    );
}

/// Like `KeptItem`, but re-reads `keep` on every update and drops the lease
/// when it clears — the shape a real "keep me while I have unsaved input"
/// item takes.
#[derive(Clone, StatefulView)]
struct ReleasableItem {
    index: usize,
    keep: bool,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
}

struct ReleasableItemState {
    index: usize,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
    lease: Option<flui_view::owner::KeepAliveLease>,
}

impl StatefulView for ReleasableItem {
    type State = ReleasableItemState;
    fn create_state(&self) -> ReleasableItemState {
        ReleasableItemState {
            index: self.index,
            log: Arc::clone(&self.log),
            lease: None,
        }
    }
}

impl ViewState<ReleasableItem> for ReleasableItemState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.log.lock().push((self.index, "init"));
        self.lease = ctx.keep_alive_lease();
    }
    fn did_update_view(&mut self, view: &ReleasableItem, _old: &ReleasableItem) {
        if !view.keep {
            // Dropping the lease is the release.
            self.lease = None;
        }
    }
    fn build(&self, _view: &ReleasableItem, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, 48.0)
    }
    fn dispose(&mut self) {
        self.log.lock().push((self.index, "dispose"));
    }
}
