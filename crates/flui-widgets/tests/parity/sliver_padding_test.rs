//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart` `SliverPadding`
//! - Render object: `packages/flutter/lib/src/rendering/sliver_padding.dart`
//!   `RenderSliverPadding` / `RenderSliverEdgeInsetsPadding`
//! - Tests: `packages/flutter/test/widgets/slivers_padding_test.dart` (tag
//!   `3.44.0`, 16 `testWidgets` cases)
//!
//! Widget → render-object mapping:
//! - `SliverPadding` → `RenderSliverPadding` (sliver child of `RenderViewport`
//!   or of another sliver, e.g. `SliverFillRemaining`)
//! - Sliver children → `RenderSliverToBoxAdapter` (`SliverToBoxAdapter`) or
//!   `RenderSliverFillRemaining` (`SliverFillRemaining`)
//! - Box leaves → `RenderConstrainedBox` (`SizedBox`) wrapping
//!   `RenderParagraph` (`Text`)
//!
//! Divergences:
//! - The axis-direction-sensitive cases (the basic geometry matrix, all four
//!   hit-testing directions, changing padding) mount [`Viewport`] directly
//!   instead of going through [`CustomScrollView`]: `CustomScrollView`
//!   hardcodes `axis_direction` from `scroll_direction`
//!   (`Axis::Vertical` → `TopToBottom`, `Axis::Horizontal` → `LeftToRight`
//!   only) and has no way to select `BottomToTop`/`RightToLeft`. `Viewport`
//!   is what `CustomScrollView::build` itself composes down to (a plain
//!   `RenderView`, no `Scrollable` involved either way), and it is also
//!   literally what Flutter's own oracle mounts directly — its shared
//!   `test()` helper builds a bare `Viewport(offset:, axisDirection:,
//!   slivers:)`, no `CustomScrollView` anywhere in this file. So this is the
//!   more faithful choice, not a workaround.
//! - Hit-testing is asserted by inspecting the hit-test path's `RenderId`s
//!   directly (`PipelineOwner::hit_test` → `HitTestResult::path()`, each
//!   entry's `target: RenderId`) rather than Flutter's `result.path.first
//!   .target is SomeType` / text-span dispatch. FLUI's `HitTestEntry` carries
//!   a data-typed `RenderId`, not a `dyn HitTestTarget`, so comparing against
//!   ids obtained from `find_text`/`find_by_render_type` is the idiomatic
//!   equivalent — and strictly more precise (exact node identity, not just
//!   type membership).
//! - "Hits nothing interactive" (Flutter: `result.path.first.target is
//!   RenderView`) is ported as "the hit path contains none of the three
//!   known content ids". FLUI's headless harness has no `RenderView`-
//!   equivalent sentinel node to assert identity against; the substituted
//!   assertion expresses the same intent (this position reaches no mounted
//!   content) through the mechanism the harness actually offers.
//! - `SliverPadding` accepts only a plain `EdgeInsets`, never an
//!   `EdgeInsetsGeometry`/`EdgeInsetsDirectional` resolved against ambient
//!   `Directionality` — see Out-of-scope below and the `docs/ROADMAP.md`
//!   Cross.H filing this port adds.
//! - Interactive (drag-gesture-driven) scrolling of a sliver `Viewport` is
//!   not wired in FLUI (`Viewport`'s own module doc: "interactive
//!   drag-to-scroll arrives with the `Scrollable`/`ScrollController`
//!   layer"); every scroll-position change in this file is programmatic
//!   (`Viewport::offset`/`CustomScrollView::offset`, or a root-swap to a new
//!   value), matching the pattern every sibling file in this directory
//!   already uses.
//!
//! Ported (11 upstream names, 9 Rust tests — all 9 new; 2 more upstream
//! names are already covered by pre-existing render-object unit tests, not
//! duplicated here):
//! - `'Viewport+SliverPadding basic test (VISUAL)'` —
//!   [`sliver_padding_viewport_basic_geometry_across_scroll_offsets`].
//! - `'Viewport+SliverPadding hit testing'` (down) —
//!   [`sliver_padding_hit_testing_down_routes_taps_to_child_or_gap`].
//! - `'Viewport+SliverPadding hit testing up'` —
//!   [`sliver_padding_hit_testing_up_routes_taps_to_child_or_gap`].
//! - `'Viewport+SliverPadding hit testing left'` —
//!   [`sliver_padding_hit_testing_left_routes_taps_to_child_or_gap`].
//! - `'Viewport+SliverPadding hit testing right'` —
//!   [`sliver_padding_hit_testing_right_routes_taps_to_child_or_gap`].
//! - `'Viewport+SliverPadding no child'` —
//!   [`sliver_padding_no_child_still_offsets_the_following_sliver`].
//! - `'SliverPadding with no child reports correct geometry as scroll
//!   offset changes'` —
//!   [`sliver_padding_no_child_paint_extent_tracks_scroll_offset`].
//! - `'Viewport+SliverPadding changing padding'` —
//!   [`sliver_padding_changing_padding_repositions_the_following_sliver`].
//! - `'SliverPadding includes preceding padding in the
//!   precedingScrollExtent provided to child'` —
//!   [`sliver_padding_includes_preceding_padding_in_child_constraints`].
//! - `"SliverPadding consumes only its padding from the overlap of its
//!   parent's constraints"` — covered elsewhere: pre-existing
//!   `crates/flui-objects/src/sliver/sliver_padding.rs`
//!   `tests::child_constraints_reduce_positive_overlap_by_before_paint_padding`
//!   asserts the identical formula (positive overlap reduced by
//!   `beforePaddingPaintExtent`, not reset to zero) — different literal
//!   magnitudes, same invariant. That test predates this port; it is cited
//!   here, not duplicated, because it already IS this oracle case at the
//!   render-object level, which is the level this specific scenario is
//!   actually pitched at (the oracle constructs a raw `RenderSliverPadding`
//!   with a raw `SliverConstraints`, no widget tree at all).
//! - `"SliverPadding passes the overlap to the child if it's negative"` —
//!   covered elsewhere: pre-existing
//!   `tests::child_constraints_negative_overlap_passthrough` asserts the
//!   identical formula (negative overlap passes through unchanged).
//!
//! Out of scope (5 upstream names):
//! - `'Viewport+SliverPadding basic test (LTR)'` — exercises
//!   `EdgeInsetsDirectional.fromSTEB(...)`; `SliverPadding` has no
//!   `EdgeInsetsGeometry`/directional-insets API at all (see the
//!   `docs/ROADMAP.md` Cross.H filing this port adds). Even though LTR's
//!   *resolved* numbers happen to coincide with case 1's plain `EdgeInsets`,
//!   porting it as such would silently exercise the wrong code path (never
//!   touching directional resolution, since none exists) — exactly the
//!   "narrow a scenario to green" this studio rejects — so it is left
//!   unported instead.
//! - `'Viewport+SliverPadding basic test (RTL)'` — same
//!   `EdgeInsetsDirectional` gap; here the resolved numbers genuinely differ
//!   from LTR (left inset 15.0 vs 25.0), so there is not even an accidental
//!   numeric match to lean on.
//! - `'Viewport+SliverPadding changing direction'` — asserts
//!   `RenderSliverPadding.afterPadding` directly. FLUI's
//!   `RenderSliverPadding` has no public before/after-padding accessor
//!   (`resolve()` is a private helper) — there is nothing to read from
//!   outside the crate. The underlying axis-direction → inset mapping this
//!   accessor exposes is unit-tested for `TopToBottom` (forward and
//!   reverse-growth) and `LeftToRight` in
//!   `crates/flui-objects/src/sliver/sliver_padding.rs`
//!   (`resolve_picks_per_axis_padding_correctly`,
//!   `child_position_helpers_use_leading_padding`,
//!   `child_position_helpers_use_reverse_growth_leading_padding`) but NOT
//!   for `RightToLeft` (Flutter's `AxisDirection.left`) — a real, narrower
//!   coverage gap that closing would need touching that file, which is out
//!   of this task's edit scope.
//! - `'SliverPadding propagates geometry offset corrections'` — requires
//!   drag-gesture-driven interactive scrolling of a `SliverList` through a
//!   real `Scrollable` hosting a sliver `Viewport`; FLUI has no such wiring
//!   today (see the Divergences note above) — the same known limitation
//!   every sibling sliver parity file in this directory works around by
//!   using only programmatic offsets.
//! - `'SliverPadding passes the paintOrigin of the child on'` — the oracle
//!   constructs a raw `RenderSliverPadding` and a mock `_MockRenderSliver`
//!   child directly (bypassing the widget tree entirely) to force a nonzero
//!   child `paintOrigin`. This is a render-object-level unit test, not a
//!   widget-mount scenario; the widget-mount harness ([`LaidOut`]) this file
//!   uses has no way to inject a mock sliver child, and adding this
//!   coverage the way the oracle does would mean editing
//!   `crates/flui-objects/src/sliver/sliver_padding.rs`, which is out of
//!   this task's edit scope. The `paint_origin` passthrough itself is a
//!   one-line assignment in `RenderSliverPadding::padded_geometry`
//!   (`paint_origin: child_geometry.paint_origin`) with no existing
//!   regression coverage — a real, narrow gap, left open here.
//!
//! Content sweep (`git grep -l SliverPadding` at tag `3.44.0` across
//! `packages/flutter/test/`, beyond the oracle file above): 12 files. Two
//! (`material/app_bar_sliver_test.dart`, `material/scaffold_test.dart`) are
//! Material-library tests — out of the corpus (FLUI has no Material parity
//! program yet). Of the remaining ten, nine use `SliverPadding` only as
//! incidental scaffolding for a different subject —
//! `rendering/debug_test.dart` (`debugPaintPadding` diagnostics
//! visualization), `rendering/viewport_test.dart`
//! (`Viewport.getOffsetToReveal`), `widgets/box_sliver_mismatch_test.dart`
//! (the box/sliver mismatch error), `widgets/decorated_sliver_test.dart`
//! (`DecoratedSliver`), `widgets/keep_alive_test.dart` (`SliverPadding`
//! appears only inside an expected diagnostics-dump string literal),
//! `widgets/layout_builder_mutations_test.dart` /
//! `widgets/layout_builder_test.dart` (`LayoutBuilder`),
//! `widgets/nested_scroll_view_test.dart` (`NestedScrollView`), and
//! `widgets/slivers_evil_test.dart` (a general multi-sliver stress test,
//! `'Evil test of sliver features - 1'`) — none of these test
//! `SliverPadding`'s own behavior. The tenth,
//! `rendering/sliver_cache_test.dart`'s `'RenderSliverPadding calculates
//! correct geometry'`, IS `SliverPadding`-subject: it constructs 30 raw
//! `RenderSliverPadding` instances directly inside a raw `RenderViewport`
//! with a 250px cache extent and asserts per-node `SliverConstraints` /
//! attachment as the cache window scrolls. Out of scope for the same reason
//! as the `paintOrigin` case above — a render-object-level construction test
//! (bypasses the widget tree, needs a raw multi-node `RenderViewport` plus
//! cache-extent/attachment introspection the widget-mount harness does not
//! expose), not a widget-level scenario this file's harness can express.

