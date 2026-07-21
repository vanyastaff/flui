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
//! # The headline finding
//!
//! FLUI's `SliverFixedExtentList` (`crates/flui-widgets/src/scroll/
//! sliver_fixed_extent_list.rs`) is **fully eager** — unlike `SliverList`/
//! `SliverGrid` (both routed through the lazy `SparseChildren` adaptor,
//! `crates/flui-view/src/element/sliver_adaptor.rs`, and the subject of the
//! lazy-adaptor builder-refresh fix — the `needs_resident_refresh` flag +
//! `SparseChildren::refresh_resident`), `SliverFixedExtentList` never
//! touches that adaptor at all — confirmed by grepping `sliver_adaptor.rs`
//! for `FixedExtent` (zero hits). Its render counterpart,
//! `RenderSliverFixedExtentList` (`crates/flui-objects/src/sliver/
//! sliver_fixed_extent_list.rs`), lays out every attached child
//! unconditionally on every `perform_layout` (`for index in
//! 0..self.child_count`) — there is no scroll-offset-driven index range, no
//! child-manager request/build/dispose protocol, and no `.builder`
//! constructor (`SliverFixedExtentList::new(item_extent, children: C:
//! ViewSeq)` is the *only* constructor — confirmed by grepping this crate
//! for `fn builder` inside `sliver_fixed_extent_list.rs`: zero hits). None of
//! `RenderSliverMultiBoxAdaptor`'s public surface
//! (`indexToLayoutOffset`/`getMinChildIndexForScrollOffset`/
//! `getMaxChildIndexForScrollOffset`/`computeMaxScrollOffset`/
//! `calculateLeadingGarbage`/`calculateTrailingGarbage`/`paintsChild`) exists
//! on FLUI's type at all.
//!
//! So: **not** "shares the merged builder-refresh path" and **not** "has its
//! own copy of the pre-fix staleness bug" — the lazy-adaptor builder-refresh
//! fix is categorically inapplicable here, because there is no lazy adaptor for it
//! to apply to. This is a new, broader finding, filed as its own Cross.H
//! entry in `docs/ROADMAP.md` (not a re-file of the existing "no per-item
//! key API" gap-1, which this port does not need — see case 3 below).
//!
//! # Ledger (9 widget-level subject cases; recounted against the 9 bullets
//! immediately below — totals match)
//!
//! 1. `'SliverFixedExtentList correctly clears garbage'` — **out of scope**:
//!    needs `SliverFixedExtentList.builder`, which does not exist in FLUI,
//!    and exercises lazy garbage collection, a concept FLUI's eager render
//!    object has no equivalent of at all.
//! 2. `'SliverFixedExtentList handles underflow when its children changes'`
//!    — **out of scope, but pinned**: uses `.list(...)`, which maps to
//!    FLUI's `::new(...)`, so the call itself compiles — see
//!    [`sliver_fixed_extent_list_offscreen_children_are_not_built_on_initial_window_pin`]
//!    (`#[ignore]`d, verified failing) below, which demonstrates the
//!    divergence directly rather than merely asserting it can't be
//!    expressed.
//! 3. `'SliverFixedExtentList Correctly layout children after rearranging'`
//!    — **ported, real green**:
//!    [`sliver_fixed_extent_list_lays_out_children_in_order_after_rearranging`].
//!    Despite superficially resembling the keyed-identity family (the
//!    oracle's children carry `Key('0')`/`Key('2')`/etc.), this case's own
//!    assertions check only FINAL presence and relative vertical order after
//!    the second `pumpWidget` — never identity/state preservation ACROSS
//!    the swap. Re-verified by reading the case body (not assumed from the
//!    name): no `initState` spy, no scroll-position carry-over, no check
//!    that a specific element survived the reorder — so it does not
//!    actually require the still-open no-key-API gap (the "no per-item
//!    view-key API" entry in `docs/ROADMAP.md`'s Cross.H, filed by
//!    `sliver_list_test.rs`) to port faithfully; a positional `Text`-by-Text
//!    reconciliation produces the identical observable outcome. Ported as a
//!    genuine, non-vacuous pass — not a weakened substitute.
//! 4. `'SliverFixedExtentList with SliverChildBuilderDelegate auto-correct
//!    scroll offset - super fast'` — **out of scope**: needs `.builder`.
//! 5. `'SliverFixedExtentList with SliverChildBuilderDelegate auto-correct
//!    scroll offset - reasonable'` — **out of scope**: needs `.builder`.
//! 6. `'SliverFixedExtentList.builder should respect semanticIndexOffset'`
//!    — **out of scope, two independent reasons**: needs `.builder` (absent);
//!    also no `IndexedSemantics`/`semanticIndexOffset` concept and no
//!    semantics-tree assertion capability exist anywhere in `flui-widgets`'
//!    headless harness — the same standing gap every other port in this
//!    directory that touches semantics already cites.
//! 7. `'SliverFixedExtentList.builder can build children'` — **out of
//!    scope**: needs `.builder`.
//! 8. `'RenderSliverFixedExtentBoxAdaptor.layoutDimensions reflects the
//!    current constraints'` — **out of scope**: constructs its sliver with
//!    an explicit `SliverChildBuilderDelegate` (the same lazy-delegate
//!    machinery `.builder` sugars over) and asserts a `layoutDimensions`
//!    getter that does not exist on FLUI's `RenderSliverFixedExtentList` at
//!    all (confirmed by reading the type: only `item_extent()`/
//!    `set_item_extent()` are public — see this file's module doc on the
//!    "2B field dedup" comment in the render object's own source, which
//!    explains transient layout inputs are deliberately not retained on
//!    `self`).
//! 9. `'SliverList.list can build children'` (line 1387 — misleadingly
//!    named; its body constructs `SliverFixedExtentList.list`, see the
//!    content-sweep note above) — **ported, real green**:
//!    [`sliver_fixed_extent_list_hit_tests_children_by_position`]. Two
//!    `itemExtent: 100` items, each wrapped in its own tap target; the
//!    oracle's mutual-exclusion assertions (tapping item 0 fires only its
//!    own counter, tapping item 1 fires only its own) are fully expressible
//!    against FLUI's real hit-test path (`GestureDetector::on_tap` +
//!    `LaidOut::dispatch_pointer_down`/`dispatch_pointer_up`) — no gap here
//!    at all, eager or otherwise: hit-testing an attached child by position
//!    does not touch any of the missing lazy-adaptor machinery the other
//!    cases above are blocked on.
//!
//! **Total: 9 subject cases found = 9 accounted for above (2 ported green —
//! rearrange + hit-test — 1 pinned red, 6 out-of-scope-by-missing-API).**
//!
//! Render-level oracle (12 subject cases, all out of scope for the same
//! root cause named above): documented in `render_object_harness.rs`
//! adjacent to the existing `harness_sliver_fixed_extent_list_geometry`
//! test rather than repeated here — no new harness tests were addable, since
//! every one of the 12 cases needs API (`indexToLayoutOffset`,
//! `getMinChildIndexForScrollOffset`/`getMaxChildIndexForScrollOffset`,
//! `computeMaxScrollOffset`, `calculateLeadingGarbage`/
//! `calculateTrailingGarbage`) that does not exist on FLUI's render object,
//! and the harness's own `viewport()`/`sliver_node()` builders construct a
//! fully-eager tree with no lazy child-manager concept to even set up a
//! "some children never attach" scenario against.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_types::Color;
use flui_types::layout::AxisDirection;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{
    Container, CustomScrollView, GestureDetector, ScrollController, SliverFixedExtentList, Text,
    Viewport,
};

