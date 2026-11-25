# render-object-coverage: requirements

- **Status:** Not started — awaiting owner approval of requirements and design
- **Date:** 2026-11-25
- **Base:** `claude/project-thread-03lo7m` @ `fcab477`
- **Reference:** `flutter/flutter` tag `3.50.1` (`ae9b586`), `packages/flutter/lib/src`. Every
  statement below about Flutter was read from that tree; statements about its issue tracker are
  limited to the issues its own source cites (see "Unverified" in [design.md](design.md)).
- **Decisions it builds on:**
  [ADR-0017](../../../adr/ADR-0017-build-during-layout-callback-seam.md),
  [ADR-0054](../../../adr/ADR-0054-the-viewport-commits-one-result.md),
  [ADR-0209](../../../adr/ADR-0209-nested-scroll-connection.md),
  [ADR-0226](../../../adr/ADR-0226-semantics-reading-order-and-sibling-barriers.md),
  [ADR-0265](../../../adr/ADR-0265-pinned-lazy-children-and-the-focusable-list.md),
  [ADR-0304](../../../adr/ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md),
  [ADR-0322](../../../adr/ADR-0322-a-control-mounts-one-element-for-its-behaviours.md),
  [ADR-0325](../../../adr/ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md).
  The spec changes none of them. It names six new ADRs that must be accepted before the tasks
  that depend on them start ([design.md](design.md), "ADRs this spec needs").
- **Design:** [design.md](design.md); tasks: [tasks.md](tasks.md)

## Why

FLUI's catalog of render objects was grown by need: a widget was wanted, its render object was
written. Flutter 3.50.1 exports 103 concrete render objects, and an audit against `fcab477` finds
80 of them in `flui-objects`, three present with a part missing, four answered by a different
FLUI design, and sixteen absent. An application ported from Flutter meets the absent ones as a
widget that does not exist (`TreeSliver`, `ListWheelScrollView`, `TapRegion`, a `WidgetSpan`
inside text, a platform view), and each of them needs a decision about the render layer before a
widget can be written.

The owner's goal is that every render object of Flutter 3.50.1 has an answer in FLUI: the same
object, an object of a different shape that reaches the same behaviour, or a recorded reason why
FLUI does not have it. This spec says which answer each one gets and how the missing ones are
built. It is not a port (AGENTS.md, "Design stance"): what crosses over is a behaviour pinned by
a behaviour test, together with the edge cases and defects Flutter's source shows for it.

Breaking FLUI's public API is allowed (pre-1.0). Every form a task replaces is named in
[design.md](design.md) with its migration, and the task that brings the replacement moves every
user and deletes the old form in the same change.

## What the audit found, corrected

The audit was checked against the code where the spec relies on it. Four entries change:

| Audit entry | What the code shows | Consequence |
|-------------|---------------------|-------------|
| `RenderBlockSemantics` missing | The behaviour exists: `Semantics::block_previous_siblings` sets the sibling barrier of ADR-0226. No `BlockSemantics` widget exists, and three documents say FLUI has none | A widget by composition (R9), no render object |
| `RenderPerformanceOverlay` missing | `crates/flui-runtime/src/performance_overlay.rs` and `UiRuntime::set_performance_overlay` draw the overlay as a runtime-composed scene node | Covered by a different design (R18) |
| `Magnifier` (`_RenderMagnification`) in the second tier | `crates/flui-widgets/src/magnifier.rs` has `RawMagnifier`, a composition over `BackdropFilter` and clips | Covered, nothing to build |
| `RenderSliverClipRect`, `RenderSliverClipRRect` | Present in Flutter 3.50.1 as `rendering/sliver_clip.dart`, with a `ClipOverlapBehavior` the audit did not list | R3 takes the overlap behaviour too |

The count of 80 of 103 was not recounted. `docs/testing.md` says the harness manifest lists 81
exported render types; the list in `crates/flui-objects/tests/render_object_harness.rs` has 106
entries. The first task removes the copied count (R1).

## Scope

In scope, in the order the tasks build them:

1. the harness accepts rows from per-family modules, so parallel tasks stop meeting in one
   16,000-line file (R1);
