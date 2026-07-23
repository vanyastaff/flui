//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart` `SliverGrid`
//!   (line 739, `.list`/`.builder` named constructors over
//!   `SliverChildListDelegate`/`SliverChildBuilderDelegate`, both composing
//!   down to the same lazy `SliverMultiBoxAdaptorWidget` machinery in Flutter).
//! - Render objects: `packages/flutter/lib/src/rendering/sliver_grid.dart`
//!   `RenderSliverGrid` (with a `childManager`, the lazy path) and its
//!   `SliverGridLayout`/`SliverGridRegularTileLayout` delegate abstraction.
//! - Tests (widget level): `packages/flutter/test/widgets/slivers_test.dart`
//!   (tag `3.44.0`) — a SHARED 38-`testWidgets`-case multi-subject file
//!   (`Viewport`, `SliverList`, `SliverFixedExtentList`, `SliverOffstage`, and
//!   this widget all live in it). **This port scopes itself strictly to the
//!   `SliverGrid`-subject cases** (enumerated below); the other subjects are
//!   separate, already-ported-elsewhere or future units — same scoping
//!   convention `sliver_list_test.rs`/`sliver_fixed_extent_list_test.rs` use
//!   for this exact shared file.
//!
//! # Content sweep (standing rule — run before naming a subject list)
//!
//! `git grep -l "SliverGrid" 3.44.0 -- 'packages/flutter/test/**'` hits 12
//! files. Classified (every candidate read by BODY, not by title):
//!
//! - **`slivers_test.dart`** — the 9 genuine `SliverGrid`-subject
//!   `testWidgets` cases this file's scope covers; ledger below.
//! - **`rendering/sliver_cache_test.dart`** — 1 genuine render-level subject
//!   case, `'RenderSliverGrid calculates correct geometry'` — its
//!   disposition is documented next to the `RenderSliverGrid`/
//!   `RenderSliverGridLazy` catalog rows in `render_object_harness.rs` rather
//!   than repeated here.
//! - **`widgets/grid_view_test.dart`** — reading this file's own case
//!   BODIES (not titles) found it is **not** pure scaffolding: it carries its
//!   own genuine `SliverGridDelegate`/`SliverGridLayout`-subject cases (e.g.
//!   `'SliverGridRegularTileLayout - can handle close to zero
//!   mainAxisStride'`, `'SliverGridDelegateWithFixedCrossAxisCount
//!   mainAxisExtent works as expected'`, `'SliverGridDelegate mainAxisExtent
//!   add assert'` — confirmed by reading their bodies, which construct a
//!   delegate and call `getLayout` directly). This is a **correction**, not
//!   an assumption: `grid_view_test.dart` is a separate oracle file entirely
//!   (`GridView`'s own test suite), out of scope for a unit scoped to
//!   `slivers_test.dart` — a future `grid_view_test.dart` port is its own
//!   unit, not "0 subject cases incidental to this one".
//! - **`widgets/animated_grid_test.dart`**, **`widgets/scroll_view_test.dart`**,
//!   **`widgets/nested_scroll_view_test.dart`**,
//!   **`widgets/scroll_cache_extent_test.dart`**,
//!   **`widgets/scrollable_restoration_test.dart`**, **`widgets/image_test.dart`**,
//!   **`material/flexible_space_bar_test.dart`**, **`rendering/viewport_test.dart`**
//!   — all use `SliverGrid`/`SliverGridDelegateWithFixedCrossAxisCount` purely
//!   as scene scaffolding for a different subject (`AnimatedGrid`,
//!   `CustomScrollView` mechanics, `NestedScrollView`, cache-extent behavior,
//!   scroll restoration, image caching, `FlexibleSpaceBar`, viewport
//!   mechanics). Incidental — 0 subject cases; verified by grepping each
//!   file's `testWidgets`/`test` titles for "SliverGrid" and finding none.
//! - **`widgets/scrollable_semantics_traversal_order_test.dart`** —
//!   `'Traversal Order of SliverGrid'` uses `SliverGrid` as scaffolding; the
//!   subject is semantics traversal order (parametrised identically across
//!   `SliverList`/`SliverFixedExtentList`/`SliverGrid` in that same file),
//!   not grid layout. Incidental, matching `sliver_fixed_extent_list_test.rs`'s
//!   identical classification of that file.
//!
//! # The headline architecture finding
//!
//! FLUI has a genuine LAZY grid render object — unlike `SliverFixedExtentList`
//! (fully eager, no lazy path at all), `SliverGrid` has both an eager widget
//! (`SliverGrid::new`, `crates/flui-widgets/src/scroll/sliver_grid.rs` →
//! `RenderSliverGrid`) AND a lazy one reachable through `GridView::builder`
//! (`crates/flui-widgets/src/scroll/grid_view.rs:120` → `SliverGridLazy` →
//! `RenderSliverGridLazy`, `crates/flui-objects/src/sliver/
//! sliver_grid_lazy.rs`, the subject of the just-merged builder-refresh fix).
//! `SliverGrid` itself (the bare widget in this crate) has no `.builder`/
//! `.list` distinction (confirmed: grepping `sliver_grid.rs` for `fn builder`/
//! `fn list` — zero hits); the lazy path is reached only via the composed
//! `GridView::builder`, not via `SliverGrid` directly.
//!
//! A SECOND, independent gap was found while porting the 4 cases that use an
//! arbitrary/custom per-child-geometry delegate — `_TestArbitrarySliverGridDelegate`/
//! `_TestArbitrarySliverGridLayout` (cases 4, 5, 6) or the analogous
//! `TestGridDelegate`/`TestGridLayout` (case 1): Flutter's `SliverGridLayout` (`rendering/sliver_grid.dart`)
//! is an `abstract class` whose load-bearing contract method is
//! `getGeometryForChildIndex(int) -> SliverGridGeometry` — arbitrary per-child
//! placement is a first-class capability (masonry/Pinterest-style irregular
//! grids). FLUI's `SliverGridLayout` (`crates/flui-rendering/src/delegates/
//! sliver_grid_delegate.rs`) is instead a concrete, regular-tile-only
//! `struct` — every per-child formula (`get_scroll_offset_of_child`,
//! `get_cross_axis_offset_of_child`, `get_min/max_child_index_for_scroll_offset`,
//! `compute_max_scroll_offset`) is a hardcoded inherent method deriving
//! `row = index / cross_axis_count`; `SliverGridDelegate::get_layout` returns
//! this struct BY VALUE with no trait-object or per-child override hook
//! anywhere (confirmed: an exhaustive grep for `get_geometry_for_child_index`/
//! `SliverGridGeometry` across the whole workspace — zero hits). No FLUI
//! `SliverGridDelegate`, custom or built-in, can express irregular placement
//! at all. Filed as a new Cross.H entry in `docs/ROADMAP.md` (see the ledger
//! below for which cases this blocks outright vs. which were still portable
//! by substituting a regular delegate for the incidental arbitrary one).
//!
//! A THIRD gap, unrelated to the two above, was found while writing the
//! `.builder` hit-test case (case 5 below): the lazy sparse-children
//! adaptor's `stamp_logical_index` (`crates/flui-view/src/element/
//! sparse_children.rs`) requires a builder-returned child to resolve its OWN
//! `render_id()` synchronously on insertion — true for a `RenderView` like
//! `Listener`, but not for a `StatefulView` like `GestureDetector`, which
//! trips that function's `debug_assert!` instead. Confirmed empirically:
//! swapping only the outer wrapper type (`GestureDetector` → `Listener`),
//! with everything else in the scene identical, flips the panic off. Also
//! filed as a new Cross.H entry.
//!
//! **A genuine production bug was found and fixed in this same pass** (not
//! a test-only workaround): both `RenderSliverGrid::perform_layout`
//! (`crates/flui-objects/src/sliver/sliver_grid.rs`) and
//! `RenderSliverGridLazy::perform_layout` (`.../sliver_grid_lazy.rs`) built
//! their committed `SliverGeometry` from `..SliverGeometry::ZERO` without
//! ever setting the `visible` field — every sibling sliver in this crate
//! (`RenderSliverFixedExtentList`, `RenderSliverPadding`,
//! `RenderSliverFillViewport`, `RenderSliverToBoxAdapter`, ...) sets
//! `visible: paint_extent > 0.0` explicitly, but both grid render objects
//! omitted it, so `visible` stayed `false` unconditionally. The viewport's
//! own hit-test walk gates on this
//! (`sliver_child_is_visible`/`hit_test_subtree_impl` in `flui-rendering/src/
//! pipeline/owner/accessors.rs`): a sliver whose geometry reports
//! `visible: false` is treated as un-hit-testable and the walk returns a
//! miss before ever recursing into its children. Consequence: **every**
//! `SliverGrid`/`GridView::builder` — with real, on-screen, painted content —
//! was silently un-hit-testable by any pointer event, in production, not
//! just in this harness; nothing before this port ever exercised a grid
//! sliver's hit-test path end-to-end to catch it (confirmed: an exhaustive
//! grep for prior `dispatch_pointer_*`/hit-test usage against `SliverGrid`
//! anywhere in the workspace — zero hits). Fixed by adding the same
//! `visible: paint_extent > 0.0` line both sibling render objects already
//! use; both hit-test cases below (5, 6) are the regression coverage this
//! fix did not have before.
//!
//! # Ledger (9 widget-level subject cases; recounted against the 9 bullets
//! immediately below — totals match)
//!
//! 1. `'Sliver grid can replace intermediate items'` — **out of scope, two
//!    independent reasons**: (a) `TestGridDelegate`/`TestGridLayout` return
//!    arbitrary per-index geometry (`crossAxisOffset: 20.0 + 20*index`, a
//!    constant `computeMaxScrollOffset`) — the same concrete-`SliverGridLayout`
//!    gap named above; (b) separately, the case's own reconciliation subject —
//!    `findChildIndexCallback` locating a child by its `ValueKey<int>` after
//!    the delegate swaps and inserts items mid-list — needs the still-open
//!    "no public per-item view-key API" gap already filed in `docs/ROADMAP.md`
//!    Cross.H by the `SliverList` port. Neither reason alone would block this
//!    case; both independently do.
//! 2. `'SliverGrid Correctly layout children after rearranging'` —
//!    **ported, real green**: [`sliver_grid_lays_out_children_in_order_after_rearranging`].
//!    Mirrors the oracle's `TestSliverGrid` helper exactly (a
//!    `CustomScrollView` over one eager 2-column `SliverGrid`); checks only
//!    final relative position after the second `pumpWidget`, matching the
//!    oracle's own `isRight`/`isBelow`/`sameHorizontal`/`sameVertical` helpers
//!    (`slivers_test.dart` lines 1656-1659) — no keyed cross-swap identity
//!    check, so the per-item-key gap above does not block this case.
//! 3. `'SliverGrid negative usableCrossAxisExtent'` — **ported, real green**:
//!    [`sliver_grid_negative_usable_cross_axis_extent_does_not_panic`]. A 4×4
//!    viewport with 8px cross/main spacing on a 2-column delegate drives
//!    `usable_cross_axis_extent` negative before
//!    `SliverGridDelegateWithFixedCrossAxisCount::get_layout`'s own
//!    `.max(0.0)` clamp; the oracle's only assertion is
//!    `tester.takeException()` is null — layout must complete, not panic.
//! 4. `'SliverGrid children can be arbitrarily placed'` — **out of scope**:
//!    needs `_TestArbitrarySliverGridDelegate`'s genuinely arbitrary
//!    placement (tiles occupying only a small, non-regular slice of a
//!    `SizedBox.expand` surface, with several "other places" taps
//!    deliberately landing in the resulting gaps) — the concrete-
//!    `SliverGridLayout` gap above. Unlike cases 5/6 below, this one's
//!    assertions are load-bearing on the exact irregular geometry (the
//!    "tapping empty space fires nothing" checks), so substituting a regular
//!    delegate would not reproduce the scenario the oracle is actually
//!    testing — no substitution attempted.
//! 5. `'SliverGrid.builder can build children'` — **ported, real green, with
//!    two documented substitutions**:
//!    [`sliver_grid_builder_hit_tests_children_by_position`]. Routes through
//!    `GridView::builder` (the lazy path — `SliverGrid` itself has no
//!    `.builder`). (a) The oracle's own delegate here is
//!    `_TestArbitrarySliverGridDelegate`, but this case's assertions are pure
//!    hit-test mutual-exclusion (unlike case 4, no "tap the gaps" checks), so
//!    a regular `SliverGridDelegateWithFixedCrossAxisCount` substitutes
//!    without weakening anything under test — same substitution precedent as
//!    `sliver_fixed_extent_list_test.rs`'s own `Viewport`-for-`CustomScrollView`
//!    swap. (b) `Listener` substitutes for `GestureDetector` — the THIRD gap
//!    named above (the lazy adaptor cannot host a `StatefulView`-typed
//!    builder-returned child). Tile centers are computed from the actual
//!    laid-out geometry, not hardcoded, mirroring
//!    `sliver_fixed_extent_list_test.rs`'s hit-test case. Mutation-checked:
//!    swapping the two tap coordinates flips the assertions red. This case,
//!    plus case 6, is also the first hit-test-level regression coverage the
//!    `visible`-field production bug fix (above) ever had.
//! 6. `'SliverGrid.list can display children'` — **ported, real green, same
//!    delegate substitution as case 5 (reason (a) only — this is the eager
//!    path, so `GestureDetector` works fine here; no `Listener` substitution
//!    needed)**: [`sliver_grid_list_hit_tests_children_by_position`]. Routes
//!    through the eager `SliverGrid::new` (Flutter's `.list` constructor is
//!    eager too).
//! 7. `'SliverGrid.list with empty children list'` — **already covered, not
//!    duplicated**: this file's pre-existing
//!    `sliver_grid_empty_children_renders_two_nodes` test (present before
//!    this port) already exercises an empty-children `SliverGrid` and asserts
//!    the viewport+sliver are both still mounted — the same observable
//!    outcome this oracle case checks (`find.byType(CustomScrollView)`
//!    `findsOneWidget`), just via render-node-count instead of a widget-type
//!    finder. No new test added for this case.
//! 8. `'SliverGrid.builder respects semanticIndexOffset'` — **out of
//!    scope**: no `IndexedSemantics`/`semanticIndexOffset` concept and no
//!    semantics-tree assertion capability exist anywhere in `flui-widgets`'
//!    headless harness — the same standing gap every other port in this
//!    directory that touches semantics already cites.
//! 9. `'SliverGridRegularTileLayout.computeMaxScrollOffset handles 0
//!    children'` — **ported, real green**:
//!    [`sliver_grid_builder_zero_items_reports_zero_max_scroll_extent`].
//!    Regression coverage for
//!    <https://github.com/flutter/flutter/issues/59663>, re-verified against
//!    both concrete `SliverGridDelegate` implementations FLUI ships
//!    (`SliverGridDelegateWithFixedCrossAxisCount`,
//!    `SliverGridDelegateWithMaxCrossAxisExtent`) via `GridView::builder` with
//!    zero items, matching the oracle's own two-part single case.
//!
//! **Total: 9 subject cases found = 9 accounted for above (5 ported green —
//! cases 2, 3, 5, 6, 9 — 1 already covered by a pre-existing test (case 7),
//! 2 out-of-scope-by-missing-API (cases 1, 4), 1 out-of-scope-by-standing-
//! semantics-gap (case 8)). No `#[ignore]`d pins in this
//! unit: every out-of-scope case is blocked by a construction that does not
//! compile at all (no arbitrary-geometry trait, no per-item key API, no
//! semantics tree) rather than one that compiles and diverges.**
//!
//! Render-level oracle (`rendering/sliver_cache_test.dart`'s
//! `'RenderSliverGrid calculates correct geometry'`, tag `3.44.0`): its
//! disposition is documented next to the existing `RenderSliverGrid`/
//! `RenderSliverGridLazy` rows in `render_object_harness.rs` rather than
//! repeated here — not portable, but for a test-harness reason, not a
//! production gap (no Cross.H entry follows from it; see that file for why).
//!
//! Geometry oracle (pre-existing, this port's own 2 tests, unchanged): with
//! `SliverGridDelegateWithFixedCrossAxisCount(2)` on an 800 × 600 viewport:
//! tile_width = 800 / 2 = 400 px, tile_height = 400 px (aspect_ratio = 1.0).
//! Two rows fit within the 600 px viewport height, so all four tiles are in
//! the visible band and receive a layout call.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_types::Color;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{
    Container, CustomScrollView, GestureDetector, GridView, Listener, ScrollController, SizedBox,
    SliverGrid, SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
    SliverGridDelegateWithMaxCrossAxisExtent, Text, Viewport,
};

