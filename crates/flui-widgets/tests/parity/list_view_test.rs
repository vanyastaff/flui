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
//!
//! Two more cases ported from the same upstream file (tag `3.44.0`):
//! - `'Updates viewport dimensions when scroll direction changes'` →
//!   [`viewport_dimension_updates_across_a_scroll_direction_rebuild`].
//! - `'ListView large scroll jump'` →
//!   [`large_scroll_jump_settles_the_new_window_without_materializing_the_skipped_band`],
//!   adapted: upstream asserts the EXACT sequence of item-builder indices
//!   invoked (`log`); FLUI's lazy virtualizer's exact cache-extent/windowing
//!   constants are a separate, already-tested concern
//!   (`crates/flui-objects/src/sliver/sliver_list.rs`), so this instead
//!   asserts the property upstream's exact log is really checking: a large
//!   jump does not force-build every index it skipped over.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::common::{lay_out, tight};
use flui_view::ViewExt;
use flui_widgets::prelude::Axis;
use flui_widgets::{ListView, ScrollController, SizedBox};

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

/// `ScrollController::viewport_dimension_pixels` must track the CURRENT
/// scroll axis's real viewport length, and must switch correctly when a
/// rebuild flips `scroll_direction` on the SAME controller (no remount).
///
/// Flutter parity: list_view_test.dart `'Updates viewport dimensions when
/// scroll direction changes'` (regression for flutter/flutter#43380) — a
/// 100×200 box hosting the list reports `viewportDimension == 100.0` when
/// horizontal, `200.0` when vertical, and `100.0` again once switched back.
#[test]
fn viewport_dimension_updates_across_a_scroll_direction_rebuild() {
    let controller = ScrollController::new();
    let list = |axis| {
        ListView::new(50.0, vec![SizedBox::new(50.0, 50.0).boxed()])
            .scroll_direction(axis)
            .position(controller.position())
    };

    // 100 wide × 200 tall: horizontal viewport dimension is the width (100).
    let mut laid = lay_out(list(Axis::Horizontal), tight(100.0, 200.0));
    assert_eq!(
        controller.viewport_dimension_pixels(),
        100.0,
        "horizontal scroll direction must report the viewport's WIDTH"
    );

    // Same controller, same root constraints, vertical instead: viewport
    // dimension becomes the height (200).
    laid.pump_widget(list(Axis::Vertical));
    assert_eq!(
        controller.viewport_dimension_pixels(),
        200.0,
        "switching to vertical must update viewport_dimension_pixels to the HEIGHT"
    );

    // Back to horizontal: must report 100 again, not get stuck at 200.
    laid.pump_widget(list(Axis::Horizontal));
    assert_eq!(
        controller.viewport_dimension_pixels(),
        100.0,
        "switching back to horizontal must update viewport_dimension_pixels back \
         to the WIDTH, not retain the previous axis's value"
    );
}

/// A single large `jump_to` well past the currently-built window must settle
/// (after the lazy virtualizer's two-tick settle sequence) by building only
/// the new visible window — not every index between the old and new
/// position. This is the property Flutter's exact-index-log assertion is
/// really checking (see this file's module doc for why the log itself isn't
/// ported verbatim).
///
/// Flutter parity: list_view_test.dart `'ListView large scroll jump'` —
/// `position.jumpTo(2025.0)` on a 20-item, 200px-extent list produces a build
/// log of `[8, 9, 10, 11, 12, 13, 14]` (the new window), never the indices in
/// between the old window (`[0..4]`) and the new one.
#[test]
fn large_scroll_jump_settles_the_new_window_without_materializing_the_skipped_band() {
    let controller = ScrollController::new();
    let built_indices: Rc<RefCell<HashSet<usize>>> = Rc::new(RefCell::new(HashSet::new()));
    let log = Rc::clone(&built_indices);

    // 30 items * 60px estimate = 1800px content in a 180px viewport ->
    // max_scroll_extent = 1620.
    let widget = ListView::builder(30, 60.0, move |index| {
        log.borrow_mut().insert(index);
        (index < 30).then(|| SizedBox::new(200.0, 60.0).boxed())
    })
    .position(controller.position());

    let mut laid = lay_out(widget, tight(200.0, 180.0));
    laid.tick();
    laid.tick();
    built_indices.borrow_mut().clear();

    // Jump deep into the list — the new visible window sits around index 20
    // (1200px / 60px per item); index 10 sits squarely in the skipped band
    // between the old window (near index 0) and the new one.
    //
    // Unlike `AnimatedBuilder`-wrapped `Scrollable`, a bare `ListView` in
    // position mode has no listener that reacts to `jump_to` on its own — a
    // `pump()` (mark-dirty, matching `list_view_position_passthrough_feeds_the_content_dimension_feedback_loop`
    // in `tests/scroll.rs`) is what makes the render tree notice the new
    // position; the two follow-up `tick()`s replay the same
    // dispatch-then-settle cadence the initial mount above needed.
    controller.jump_to(1200.0);
    laid.pump();
    laid.tick();
    laid.tick();

    let built = built_indices.borrow();
    assert!(
        !built.contains(&10),
        "a large jump must not force-build an index in the skipped band between \
         the old and new visible windows; built indices: {built:?}"
    );
    assert!(
        built.iter().any(|&index| (18..=23).contains(&index)),
        "a large jump must build items in the new visible window (around index 20); \
         built indices: {built:?}"
    );
}