2. the open gap "An overflow clip cuts off paint but not semantics or input regions"
   ([gaps.md](../../../gaps.md)) for the six objects it names (R2). The spec takes it because
   every new object below reports its clip the same way, and the gap's cure (one owner for the
   overflow answer) is the form the new objects copy;
3. the missing sliver objects: clips, decoration, varied-extent list, tree, child-sized headers,
   a sliver layout builder (R3–R8);
4. widgets reached by composing existing objects, and one new box layout (R9);
5. the missing paint properties of `RenderImage` and the subtree filters (R10, R11);
6. tap regions (R12);
7. inline children in `RenderParagraph` and `RenderEditable` (R13, R14);
8. the list wheel and the two-axis viewport (R15, R16);
9. native views, which wait for an owner decision (R17);
10. the record of what stays covered by a different design (R18).

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
  there, with the files of the frozen crate it needs.
- **G2.** Everything else (`flui-objects`, `flui-view`, `flui-widgets`, `flui-interaction`,
  `packages/`) is ungated and is ordered first.
- **G3.** A gated change is the smallest one that serves its requirement, and it comes with the
  ADR that decides it. No task adds a hook to a frozen crate "for later".

R1–R9, R12 and the first half of R10 are ungated. R11, R13–R17 and the second half of R10 are
gated.

## Requirements

Each requirement names the behaviour test that holds it. Harness rows are functions of
`crates/flui-objects/tests/` called from a `family_*` table; widget tests are modules of
`crates/flui-widgets/tests/main.rs`. Test names are proposals fixed when the task lands, and
`tasks.md` records the final names as validation evidence.

### R1. The catalog rule holds for every new object, and the harness takes parallel work

- Every concrete `RenderBox` or `RenderSliver` this spec adds is exported from
  `crates/flui-objects/src/lib.rs`, listed in `RENDER_OBJECT_TYPES`, has at least one
  `harness_*` row, and has a widget in `flui-widgets` named in [design.md](design.md).
- A harness row may live in a family module file beside the root file, and the two guards
  (`catalog_covers_every_render_object_name`, `render_object_types_match_exports`) still fail
  for an exported object without a row.
- No document repeats the number of exported render types.

Acceptance: `catalog_covers_every_render_object_name` fails when a row in a family module is
deleted (shown once by hand in the task that moves the guard); `docs/testing.md` links the list
instead of counting it.

### R2. An overflow clip cuts semantics and input regions where it cuts paint

For `RenderStack`, `RenderIndexedStack`, `RenderWrap`, `RenderFittedBox`, `RenderAnimatedSize`
and `RenderConstraintsTransformBox`, with a clip behaviour other than `Clip::None`:

- a child that overflows the box publishes a semantics rectangle cut to the box, and an input
  region cut to the box;
- a child wholly outside the box publishes neither;
- when nothing overflows, no clip is reported and no clip scope is pushed;
- the overflow answer that paint, semantics and input regions read is the one written by the
  last layout that committed. A layout that returns early or fails leaves the previous answer,
  never a cleared one.

Acceptance: one row per object for the semantics and region cut-off
(`harness_<object>_cuts_semantics_and_regions_at_its_overflow_clip`) and one row for the order
(`harness_overflow_answer_survives_a_layout_that_fails`). The gap entry leaves
[gaps.md](../../../gaps.md) in the same change.

Flutter edge cases: `Clip::None` reports no clip even when content overflows; `RenderFittedBox`
with `BoxFit::None` and an empty child reports no overflow.

### R3. A sliver clips its child to a rectangle or a rounded rectangle

`RenderSliverClipRect` and `RenderSliverClipRRect`, widgets `SliverClipRect` and
`SliverClipRRect`.

- The clip is the sliver's maximum paint rectangle: its scroll extent laid along the main axis
  from its paint origin, across the full cross axis.
- `ClipOverlapBehavior` says what the clip does where a pinned sliver before it overlaps it:
  `None` keeps the full rectangle, `FollowEdge` moves the leading edge to the end of the
  overlap, `PreserveShape` (rounded clip) keeps the corner radii while the edge moves.
