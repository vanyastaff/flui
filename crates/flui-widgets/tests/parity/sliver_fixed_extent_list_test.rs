//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart` `SliverFixedExtentList`
//!   (`.list`/`.builder` named constructors over `SliverChildListDelegate`/
//!   `SliverChildBuilderDelegate`).
//! - Render object: `packages/flutter/lib/src/rendering/sliver_fixed_extent_list.dart`
//!   `RenderSliverFixedExtentList` (extends `RenderSliverFixedExtentBoxAdaptor`
//!   extends `RenderSliverMultiBoxAdaptor`).
//! - Tests (widget level): `packages/flutter/test/widgets/slivers_test.dart`
//!   (tag `3.44.0`) — a SHARED 38-`testWidgets`-case multi-subject file
//!   (`Viewport`, `SliverList`, `SliverGrid`, `SliverOffstage`, and this
//!   widget all live in it). **This port scopes itself strictly to the
//!   `SliverFixedExtentList`-subject cases** (enumerated below); the
//!   `SliverList`/`SliverGrid`/`Viewport`/offstage subjects are separate,
//!   already-ported-elsewhere or future units — same scoping convention
//!   `sliver_list_test.rs`'s own module doc uses for this exact file.
//!
//! # Content sweep (standing rule — run before naming a subject list)
//!
//! `git grep -l "SliverFixedExtentList" 3.44.0 -- packages/flutter/test/`
//! hits 12 files. Classified:
//!
//! - **`slivers_test.dart`** — 9 genuine `SliverFixedExtentList`-subject
//!   `testWidgets` cases (this file's scope; ledger below). One of the 9 is
//!   a naming trap the first pass of this sweep missed: the case at line
//!   1387, titled `'SliverList.list can build children'` (there are two
//!   identically-named cases in this file — the other, at line 1223, is a
//!   genuine `SliverList` case, out of this port's scope), constructs
//!   `SliverFixedExtentList.list(itemExtent: 100, children: [...])` in its
//!   BODY — a copy-paste artifact in the oracle itself. Caught only by
//!   reading every candidate's body, not by titles/`grep`-ing test names —
//!   the discipline this module doc's own "content sweep" section exists to
//!   enforce.
//! - **`rendering/sliver_cache_test.dart`** — 1 genuine subject case,
//!   `'RenderSliverFixedExtentList calculates correct geometry'` — but it
//!   lives in a file this task did not assign (the assigned render-level
//!   oracle is `rendering/sliver_fixed_extent_layout_test.dart` only).
//!   Left unaccounted for, same as `sliver_list_test.rs` left
//!   `slivers_test.dart`'s own `SliverList` cases for a future unit —
//!   flagged here, not silently dropped.
//! - **`widgets/scrollable_semantics_traversal_order_test.dart`** —
//!   `'Traversal Order of SliverFixedExtentList'` uses
//!   `SliverFixedExtentList.list` as scaffolding; the subject under test is
//!   semantics traversal order (parametrised identically across
//!   `SliverList`/`SliverFixedExtentList`/`SliverGrid` in that same file),
//!   not fixed-extent layout. Incidental.
//! - **`gestures/gesture_config_regression_test.dart`**,
//!   **`material/sliver_app_bar_test.dart`**, **`widgets/keep_alive_test.dart`**,
//!   **`widgets/nested_scroll_view_test.dart`**,
//!   **`widgets/reorderable_list_test.dart`**,
//!   **`widgets/sliver_fill_remaining_test.dart`**,
//!   **`widgets/sliver_persistent_header_test.dart`**,
//!   **`widgets/slivers_evil_test.dart`** — all use
//!   `SliverFixedExtentList`/`.builder`/`.list` purely as scene scaffolding
//!   for a different subject (`ScrollConfiguration`, `SliverAppBar`,
//!   `KeepAlive`, `NestedScrollView`, `SliverReorderableList`,
//!   `SliverFillRemaining`, persistent-header stretch/show-on-screen
//!   behavior, general sliver-removal robustness). Incidental — 0 subject
//!   cases.
//!
//! Render-level oracle (`rendering/sliver_fixed_extent_layout_test.dart`,
//! tag `3.44.0`, verified to exist): 16 `test(...)` cases total. One
//! (`'RenderSliverFixedExtentList layout test - rounding error'`) is
//! misleadingly named — its body constructs
//! `childManager.createRenderSliverFillViewport()`, a sibling class, not our
//! subject; incidental despite the name. Three more
//! (`'Implements paintsChild correctly'` and both
//! `'RenderSliverFillViewport correctly references itemExtent, ...'` cases)
//! likewise exercise `RenderSliverFillViewport`, not
//! `RenderSliverFixedExtentList`. The remaining 12 genuinely exercise
//! `RenderSliverFixedExtentList`/its abstract base
//! `RenderSliverFixedExtentBoxAdaptor`: the 9-case
//! `group('getMaxChildIndexForScrollOffset')`, the two
//! `'RenderSliverFixedExtentList correctly references itemExtent, ...'`
//! cases, and `'RenderSliverMultiBoxAdaptor has calculate leading and
//! trailing garbage'` (constructed via `createRenderSliverFixedExtentList`).
//!
//! # Status
//!
//! `SliverFixedExtentList` is lazy: its children are a delegate served by
//! index through the one lazy multi-box adaptor (ADR-0053), so only the
//! viewport's cache window is built and everything outside it is evicted —
//! the same lifecycle `SliverList` and the lazy grid have. The render object
//! carries Flutter's index math (`crates/flui-objects/src/sliver/
//! sliver_fixed_extent_list.rs`, whose unit tests port the render-level
//! oracle's index cases). What it deliberately does NOT carry is
//! `scrollOffsetCorrection`: a source that shrinks under the viewport clamps
//! the count and the viewport clamps its pixels (the clamp contract, ADR-0053
//! decision 3), which is the divergence cases 4 and 5 below record.
//!
//! # Ledger (9 widget-level subject cases)
//!
//! 1. `'SliverFixedExtentList correctly clears garbage'` — **ported**:
//!    [`sliver_fixed_extent_list_clears_garbage_across_a_head_insert`].
//!    `tester.drag` substitutes with `jump_to` (the standing Finding-3
//!    substitution in this directory); FLUI has no `AutomaticKeepAlive`, so
//!    the oracle's kept-alive leading items are simply evicted — its own
//!    assertions (`findsNothing` for them, since `find.text` skips offstage)
//!    read the same either way.
//! 2. `'SliverFixedExtentList handles underflow when its children changes'`
//!    — **ported in two halves**:
//!    [`sliver_fixed_extent_list_offscreen_children_are_not_built_on_initial_window`]
//!    (residency at the settled tail: the items above the window are never
//!    built) and
//!    [`sliver_fixed_extent_list_shrinking_children_clamps_the_position`]
//!    (the children shrink under the viewport; the position clamps to the
//!    new extent and only the remaining tail is built).
//! 3. `'SliverFixedExtentList Correctly layout children after rearranging'`
//!    — **ported**:
//!    [`sliver_fixed_extent_list_lays_out_children_in_order_after_rearranging`].
//!    Its assertions check final presence and relative order only (re-read,
//!    not assumed from the name: no `initState` spy, no identity check), so
//!    a positional reconcile is a faithful port.
//! 4. `'SliverFixedExtentList with SliverChildBuilderDelegate auto-correct scroll
//!    offset - super fast'` — **ported as the clamp contract**: [`sliver_fixed_extent_list_far_jump_past_the_end_clamps_to_the_real_extent`].
//!    Flutter discovers the builder's end by bisecting the delegate and
//!    corrects the offset in one frame; FLUI discovers it by the requests
//!    the window makes (the first `None` clamps the count) and the viewport
//!    clamps the pixels — the same settled offset (`7 × 200 − 600 = 800`)
//!    over a few layout passes instead of one. Recorded divergence.
//! 5. `'SliverFixedExtentList with SliverChildBuilderDelegate auto-correct scroll
//!    offset - reasonable'` — **ported as the clamp contract**: [`sliver_fixed_extent_list_overscroll_past_the_end_clamps_to_the_real_extent`].
//!    Flutter animates the over-scroll back; FLUI clamps. Same settled
//!    offset (800), no animation.
//! 6. `'SliverFixedExtentList.builder should respect semanticIndexOffset'`
//!    — **out of scope**: no `IndexedSemantics`/`semanticIndexOffset`
//!    concept and no semantics-tree assertion in the headless harness — the
//!    standing gap every semantics-touching port here cites.
//! 7. `'SliverFixedExtentList.builder can build children'` — **ported**:
//!    [`sliver_fixed_extent_list_builder_hit_tests_children_by_position`].
//! 8. `'RenderSliverFixedExtentBoxAdaptor.layoutDimensions reflects the
//!    current constraints'` — **out of scope**: asserts a `layoutDimensions`
//!    getter FLUI's render object does not retain (transient layout inputs
//!    are deliberately not kept on `self`).
//! 9. `'SliverList.list can build children'` (its body constructs
//!    `SliverFixedExtentList.list`) — **ported**:
//!    [`sliver_fixed_extent_list_hit_tests_children_by_position`].
//!
//! **Total: 9 subject cases = 7 ported (two of them as the recorded clamp
//! divergence) + 2 out of scope by missing API.**

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_rendering::view::CacheExtentStyle;
use flui_types::Color;
use flui_types::layout::AxisDirection;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{
    Container, CustomScrollView, GestureDetector, ScrollController, SliverFixedExtentList, Text,
    Viewport,
};

