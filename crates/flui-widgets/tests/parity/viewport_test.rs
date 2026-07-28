//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/slivers_test.dart` (tag
//! `3.44.0`) — a shared multi-subject file (see `sliver_list_constructors_test.rs`
//! and `sliver_fixed_extent_list_test.rs`'s own module docs for that file's
//! `SliverList`/`SliverFixedExtentList`-subject slices). **This unit scopes
//! itself strictly to the file's 3 `Viewport`-subject cases** — the pre-scoped
//! case set this port was assigned:
//!
//! 1. `'Viewport basic test'` — **ported, real green**:
//!    [`viewport_basic_test_reports_size_offsets_and_visibility_at_four_scroll_offsets`].
//!    The oracle's own `verify()` helper reads
//!    `RenderSliverToBoxAdapter.geometry!.visible` directly (a `SliverGeometry`
//!    field, not `debugNeedsPaint`) — a literal, field-for-field match with
//!    FLUI's `SliverGeometry::visible` (same name, same `paint_extent > 0.0`
//!    formula on both sides, confirmed by reading
//!    `RenderSliverToBoxAdapter::perform_layout`,
//!    `crates/flui-objects/src/sliver/sliver_to_box_adapter.rs`). No
//!    interpretation or harness addition was needed for the paint-observability
//!    question this unit was scoped to investigate — `LaidOut::sliver_geometry`
//!    already exposes the identical field.
//! 2. `'Viewport anchor test'` — **out-of-scope-by-missing-feature, no pin
//!    possible**: FLUI's `RenderViewport` does carry center-sliver
//!    reverse-side layout (`set_center_sliver_index` splits the children
//!    into forward/reverse groups in `perform_layout` — its module doc
//!    predates that and still claims otherwise), but Flutter's `anchor`
//!    fraction has no equivalent anywhere: the center offset derives from
//!    the scroll offset alone, with no `mainAxisExtent * anchor` term
//!    (Flutter's `RenderViewport`, `rendering/viewport.dart`, tag `3.44.0`),
//!    and the `Viewport` widget (`crates/flui-widgets/src/scroll/viewport.rs`)
//!    exposes neither `anchor` nor `center` — there is no parameter to pass
//!    a value into, so unlike a behavior gap this can't even be pinned with
//!    an `#[ignore]`d red test. Filed as a Cross.H "Known gap" entry in
//!    `docs/ROADMAP.md` (root cause, file:line, and this ledger as the
//!    tracking reference) instead of a test stub.
//! 3. `'Multiple grids and lists'` — **ported, real green**:
//!    [`multiple_grids_and_lists_scrolling_reveals_each_groups_text_and_clamps_at_max_extent`].
//!    The oracle drives this with `tester.startGesture`/`gesture.moveBy` pan
//!    gestures, but every assertion in the case is about which text is on
//!    screen at a given scroll offset (layout-at-offset), never about drag
//!    physics (velocity, fling, slop) — so the port drives the identical
//!    resulting offsets directly through `ScrollController::jump_to` +
//!    `LaidOut::pump`, the same substitution precedent
//!    `resizing_the_viewport_reclamps_an_already_scrolled_position`
//!    (`scroll_controller_test.rs`) already established for exercising a real
//!    relayout without a synthetic pointer stream. The oracle's final drag
//!    (cumulative offset 210.0, three `-70.0` `moveBy`s) exceeds
//!    `max_scroll_extent` (184.2) — the port pins that `jump_to` clamps
//!    against the content dimensions real sliver layout published (a
//!    windowing or grid-row bug shifts the ceiling and fails the exact
//!    assert), matching the oracle's own implicit clamped-scroll
//!    expectation, not a hand-picked offset. (The layout-side
//!    `apply_content_dimensions` clamp branch itself is pinned by
//!    `resizing_the_viewport_reclamps_an_already_scrolled_position` in
//!    `scroll_controller_test.rs`.)
//!
//! **Total: 3 case names = 3 accounted for above (2 ported green, 0 pinned, 1
//! out-of-scope-by-missing-feature) — no case narrowed, silently dropped, or
//! laundered as passing.**
//!
//! Widget → render-object mapping:
//! - `Viewport` → [`Viewport`] (`crates/flui-widgets/src/scroll/viewport.rs`)
//!   → `RenderViewport<ScrollPosition>`
//!   (`crates/flui-objects/src/sliver/viewport.rs`).
//! - `SliverToBoxAdapter` → [`SliverToBoxAdapter`] →
//!   `RenderSliverToBoxAdapter`
//!   (`crates/flui-objects/src/sliver/sliver_to_box_adapter.rs`).
//! - `SliverList.list` → [`SliverList::list`] → `RenderSliverList`
//!   (`crates/flui-objects/src/sliver/sliver_list.rs`) — the lazy,
//!   `SparseChildren`-backed adaptor.
//! - `SliverFixedExtentList.list` → [`SliverFixedExtentList::new`] (FLUI has no
//!   separate named `.list` constructor — `::new(item_extent, children)` is
//!   already Flutter's `.list` semantics, the same mapping
//!   `sliver_fixed_extent_list_test.rs`'s own ledger already established) →
//!   `RenderSliverFixedExtentList`
//!   (`crates/flui-objects/src/sliver/sliver_fixed_extent_list.rs`) — eager,
//!   `ViewSeq`-backed, lays out every declared child unconditionally every
//!   frame (no windowed child creation).
//! - `SliverGrid` → [`SliverGrid::new`] → `RenderSliverGrid`
//!   (`crates/flui-objects/src/sliver/sliver_grid.rs`) — eager, `ViewSeq`-backed,
//!   but DOES window which children get laid out per frame (its own module
//!   doc: "Only the in-band rows... are laid out per frame").
//!
//! # Divergence note — case 3's "is this text on screen" predicate
//!
//! Flutter's `find.text(...)` defaults `skipOffstage: true` — an
//! onstage-filtered walk (`SliverMultiBoxAdaptorElement`'s
//! `debugVisitOnstageChildren` restricts it to children strictly
//! intersecting the visible window), on top of the
//! `RenderSliverMultiBoxAdaptor` family never creating an `Element` at all
//! outside the cache-extended window — the oracle's finder is
//! visibility-filtered twice over, never a bare existence check. FLUI's three
//! sliver families diverge from each other AND from Flutter here:
//! `SliverFixedExtentList` lays out every child unconditionally (documented in
//! its own module doc); `SliverGrid` windows layout but still keeps every
//! child's slot mounted; `SliverList` is the only one genuinely lazy. Given
//! this content (244.2px total) comfortably fits inside `RenderViewport`'s
//! default 250px cache extent on each side regardless of scroll offset, all
//! four texts stay mounted (and, except where windowed out, laid out) for the
//! ENTIRE test — `find_text(...).is_some()` alone would report every text
//! "present" at every checkpoint, never matching the oracle's expected
//! transitions. The port instead checks the found node's own committed
//! absolute geometry against the viewport's visible band
//! (`text_on_screen_at`, below) — the honest FLUI-observable equivalent of
//! "is this text painted on screen right now," which is what the oracle is
//! actually validating.
//!
//! Confirmed empirically, not just reasoned: the lazy `SliverList`'s virtual
//! child (`TOP`) is genuinely unmounted (`find_text` returns `None`, not just
//! geometrically off-screen) on the frame `lay_out`/`jump_to` itself produces
//! — it needs the same two-tick `settle` sequence
//! `sliver_list_constructors_test.rs` documents (lazy children build *after*
//! paint) before its presence check is meaningful at all. `settle` runs after
//! every scroll-position change in this test for that reason, not just after
//! the initial mount.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
use flui_types::Offset;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    ScrollController, SizedBox, SliverFixedExtentList, SliverGrid, SliverList, SliverToBoxAdapter,
    Text, Viewport,
};