- A point outside the clip does not hit the child. Semantics and input regions of the child are
  cut by the same clip.
- Nothing is painted and no scope is pushed when the child is not visible or the clip is empty.

Acceptance: `harness_sliver_clip_rect_follows_the_pinned_edge`,
`harness_sliver_clip_rrect_preserves_its_shape_under_an_overlap`,
`harness_sliver_clip_rejects_a_hit_outside_the_clip`,
`harness_sliver_clip_of_an_unbounded_child_is_finite`, each in all four axis directions.

Flutter edge cases to pin: a child whose maximum paint extent is infinite gets a clip bounded by
the cache window, never a non-finite rectangle; reversed axis directions flip the leading edge;
the defaults differ (`Clip::HardEdge` for the rectangle, `Clip::AntiAlias` for the rounded one).

### R4. A sliver paints a decoration behind or in front of its child

`RenderDecoratedSliver`, widget `DecoratedSliver`.

- The decoration covers the sliver's maximum paint rectangle (R3's rectangle), so it scrolls
  with the content and covers items that are not built.
- `DecorationPosition` chooses background or foreground, as for `RenderDecoratedBox`.
- An invisible child paints no decoration.

Acceptance: `harness_decorated_sliver_covers_its_whole_scroll_extent`,
`harness_decorated_sliver_over_an_unbounded_list_is_finite`,
`harness_decorated_sliver_paints_nothing_when_its_child_is_invisible`.

### R5. A lazy list takes each item's extent from a delegate

`RenderSliverVariedExtentList`, widget `SliverVariedExtentList` and a `ListView` constructor.

- The extent of item `i` is the delegate's answer for `i` and the list's dimensions (viewport
  main-axis extent, cross-axis extent). The answer does not depend on the scroll offset.
- The scroll extent is exact (the sum of all answers), from the first layout.
- After the first layout under a delegate and dimensions, a layout costs O(log n) in the item
  count plus the items in the window, not O(n).
- A non-finite or negative answer is a layout error naming the index, not a panic and not a
  published geometry.
- The number of children built in one layout is bounded even when every extent is zero.

Acceptance: `harness_varied_extent_list_places_items_at_the_sum_of_their_extents`,
`harness_varied_extent_list_asks_the_delegate_once_per_item_per_dimensions` (a counting
delegate over 100,000 items, scrolled), `harness_varied_extent_list_refuses_a_non_finite_extent`,
`harness_varied_extent_list_of_zero_extents_builds_a_bounded_window`.

Defects in Flutter's source to pin against (`rendering/sliver_fixed_extent_list.dart`): the
offset of an index, the index at an offset and the maximum scroll offset each walk from item 0
on every call; with an unknown count and extents that are all zero the walk `while (position <
scrollOffset)` does not end.

### R6. A lazy list shows a tree of rows

`RenderTreeSliver`, widget `TreeSliver`.

- Rows are the visible nodes in depth-first order. A row is indented on the cross-axis start
  edge by its depth times the indentation.
- Expanding or collapsing a node inserts or removes its descendants' rows. While the change
  animates, the rows being revealed are clipped to the region below their parent row, in the
  coordinates the rows are painted in.
- Offsets cost what R5 costs; a change at row `i` recomputes rows from `i` on.
- A row publishes its level, its position in its sibling set and its expanded state.

Acceptance: `harness_tree_sliver_indents_a_row_by_its_depth`,
`harness_tree_sliver_reveal_clip_holds_under_a_scroll_offset` (the clip is checked with the
sliver scrolled by more than one viewport), `harness_tree_sliver_indents_from_the_right_in_rtl`,
widget tests `a_tree_row_publishes_its_level_and_expanded_state` and
`collapsing_a_node_removes_its_rows`.

Flutter edge cases and defects to pin (`rendering/sliver_tree.dart`): the render object asserts
`AxisDirection.down` and supports no other direction (FLUI supports all four); the reveal clip
is built from layout offsets while the rows are painted at layout offset minus scroll offset
(read from the source, not reproduced by running it); animation offsets are fixed when an
animation starts, so a second toggle during the first is the order to test.

