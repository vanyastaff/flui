//! Integration tests for `GridView::builder` (RenderSliverGridLazy → the
//! lazy-sliver element-wiring backend, U4.3).
//!
//! Mirrors `lazy_list.rs`'s frame-sequence model: `pump_frame` calls
//! `service_child_requests` after `run_frame`, so two `tick` calls settle a
//! visible window — the first dispatches the child-build requests emitted by
//! `RenderSliverGridLazy::perform_layout`, the second lays out the now-built
//! tiles.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_rendering::delegates::{SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount};
use flui_view::ViewExt;
use flui_widgets::prelude::*;

fn two_column_delegate() -> Arc<dyn SliverGridDelegate> {
    Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2))
}

// ============================================================================
// Test 1 — basic settle: only the visible window is built
// ============================================================================

/// A 2-column grid over 4 items whose combined extent (2 rows × 100 px = 200
/// px) fits within a 200 px-tall viewport must have exactly 4 tile render
/// nodes after settling, plus 1 for `RenderViewport` and 1 for
/// `RenderSliverGridLazy`.
#[test]
fn lazy_grid_view_builder_builds_visible_tiles() {
    let mut laid = lay_out(
        GridView::builder(two_column_delegate(), 4, |i| {
            if i < 4 {
                Some(SizedBox::square(100.0).boxed())
            } else {
                None
            }
        }),
        tight(200.0, 200.0),
    );

    // tick1: run_frame requests tiles → service builds them.
    laid.tick();
    // tick2: sliver dirty → laid out with real tiles.
    laid.tick();

    // Expected: 1 (RenderViewport) + 1 (RenderSliverGridLazy) + 4 (tiles) = 6.
    let nodes_after_settle = laid.render_node_count();
    assert_eq!(
        nodes_after_settle, 6,
        "after settle, render tree should have 1 viewport + 1 lazy grid + 4 tiles = 6; \
         got {nodes_after_settle}"
    );
}

// ============================================================================
// Test 2 — oracle 2-D positions
// ============================================================================

/// A 2-column 200 px-wide grid with square 100×100 tiles must place tiles at
/// (0, 0), (100, 0), (0, 100), (100, 100) — the same oracle used by the eager
/// `RenderSliverGrid` harness test, proving the lazy backend reproduces
/// identical 2-D geometry.
///
/// Tiles are located by render type rather than by walking
/// `RenderSliverGridLazy`'s child list: the lazy backend's `ChildManager`
/// attaches each built tile's *parent* link but does not push it onto the
/// sliver's own `children()` array (shared behavior with the `RenderSliverList`
/// lazy backend — confirmed by inspecting both trees' `children()` output),
/// so the offsets are compared as an unordered set instead of by slot index.
#[test]
fn lazy_grid_view_builder_places_tiles_at_oracle_positions() {
    let mut laid = lay_out(
        GridView::builder(two_column_delegate(), 4, |i| {
            if i < 4 {
                Some(SizedBox::square(100.0).boxed())
            } else {
                None
            }
        }),
        tight(200.0, 200.0),
    );

    laid.tick();
    laid.tick();

    let tile_ids = laid.find_all_by_render_type("RenderConstrainedBox");
    assert_eq!(
        tile_ids.len(),
        4,
        "all 4 tiles must be built and attached; got {tile_ids:?}"
    );

    let mut tile_positions: Vec<(f32, f32)> = tile_ids
        .iter()
        .map(|&id| {
            let tile_offset = laid.offset(id);
            (tile_offset.dx.get(), tile_offset.dy.get())
        })
        .collect();
    tile_positions.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mut expected_positions = vec![(0.0, 0.0), (100.0, 0.0), (0.0, 100.0), (100.0, 100.0)];
    expected_positions.sort_by(|a, b| a.partial_cmp(b).unwrap());

    assert_eq!(
        tile_positions, expected_positions,
        "tile offsets must match the 2-column grid oracle \
         (col0/row0, col1/row0, col0/row1, col1/row1)"
    );
}

// ============================================================================
// Test 3 — disposal on scroll: built set shifts, count bounded, ABA-safe
// ============================================================================

/// A large grid where the viewport shows only a couple of rows. After
/// settling, the render tree must contain only the visible + cache-band
/// tiles, not all N — confirming off-band tiles are evicted via the
/// retain-band channel. A post-settle relayout tick must not grow the node
/// count (no leak) and must not panic (an ABA double-remove would surface as
/// a slab-index panic).
#[test]
fn lazy_grid_view_builder_off_band_eviction_bounded() {
    const ITEM_COUNT: usize = 200;
    // 2 columns × 100 px tiles; viewport 200×200 fits exactly 2 rows (4 tiles).
    let mut laid = lay_out(
        GridView::builder(two_column_delegate(), ITEM_COUNT, |i| {
            if i < ITEM_COUNT {
                Some(SizedBox::square(100.0).boxed())
            } else {
                None
            }
        }),
        tight(200.0, 200.0),
    );

    // Settle over multiple ticks (extra passes in case the cache band takes a
    // frame to stabilize).
    for _ in 0..4 {
        laid.tick();
    }

    let nodes_after_settle = laid.render_node_count();

    // Off-band eviction: only the viewport + cache-band tiles should be built,
    // far fewer than ITEM_COUNT.
    assert!(
        nodes_after_settle <= 40,
        "off-band eviction must limit built tiles to the viewport + cache band; \
         got {nodes_after_settle} render nodes for {ITEM_COUNT} items \
         in a 200×200 viewport (expected <=40)"
    );

    // A further relayout tick must not panic (no ABA double-remove) and must
    // not grow the node count.
    laid.tick();
    let nodes_after_relayout = laid.render_node_count();
    assert!(
        nodes_after_relayout <= nodes_after_settle,
        "a post-settle relayout tick must not leak render nodes; \
         count went from {nodes_after_settle} to {nodes_after_relayout}"
    );
}

