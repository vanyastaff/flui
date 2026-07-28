//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart` `SliverList`
//!   named constructors `.builder`/`.separated`/`.list` (tag `3.44.0`), all
//!   three producing the same `SliverList` widget class — backed by
//!   `SliverChildBuilderDelegate` (`.builder`/`.separated`) or
//!   `SliverChildListDelegate` (`.list`).
//! - Render object: `packages/flutter/lib/src/rendering/sliver_list.dart`
//!   `RenderSliverList`.
//! - Tests: `packages/flutter/test/widgets/slivers_test.dart` (tag
//!   `3.44.0`) — a SHARED multi-subject file (`Viewport`, `SliverList`,
//!   `SliverFixedExtentList`, `SliverGrid`, `SliverOffstage` all live in
//!   it; see `sliver_fixed_extent_list_test.rs`'s own module doc for that
//!   file's `SliverFixedExtentList`-subject slice). **This unit scopes
//!   itself strictly to the 4 `SliverList`-*named-constructor* parity case
//!   names enumerated in the ledger below** — the pre-scoped case set this
//!   port was assigned. The remaining `SliverList`-subject cases in the
//!   same oracle file (scroll-offset-correction scenarios etc. —
//!   `sliver_list_test.rs`'s own module doc counts "8 more"
//!   `SliverList`-titled cases across this file) are a separate,
//!   not-yet-picked-up unit, the same scoping convention `sliver_list_test.rs`
//!   already established for a different subset of this same oracle file.
//!
//! Widget → render-object mapping:
//! - `SliverList.builder`/`.separated`/`.list` → FLUI [`SliverList`] (the
//!   lazy-sliver adaptor `View` defined in `flui-view`,
//!   `crates/flui-view/src/element/sliver_adaptor.rs`, re-exported here) →
//!   `RenderSliverList` (`crates/flui-objects/src/sliver/sliver_list.rs`).
//! - Tap targets: `GestureDetector` → `Container` (painted, opaque hit
//!   band) → `Text`, mirroring the oracle's own `_buildIndexedTapTarget`/
//!   `_buildTapTarget` helpers (`slivers_test.dart`, tag `3.44.0`) — see
//!   [`indexed_tap_target`]/[`tap_target`]'s own doc comments for the one
//!   FLUI-specific addition (an outer `SizedBox` wrapper).
//!
//! # Implementation additions landed with this port
//!
//! FLUI's `SliverList` (`crates/flui-view/src/element/sliver_adaptor.rs`)
//! previously had exactly ONE constructor, `SliverList::new(item_count,
//! item_extent_estimate, builder)` — already Flutter's `.builder` semantics
//! (a raw index → view closure). `.separated` and `.list` did not exist
//! anywhere in the workspace before this port (confirmed by grep: zero
//! hits for either name on `SliverList`). Both are added as inherent
//! `SliverList` associated functions — not a `flui-widgets`-layer wrapper —
//! the natural home, since `.separated`/`.list` are just alternate ways to
//! construct the SAME `SliverList` view FLUI already has, mirroring how
//! Flutter's own three named constructors all produce one `SliverList`
//! widget class. Both delegate to `SliverList::new` rather than duplicating
//! its extent-estimate validation.
//!
//! - **`SliverList::separated`** — interleaves an item builder (even
//!   logical indices) with a separator builder (odd logical indices);
//!   effective child count `2 * item_count - 1` (`0` when `item_count ==
//!   0`) — Flutter's own `math.max(0, itemCount * 2 - 1)`
//!   (`widgets/sliver.dart` `SliverList.separated`, tag `3.44.0`). The
//!   exact index arithmetic is pinned by a dedicated unit test in
//!   `sliver_adaptor.rs`
//!   (`separated_maps_logical_index_to_item_and_separator_index_correctly`),
//!   which fails if the item/separator branches are swapped.
//! - **`SliverList::list`** — FLUI's
//!   lazy-adaptor protocol can re-consult the builder for an
//!   already-resident index (`SparseChildren::refresh_resident`), so an
//!   owned `Vec<BoxedView>` handed out by value cannot serve as the backing
//!   store directly. Resolved soundly, not forced: `BoxedView` already
//!   implements `Clone` (`crates/flui-view/src/view/into_view.rs`, a real
//!   `dyn_clone::clone_box` deep clone, confirmed by reading the impl, not
//!   assumed), so `SliverList::list` stores `Rc<Vec<BoxedView>>` and its
//!   builder closure clones the entry at each call — faithful to Flutter's
//!   own `SliverChildListDelegate.build`, which likewise hands back the
//!   same immutable `Widget` value on every call. No Cross.H gap filed for
//!   `.list` — a faithful shape was expressible today.
//! - **`LaidOut::find_all_text`** (`tests/common/mod.rs`) — the multi-match
//!   counterpart to `find_text` (which panics on more than one match),
//!   needed for case 3's `findsNWidgets(2)`/`findsNWidgets(1)` count
//!   assertions. `find_text`'s own behavior is unchanged.
//!
//! # Render-node-ownership wrapper (established pattern, not a new finding)
//!
//! FLUI's lazy `SliverList` requires the top-level view its builder returns
//! to already own a render node at mount time (`SparseChildren::ensure`'s
//! invariant, `crates/flui-view/src/element/sparse_children.rs`) — a bare
//! `GestureDetector`/`Text` (both composites with no render node of their
//! own until their first build) trips it. `sliver_list_test.rs`'s own scene
//! builders already work around this by wrapping every item in `SizedBox`;
//! this port does the same (see [`indexed_tap_target`]/[`tap_target`]) — a
//! pre-existing, already-documented divergence in HOW a tree is composed,
//! not a new one and not a behavior gap: the oracle's own
//! `_buildIndexedTapTarget`/`_buildTapTarget` return the `GestureDetector`
//! directly with no wrapper, since Flutter's element model builds
//! composites synchronously and has no equivalent constraint.
//!
//! # Ledger (4 case names — every `SliverList.builder`/`.separated`/
//! `.list`-subject occurrence in `slivers_test.dart` at tag `3.44.0`;
//! recounted against the 4 bullets immediately below — totals match)
//!
//! 1. `'SliverList.builder can build children'` — appears TWICE verbatim in
//!    the oracle (an identical duplicate test body at both occurrences, not
//!    two distinct scenarios) — **ported once, real green**:
//!    [`sliver_list_builder_hit_tests_children_by_position`]. Both oracle
//!    occurrences are accounted by this one port; porting the identical
//!    body twice would add no coverage.
//! 2. `'SliverList.separated can build children'` — **ported, real green**:
//!    [`sliver_list_separated_hit_tests_children_by_position`].
//! 3. `'SliverList.separated has correct number of children'` — **ported,
//!    real green**: [`sliver_list_separated_has_correct_number_of_children`].
//! 4. `'SliverList.list can build children'` — the oracle has TWO
//!    identically-titled cases in `slivers_test.dart`; this ledger accounts
//!    ONLY the one whose body constructs `SliverList.list(...)`. The other
//!    (body constructs `SliverFixedExtentList.list(...)`, a copy-paste
//!    artifact in the oracle itself) is accounted in
//!    `sliver_fixed_extent_list_test.rs`'s own ledger (its case 9,
//!    `sliver_fixed_extent_list_hit_tests_children_by_position`) — cross-
//!    referenced, not double-counted here. **Ported, real green**:
//!    [`sliver_list_list_hit_tests_children_by_position`].
//!
//! **Total: 4 case names = 4 accounted for above (4 ported green, 0
//! pinned, 0 out-of-scope) — covering 5 oracle `testWidgets` occurrences
//! (case 1's literal duplicate counts as 2), all real, non-vacuous
//! passes** (each tap test asserts both the fired AND the not-fired
//! counter, in both tap directions; case 3's count assertions are
//! mutation-checked — see the per-test doc comments). Two further oracle
//! occurrences in the same file that superficially match this unit's
//! case-set titles — `'SliverFixedExtentList.builder can build children'`
//! and the second `'SliverList.list can build children'` (body constructs
//! `SliverFixedExtentList.list`) — belong to
//! `sliver_fixed_extent_list_test.rs`'s scope, already accounted there; see
//! its own ledger, not repeated here.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_types::Color;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{Container, CustomScrollView, GestureDetector, SizedBox, SliverList, Text};