use crate::common::{LaidOut, lay_out, offset, size, tight};

// ============================================================================
// Case 1 — 'Viewport basic test'
// ============================================================================

/// Five `SliverToBoxAdapter`s, each wrapping a 400px-tall `SizedBox` — the
/// oracle's `test(tester, offset)` helper's fixed scene
/// (`slivers_test.dart`, tag `3.44.0`).
fn five_400px_slivers() -> Vec<BoxedView> {
    (0..5)
        .map(|_| {
            SliverToBoxAdapter::new()
                .child(SizedBox::height(400.0))
                .boxed()
        })
        .collect()
}

/// Reads each of the viewport's 5 sliver children's box child absolute
/// (root-local) offset, and the sliver's own committed `SliverGeometry::visible`
/// — the exact two quantities the oracle's `verify()` helper reads off
/// `RenderBox.localToGlobal` and `RenderSliverToBoxAdapter.geometry!.visible`.
fn viewport_child_offsets_and_visibility(
    laid: &LaidOut,
    viewport: RenderId,
) -> ([Offset; 5], [bool; 5]) {
    let mut offsets = [offset(0.0, 0.0); 5];
    let mut visibility_flags = [false; 5];
    for (index, (out_offset, out_visible)) in offsets
        .iter_mut()
        .zip(visibility_flags.iter_mut())
        .enumerate()
    {
        let sliver = laid.child(viewport, index);
        let box_child = laid.only_child(sliver);
        *out_offset = laid.absolute_offset(box_child);
        *out_visible = laid.sliver_geometry(sliver).visible;
    }
    (offsets, visibility_flags)
}