### R7. A header sliver is sized by its child

Widgets `PinnedHeaderSliver`, `SliverResizingHeader`, `SliverFloatingHeader` and their render
objects.

- A pinned header stays at the viewport's leading edge once reached; its extent is its child's.
- A resizing header shrinks from its maximum to its minimum extent as it scrolls, both taken
  from prototype children that are laid out and never painted, hit or published.
- A floating header returns on a reverse scroll, by the amount scrolled, and can snap.
- Each reports the scroll obstruction it causes, so a reveal of a rectangle and R3's `FollowEdge`
  stop at its trailing edge.

Acceptance: `harness_pinned_header_takes_its_extent_from_its_child`,
`harness_resizing_header_never_paints_its_prototypes`,
`harness_resizing_header_reports_its_minimum_as_obstruction`,
`harness_floating_header_returns_by_the_distance_scrolled_back`, and a widget test
`a_pinned_header_keeps_a_following_clip_at_its_edge`.

Flutter edge cases: a header taller than the viewport; a resizing header with no maximum
prototype takes its maximum from the child's unconstrained size; a child whose size changes
while pinned.

### R8. A sliver can be built from its constraints

Widget `SliverLayoutBuilder`; `PageView::pad_ends`.

- The builder runs during layout with the sliver constraints and its result is laid out with
  the same constraints; the builder's sliver geometry is the result's.
- It rebuilds when the constraints change or the widget asks, and not otherwise.
- A page view whose `viewport_fraction` is below 1 can centre its first and last page.

Acceptance: widget tests `a_sliver_layout_builder_rebuilds_only_when_its_constraints_change`,
`a_sliver_layout_builder_adopts_its_childs_geometry`,
`a_padded_page_view_centres_its_first_and_last_page`.

### R9. Three widgets are reached by composition or one new layout

- `BlockSemantics` hides the semantics of everything painted before it in its parent, through
  the sibling barrier of ADR-0226.
- `SliverVisibility` shows, hides or replaces a sliver, with the same maintain switches as
  `Visibility`.
- `OverflowBar` lays its children in a row when they fit the available width and in a column
  when they do not.

Acceptance: `block_semantics_hides_what_was_painted_before_it`,
`a_hidden_sliver_takes_no_extent_and_no_hits`,
`a_hidden_sliver_that_maintains_size_keeps_its_extent`,
`harness_overflow_bar_wraps_to_a_column_when_its_row_does_not_fit`,
`harness_overflow_bar_intrinsics_match_its_layout`. The three documents that say FLUI has no
`BlockSemantics` are corrected in the same change.

### R10. An image is tinted, faded, inverted and sampled as asked

`RenderImage` gains `color` with `color_blend_mode`, `opacity`, `invert_colors` and
`filter_quality`; `Image` gains the matching builder methods.

Acceptance: readback rows whose samples distinguish each property from its absence
(`an_image_is_tinted_by_its_colour_and_blend_mode`, `an_image_is_faded_by_its_opacity`,
`an_inverted_tinted_image_inverts_after_the_tint`,
`an_upscaled_image_is_sampled_by_its_filter_quality`).

Edge cases: opacity 0 paints nothing; inversion applies after the tint, as in Flutter;
`FilterQuality::None` gives nearest sampling. The first two properties are ungated; the last
two change `flui-painting` and `flui-engine` (G1).

### R11. A widget filters the colours or the pixels of its subtree

Widgets `ColorFiltered` and `ImageFiltered`. Closes the gap "No widget filters its subtree's
colours or pixels" ([gaps.md](../../../gaps.md)).

Acceptance: a readback row per filter kind, hit-testing unchanged by the filter
(`a_filtered_subtree_is_hit_where_it_is_laid_out`), and a disabled `ImageFiltered` pushes no
layer.

### R12. A region learns of a tap inside or outside it

Widgets `TapRegionSurface` and `TapRegion`, render object `RenderTapRegion`.

- A pointer down or up is *inside* a region when the region, or a region of its group, is on the
  pointer's hit path; otherwise it is *outside*. Geometry is not consulted.
