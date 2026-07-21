//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart` `SliverList`
//! - Render object: `packages/flutter/lib/src/rendering/sliver_list.dart`
//!   `RenderSliverList`
//! - Tests: `packages/flutter/test/widgets/sliver_list_test.dart` (tag
//!   `3.44.0`, 8 `testWidgets` cases)
//!
//! Widget → render-object mapping:
//! - `SliverList`/`SliverList.builder` → FLUI [`SliverList`] (the lazy-sliver
//!   adaptor `View` defined in `flui-view`, re-exported here) →
//!   `RenderSliverList` (`crates/flui-objects/src/sliver/sliver_list.rs`).
//! - Box leaves → `RenderConstrainedBox` (`SizedBox`) wrapping
//!   `RenderParagraph` (`Text`).
//!
//! # Reconciliation mechanism (investigated before porting, per task
//! instructions)
//!
//! FLUI's lazy `SliverList` does **not** use the keyed dense reconciler
//! (`flui-view/src/tree/id_reconcile.rs`, the one `Stack`/`Flow`/`Table`'s
//! flat multi-child element goes through). It opts out entirely —
//! `RenderView::has_children()` returns `false` and `visit_child_views` is a
//! no-op (`crates/flui-view/src/element/sliver_adaptor.rs`) — and instead
//! uses `SparseChildren` (`crates/flui-view/src/element/sparse_children.rs`),
//! whose own module doc states: "Children are keyed by *logical index* ...
//! not by dense slot". `SparseChildren::ensure` is explicitly documented as
//! "Idempotent: a second call for an already-built index returns the
//! existing id and does **not** rebuild (reconciling a changed `view` is a
//! later concern — Flutter's `updateChild`)" — i.e. a resident lazy child is
//! never re-diffed against a new view at all, key or no key.
//!
//! Two compounding gaps, both confirmed by reading the source (not
//! guessed), make the oracle's "(with keys)" scenarios unportable as
//! specified, independent of each other:
//!
//! 1. **No public per-item view-key API exists.** Flutter's oracle keys
//!    each item with `ValueKey<int>(items[i])`. FLUI has a real
//!    `ValueKey<T>` type (`flui_foundation::key::ValueKey`, re-exported at
//!    `flui_view::ValueKey`) and a real Flutter-faithful keyed-matching
//!    engine (`id_reconcile.rs`'s "Matching semantics (Flutter-faithful)"),
//!    but the generic attachment combinator, `Keyed<V>`
//!    (`flui_foundation::key::Keyed`, produced by
//!    `WithKey::with_value_key`), has **no `View` implementation anywhere
//!    in the workspace** — confirmed by an exhaustive `impl View for
//!    Keyed` search across every crate; the only matches are unrelated
//!    test-local types that shadow the name. Nor does `SizedBox`/`Text`
//!    carry a bespoke `.key(...)` builder of their own (unlike
//!    `AnimatedSwitcher`'s purpose-built `KeyedEntry`). There is therefore
//!    no way, through any public composition API, to attach a `ValueKey`
//!    to a plain leaf view at all — `Keyed<SizedBox>` cannot be used
//!    anywhere a `View`/`BoxedView`/`IntoView` is expected; it does not
//!    compile.
//! 2. **Independent of (1):** `SliverListAdaptorManager`'s item-builder
//!    closure (`crates/flui-view/src/element/sliver_adaptor.rs`) is
//!    captured exactly once, in `SliverListAdaptorBehavior::new`, which
//!    only runs on the element's first mount (`SliverList::create_element`).
//!    Neither `on_update` nor `on_view_updated` (both present on
//!    `SliverListAdaptorBehavior`) ever refresh `self.manager`'s stored
//!    `builder` from the new `SliverList` view — both delegate wholly to
//!    the generic `RenderBehavior<SliverList>`, which only pushes
//!    `item_count` into the render object
//!    (`RenderSliverList::set_item_count`). So even a **keyless** rebuild
//!    that swaps a `SliverList`'s backing item list/builder in place (a
//!    `pump_widget` root-swap, same type/slot, matching Flutter's own
//!    `ListView.builder(itemBuilder: newClosure)`) leaves any
//!    **already-resident** child showing its stale old content forever
//!    (gap 1's "idempotent, no rebuild"), and any **newly-requested**
//!    child (one that scrolls into view for the first time after the
//!    swap) is still built from the **old, pre-swap** closure — the new
//!    items never reach the sliver at all post-mount.
//!
//! Both are confirmed empirically below, not just by static reading: every
//! oracle case that mutates a `SliverList`'s backing item list/builder
//! *after* the first `pump_widget` was red-checked against gap 2. Five of
//! them (cases 1, 2, 5, 6, 7) actually trip it and are pinned `#[ignore]`d
//! rather than narrowed to pass; case 3 (dropping only the LAST item)
//! turns out not to exercise gap 2 at all and is a genuine pass — see its
//! own entry in the ledger below for why. Filed as a new Cross.H entry in
//! `docs/ROADMAP.md` (search `SliverListAdaptorManager`).
//!
//! A third candidate finding was raised and then RETRACTED during review: an
//! earlier draft of this port misdiagnosed a probe artifact as a FLUI
//! cache-window divergence. `LaidOut::find_text` is a pure **residency**
//! probe — it walks the entire live render tree
//! (`PipelineOwner::render_tree`), matching any mounted node regardless of
//! whether it is currently painted. Flutter's `find.text(...)` is NOT a
//! residency probe: it defaults `skipOffstage: true`
//! (`packages/flutter_test/lib/src/finders.dart`, tag `3.44.0`), which
//! routes traversal through `Element.debugVisitOnstageChildren`
//! (`.../tree_traversal.dart`'s `_DepthFirstElementTreeIterator`) instead of
//! `Element.visitChildren`. For a lazy sliver's element,
//! `SliverMultiBoxAdaptorElement.debugVisitOnstageChildren`
//! (`packages/flutter/lib/src/widgets/sliver.dart`, tag `3.44.0`) filters to
//! children satisfying a STRICT overlap test against
//! `[scrollOffset, scrollOffset + remainingPaintExtent)` — the
//! strictly-visible paint window, boundary-touching excluded — which is
//! narrower than the cache-retention window (`remainingCacheExtent`) a
//! lazy sliver actually keeps resident. So `find.text(...) findsNothing` on
//! a lazy-sliver child proves only "not currently painted", never "not
//! resident" — cache-retained-but-offscreen children (e.g. the boundary
//! item just behind the visible window after a large scroll jump) are
//! invisible to it by design. Re-verified by hand-tracing Flutter's own
//! `RenderViewport.layoutChildSequence` + `RenderSliverList.performLayout`
//! (`.../rendering/viewport.dart`/`sliver_list.dart`, tag `3.44.0`): for a
//! jump straight to offset 800 (the scene cases 5/6 use), Flutter's own
//! formula computes the identical retained band FLUI produces
//! (`scrollOffset - cacheOrigin` = 550, `targetEndScrollOffset` = 1250 →
//! indices 11–19) — FLUI's cache behavior was never the divergence; the
//! test's own probe was asking a different question than the oracle's
//! assertion. [`is_onstage`]/[`is_onstage_text`] below are the corrected,
//! test-only substitute — they intersect a node's absolute paint rect
//! against the viewport's own on-screen band, reproducing
//! `debugVisitOnstageChildren`'s exact strict-overlap predicate. A
//! shared-harness version (in `tests/common/mod.rs`) is a natural follow-up
//! if another lazy-sliver port needs the same distinction — not attempted
//! here, since this file's own copy is sufficient and keeps the diff
//! scoped to the task at hand.
//!
//! # Divergences (beyond the reconciliation gaps above)
//!
//! - Every scroll-position change is programmatic
//!   (`ScrollController::jump_to`/`Viewport::position`), never a real
//!   `tester.drag(...)` gesture — FLUI's headless sliver `Viewport` has no
//!   interactive drag-to-scroll wiring (`Viewport`'s own module doc), the
//!   same known limitation every sibling port in this directory works
//!   around. Where the oracle drags by an overshooting delta that the
//!   `Scrollable`'s clamping physics resolves to an exact
//!   `max_scroll_extent`, this port jumps directly to that
//!   independently-computed value (documented per-test).
//! - `find.text(...)` presence maps to [`is_onstage_text`]
//!   (`find_text` composed with the onstage-rect check), NOT to bare
//!   `LaidOut::find_text` — see the finding above for why the two are not
//!   interchangeable for a lazy sliver's off-window children. Bare
//!   `find_text` is used directly only where the oracle's own assertion is
//!   about a value that is either comfortably onstage either way or
//!   genuinely absent from the whole tree (not merely offstage) under the
//!   scenario being asserted — documented per assertion where it matters.
//! - Case 7's tail `Key('key0')`/`Key('key1')` identity checks are ported
//!   using logical-index/text identity instead of real keys (gap 1 above);
//!   the case is otherwise ported faithfully (same item geometry, same
//!   scroll arithmetic, same "does layout complete without corrupting
//!   state when both resident children lose their layout offset"
//!   regression target).
//!
//! Ported (7 of 8 upstream names attempted; 2 real/green, 5 red-checked
//! `#[ignore]`d pins, all five confirmed rooted in gap 2 specifically — not
//! merely "still failing somewhere" — see the per-test doc comment for
//! each one's exact evidence). Total recounted against this file's own
//! `#[test]` functions: 7 attempted + 1 out-of-scope = 8, matching the
//! oracle's own count.
//! - `'SliverList reverse children (with keys)'` →
//!   [`sliver_list_reverse_children_keeps_scroll_offset_and_shows_reversed_window`]
//!   (`#[ignore]`d — gap 2; its first assertion block, before any
//!   `pump_widget`, is real and passes).
//! - `'SliverList replace children (with keys)'` →
//!   [`sliver_list_replace_children_keeps_scroll_offset_and_shows_new_values`]
//!   (`#[ignore]`d — gap 2; its first assertion block also passes).
//! - `'SliverList replace with shorter children list (with keys)'` →
//!   [`sliver_list_replace_with_shorter_list_shifts_scroll_offset_by_removed_extent`]
//!   — **real, green**. Dropping the LAST item never changes any surviving
//!   index's VALUE (only tail eviction and the offset reclamp are
//!   exercised, both gap-2-independent), so this scenario happens not to
//!   need the builder refresh gap 2 is missing; see its own doc comment for
//!   why that is a genuine (not vacuous) pass.
//! - `'SliverList should layout first child in case of child reordering'`
//!   → [`sliver_list_reordering_two_items_keeps_both_visible`] — **real,
//!   green**, but a weaker regression guard than the oracle intends; see
//!   its own doc comment.
//! - `'SliverList should recalculate inaccurate layout offset case 1'` →
//!   [`sliver_list_recalculates_offset_when_item_prepended_while_scrolled_to_end`]
//!   (`#[ignore]`d — gap 2, confirmed at the post-insert block: value 15
//!   should be onstage at the shifted-by-one index 16, but the stale
//!   builder never builds it — its pre-insert instance survives offstage
//!   at old index 15, and no onstage `"Tile 15"` ever appears).
//! - `'SliverList should recalculate inaccurate layout offset case 2'` →
//!   [`sliver_list_recalculates_offset_when_items_swapped_while_scrolled_to_end`]
//!   (`#[ignore]`d — gap 2, confirmed at the post-swap block: `"Tile 3"`,
//!   the value swapped into the resident band, never gets built anywhere).
//! - `'SliverList should start to perform layout from the initial child
//!   when there is no valid offset'` (regression test for
//!   flutter/flutter#66198) →
//!   [`sliver_list_falls_back_to_initial_child_when_no_valid_layout_offset_survives`]
//!   (`#[ignore]`d — gap 2, confirmed at the post-shrink block: `"Tile 0"`
//!   (stale pre-shrink content) is still onstage where the reclamped
//!   `"Marker 0"`/`"Marker 1"` should be; the offset-reclamp half is
//!   independently real, per `scroll_controller_test.rs`'s
//!   `resizing_the_viewport_reclamps_an_already_scrolled_position`).
//!
//! Out of scope (1 upstream name):
//! - `'SliverList.builder respects semanticIndexOffset'` — no
//!   `IndexedSemantics`/`semanticIndexOffset` concept exists anywhere in
//!   FLUI (confirmed by an exhaustive workspace search: the only match for
//!   either name is an unrelated reference-hierarchy markdown doc, no
//!   code), and `flui-widgets`' headless harness has no semantics-tree
//!   assertion capability at all — the same standing gap already named as
//!   out-of-scope by every other port in this directory that touches
//!   semantics.
//!
//! Content sweep (`git -C /mnt/data/dev/flutter grep -l SliverList 3.44.0
//! -- packages/flutter/test/`, beyond the oracle file above): 49 files hit
//! the string. `packages/flutter/test/widgets/slivers_test.dart` alone
//! carries 8 more `testWidgets('SliverList...')` cases of its own —
//! `'SliverList can handle inaccurate scroll offset due to changes in
//! children list'`, `'SliverList handles 0 scrollOffsetCorrection'`,
//! `'SliverList.builder can build children'`, and others — plus
//! `sliver_fixed_extent_list_test.dart`, `sliver_padding_test.dart` (its
//! sliver children are frequently `SliverList`, already covered by
//! `sliver_padding_test.rs`), and Material-library tests (out of the
//! corpus; FLUI has no Material parity program yet). This unit scopes
//! itself strictly to the dedicated `sliver_list_test.dart` file (8 cases
//! above); the `slivers_test.dart` `SliverList`-subject cases are NOT
//! ported here and remain permanently unaccounted for until a future
//! `slivers_test.dart` parity unit picks them up — flagged explicitly so
//! this scoping decision is not silently lost.