use crate::common::{self, LaidOut};
use crate::harness;

/// Mirrors the oracle's `TestSliverFixedExtentList` helper (a
/// `CustomScrollView` over one `SliverFixedExtentList.list`): every label
/// becomes a direct `Text` sliver child, matching the oracle's own bare
/// `Text(..., key: ...)` children (a composite lazy child is stamped at
/// adoption, so it needs no wrapping box).
fn fixed_extent_list_scene(item_extent: f32, labels: &[&str]) -> impl View {
    let children: Vec<BoxedView> = labels
        .iter()
        .map(|&label| Text::new(label.to_string()).boxed())
        .collect();
    CustomScrollView::new((SliverFixedExtentList::new(item_extent, children),))
}

/// Mirrors the oracle's `testSliverFixedExtentList`/underflow scene: a
/// `Viewport` over one `SliverFixedExtentList` bound to a live
/// `ScrollController` (`CustomScrollView` has no `.position(ScrollPosition)`
/// passthrough, only a plain `.offset(f32)` — same precedent
/// `sliver_list_test.rs`'s own module doc documents; `CustomScrollView`
/// itself composes down to exactly this `Viewport` shape).
fn fixed_extent_underflow_scene(
    labels: &[&str],
    item_extent: f32,
    controller: &ScrollController,
) -> impl View {
    let children: Vec<BoxedView> = labels
        .iter()
        .map(|&label| Text::new(label.to_string()).boxed())
        .collect();
    Viewport::new((SliverFixedExtentList::new(item_extent, children),))
        .axis_direction(AxisDirection::TopToBottom)
        .position(controller.position())
}