- A region can consume outside taps: the tap dismisses and reaches nothing else.
- An accessibility activation is classified by whether its target lies in the region's subtree.
- A region whose page is not the current one is not told of outside taps.

Acceptance: `a_tap_on_an_overlay_above_a_region_is_outside_it`,
`a_tap_inside_a_group_member_is_inside_every_member`,
`a_consumed_outside_tap_reaches_no_detector`,
`a_screen_reader_activation_is_classified_by_ancestry`,
`a_region_on_a_covered_page_hears_no_outside_tap`.

Defect in Flutter's source to pin against (`widgets/tap_region.dart`): accessibility taps are
classified by hit-testing the centre of the node's rectangle, and a TODO there says
`consumeOutsideTaps` cannot stop a semantics action (it cites flutter/flutter pull request
183093).

### R13. A paragraph lays out the boxes its spans embed

`RenderParagraph` with inline children; `Text::rich` and `RichText` accept a widget inside a
span at any depth.

- An inline child is laid out under the paragraph's maximum width, takes its place in the line
  by its `PlaceholderAlignment`, and moves with the text when the line breaks.
- A child whose placeholder is cut by `max_lines` or an ellipsis is not painted, not hit and
  not published. Children before it are unaffected.
- Intrinsic sizes and dry layout include the children.
- The paragraph's text offsets count one position per placeholder.

Acceptance: `harness_paragraph_places_an_inline_child_on_its_line`,
`harness_paragraph_aligns_an_inline_child_to_the_baseline`,
`harness_paragraph_drops_a_truncated_inline_child_from_paint_hits_and_semantics`,
`harness_paragraph_intrinsics_include_inline_children`, widget tests
`a_widget_nested_in_a_styled_span_is_laid_out_in_the_text` and
`a_selection_crosses_an_inline_widget_as_one_position`.

Flutter edge cases to pin (`rendering/paragraph.dart`): paint and hit-testing stop at the first
child without an offset, so every child after a truncated one is skipped; a baseline is queried
only for `PlaceholderAlignment::Baseline`; `WidgetSpan` scales its child by a linear factor of
the text scaler, which FLUI's non-linear `TextSizing` (ADR-0183) cannot reproduce.

### R14. An editable field lays out inline children the same way

`RenderEditable` shares R13's seam: the caret, selection and hit positions treat a placeholder
as one position, and the caret rectangle beside a placeholder takes the line's height.

Acceptance: `harness_editable_steps_the_caret_over_an_inline_child`,
`harness_editable_caret_beside_an_inline_child_has_the_line_height`.

### R15. A wheel of items scrolls around a cylinder

Widget `ListWheelScrollView`, render object proposed as `RenderSliverWheel`.

- Items of one extent lie on a cylinder; the item at the centre is the selected one. Diameter
  ratio, perspective, off-axis fraction, squeeze, magnification of the centre band and the
  opacity of the others are parameters.
- Items are built lazily. A looping wheel repeats its items without end in both directions.
- An item is hit where it is drawn: the transform that paints it is the transform that
  hit-tests it and that maps its semantics rectangle, and it exists after layout, before any
  paint.
- Scrolling settles on an item.

Acceptance: `harness_wheel_hits_an_item_through_its_cylinder_transform_before_the_first_paint`,
`harness_wheel_hits_the_magnified_item_through_the_magnifier_transform`,
`harness_wheel_skips_items_beyond_a_quarter_turn`, widget tests
`a_looping_wheel_scrolls_past_its_last_item_to_its_first` and
`a_wheel_settles_on_an_item`.

Defect in Flutter's source to pin against (`rendering/list_wheel_viewport.dart`): hit-testing
uses the transform saved into parent data during paint, written only by the cylindrical paint
path, so a magnified centre item is hit through the cylinder transform and a child that was
never painted is not hit at all.

### R16. A viewport scrolls lazily built children along two axes

Render object proposed as `RenderTwoAxisViewport`, widget proposed as `TwoAxisScrollView`.

- Children are identified by a vicinity (column, row) and built on demand for the visible and
  cached region; a layout policy places them.
