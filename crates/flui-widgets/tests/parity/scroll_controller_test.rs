//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/scroll_controller_test.dart`
//! (tag `3.44.0`).
//!
//! Ported (this file):
//! - `'ScrollController control test'` (2nd variant, `GridView.count` +
//!   `initialScrollOffset`) → [`initial_scroll_offset_and_position_persist_across_a_column_count_rebuild`].
//! - `'ScrollController control test'` (1st variant, the `realOffset() ==
//!   controller.offset` invariant) → [`committed_geometry_matches_the_controllers_pixels_after_jump_to`].
//! - The "more than one position" read/write tests →
//!   [`one_controller_shared_by_two_independently_mounted_scrollables_stays_in_sync`],
//!   ported as a documented divergence (see that test's doc comment).
//!
//! Not isolated in any single upstream test but derived directly from FLUI's
//! own render-object contract (same precedent as
//! `parity/single_child_scroll_view_test.rs`'s offset-clamp case):
//! [`resizing_the_viewport_reclamps_an_already_scrolled_position`].
//!
//! Skipped — unbuilt machinery (v1 restrictions documented in
//! `crates/flui-widgets/src/scroll/scroll_controller.rs`'s module doc):
//! - `animateTo` and everything that exercises it (`'DrivenScrollActivity
//!   ending after dispose'`, the `animateTo` halves of both `'ScrollController
//!   control test'`s, `'Write operations on ScrollControllers with more than
//!   one position do not throw'`) — `ScrollController` exposes `set_pixels`/
//!   `jump_to` only; animated-to-a-target-with-a-curve requires a ticking
//!   driver that does not exist yet.
//! - `'Read/Write operations on ScrollControllers with no/more-than-one
//!   positions fail'` (the `throwsAssertionError` halves) — FLUI's
//!   `ScrollController` is bound 1:1 to one `ScrollPosition` at construction;
//!   there is no attach/detach protocol and therefore no "how many positions
//!   are attached right now" state to assert on. The controller is trivially
//!   always readable (starts at `ScrollPosition::zero()`), so the "no
//!   positions attached" failure mode doesn't exist either.
//! - `'keepScrollOffset'` — no `PageStorage` equivalent exists in FLUI.
//! - `'isScrollingNotifier works with pointer scroll'` — no
//!   `isScrollingNotifier`/mouse-wheel pointer-signal support exists.
//! - `'$ScrollController dispatches object creation in constructor'` — a Dart
//!   `leak_tracker` tooling assertion; not a portable behavior.
//!
//! Adequately covered elsewhere, not re-ported to avoid duplication:
//! - `'Scroll controllers notify when the position changes'` — the
//!   notify-on-change contract is already pinned by
//!   `scroll_controller.rs`'s own `set_pixels_updates_position_and_notifies_listener`
//!   unit test and exercised end-to-end by `tests/scroll.rs`'s drag tests.
//!
//! Widget → render-object mapping: `GridView`/`ListView` → `Viewport`
//! (`RenderViewport`) → sliver child → box children
//! (`crates/flui-widgets/src/scroll/{grid_view,list_view}.rs`).
//!
//! Fix applied: `ScrollController` had no way to seed an initial pixel value
//! at construction (Flutter's `ScrollController(initialScrollOffset: ...)`) —
//! added `ScrollController::with_initial_scroll_offset(f32)`
//! (`crates/flui-widgets/src/scroll/scroll_controller.rs`), a localized
//! addition that seeds `set_pixels` on a fresh `ScrollPosition::zero()`; no
//! `flui-rendering` change was needed (tripwire not crossed).

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_view::ViewExt;
use flui_widgets::{GridView, ListView, ScrollController, SizedBox};

use crate::common::{lay_out, offset, tight};