/// Mirrors the oracle's `_buildTapTarget` scene for case 9: a
/// `CustomScrollView` over one `SliverFixedExtentList` of two tap targets,
/// each a `GestureDetector` (over a colored `Container` so the whole band
/// is hittable, matching `container_with_color_is_hittable`'s established
/// pattern) incrementing its own counter.
fn fixed_extent_list_tap_scene(
    item_extent: f32,
    counter0: Arc<AtomicUsize>,
    counter1: Arc<AtomicUsize>,
) -> impl View {
    let item0 = GestureDetector::new()
        .on_tap(move || {
            counter0.fetch_add(1, Ordering::SeqCst);
        })
        .child(
            Container::new()
                .color(Color::rgb(0, 255, 0))
                .child(Text::new("Index 0")),
        )
        .boxed();
    let item1 = GestureDetector::new()
        .on_tap(move || {
            counter1.fetch_add(1, Ordering::SeqCst);
        })
        .child(
            Container::new()
                .color(Color::rgb(255, 0, 0))
                .child(Text::new("Index 1")),
        )
        .boxed();
    CustomScrollView::new((SliverFixedExtentList::new(item_extent, vec![item0, item1]),))
}

// ============================================================================
// CASE 3 — Correctly layout children after rearranging
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList Correctly
/// layout children after rearranging'` (tag `3.44.0`).
///
/// Real, non-vacuous green — see the module doc's ledger entry 3 for why
/// this case does not actually need the still-open no-per-item-key gap:
/// its own assertions check only final presence + relative vertical order
/// after the second `pumpWidget`, not cross-swap identity preservation.
#[test]
fn sliver_fixed_extent_list_lays_out_children_in_order_after_rearranging() {
    const ITEM_EXTENT: f32 = 10.0;

    let mut laid = harness::pump_widget(
        fixed_extent_list_scene(ITEM_EXTENT, &["item0", "item2", "item1"]),
        harness::screen(),
    );

    laid.pump_widget(fixed_extent_list_scene(
        ITEM_EXTENT,
        &["item0", "item3", "item1", "item4", "item2"],
    ));

    let item0 = laid
        .find_text("item0")
        .expect("'item0' must be mounted after the rearrange");
    let item3 = laid
        .find_text("item3")
        .expect("'item3' must be mounted after the rearrange");
    let item1 = laid
        .find_text("item1")
        .expect("'item1' must be mounted after the rearrange");
    let item4 = laid
        .find_text("item4")
        .expect("'item4' must be mounted after the rearrange");
    let item2 = laid
        .find_text("item2")
        .expect("'item2' must be mounted after the rearrange");

    let top_of = |id| laid.absolute_offset(id).dy.get();

    assert!(
        top_of(item0) < top_of(item3),
        "'item0' (new index 0) must sit above 'item3' (new index 1)"
    );
    assert!(
        top_of(item3) < top_of(item1),
        "'item3' (new index 1) must sit above 'item1' (new index 2)"
    );
    assert!(
        top_of(item1) < top_of(item4),
        "'item1' (new index 2) must sit above 'item4' (new index 3)"
    );
    assert!(
        top_of(item4) < top_of(item2),
        "'item4' (new index 3) must sit above 'item2' (new index 4)"
    );

    // Cross-axis: the oracle also checks `sameVertical` (a shared column —
    // every item's `dx` matches) alongside `isBelow` for each pair. Faithful
    // here too: a fixed-extent list only varies the main axis per item, so
    // every item must share `item0`'s horizontal position.
    let left_of = |id| laid.absolute_offset(id).dx.get();
    let expected_left = left_of(item0);
    for (label, id) in [
        ("item3", item3),
        ("item1", item1),
        ("item4", item4),
        ("item2", item2),
    ] {
        assert_eq!(
            left_of(id),
            expected_left,
            "'{label}' must share 'item0's horizontal (cross-axis) position — \
             the oracle's `sameVertical` check"
        );
    }
}