use crate::common::LaidOut;
use crate::harness;

// ============================================================================
// SHARED SCENE HELPERS
// ============================================================================

/// `_debugEvenColor`/`_debugOddColor` (`slivers_test.dart`, tag `3.44.0`):
/// `Color(0xFF00FF00)` (green) for even indices, `Color(0xFFFF0000)` (red)
/// for odd — matches `sliver_fixed_extent_list_test.rs`'s own tap scene,
/// which cites the same two constants.
fn even_or_odd_color(index: usize) -> Color {
    if index.is_multiple_of(2) {
        Color::rgb(0, 255, 0)
    } else {
        Color::rgb(255, 0, 0)
    }
}

/// Mirrors the oracle's `_buildIndexedTapTarget` helper (`slivers_test.dart`,
/// tag `3.44.0`): a `GestureDetector` over a colored `Container` (the whole
/// band is hittable — `HitTestBehavior.opaque` in the oracle, matched here
/// by painting a color so the region is hit-testable, the same precedent
/// `sliver_fixed_extent_list_test.rs`'s own tap scene documents) wrapping
/// `Text('Index {index}')`, firing `on_tap` when hit.
///
/// Wrapped in `SizedBox::height(item_extent)` — FLUI's lazy `SliverList`
/// requires its direct child to own a render node at mount time (this
/// module doc's "Render-node-ownership wrapper" section); `GestureDetector`
/// alone has no render node of its own until its first build.
fn indexed_tap_target(item_extent: f32, index: usize, on_tap: impl Fn() + 'static) -> BoxedView {
    let color = even_or_odd_color(index);
    SizedBox::height(item_extent)
        .child(
            GestureDetector::new().on_tap(on_tap).child(
                Container::new()
                    .color(color)
                    .child(Text::new(format!("Index {index}"))),
            ),
        )
        .boxed()
}