use std::rc::Rc;

use flui_foundation::RenderId;
use flui_types::layout::AxisDirection;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{ScrollController, SizedBox, SliverList, Text, Viewport};

use crate::common::LaidOut;
use crate::harness;

// ============================================================================
// SHARED SCENE BUILDERS
// ============================================================================

/// Mirrors the oracle's `_buildSliverList` helper: a `Viewport` over one
/// lazy `SliverList`, each item a `SizedBox(height: item_height,
/// child: Text('Tile {value}'))`, driven by `controller`'s `ScrollPosition`.
///
/// `CustomScrollView` is not used here (unlike most sibling ports) because
/// it has no `.position(ScrollPosition)` passthrough — only a plain
/// `.offset(f32)` — and these tests need a live `ScrollController` to read
/// `.pixels()` back. `CustomScrollView::build` itself composes down to
/// exactly this `Viewport` shape, so mounting it directly is not a
/// divergence, matching the precedent set by `sliver_padding_test.rs`.
///
/// A fresh builder closure is created on every call (capturing `items` by
/// value), matching the oracle's own `_buildSliverList` reconstructing a
/// fresh `SliverChildBuilderDelegate` closure on every `pumpWidget` call.
fn sliver_list_scene(
    items: Vec<i32>,
    item_height: f32,
    controller: &ScrollController,
) -> impl View {
    let items = Rc::new(items);
    let item_count = items.len();
    let builder = {
        let items = Rc::clone(&items);
        move |index: usize| -> Option<BoxedView> {
            items.get(index).map(|&value| {
                SizedBox::height(item_height)
                    .child(Text::new(format!("Tile {value}")))
                    .boxed()
            })
        }
    };
    Viewport::new((SliverList::new(item_count, item_height, Rc::new(builder)),))
        .axis_direction(AxisDirection::TopToBottom)
        .position(controller.position())
}