// ============================================================================
// Test 4 — 1000-item scrolled grid stays bounded
// ============================================================================

/// A 1000-item grid scrolled deep into the list must still build only the
/// visible/cache band. This is the Core.2 1000-item sliver-scroll smoke for
/// the lazy-grid backend: no eager materialization, no unbounded build storm,
/// and positioned children remain in the viewport neighborhood.
#[test]
fn lazy_grid_view_builder_1000_item_scroll_stays_bounded() {
    const ITEM_COUNT: usize = 1000;
    let tiles_built = Arc::new(AtomicUsize::new(0));

    let mut laid = lay_out(
        GridView::builder(two_column_delegate(), ITEM_COUNT, {
            let tiles_built = Arc::clone(&tiles_built);
            move |i| {
                if i < ITEM_COUNT {
                    tiles_built.fetch_add(1, Ordering::Relaxed);
                    Some(SizedBox::square(100.0).boxed())
                } else {
                    None
                }
            }
        })
        .offset(5_000.0),
        tight(200.0, 200.0),
    );

    for _ in 0..4 {
        laid.tick();
    }

    let nodes_after_settle = laid.render_node_count();
    let builds_after_settle = tiles_built.load(Ordering::Relaxed);
    assert!(
        builds_after_settle > 0 && builds_after_settle <= 40,
        "1000-item lazy grid must build only the scrolled viewport/cache band; \
         built {builds_after_settle} tiles"
    );
    assert!(
        nodes_after_settle <= 42,
        "1000-item lazy grid must keep render nodes bounded; got {nodes_after_settle}"
    );

    for tile in laid.find_all_by_render_type("RenderConstrainedBox") {
        let offset = laid.offset(tile);
        assert!(
            (offset.dx.get() == 0.0 || offset.dx.get() == 100.0)
                && offset.dy.get() >= -400.0
                && offset.dy.get() <= 400.0,
            "scrolled grid tile must remain near the viewport/cache window; \
             got offset ({}, {})",
            offset.dx.get(),
            offset.dy.get()
        );
    }
}

// ============================================================================
// Test 5 — quiescence: a third tick builds zero new tiles
// ============================================================================

/// After the grid has settled (two ticks), a third tick must NOT add or
/// remove any render nodes, and the builder closure must NOT be called again.
/// The `Arc<AtomicUsize>` build counter gives a precise quiescence signal.
#[test]
fn lazy_grid_view_builder_third_tick_is_idempotent() {
    let tiles_built = Arc::new(AtomicUsize::new(0));

    let mut laid = lay_out(
        GridView::builder(two_column_delegate(), 4, {
            let tiles_built = Arc::clone(&tiles_built);
            move |i| {
                if i < 4 {
                    tiles_built.fetch_add(1, Ordering::Relaxed);
                    Some(SizedBox::square(100.0).boxed())
                } else {
                    None
                }
            }
        }),
        tight(200.0, 200.0),
    );

    laid.tick(); // tick1: service builds tiles, build counter increments
    laid.tick(); // tick2: sliver lays out built tiles (no new builds needed)

    let nodes_at_settle = laid.render_node_count();
    let builds_at_settle = tiles_built.load(Ordering::Relaxed);

    // tick3: no-op — neither the element tree nor the sliver is dirty after settle.
    laid.tick();

    let nodes_at_third_tick = laid.render_node_count();
    let builds_at_third_tick = tiles_built.load(Ordering::Relaxed);

    assert_eq!(
        nodes_at_settle, nodes_at_third_tick,
        "a third tick must not change the render node count: \
         settled at {nodes_at_settle}, after tick3: {nodes_at_third_tick}"
    );
    assert_eq!(
        builds_at_settle, builds_at_third_tick,
        "a third tick must trigger zero new tile builds (quiescence invariant); \
         settled after {builds_at_settle} builds, \
         tick3 raised the count to {builds_at_third_tick}"
    );
}

// ============================================================================
// Test 6 — None-at-K caps the build count
// ============================================================================

/// When the builder returns `None` for indices ≥ K, the grid must stop
/// building at K tiles even if `item_count` is larger. The stricter bound
/// wins.
#[test]
fn lazy_grid_view_builder_none_at_k_caps_build_count() {
    const K: usize = 3;
    // item_count=50 but builder returns None for i >= K.
    let mut laid = lay_out(
        GridView::builder(two_column_delegate(), 50, |i| {
            if i < K {
                Some(SizedBox::square(100.0).boxed())
            } else {
                None
            }
        }),
        // Viewport tall enough to request many rows if all tiles were present.
        tight(200.0, 1000.0),
    );

    laid.tick();
    laid.tick();

    // Expected: 1 (viewport) + 1 (lazy grid) + K (tiles capped by None-return) = 5.
    let nodes_after_settle = laid.render_node_count();
    let expected = 1 + 1 + K;
    assert_eq!(
        nodes_after_settle, expected,
        "None-at-K must cap build count: expected {expected} nodes, \
         got {nodes_after_settle}"
    );

    let sliver = laid.find_by_render_type("RenderSliverGridLazy");
    let geometry = laid.sliver_geometry(sliver);
    assert_eq!(
        geometry.scroll_extent, 200.0,
        "None-at-K must cap scroll extent to the actual 3-tile grid: \
         2 rows × 100px = 200px; got {}",
        geometry.scroll_extent
    );
    assert_eq!(
        geometry.max_paint_extent, 200.0,
        "None-at-K must cap max paint extent with the same effective child count"
    );
}