use crate::harness;

/// Mirrors the oracle's `TestSliverFixedExtentList` helper (a
/// `CustomScrollView` over one `SliverFixedExtentList.list`): every label
/// becomes a direct `Text` sliver child (no wrapping box needed — unlike the
/// lazy `SliverList`'s `SparseChildren::ensure` invariant, this eager
/// adaptor has no "child must own its own render node" requirement, so a
/// bare `Text` composes fine, matching the oracle's own bare `Text(...,
/// key: ...)` children).
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
/// underflow when its children changes'` (tag `3.44.0`).
///
/// **`#[ignore]`d divergence pin, verified failing for the stated reason.**
/// Named for the oracle behavior it asserts (offscreen children are *not
/// built*), not for FLUI's divergent one. The oracle's first-phase
/// assertion: with 6 items of `itemExtent: 900` scrolled to
/// `max_scroll_extent` (`5400 - 600 = 4800`), only the single onstage tail
/// item ever has its `State` initialized — each child is a
/// `StateInitSpy(item, () => initializedChild.add(item), ...)`, and the
/// oracle asserts `listEquals<String>(initializedChild, <String>['6'])` —
/// Flutter's real `SliverMultiBoxAdaptorElement` never builds an off-window
/// child at all, so `find.text('1')` through `find.text('5')` are
/// `findsNothing` (a genuine RESIDENCY absence, not merely an offstage one —
/// no `is_onstage_text` nuance applies here). FLUI's `SliverFixedExtentList`
/// mounts and lays out every child unconditionally (module doc's headline
/// finding), so all 6 items are found in the tree regardless of scroll
/// position — this assertion is expected to, and does, fail.
///
/// **Scope: this pin reproduces only the oracle's initial-window phase**,
/// deliberately not its second phase (`jumpTo(0)` + swapping `children[0]`
/// with `children[5]` then re-pumping, which the oracle expects to keep
/// `initializedChild == ['6']` because the keyed `'6'` element is reused at
/// the new head while the rest still never build). That second phase is
/// omitted, not overlooked: (1) FLUI is fully eager, so the initial-window
/// assertion above already diverges — a later child swap cannot change that
/// outcome, and (2) the second phase's keyed-identity-preservation half
/// additionally needs the still-open no-per-item-view-key API (the separate
/// Cross.H gap the `SliverList` port filed), which `SliverFixedExtentList`'s
/// bare-`ViewSeq` children cannot express today. Scoping the pin to
/// initial-window laziness keeps it honest rather than adding a swap step
/// that would be a no-op against an eager, keyless type; the name reflects
/// that scope so a future reader does not unignore it expecting
/// child-change/underflow coverage it never had.
///
/// See the Cross.H entry this pin references (`docs/ROADMAP.md`) for the
/// architectural gap (no lazy child-manager for this render object) that
/// would need closing before the first-phase assertion could pass.
#[test]
#[ignore = "divergence pin: SliverFixedExtentList is eager (no lazy child-manager) — \
            see the 'SliverFixedExtentList/RenderSliverFixedExtentList is a fully eager' \
            Cross.H entry in docs/ROADMAP.md"]
fn sliver_fixed_extent_list_offscreen_children_are_not_built_on_initial_window_pin() {
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
            "Flutter's oracle expects item '{absent}' to never be built at all \
             (a residency absence — its State never initializes); FLUI's eager \
             SliverFixedExtentList mounts it regardless of scroll position"
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
