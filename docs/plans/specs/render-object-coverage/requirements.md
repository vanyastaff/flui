# render-object-coverage: requirements

- **Status:** Approved by the owner on 2026-11-25 (the decisions in design.md stand as recommended); tasks in progress, see tasks.md
- **Date:** 2026-11-25
- **Base:** `main` @ `fcab477`
- **Reference:** `flutter/flutter` tag `3.50.1` (`ae9b586`), `packages/flutter/lib/src`. Statements
  about Flutter's code were read from that tree. Statements about its tracker carry a
  flutter/flutter issue or pull-request number: a number marked `†` was seen only as a search
  result (title and first lines), every other one was read as a thread. A root cause a thread
  never confirmed is called unconfirmed.
- **Decisions it builds on:**
  [ADR-0003](../../../adr/ADR-0003-virtualization-core-and-reentrant-build.md),
  [ADR-0017](../../../adr/ADR-0017-build-during-layout-callback-seam.md),
  [ADR-0054](../../../adr/ADR-0054-the-viewport-commits-one-result.md),
  [ADR-0098](../../../adr/ADR-0098-owned-f64-geometry-values.md),
  [ADR-0208](../../../adr/ADR-0208-gesture-ownership-table-and-composition.md),
  [ADR-0209](../../../adr/ADR-0209-nested-scroll-connection.md),
  [ADR-0224](../../../adr/ADR-0224-geometry-aware-semantics-child-selection.md),
  [ADR-0226](../../../adr/ADR-0226-semantics-reading-order-and-sibling-barriers.md),
  [ADR-0265](../../../adr/ADR-0265-pinned-lazy-children-and-the-focusable-list.md),
  [ADR-0276](../../../adr/ADR-0276-a-follower-takes-its-leaders-full-transform-and-may-hide-with-its-clips.md),
  [ADR-0304](../../../adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md),
  [ADR-0308](../../../adr/ADR-0308-layout-build-fixpoint-charges-only-passes-without-progress.md),
  [ADR-0322](../../../adr/ADR-0322-a-control-mounts-one-element-for-its-behaviours.md),
  [ADR-0325](../../../adr/ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md).
  The spec changes one of them: the tap ADR supersedes the part of ADR-0208 §2 rule 4 that
  names who produces a hold, and states the new row of the ownership table itself; ADR-0208
  keeps its text (R12). It builds one thing
  an accepted record decided and the code lacks, ADR-0003's insert and delete in the middle
  of the offset index (R6). It names six new ADRs that must be accepted before the tasks that
  depend on them start ([design.md](design.md), "ADRs this spec needs").
- **Design:** [design.md](design.md) (shared seams, tables, decisions),
  [design-parity.md](design-parity.md) (W1–W18), [design-beyond.md](design-beyond.md) (W19–W26);
  tasks: [tasks.md](tasks.md)

## Why

FLUI's catalog of render objects was grown by need: a widget was wanted, its render object was
written. An application ported from Flutter meets the missing ones as a widget that does not
exist (`TreeSliver`, `ListWheelScrollView`, `TapRegion`, a `WidgetSpan` inside text, a platform
view), and each needs a decision about the render layer before a widget can be written.

The owner's goal has two parts:

1. **Every render object of Flutter 3.50.1 has an answer in FLUI** (R1–R18): the same object, an
   object of a different shape that reaches the same behaviour, or a recorded reason why FLUI
   does not have it. It is not a port (AGENTS.md, "Design stance"): what crosses over is a
   behaviour pinned by a behaviour test, with the defects Flutter's tracker records for it on the
   checklist.
2. **FLUI has the render-layer primitives application authors reach for and Flutter lacks**
   (R19–R26), where one fits FLUI's architecture and its cost is known. Each was judged against
   the same bar; [design-beyond.md](design-beyond.md) lists what was adopted, deferred and
   rejected, with the reason.

Breaking FLUI's public API is allowed (pre-1.0). Every form a task replaces is named in the
design with its migration, and the task that brings the replacement moves every user and deletes
the old form in the same change.

## The coverage baseline

The count is over **concrete public classes named `Render*` in `rendering/` and `widgets/`, plus
`TextureBox` and `PlatformViewRenderBox`**. Abstract bases and mixins are out (among them
`RenderEditablePainter` and `RenderSliverBoxChildManager`), and so are the three names the prefix
catches that are not render objects: `RenderObjectToWidgetAdapter` (a widget),
`RenderObjectToWidgetElement` (an element) and `RenderingFlutterBinding` (a binding).

```sh
cd packages/flutter/lib/src    # flutter/flutter at tag 3.50.1
grep -rhoE '^(abstract |base |final |sealed |interface |mixin )*class Render[A-Za-z0-9_]+' \
    rendering widgets \
  | grep -v abstract | sed -E 's/^(base |final )*class //' \
  | grep -vE '^(RenderObjectToWidgetAdapter|RenderObjectToWidgetElement|RenderingFlutterBinding)$' \
  | sort -u | wc -l            # 97: 91 in rendering/, 6 in widgets/
```

97 and the two named classes make **99**. Against `RENDER_OBJECT_TYPES`
(`crates/flui-objects/tests/render_object_harness.rs`) at `fcab477`:

| Answer in FLUI | Count | Which |
|----------------|:-----:|-------|
| Present | 79 | 74 under the same name; 4 under a FLUI name (`RenderPositionedBox` → `RenderAlign`, `RenderPointerListener` → `RenderListener`, `RenderFractionallySizedOverflowBox` → `RenderFractionallySizedBox`, `TextureBox` → `RenderTexture`); `RenderView` in `flui-rendering`. Three of the 79 have a part missing: `RenderImage` (R10), `RenderParagraph` (R13), `RenderEditable` (R14) |
| No object needed | 1 | `RenderProxyBox`: a base class Dart can instantiate. A FLUI proxy implements the trait and has no base object |
| Covered by a different design | 7 | `RenderBlockSemantics`, `RenderPerformanceOverlay`, `RenderSemanticsGestureHandler`, `RenderNestedScrollViewViewport`, `RenderSliverOverlapAbsorber`, `RenderSliverOverlapInjector`, `RenderTapRegionSurface` (R18) |
| Absent, built by this spec | 7 | `RenderSliverClipRect`, `RenderSliverClipRRect` (R3), `RenderDecoratedSliver` (R4), `RenderSliverVariedExtentList` (R5), `RenderTreeSliver` (R6), `RenderTapRegion` (R12), `RenderListWheelViewport` (R15) |
| Absent, waiting for an owner decision | 5 | `RenderAndroidView`, `RenderUiKitView`, `RenderAppKitView`, `PlatformViewRenderBox`, `RenderWebImage` (R17) |

The abstract `RenderTwoDimensionalViewport` is outside the count and inside the scope (R16), and
so are the render objects Flutter keeps private behind public widgets
([design.md](design.md), "Flutter's private render objects").

Four Flutter objects that look absent by name are answered by code that already exists, or need
more than their name suggests:

| Flutter object | What the code shows | Consequence |
|----------------|---------------------|-------------|
| `RenderBlockSemantics` | `Semantics::block_previous_siblings` sets the sibling barrier of ADR-0226. No `BlockSemantics` widget exists, and three documents say FLUI has none | A widget by composition (R9), no render object |
| `RenderPerformanceOverlay` | `crates/flui-runtime/src/performance_overlay.rs` and `UiRuntime::set_performance_overlay` draw the overlay as a runtime-composed scene node | Covered by a different design (R18) |
| `Magnifier` (`_RenderMagnification`) | `crates/flui-widgets/src/magnifier.rs` has `RawMagnifier`, a composition over `BackdropFilter` and clips | Covered, nothing to build |
| `RenderSliverClipRect`, `RenderSliverClipRRect` | `rendering/sliver_clip.dart` gives them a `ClipOverlapBehavior` | R3 takes the overlap behaviour too |

## Scope

Two tiers. Tier A is Flutter parity, in the order the tasks build it:

1. the harness accepts rows from per-family modules, so parallel tasks stop meeting in one
   16,000-line file (R1);
2. one owner for what a layout decided to show, starting with the open gap "An overflow clip cuts
   off paint but not semantics or input regions" ([gaps.md](../../../gaps.md)) (R2). Every object
   below that clips, hides or transforms a child reads the same answer in paint, hit testing,
   semantics and input regions;
3. the missing sliver objects: clips, decoration, varied-extent list, tree, child-sized headers,
   a sliver layout builder and fractional padding (R3–R8);
4. widgets reached by composing existing objects, and one new box layout (R9);
5. the missing paint properties of `RenderImage` and the subtree filters (R10, R11);
6. tap regions (R12);
7. inline children in `RenderParagraph` and `RenderEditable` (R13, R14);
8. the list wheel and the two-axis viewport (R15, R16);
9. native views, which wait for an owner decision (R17);
10. the record of what stays covered by a different design (R18).

Tier B is beyond Flutter (R19–R26): geometry and visibility observers, sticky rows in lazy lists,
a grid layout over a shared track-sizing module, a wrap that stops and shows an overflow
indicator, one placement policy for floating children, a first-that-fits layout, per-item scroll
effects, and an isolated blend group with a widget mask.

Out of scope: the material and cupertino widgets themselves. [design.md](design.md) lists which
render-layer primitives their private render objects need, so that a catalog task can tell
whether it is blocked on this spec.

## The frozen crates