/// Mirrors the oracle's `_buildTapTarget` helper (`slivers_test.dart`, tag
/// `3.44.0`) — like [`indexed_tap_target`] but with an explicit label/color
/// instead of deriving them from an index; used by case 4's
/// `SliverList.list` scene, matching the oracle's own `_buildTapTarget`
/// call sites.
fn tap_target(
    item_extent: f32,
    label: &str,
    color: Color,
    on_tap: impl Fn() + 'static,
) -> BoxedView {
    SizedBox::height(item_extent)
        .child(
            GestureDetector::new().on_tap(on_tap).child(
                Container::new()
                    .color(color)
                    .child(Text::new(label.to_string())),
            ),
        )
        .boxed()
}

/// Drives the lazy virtualizer's request → service → re-layout settle
/// sequence to completion — two ticks, the same convention
/// `sliver_list_test.rs`'s own `settle` documents: lazy children build
/// *after* paint, so a scene needs one tick to emit build requests and a
/// second to re-lay-out with them built.
fn settle(laid: &mut LaidOut) {
    laid.tick();
    laid.tick();
}

/// Taps 'Index 0' then 'Index 1' and asserts each tap fires only its own
/// counter, both directions — the mutual-exclusion assertion pattern shared
/// verbatim by the oracle's `'SliverList.builder'`/`'.separated'`/`'.list
/// can build children'` cases (`expect(firstTapped, ...);
/// expect(secondTapped, ...)` after each `tester.tap(...)`).
///
/// Centers are computed from the actual laid-out geometry, not hardcoded —
/// mirrors `sliver_fixed_extent_list_hit_tests_children_by_position`'s
/// established `center_of` pattern.
///
/// Mutation-checked: swapping the two tap coordinates flips every caller of
/// this helper red (item 1's tap would fire counter 0 instead of counter 1),
/// confirming the assertions are not vacuously true.
fn assert_taps_are_mutually_exclusive(
    laid: &LaidOut,
    counter0: &AtomicUsize,
    counter1: &AtomicUsize,
) {
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
// CASE 1 — SliverList.builder can build children (oracle duplicate)
// ============================================================================

/// Mirrors the oracle's scene inline in `'SliverList.builder can build
/// children'` (`slivers_test.dart`, tag `3.44.0` — appears twice verbatim;
/// see the module doc's ledger entry 1): a `CustomScrollView` over one
/// `SliverList.builder`-equivalent sliver (FLUI's [`SliverList::new`] IS
/// Flutter's `.builder` semantics — see the module doc's widget mapping) of
/// two tap targets.
fn sliver_list_builder_tap_scene(
    counter0: Arc<AtomicUsize>,
    counter1: Arc<AtomicUsize>,
) -> impl View {
    const ITEM_EXTENT: f32 = 100.0;
    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |index: usize| {
        if index >= 2 {
            return None;
        }
        let (counter0, counter1) = (Arc::clone(&counter0), Arc::clone(&counter1));
        Some(indexed_tap_target(ITEM_EXTENT, index, move || {
            if index.is_multiple_of(2) {
                counter0.fetch_add(1, Ordering::SeqCst);
            } else {
                counter1.fetch_add(1, Ordering::SeqCst);
            }
        }))
    });
    CustomScrollView::new((SliverList::new(2, ITEM_EXTENT, builder),))
}