use flui_foundation::RenderId;
use flui_geometry::{EdgeInsets, px};
use flui_rendering::hit_testing::HitTestResult;
use flui_types::layout::AxisDirection;
use flui_types::{Offset, Size};
use flui_view::View;
use flui_widgets::{
    CustomScrollView, SizedBox, SliverFillRemaining, SliverPadding, SliverToBoxAdapter, Text,
    Viewport,
};

use crate::common::{LaidOut, offset, size};
use crate::harness;

/// Builds the oracle's shared `test()` scene: a `before`/`padded`/`after`
/// sequence of three 400×400 `SizedBox`es, the middle one wrapped in
/// `SliverPadding(padding)`, inside a `Viewport` scrolled to `offset_px`
/// along `axis_direction`.
///
/// Flutter parity: `slivers_padding_test.dart`'s own `test()` helper, shared
/// by the basic-geometry and hit-testing cases below.
fn padding_scene(offset_px: f32, padding: EdgeInsets, axis_direction: AxisDirection) -> impl View {
    Viewport::new((
        SliverToBoxAdapter::new().child(SizedBox::new(400.0, 400.0).child(Text::new("before"))),
        SliverPadding::new(padding).child(
            SliverToBoxAdapter::new().child(SizedBox::new(400.0, 400.0).child(Text::new("padded"))),
        ),
        SliverToBoxAdapter::new().child(SizedBox::new(400.0, 400.0).child(Text::new("after"))),
    ))
    .axis_direction(axis_direction)
    .offset(offset_px)
}

