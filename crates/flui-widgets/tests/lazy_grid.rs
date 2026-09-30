//! Integration tests for `GridView::builder` (RenderSliverGrid → the
//! lazy-sliver element-wiring backend).
//!
//! Mirrors `lazy_list.rs`'s frame-sequence model: `pump_frame` calls
//! `service_child_requests` after `run_frame`, so two `tick` calls settle a
//! visible window — the first dispatches the child-build requests emitted by
//! `RenderSliverGrid::perform_layout`, the second lays out the now-built
//! tiles.

use std::sync::Arc;

use crate::common::{lay_out, tight};
use flui_rendering::delegates::{SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount};
use flui_view::ViewExt;
use flui_widgets::prelude::*;

fn two_column_delegate() -> Arc<dyn SliverGridDelegate> {
    Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2))
}

// ============================================================================
// Oracle 2-D positions
// ============================================================================

/// A 2-column 200 px-wide grid with square 100×100 tiles must place tiles at
/// (0, 0), (100, 0), (0, 100), (100, 100), proving the delegate-windowed
/// geometry holds when the children arrive through the element tree instead
/// of being pre-seeded directly.
///
/// Tiles are located by render type rather than by walking
/// `RenderSliverGrid`'s child list: the lazy backend's `ChildManager`
/// attaches each built tile's *parent* link but does not push it onto the
/// sliver's own `children()` array (shared behavior with the `RenderSliverList`
/// lazy backend — confirmed by inspecting both trees' `children()` output),
/// so the offsets are compared as an unordered set instead of by slot index.
pub(crate) fn lazy_grid_view_builder_places_tiles_at_oracle_positions() {
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

    let mut tile_positions: Vec<(f64, f64)> = tile_ids
        .iter()
        .map(|&id| {
            let tile_offset = laid.offset(id);
            (tile_offset.dx, tile_offset.dy)
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