/// Mirrors the oracle's `_buildSliverListRenderWidgetChild` helper: a
/// `Viewport` over one lazy `SliverList.builder`-style sliver of
/// `String`-valued items, no explicit per-item height (natural content
/// size), matching `SizedBox(key:, child: Text('Tile {value}'))`.
///
/// `key:` is dropped — see the module doc's gap 1 (no per-item view-key API
/// exists to express it).
fn sliver_list_indexed_scene(items: Vec<String>) -> impl View {
    // The oracle's `SizedBox(key:, child: Text(...))` gives no explicit
    // height, so there is no "real" number to match here — a fixed,
    // arbitrary positive extent standing in for "whatever the natural
    // content size would be" (this scene's tests assert presence only,
    // never exact geometry). Also satisfies the lazy sliver's invariant
    // that each child own its own render node directly: a bare `Text`
    // (composed through a nested element) does not, and trips
    // `SparseChildren::ensure`'s "lazy sliver child must own a render
    // node" panic — `SizedBox` (backed by `RenderConstrainedBox`) does.
    const ITEM_HEIGHT: f32 = 20.0;

    let items = Rc::new(items);
    let item_count = items.len();
    let builder = {
        let items = Rc::clone(&items);
        move |index: usize| -> Option<BoxedView> {
            items.get(index).map(|value| {
                SizedBox::height(ITEM_HEIGHT)
                    .child(Text::new(format!("Tile {value}")))
                    .boxed()
            })
        }
    };
    Viewport::new((SliverList::new(item_count, ITEM_HEIGHT, Rc::new(builder)),))
        .axis_direction(AxisDirection::TopToBottom)
        .offset(0.0)
}