AGENTS.md, "Build what is accepted before designing further": while ADR-0304, ADR-0305 or
ADR-0306 has a bullet in [gaps.md](../../../gaps.md#accepted-adr-sections-not-built-yet), a change
to `flui-layer`, `flui-rendering`, `flui-painting` or `flui-engine` builds a task of
`docs/plans/specs/render-scaling/` or fixes a defect. At `fcab477` ADR-0304 and ADR-0305 have
bullets and ADR-0306 has none, so the four crates are frozen.

- **G1.** A task of this spec that edits a frozen crate does not start while a bullet stands,
  unless the owner grants that task an exception in `tasks.md`. Such a task is marked *gated*
  there, with the files of the frozen crate it needs. A task that edits no frozen crate and
  depends on a gated one is not gated; it waits on its dependency.
- **G2.** Everything else (`flui-objects`, `flui-view`, `flui-widgets`, `flui-interaction`,
  `flui-semantics`, `flui-runtime`, `packages/`) is ungated and is ordered first.
- **G3.** A gated change is the smallest one that serves its requirement, and it comes with the
  ADR that decides it. No task adds a hook to a frozen crate "for later".

Ungated: R1–R5, R7–R9, R12, R18, R20–R24, the geometry observer of R19 and the first half of
R10. Gated in part: R11, R13–R17, R25, R26, the visibility observer of R19 (two fields on the
leader watch) and the second half of R10. R6 waits on one defect fix in `flui-rendering`
(`Virtualizer::splice`, which ADR-0003 decided and the code lacks); the freeze admits a defect
fix, and the owner confirms that reading ([design.md](design.md), "Decisions for the owner",
12). R5 uses one read that lands with the same fix (decision 13) and R3 may need a defect fix
in `flui-engine`'s clip snapping, only if its readback row shows a seam.

## How a requirement reads

Each requirement names the behaviour tests that hold it. Harness rows are functions of
`crates/flui-objects/tests/` called from a `family_*` table; widget tests are modules of
`crates/flui-widgets/tests/main.rs`. Test names are proposals fixed when the task lands, and
`tasks.md` records the final names as validation evidence.

Under "Flutter's record" each defect has one of two answers:

- **pin**: a behaviour test in the acceptance list fails if FLUI repeats it;
- **avoid**: the design makes it unrepresentable, and the sentence says by what.

## Tier A: Flutter parity

### R1. The catalog rule holds for every new object, and the harness takes parallel work

- Every concrete `RenderBox` or `RenderSliver` this spec adds is exported from
  `crates/flui-objects/src/lib.rs`, listed in `RENDER_OBJECT_TYPES`, has at least one
  `harness_*` row, and has a widget in `flui-widgets` named in the design.
- A harness row may live in a family module file beside the root file, and the two guards
  (`catalog_covers_every_render_object_name`, `render_object_types_match_exports`) still fail
  for an exported object without a row.
- A row counts whatever its visibility, a type counts only under its whole name (a row that
  names a longer type does not cover a shorter one), and a family file counts only when the
  root mounts it: a file that never compiles covers nothing.
- No document repeats the number of exported render types.

Acceptance: `the_guard_counts_a_pub_row_of_a_family_file`,
`the_guard_refuses_a_name_inside_a_longer_one`,
`the_guard_refuses_a_family_file_that_is_not_mounted` (each run over literal sources, and each
failing with the matcher as it stands at `fcab477`), `family_files_are_mounted_and_listed`;
`docs/testing.md` links the list instead of counting it.

### R2. What a layout decided to show is one answer, read by paint, hits, semantics and regions

For `RenderStack`, `RenderIndexedStack`, `RenderWrap`, `RenderFittedBox`, `RenderAnimatedSize`
and `RenderConstraintsTransformBox`, with a clip behaviour other than `Clip::None`:

- a child that overflows the box publishes a semantics rectangle cut to the box, and an input
  region cut to the box;
- a child wholly outside the box has no input region. Its semantics follow the rule every
  authored clip has: it publishes what the same child under a `RenderClipRect` of the box
  publishes. Where no ancestor can reveal it, that is nothing; under a scrollable it stays
  published and hidden;
- when nothing overflows, no clip is reported and no clip scope is pushed;
- the answer that paint, semantics and input regions read is the one written by the last layout
  that returned `Ok`. Every such layout writes it, one that returns early included (no
  children, a hidden index). After a layout that fails or panics the box clips to the size it
  keeps: a failed pass has moved children, so no earlier answer is true of them;
- the objects' public `has_visual_overflow()` reads the same answer.

For every object of this spec that clips, hides or transforms a child (R3, R5, R6, R7, R13, R15,
R16, R20, R21, R22, R24, R25), and for the four lazy slivers `main` has (the carousel, which
already works this way, and the list, the fixed-extent list and the grid, which ask
`PinnedChildren::is_pinned_slot` instead):

- the clip, the transform and whether the child is shown at all are computed in layout and kept
  in one record ([design.md](design.md), "The shown answer"). Paint, hit testing, the
  paint-transform hook, the reported clip and the semantics visit read that record and compute
  nothing of their own;
- a child is in one of three states, and the record is the only place that says which: shown
  (painted, hit, published); announced only (published, not painted, not hit: a lazy child
  kept built for the focusable list, ADR-0265); absent (none of the three);
- a sliver hits a child where it paints it in all four axis directions and both growth
  directions;
- an object commits the record once per layout, whatever number of rules decided it, and one
  child is laid out by one of them;
- after a layout that fails or panics the record is empty: no child is hit or published until
  a layout succeeds, and the picture stays the last one painted;
- no lazy sliver answers "is this slot pinned" anywhere but the record: `PinnedChildren`'s
  `slots`, `is_pinned_slot` and `clear_slots` are gone;
- a type that keeps a record runs the shared row, and a test of the crate fails for one that
  does not.

Acceptance: one row per object for the semantics and region cut-off
(`harness_<object>_cuts_semantics_and_regions_at_its_overflow_clip`),
`harness_<object>_overflow_clip_publishes_what_a_clip_rect_publishes` (one table over the six
objects, alone and under a scrollable, with a child partly outside and one wholly outside),
`harness_an_overflow_box_clips_after_a_layout_that_fails` (a returned error and a panic),
`harness_an_overflow_answer_is_cleared_by_a_layout_that_returns_early` (an overflowing stack
loses its children; an indexed stack's index becomes none),
`harness_no_overflow_reports_no_clip`,
`harness_an_unclipped_overflow_is_painted_and_published_but_hit_only_inside_the_box`,
`harness_a_sliver_part_is_hit_where_it_is_painted` (four axis directions, both growth
directions), `harness_a_lazy_sliver_shows_nothing_after_a_layout_that_fails` (a returned
error and a panic), `harness_overlapping_parts_are_hit_in_reverse_paint_order`,
`harness_a_part_with_a_non_finite_clip_is_not_shown`,
`every_keeper_of_a_shown_record_runs_the_shared_row` with its matcher's test on literal text,
and for every object that keeps a record the shared row
`harness_<object>_hits_and_publishes_a_child_only_where_it_paints`, run alone and under a
scrollable, with a layout that shows and one that takes the early exit. The four lazy
slivers' existing rows stay green over the shared record, with a pinned-slot row for each
(`harness_<sliver>_publishes_its_pinned_child_without_painting_it`, named or added), and `rg
is_pinned_slot crates/` finds nothing. The gap entry leaves [gaps.md](../../../gaps.md) with
the overflow objects' change.

Flutter edge cases: `Clip::None` reports no clip even when content overflows; `RenderFittedBox`
with `BoxFit::None` and an empty child reports no overflow.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #196649 | Eleven render objects answer `paintsChild` true for a child they do not paint (an indexed stack's hidden children, maintained `Visibility`, sliver opacity and offstage, a viewport's cache), so an ink effect keeps painting after its parent stopped | **avoid**: FLUI has no per-object "is this child painted" answer. The pipeline's walks read the paint record of the last paint (`last_child_paint_orders`), and paint draws only what the shown record holds. **pin**: the shared row above |
| #19445 | A `Positioned` child that overflows a `Stack` shows but takes no gesture outside the stack's box; closed as documentation, contested | **pin** the behaviour as decided: an unclipped overflow is painted, published and reports its input regions, and a pointer reaches it only inside its parent's box, because a hit test is bounded by each ancestor's box. "Mapping decisions" states the asymmetry and that the remedy is a parent sized to contain what must be hit |

### R3. A sliver clips its child to a rectangle or a rounded rectangle

`RenderSliverClipRect` and `RenderSliverClipRRect`, widgets `SliverClipRect` and
`SliverClipRRect`.

- The clip is the sliver's maximum paint rectangle: its scroll extent laid along the main axis
  from its paint origin, across its cross-axis extent.
- `ClipOverlapBehavior` says what the clip does where a pinned sliver before it overlaps it:
  `None` keeps the full rectangle, `FollowEdge` moves the leading edge to the end of the
  overlap, `PreserveShape` (rounded clip) keeps the corner radii while the edge moves.
- A point outside the clip does not hit the child. Semantics and input regions of the child are
  cut by the same clip (R2).
- Nothing is painted and no scope is pushed when the child is not visible or the clip is empty.

Acceptance: `harness_sliver_clip_rect_follows_the_pinned_edge`,
`harness_sliver_clip_rrect_preserves_its_shape_under_an_overlap`,
`harness_sliver_clip_rejects_a_hit_outside_the_clip`,
`harness_sliver_clip_of_an_unbounded_child_is_finite`,
`harness_sliver_clip_under_a_narrowed_cross_axis_takes_the_narrow_extent`, each in all four axis
directions; a readback row `adjacent_clipped_slivers_leave_no_seam_at_a_fractional_offset`.

Flutter edge cases to pin: a child whose maximum paint extent is infinite gets a clip bounded by
the cache the geometry reports, never a non-finite rectangle; reversed axis directions flip the
leading edge; the defaults differ (`Clip::HardEdge` for the rectangle, `Clip::AntiAlias` for the
rounded one).

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #179002, PR #179003 | The proposal: a boolean `clipOverlap`, clipped regions take no touches, the two defaults. Its review found no technical defect | **pin** the three behaviours. The three-valued `ClipOverlapBehavior` comes from the 3.50.1 source alone: no user report was found for the corner shape under an overlap, so its rows pin the source's behaviour |
| #129870, #166822† | A seam of a fraction of a pixel between two adjacent slivers at a fractional scroll offset; an anti-aliasing artefact of one backend | **pin** with the readback row. The render object does not snap (ADR-0098 §6: snapping is the engine's, in raster-target coordinates). A seam the row shows is a defect of the engine's clip snapping, fixed there or listed in [gaps.md](../../../gaps.md) with the row |

### R4. A sliver paints a decoration behind or in front of its child

`RenderDecoratedSliver`, widget `DecoratedSliver`.

- The decoration covers the sliver's maximum paint rectangle (R3's rectangle), so it scrolls
  with the content and covers items that are not built. Its corners are the corners of that
  rectangle: a rounded corner sits at the sliver's real start and end, not at the edge of the
  visible slice.
- `DecorationPosition` chooses background or foreground, as for `RenderDecoratedBox`.
- An invisible child paints no decoration.
- A decoration is clipped by the viewport like any other paint: a shadow that reaches outside
  the viewport shows only when the scroll view's `clip_behavior` is `Clip::None`.

Acceptance: `harness_decorated_sliver_covers_its_whole_scroll_extent`,
`harness_decorated_sliver_over_an_unbounded_list_is_finite`,
`harness_decorated_sliver_paints_nothing_when_its_child_is_invisible`,
`harness_decorated_sliver_rounds_its_real_ends_not_the_visible_slice`,
`harness_decorated_sliver_around_a_pinned_child_follows_it` (overscrolled at both ends, forward
and reversed), widget test `a_decorated_slivers_shadow_is_cut_by_the_viewport_clip`.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #107498 | The first attempt sized the decoration from the child's paint extent, so rounded corners sat on the visible slice, and ignored overlap and the horizontal axis | **pin**: the rounded-ends row, in all four axis directions |
| #179801, PR #179802† | A decorated sliver around a pinned header is wrong under bouncing overscroll and `reverse` | **pin**: the pinned-child row |
| #156819 | A shadow is cut on all sides by the scroll view's hard-edge clip; closed as intended | **pin** the stated rule (fourth bullet) with the widget test |
| #166822† | A gap of a few pixels between two decorated slivers while scrolling | **pin**: R3's readback row runs over two decorated slivers too |

### R5. A lazy list takes each item's extent from a delegate

`RenderSliverVariedExtentList`, widget `SliverVariedExtentList` and a `ListView` constructor.

- The extent of item `i` is the delegate's answer for `i` and the list's dimensions (viewport
  main-axis extent, cross-axis extent). The answer does not depend on the scroll offset. An item
  is laid out at exactly that extent, whatever size it would take on its own: the child is
  given the extent as a tight constraint, the stored value is the answer, never a difference
  of two offsets, and never the child's measured size.
- The delegate is asked for an index only below the item count, and at most once per item
  between two invalidations. An invalidation is a change of delegate, or a change of
  dimensions when the delegate says its answers depend on them.
- What the scroll extent is depends on what is known about the count:
  - a known count (`ItemCount::Exact`) up to the eager limit: every item not yet answered is
    asked in one layout, and the scroll extent is exact from that layout on;
  - a known count above the limit, or an unknown one (`ItemCount::Unknown`, an endless source
    included): an item is asked when the window reaches it, the rest are estimated at the mean
    of the answers so far, and the scroll extent is provisional until every item is asked. A
    correction above the viewport keeps the visible items where they are.
- One layout asks the delegate for at most the eager limit or its window, whichever applies,
  after an invalidation too. The eager limit is a budget of this list, held by a perf
  scenario; it is not the bound on an unbounded window.
- After an eager layout, or a layout of the window, a layout costs O(log n) in the item count
  plus the items in the window, not O(n). Restoring a scroll offset costs the same.
- A non-finite or negative answer is a layout error naming the index, not a panic and not a
  published geometry.
- The number of children built in one layout is bounded even when every extent is zero.
- Extents that are known only by laying an item out are not this list's job: `SliverList`
  measures them, over the same offset index.
- The list keeps the shown record from its first version (R2), and every operation that
  writes its offset index is stated in the design, with the rule for each.

Acceptance: the shared row of R2 and
`harness_varied_extent_list_publishes_its_pinned_child_without_painting_it`,
`harness_varied_extent_list_keeps_the_visible_items_in_place_when_items_above_are_first_asked`,
`harness_varied_extent_list_places_items_at_the_sum_of_their_extents`,
`harness_varied_extent_list_gives_a_child_its_extent_whatever_the_child_wants` (a child that
would be 30 high under an answer of 100: the child is 100 and the next item starts 100 later),
`harness_varied_extent_list_asks_the_delegate_once_per_item_between_invalidations` (a counting
delegate over 100,000 items, scrolled),
`harness_varied_extent_list_asks_at_most_its_budget_in_one_layout` (the first layout and a
resize, below and above the eager limit),
`harness_varied_extent_list_with_fixed_answers_asks_nothing_again_on_a_resize`,
`harness_the_measured_list_lays_out_as_before_over_the_shared_walk`,
`harness_varied_extent_list_never_asks_for_an_index_at_or_past_its_count` (the delegate panics
for such an index; the cache extent reaches past the last item),
`harness_varied_extent_list_lays_an_item_out_at_exactly_its_extent` (an answer of 100.0 after
items whose extents do not sum exactly in binary),
`harness_varied_extent_list_refuses_a_non_finite_extent`,
`harness_varied_extent_list_of_zero_extents_builds_a_bounded_window`,
`harness_varied_extent_list_with_an_endless_source_asks_only_its_window`,
`harness_varied_extent_list_restores_a_deep_offset_without_walking_from_zero`.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #138912 | The extent callback is called for indices at or past `itemCount`, because the walk `while (position < scrollOffset)` runs into the cache extent; a callback returning 0 there never ends the walk | **avoid**: the walk is bounded by the count the index holds. **pin**: the never-asks row and the zero-extents row |
| #144390 | A small scroll near item 45 makes about 10,005 callback calls: offsets walk from item 0, and the callback was a text layout | **avoid**: an answer is stored. **pin**: the counting-delegate row and the budget row |
| PR #192384 | The fix memoises prefix sums; review found 100.0 turning into 99.9999999999999 when an extent is recovered by subtracting offsets | **avoid**: the index stores extents and the list reads the stored one back; offsets are sums of them. **pin**: the exact-extent row |
| PR #131393 | Reviewers asked the documentation to say the builder is called several times in a layout | **avoid**: once per item between invalidations is the contract |
| #52207 | With extents known only by layout, a jump or a scrollbar drag lays out everything between, and the thumb jumps because the total is an estimate | Out of this list's scope by the last bullet; stated so a caller chooses the right list |
| #171289 | Restoring a deep scroll offset lays out every item from 0 and freezes for seconds; cause unconfirmed | **pin**: the restore row |
| PR #178021 | Reviewers: "summing extents does not work for infinite lists" | **avoid**: the unknown-count case is stated above. **pin**: the endless-source row |

### R6. A lazy list shows a tree of rows

`RenderTreeSliver`, widget `TreeSliver`.

- Rows are the visible nodes in depth-first order. A row is laid out in the cross-axis extent
  left after its indent (its depth times the indentation) and placed after the indent, so its
  position, its hit area and its published rectangle include the indent, and it does not
  overflow the trailing edge.
- Expanding a node adds its descendants' rows; collapsing removes them. While the change
  animates, the rows of the changing node slide as one block from under their parent row and are
  clipped to the region the block has uncovered; the rows after the block follow it. One value
  per animating block decides its layout extent, its rows' positions and its clip.
- Paint, hit testing and semantics read the same positions and clips (R2): a row hidden by the
  clip takes no tap.
- A node's children are asked for when it is expanded and not before. Rows are built lazily;
  the list of visible nodes is held whole.
- A toggle with no duration changes the rows at once. A second toggle of a node while its first
  runs turns the block back from where it is.
- Offsets cost what R5 costs. A toggle at row `i` is an edit in the middle of the offset
  index: it costs O(log n), the delegate is asked only for the rows it added, and every other
  row keeps its extent and its state. Several toggles in one build apply in the order they
  were made, and the row at the top of the viewport stays where it is when rows are added or
  removed above it: the edit returns the correction and the sliver reports it.
- A layout asks one revision of the rows, the one whose edits the offset index has applied.
  An edit made after that revision was handed over is not seen until the next one is. A
  replaced tree, controller or extent rule is a new roster: the index is reset, and the old
  roster's revision numbers mean nothing to the new one.
- The row count has one source, that revision. Nothing else lengthens or shortens the index:
  not the element's item count, and not a row that failed to build.
- A row publishes its level, its position in its sibling set and its expanded state.
- All four axis directions work, and a right-to-left context indents from the right.

Acceptance: `harness_tree_sliver_indents_a_row_by_its_depth` (layout offset, hit area and the
rectangle `transform_to` reports), `harness_tree_sliver_indents_from_the_right_in_rtl`,
`harness_tree_sliver_paints_a_row_at_its_offset_minus_the_scroll_offset` (scrolled by more than
a viewport, and after a preceding sliver),
`harness_tree_sliver_reveal_clip_starts_at_the_parent_rows_trailing_edge` (the parent is row 0,
and row 1), `harness_tree_sliver_reveal_clip_holds_under_a_scroll_offset`,
`harness_tree_sliver_a_row_under_the_reveal_clip_takes_no_hit`,
`harness_tree_sliver_a_nested_reveal_is_clipped_by_both_blocks`,
`harness_tree_sliver_a_toggle_asks_only_for_the_rows_it_adds` (a counting delegate, a toggle
at row 10 of 10,000),
`harness_tree_sliver_two_toggles_in_one_update_apply_in_order`,
`harness_tree_sliver_an_outrun_edit_log_resets_the_index`,
`harness_tree_sliver_a_toggle_above_the_viewport_keeps_the_top_row_in_place` (every visible
row already asked, so only the splice can correct),
`harness_tree_sliver_answers_from_the_revision_it_was_given` (the roster is edited after the
hand-over and before layout),
`harness_tree_sliver_a_replaced_roster_with_the_same_edit_number_resets_the_index`,
`harness_tree_sliver_a_row_that_fails_to_build_does_not_shorten_the_index`,
`harness_tree_sliver_ignores_an_item_count_that_differs_from_its_revision`; in
`flui-rendering`'s tests the splice's own (`a_splice_keeps_the_extents_on_both_sides`,
`a_splice_moves_the_anchor_with_its_item`, `a_splice_above_the_anchor_returns_its_correction`,
`a_splice_of_a_removed_anchor_lands_on_the_edit`);
widget tests `a_row_keeps_its_state_when_rows_are_inserted_above_it`,
`a_changed_extent_rule_starts_a_new_roster`,
`a_toggle_during_a_layout_builder_pass_keeps_rows_and_extents_in_step`,
`a_tree_row_publishes_its_level_and_expanded_state`, `collapsing_a_node_removes_its_rows`,
`a_second_toggle_during_a_reveal_turns_it_back`,
`a_parent_collapsing_over_an_animating_child_drops_the_childs_animation`,
`collapsing_every_node_after_expanding_every_node_settles`,
`a_toggle_without_a_duration_changes_rows_at_once`,
`a_multilevel_collapse_slides_by_its_whole_descendant_extent`,
`collapsing_the_last_parent_at_the_end_of_the_scroll_keeps_rows_on_screen`,
`a_tree_asks_for_a_nodes_children_only_when_it_expands`.

Flutter's record (`rendering/sliver_tree.dart` supports only `AxisDirection.down` and lays a row
out at the full cross-axis extent, shifted by its indent):