use crate::common::LaidOut;
use crate::harness;

/// A 2-column `SliverGrid` with 4 eager children inside a `Viewport` builds
/// the correct render-node count.
///
/// Expected: 1 `RenderViewport` + 1 `RenderSliverGrid` + 4 tile
/// `RenderConstrainedBox` nodes = 6 total.
///
/// Flutter parity: `sliver.dart` `SliverGrid` over `RenderSliverGrid` —
/// eager children attached; delegate computes tile geometry.
#[test]
fn sliver_grid_two_columns_four_tiles_builds_six_render_nodes() {
    let delegate = Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2));
    // Tile children: SizedBox::shrink gives 0×0; the grid delegate overrides
    // their cross/main extents to 400×400 via tight sliver constraints.
    let tiles: Vec<BoxedView> = (0..4).map(|_| SizedBox::shrink().boxed()).collect();
    let root = Viewport::new((SliverGrid::new(delegate, tiles).boxed(),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        6,
        "SliverGrid(2 cols, 4 tiles): expected 6 render nodes \
         (1 RenderViewport + 1 RenderSliverGrid + 4 tile nodes)"
    );
}

/// An empty `SliverGrid` (no children) renders exactly 2 nodes.
///
/// Edge case: the grid delegate is valid but child_count is 0; the sliver
/// reports a zero scroll extent and the viewport renders nothing inside it.
#[test]
fn sliver_grid_empty_children_renders_two_nodes() {
    let delegate = Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(3));
    let root = Viewport::new((SliverGrid::new(delegate, Vec::<BoxedView>::new()).boxed(),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "empty SliverGrid: expected 2 render nodes (1 RenderViewport + 1 RenderSliverGrid)"
    );
}