/// Mirrors the oracle's `buildSliverList` closure inside case 7: a
/// `Viewport` over one lazy sliver whose item count and content flip
/// between 23 items (20 numbered 50px tiles + a zero-size placeholder + two
/// more 50px "tail" tiles) and just the 3 tail items, depending on
/// `show_numbered_items`.
///
/// The oracle's tail items carry `Key('key0')`/`Key('key1')`; ported here
/// as plain text identity (`"Marker 0"`/`"Marker 1"`) — see the module
/// doc's gap 1.
fn sliver_list_shrinking_tail_scene(
    show_numbered_items: bool,
    controller: &ScrollController,
) -> impl View {
    const ITEM_HEIGHT: f32 = 50.0;
    let item_count: usize = if show_numbered_items { 23 } else { 3 };
    let builder = move |index: usize| -> Option<BoxedView> {
        if show_numbered_items {
            match index {
                0..=19 => Some(
                    SizedBox::height(ITEM_HEIGHT)
                        .child(Text::new(format!("Tile {index}")))
                        .boxed(),
                ),
                // Occupies the slot that sits at offset 0 once the numbered
                // tiles are gone, matching the oracle's own comment.
                20 => Some(SizedBox::shrink().boxed()),
                21 => Some(
                    SizedBox::height(ITEM_HEIGHT)
                        .child(Text::new("Marker 0"))
                        .boxed(),
                ),
                22 => Some(
                    SizedBox::height(ITEM_HEIGHT)
                        .child(Text::new("Marker 1"))
                        .boxed(),
                ),
                _ => None,
            }
        } else {
            match index {
                0 => Some(SizedBox::shrink().boxed()),
                1 => Some(
                    SizedBox::height(ITEM_HEIGHT)
                        .child(Text::new("Marker 0"))
                        .boxed(),
                ),
                2 => Some(
                    SizedBox::height(ITEM_HEIGHT)
                        .child(Text::new("Marker 1"))
                        .boxed(),
                ),
                _ => None,
            }
        }
    };
    Viewport::new((SliverList::new(item_count, ITEM_HEIGHT, Rc::new(builder)),))
        .axis_direction(AxisDirection::TopToBottom)
        .position(controller.position())
}

/// Drives the lazy virtualizer's request → service → re-layout settle
/// sequence to completion. Two ticks, matching the established convention
/// in `list_view_test.rs` (see that file's module doc for the rationale:
/// lazy children build *after* paint, so a triggering change needs one
/// tick to emit build requests and a second to re-lay-out with them built).
fn settle(laid: &mut LaidOut) {
    laid.tick();
    laid.tick();
}

/// Whether `id`'s absolute paint rect overlaps the viewport's own on-screen
/// band `[0, viewport_height)` on the main (vertical) axis — the FLUI
/// equivalent of Flutter's `find.text(..., skipOffstage: true)` (the
/// default every un-annotated `find.text(...)` call in the oracle uses).
///
/// Flutter's `skipOffstage: true` does not walk the full element tree; for
/// a lazy sliver it routes through
/// `SliverMultiBoxAdaptorElement.debugVisitOnstageChildren`
/// (`packages/flutter/lib/src/widgets/sliver.dart`, tag `3.44.0`), whose
/// filter is a STRICT overlap test against
/// `[scrollOffset, scrollOffset + remainingPaintExtent)` — a child whose
/// paint rect only touches the window boundary (zero-area overlap) is
/// offstage. A child's absolute Y position already has the current scroll
/// offset folded in (`child_paint_offset`'s `-pixels` translation, cited in
/// `scroll_controller_test.rs`), so the window becomes exactly
/// `[0, viewport_height)` in that same coordinate space — no separate
/// `scrollOffset` term to track here.
fn is_onstage(laid: &LaidOut, id: RenderId, viewport_height: f32) -> bool {
    let top = laid.absolute_offset(id).dy.get();
    let bottom = top + laid.size(id).height.get();
    top < viewport_height && bottom > 0.0
}