/// Reads the current `(absolute_offset, size)` rect of the `before`/
/// `padded`/`after` text boxes and compares them against the expected
/// triple — the Rust equivalent of the oracle's `verify(tester, answerKey)`.
///
/// Reads the *paragraph's* geometry, not the enclosing `SizedBox`'s: the
/// `SizedBox` forces tight constraints on its `Text` child, so both report
/// the identical committed box (same size, same absolute offset) — the
/// paragraph is just what `find_text` can locate.
fn assert_scene_rects(
    laid: &LaidOut,
    before: (Offset, Size),
    padded: (Offset, Size),
    after: (Offset, Size),
) {
    let before_id = laid
        .find_text("before")
        .expect("'before' paragraph mounted");
    let padded_id = laid
        .find_text("padded")
        .expect("'padded' paragraph mounted");
    let after_id = laid.find_text("after").expect("'after' paragraph mounted");

    assert_eq!(laid.absolute_offset(before_id), before.0, "'before' origin");
    assert_eq!(laid.size(before_id), before.1, "'before' size");
    assert_eq!(laid.absolute_offset(padded_id), padded.0, "'padded' origin");
    assert_eq!(laid.size(padded_id), padded.1, "'padded' size");
    assert_eq!(laid.absolute_offset(after_id), after.0, "'after' origin");
    assert_eq!(laid.size(after_id), after.1, "'after' size");
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding
/// basic test (VISUAL)'` (tag `3.44.0`) — plain (non-directional)
/// `EdgeInsets` padding around a sliver inside a raw `Viewport`, checked at
/// five scroll offsets, including one that scrolls the padded content fully
/// past the trailing sliver.
#[test]
fn sliver_padding_viewport_basic_geometry_across_scroll_offsets() {
    // EdgeInsets.fromLTRB(25.0, 20.0, 15.0, 35.0): left=25, top=20, right=15, bottom=35.
    let padding = EdgeInsets::new(px(20.0), px(15.0), px(35.0), px(25.0));

    let mut laid = harness::pump_widget(
        padding_scene(0.0, padding, AxisDirection::TopToBottom),
        harness::screen(),
    );
    assert_scene_rects(
        &laid,
        (offset(0.0, 0.0), size(800.0, 400.0)),
        (offset(25.0, 420.0), size(760.0, 400.0)),
        (offset(0.0, 855.0), size(800.0, 400.0)),
    );

    laid.pump_widget(padding_scene(200.0, padding, AxisDirection::TopToBottom));
    assert_scene_rects(
        &laid,
        (offset(0.0, -200.0), size(800.0, 400.0)),
        (offset(25.0, 220.0), size(760.0, 400.0)),
        (offset(0.0, 655.0), size(800.0, 400.0)),
    );

    laid.pump_widget(padding_scene(390.0, padding, AxisDirection::TopToBottom));
    assert_scene_rects(
        &laid,
        (offset(0.0, -390.0), size(800.0, 400.0)),
        (offset(25.0, 30.0), size(760.0, 400.0)),
        (offset(0.0, 465.0), size(800.0, 400.0)),
    );

    laid.pump_widget(padding_scene(490.0, padding, AxisDirection::TopToBottom));
    assert_scene_rects(
        &laid,
        (offset(0.0, -490.0), size(800.0, 400.0)),
        (offset(25.0, -70.0), size(760.0, 400.0)),
        (offset(0.0, 365.0), size(800.0, 400.0)),
    );

    laid.pump_widget(padding_scene(10000.0, padding, AxisDirection::TopToBottom));
    assert_scene_rects(
        &laid,
        (offset(0.0, -10000.0), size(800.0, 400.0)),
        (offset(25.0, -9580.0), size(760.0, 400.0)),
        (offset(0.0, -9145.0), size(800.0, 400.0)),
    );
}

/// The `padding_scene` mounted for hit-testing, plus the ids the four
/// directional tests below all probe: `EdgeInsets.all(30.0)`, scrolled to
/// 350.0, matching every `'... hit testing ...'` oracle case.
struct HitTestScene {
    laid: LaidOut,
    before: RenderId,
    padded: RenderId,
    after: RenderId,
    sliver_padding: RenderId,
}

impl HitTestScene {
    fn mount(axis_direction: AxisDirection) -> Self {
        let laid = harness::pump_widget(
            padding_scene(350.0, EdgeInsets::all(px(30.0)), axis_direction),
            harness::screen(),
        );
        let before = laid
            .find_text("before")
            .expect("'before' paragraph mounted");
        let padded = laid
            .find_text("padded")
            .expect("'padded' paragraph mounted");
        let after = laid.find_text("after").expect("'after' paragraph mounted");
        let sliver_padding = laid.find_by_render_type("RenderSliverPadding");
        Self {
            laid,
            before,
            padded,
            after,
            sliver_padding,
        }
    }

    /// Hit-tests at root-local `(x, y)` and returns the leaf-first path of
    /// hit `RenderId`s — the same `PipelineOwner::hit_test` primitive
    /// `common::LaidOut::route_event` dispatches through, but returning the
    /// raw path instead of routing an event, so this file can assert on
    /// WHICH ids were hit (matching the oracle's `result.path` inspection).
    fn hit(&self, x: f32, y: f32) -> Vec<RenderId> {
        self.laid.enter_owner_scope(|| {
            let owner = self.laid.pipeline_owner();
            let owner = owner.read();
            let mut result = HitTestResult::new();
            owner.hit_test(Offset::new(px(x), px(y)), &mut result);
            result.path().iter().map(|entry| entry.target).collect()
        })
    }

    /// Whether `(x, y)` hits none of the three known content boxes AND not
    /// the `RenderSliverPadding` itself — the substitute for Flutter's
    /// `result.path.first.target is RenderView`, which pins that NOTHING
    /// registers in a padding gap, the padding sliver included (a
    /// self-hitting `RenderSliverPadding` would pass a contents-only check
    /// while failing the oracle's assertion). See the module-level
    /// Divergences note.
    fn hits_no_content(&self, x: f32, y: f32) -> bool {
        let path = self.hit(x, y);
        !path.contains(&self.before)
            && !path.contains(&self.padded)
            && !path.contains(&self.after)
            && !path.contains(&self.sliver_padding)
    }
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding hit
/// testing'` (tag `3.44.0`) — `AxisDirection.down`.
#[test]
fn sliver_padding_hit_testing_down_routes_taps_to_child_or_gap() {
    let scene = HitTestScene::mount(AxisDirection::TopToBottom);

    assert!(
        scene.hit(10.0, 10.0).contains(&scene.before),
        "(10, 10) should hit 'before'"
    );
    assert!(
        scene.hits_no_content(10.0, 60.0),
        "(10, 60) is the leading padding gap — should hit no content box"
    );

    let padded_hit = scene.hit(100.0, 100.0);
    assert!(
        padded_hit.contains(&scene.padded),
        "(100, 100) should hit 'padded'"
    );
    assert!(
        padded_hit.contains(&scene.sliver_padding),
        "the hit path through 'padded' must include RenderSliverPadding itself"
    );

    assert!(
        scene.hits_no_content(100.0, 490.0),
        "(100, 490) is the trailing padding gap — should hit no content box"
    );
    assert!(
        scene.hit(10.0, 520.0).contains(&scene.after),
        "(10, 520) should hit 'after'"
    );
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding hit
/// testing up'` (tag `3.44.0`) — `AxisDirection.up`
/// (FLUI `AxisDirection::BottomToTop`).
#[test]
fn sliver_padding_hit_testing_up_routes_taps_to_child_or_gap() {
    let scene = HitTestScene::mount(AxisDirection::BottomToTop);

    assert!(
        scene.hit(10.0, 590.0).contains(&scene.before),
        "(10, 590) should hit 'before'"
    );
    assert!(
        scene.hits_no_content(10.0, 540.0),
        "(10, 540) is the leading padding gap — should hit no content box"
    );

    let padded_hit = scene.hit(100.0, 500.0);
    assert!(
        padded_hit.contains(&scene.padded),
        "(100, 500) should hit 'padded'"
    );
    assert!(
        padded_hit.contains(&scene.sliver_padding),
        "the hit path through 'padded' must include RenderSliverPadding itself"
    );

    assert!(
        scene.hits_no_content(100.0, 110.0),
        "(100, 110) is the trailing padding gap — should hit no content box"
    );
    assert!(
        scene.hit(10.0, 80.0).contains(&scene.after),
        "(10, 80) should hit 'after'"
    );
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding hit
/// testing left'` (tag `3.44.0`) — `AxisDirection.left`
/// (FLUI `AxisDirection::RightToLeft`).
#[test]
fn sliver_padding_hit_testing_left_routes_taps_to_child_or_gap() {
    let scene = HitTestScene::mount(AxisDirection::RightToLeft);

    assert!(
        scene.hit(790.0, 10.0).contains(&scene.before),
        "(790, 10) should hit 'before'"
    );
    assert!(
        scene.hits_no_content(740.0, 10.0),
        "(740, 10) is the leading padding gap — should hit no content box"
    );

    let padded_hit = scene.hit(700.0, 100.0);
    assert!(
        padded_hit.contains(&scene.padded),
        "(700, 100) should hit 'padded'"
    );
    assert!(
        padded_hit.contains(&scene.sliver_padding),
        "the hit path through 'padded' must include RenderSliverPadding itself"
    );

    assert!(
        scene.hits_no_content(310.0, 100.0),
        "(310, 100) is the trailing padding gap — should hit no content box"
    );
    assert!(
        scene.hit(280.0, 10.0).contains(&scene.after),
        "(280, 10) should hit 'after'"
    );
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding hit
/// testing right'` (tag `3.44.0`) — `AxisDirection.right`
/// (FLUI `AxisDirection::LeftToRight`).
#[test]
fn sliver_padding_hit_testing_right_routes_taps_to_child_or_gap() {
    let scene = HitTestScene::mount(AxisDirection::LeftToRight);

    assert!(
        scene.hit(10.0, 10.0).contains(&scene.before),
        "(10, 10) should hit 'before'"
    );
    assert!(
        scene.hits_no_content(60.0, 10.0),
        "(60, 10) is the leading padding gap — should hit no content box"
    );

    let padded_hit = scene.hit(100.0, 100.0);
    assert!(
        padded_hit.contains(&scene.padded),
        "(100, 100) should hit 'padded'"
    );
    assert!(
        padded_hit.contains(&scene.sliver_padding),
        "the hit path through 'padded' must include RenderSliverPadding itself"
    );

    assert!(
        scene.hits_no_content(490.0, 100.0),
        "(490, 100) is the trailing padding gap — should hit no content box"
    );
    assert!(
        scene.hit(520.0, 10.0).contains(&scene.after),
        "(520, 10) should hit 'after'"
    );
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding no
/// child'` (tag `3.44.0`) — a childless `SliverPadding` still consumes its
/// own padded scroll extent, so a following sliver is offset past it.
#[test]
fn sliver_padding_no_child_still_offsets_the_following_sliver() {
    let root = CustomScrollView::new((
        SliverPadding::new(EdgeInsets::all(px(100.0))),
        SliverToBoxAdapter::new().child(SizedBox::new(400.0, 400.0).child(Text::new("x"))),
    ));
    let laid = harness::pump_widget(root, harness::screen());

    let x = laid.find_text("x").expect("'x' paragraph mounted");
    assert_eq!(laid.absolute_offset(x), offset(0.0, 200.0));
}

/// Flutter parity: `slivers_padding_test.dart` `'SliverPadding with no
/// child reports correct geometry as scroll offset changes'` (tag
/// `3.44.0`) — regression test for
/// <https://github.com/flutter/flutter/issues/64506>: a childless
/// `SliverPadding`'s committed `paint_extent` must track a LIVE scroll
/// offset change, not stay pinned to its first-layout value.
#[test]
fn sliver_padding_no_child_paint_extent_tracks_scroll_offset() {
    fn scene(offset_px: f32) -> impl View {
        CustomScrollView::new((
            SliverPadding::new(EdgeInsets::all(px(100.0))),
            SliverToBoxAdapter::new().child(SizedBox::new(400.0, 400.0).child(Text::new("x"))),
        ))
        .offset(offset_px)
    }

    let mut laid = harness::pump_widget(scene(0.0), harness::screen());
    let x = laid.find_text("x").expect("'x' paragraph mounted");
    assert_eq!(laid.absolute_offset(x), offset(0.0, 200.0));
    let sliver_padding = laid.find_by_render_type("RenderSliverPadding");
    assert_eq!(laid.sliver_geometry(sliver_padding).paint_extent, 200.0);

    laid.pump_widget(scene(50.0));
    let sliver_padding = laid.find_by_render_type("RenderSliverPadding");
    assert_eq!(
        laid.sliver_geometry(sliver_padding).paint_extent,
        150.0,
        "paint_extent must track the live scroll offset, not stay pinned to its first-layout value"
    );
}

/// Flutter parity: `slivers_padding_test.dart` `'Viewport+SliverPadding
/// changing padding'` (tag `3.44.0`) — a childless `SliverPadding` ahead of
/// a `SliverToBoxAdapter` shifts where the adapter's child lands; swapping
/// the padding value repositions it on the next frame.
#[test]
fn sliver_padding_changing_padding_repositions_the_following_sliver() {
    fn scene(padding: EdgeInsets) -> impl View {
        Viewport::new((
            SliverPadding::new(padding),
            SliverToBoxAdapter::new().child(SizedBox::width(201.0).child(Text::new("x"))),
        ))
        .axis_direction(AxisDirection::RightToLeft)
        .offset(0.0)
    }

    // EdgeInsets.fromLTRB(90.0, 1.0, 110.0, 2.0): left=90, top=1, right=110, bottom=2.
    let padding1 = EdgeInsets::new(px(1.0), px(110.0), px(2.0), px(90.0));
    let mut laid = harness::pump_widget(scene(padding1), harness::screen());
    let x = laid.find_text("x").expect("'x' paragraph mounted");
    assert_eq!(laid.absolute_offset(x), offset(399.0, 0.0));

    // EdgeInsets.fromLTRB(110.0, 1.0, 80.0, 2.0): left=110, top=1, right=80, bottom=2.
    let padding2 = EdgeInsets::new(px(1.0), px(80.0), px(2.0), px(110.0));
    laid.pump_widget(scene(padding2));
    let x = laid
        .find_text("x")
        .expect("'x' paragraph mounted after the padding swap");
    assert_eq!(laid.absolute_offset(x), offset(409.0, 0.0));
}

/// Flutter parity: `slivers_padding_test.dart` `'SliverPadding includes
/// preceding padding in the precedingScrollExtent provided to child'` (tag
/// `3.44.0`) — regression test for
/// <https://github.com/flutter/flutter/issues/49195>: a leading
/// `SliverPadding` must count toward the `preceding_scroll_extent` a
/// `SliverFillRemaining` child receives, shrinking how much of the viewport
/// it fills.
#[test]
fn sliver_padding_includes_preceding_padding_in_child_constraints() {
    let root = CustomScrollView::new((SliverPadding::new(EdgeInsets::only_top(px(30.0)))
        .child(SliverFillRemaining::new().child(SizedBox::shrink())),));
    let laid = harness::pump_widget(root, harness::screen());

    let filler = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(filler).height,
        px(570.0),
        "SliverFillRemaining must fill only the 570px left after the 30px leading padding \
         (viewport height 600 - 30), matching Flutter's precedingScrollExtent plumbing"
    );
}