/// `ScrollController::with_initial_scroll_offset` seeds the position before
/// any layout runs, and that seed — plus later `jump_to` writes — survives a
/// rebuild that changes the grid's own shape (`cross_axis_count` 4 → 2) as
/// long as the SAME controller is handed to both builds, because
/// `pump_widget` reuses the root element (no remount): the injected
/// `ScrollPosition` is the identical shared `Arc` before and after.
///
/// Flutter parity: `scroll_controller_test.dart` `'ScrollController control
/// test'` (2nd variant) — `ScrollController(initialScrollOffset: 209.0)` on a
/// `GridView.count(crossAxisCount: 4)` reads `offset == 209.0` immediately;
/// `jumpTo(105.0)` then a rebuild to `crossAxisCount: 2` (same controller, no
/// key change) leaves `offset == 105.0` unchanged.
#[test]
fn initial_scroll_offset_and_position_persist_across_a_column_count_rebuild() {
    // 24 square tiles, 4 columns, viewport 200×200: cross axis 200/4 = 50px
    // tiles -> 6 rows * 50px = 300px content -> max_scroll_extent = 100.
    let controller = ScrollController::with_initial_scroll_offset(60.0);
    let four_columns = || {
        let tiles: Vec<_> = (0..24).map(|_| SizedBox::shrink().boxed()).collect();
        GridView::count(4, tiles).position(controller.position())
    };

    let mut laid = lay_out(four_columns(), tight(200.0, 200.0));
    assert_eq!(
        controller.pixels(),
        60.0,
        "with_initial_scroll_offset's seed must still read back after the first \
         layout commits extents that comfortably contain it (max_scroll_extent = 100)"
    );

    controller.jump_to(30.0);
    assert_eq!(controller.pixels(), 30.0);

    // Rebuild in place (same controller, no key/type change): 2 columns, same
    // 24 tiles -> 100px tiles -> 12 rows * 100px = 1200px content ->
    // max_scroll_extent = 1000. 30.0 stays comfortably in range.
    let two_columns = {
        let tiles: Vec<_> = (0..24).map(|_| SizedBox::shrink().boxed()).collect();
        GridView::count(2, tiles).position(controller.position())
    };
    laid.pump_widget(two_columns);

    assert_eq!(
        controller.pixels(),
        30.0,
        "a structurally different rebuild (2 columns instead of 4) that reuses the \
         same ScrollController must not reset or otherwise disturb its position"
    );
}

/// The scroll controller's `pixels()` and the REAL committed render geometry
/// it drives must agree exactly, not just "both changed" — reading through
/// two different subsystems (the controller handle vs. the render tree's
/// committed child offset) must land on the identical number.
///
/// Flutter parity: `scroll_controller_test.dart` `'ScrollController control
/// test'` (1st variant) — every assertion pairs `controller.offset` against
/// `realOffset()` (`ScrollableState.position.pixels`, read straight off the
/// mounted `Scrollable`) and requires them equal; `jumpTo(653.0)` then
/// `await tester.pump()` before comparing. FLUI has no synchronous
/// `Scrollable` rebuild on `jump_to` (`scrollable_position_mode_relayouts_from_external_mutation_with_no_pixels_push`
/// in `tests/scroll.rs` pins that a pump is required), so this pumps once
/// before reading geometry, matching upstream's own `await tester.pump()`.
#[test]
fn committed_geometry_matches_the_controllers_pixels_after_jump_to() {
    // 25 items * 200px = 5000px content in a 300px viewport ->
    // max_scroll_extent = 4700, comfortably containing Flutter's literal 653.0.
    let controller = ScrollController::new();
    let items = || {
        let items: Vec<_> = (0..25).map(|_| SizedBox::shrink().boxed()).collect();
        ListView::new(200.0, items).position(controller.position())
    };

    let mut laid = lay_out(items(), tight(300.0, 300.0));
    let viewport = laid.root();
    let sliver = laid.only_child(viewport);
    let item0 = laid.only_child(sliver);

    assert_eq!(controller.pixels(), 0.0);
    assert_eq!(laid.offset(item0), offset(0.0, 0.0));

    controller.jump_to(653.0);
    laid.pump();

    assert_eq!(controller.pixels(), 653.0);
    // A right-way-up (TopToBottom) sliver's child paint offset is
    // `-pixels` on the main axis (`sliver_layout.rs`'s `child_paint_offset`
    // contract, cited in `single_child_scroll_view_test.rs`) — the render
    // tree's own, independently-computed number must equal the controller's.
    assert_eq!(
        laid.offset(item0),
        offset(0.0, -653.0),
        "the committed child paint offset must equal exactly -controller.pixels() \
         after a pump — a divergence here means the position-mode pixel value \
         reached the controller but not the render tree (or vice versa)"
    );
}