// ============================================================================
// Shared scene builders
// ============================================================================

/// Mirrors the oracle's `TestSliverGrid` helper (`slivers_test.dart`): a
/// `CustomScrollView` over one eager, 2-column `SliverGrid` of bare `Text`
/// children.
fn two_column_grid_scene(labels: &[&str]) -> impl View {
    let delegate: Arc<dyn SliverGridDelegate> =
        Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2));
    let children: Vec<BoxedView> = labels
        .iter()
        .map(|&label| Text::new(label.to_string()).boxed())
        .collect();
    CustomScrollView::new((SliverGrid::new(delegate, children),))
}

/// The center point of `id`'s laid-out box, in absolute (viewport) pixels —
/// shared by the two hit-test cases below. Mirrors
/// `sliver_fixed_extent_list_test.rs`'s own `center_of` closure, factored out
/// here since two independent tests need it.
fn tile_center(laid: &LaidOut, id: RenderId) -> (f32, f32) {
    let offset = laid.absolute_offset(id);
    let extent = laid.size(id);
    (
        offset.dx.get() + extent.width.get() / 2.0,
        offset.dy.get() + extent.height.get() / 2.0,
    )
}

// ============================================================================
// CASE 2 — Correctly layout children after rearranging
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverGrid Correctly layout children
/// after rearranging'` (tag `3.44.0`).
///
/// Real, non-vacuous green: checks only relative position after the second
/// `pumpWidget`, mirroring the oracle's own `isRight`/`isBelow`/
/// `sameHorizontal`/`sameVertical` helpers exactly — no keyed cross-swap
/// identity assertion, so the still-open per-item-key gap does not block it.
#[test]
fn sliver_grid_lays_out_children_in_order_after_rearranging() {
    let mut laid = harness::pump_widget(
        two_column_grid_scene(&["item0", "item1"]),
        harness::screen(),
    );

    laid.pump_widget(two_column_grid_scene(&["item0", "item3", "item4", "item1"]));

    let item0 = laid
        .find_text("item0")
        .expect("'item0' must be mounted after the rearrange");
    let item3 = laid
        .find_text("item3")
        .expect("'item3' must be mounted after the rearrange");
    let item4 = laid
        .find_text("item4")
        .expect("'item4' must be mounted after the rearrange");
    let item1 = laid
        .find_text("item1")
        .expect("'item1' must be mounted after the rearrange");

    let dx = |id| laid.absolute_offset(id).dx.get();
    let dy = |id| laid.absolute_offset(id).dy.get();

    assert!(
        dx(item3) > dx(item0),
        "'item3' (row 0, col 1) must sit to 'item0's (row 0, col 0) right — oracle's `isRight`"
    );
    assert_eq!(
        dy(item3),
        dy(item0),
        "'item3' must share 'item0's row — oracle's `sameHorizontal`"
    );
    assert!(
        dy(item4) > dy(item0),
        "'item4' (row 1, col 0) must sit below 'item0' — oracle's `isBelow`"
    );
    assert_eq!(
        dx(item4),
        dx(item0),
        "'item4' must share 'item0's column — oracle's `sameVertical`"
    );
    assert!(
        dy(item1) > dy(item0),
        "'item1' (row 1, col 1) must sit below 'item0' — oracle's `isBelow`"
    );
    assert!(
        dx(item1) > dx(item0),
        "'item1' must also sit right of 'item0' — oracle's `isRight`"
    );
}