- Children are hit in reverse paint order.
- The viewport clips only when a child extends past it.
- Pinned vicinities stay built outside the region (ADR-0265).

Acceptance: `harness_two_axis_viewport_builds_only_the_visible_region`,
`harness_two_axis_viewport_hits_the_child_painted_last`,
`harness_two_axis_viewport_pushes_no_clip_when_nothing_overflows`,
`harness_two_axis_viewport_keeps_a_pinned_vicinity`.

Defects in Flutter's source to pin against (`widgets/two_dimensional_viewport.dart`): hit
testing walks the child map in map order rather than reverse paint order; the overflow flag is
set by comparing an `Offset` with a `Size`, which is always unequal, so the viewport always
clips.

### R17. A native view sits in the UI

`RenderAndroidView`, `RenderUiKitView`, `RenderAppKitView`, `PlatformViewRenderBox` and
`RenderWebImage` in Flutter. In FLUI this is the open gap "No native views inside the UI (web
view, map, video)" ([gaps.md](../../../gaps.md)), which says it needs an owner decision.

This spec does not decide it. It requires, before any code:

- the owner's decision that native views enter the contract, and for which hosts;
- the ADR named in [design.md](design.md), which decides how a native view sits in the scene,
  composites with the GPU surface, takes hit tests and joins the semantics tree;
- that the frame path stays synchronous: no resize handshake that spans frames inside layout.

Defects Flutter's source cites for its own design (`rendering/platform_view.dart`):
flutter/flutter issues 20456 and 190833, both about the asynchronous resize of an Android view
lagging its widget.

### R18. What stays covered by a different design is recorded with what it cannot reach

`crates/flui-objects/ARCHITECTURE.md`, "Mapping decisions", gains one entry for each of:
`RenderSemanticsGestureHandler`, `RenderNestedScrollViewViewport` with
`RenderSliverOverlapAbsorber` and `RenderSliverOverlapInjector`, `RenderPerformanceOverlay`,
`RenderBlockSemantics`. Each entry names the FLUI form, the ADR, and the Flutter behaviours that
form does not reach ([design.md](design.md), "Covered by a different design").

Acceptance: the entries exist and cite tests by name; the `checks` job's document steps pass.

## What is not here

Each item is absent for a stated reason, not by omission:

| Flutter item | Why it is out | What an application does instead |
|--------------|---------------|----------------------------------|
| `OverlayPortal` (`_RenderDeferredLayoutBox`, `_RenderLayoutSurrogateProxyBox`) | It needs an element whose render parent is not its element parent, a capability of the element tree and of the overlay, not a render object | `PageHandle::push_popup` with a `PopupRoute`, or `OverlayHandle::insert` |
| `SnapshotWidget` | An optimisation the retained scene and repaint boundaries already give (ADR-0304). Its custom snapshot painter has no FLUI counterpart | A repaint boundary |
| `SliverPrototypeExtentList` | It needs an element-side prototype slot beside the lazy children | `SliverFixedExtentList` with a measured extent, or `SliverList` |
| `SizeChangedLayoutNotifier` | It reports through a notification, and how notifications and their listeners are exposed is an open entry ("No scroll notifications", [gaps.md](../../../gaps.md)) | `LayoutBuilder` |
| `_RenderSliverEnsureSemantics` | Owned by the gap "A list that fills part of its viewport publishes its whole cache to a screen reader" | — |
| `_RenderScrollSemantics`, `_RenderSemanticsClipper` | The same gap: it closes with the scroll semantics node formed inside the viewport's clip | — |
| `_RenderExclusiveMouseRegion` | Private to Flutter's tooltip; it belongs to a tooltip spec, with the tooltip widget FLUI does not have | `MouseRegion::opaque` |
| `_RenderCompositionCallback` | Flutter's way to track a text field's transform for the IME; FLUI's cursor-area loop is ADR-0030's | — |
| Material and cupertino private render objects | Catalog work; [design.md](design.md) lists the primitives they need | — |
| Add-to-app embedding of FLUI in a native view | FLUI owns the process ("Add-to-app: no FLUI view inside an existing native app", [gaps.md](../../../gaps.md)); the reverse of R17 | — |