| Issue | Scenario | Answer |
|-------|----------|--------|
| PR #188626, #188305† | `parentIndex == 0` meant both "no parent" and "the parent is row 0", so the first node's reveal clip started at its leading edge and its children painted over it | **avoid**: a reveal is addressed by the range of rows it covers, never by a parent index, and an absent row is `Option`. **pin**: the trailing-edge row |
| PR #194172 (open) | A parent finishes collapsing while a child still animates: a hidden child keeps a live ticker, a null check fails in paint, replacing the tree mid-animation throws. Taps reach an invisible child because hit testing does not apply paint's clip; the clip omits the scroll offset; `expandAll()` shifts rows for one frame | **avoid**: one owner holds expansion and its animations, and drops a block's animation when an enclosing block ends or the tree is replaced; hit testing reads paint's record. **pin**: the parent-over-child test, the no-hit row, the scroll-offset row |
| PR #170929†, #170757† | A double click on a node fails a null check | **pin**: the second-toggle test |
| #178962 | `collapseAll()` after `expandAll()` fails an assertion on a missing animation offset during paint | **pin**: the collapse-every-node test |
| #153149, #160017† | Rows disappear and reappear while scrolling a tree taller than the screen, and paint at the top instead of below a preceding sliver; cause unconfirmed | **pin**: the paint-offset row |
| PR #167928, PR #166442†, #166326† | A child row's global offset omits the indent, and hit testing is wrong; a tree paints behind a preceding app bar | **avoid**: the indent is the row's committed paint offset. **pin**: the indent row |
| #153889†, PR #153438†, #153418† | A collapse with a zero duration removes the node and freezes | **pin**: the no-duration test |
| PR #167882 | The animation counts only direct children, so a multilevel collapse jumps | **avoid**: a block is every visible descendant row. **pin**: the multilevel test |
| #149182†, #164981† | Collapsing the last parent while scrolled to the end blanks the view | **pin**: the end-of-scroll test |
| #187084† | After a cross-axis scroll, the part of a wide row that was outside the viewport takes no gesture | Not reachable: a row is never wider than the cross-axis extent (first bullet) |
| #148358† (open) | Children loaded on expand are not supported; the whole tree is expected up front | **avoid**: the fourth bullet. **pin**: the children-on-expand test |

### R7. A header sliver is sized by its child

Widgets `PinnedHeaderSliver`, `SliverResizingHeader`, `SliverFloatingHeader` and their render
objects.

- A pinned header stays at the viewport's leading edge once reached; its extent is its child's.
- A resizing header shrinks from its maximum to its minimum extent as it scrolls. The minimum
  comes from a prototype child that is laid out and never painted, hit or published. The maximum
  comes from a second prototype, or without one from the child's own maximum intrinsic extent.
- A floating header returns on a reverse scroll, by the amount scrolled, under a preceding
  pinned sliver too, and can snap. It hears a scroll of a scrollable nested in its body.
- Each reports the scroll obstruction it causes, so a reveal of a rectangle and R3's
  `FollowEdge` stop at its trailing edge. The resizing header's obstruction is its minimum.