// ============================================================================
// CASE 3 — negative usableCrossAxisExtent
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverGrid negative
/// usableCrossAxisExtent'` (tag `3.44.0`).
///
/// A 4×4 viewport with 8px cross/main spacing on a 2-column delegate drives
/// `usable_cross_axis_extent` negative — `(4 - 8*(2-1)) = -4` — before
/// `SliverGridDelegateWithFixedCrossAxisCount::get_layout`'s own `.max(0.0)`
/// clamp. The oracle's sole assertion is `tester.takeException()` is null:
/// layout must complete, not panic. Real, non-vacuous green: an unclamped
/// negative extent would drive `BoxConstraints` construction into a
/// zero/negative-size panic during the tile layout pass.
#[test]
fn sliver_grid_negative_usable_cross_axis_extent_does_not_panic() {
    let delegate: Arc<dyn SliverGridDelegate> = Arc::new(
        SliverGridDelegateWithFixedCrossAxisCount::new(2)
            .with_cross_axis_spacing(8.0)
            .with_main_axis_spacing(8.0),
    );
    let children: Vec<BoxedView> = ["A", "B", "C", "D"]
        .iter()
        .map(|&label| Text::new(label.to_string()).boxed())
        .collect();
    let root = CustomScrollView::new((SliverGrid::new(delegate, children),));

    let laid = harness::pump_widget(root, harness::screen_of(4.0, 4.0));

    assert_eq!(
        laid.render_node_count(),
        6,
        "layout must complete and attach all 4 tiles despite the clamped \
         (would-be-negative) usable cross-axis extent: 1 RenderViewport + \
         1 RenderSliverGrid + 4 tile nodes"
    );
}