/// FLUI's `ScrollController` has no attach/detach protocol (see this file's
/// module doc for the "more than one position" tests this replaces): handing
/// the SAME controller's `.position()` to two independently mounted trees
/// does not throw — both trees share the identical `ScrollPosition` `Arc`,
/// so a mutation through the controller is visible in BOTH trees' committed
/// geometry. This is an intentional simplification versus Flutter's
/// multi-attach bookkeeping (documented as a v1 restriction: "one position
/// per controller" — trivially true here since it IS the one position, just
/// consumed by two unrelated render trees).
#[test]
fn one_controller_shared_by_two_independently_mounted_scrollables_stays_in_sync() {
    // 5 items * 60px = 300px content in a 180px viewport for BOTH trees ->
    // identical max_scroll_extent (120) regardless of which tree lays out last.
    let controller = ScrollController::new();
    let five_items = || {
        let items: Vec<_> = (0..5).map(|_| SizedBox::shrink().boxed()).collect();
        ListView::new(60.0, items).position(controller.position())
    };

    let mut laid_a = lay_out(five_items(), tight(200.0, 180.0));
    let mut laid_b = lay_out(five_items(), tight(200.0, 180.0));
    let item_a0 = laid_a.only_child(laid_a.only_child(laid_a.root()));
    let item_b0 = laid_b.only_child(laid_b.only_child(laid_b.root()));

    assert_eq!(laid_a.offset(item_a0), offset(0.0, 0.0));
    assert_eq!(laid_b.offset(item_b0), offset(0.0, 0.0));

    controller.jump_to(90.0);
    laid_a.pump();
    laid_b.pump();

    assert_eq!(
        laid_a.offset(item_a0),
        offset(0.0, -90.0),
        "tree A must observe the shared controller's jump_to"
    );
    assert_eq!(
        laid_b.offset(item_b0),
        offset(0.0, -90.0),
        "tree B must observe the SAME shared controller's jump_to, proving both \
         trees hold the identical ScrollPosition rather than independent copies"
    );
}

/// A viewport that shrinks so much its `max_scroll_extent` drops below the
/// CURRENT `pixels` value must reclamp the position down to the new maximum,
/// through the real `RenderViewport::perform_layout` → `apply_content_dimensions`
/// path — not just when a caller happens to call `update_dimensions` again by
/// hand (`update_dimensions_clamps_existing_pixels_to_new_extents` in
/// `scroll_controller.rs` already pins that half at the controller level;
/// this proves the SAME reclamp fires from an actual relayout).
///
/// No single upstream test isolates this exact geometry-only scenario across
/// the three fetched Flutter source files, so the oracle here is FLUI's own
/// render-object contract: `ScrollableViewportOffset::apply_content_dimensions`
/// (`crates/flui-rendering/src/view/viewport_offset.rs`) clamps `pixels` into
/// `[min_scroll_extent, max_scroll_extent]` every time `RenderViewport`
/// commits new extents — the same clamp Flutter's
/// `ScrollPosition.applyContentDimensions` performs under
/// `ClampingScrollPhysics` (`widgets/scroll_position_with_single_context.dart`).
#[test]
fn resizing_the_viewport_reclamps_an_already_scrolled_position() {
    // 6 items * 100px = 600px content, fixed regardless of viewport size.
    let controller = ScrollController::new();
    let list_at_height = |height: f32| {
        let items: Vec<_> = (0..6).map(|_| SizedBox::shrink().boxed()).collect();
        SizedBox::new(200.0, height)
            .child(ListView::new(100.0, items).position(controller.position()))
    };

    // Tight width, LOOSE height (0..600) so each `SizedBox`'s own requested
    // height actually takes effect instead of being overridden by a tight
    // root constraint (the same shrink-wrap-style constraints `tests/scroll.rs`
    // uses for its own resize-sensitive cases).
    let root_constraints = BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(600.0));

    // Viewport 200px tall -> max_scroll_extent = 600 - 200 = 400. Scroll to
    // the very bottom.
    let mut laid = lay_out(list_at_height(200.0), root_constraints);
    controller.jump_to(400.0);
    laid.pump();
    assert_eq!(controller.pixels(), 400.0);

    // Grow the viewport to 550px (same root constraints, same controller, no
    // remount) -> max_scroll_extent = 600 - 550 = 50, well below the current
    // 400 pixels -> the layout-driven reclamp must fire.
    laid.pump_widget(list_at_height(550.0));

    assert_eq!(
        controller.pixels(),
        50.0,
        "a viewport resize that shrinks max_scroll_extent below the current pixels \
         must reclamp pixels down to the new maximum through real layout, not leave \
         the position stuck past its new legal range"
    );
}