- Each publishes, for its child to read in build, its minimum and maximum extent and whether
  content has scrolled under it; the current extent is the child's own constraint. The
  answer comes from the header's own layout, not from a flag the next sliver sets. A child
  that reads it is rebuilt in the frame after one in which a value changed, including a
  change that lays out neither the child nor the header (content scrolling under a pinned
  header). A scroll that changes none of the values rebuilds nothing, and no callback runs
  per frame to find out.
- A resizing header whose children are not the child and exactly the prototypes its
  configuration names is a layout error that states both numbers.
- Inside a main-axis group a pinned header is pushed off by the end of its group, and two pinned
  headers in one group stack.
- A pinned header takes its place in the reading order where it is painted: assistive focus
  reaches the content it overlaps before it skips ahead.

Acceptance: `harness_pinned_header_takes_its_extent_from_its_child`,
`harness_pinned_header_taller_than_the_viewport_stays_finite`,
`harness_resizing_header_never_paints_hits_or_publishes_its_prototypes`,
`harness_resizing_header_reports_its_minimum_as_obstruction`,
`harness_resizing_header_without_a_maximum_prototype_takes_its_childs_intrinsic_extent`,
`harness_child_sized_header_publishes_its_extents_and_overlap`,
`harness_resizing_header_refuses_children_that_differ_from_its_prototype_flags`,
`harness_floating_header_returns_by_the_distance_scrolled_back`,
`harness_floating_header_returns_under_a_preceding_pinned_sliver` (both scroll directions, all
four axis directions), `harness_floating_header_snaps_open_when_released_past_half`,
`harness_a_pinned_header_in_a_group_leaves_with_its_group`,
`harness_two_pinned_headers_in_one_group_stack`,
`harness_a_pinned_header_in_a_cross_axis_group_sits_below_a_preceding_pinned_sliver`; widget
tests `a_pinned_header_keeps_a_following_clip_at_its_edge`,
`a_pinned_header_is_read_where_it_is_painted`,
`a_header_child_rebuilds_when_content_scrolls_under_it` (the child is not laid out by the
scroll), `a_scroll_that_changes_no_header_value_rebuilds_nothing`,
`a_rebuilt_header_delivers_to_its_current_child`,
`a_floating_header_returns_on_a_scroll_of_its_nested_body`.

Flutter edge cases: a header taller than the viewport; a child whose size changes while pinned.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #162661, PR #162955† | `SliverResizingHeader` reports its child's extent, not its minimum, as the scroll obstruction | **pin**: the obstruction row |
| #153014 (open) | Prototypes are children, so they enter the semantics tree; the child cannot learn the current extents | **pin**: the prototypes row. **avoid**: the fifth bullet |
| #179687, PR #179690, #179022, PR #179023 | With a screen reader a pinned header takes focus before the list reaches its top, and items are skipped | **pin**: the reading-order test |
| #129869 (open) | A floating persistent header stays hidden until the list is at the top: the overlap is ignored | **pin**: the preceding-pinned-sliver row |
| PR #151145 | In review a `min` stood where a `max` was needed in the overlap arithmetic; the list jumps on snap when paint and layout extents differ | **pin**: the same row in every direction, and the snap row |
| #26667 (open) | `overlapsContent` is false for the first header and for the child-sized headers | **avoid**: the fifth bullet |
| #178808, PR #178809 | A pinned child of a group unpins when the group scrolls past; a `keepPinned` flag was refused | **pin** the stated rule: a header belongs to its group |
| #155758†, PR #163528 | Several pinned headers in one group misbehave | **pin**: the stacking row |
| #134535†, #147704† | Pinned headers inside groups do not respect a preceding pinned sliver | **pin**: the cross-axis group row |
| #17518 | A floating app bar in a nested scroll view does not return on an inner scroll | **pin**: the nested-body test |

### R8. A sliver can be built from its constraints, and a page view can centre its end pages

Widgets `SliverLayoutBuilder` and `SliverFractionalPadding`; `PageView::pad_ends`.

- The builder runs during layout with the sliver's layout information, and its result is laid
  out with the same constraints; the builder's sliver geometry is the result's.
- It rebuilds when the information it was built for changes or the widget asks, and not
  otherwise. Two forms state what "the information" is: the full constraints, which change on
  every scroll step, and the dimensions (cross-axis extent, viewport main-axis extent, axis and
  growth directions), which change only on a resize.
- A rebuild requested while a layout pass runs (by anything that pass calls) is delivered before
  the frame paints or in the next frame, and a later request is honoured again. The rule is the
  box builder's and is held in one place for both.
- `SliverFractionalPadding` pads a sliver by a fraction of the viewport's main-axis extent,
  computed in layout with no rebuild. A page view whose `viewport_fraction` is below 1 centres
  its first and last page with it when `pad_ends` is set (the default), and leaves no blank
  space when it is not.
- Every page can become the current page under either value of `pad_ends`: a target past the
  maximum scroll extent clamps to it, and at the maximum extent the current page is the last.

Acceptance: widget tests `a_sliver_layout_builder_rebuilds_only_when_its_dimensions_change`
(scrolled; the builder runs once), `a_sliver_layout_builder_of_constraints_rebuilds_on_a_scroll`,
`a_sliver_layout_builder_adopts_its_childs_geometry`,
`a_rebuild_requested_during_layout_is_delivered_and_the_next_one_too` (one table over
`LayoutBuilder` and `SliverLayoutBuilder`), `a_padded_page_view_centres_its_first_and_last_page`,
`an_unpadded_page_view_leaves_no_blank_space`, `an_unpadded_page_view_reaches_its_last_page`,
`a_page_overlapping_the_viewport_edge_is_hit_where_it_is_painted`; harness rows
`harness_sliver_fractional_padding_pads_by_the_viewport_fraction`,
`harness_sliver_constraints_report_the_logical_scroll_direction` (all four axis directions).

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| PR #193331 (unmerged) | A rebuild scheduled while the builder's own layout runs sets the rebuild flag, and the layout then clears the layout flag: the rebuild is lost for good, silently | **avoid**: build never runs inside a layout pass (ADR-0017), and a dirty element under a builder is serviced with it between passes. **pin**: the delivered-rebuild test |
| #168064 (open) | A sliver layout builder rebuilds on every scroll tick, because the constraints carry the scroll offset | **avoid**: the dimensions form. **pin**: the first two tests |
| #79250 | `userScrollDirection` reports the physical direction while its documentation describes the logical one | **pin**: the scroll-direction row |
| PR #37024†, PR #44778† | `PageView` was rebuilt on `SliverLayoutBuilder` and reverted for a 4.5 % build regression | **avoid**: `pad_ends` is a padding sliver, not a builder |
| #48252, PR #48207 | Padding both ends shows blank space when the fill-viewport sliver shares a scroll view | **pin**: the no-blank-space test |
| #115755 (open) | With `padEnds: false` and a fraction below 1 the last pages never become the current page | **pin**: the last-page test |
| #95710 | A fraction computed from a zero width (minus infinity) shows a white screen in release | Already avoided: `ViewportFraction::try_new` refuses a fraction that is not finite and positive |
| PR #34608† | Pages that overlap the viewport edge were hit in the wrong place | **pin**: the hit test |

### R9. Three widgets are reached by composition or one new layout

- `BlockSemantics` hides the semantics of everything painted before it in its parent, through
  the sibling barrier of ADR-0226. Removing it restores the earlier siblings without announcing
  them again.
- `SliverVisibility` shows, hides or replaces a sliver, with the same maintain switches as
  `Visibility`.
- `OverflowBar` lays its children in a row when they fit the available width and in a column
  when they do not. Its children carry no flex parent data: a `Spacer` or an `Expanded` under it
  is refused in every build profile.

Acceptance: `block_semantics_hides_what_was_painted_before_it`,
`removing_block_semantics_restores_earlier_siblings_without_an_announcement`,
`a_hidden_sliver_takes_no_extent_and_no_hits`,
`a_hidden_sliver_that_maintains_size_keeps_its_extent`,
`a_hidden_sliver_that_maintains_semantics_is_still_published`,
`harness_overflow_bar_wraps_to_a_column_when_its_row_does_not_fit`,
`harness_overflow_bar_intrinsics_match_its_layout`,
`harness_overflow_bar_orders_its_column_by_direction`,
`harness_overflow_bar_mirrors_its_row_in_rtl`,
`an_expanded_child_of_an_overflow_bar_is_refused`. The three documents that say FLUI has no
`BlockSemantics` are corrected in the same change.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #188272, PR #191751 (open) | A `Spacer` in `AlertDialog.actions`: the parent data is the overflow bar's, a cast fails in release and debug shows nothing | **pin**: the refused-child test, run in the release profile too |
| #166258†, #134297† | A live region announces again when a dialog closes; an expanded menu leaves other nodes focusable | **pin**: the restore test |
| #44290†, #45182† | `SliverVisibility` was built from sliver opacity, ignore-pointer and offstage | The same composition |

### R10. An image is tinted, faded, inverted and sampled as asked

`RenderImage` gains `color` with `color_blend_mode`, `opacity`, `invert_colors` and
`filter_quality`; `Image` gains the matching builder methods.

Acceptance: readback rows whose samples distinguish each property from its absence
(`an_image_is_tinted_by_its_colour_and_blend_mode`, `an_image_is_faded_by_its_opacity`,
`an_image_of_zero_opacity_paints_nothing`,
`a_tinted_image_keeps_its_transparent_pixels_transparent` (the default blend mode),
`a_fully_transparent_tint_with_src_in_hides_the_image`,
`an_inverted_tinted_image_inverts_after_the_tint`,
`an_upscaled_image_is_sampled_by_its_filter_quality`), and
`harness_image_intrinsics_are_finite_before_the_image_resolves`.

Edge cases: opacity 0 paints nothing; inversion applies after the tint, as in Flutter's source;
`FilterQuality::None` gives nearest sampling. The first two properties are ungated; the last
two change `flui-painting` and `flui-engine` (G1).

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #12990 (open) | A tint with most blend modes fills the transparent pixels of the image | **pin**: the transparent-pixels row; the builder method's documentation names the blend modes that fill |
| #143670 | A fully transparent colour with `srcIn` makes the image invisible; correct blending, surprising | **pin** and document: no colour shows the original |
| #32116 | Intrinsics return infinity before the image loads under unbounded constraints | **pin**: the intrinsics row |
| #27827 (open) | A centre slice with a destination smaller than the source asserts: the slice is in image pixels, the output in logical pixels, and the scale is not applied | Outside R10; `Image::center_slice` exists, and the first step of the image task runs this case and records the result |

No recorded defect was found for `invertColors` or `filterQuality` alone.

### R11. A widget filters the colours or the pixels of its subtree

Widgets `ColorFiltered` and `ImageFiltered`. Closes the gap "No widget filters its subtree's
colours or pixels" ([gaps.md](../../../gaps.md)).

- A filter built from the box (a matrix about its centre) is a function of the box's size,
  evaluated again when the size changes, never of its offset.
- A filtered subtree at a non-zero offset, and at a fractional one, paints where it is laid out,
  and a partial repaint of it leaves no edge artefact.
- A disabled `ImageFiltered` pushes no layer. Hit testing is the child's.

