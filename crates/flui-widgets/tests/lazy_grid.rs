//! Integration tests for `GridView::builder` (RenderSliverGrid → the
//! lazy-sliver element-wiring backend).
//!
//! Mirrors `lazy_list.rs`'s frame-sequence model: `pump_frame` calls
//! `service_child_requests` after `run_frame`, so two `tick` calls settle a
//! visible window — the first dispatches the child-build requests emitted by
//! `RenderSliverGrid::perform_layout`, the second lays out the now-built
//! tiles.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, tight};
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
/// `RenderSliverGrid`.
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

    // Expected: 1 (RenderViewport) + 1 (RenderSliverGrid) + 4 (tiles) = 6.
    let nodes_after_settle = laid.render_node_count();
    assert_eq!(
        nodes_after_settle, 10,
        "after settle, render tree should have 1 viewport + 1 lazy grid + 4 \
         repaint boundaries + 4 tiles = 10; got {nodes_after_settle}"
    );
}

// ============================================================================
// Test 2 — oracle 2-D positions
// ============================================================================

/// A 2-column 200 px-wide grid with square 100×100 tiles must place tiles at
/// (0, 0), (100, 0), (0, 100), (100, 100) — the same oracle
/// `crates/flui-objects/tests/render_object_harness.rs`'s
/// `harness_render_sliver_grid_pre_seeded_tiles_lay_out_correctly` pins at the
/// render-object level, proving the delegate-windowed geometry is unchanged
/// when the children arrive through the element tree instead of being
/// pre-seeded directly.
///
/// Tiles are located by render type rather than by walking
/// `RenderSliverGrid`'s child list: the lazy backend's `ChildManager`
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

    // The grid positions the per-item `RenderRepaintBoundary`; the tile inside
    // it sits at (0, 0) relative to that. Reading the leaf's offset would give
    // four zeroes — the same structure Flutter produces, since its delegates
    // wrap children in a boundary by default.
    let tile_ids = laid.find_all_by_render_type("RenderRepaintBoundary");
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
    // `+ 2 * K`, not `+ K`: each item carries its own `RenderRepaintBoundary`,
    // which `SliverChildBuilderDelegate` adds by default exactly as Flutter's
    // does (`widgets/scroll_delegate.dart:560`).
    let expected = 1 + 1 + 2 * K;
    assert_eq!(
        nodes_after_settle, expected,
        "None-at-K must cap build count: expected {expected} nodes, \
         got {nodes_after_settle}"
    );

    let sliver = laid.find_by_render_type("RenderSliverGrid");
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

// ============================================================================
// Test — builder swap refreshes resident tiles end to end (FLUI-added)
// ============================================================================

/// FLUI-added — the Flutter grid corpus has no builder-closure-swap case (it
/// exercises only scroll-driven eviction/rebuild), so this carries no oracle
/// citation. It guards the `SliverAdaptorBehavior<RenderSliverGrid>::on_view_updated` →
/// `needs_resident_refresh` wiring that the two `service`-level unit tests
/// bypass by setting the flag by hand: a `pump_widget` that keeps the grid
/// shape but hands the builder a fresh label set at every index must refresh
/// the already-resident tiles in place. Deleting the `on_view_updated` wiring
/// leaves the pre-swap labels resident and fails this test — the produce half
/// of the fix the unit tests cannot see.
#[test]
fn lazy_grid_view_builder_swap_refreshes_resident_tiles() {
    fn grid(labels: &'static [&'static str]) -> impl View {
        GridView::builder(two_column_delegate(), labels.len(), move |i| {
            labels
                .get(i)
                .map(|label| SizedBox::square(100.0).child(Text::new(*label)).boxed())
        })
    }

    let mut laid = lay_out(grid(&["A0", "A1", "A2", "A3"]), tight(200.0, 200.0));
    laid.tick();
    laid.tick();

    assert!(
        laid.find_text("A0").is_some(),
        "pre-swap tile 0 must show A0"
    );
    assert!(
        laid.find_text("A3").is_some(),
        "pre-swap tile 3 must show A3"
    );

    // Swap the builder closure for the same grid shape (same delegate, same
    // item_count) with a fresh label at every index.
    laid.pump_widget(grid(&["B0", "B1", "B2", "B3"]));
    laid.tick();
    laid.tick();

    assert!(
        laid.find_text("B0").is_some(),
        "resident tile 0 must refresh to B0 after the builder swap"
    );
    assert!(
        laid.find_text("B3").is_some(),
        "resident tile 3 must refresh to B3 after the builder swap"
    );
    assert!(
        laid.find_text("A0").is_none(),
        "stale pre-swap label A0 must be gone after the refresh"
    );
    assert!(
        laid.find_text("A3").is_none(),
        "stale pre-swap label A3 must be gone after the refresh"
    );
}