// ============================================================================
// CASE 5 — SliverGrid.builder can build children (hit-test, lazy path)
// ============================================================================

/// Mirrors the oracle's `.builder` tap scene (`_buildIndexedTapTarget`), with
/// **two** documented substitutions:
///
/// 1. A regular `SliverGridDelegateWithFixedCrossAxisCount` replaces the
///    oracle's `_TestArbitrarySliverGridDelegate` — see this file's module
///    doc: FLUI's `SliverGridDelegate::get_layout` returns a concrete,
///    regular-tile-only `SliverGridLayout` struct, so no arbitrary-placement
///    delegate is constructible at all. This case's assertions are pure
///    hit-test mutual exclusion (unlike case 4's "tap the gaps" checks), so
///    the substitution changes only where the two tiles land on screen, not
///    the hit-test behavior under test.
/// 2. `Listener` (a direct `RenderView`) replaces `GestureDetector` (a
///    `StatefulView`) as the tap target's outer wrapper. A SECOND, newly
///    surfaced gap: the lazy sparse-children adaptor's `stamp_logical_index`
///    (`crates/flui-view/src/element/sparse_children.rs`) asserts the
///    builder-returned child resolves its OWN `render_id()` synchronously
///    upon insertion — true for `Listener` (a `RenderView`) but not for
///    `GestureDetector` (a `StatefulView` whose element wraps a built
///    subtree), which trips that assertion instead of compiling into a
///    silently-wrong state. Confirmed empirically: swapping only the wrapper
///    type between the two, with everything else identical, flips this
///    scene from panicking to working. `on_pointer_down` (fired once per
///    dispatched pointer-down) substitutes for `on_tap` (fired once per
///    matched down+up pair); since every assertion below dispatches a
///    down+up pair per tap, the observed counts are identical either way.
///    Routes through `GridView::builder` since bare `SliverGrid` has no
///    `.builder` of its own.
fn grid_builder_tap_scene(counter0: Arc<AtomicUsize>, counter1: Arc<AtomicUsize>) -> impl View {
    let delegate: Arc<dyn SliverGridDelegate> =
        Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2));
    GridView::builder(delegate, 2, move |index| {
        let (label, color, counter) = match index {
            0 => ("Index 0", Color::rgb(0, 255, 0), Arc::clone(&counter0)),
            1 => ("Index 1", Color::rgb(255, 0, 0), Arc::clone(&counter1)),
            _ => return None,
        };
        Some(
            Listener::new()
                .on_pointer_down(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
                .child(
                    Container::new()
                        .color(color)
                        .child(Text::new(label.to_string())),
                )
                .boxed(),
        )
    })
}