Acceptance: a readback row per filter kind, `a_filtered_subtree_is_hit_where_it_is_laid_out`,
`a_filtered_subtree_at_an_offset_paints_at_that_offset`,
`a_blurred_subtree_at_a_fractional_scroll_offset_repaints_without_an_edge`,
`a_disabled_image_filter_pushes_no_layer`,
`a_filter_change_does_not_repaint_the_child`, and in `flui-rendering`'s tests
`a_node_with_a_clip_and_a_filter_cuts_the_filters_output` (the order the ADR fixes, on a test
object that returns both).

Flutter's record (no recorded defect was found for hit testing under a filter, for `enabled` or
for the interplay with opacity; the searches returned backend noise, so those rows pin FLUI's
own contract):

| Issue | Scenario | Answer |
|-------|----------|--------|
| #105674 | A blur over a scrolling list shows edge artefacts: layer bounds were computed with an integral transform while layers painted at fractional offsets | **pin**: the fractional-offset row, over the retained scene's damage |
| PR #73613 (unmerged) | Whether a filter relative to the child's bounds is a function of the size or of the paint offset | **avoid**: the first bullet |
| PR #116229 | Golden files of a filtered subtree missed offsets | **pin**: the offset row |
| PR #188542 | A native view cannot sit under a filter layer | **avoid**: R17's ADR states what a filter over a native view does |

### R12. A region learns of a tap inside or outside it

Widget `TapRegion`, render object `RenderTapRegion`.

The tap ADR decides the mechanism; these are what it must deliver
([design-parity.md](design-parity.md), W12).

- A region hears a typed event that says the phase, the buttons, and the origin (a pointer of
  a kind, or an accessibility activation). The phases are `Down`, then exactly one of `Up` and
  `Cancel`, and `Tap` after an `Up` whose contact never travelled past the hit slop of its
  pointer kind. Every way a contact ends delivers that one terminal event: an up, a platform
  cancel, a second down on the same pointer, eviction at the pointer cap, a lost capture, a
  removed device, a closing presentation.
- A pointer event is *inside* a region when the region, or a region of its group, is on the hit
  path of the contact's down as the hit test returned it, before the binding prunes that path
  for any reason; otherwise it is *outside*. Geometry is not consulted. The up and the tap
  of a contact are classified against the path of its down.
- A contact reports to the groups that were live at its down, and to no other: a region that
  mounts while the contact is down hears no `Up` and no `Tap` of it.
- A region is on the path wherever a pointer lands inside its box, whether or not its child
  takes the hit.
- A popup carries the group of the widget that opened it, so a tap inside the popup is inside
  the opener. A field's own overlays (its selection handles, its selection toolbar) are in
  the field's group, so a press on one is inside the field.
- A region can consume outside taps: the tap dismisses and no tap or long-press detector under
  it acts. A drag or a scroll that starts outside still runs, and a raw `Listener` under the
  contact is not cancelled by the region.
- An accessibility activation is classified by render ancestry between its target and the
  region, in either direction, once per activation, at the one door production code has.
  Every action has a class, and adding an action without one does not pass the checks. An
  action that acts on its target is classified and stopped by a consuming region outside it;
  one that only scrolls, reveals or shows a tooltip is not. A stopped activation is reported
  as stopped to whoever asked, never as run and never as a missing node. A keyboard
  activation is not classified.
- A group is live when one of its members is presented and, where it has a page, on the
  current one, decided once at the down. Every member of a live group hears, a member on a
  page its own popup covers included. No navigator observer is involved and nothing is read
  per frame.
- A region uses the callbacks and the consume flag of its latest build.
- An action the host performs without a contact (a scroll-to-top from the status bar) is never
  reported as a pointer at an invented position.

Acceptance: `a_tap_on_an_overlay_above_a_region_is_outside_it`,
`a_tap_inside_a_group_member_is_inside_every_member`,
`a_tap_inside_a_popup_is_inside_its_opener`,
`a_tap_on_an_empty_part_of_a_region_is_inside_it`,
`a_tap_on_a_second_field_is_outside_the_first`,
`a_drag_that_starts_outside_reports_down_and_up_but_no_tap`,
`a_secondary_button_outside_is_reported_with_its_button`,
`a_consumed_outside_tap_reaches_no_detector`,
`a_drag_that_starts_outside_a_consuming_region_still_scrolls`,
`a_consumed_outside_tap_cancels_no_listener`,
`a_contact_that_ends_without_an_up_reports_cancel_once` (one table: a platform cancel, a
second down on the pointer, eviction at the pointer cap, a lost capture, a removed device, a
closing presentation),
`a_first_click_inside_a_region_is_inside_it` (a first-click target under a region in an
inactive window),
`pressing_a_selection_handle_is_inside_the_fields_region`,
`tapping_the_selection_toolbar_is_inside_the_fields_region`,
`a_rebuilt_region_uses_its_new_callback_and_consume_flag`,
`a_contact_that_leaves_its_slop_and_returns_is_no_tap` (touch and mouse, whose slops differ),
`a_region_mounted_during_a_contact_hears_nothing_of_it`,
`a_screen_reader_activation_is_classified_by_ancestry`,
`an_activation_of_a_merged_node_above_a_region_is_inside_it`,
`a_screen_reader_activation_is_classified_once`,
`a_consumed_outside_activation_does_not_run_its_action` (through the runtime's dispatch, the
door the hosts and the agent use),
`a_consumed_outside_activation_is_reported_as_consumed` (the dispatch's result and the
agent's outcome),
`a_panicking_region_callback_still_decides_the_activation`,
`every_semantics_action_has_a_tap_class` (or the compile-time form the ADR chooses),
`a_scroll_action_outside_a_consuming_region_still_runs`,
`a_field_under_its_own_popup_still_hears_a_tap_outside_both`,
`a_region_on_a_covered_page_hears_no_outside_tap`,
`a_region_on_a_branch_that_is_not_shown_hears_no_outside_tap`,
`a_region_under_an_offstage_parent_hears_no_outside_tap`,
`an_up_is_classified_against_the_path_of_its_down`,
`a_panicking_outside_callback_leaves_the_registry_usable`; the row "consuming region, outside
contact" that the tap ADR states, in `assert_ownership_table`.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #138190, #140271 | `onTapOutside` fires on pointer down only, so a scroll that starts outside a text field unfocuses it and closes the keyboard; the callback carries no kind | **avoid**: the event's phase. **pin**: the drag test |
| #177992, #175606 | A tap on the status bar makes the engine inject a touch at (0, 0), which is outside every region, so drawers and menus close | **avoid**: the last bullet |
| #136706, #139555† | A region sees only the primary button; a desktop menu does not close on a right click outside | **pin**: the secondary-button test |
| PR #155297, #153093 | Outside taps fired for regions on other routes; the fix uses a navigator observer and misses shell routes and branch switches, leaving stale regions | **avoid**: liveness is "presented and on the current page", asked per group at the down. **pin**: the covered-page, branch, offstage and field-under-its-popup tests |
| #135327 | The click that closes a menu also fires the handler beneath it | **pin**: the consumed-tap test. FLUI departs where Flutter's claim also stops a scroll that starts outside: the drag test and the listener test pin the departure |
| PR #183093, #167487 | With semantics on, a web click became a semantics action and bypassed regions; the fix hit-tests the centre of the node's rectangle, the node has no link to its region, a consuming region cannot stop the action, and a reviewer asked whether a tap can be handled twice | **avoid**: classification by ancestry at the one door, with a typed outcome when a region stops the action. **pin**: the activation tests |
| #128317 (open) | A region around a dropdown cannot cover its menu, which is an overlay route outside the subtree | **avoid**: the popup's group. **pin**: the popup test |
| #112924 | A region around a child that claims no hit hears no inside tap on its empty areas | **pin**: the empty-part test |
| #127597, PR #148206 | The first field's outside callback does not fire on a tap in a second field; cause unconfirmed | **pin**: the second-field test |
| #144221† (open) | A tap that stops a fling fires the handler under it | The tap ADR decides whether a down that stops a fling counts as a `Tap`; recommended: it does not ([design-parity.md](design-parity.md), W12) |

### R13. A paragraph lays out the boxes its spans embed

`RenderParagraph` with inline children; `Text::rich` and `RichText` accept a widget inside a
span at any depth.

- An inline child is laid out under the paragraph's maximum width, takes its place in the line
  by its `PlaceholderAlignment`, and moves with the text when the line breaks, when a line is
  justified, centred or end-aligned, and when the text runs right to left.
- Each child is placed or not placed on its own. The n-th measured placeholder of the span
  tree belongs to the n-th child, and the tie survives a rebuild of the span tree: a child
  keeps its state when the text around it changes. A child whose placeholder is cut by
  `max_lines` or an ellipsis is not painted, not hit and not published; every other child, a
  later one included, is unaffected. A child that publishes no semantics node leaves the
  others' nodes with their owners. A span tree whose measured placeholders and children
  differ in number is a layout error that states both numbers.
- Intrinsic sizes and dry layout include the children, over every line of a wrapped paragraph.
- The paragraph's text offsets count one position per placeholder. The spoken text reads the
  child's content where the placeholder stands, in reading order.
- An inline child is not scaled by the text scale. It reads the ambient text sizing like any
  other widget, so text inside it is scaled once.
- The text painter takes the placeholder sizes from the paragraph that owns the children; no
  caller measures them for it.

Acceptance: `harness_paragraph_places_an_inline_child_on_its_line`,
`harness_paragraph_aligns_an_inline_child_to_the_baseline`,
`harness_paragraph_drops_a_truncated_inline_child_from_paint_hits_and_semantics`,
`harness_paragraph_places_a_child_after_a_truncated_one` (a paragraph whose first placeholder
has no box and whose second has one),
`harness_paragraph_orders_inline_children_right_to_left_in_rtl`,
`harness_paragraph_places_inline_children_in_mixed_direction_text`,
`harness_paragraph_moves_an_inline_child_with_justified_text`,
`harness_paragraph_intrinsics_include_inline_children` (one line, and wrapped),
`harness_paragraph_without_inline_children_lays_out_as_before`; widget tests
`a_widget_nested_in_a_styled_span_is_laid_out_in_the_text`,
`a_selection_crosses_an_inline_widget_as_one_position`,
`text_inside_an_inline_widget_is_scaled_once`,
`an_inline_widget_is_read_where_it_stands`,
`an_inline_widget_without_semantics_leaves_the_others_nodes_in_place`,
`an_inline_widget_keeps_its_state_when_the_text_around_it_changes`; harness row
`harness_paragraph_refuses_children_that_differ_from_its_placeholders`.

Flutter's record (`rendering/paragraph.dart` stops paint and hit testing at the first child
without an offset, and queries a baseline only for `PlaceholderAlignment.baseline`):

| Issue | Scenario | Answer |
|-------|----------|--------|
| #35869 | A placeholder cut by `maxLines` gets no box; loops index past the boxes | **avoid**: the second bullet. **pin**: the truncated row and the child-after row |
| #69787 | A range error in semantics assembly: a node per `WidgetSpan` is assumed, and a clipped or excluded child has none | **avoid**: a node is attached by its child's slot, not by counting nodes. **pin**: the without-semantics test |
| #111931, PR #113461 | The label of rich text with a widget reads the object-replacement character and puts the widget's text last | **pin**: the read-where-it-stands test |
| #54400, #111381 | In right-to-left text several widget spans on a line lay out left to right and overlap; cause unconfirmed | **pin**: the two direction rows |
| #50009 | With justified text the boxes are not moved after justification | **pin**: the justify row |
| #126962 (open) | The text scale is applied to the span's child as a transform, so text inside is scaled twice and paddings and icons scale too | **avoid**: the fifth bullet. **pin**: the scaled-once test |
| #49883 (open) | `TextPainter.layout` asserts unless the caller has measured the placeholders itself | **avoid**: the last bullet |
| #27293†, #48679† | The intrinsic height of wrapped rich text counts the first line only | **pin**: the wrapped intrinsics row |

