//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/list_view_test.dart`
//! Pattern: a `ListView.builder` over N items whose combined extent fits the
//! viewport builds all N items after the lazy settle sequence (two ticks).
//!
//! Widget → render-object mapping:
//! - `ListView` → `RenderViewport` (root) + `RenderSliverList` (sliver child)
//! - Each item → one `RenderConstrainedBox` (via `SizedBox`)
//!
//! Divergence: Flutter's test asserts item count via `find.byType`; FLUI uses
//! `render_node_count` (render-tree node count) because the type-finder is a
//! new primitive verified elsewhere. The frame sequence (two ticks) is an
//! intentional FLUI-specific detail documented in `lazy_list.rs`.

use crate::common::{lay_out, tight};
use flui_view::ViewExt;
use flui_widgets::{ListView, SizedBox};

/// A `ListView.builder` over 3 items that all fit in the viewport builds
/// all 3 items after the two-tick lazy-settle sequence.
///
/// Flutter parity: list_view_test.dart — dynamic list populates the viewport
/// when all items are visible (C1.7 / C2-dynamic contract).
///
/// Frame sequence (see `lazy_list.rs` module doc for rationale):
/// - After mount: sliver has no children yet.
/// - After tick 1: `run_frame` emits build requests; `service_child_requests`
///   builds all 3 children.
/// - After tick 2: sliver re-lays with real children; tree is settled.
///
/// Expected node count: 1 `RenderViewport` + 1 `RenderSliverList` + 3 items
/// = 5 render nodes total.
#[test]
fn list_view_builder_builds_all_visible_items() {
    // 3 items × 60 px = 180 px total; viewport is 300 × 300 → all visible.
    let mut laid = lay_out(
        ListView::builder(3, 60.0, |index| {
            if index < 3 {
                Some(SizedBox::new(300.0, 60.0).boxed())
            } else {
                None
            }
        }),
        tight(300.0, 300.0),
    );

    // tick 1: dispatches child-build requests.
    laid.tick();
    // tick 2: sliver re-lays with the built children.
    laid.tick();

    // 1 RenderViewport + 1 RenderSliverList + 3 SizedBox nodes = 5.
    assert_eq!(
        laid.render_node_count(),
        5,
        "ListView(3 items) must have exactly 5 render nodes after settle \
         (1 viewport + 1 sliver-list + 3 items)"
    );
}