/// `find_text` (a residency probe — does the node exist anywhere in the
/// render tree, regardless of paint-window overlap) composed with
/// [`is_onstage`] — the direct substitute for the oracle's
/// `find.text(...)` default (`skipOffstage: true`). See [`is_onstage`]'s
/// doc for why these are NOT the same question, and the module doc's
/// Divergences section for which probe each assertion in this file uses
/// and why.
fn is_onstage_text(laid: &LaidOut, text: &str, viewport_height: f32) -> bool {
    laid.find_text(text)
        .is_some_and(|id| is_onstage(laid, id, viewport_height))
}

// ============================================================================
// CASE 1 — reverse children (with keys)
// ============================================================================

/// Flutter parity: `sliver_list_test.dart` `'SliverList reverse children
/// (with keys)'` (tag `3.44.0`).
///
/// **Red-checked, `#[ignore]`d** — confirmed failing (not merely
/// hypothesized) by running it un-ignored: after the `pump_widget` with the
/// reversed item list, `find_text("Tile 1")`/`find_text("Tile 0")` are
/// absent and the STALE pre-reversal content (`"Tile 19"`/`"Tile 18"`) is
/// still what's resident, because `SliverListAdaptorManager`'s builder
/// closure never refreshes past first mount (module doc gap 2). This is
/// not a bare-residency probe artifact (the module doc's onstage-vs-
/// residency finding does not apply here): at `SCROLL_POSITION = 5400`
/// with `ITEM_HEIGHT = 300`, item 19 (`[5700, 6000)`) fully overlaps the
/// strictly-visible window `[5400, 5900)`, so it is genuinely onstage with
/// the wrong (stale) content, not merely cache-resident-but-offscreen.
/// Filed to Cross.H (`docs/ROADMAP.md`, `SliverListAdaptorManager`).
#[test]
#[ignore = "documented divergence: SliverListAdaptorManager's item-builder \
            closure is captured once at mount and never refreshed on a \
            pump_widget root-swap — see module doc gap 2; filed to Cross.H"]