### R14. An editable field lays out inline children the same way

`RenderEditable` shares R13's seam.

- The editing value's text holds one object-replacement character where each placeholder
  stands, so the text model and the paragraph have one offset space. The field builds its
  spans from the value and asks for the child of each placeholder, so no span tree with
  another text exists to disagree with it.
- A placeholder has an identity the value holds beside its character. Every edit of the
  value, the controller's and an input session's alike, is a replacement of a range: a
  placeholder outside the range keeps its identity and its child's state, and one inside it
  is gone. A composition beside a placeholder leaves it in place.
- A placeholder enters the value only through the method that inserts one. An
  object-replacement character in pasted text, in composed text or in a programmatic
  replacement of the text is removed from the incoming text, and no write removes one that
  is already in the value.
- An obscured field masks a placeholder as one obscuring character and builds no inline
  child; offsets count the same positions as in the unobscured field.
- A placeholder is one caret stop and one deletion: the caret stands before or after it, never
  inside; backspace removes it whole; a selection across it includes it.
- The caret beside a placeholder takes the line's height, and can stand after a placeholder at
  the end of the text.
- The selection highlight is visible over an inline child that paints a background.

Acceptance: `harness_editable_steps_the_caret_over_an_inline_child`,
`harness_editable_caret_beside_an_inline_child_has_the_line_height`,
`harness_editable_caret_reaches_the_end_after_a_trailing_inline_child`,
`harness_editable_steps_over_two_leading_inline_children`,
`harness_editable_selection_shows_over_an_inline_child_with_a_background`,
`harness_editable_backspace_removes_an_inline_child_whole`,
`harness_editable_offsets_agree_beside_a_multi_unit_character` (a placeholder next to a
character outside the basic plane); widget tests
`a_field_builds_one_inline_child_per_placeholder_in_its_value`,
`pasting_an_object_replacement_character_adds_no_placeholder` (paste, `set_text` and an IME
commit, one table), `composing_beside_a_chip_keeps_the_chip` (a composition started, updated
and committed through the input session),
`deleting_the_first_of_two_chips_keeps_the_seconds_state`,
`an_edit_through_any_controller_method_keeps_text_and_placeholders_in_step` (one table over
the controller's write methods and a session write),
`an_obscured_field_masks_a_placeholder_and_builds_no_child`.

Flutter's record:

| Issue | Scenario | Answer |
|-------|----------|--------|
| #30688, PR #80185 | Widget spans in the editable: the caret cannot reach the last position, its height does not follow the span, two leading spans fail an assertion, a span's background hides the selection | **pin**: the caret rows, the two-leading row, the selection row |
| #107432 (open), #161213 | The text model holds the replaced text (six characters) while the engine counts one position for the span, so caret and backspace are wrong; a marker outside the basic plane counts as one or two | **avoid**: the first two bullets: one text with its placeholders' identities, from which the spans are built. **pin**: the multi-unit row, the one-child-per-placeholder test and the composition test |

### R15. A wheel of items scrolls around a cylinder

Widget `ListWheelScrollView`, render object `RenderSliverWheel`.

- Items of one extent lie on a cylinder; the item at the centre is the selected one. Diameter
  ratio, perspective, off-axis fraction, squeeze, magnification of the centre band and the
  opacity of the others are parameters. The wheel scrolls along either axis.
- Items are built lazily. A looping wheel repeats its items without end in both directions.
- An item is hit where it is drawn: the transform that paints it is the transform that
  hit-tests it and that maps its semantics rectangle, and it exists after layout, before any
  paint (R2). The magnification and the cylinder are continuous across the edge of the centre
  band.
- An item that crosses the band's edge is drawn in both parts and is one hit target, one
  semantics node and one set of input regions.
- The selected item is defined from the first layout on: a child that reads it during its first
  build gets the initial item.
- A change of the item count or of the delegate lays the wheel out again.
- A wheel under an unbounded cross axis, or one whose transform is not finite, is a layout error
  and publishes nothing.
- A released drag or fling settles on an item. One notch of a mouse wheel moves one item,
  whatever distance the host gives a line, and counts from where the previous notch was
  heading: two quick notches move two items. A notch is what the host says is one: a packet
  in detents, whose fractions add up to a whole one before the wheel moves, or a packet the
  host marks as notched. Any other packet, a touchpad's and every browser packet today,
  scrolls by its pixels and the wheel settles on an item when the scroll ends.
- The scrollable keeps one answer for where a wheel is heading. A drag, a programmatic
  scroll or a replaced scrollable ends it, and the next notch counts from where the list is.

Acceptance: `harness_wheel_hits_an_item_through_its_cylinder_transform_before_the_first_paint`,
`harness_wheel_hits_the_magnified_item_through_the_magnifier_transform`,
`harness_wheel_magnification_is_continuous_across_the_band_edge`,
`harness_wheel_skips_items_beyond_a_quarter_turn`,
`harness_wheel_semantics_rect_follows_its_transform`,
`harness_wheel_item_across_the_band_edge_reports_its_regions_once`,
`harness_wheel_under_unbounded_constraints_is_a_layout_error`,
`harness_wheel_relays_out_when_its_item_count_changes`,
`harness_wheel_lays_out_along_the_horizontal_axis`; widget tests
`a_looping_wheel_scrolls_past_its_last_item_to_its_first`, `a_wheel_settles_on_an_item`,
`a_wheel_reports_its_selected_item_once_per_change`,
`a_wheel_child_reads_the_selected_item_in_its_first_build` (a negative initial item and a
looping wheel included), `one_notch_of_a_mouse_wheel_moves_one_item` (under two line heights),
`two_quick_notches_move_two_items` (the second arrives while the first still animates),
`a_touchpad_scroll_of_a_wheel_settles_on_an_item`,
`a_wheel_packet_steps_or_scrolls_by_its_unit_and_precision` (one table over the packets the
hosts send: whole and fractional detents, notched lines, notched pixels, unmarked lines,
precise pixels), `a_high_resolution_wheel_steps_once_per_whole_detent`,
`a_notch_after_a_programmatic_scroll_steps_from_where_the_list_is`,
`a_shifted_notch_steps_a_horizontal_wheel`,
`a_scrollable_whose_policy_has_no_step_scrolls_a_notch_as_before`, the four tests the record
"Wheel precision selects local animation policy" names, unchanged,
`a_tap_on_a_wheel_item_reaches_its_detector`.

Flutter's record (`rendering/list_wheel_viewport.dart` hit-tests with a transform saved into
parent data during paint, written only by the cylindrical paint path):

| Issue | Scenario | Answer |
|-------|----------|--------|
| #38803 | Children of a wheel take no tap; nobody confirms a cause in 67 comments | **pin**: the tap test and the two hit rows |
| #126491 | Dragging a picker asserts "child paint transform happened to be null": hit testing reads what paint writes | **avoid**: the transform is a layout result. **pin**: the before-first-paint row |
| PR #194859, #125997, #60578 | Children are built before the content dimensions are applied, so a child that asks for the item index in its first build hits a null check | **pin**: the first-build test |
| #31650 | A wheel under unbounded constraints produces a non-finite transform that crashes the engine | **pin**: the layout-error row |
| #58144, #22999† | Changing the child count does not update the wheel: its element does not mark the render object for layout | **pin**: the count-change row |
| #131544 (open) | Items at the band's edge jump in size: the magnification is a step | **pin**: the continuity row |
| #125291† (open) | One click of a mouse wheel scrolls past several items | **avoid**: a notch is a step in items at the scrollable, never the host's pixels. **pin**: the two notch tests and the packet table |
| #24089† | Tapping an item should bring it to the centre | Catalog work on top of the tap test |
| #25074† (open) | There is no horizontal wheel | **avoid**: the first bullet. **pin**: the horizontal row |

### R16. A viewport scrolls lazily built children along two axes

Render object `RenderTwoAxisViewport`, widget `TwoAxisScrollView`.

- Children are identified by a vicinity (column, row) and built on demand for the visible and
  cached region; a layout policy places them. Building and caching are symmetric in the four
  scroll directions.
- One owner maps a vicinity to its child, kept-alive and pinned children included. Moving a
  keyed child onto an occupied vicinity swaps the two and loses neither. A kept-alive child that
  leaves the region is removed once nothing holds it, and no query reads geometry of a child
  that has none.
- Children are hit in reverse paint order, so a pinned cell is hit before the cell it covers, on
  the leading and the trailing side, with either axis reversed.
- The viewport clips only when a child extends past it, and never with `Clip::None`.
- Pinned vicinities stay built outside the region (ADR-0265).
- A drag scrolls every axis that can scroll: with one axis that cannot, a diagonal drag still
  scrolls the other. The view says whether a drag off the axes moves both or is locked to
  the axis that led.
- The two axes begin a contact together or not at all. A listener of one axis that panics
  while the contact begins leaves both axes in the contact, and the panic is raised after
  both began; an axis that cannot begin leaves neither in it.
- A resize or a jump that invalidates the built region during a frame settles in that frame,
  within the bound of ADR-0308.

Acceptance: `harness_two_axis_viewport_builds_only_the_visible_region`,
`harness_two_axis_viewport_builds_the_same_region_scrolling_either_way`,
`harness_two_axis_viewport_hits_the_child_painted_last`,
`harness_two_axis_viewport_hits_a_pinned_cell_before_the_cell_it_covers` (leading and trailing,
row and column, each axis forward and reversed),
`harness_two_axis_viewport_pushes_no_clip_when_nothing_overflows`,
`harness_two_axis_viewport_never_clips_with_clip_none`,
`harness_two_axis_viewport_keeps_a_pinned_vicinity`,
`harness_two_axis_viewport_swaps_children_moved_onto_each_other`,
`harness_two_axis_viewport_removes_a_released_keep_alive_child`,
`harness_two_axis_viewport_cost_does_not_grow_with_its_extent` (a 20,000 by 200 grid with a
pinned row and column, scrolled to the end; children built and laid out are counted); widget
tests `a_diagonal_drag_scrolls_both_axes`,
`a_diagonal_drag_with_one_unscrollable_axis_scrolls_the_other`,
`a_locked_diagonal_drag_scrolls_the_axis_that_led`,
`a_panic_while_one_axis_begins_a_contact_leaves_both_axes_in_it` (the other axis was
flinging; it has stopped, and the next move scrolls both),
`an_axis_that_cannot_begin_a_contact_begins_neither`,
`a_two_axis_view_builds_no_child_outside_its_cache`,
`a_two_axis_view_resized_while_scrolling_settles_in_one_frame`.

Flutter's record (`widgets/two_dimensional_viewport.dart` walks the child map in map order for
hit testing):