/// Flutter parity: `slivers_test.dart` `'Viewport basic test'` (tag `3.44.0`)
/// — 4 sub-checks, one per scroll offset (0.0, 200.0, 600.0, 900.0), each a
/// fresh `pumpWidget` with a new `ViewportOffset.fixed(offset)` (FLUI:
/// `Viewport::offset(f32)`, the widget-owned "Pixels" offset mode — see
/// `viewport.rs`'s `OffsetSource` doc). Every offset/visibility constant below
/// is copied verbatim from the oracle, per the house rule that oracle-hardcoded
/// expectations port as literal constants, not re-derived geometry.
#[test]
fn viewport_basic_test_reports_size_offsets_and_visibility_at_four_scroll_offsets() {
    let mut laid = lay_out_at(0.0);
    assert_eq!(
        laid.size(laid.root()),
        size(800.0, 600.0),
        "the viewport sizes to its incoming (bounded) constraints"
    );
    assert_viewport_state(
        &laid,
        0.0,
        [
            offset(0.0, 0.0),
            offset(0.0, 400.0),
            offset(0.0, 800.0),
            offset(0.0, 1200.0),
            offset(0.0, 1600.0),
        ],
        [true, true, false, false, false],
    );

    laid.pump_widget(Viewport::new(five_400px_slivers()).offset(200.0));
    assert_viewport_state(
        &laid,
        200.0,
        [
            offset(0.0, -200.0),
            offset(0.0, 200.0),
            offset(0.0, 600.0),
            offset(0.0, 1000.0),
            offset(0.0, 1400.0),
        ],
        [true, true, false, false, false],
    );

    laid.pump_widget(Viewport::new(five_400px_slivers()).offset(600.0));
    assert_viewport_state(
        &laid,
        600.0,
        [
            offset(0.0, -600.0),
            offset(0.0, -200.0),
            offset(0.0, 200.0),
            offset(0.0, 600.0),
            offset(0.0, 1000.0),
        ],
        [false, true, true, false, false],
    );

    laid.pump_widget(Viewport::new(five_400px_slivers()).offset(900.0));
    assert_viewport_state(
        &laid,
        900.0,
        [
            offset(0.0, -900.0),
            offset(0.0, -500.0),
            offset(0.0, -100.0),
            offset(0.0, 300.0),
            offset(0.0, 700.0),
        ],
        [false, false, true, true, false],
    );
}

fn lay_out_at(scroll_offset: f32) -> LaidOut {
    lay_out(
        Viewport::new(five_400px_slivers()).offset(scroll_offset),
        tight(800.0, 600.0),
    )
}