// ============================================================================
// GridView::count over StaticChildren — mirrors lazy_list.rs's ListView::new
// coverage (ADR-0053: GridView::count|extent route over the same
// request-strategy adaptor as GridView::builder)
// ============================================================================

/// `GridView::count` over a thousand static tiles materialises only its
/// window: the tiles are a delegate served by index, not dense children.
/// Flutter's `SliverChildListDelegate` gives `GridView(children:)` the same
/// bound; before ADR-0053 the eager grid attached every tile densely.
/// Mirrors `lazy_list.rs`'s `list_view_new_materialises_only_the_window`.
#[test]
fn grid_view_count_materialises_only_the_window() {
    const ITEM_COUNT: usize = 1000;
    let children: Vec<BoxedView> = (0..ITEM_COUNT)
        .map(|i| Text::new(format!("tile{i}")).boxed())
        .collect();
    let laid = lay_out(
        GridView::count(2, children).repaint_boundaries(false),
        tight(200.0, 200.0),
    );
    // 200 px viewport + 250 px cache below at 100 px (2-column) rows: a
    // handful of rows, and one render node each; a thousand tiles over 500
    // rows would be hundreds of nodes if every row were built.
    let nodes = laid.render_node_count();
    assert!(
        nodes < 400,
        "only the window is built: {nodes} render nodes for {ITEM_COUNT} tiles"
    );
    assert!(
        laid.find_text("tile0").is_some(),
        "the head tile is resident"
    );
    assert!(
        laid.find_text("tile500").is_none(),
        "a tile far below the window is never built"
    );
}

/// A keyed tile carrying an id and a shared log of the `id`s its state was
/// ever created for — one entry per STATE, not per view rebuild, so a
/// preserved element never adds a second entry while a remounted one does.
/// Mirrors `lazy_list.rs`'s `KeyedRow`/`KeyedRowState`.
#[derive(Clone)]
struct KeyedTile {
    id: u32,
    key: flui_foundation::ValueKey<u32>,
    inits: Arc<parking_lot::Mutex<Vec<u32>>>,
}

struct KeyedTileState {
    born_as: u32,
    log: Arc<parking_lot::Mutex<Vec<u32>>>,
}

impl StatefulView for KeyedTile {
    type State = KeyedTileState;
    fn create_state(&self) -> KeyedTileState {
        KeyedTileState {
            born_as: self.id,
            log: Arc::clone(&self.inits),
        }
    }
}

impl ViewState<KeyedTile> for KeyedTileState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        self.log.lock().push(self.born_as);
    }
    fn build(&self, _view: &KeyedTile, _ctx: &dyn BuildContext) -> impl IntoView {
        // The label carries the STATE's id (what the element was born as), so
        // a remounted element reads differently from a preserved one. The
        // grid's own tight tile constraints override any size this requests.
        Text::new(format!("tile{}", self.born_as))
    }
}

impl flui_view::View for KeyedTile {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

/// The static delegate's key map: a keyed tile of `GridView::count` whose
/// data moved out of the resident band while the viewport jumped to its new
/// place keeps its state — the same contract `GridView::builder` has with a
/// `find_index_by_key` callback, here with no callback at all (Flutter's
/// `SliverChildListDelegate` derives `findIndexByKey` from its children).
/// Mirrors `lazy_list.rs`'s
/// `list_view_new_keyed_row_moving_with_the_viewport_keeps_state`, over a
/// 2-D grid instead of a 1-D list.
#[test]
fn grid_view_count_keyed_tile_moving_with_the_viewport_keeps_state() {
    let inits = Arc::new(parking_lot::Mutex::new(Vec::<u32>::new()));
    let tiles = |order: &[u32]| -> Vec<BoxedView> {
        order
            .iter()
            .map(|&id| {
                KeyedTile {
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
        GridView::count(2, tiles(&initial)).repaint_boundaries(false),
        tight(200.0, 200.0),
    );
    assert!(
        laid.find_text("tile1").is_some(),
        "tile 1 is resident at the head"
    );

    // Tile 1 moves to (0-based) index 60 — row 30, column 0 at a 100px tile
    // stride — and the viewport jumps there in the same frame.
    let mut moved: Vec<u32> = (2..=100).collect();
    moved.insert(60, 1);
    laid.pump_widget(
        GridView::count(2, tiles(&moved))
            .repaint_boundaries(false)
            .offset(2900.0),
    );
    let tile1 = laid
        .find_text("tile1")
        .expect("tile 1 is resident at its new index");
    let top = laid.absolute_offset(tile1).dy.get();
    assert!(
        (0.0..200.0).contains(&top),
        "tile 1 is on screen at its new index; top={top}"
    );
    let states_for_1 = inits.lock().iter().filter(|&&id| id == 1).count();
    assert_eq!(
        states_for_1, 1,
        "tile 1's state must be created once and carried to its new index"
    );
}