fn sliver_list_reverse_children_keeps_scroll_offset_and_shows_reversed_window() {
    let items: Vec<i32> = (0..20).collect();
    const ITEM_HEIGHT: f32 = 300.0;
    const VIEWPORT_HEIGHT: f32 = 500.0;
    const SCROLL_POSITION: f32 = 18.0 * ITEM_HEIGHT;

    let controller = ScrollController::with_initial_scroll_offset(SCROLL_POSITION);
    let mut laid = harness::pump_widget(
        sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    settle(&mut laid);

    assert_eq!(controller.pixels(), SCROLL_POSITION);
    assert!(laid.find_text("Tile 0").is_none());
    assert!(laid.find_text("Tile 1").is_none());
    assert!(laid.find_text("Tile 18").is_some());
    assert!(laid.find_text("Tile 19").is_some());

    let reversed: Vec<i32> = items.iter().rev().copied().collect();
    laid.pump_widget(sliver_list_scene(reversed, ITEM_HEIGHT, &controller));
    settle(&mut laid);

    assert_eq!(controller.pixels(), SCROLL_POSITION);
    assert!(laid.find_text("Tile 19").is_none());
    assert!(laid.find_text("Tile 18").is_none());
    assert!(laid.find_text("Tile 1").is_some());
    assert!(laid.find_text("Tile 0").is_some());

    controller.jump_to(0.0);
    laid.pump();
    settle(&mut laid);

    assert_eq!(controller.pixels(), 0.0);
    assert!(laid.find_text("Tile 19").is_some());
    assert!(laid.find_text("Tile 18").is_some());
    assert!(laid.find_text("Tile 1").is_none());
    assert!(laid.find_text("Tile 0").is_none());
}

// ============================================================================
// CASE 2 — replace children (with keys)
// ============================================================================

/// Flutter parity: `sliver_list_test.dart` `'SliverList replace children
/// (with keys)'` (tag `3.44.0`).
///
/// **Red-checked, `#[ignore]`d** — same root cause as case 1 (module doc
/// gap 2), and same non-artifact confirmation: this scene uses the
/// identical `SCROLL_POSITION`/`ITEM_HEIGHT` as case 1, so item 18
/// (`[5400, 5700)`) is likewise genuinely onstage within `[5400, 5900)`.
/// After replacing the item list with `items.map(|i| i + 100)`, the stale
/// pre-swap builder means `"Tile 18"`/`"Tile 19"` remain resident (and
/// onstage, with the wrong content) and `"Tile 118"`/`"Tile 119"` never
/// appear anywhere. Filed to Cross.H.
#[test]
#[ignore = "documented divergence: SliverListAdaptorManager's item-builder \
            closure is captured once at mount and never refreshed on a \
            pump_widget root-swap — see module doc gap 2; filed to Cross.H"]
fn sliver_list_replace_children_keeps_scroll_offset_and_shows_new_values() {
    let items: Vec<i32> = (0..20).collect();
    const ITEM_HEIGHT: f32 = 300.0;
    const VIEWPORT_HEIGHT: f32 = 500.0;
    const SCROLL_POSITION: f32 = 18.0 * ITEM_HEIGHT;

    let controller = ScrollController::with_initial_scroll_offset(SCROLL_POSITION);
    let mut laid = harness::pump_widget(
        sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    settle(&mut laid);

    assert_eq!(controller.pixels(), SCROLL_POSITION);
    assert!(laid.find_text("Tile 18").is_some());
    assert!(laid.find_text("Tile 19").is_some());

    let shifted: Vec<i32> = items.iter().map(|value| value + 100).collect();
    laid.pump_widget(sliver_list_scene(shifted, ITEM_HEIGHT, &controller));
    settle(&mut laid);

    assert_eq!(controller.pixels(), SCROLL_POSITION);
    assert!(laid.find_text("Tile 18").is_none());
    assert!(laid.find_text("Tile 19").is_none());
    assert!(laid.find_text("Tile 118").is_some());
    assert!(laid.find_text("Tile 119").is_some());

    controller.jump_to(0.0);
    laid.pump();
    settle(&mut laid);

    assert_eq!(controller.pixels(), 0.0);
    assert!(laid.find_text("Tile 100").is_some());
    assert!(laid.find_text("Tile 101").is_some());
    assert!(laid.find_text("Tile 118").is_none());
    assert!(laid.find_text("Tile 119").is_none());
}

// ============================================================================
// CASE 3 — replace with shorter children list (with keys)
// ============================================================================

/// Flutter parity: `sliver_list_test.dart` `'SliverList replace with
/// shorter children list (with keys)'` (tag `3.44.0`).
///
/// **Real, green** — genuinely, not vacuously: gap 2 (module doc — the
/// item-builder never refreshes on a `pump_widget` root-swap) does NOT
/// block this specific scenario, because dropping only the LAST item never
/// changes any SURVIVING index's value (`items[0..len-1]` is a prefix of
/// the original list) — there is nothing for a stale builder to get wrong.
/// What this scenario actually exercises, and what genuinely passes: (1)
/// `RenderSliverList::set_item_count` correctly shrinks `item_count`
/// independent of the builder gap, so the now-out-of-bounds trailing child
/// (`"Tile 19"`) is evicted for real — witnessed by the closing residency
/// assert (`find_text` finds no instance anywhere in the tree, which the
/// onstage probe alone could not distinguish from retained-but-offstage);
/// (2) the scroll-offset reclamp
/// (`scroll_position - ITEM_HEIGHT`) fires through the same real
/// `apply_content_dimensions` path `scroll_controller_test.rs`'s
/// `resizing_the_viewport_reclamps_an_already_scrolled_position` already
/// pins. Both are real, working FLUI behavior, confirmed by running this
/// exact scenario, not assumed.
#[test]
fn sliver_list_replace_with_shorter_list_shifts_scroll_offset_by_removed_extent() {
    let items: Vec<i32> = (0..20).collect();
    const ITEM_HEIGHT: f32 = 300.0;
    const VIEWPORT_HEIGHT: f32 = 500.0;
    let scroll_position: f32 = items.len() as f32 * ITEM_HEIGHT - VIEWPORT_HEIGHT;

    let controller = ScrollController::with_initial_scroll_offset(scroll_position);
    let mut laid = harness::pump_widget(
        sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    settle(&mut laid);

    assert_eq!(controller.pixels(), scroll_position);
    assert!(!is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));

    let shorter: Vec<i32> = items[..items.len() - 1].to_vec();
    laid.pump_widget(sliver_list_scene(shorter, ITEM_HEIGHT, &controller));
    settle(&mut laid);

    assert_eq!(controller.pixels(), scroll_position - ITEM_HEIGHT);
    assert!(is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(!is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));
    // Residency, not just onstage-ness: the onstage probe above cannot see
    // eviction (item 19's slot is outside the reclamped window whether the
    // child was disposed or retained) — this pins that the out-of-bounds
    // child is genuinely gone from the tree.
    assert!(
        laid.find_text("Tile 19").is_none(),
        "the out-of-item_count trailing child must be evicted, not retained"
    );
}

// ============================================================================
// CASE 4 — layout first child in case of child reordering
// ============================================================================

/// Flutter parity: `sliver_list_test.dart` `'SliverList should layout first
/// child in case of child reordering'` (tag `3.44.0`) — regression test for
/// <https://github.com/flutter/flutter/issues/35904>.
///
/// **Real, green** — but a strictly weaker regression guard than the
/// oracle's, and honestly so: with only 2 small items in a 500px viewport,
/// neither item ever leaves the resident/visible band across the reorder,
/// so `SliverListAdaptorManager` never calls its (stale — module doc gap 2)
/// builder again at all; both items simply stay mounted from the first
/// build regardless of order. `find_text` presence alone cannot
/// distinguish "correctly re-laid-out after reordering" from "untouched
/// because nothing was ever re-requested" — both produce the same
/// observation here. The `find_all_by_render_type` count below is added
/// beyond the oracle's own assertions to at least rule out the one failure
/// mode this scenario COULD still expose (a duplicate/leaked child from a
/// broken reorder), which is a real, meaningful (if narrower) regression
/// guard.
#[test]
fn sliver_list_reordering_two_items_keeps_both_visible() {
    let mut laid = harness::pump_widget(
        sliver_list_indexed_scene(vec!["1".to_string(), "2".to_string()]),
        harness::screen_of(800.0, 500.0),
    );
    settle(&mut laid);

    assert!(laid.find_text("Tile 1").is_some());
    assert!(laid.find_text("Tile 2").is_some());
    assert_eq!(
        laid.find_all_by_render_type("RenderParagraph").len(),
        2,
        "exactly 2 items should be resident before the reorder"
    );

    laid.pump_widget(sliver_list_indexed_scene(vec![
        "2".to_string(),
        "1".to_string(),
    ]));
    settle(&mut laid);

    assert!(laid.find_text("Tile 1").is_some());
    assert!(laid.find_text("Tile 2").is_some());
    assert_eq!(
        laid.find_all_by_render_type("RenderParagraph").len(),
        2,
        "reordering must not duplicate or leak a resident child"
    );
}

// ============================================================================
// CASES 5 & 6 — recalculate inaccurate layout offset
// ============================================================================

/// Flutter parity: `sliver_list_test.dart` `'SliverList should recalculate
/// inaccurate layout offset case 1'` (tag `3.44.0`) — regression test for
/// <https://github.com/flutter/flutter/issues/42142>.
///
/// **Red-checked, `#[ignore]`d** — gap 2 (module doc): after the insert,
/// value 15 should occupy the shifted-by-one index 16 and be onstage
/// there, but the stale pre-insert builder never builds it — the stale
/// world instead leaves value 15's pre-insert instance at its old index
/// 15, whose slot is offstage after the insert, so the onstage instance
/// at index 16 never appears and [`is_onstage_text`] stays false either
/// way the failure is read. The first assertion block (offset 800, before the
/// insert) uses [`is_onstage_text`] rather than bare `find_text`, since
/// its boundary items (`"Tile 15"` absent, `"Tile 16"`..`"Tile 19"`
/// present) previously misfired on cache-resident-but-offscreen content —
/// see the module doc's retracted-finding note.
///
/// `tester.drag(find.text('Tile 2'), Offset(0, -1000))` is substituted with
/// a direct `jump_to(800.0)` — the exact value Flutter's own clamping
/// physics resolves that overshooting drag to (20 items × 50px = 1000px
/// content in a 200px viewport → `max_scroll_extent` = 800), per this
/// file's module doc.
#[test]
#[ignore = "documented divergence: SliverListAdaptorManager's item-builder \
            closure is captured once at mount and never refreshed on a \
            pump_widget root-swap — see module doc gap 2; filed to Cross.H"]
fn sliver_list_recalculates_offset_when_item_prepended_while_scrolled_to_end() {
    let mut items: Vec<i32> = (0..20).collect();
    const ITEM_HEIGHT: f32 = 50.0;
    const VIEWPORT_HEIGHT: f32 = 200.0;
    // 20 * 50 = 1000px content in a 200px viewport -> max_scroll_extent = 800.
    const DRAG_CLAMPED_OFFSET: f32 = 800.0;

    let controller = ScrollController::new();
    let mut laid = harness::pump_widget(
        sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    settle(&mut laid);

    controller.jump_to(DRAG_CLAMPED_OFFSET);
    laid.pump();
    settle(&mut laid);

    assert_eq!(controller.pixels(), DRAG_CLAMPED_OFFSET);
    assert!(!is_onstage_text(&laid, "Tile 15", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 16", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));

    items.insert(0, -1);
    laid.pump_widget(sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller));
    settle(&mut laid);

    assert_eq!(controller.pixels(), DRAG_CLAMPED_OFFSET);
    assert!(!is_onstage_text(&laid, "Tile 14", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 15", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 16", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(!is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));

    controller.jump_to(0.0);
    laid.pump();
    settle(&mut laid);

    assert_eq!(controller.pixels(), 0.0);
    assert!(is_onstage_text(&laid, "Tile -1", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 0", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 1", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 2", VIEWPORT_HEIGHT));
    assert!(!is_onstage_text(&laid, "Tile 3", VIEWPORT_HEIGHT));
}

/// Flutter parity: `sliver_list_test.dart` `'SliverList should recalculate
/// inaccurate layout offset case 2'` (tag `3.44.0`) — regression test for
/// <https://github.com/flutter/flutter/issues/42142>.
///
/// **Red-checked, `#[ignore]`d** — gap 2 (module doc), confirmed at the
/// post-swap block: `items.swap(3, 19)` moves value 3 into the resident
/// band (index 19), but the stale pre-swap builder never rebuilds that
/// index, so `"Tile 3"` never appears anywhere in the tree — not merely
/// offstage. `"Tile 14"`/`"Tile 15"` immediately before it in the same
/// block are genuinely absent from the STRICT visible window either way
/// (unaffected by the swap, which only touches indices 3 and 19), so they
/// pass under [`is_onstage_text`] regardless of gap 2 — using bare
/// `find_text` there would have wrongly flagged them as failures too, the
/// same probe-artifact class the module doc's retracted finding describes.
///
/// `tester.drag(find.text('Tile 2'), Offset(0, -1000))` is substituted the
/// same way as the sibling case above (`jump_to(800.0)`).
#[test]
#[ignore = "documented divergence: SliverListAdaptorManager's item-builder \
            closure is captured once at mount and never refreshed on a \
            pump_widget root-swap — see module doc gap 2; filed to Cross.H"]
fn sliver_list_recalculates_offset_when_items_swapped_while_scrolled_to_end() {
    let mut items: Vec<i32> = (0..20).collect();
    const ITEM_HEIGHT: f32 = 50.0;
    const VIEWPORT_HEIGHT: f32 = 200.0;
    const DRAG_CLAMPED_OFFSET: f32 = 800.0;

    let controller = ScrollController::new();
    let mut laid = harness::pump_widget(
        sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    settle(&mut laid);

    controller.jump_to(DRAG_CLAMPED_OFFSET);
    laid.pump();
    settle(&mut laid);

    assert_eq!(controller.pixels(), DRAG_CLAMPED_OFFSET);
    assert!(!is_onstage_text(&laid, "Tile 15", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 16", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));

    // Reorders item 19 to the front: this should make item 19 the first
    // child with a layout offset the virtualizer cannot dead-reckon.
    items.swap(3, 19);
    laid.pump_widget(sliver_list_scene(items.clone(), ITEM_HEIGHT, &controller));
    settle(&mut laid);

    assert_eq!(controller.pixels(), DRAG_CLAMPED_OFFSET);
    assert!(!is_onstage_text(&laid, "Tile 14", VIEWPORT_HEIGHT));
    assert!(!is_onstage_text(&laid, "Tile 15", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 16", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 3", VIEWPORT_HEIGHT));
}

// ============================================================================
// CASE 7 — start layout from the initial child when there is no valid offset
// ============================================================================

/// Flutter parity: `sliver_list_test.dart` `'SliverList should start to
/// perform layout from the initial child when there is no valid offset'`
/// (tag `3.44.0`) — regression test for
/// <https://github.com/flutter/flutter/issues/66198>.
///
/// **Red-checked, `#[ignore]`d** — gap 2 (module doc), confirmed at the
/// post-shrink block: the render object's own `item_count` DOES update on
/// `pump_widget` (`RenderSliverList::set_item_count`, wired through the
/// generic `update_render_object`, independent of the builder-staleness
/// gap), so the geometry half genuinely works — `controller.pixels()`
/// reclamps to `0.0` once the shrunk content's `max_scroll_extent` drops
/// below the current position, the same real path
/// `scroll_controller_test.rs`'s
/// `resizing_the_viewport_reclamps_an_already_scrolled_position` pins. But
/// the content half does not: once offset 0 makes the tail band (logical
/// indices 0..3) the resident/onstage band, the manager calls the STALE
/// pre-shrink builder (23-item closure) for those indices, returning
/// `"Tile 0"`/`"Tile 1"`/`"Tile 2"` instead of the expected placeholder +
/// `"Marker 0"` + `"Marker 1"`. `"Tile 0"` is genuinely onstage with this
/// wrong content (item 0 is `[0, 50)`, fully inside the `[0, 200)` visible
/// window) — not a residency-vs-onstage artifact.
///
/// The first assertion block (offset 900, before the shrink) uses
/// [`is_onstage_text`] rather than bare `find_text` for the same reason as
/// cases 5/6: `"Tile 17"` is cache-resident-but-offscreen at that position
/// and would otherwise misfire — see the module doc's retracted-finding
/// note.
///
/// `tester.drag(find.text('Tile 2'), Offset(0, -1000))` is substituted with
/// `jump_to(900.0)` — 20×50 + 0 + 50 + 50 = 1100px content in a 200px
/// viewport → `max_scroll_extent` = 900, matching the oracle's own
/// `expect(controller.offset, 900.0)`.
#[test]
#[ignore = "documented divergence: SliverListAdaptorManager's item-builder \
            closure is captured once at mount and never refreshed on a \
            pump_widget root-swap — see module doc gap 2; filed to Cross.H"]
fn sliver_list_falls_back_to_initial_child_when_no_valid_layout_offset_survives() {
    const VIEWPORT_HEIGHT: f32 = 200.0;
    const DRAG_CLAMPED_OFFSET: f32 = 900.0;

    let controller = ScrollController::new();
    let mut laid = harness::pump_widget(
        sliver_list_shrinking_tail_scene(true, &controller),
        harness::screen_of(800.0, VIEWPORT_HEIGHT),
    );
    settle(&mut laid);

    controller.jump_to(DRAG_CLAMPED_OFFSET);
    laid.pump();
    settle(&mut laid);

    assert_eq!(controller.pixels(), DRAG_CLAMPED_OFFSET);
    assert!(!is_onstage_text(&laid, "Tile 17", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 18", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Marker 0", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Marker 1", VIEWPORT_HEIGHT));

    laid.pump_widget(sliver_list_shrinking_tail_scene(false, &controller));
    settle(&mut laid);

    // Layout must complete without panicking/hanging even though both
    // resident children lose their layout offset in the same pass — the
    // core of the #66198 regression — and the offset must reclamp since
    // the shrunk content's max_scroll_extent (100px) is now below 200px.
    assert_eq!(controller.pixels(), 0.0);
    assert!(!is_onstage_text(&laid, "Tile 0", VIEWPORT_HEIGHT));
    assert!(!is_onstage_text(&laid, "Tile 19", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Marker 0", VIEWPORT_HEIGHT));
    assert!(is_onstage_text(&laid, "Marker 1", VIEWPORT_HEIGHT));
}