/// Flutter parity: `slivers_test.dart` `'SliverGrid.builder can build
/// children'` (tag `3.44.0`).
///
/// Real, non-vacuous green: tapping tile 0's center fires ONLY counter 0;
/// tapping tile 1's center fires ONLY counter 1. Mutation-checked: swapping
/// the two tap coordinates flips this test red.
#[test]
fn sliver_grid_builder_hit_tests_children_by_position() {
    let counter0 = Arc::new(AtomicUsize::new(0));
    let counter1 = Arc::new(AtomicUsize::new(0));

    let mut laid = harness::pump_widget(
        grid_builder_tap_scene(Arc::clone(&counter0), Arc::clone(&counter1)),
        harness::screen(),
    );
    // Lazy grid tiles settle over 2 frames: the first services the build
    // requests `RenderSliverGridLazy::perform_layout` emits, the second lays
    // out the now-resident tiles (mirrors `lazy_grid.rs`'s settle pattern).
    laid.tick();
    laid.tick();

    let text0 = laid
        .find_text("Index 0")
        .expect("'Index 0' must be mounted after settling");
    let text1 = laid
        .find_text("Index 1")
        .expect("'Index 1' must be mounted after settling");
    let (x0, y0) = tile_center(&laid, text0);
    let (x1, y1) = tile_center(&laid, text1);

    laid.dispatch_pointer_down(x0, y0);
    laid.dispatch_pointer_up(x0, y0);
    assert_eq!(
        counter0.load(Ordering::SeqCst),
        1,
        "tapping tile 0's center must fire its own counter"
    );
    assert_eq!(
        counter1.load(Ordering::SeqCst),
        0,
        "tapping tile 0's center must NOT fire tile 1's counter"
    );

    laid.dispatch_pointer_down(x1, y1);
    laid.dispatch_pointer_up(x1, y1);
    assert_eq!(
        counter0.load(Ordering::SeqCst),
        1,
        "tapping tile 1's center must NOT fire tile 0's counter again"
    );
    assert_eq!(
        counter1.load(Ordering::SeqCst),
        1,
        "tapping tile 1's center must fire its own counter"
    );
}