fn assert_viewport_state(
    laid: &LaidOut,
    scroll_offset: f32,
    expected_offsets: [Offset; 5],
    expected_visible: [bool; 5],
) {
    let (offsets, visibility_flags) = viewport_child_offsets_and_visibility(laid, laid.root());
    assert_eq!(
        offsets, expected_offsets,
        "child absolute offsets at scroll offset {scroll_offset}"
    );
    assert_eq!(
        visibility_flags, expected_visible,
        "child sliver geometry.visible at scroll offset {scroll_offset}"
    );
}

// ============================================================================
// Case 3 — 'Multiple grids and lists'
// ============================================================================

/// The viewport's fixed cross-axis extent in the oracle's scene
/// (`SizedBox(width: 44.4, height: 60.0)`, `slivers_test.dart`, tag `3.44.0`).
const SCENE_WIDTH: f32 = 44.4;
/// The viewport's main-axis extent (see [`SCENE_WIDTH`]).
const SCENE_HEIGHT: f32 = 60.0;
/// Every list/grid item's main-axis extent in the oracle's scene.
const ITEM_EXTENT: f32 = 22.2;

/// The oracle's `CustomScrollView` sliver sequence: a 3-item `SliverList.list`
/// (item 0 carries "TOP"), a 3-item `SliverFixedExtentList.list` (item 1
/// carries "A"), a 3-item 2-column `SliverGrid` (item 1 carries "B"), and a
/// second 3-item `SliverList.list` (item 2 carries "BOTTOM") —
/// `slivers_test.dart`'s `'Multiple grids and lists'` case (tag `3.44.0`),
/// mirrored tree-shape-for-tree-shape.
///
/// The 4 plain (textless) items port Flutter's bare `SizedBox()` (no width or
/// height — a pure constraint pass-through) as [`SizedBox::shrink`]: for a
/// childless `RenderConstrainedBox`, `incoming.constrain(combined.smallest())`
/// equals Flutter's `enforce(...).constrain(Size.zero)` under ANY incoming
/// constraints — and here the slivers hand down tight constraints anyway
/// (`SliverFixedExtentList`'s `item_extent`, `SliverGrid`'s tile size —
/// "tiles do not measure themselves", `sliver_grid.rs`), so the two shapes
/// are size-equivalent unconditionally, not merely in this tree.
fn multiple_grids_and_lists_scene() -> Viewport<Vec<BoxedView>> {
    Viewport::new(vec![
        SliverList::list(
            ITEM_EXTENT,
            vec![
                SizedBox::height(ITEM_EXTENT)
                    .child(Text::new("TOP"))
                    .boxed(),
                SizedBox::height(ITEM_EXTENT).boxed(),
                SizedBox::height(ITEM_EXTENT).boxed(),
            ],
        )
        .boxed(),
        SliverFixedExtentList::new(
            ITEM_EXTENT,
            vec![
                SizedBox::shrink().boxed(),
                Text::new("A").boxed(),
                SizedBox::shrink().boxed(),
            ],
        )
        .boxed(),
        SliverGrid::new(
            Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
            vec![
                SizedBox::shrink().boxed(),
                Text::new("B").boxed(),
                SizedBox::shrink().boxed(),
            ],
        )
        .boxed(),
        SliverList::list(
            ITEM_EXTENT,
            vec![
                SizedBox::height(ITEM_EXTENT).boxed(),
                SizedBox::height(ITEM_EXTENT).boxed(),
                SizedBox::height(ITEM_EXTENT)
                    .child(Text::new("BOTTOM"))
                    .boxed(),
            ],
        )
        .boxed(),
    ])
}

/// Whether `text`'s `RenderParagraph` is mounted AND its committed absolute
/// (root-local) geometry intersects the viewport's visible band
/// `[0, SCENE_HEIGHT)` — see this module's "Divergence note" doc for why mere
/// `find_text(...).is_some()` can't serve as the oracle's `find.text(...)`
/// presence check here.
fn text_on_screen_at(laid: &LaidOut, text: &str) -> bool {
    let Some(id) = laid.find_text(text) else {
        return false;
    };
    let top = laid.absolute_offset(id).dy.get();
    let bottom = top + laid.size(id).height.get();
    top < SCENE_HEIGHT && bottom > 0.0
}