/// Flutter parity: `slivers_test.dart` `'SliverList.builder can build
/// children'` (tag `3.44.0` — appears twice verbatim in the oracle; ported
/// once, see the module doc's ledger entry 1).
#[test]
fn sliver_list_builder_hit_tests_children_by_position() {
    let counter0 = Arc::new(AtomicUsize::new(0));
    let counter1 = Arc::new(AtomicUsize::new(0));

    let mut laid = harness::pump_widget(
        sliver_list_builder_tap_scene(Arc::clone(&counter0), Arc::clone(&counter1)),
        harness::screen(),
    );
    settle(&mut laid);

    assert_taps_are_mutually_exclusive(&laid, &counter0, &counter1);
}

// ============================================================================
// CASE 2 — SliverList.separated can build children
// ============================================================================

/// Mirrors the oracle's scene inline in `'SliverList.separated can build
/// children'` (`slivers_test.dart`, tag `3.44.0`): a `CustomScrollView` over
/// one `SliverList::separated` sliver of two tap targets, with a plain
/// `Text('Separator {index}')` between them (the oracle's own
/// `separatorBuilder: (context, index) => Text('Separator $index')`).
fn sliver_list_separated_tap_scene(
    counter0: Arc<AtomicUsize>,
    counter1: Arc<AtomicUsize>,
) -> impl View {
    const ITEM_EXTENT: f32 = 100.0;
    const SEPARATOR_EXTENT: f32 = 20.0;

    let item_builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |index: usize| {
        if index >= 2 {
            return None;
        }
        let (counter0, counter1) = (Arc::clone(&counter0), Arc::clone(&counter1));
        Some(indexed_tap_target(ITEM_EXTENT, index, move || {
            if index.is_multiple_of(2) {
                counter0.fetch_add(1, Ordering::SeqCst);
            } else {
                counter1.fetch_add(1, Ordering::SeqCst);
            }
        }))
    });
    let separator_builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|index: usize| {
        Some(
            SizedBox::height(SEPARATOR_EXTENT)
                .child(Text::new(format!("Separator {index}")))
                .boxed(),
        )
    });

    CustomScrollView::new((SliverList::separated(
        2,
        ITEM_EXTENT,
        item_builder,
        separator_builder,
    ),))
}

/// Flutter parity: `slivers_test.dart` `'SliverList.separated can build
/// children'` (tag `3.44.0`).
#[test]
fn sliver_list_separated_hit_tests_children_by_position() {
    let counter0 = Arc::new(AtomicUsize::new(0));
    let counter1 = Arc::new(AtomicUsize::new(0));

    let mut laid = harness::pump_widget(
        sliver_list_separated_tap_scene(Arc::clone(&counter0), Arc::clone(&counter1)),
        harness::screen(),
    );
    settle(&mut laid);

    assert_taps_are_mutually_exclusive(&laid, &counter0, &counter1);
}