// ============================================================================
// CASE 6 — SliverGrid.list can display children (hit-test, eager path)
// ============================================================================

/// Mirrors the oracle's `.list` tap scene (`_buildTapTarget`), substituting a
/// regular delegate for the same Cross.H reason `grid_builder_tap_scene`
/// documents above. Routes through the eager `SliverGrid::new` — Flutter's
/// `.list` constructor is eager too.
fn grid_list_tap_scene(counter0: Arc<AtomicUsize>, counter1: Arc<AtomicUsize>) -> impl View {
    let delegate: Arc<dyn SliverGridDelegate> =
        Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2));
    let first = GestureDetector::new()
        .on_tap(move || {
            counter0.fetch_add(1, Ordering::SeqCst);
        })
        .child(
            Container::new()
                .color(Color::rgb(0, 255, 0))
                .child(Text::new("First")),
        )
        .boxed();
    let second = GestureDetector::new()
        .on_tap(move || {
            counter1.fetch_add(1, Ordering::SeqCst);
        })
        .child(
            Container::new()
                .color(Color::rgb(255, 0, 0))
                .child(Text::new("Second")),
        )
        .boxed();
    CustomScrollView::new((SliverGrid::new(delegate, vec![first, second]),))
}

/// Flutter parity: `slivers_test.dart` `'SliverGrid.list can display
/// children'` (tag `3.44.0`).
///
/// Real, non-vacuous green, same shape as the `.builder` case above: tapping
/// 'First's center fires only counter 0; tapping 'Second's center fires only
/// counter 1.
#[test]
fn sliver_grid_list_hit_tests_children_by_position() {
    let counter0 = Arc::new(AtomicUsize::new(0));
    let counter1 = Arc::new(AtomicUsize::new(0));

    let laid = harness::pump_widget(
        grid_list_tap_scene(Arc::clone(&counter0), Arc::clone(&counter1)),
        harness::screen(),
    );

    let first = laid.find_text("First").expect("'First' must be mounted");
    let second = laid.find_text("Second").expect("'Second' must be mounted");
    let (x0, y0) = tile_center(&laid, first);
    let (x1, y1) = tile_center(&laid, second);

    laid.dispatch_pointer_down(x0, y0);
    laid.dispatch_pointer_up(x0, y0);
    assert_eq!(
        counter0.load(Ordering::SeqCst),
        1,
        "tapping 'First's center must fire its own counter"
    );
    assert_eq!(
        counter1.load(Ordering::SeqCst),
        0,
        "tapping 'First's center must NOT fire 'Second's counter"
    );

    laid.dispatch_pointer_down(x1, y1);
    laid.dispatch_pointer_up(x1, y1);
    assert_eq!(
        counter0.load(Ordering::SeqCst),
        1,
        "tapping 'Second's center must NOT fire 'First's counter again"
    );
    assert_eq!(
        counter1.load(Ordering::SeqCst),
        1,
        "tapping 'Second's center must fire its own counter"
    );
}