| Issue | Scenario | Answer |
|-------|----------|--------|
| #149907† | The overflow flag compares an `Offset` with a `Size`, which is always unequal, so the viewport always clips | **pin**: the no-clip row |
| #141101 | Swapping the vicinity-to-key mapping: moving a child reassigns the map entry and drops the live child already there | **avoid**: one owner of the mapping. **pin**: the swap row |
| #138977, PR #149639 | Hovering a kept-alive cell while scrolling fast fails a null check: kept-alive children were not removed when they should be | **pin**: the keep-alive row |
| #144982, #135384† | With free diagonal drag and one axis that fits, no drag works: a recogniser is attached per scrollable axis | **avoid**: one two-axis drag that hands each axis its share. **pin**: the unscrollable-axis test |
| #186876 | A trailing pinned column loses hit tests to the cells under it | **pin**: the pinned-cell row |
| #135386 | Pinned rows and columns with one axis reversed show only the corner or leave a gap | **pin**: the same row's reversed cases |
| #161421, #138271 | A large table with a pinned row and column lags after scrolling to the end; no cause | **pin**: the cost row |
| #157350† | Children are cached or repainted only when scrolling down | **pin**: the either-way row |
| #134858† | A button is clipped even with `Clip.none` | **pin**: the clip-none row |
| #147292†, #140589† | Crashes on a fast mouse wheel and on a quick resize | **pin**: the resize test |

### R17. A native view sits in the UI

`RenderAndroidView`, `RenderUiKitView`, `RenderAppKitView`, `PlatformViewRenderBox` and
`RenderWebImage` in Flutter. In FLUI this is the open gap "No native views inside the UI (web
view, map, video)" ([gaps.md](../../../gaps.md)), which says it needs an owner decision.

This spec does not decide it. It requires, before any code:

- the owner's decision that native views enter the contract, and for which hosts;
- the ADR named in the design, which decides how a native view sits in the scene, composites
  with the GPU surface, takes hit tests and joins the semantics tree;
- that the frame path stays synchronous: no resize handshake that spans frames inside layout;
- that a view with no placement in a frame is hidden with that frame, and the ADR states what
  a host does with a frame's placements when its present fails;
- that a host which cannot embed a view, the headless one included, answers the request with
  a typed refusal, the same on every host;
- that the ADR answers each row below.

Flutter's record, as the list the ADR answers:

| Issue | Scenario | What the ADR must state |
|-------|----------|-------------------------|
| #139230 (open) | A native text field in a texture stretches and jumps on resize: the platform thread swaps the image while the UI thread has already scheduled a frame, and the raster thread can draw in between | How a resize reaches the native side without a frame in which size and content disagree |
| #31243 | A web view in a fill-remaining sliver is resized on every scroll tick, each a new virtual display; a drag is not handed back to the slivers | The cost of a resize, and which side owns a drag and how it hands it back |
| #20456 | A zero-size frame (the screen is off) reaches the creation of a virtual display, which throws | Geometry is never sent at zero size |
| #190833 | A view mounted during a route transition asks for its position before its first layout and throws | Geometry is never asked for before the first layout: a typed "not laid out" outcome |
| #53490 | A FLUI widget over a web view freezes: the native recogniser blocks the forwarder; closed as unfixable under the architecture | Which side wins a contact, as a rule of the arena |
| #49084† (open) | A gesture detector that is the parent of a platform view never gets a tap | The same rule |
| #50183, #143747† | A backdrop blur cannot read a native view | What a filter over a native view does: refused or defined (R11) |
| #164899† (open) | Video from a platform view appears above the UI when scrolled out of view | Order and clipping under a scroller |
| #19718† | An embedded view cannot bring up the keyboard | Who owns focus and the IME |
| PR #171916†, #163288†, #164405† | `RenderWebImage` lays out wrongly for most fits and alignments | Rows over every fit and alignment |
| PR #193408† | The image element's source is not cleared on disposal | Disposal releases the native element |

### R18. What stays covered by a different design is recorded with what it must define

`crates/flui-objects/ARCHITECTURE.md`, "Mapping decisions", gains one entry for each of:
`RenderSemanticsGestureHandler`, `RenderNestedScrollViewViewport` with
`RenderSliverOverlapAbsorber` and `RenderSliverOverlapInjector`, `RenderPerformanceOverlay`,
`RenderBlockSemantics`, `RenderTapRegionSurface`, `RenderProxyBox`. Each entry names the FLUI
form, the ADR, and the Flutter behaviours that form does not reach
([design.md](design.md), "Covered by a different design").

The nested-scroll entry does more than list what is not reached. For each behaviour below it
names the section of ADR-0209 that defines it and the test that holds it, or says that nothing
defines it and adds the entry to [gaps.md](../../../gaps.md):

| Behaviour | Flutter's record |
|-----------|------------------|
| Several inner scrollables under one header (tabs kept alive): each is bound to the header by its own place in the tree, and scrolling one does not move the others | #40740: the coordinator assumes one inner position, and positions attached to one ambient primary controller accumulate |
| A floating header returns on an inner scroll, and whether its snap works through the connection | #17518: `floatHeaderSlivers` was added and snap documented as unsupported |
| Overscroll at the boundary: who stretches or bounces when the inner scrollable is at its edge | #54059 (open): the outer view is assumed never to overscroll |
| A fling handed from the outer scrollable to the inner one and back is continuous | #59440† (open) |
| A snapping header under a pull-to-refresh | #31898† |
| No pair of slivers is wired by hand to pass an overlap along | #22393†: the absorber takes one child |

Acceptance: the entries exist and cite tests by name; each nested-scroll row has a section and a
test, or a gap entry; the `checks` job's document steps pass.

## Tier B: beyond Flutter

Each is a primitive Flutter's rendering library lacks. The pitfalls listed from other frameworks
are recollections, not checked against their trackers or sources ([design.md](design.md),
"Unverified"); each is pinned by a FLUI test all the same.

### R19. A widget reports where its subtree is and how much of it shows

Widgets `GeometryObserver` and `VisibilityObserver`.

- A geometry observer reports its subtree's size and its origin in the window when either
  changed in a frame. The first frame reports once.
- A visibility observer reports the fraction of its subtree the ancestors' clips leave
  visible when the fraction crosses one of its thresholds; by default the one between nothing
  shown and something shown. A margin grows or shrinks the clip the fraction is taken
  against. It is gated: it needs the leader watch to report the clip and the transform.
- A report is delivered after the frame is laid out and painted, with no borrow held, by the
  owner that saw the change. An observer whose subtree does not move runs nothing in a
  frame, and one whose callback changes nothing causes no further frame.
- The fraction counts the clips and transforms the scene applies; it does not count siblings
  painted over the subtree. A subtree that is not painted reports zero. A hidden window
  draws no frame, so the last report stands; the documentation says what to combine it with.
- A callback that changes layout runs again at most once per frame.
- A callback that panics is contained and the observer goes on reporting later changes; the
  change that panicked is not delivered again.
- A rebuilt observer calls the callback of its latest build. After its widget is disposed
  an observer runs no callback and reads nothing.