// ============================================================================
// CASE 2 (divergence pin) — underflow when children change
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList handles
/// underflow when its children changes'` — the residency half of the case:
/// scrolled to the settled tail position, the five items above the window
/// are never built at all (their `State` never initialises); only the
/// onstage tail item exists. The static children go through the lazy
/// delegate, so the fixed-extent list builds only its window (ADR-0053).
/// The second half of the oracle (the child list shrinking under the
/// viewport and the underflow that follows) is
/// `sliver_fixed_extent_list_shrinking_children_clamps_the_position`.
#[test]
fn sliver_fixed_extent_list_offscreen_children_are_not_built_on_initial_window() {
    const ITEM_EXTENT: f32 = 900.0;
    const VIEWPORT_HEIGHT: f32 = 600.0;
    // 6 items * 900px = 5400px total scroll extent; max_scroll_extent =
    // 5400 - 600 = 4800, matching the oracle's own settled position.
    const SETTLED_OFFSET: f32 = 4800.0;
    let items = ["1", "2", "3", "4", "5", "6"];

    let controller = ScrollController::with_initial_scroll_offset(SETTLED_OFFSET);
    let laid = harness::pump_widget(
        fixed_extent_underflow_scene(&items, ITEM_EXTENT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );

    for absent in ["1", "2", "3", "4", "5"] {
        assert!(
            laid.find_text(absent).is_none(),
            "item '{absent}' sits above the window and must never be built \
             (a residency absence — its State never initialises)"
        );
    }
    assert!(
        laid.find_text("6").is_some(),
        "the single onstage tail item must still be present"
    );
}

// ============================================================================
// CASE 9 — hit-test by position (misleadingly-named oracle case)
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverList.list can build
/// children'` (tag `3.44.0`, line 1387 — misleadingly named; its body
/// constructs `SliverFixedExtentList.list`, a copy-paste artifact in the
/// oracle itself, see the module doc's content-sweep note).
///
/// Real, non-vacuous green: two `itemExtent: 100` items, each its own tap
/// target. Mirrors the oracle's mutual-exclusion assertions exactly —
/// tapping item 0's center fires ONLY counter 0 (counter 1 stays at 0);
/// tapping item 1's center fires ONLY counter 1 (counter 0 stays at its
/// prior count). Both the "fired" and "did not fire" side of each tap are
/// asserted, matching the oracle's own `expect(firstTapped, ...);
/// expect(secondTapped, ...)` pair after each `tester.tap(...)`.
///
/// Mutation-checked: temporarily swapping the two tap coordinates flips
/// this test red (item 1's tap would fire counter 0 instead of counter 1),
/// confirming the assertions are not vacuously true from e.g. both
/// `GestureDetector`s sharing a hit region.
#[test]
fn sliver_fixed_extent_list_hit_tests_children_by_position() {
    const ITEM_EXTENT: f32 = 100.0;

    let counter0 = Arc::new(AtomicUsize::new(0));
    let counter1 = Arc::new(AtomicUsize::new(0));

    let laid = harness::pump_widget(
        fixed_extent_list_tap_scene(ITEM_EXTENT, Arc::clone(&counter0), Arc::clone(&counter1)),
        harness::screen(),
    );

    // Centers computed from the actual laid-out geometry, not hardcoded:
    // each item's text node's absolute rect midpoint is guaranteed to fall
    // within its ancestor `Container`/`GestureDetector`'s hit region.
    let center_of = |id: RenderId| {
        let offset = laid.absolute_offset(id);
        let extent = laid.size(id);
        (
            offset.dx.get() + extent.width.get() / 2.0,
            offset.dy.get() + extent.height.get() / 2.0,
        )
    };

    let text0 = laid
        .find_text("Index 0")
        .expect("'Index 0' must be mounted");
    let text1 = laid
        .find_text("Index 1")
        .expect("'Index 1' must be mounted");
    let (x0, y0) = center_of(text0);
    let (x1, y1) = center_of(text1);

    laid.dispatch_pointer_down(x0, y0);
    laid.dispatch_pointer_up(x0, y0);
    assert_eq!(
        counter0.load(Ordering::SeqCst),
        1,
        "tapping item 0's center must fire its own counter"
    );
    assert_eq!(
        counter1.load(Ordering::SeqCst),
        0,
        "tapping item 0's center must NOT fire item 1's counter"
    );

    laid.dispatch_pointer_down(x1, y1);
    laid.dispatch_pointer_up(x1, y1);
    assert_eq!(
        counter0.load(Ordering::SeqCst),
        1,
        "tapping item 1's center must NOT fire item 0's counter again"
    );
    assert_eq!(
        counter1.load(Ordering::SeqCst),
        1,
        "tapping item 1's center must fire its own counter"
    );
}

// ============================================================================
// CASE 1 — correctly clears garbage
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList correctly
/// clears garbage'`. Three drags (−1200, −1200, −800) become one `jump_to`
/// of 3200 px; 900 px items in a 600 px viewport. After the jump items 4–5
/// are the window and 1–3 are gone; inserting '0' at the head shifts every
/// index by one, and the window at the same offset now reads 3–4 while
/// 0–2 are never built — the leading and trailing garbage the oracle asserts.
#[test]
fn sliver_fixed_extent_list_clears_garbage_across_a_head_insert() {
    const ITEM_EXTENT: f32 = 900.0;
    const VIEWPORT_HEIGHT: f32 = 600.0;
    let controller = ScrollController::new();
    let items = ["1", "2", "3", "4", "5", "6"];
    let mut laid = harness::pump_widget(
        fixed_extent_underflow_scene(&items, ITEM_EXTENT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    controller.jump_to(3200.0);
    laid.pump();
    common::settle_lazy(&mut laid);
    for gone in ["1", "2", "3"] {
        assert!(
            laid.find_text(gone).is_none(),
            "'{gone}' scrolled out and was evicted"
        );
    }
    for present in ["4", "5"] {
        assert!(
            laid.find_text(present).is_some(),
            "'{present}' is in the window"
        );
    }

    let shifted = ["0", "1", "2", "3", "4", "5", "6"];
    laid.pump_widget(fixed_extent_underflow_scene(
        &shifted,
        ITEM_EXTENT,
        &controller,
    ));
    common::settle_lazy(&mut laid);
    for gone in ["0", "1", "2"] {
        assert!(
            laid.find_text(gone).is_none(),
            "'{gone}' is leading garbage"
        );
    }
    for present in ["3", "4"] {
        assert!(
            laid.find_text(present).is_some(),
            "'{present}' fills the window after the shift"
        );
    }
}

// ============================================================================
// CASE 2 (second half) — the children shrink under the viewport
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList handles
/// underflow when its children changes'`, the underflow half: settled at the
/// tail of six 900 px items, the list shrinks to three. The position clamps
/// to the new extent (`3 × 900 − 600 = 2100`) and only the remaining tail
/// item is built.
#[test]
fn sliver_fixed_extent_list_shrinking_children_clamps_the_position() {
    const ITEM_EXTENT: f32 = 900.0;
    const VIEWPORT_HEIGHT: f32 = 600.0;
    let controller = ScrollController::with_initial_scroll_offset(4800.0);
    let mut laid = harness::pump_widget(
        fixed_extent_underflow_scene(&["1", "2", "3", "4", "5", "6"], ITEM_EXTENT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    assert!(laid.find_text("6").is_some());

    laid.pump_widget(fixed_extent_underflow_scene(
        &["1", "2", "3"],
        ITEM_EXTENT,
        &controller,
    ));
    settle_pixels(&mut laid, &controller);
    assert_eq!(
        controller.pixels(),
        2100.0,
        "clamped to the shrunken extent"
    );
    assert!(laid.find_text("3").is_some(), "the new tail item is built");
    assert!(laid.find_text("6").is_none(), "the removed item is gone");
}

// ============================================================================
// CASES 4 and 5 — the clamp contract where Flutter auto-corrects
// ============================================================================

/// Seven 200 px pages built on demand (the builder alone knows the end), a
/// 600 px viewport with no cache, starting at 600 px — the oracle's scene.
fn on_demand_pages(controller: &ScrollController) -> impl View {
    let sliver = SliverFixedExtentList::builder(200.0, usize::MAX, |index| {
        (index <= 6).then(|| Text::new(format!("Page {index}")).boxed())
    });
    Viewport::new((sliver,))
        .axis_direction(AxisDirection::TopToBottom)
        .cache_extent(0.0, CacheExtentStyle::Pixel)
        .position(controller.position())
}

/// Pump until the position stops moving (a far jump past an end the builder
/// has not revealed yet settles over a few layout passes: each pass's
/// requests reveal the end, the count clamps, the viewport clamps).
fn settle_pixels(laid: &mut LaidOut, controller: &ScrollController) {
    for _ in 0..8 {
        let before = controller.pixels();
        laid.pump();
        common::settle_lazy(laid);
        if controller.pixels() == before {
            return;
        }
    }
}

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList with
/// SliverChildBuilderDelegate auto-correct scroll offset - super fast'`,
/// as the clamp contract (ADR-0053): a jump of 1000 px from 600 px lands
/// past the end; the settled offset is the real maximum, `7 × 200 − 600`.
#[test]
fn sliver_fixed_extent_list_far_jump_past_the_end_clamps_to_the_real_extent() {
    let controller = ScrollController::with_initial_scroll_offset(600.0);
    let mut laid = harness::pump_widget(
        on_demand_pages(&controller),
        harness::screen_of(800.0, 600.0),
    );
    assert!(laid.find_text("Page 0").is_none());
    assert!(laid.find_text("Page 6").is_none());

    controller.jump_to(1600.0);
    settle_pixels(&mut laid, &controller);
    assert_eq!(controller.pixels(), 800.0);
    assert!(laid.find_text("Page 0").is_none());
    assert!(laid.find_text("Page 6").is_some());
    // Settled: another pump moves nothing.
    laid.pump();
    assert_eq!(controller.pixels(), 800.0);
}

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList with
/// SliverChildBuilderDelegate auto-correct scroll offset - reasonable'` as the
/// clamp contract: a 10 px over-scroll past the end clamps to the real
/// maximum; Flutter animates back to the same 800 px.
#[test]
fn sliver_fixed_extent_list_overscroll_past_the_end_clamps_to_the_real_extent() {
    let controller = ScrollController::with_initial_scroll_offset(600.0);
    let mut laid = harness::pump_widget(
        on_demand_pages(&controller),
        harness::screen_of(800.0, 600.0),
    );
    controller.jump_to(810.0);
    settle_pixels(&mut laid, &controller);
    assert_eq!(controller.pixels(), 800.0);
}

// ============================================================================
// CASE 7 — builder can build children
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'SliverFixedExtentList.builder can
/// build children'`: two on-demand tap targets, each firing only its own
/// counter.
#[test]
fn sliver_fixed_extent_list_builder_hit_tests_children_by_position() {
    const ITEM_EXTENT: f32 = 100.0;
    let counters = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
    let scene = {
        let counters = counters.clone();
        CustomScrollView::new((SliverFixedExtentList::builder(
            ITEM_EXTENT,
            2,
            move |index| {
                let counter = Arc::clone(&counters[index]);
                Some(
                    GestureDetector::new()
                        .on_tap(move || {
                            counter.fetch_add(1, Ordering::SeqCst);
                        })
                        .child(
                            Container::new()
                                .color(if index == 0 {
                                    Color::rgb(0, 255, 0)
                                } else {
                                    Color::rgb(255, 0, 0)
                                })
                                .child(Text::new(format!("Index {index}"))),
                        )
                        .boxed(),
                )
            },
        ),))
    };
    let laid = harness::pump_widget(scene, harness::screen());
    let center_of = |id: RenderId| {
        let offset = laid.absolute_offset(id);
        let extent = laid.size(id);
        (
            offset.dx.get() + extent.width.get() / 2.0,
            offset.dy.get() + extent.height.get() / 2.0,
        )
    };
    let (x0, y0) = center_of(laid.find_text("Index 0").expect("'Index 0' is built"));
    let (x1, y1) = center_of(laid.find_text("Index 1").expect("'Index 1' is built"));
    laid.dispatch_pointer_down(x0, y0);
    laid.dispatch_pointer_up(x0, y0);
    assert_eq!(counters[0].load(Ordering::SeqCst), 1);
    assert_eq!(counters[1].load(Ordering::SeqCst), 0);
    laid.dispatch_pointer_down(x1, y1);
    laid.dispatch_pointer_up(x1, y1);
    assert_eq!(counters[0].load(Ordering::SeqCst), 1);
    assert_eq!(counters[1].load(Ordering::SeqCst), 1);
}