// ============================================================================
// CASE 9 — SliverGridRegularTileLayout.computeMaxScrollOffset handles 0 children
// ============================================================================

/// Flutter parity: `slivers_test.dart`
/// `'SliverGridRegularTileLayout.computeMaxScrollOffset handles 0 children'`
/// (tag `3.44.0`). Regression coverage for
/// <https://github.com/flutter/flutter/issues/59663>.
///
/// Real, non-vacuous green, re-verified against both concrete
/// `SliverGridDelegate` implementations FLUI ships, matching the oracle's own
/// two-part single case (fixed-cross-axis-count, then max-cross-axis-extent).
#[test]
fn sliver_grid_builder_zero_items_reports_zero_max_scroll_extent() {
    // SliverGridDelegateWithFixedCrossAxisCount, itemCount: 0.
    let controller = ScrollController::new();
    let delegate: Arc<dyn SliverGridDelegate> = Arc::new(
        SliverGridDelegateWithFixedCrossAxisCount::new(1)
            .with_main_axis_spacing(10.0)
            .with_child_aspect_ratio(2.1),
    );
    let mut laid = harness::pump_widget(
        GridView::builder(delegate, 0, |_| None).position(controller.position()),
        harness::screen(),
    );
    laid.tick();
    assert_eq!(
        controller.max_scroll_extent(),
        0.0,
        "an empty SliverGridDelegateWithFixedCrossAxisCount grid must report a \
         zero max scroll extent, not panic/NaN on the zero-children division"
    );

    // SliverGridDelegateWithMaxCrossAxisExtent, itemCount: 0.
    let controller = ScrollController::new();
    let delegate: Arc<dyn SliverGridDelegate> =
        Arc::new(SliverGridDelegateWithMaxCrossAxisExtent::new(30.0));
    let mut laid = harness::pump_widget(
        GridView::builder(delegate, 0, |_| None).position(controller.position()),
        harness::screen(),
    );
    laid.tick();
    assert_eq!(
        controller.max_scroll_extent(),
        0.0,
        "an empty SliverGridDelegateWithMaxCrossAxisExtent grid must also \
         report a zero max scroll extent"
    );
}