/// `[top, a, b, bottom]` — one flag per text, in that fixed order.
fn assert_on_screen(laid: &LaidOut, scroll_offset: f32, expected: [bool; 4]) {
    assert_eq!(
        [
            text_on_screen_at(laid, "TOP"),
            text_on_screen_at(laid, "A"),
            text_on_screen_at(laid, "B"),
            text_on_screen_at(laid, "BOTTOM"),
        ],
        expected,
        "[TOP, A, B, BOTTOM] on-screen at scroll offset {scroll_offset}"
    );
}

/// Drives the lazy `SliverList` virtualizer's request -> service -> re-layout
/// settle sequence to completion — two ticks, the same convention
/// `sliver_list_constructors_test.rs`'s own `settle` documents: lazy children
/// build *after* paint, so a scroll-position change needs one tick to emit
/// build requests for the newly-demanded residents and a second to re-lay-out
/// with them actually built.
fn settle(laid: &mut LaidOut) {
    laid.tick();
    laid.tick();
}

/// Flutter parity: `slivers_test.dart` `'Multiple grids and lists'` (tag
/// `3.44.0`) — see this module's doc comment for the gesture-to-`jump_to`
/// substitution and the final-offset clamp derivation.
#[test]
fn multiple_grids_and_lists_scrolling_reveals_each_groups_text_and_clamps_at_max_extent() {
    let controller = ScrollController::new();
    let mut laid = lay_out(
        multiple_grids_and_lists_scene().position(controller.position()),
        tight(SCENE_WIDTH, SCENE_HEIGHT),
    );
    settle(&mut laid);

    assert_on_screen(&laid, controller.pixels(), [true, false, false, false]);

    controller.jump_to(70.0);
    laid.pump();
    settle(&mut laid);
    assert_on_screen(&laid, controller.pixels(), [false, true, false, false]);

    controller.jump_to(140.0);
    laid.pump();
    settle(&mut laid);
    assert_on_screen(&laid, controller.pixels(), [false, false, true, false]);

    // Oracle drags a further -70 (cumulative -210 across 3 `moveBy(Offset(0.0,
    // -70.0))`s). Total scroll extent, in `ITEM_EXTENT` units: the TOP-group
    // `SliverList` (3 items), the `SliverFixedExtentList` (3 items), and the
    // BOTTOM-group `SliverList` (3 items) each contribute 3 * ITEM_EXTENT; the
    // 2-column `SliverGrid` (3 tiles -> 2 rows, tile main-axis extent ==
    // ITEM_EXTENT since its cross-axis tile extent SCENE_WIDTH / 2 == 22.2 ==
    // ITEM_EXTENT and `child_aspect_ratio` defaults to 1.0) contributes
    // 2 * ITEM_EXTENT — 11 * ITEM_EXTENT = 244.2 total. max_scroll_extent =
    // 244.2 - SCENE_HEIGHT = 184.2. `jump_to` clamps eagerly at the
    // controller against the extents layout published in the frames above —
    // so this pins the layout-derived ceiling (any windowing or grid-row bug
    // moves it), while the layout-side `apply_content_dimensions` clamp
    // branch is pinned by
    // `resizing_the_viewport_reclamps_an_already_scrolled_position`
    // (`scroll_controller_test.rs`).
    controller.jump_to(210.0);
    laid.pump();
    settle(&mut laid);
    let max_scroll_extent = 11.0 * ITEM_EXTENT - SCENE_HEIGHT;
    assert_eq!(
        controller.pixels(),
        max_scroll_extent,
        "3 * -70.0 requests a cumulative 210.0, past max_scroll_extent \
         ({max_scroll_extent}) — the position must clamp to the layout-published ceiling"
    );
    assert_on_screen(&laid, controller.pixels(), [false, false, false, true]);
}