Acceptance: `a_geometry_observer_reports_a_size_change_once`,
`a_geometry_observer_reports_a_scroll_that_moves_it`,
`a_geometry_observer_reports_nothing_when_nothing_changed`,
`an_observer_whose_subtree_does_not_move_runs_nothing_in_a_frame` (a counting post-frame
lane under an unrelated animation),
`a_rebuilt_observer_calls_its_new_callback`,
`a_visibility_observer_reports_the_fraction_a_viewport_leaves`,
`a_visibility_observer_reports_only_when_a_threshold_is_crossed` (a scroll of many frames
across one threshold: one report),
`a_visibility_observer_with_a_margin_reports_before_its_subtree_shows`,
`a_visibility_observer_reports_zero_for_an_offstage_subtree`,
`a_visibility_observer_under_a_rotation_reports_its_bounding_fraction`,
`a_leader_watch_reports_a_change_of_its_clip_and_transform` (in `flui-rendering`'s tests),
`an_observer_that_changes_nothing_requests_no_frame`,
`an_observer_callback_that_resizes_its_subtree_settles`,
`a_panicking_observer_leaves_the_others_reporting`,
`an_observer_that_panicked_reports_its_next_change`,
`a_disposed_observer_runs_no_callback` (disposed between a frame and its post-frame
callbacks).

Pitfalls elsewhere (recollection): a position callback that sets state and loops every frame; a
visibility package that reports from a 500 ms timer, which leaks into tests and delays
impressions; a fraction that ignores occlusion being read as "seen".

### R20. A lazy list keeps the header of the current section in view

`SliverList` and `SliverVariedExtentList` with sticky rows.

- The list is told which indices are headers. The nearest header at or before the first visible
  row is painted at the leading edge, and the next header pushes it off as it arrives.
- A stuck header is hit before the rows it covers and is read where it is painted (R2). It stays
  built while its section is in view, however far its own row has scrolled out of the band the
  list retains. It is not one of ADR-0265's pinned children, which are published and neither
  painted nor hit: the list keeps one index outside its band for it.
- The lazy element has one set of children kept outside the band, the held ones and the
  stuck header, and every check that spares or admits a child outside the band reads it. A
  stuck header whose row content changes is rebuilt, and one whose index moves because a
  keyed row was inserted above it keeps its element and its state.
- One rule lays a stuck header out in a pass. A header that is both stuck and held (it has
  focus) is laid out, placed and measured by the sticky rule alone.
- The list reports the stuck header's extent as its scroll obstruction, so a reveal of a row
  stops below the header.
- Finding the header for an offset costs O(log n) in the number of headers.

Acceptance: `harness_sticky_row_stays_at_the_leading_edge_within_its_section`,
`harness_sticky_row_is_pushed_off_by_the_next_header`,
`harness_sticky_row_is_hit_before_the_row_it_covers`,
`harness_sticky_row_reports_its_extent_as_obstruction`,
`harness_sticky_row_works_in_all_four_axis_directions`,
`harness_the_list_without_sticky_rows_paints_and_hits_as_before`; widget tests
`a_sticky_header_far_above_the_band_stays_built_and_painted` (a section of a thousand rows,
scrolled to its end), `a_sticky_header_that_is_no_longer_current_is_released`,
`a_focused_sticky_header_is_painted_and_hit`,
`harness_a_stuck_header_that_is_also_pinned_is_laid_out_once` (a counting child; its offset
is the sticky rule's),
`a_stuck_header_outside_the_band_rebuilds_when_its_data_changes` (keyless rows),
`a_stuck_header_keeps_its_state_when_a_row_is_inserted_above_it` (keyed rows),
`revealing_a_row_stops_below_the_sticky_header`,
`a_sticky_header_is_read_before_its_sections_rows`,
`a_list_of_ten_thousand_sections_builds_only_its_window`.

Pitfalls elsewhere (recollection): scrolling to an index lands the item behind the sticky
header; sticky headers flicker with item placement animations; only one level pins.

### R21. A box lays its children out on a grid of tracks

`RenderGrid`, widget `Grid`; `RenderTable`'s column widths are resolved by the same module.

- Columns and rows are tracks: a fixed length, a share of the free space, the content's size, or
  a minimum and maximum of two of those. Tracks are separated by gaps.
- A child states the cell it starts in and how many tracks it spans, or is placed in the next
  free cell in row order. A child that names a cell outside the declared tracks adds content-
  sized tracks.
- Tracks are sized from the children's intrinsic sizes: columns first, then rows with the column
  widths known. A spanning child's size is shared among the tracks it spans that can grow.
- A child is aligned in its cell on both axes; intrinsic sizes and dry layout follow the same
  resolution as layout.
- Reading and focus order are the children's order, whatever cells they name.
- With no spans, the column widths are the ones `RenderTable` computes today.
- The track type has one name. `TableColumnWidth` is gone from every crate and package that
  named it, the facade's re-export and the material data table included.

Acceptance: `harness_grid_shares_free_space_among_fraction_tracks`,
`harness_grid_sizes_a_content_track_to_its_widest_child`,
`harness_grid_shares_a_spanning_childs_width_among_its_tracks`,
`harness_grid_sizes_rows_after_columns` (a wrapping paragraph),
`harness_grid_places_unplaced_children_in_row_order`,
`harness_grid_refuses_overlapping_explicit_cells`,
`harness_grid_intrinsics_match_its_layout`, `harness_grid_mirrors_its_columns_in_rtl`,
`harness_grid_of_fraction_tracks_under_an_unbounded_axis_is_a_layout_error`; the rows of
`RenderTable` unchanged over the shared module; `flui-sdk`'s surface test and the material
data table's tests green under the new name.

Pitfalls elsewhere (recollection): a share of free space with a content minimum that overflows;
a height that depends on a width resolved in the wrong pass; automatic dense placement that
makes visual order differ from reading order (not offered here); a cell that asks for infinite
width stretching every column.

### R22. A wrap stops after a number of runs and shows what is left over

`RenderWrap` with `max_runs` and an overflow indicator; widget `Wrap`.

- The wrap lays out at most `max_runs` runs. When children are left over, its last child, the
  indicator, is placed at the end of the last run, and the run gives up as many children as the
  indicator needs.
- The indicator is told how many children are hidden, after the split is known: the wrap's
  widget rebuilds it in the frame after a layout changed the count, with nothing else
  triggering the rebuild, and a layout that leaves the count as it was rebuilds nothing.
  Relabelling ends: room held for a wider indicator is kept while the constraints, the
  wrap's configuration and the sizes of the children before the indicator are the same, so
  the hidden count can only grow while a label settles, and it is bounded by the number of
  children. The indicator's own size is not part of that key; a change of any part of it
  starts the room again from the indicator's size.
- A hidden child is not painted, not hit and not published (R2). With nothing left over the
  indicator is hidden.
- The rule that splits a run and holds room for an indicator is the one `RenderToolbarItems`
  uses for its collapsed row.

Acceptance: `harness_wrap_stops_after_its_maximum_runs`,
`harness_wrap_places_its_indicator_at_the_end_of_the_last_run`,
`harness_wrap_hides_its_indicator_when_everything_fits`,
`harness_wrap_reports_the_number_of_hidden_children`,
`harness_wrap_split_is_stable_when_the_indicator_changes_width`,
`harness_wrap_hidden_children_are_not_hit_or_published`,
`harness_wrap_without_a_maximum_lays_out_as_before`,
`harness_wrap_keeps_its_reserve_when_the_indicator_shrinks`,
`harness_wrap_drops_its_reserve_when_a_child_changes_size` (and when the constraints or the
spacing change); widget tests
`a_wrap_relabels_its_indicator_in_the_frame_after_the_count_changed` (nothing but the layout
changed), `a_wrap_whose_label_widens_settles` (9 hidden becoming 10),
`a_wrap_whose_label_narrows_when_more_is_hidden_settles` (a label that becomes "99+"),
`a_wrap_whose_count_did_not_change_rebuilds_nothing`,
`a_wrap_without_an_indicator_schedules_nothing`; the rows of `RenderToolbarItems` unchanged.

Pitfalls elsewhere (recollection): the indicator's label depends on the hidden count and the
count depends on the indicator's width, which oscillates unless one side is fixed first.

### R23. One policy places a floating child against its anchor

`AnchoredPlacement` in `flui-widgets`; the popup menu and the selection toolbar use it.

- Given the anchor's rectangle, the child's size and the area it may use, the policy returns
  the child's position and the side it chose: the preferred side when it fits, the next of an
  ordered list of fallbacks when it does not, then shifted to stay inside the area.
- The child's size limit is known before its side and does not depend on it. A caller chooses
  what happens when the child fits on no side: it is limited beforehand to the roomiest listed
  side, so it always fits; or it keeps its size, is clamped into the area and the result says
  that it covers the anchor.
- The choice is stable: a child that fits on the side it is on stays there when the anchor moves,
  though the preferred side would fit again only just. The side is remembered across rebuilds
  of the popup and forgotten when the popup closes.
- The area is the overlay less the view insets and padding (the keyboard, a cutout), taken by
  one function for every caller.
- Start and end follow the reading direction. A result is always finite and inside the area
  when the child is no larger than it.

Acceptance: `a_placement_takes_its_preferred_side_when_it_fits`,
`a_placement_falls_back_in_order`, `a_placement_shifts_to_stay_inside`,
`a_placement_does_not_flip_back_and_forth_at_the_threshold`,
`a_placement_keeps_its_side_across_a_rebuild`, `a_placement_mirrors_in_rtl`,
`a_placement_of_a_child_larger_than_the_area_is_finite`,
`a_placement_that_shrinks_fits_on_its_roomiest_side`,
`a_placement_that_overlaps_reports_that_it_covers_its_anchor`,
`a_placement_stays_above_the_keyboard`; the popup menu's and the selection toolbar's placement
tests unchanged.

Pitfalls elsewhere (recollection): flip oscillation at the threshold; the order of size limit
and flip; an anchor outside the area; keyboard insets.

### R24. A layout shows the first of its children that fits

`RenderFirstThatFits`, widget `FirstThatFits`.

- Children are alternatives in order of preference. The first whose size, laid out without the
  limit on the chosen axes, is within the incoming constraints is shown at that size; when none
  fits, the last is shown under the constraints.
- The others stay mounted and are not painted, not hit and not published (R2).
- A child is laid out at most once in a layout pass, under one set of constraints, so an
  alternative that builds during layout is built once per pass and the choice settles in the
  frame.
- Intrinsic sizes are those of the first alternative; the dry layout runs the same choice.

Acceptance: `harness_first_that_fits_shows_the_first_alternative_that_fits`,
`harness_first_that_fits_lays_each_alternative_out_once_per_pass`,
`harness_first_that_fits_falls_back_to_its_last_alternative`,
`harness_first_that_fits_hides_the_others_from_hits_and_semantics`,
`harness_first_that_fits_on_one_axis_ignores_the_other`,
`harness_first_that_fits_dry_layout_makes_the_same_choice`; widget test
`a_layout_builder_in_an_alternative_builds_once_per_pass_and_settles`.

Pitfalls elsewhere (recollection): every alternative is instantiated, with its side effects; an
alternative that expands always "fits".

### R25. A lazy list transforms each item by its place in the viewport

`SliverList`, `SliverFixedExtentList`, `SliverVariedExtentList` and `SliverGrid` with an item
effect.

- An effect is a pure function from an item's progress through the viewport to a transform and
  an opacity. It is evaluated in layout; scrolling rebuilds nothing.
- An item is hit, and its semantics rectangle is published, through the transform it is painted
  with (R2). An effect never changes layout: scroll extent and item positions are the same with
  and without it.
- A non-finite transform hides the item.
- With sticky rows (R20) the list still commits what it shows once per layout: the effect is
  applied to the rows and not to the stuck header, which stays over them.

Acceptance: `harness_item_effect_scales_an_item_at_the_viewport_edge`,
`harness_item_effect_leaves_a_stuck_header_in_place_over_its_rows`,
`harness_the_fixed_extent_list_and_the_grid_without_an_effect_paint_and_hit_as_before`,
`harness_item_effect_is_hit_through_its_transform`,
`harness_item_effect_leaves_the_scroll_extent_unchanged`,
`harness_item_effect_with_a_non_finite_transform_hides_the_item`,
`harness_a_sliver_without_an_effect_maps_its_children_as_before`; widget test
`scrolling_a_list_with_an_item_effect_rebuilds_nothing`.

Pitfalls elsewhere (recollection): hit testing against the untransformed rectangle; a layer per
item; progress measured from an edge by one framework and from the centre by another.

### R26. A subtree is composited as a group with a blend mode, and a widget masks another

Widgets `BlendGroup` and `Mask`.

- A blend group draws its subtree into an isolated group and composites the group with a blend
  mode. Inside the group, children blend with each other and not with what is behind it.
- A mask multiplies its content by the alpha of a second child. The mask child is laid out at
  the content's size, is not hit and is not published.
- Hit testing and layout are the content's. A group that is fully transparent or empty pushes
  no layer.
- A blend mode and an opacity on one node are one composite: the group is drawn once and
  composited with that alpha and that mode.

Acceptance: readback rows `a_blend_group_blends_as_a_whole_with_what_is_behind_it`,
`a_half_transparent_blend_group_composites_once_against_the_page` (a multiply group at half
opacity over a coloured page; the samples are those of one composite with that alpha and mode,
and the row records one offscreen group),
`children_of_a_blend_group_do_not_blend_with_the_backdrop`,
`a_mask_multiplies_its_content_by_the_masks_alpha`,
`a_mask_at_a_fractional_offset_leaves_no_edge`; `a_masked_subtree_is_hit_where_its_content_is`,
`a_mask_child_is_not_hit_or_published`.

Pitfalls elsewhere (recollection): a blend mode outside an isolated group blends with whatever
is behind; clear and destination-in modes are silently wrong without an offscreen group; the
cost of an offscreen group per use.

## What is not here

Each item is absent for a stated reason, not by omission:

| Item | Why it is out | What an application does instead |
|------|---------------|----------------------------------|
| `OverlayPortal` (`_RenderDeferredLayoutBox`, `_RenderLayoutSurrogateProxyBox`) | It needs an element whose render parent is not its element parent, a capability of the element tree and of the overlay, not a render object | `PageHandle::push_popup` with a `PopupRoute`, or `OverlayHandle::insert` |
| `SnapshotWidget` | An optimisation the retained scene and repaint boundaries already give (ADR-0304). Its custom snapshot painter has no FLUI counterpart | A repaint boundary |
| `SliverPrototypeExtentList` | It needs an element-side prototype slot beside the lazy children | `SliverFixedExtentList` with a measured extent, or `SliverList` |
| `SizeChangedLayoutNotifier` | It reports through a notification. `flui-view` has the transport (`element/notification.rs`) and no production listener widget or sender ("No scroll notifications", [gaps.md](../../../gaps.md)) | `GeometryObserver` (R19), or `LayoutBuilder` |
| `_RenderSliverEnsureSemantics` | Owned by the gap "A list that fills part of its viewport publishes its whole cache to a screen reader" | — |
| `_RenderScrollSemantics`, `_RenderSemanticsClipper` | The same gap: it closes with the scroll semantics node formed inside the viewport's clip | — |
| `_RenderExclusiveMouseRegion` | Private to Flutter's tooltip; it belongs to a tooltip spec, with the tooltip widget FLUI does not have | `MouseRegion::opaque` |
| `_RenderCompositionCallback` | Flutter's way to track a text field's transform for the IME; FLUI's cursor-area loop is ADR-0030's | — |
| Material and cupertino private render objects | Catalog work; the design lists the primitives they need | — |
| Add-to-app embedding of FLUI in a native view | FLUI owns the process ("Add-to-app: no FLUI view inside an existing native app", [gaps.md](../../../gaps.md)); the reverse of R17 | — |
| Beyond-Flutter candidates that were deferred or rejected | [design-beyond.md](design-beyond.md), "Deferred" and "Rejected", one reason each | — |