// ============================================================================
// CASE 3 — SliverList.separated has correct number of children
// ============================================================================

/// Mirrors the oracle's scene inline in `'SliverList.separated has correct
/// number of children'` (`slivers_test.dart`, tag `3.44.0`): 2 items each
/// rendering the literal text `'item'`, 1 separator rendering the literal
/// text `'separator'` between them.
fn sliver_list_separated_count_scene() -> impl View {
    const ITEM_EXTENT: f32 = 20.0;
    let item_builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|_index: usize| {
        Some(
            SizedBox::height(ITEM_EXTENT)
                .child(Text::new("item"))
                .boxed(),
        )
    });
    let separator_builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|_index: usize| {
        Some(
            SizedBox::height(ITEM_EXTENT)
                .child(Text::new("separator"))
                .boxed(),
        )
    });
    CustomScrollView::new((SliverList::separated(
        2,
        ITEM_EXTENT,
        item_builder,
        separator_builder,
    ),))
}

/// Flutter parity: `slivers_test.dart` `'SliverList.separated has correct
/// number of children'` (tag `3.44.0`).
///
/// Mutation-checked: swapping the expected counts (2 items / 1 separator ->
/// 1 item / 2 separators) flips this test red, confirming `find_all_text`
/// genuinely counts distinct nodes by content rather than degenerating to a
/// constant.
#[test]
fn sliver_list_separated_has_correct_number_of_children() {
    let mut laid = harness::pump_widget(sliver_list_separated_count_scene(), harness::screen());
    settle(&mut laid);

    assert_eq!(
        laid.find_all_text("item").len(),
        2,
        "2 items must be built (oracle: findsNWidgets(2))"
    );
    assert_eq!(
        laid.find_all_text("separator").len(),
        1,
        "1 separator must be built between the 2 items (oracle: findsNWidgets(1))"
    );
}

// ============================================================================
// CASE 4 — SliverList.list can build children
// ============================================================================

/// Mirrors the oracle's scene inline in the `SliverList.list`-bodied
/// `'SliverList.list can build children'` case (`slivers_test.dart`, tag
/// `3.44.0` — see the module doc's ledger entry 4 for why only this one of
/// the two identically-titled oracle cases is ported here): a
/// `CustomScrollView` over one `SliverList::list` sliver of two pre-built
/// tap targets.
fn sliver_list_list_tap_scene(counter0: Arc<AtomicUsize>, counter1: Arc<AtomicUsize>) -> impl View {
    const ITEM_EXTENT: f32 = 100.0;
    let item0 = tap_target(ITEM_EXTENT, "Index 0", even_or_odd_color(0), move || {
        counter0.fetch_add(1, Ordering::SeqCst);
    });
    let item1 = tap_target(ITEM_EXTENT, "Index 1", even_or_odd_color(1), move || {
        counter1.fetch_add(1, Ordering::SeqCst);
    });
    CustomScrollView::new((SliverList::list(ITEM_EXTENT, vec![item0, item1]),))
}

/// Flutter parity: `slivers_test.dart` `'SliverList.list can build
/// children'` (tag `3.44.0` — the oracle has TWO identically-titled cases
/// in this file; this ports only the one whose body constructs
/// `SliverList.list(...)`, see the module doc's ledger entry 4).
#[test]
fn sliver_list_list_hit_tests_children_by_position() {
    let counter0 = Arc::new(AtomicUsize::new(0));
    let counter1 = Arc::new(AtomicUsize::new(0));

    let mut laid = harness::pump_widget(
        sliver_list_list_tap_scene(Arc::clone(&counter0), Arc::clone(&counter1)),
        harness::screen(),
    );
    settle(&mut laid);

    assert_taps_are_mutually_exclusive(&laid, &counter0, &counter1);
}
