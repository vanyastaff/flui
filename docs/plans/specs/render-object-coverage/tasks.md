# render-object-coverage: tasks

- **Status:** Not started — awaiting owner approval of requirements and design
- **Date:** 2026-11-25
- **Base:** `claude/project-thread-03lo7m` @ `fcab477`
- **Design:** [design.md](design.md) (shared seams, ADR list, decisions),
  [design-parity.md](design-parity.md) (W1–W18), [design-beyond.md](design-beyond.md)
  (W19–W26); [requirements.md](requirements.md) (R1–R26, gates G1–G3).

## Execution rules

- **One task, one branch, one pull request, one writer per file set** (AGENTS.md, "Working
  here"). "Owns" below is the file set; a task that finds it needs a file another open task
  owns stops and says so here.
- **Shared one-line files.** A task that adds a render object or a widget adds one line to
  each of: `crates/flui-objects/src/lib.rs` and the family's `mod.rs`; the `RENDER_OBJECT_TYPES`
  list and, for a new family, the `mod` line, guard entry and `family_*` test in
  `crates/flui-objects/tests/render_object_harness.rs`; the alias line in
  `crates/flui-view/src/element/sliver_adaptor.rs` for a lazy sliver;
  `crates/flui-widgets/src/lib.rs`, the family's `mod.rs` and
  `crates/flui-widgets/tests/main.rs`. An entry in "Mapping decisions" of
  `crates/flui-objects/ARCHITECTURE.md` is its own paragraph. These edits are not ownership;
  they are rebased mechanically.
- **Checks of the crates touched, in the foreground, one at a time** (builds queue through
  heli): `cargo fmt --all`, then for each crate `cargo heli clippy -p <crate> --all-targets --
  -D warnings` and `cargo heli nextest run -p <crate>`. A task that changes a public API also
  runs its dependents. Whoever coordinates runs the workspace-wide checks once.
- **Every behaviour has a test that fails without it** ("Before each push", item 10): the
  task's rows are run with the change reverted, and the mutation run over its own lines goes
  to the review.
- **A consumer-visible change adds `changelog.d/<branch-slug>.md`.** A task that replaces a
  form names the old form and its migration there.
- **Gated tasks** (G1) are marked *gated* with the frozen-crate files they need. None starts
  while ADR-0304 or ADR-0305 has a bullet in
  [gaps.md](../../../gaps.md#accepted-adr-sections-not-built-yet), unless the owner writes an
  exception beside the task here. A task that edits no frozen crate and depends on a gated one
  is not gated; it waits on its dependency.
- **An ADR task is a design change:** it is read with the design lenses before it merges
  (AGENTS.md, "Before a merge"), and the code tasks it blocks do not start until it is
  accepted.
- **The record first.** A task that pins a tracker row marked `†` in
  [requirements.md](requirements.md) reads the thread before writing the test and corrects the
  row. A task of R19–R26 checks the pitfalls it relies on against the other framework's source
  or tracker and replaces "recollection" in its requirement with what it found.
- **The shared row.** A task whose object keeps a `ShownChildren` adds its call of
  `harness_<object>_hits_and_publishes_a_child_only_where_it_paints`.

## Status

| Task | Content | Req. | Crates | Depends on | Gated | Status |
|------|---------|------|--------|------------|:-----:|--------|
| RC0 | Harness reads family modules; `docs/testing.md` drops the copied count | R1 | `flui-objects` (tests), docs | — | | not started |
| RC1 | The `shown` module (`ChildClip`, `ShownChildren`); six overflow objects and the carousel move onto it; the gap entry leaves | R2 | `flui-objects`, docs | RC0 | | not started |
| RC2 | Sliver pass-through base; six proxies move to it | R3, R4 | `flui-objects` | RC0 | | not started |
| RC3 | `MaxPaintRect`, `RenderSliverClip<S>`, `SliverClipRect`, `SliverClipRRect` | R3 | `flui-objects`, `flui-widgets` | RC1, RC2 | | not started |
| RC4 | `RenderDecoratedSliver`, `DecoratedSliver` | R4 | `flui-objects`, `flui-widgets` | RC3 | | not started |
| RC5 | `BlockSemantics`, `SliverVisibility` | R9 | `flui-widgets`, `flui-material` docs | — | | not started |
| RC6 | `RenderOverflowBar`, `OverflowBar` | R9 | `flui-objects`, `flui-widgets` | RC0 | | not started |
| RC7 | `RenderImage` colour, blend mode and opacity | R10 | `flui-objects`, `flui-widgets` | RC0 | | not started |
| RC8 | `RenderSliverVariedExtentList` on the offset index, `SliverVariedExtentList`, the `ListView` constructor | R5 | `flui-objects`, `flui-view` (alias), `flui-widgets` | RC0 | | not started |
| RC9 | `RenderTreeSliver` with the reveal map, `TreeSliver` and its roster | R6 | `flui-objects`, `flui-view` (alias), `flui-widgets` | RC8, RC1 | | not started |
| RC10 | `RenderPinnedHeaderSliver`, `RenderSliverResizingHeader`, `HeaderFrame`, their widgets; group rows | R7 | `flui-objects`, `flui-widgets` | RC1 | | not started |
| RC11 | `RenderSliverFloatingHeader`, `SliverFloatingHeader`; the floating state leaves `sliver_persistent_header.rs` | R7 | `flui-objects`, `flui-widgets` | RC10 | | not started |
| RC12 | `RenderSliverLayoutBuilder` and its element, `RenderSliverFractionalPadding`, `PageView::pad_ends` | R8 | `flui-objects`, `flui-view`, `flui-widgets` | RC2 | | not started |
| RC13 | "Mapping decisions" entries for what stays covered by a different design; the nested-scroll behaviours against ADR-0209 | R18 | docs, `flui-widgets` (tests) | — | | not started |
| RC14 | ADR: a tap is classified against the regions on the path of its down, by the binding | R12 | `docs/adr` | — | | not started |
| RC15 | Tap regions, the pointer route: event, registry, capability, `RenderTapRegion`, `TapRegion`, the popup's group | R12 | `flui-interaction`, `flui-objects`, `flui-view`, `flui-runtime`, `flui-widgets` | RC14, RC0 | | not started |
| RC16 | ADR: a subtree's filter and its group blend are paint effects | R11, R26 | `docs/adr` | — | | not started |
| RC17 | The filter effect, `RenderColorFiltered`, `RenderImageFiltered` and their widgets | R11 | `flui-rendering`, `flui-engine` (tests), `flui-objects`, `flui-widgets` | RC16, RC0 | yes | not started |
| RC18 | `RenderImage` inversion and filter quality | R10 | `flui-painting`, `flui-engine`, `flui-objects`, `flui-widgets` | RC7, RC16 | yes | not started |
| RC19 | ADR: a paragraph lays out the boxes its spans embed | R13, R14 | `docs/adr` | — | | not started |
| RC20 | Span children are `InlineSpan`s; placeholders carry an id and shape as inline boxes | R13 | `flui-painting` and moved call sites | RC19 | yes | not started |
| RC21 | `RenderParagraph` inline children, the span builder, `Text::rich` and `RichText` | R13 | `flui-objects`, `flui-view`, `flui-widgets` | RC20, RC1 | | not started |
| RC22 | `RenderEditable` inline children; one position per placeholder in the editing value | R14 | `flui-objects`, `flui-widgets` | RC21 | | not started |
| RC23 | ADR: lazy children are requested by a typed key | R16 | `docs/adr` | — | | not started |
| RC24 | The request strategy, its element, retain set and pinned set take a key type | R16 | `flui-rendering`, `flui-view` | RC23 | yes | not started |
| RC25 | `RenderTwoAxisViewport`, the uniform-cell layout, `TwoAxisScrollView` | R16 | `flui-objects`, `flui-view`, `flui-widgets` | RC24, RC1 | | not started |
| RC26 | ADR: a sliver states a per-child paint transform, and the list wheel is a sliver | R15, R25 | `docs/adr` | RC23 | | not started |
| RC27 | `WheelGeometry`, `RenderSliverWheel` | R15 | `flui-objects`, `flui-view` (alias) | RC31, RC1 | | not started |
| RC28 | `ListWheelScrollView`, `WheelSnap`, the looping wheel | R15 | `flui-widgets` | RC27 | | not started |
| RC29 | ADR: a native view is a scene node the host places | R17 | `docs/adr`, `docs/gaps.md` | owner decision | | not started |
| RC30 | Native views, broken into tasks when RC29 is accepted | R17 | set by RC29 | RC29 | yes | not started |
| RC31 | The sliver per-child transform seam: the hook, the hit-test transform, a twice-painted child walked once | R15, R25 | `flui-rendering` | RC26 | yes | not started |
| RC32 | Tap regions, accessibility activations: classification by ancestry and the consuming stop | R12 | `flui-semantics`, `flui-runtime`, `flui-widgets` (tests) | RC15 | | not started |
| RC33 | `AnchoredPlacement`; the popup menu and the selection toolbar move onto it | R23 | `flui-widgets` | — | | not started |
| RC34 | `RenderFirstThatFits`, `FirstThatFits` | R24 | `flui-objects`, `flui-widgets` | RC1 | | not started |
| RC35 | The leading-run rule; `RenderWrap` with `max_runs` and an indicator; `Wrap` | R22 | `flui-objects`, `flui-widgets` | RC1 | | not started |
| RC36 | The `tracks` module; `TableColumnWidth` becomes `TrackSize`; `RenderTable` moves onto it | R21 | `flui-objects`, `flui-widgets` | — | | not started |
| RC37 | `GeometryObserver`, `VisibilityObserver` | R19 | `flui-widgets` | — | | not started |
| RC38 | Sticky rows in `RenderSliverList` and `RenderSliverVariedExtentList` | R20 | `flui-objects`, `flui-widgets` | RC8, RC1 | | not started |
| RC39 | `ItemEffect` on the four lazy slivers | R25 | `flui-objects`, `flui-widgets` | RC31, RC38 | | not started |
| RC40 | The group layer, `RenderBlendGroup`, `RenderMask`, `BlendGroup`, `Mask` | R26 | `flui-layer`, `flui-engine`, `flui-rendering`, `flui-objects`, `flui-widgets` | RC16, RC17 | yes | not started |
| RC41 | `RenderGrid`, `GridParentData`, `Grid`, `GridCell` | R21 | `flui-objects`, `flui-widgets` | RC36, RC1, RC0 | | not started |

## Order and parallelism

1. **RC0 first.** Every task that adds a harness row depends on it.
2. **Then, in parallel** (file sets are disjoint): RC1, RC2, RC5, RC6, RC7, RC8, RC13, RC33,
   RC36, RC37, and the ADR tasks RC14, RC16, RC19, RC23, which touch only `docs/adr/`.
3. **After RC1:** RC10, RC34, RC35 in parallel (RC35 owns `wrap.rs` after RC1 leaves it).
   **After RC1 and RC2:** RC3, then RC4. **After RC2:** RC12. **After RC10:** RC11.
   **After RC14:** RC15, then RC32. **After RC36 and RC1:** RC41.
4. **After RC8 and RC1:** RC9 and RC38 one after the other, in either order: both edit
   `virtualized_band.rs`.
5. **Gated, once the freeze lifts or an exception is written:** RC17, then RC18 and RC40;
   RC20, then RC21 and RC22; RC24, then RC25; RC26 is written after RC23 is accepted, then
   RC31, then RC27 and RC28, and RC39 after RC31 and RC38. RC17, RC24, RC31 and RC40 each edit
   `flui-rendering`, and no two of them run at once unless their files in it differ (RC17 and
   RC40 share `traits/paint_effects.rs`; RC24 is `context/layout.rs`; RC31 is
   `traits/render_sliver.rs`, the sliver hit-test context and two walks). RC20 and RC18 are
   `flui-painting` with different files.
6. **RC29 waits for the owner.**

Two files are edited by tasks that do not depend on each other and are rebased, not owned
twice: `crates/flui-widgets/src/navigator/page_route.rs` (a doc sentence in RC5, the popup's
group in RC15) and `crates/flui-widgets/src/popup_menu.rs` (the placement in RC33, passing the
opener's group in RC15). Whoever lands second rebases.

The cheapest tasks are RC2, RC6, RC7, RC33 and RC37, then RC4 and RC34; RC5 and RC37 are
`flui-widgets` alone.

## Tier A, ungated

### RC0. The harness reads its families from more than one file

- **What:** W1. The guard `catalog_covers_every_render_object_name` reads the root file and
  every listed family file, concatenated before it splits `fn harness_*` bodies; a family
  module that is declared and not listed fails it. No existing row moves. `docs/testing.md`
  links `RENDER_OBJECT_TYPES` instead of saying "81".
- **Owns:** `crates/flui-objects/tests/render_object_harness.rs` (the guard and the module
  list), `crates/flui-objects/tests/harness/` (new), the harness paragraph of
  `docs/testing.md`.
- **Tests:** the two guards pass; by hand, a row placed in a family module and then deleted
  makes `catalog_covers_every_render_object_name` fail (recorded in the pull request).
- **Checks:** clippy and nextest for `flui-objects`; the `checks` job's document steps (typos,
  lychee, unmounted-tests).
- **Depends:** — **Size:** S

### RC1. The shown answer

- **What:** W2 and the seam of [design.md](design.md), "The shown answer".
  `crates/flui-objects/src/shown.rs` with `ChildClip` and `ShownChildren`. `ChildClip` replaces
  `has_visual_overflow` in `RenderStack`, `RenderIndexedStack`, `RenderWrap`, `RenderFittedBox`,
  `RenderAnimatedSize` and `RenderConstraintsTransformBox`, each reporting it from
  `describe_approximate_paint_clip`. `RenderSliverCarousel` moves from its private `Shown`
  table to `ShownChildren`. The generic shared row is added to the harness. The gap entry "An
  overflow clip cuts off paint but not semantics or input regions" leaves `docs/gaps.md`.
  "Mapping decisions" records the contract and the hit rule of an unclipped overflow.
- **Owns:** `crates/flui-objects/src/shown.rs` (new), `layout/stack.rs`, `wrap.rs`,
  `fitted_box.rs`, `animated_size.rs`, `constraints_transform_box.rs`,
  `sliver/carousel/render.rs`; `crates/flui-objects/tests/harness/shown.rs` (new); that entry
  of `docs/gaps.md`.
- **Tests:** `harness_<object>_cuts_semantics_and_regions_at_its_overflow_clip` for the six
  objects (one table), `harness_overflow_answer_survives_a_layout_that_fails`,
  `harness_no_overflow_reports_no_clip`,
  `harness_an_unclipped_overflow_is_painted_and_published_but_hit_only_inside_the_box`,
  `harness_sliver_carousel_hits_and_publishes_a_child_only_where_it_paints`; the carousel's
  existing rows unchanged.
- **Checks:** clippy and nextest for `flui-objects`; nextest for `flui-widgets` (the objects'
  widgets have tests there).
- **Depends:** RC0 **Size:** M

### RC2. The sliver pass-through base

- **What:** W3, the pass-through half: `sliver/pass_through.rs`; the six pass-through slivers
  call the shared rules, the layout rule taking the child's constraints as a parameter. No
  behaviour change. `MaxPaintRect` is not built here: it is `pub(crate)` and its first caller
  is RC3, which brings it.
- **Owns:** the new file, `sliver_semantics.rs`, `sliver_ignore_pointer.rs`,
  `sliver_opacity.rs`, `sliver_offstage.rs`, `sliver_animated_opacity.rs`,
  `sliver_constrained_cross_axis.rs` under `crates/flui-objects/src/sliver/`.
- **Tests:** the existing rows of `family_sliver_wrappers` unchanged and green. The change is a
  refactoring, so its evidence is the mutation run over the two shared rules: each mutant is
  caught by an existing row, or the task adds the row that catches it
  (`harness_a_childless_sliver_proxy_has_zero_geometry`, one table over the six objects).
- **Checks:** clippy and nextest for `flui-objects`.
- **Depends:** RC0 **Size:** S

### RC3. Sliver clips

- **What:** W4 and the rectangle half of W3. `MaxPaintRect` (`sliver/max_paint_rect.rs`),
  `RenderSliverClip<S>` with its two aliases, `ClipOverlapBehavior`, widgets `SliverClipRect`
  and `SliverClipRRect`. The generic root joins the export guard's exclusion list.
- **Owns:** `crates/flui-objects/src/sliver/max_paint_rect.rs`, `sliver_clip.rs` (new),
  `crates/flui-objects/tests/harness/sliver_clip.rs` (new),
  `crates/flui-widgets/src/scroll/sliver_clip.rs` (new), its test module.
- **Tests:** `harness_sliver_clip_rect_follows_the_pinned_edge`,
  `harness_sliver_clip_rrect_preserves_its_shape_under_an_overlap`,
  `harness_sliver_clip_rejects_a_hit_outside_the_clip`,
  `harness_sliver_clip_of_an_unbounded_child_is_finite`,
  `harness_sliver_clip_under_a_narrowed_cross_axis_takes_the_narrow_extent`, each over the four
  axis directions; the readback row
  `adjacent_clipped_slivers_leave_no_seam_at_a_fractional_offset`; widget test
  `a_pinned_header_keeps_a_following_clip_at_its_edge` (with a persistent header; RC10 adds the
  child-sized one to the same table).
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC1, RC2 **Size:** M

### RC4. Decorated sliver

- **What:** W5.
- **Owns:** `crates/flui-objects/src/sliver/sliver_decorated.rs` (new),
  `crates/flui-objects/tests/harness/sliver_decorated.rs` (new),
  `crates/flui-widgets/src/scroll/decorated_sliver.rs` (new).
- **Tests:** `harness_decorated_sliver_covers_its_whole_scroll_extent`,
  `harness_decorated_sliver_over_an_unbounded_list_is_finite`,
  `harness_decorated_sliver_paints_nothing_when_its_child_is_invisible`,
  `harness_decorated_sliver_rounds_its_real_ends_not_the_visible_slice`,
  `harness_decorated_sliver_around_a_pinned_child_follows_it`; widget test
  `a_decorated_slivers_shadow_is_cut_by_the_viewport_clip`; RC3's readback row gains the
  two-decorated-slivers case.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC3 (for `MaxPaintRect`) **Size:** S

### RC5. `BlockSemantics` and `SliverVisibility`

- **What:** W6, the two compositions. The three documents that say FLUI has no
  `BlockSemantics` are corrected.
- **Owns:** `crates/flui-widgets/src/semantics/block_semantics.rs` (new),
  `crates/flui-widgets/src/scroll/sliver_visibility.rs` (new), their test modules; the doc
  comments in `crates/flui-widgets/src/navigator/page_route.rs`,
  `crates/flui-widgets/tests/modal_route.rs`, `packages/flui-material/src/drawer.rs`.
- **Tests:** `block_semantics_hides_what_was_painted_before_it`,
  `removing_block_semantics_restores_earlier_siblings_without_an_announcement`,
  `a_hidden_sliver_takes_no_extent_and_no_hits`,
  `a_hidden_sliver_that_maintains_size_keeps_its_extent`,
  `a_hidden_sliver_that_maintains_semantics_is_still_published`.
- **Checks:** clippy and nextest for `flui-widgets`; clippy for `flui-material` (a doc comment
  changed).
- **Depends:** — **Size:** S

### RC6. Overflow bar

- **What:** W6, `RenderOverflowBar` and `OverflowBar`.
- **Owns:** `crates/flui-objects/src/layout/overflow_bar.rs` (new),
  `crates/flui-objects/tests/harness/overflow_bar.rs` (new),
  `crates/flui-widgets/src/layout/overflow_bar.rs` (new) and its test module.
- **Tests:** `harness_overflow_bar_wraps_to_a_column_when_its_row_does_not_fit`,
  `harness_overflow_bar_intrinsics_match_its_layout`,
  `harness_overflow_bar_orders_its_column_by_direction`,
  `harness_overflow_bar_mirrors_its_row_in_rtl`,
  `an_expanded_child_of_an_overflow_bar_is_refused`.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC0 **Size:** S

### RC7. Image colour and opacity

- **What:** W7, the ungated half: `color`, `color_blend_mode`, `opacity` on `RenderImage` and
  `Image`. The first step runs the centre-slice case of #27827 and records the result under
  "Validation evidence".
- **Owns:** `crates/flui-objects/src/image/render_image.rs`,
  `crates/flui-widgets/src/image/image.rs`, the image rows of the harness (a new
  `tests/harness/image_paint.rs`).
- **Tests:** `an_image_is_tinted_by_its_colour_and_blend_mode`,
  `an_image_is_faded_by_its_opacity`, `an_image_of_zero_opacity_paints_nothing`,
  `a_tinted_image_keeps_its_transparent_pixels_transparent`,
  `a_fully_transparent_tint_with_src_in_hides_the_image`,
  `harness_image_intrinsics_are_finite_before_the_image_resolves`; the readback samples are
  chosen so the unset property gives a different pixel.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`; the `image` test
  targets of `flui-widgets`.
- **Depends:** RC0 **Size:** S

### RC8. Varied-extent list

- **What:** W8. `ItemExtentDelegate`, `ItemExtentDimensions`,
  `RenderSliverVariedExtentList` over `Virtualizer` and `walk_virtualizer_band`, with
  `set_placement`; the alias in `flui-view`; `SliverVariedExtentList` and the `ListView`
  constructor. The unbounded-window constant moves from `sliver_grid.rs` to
  `virtualized_band.rs`. "Mapping decisions" records that the dimensions omit the scroll
  offset and the three count cases.
- **Owns:** `crates/flui-objects/src/sliver/item_extent.rs`, `sliver_varied_extent_list.rs`
  (new), `virtualized_band.rs`; the constant's definition in `sliver_grid.rs` and its import in
  `sliver_fixed_extent_list.rs`; `crates/flui-objects/tests/harness/varied_extent.rs` (new);
  `crates/flui-widgets/src/scroll/sliver_varied_extent_list.rs` (new), `list_view.rs`.
- **Tests:** `harness_varied_extent_list_places_items_at_the_sum_of_their_extents`,
  `harness_varied_extent_list_asks_the_delegate_once_per_item_per_dimensions`,
  `harness_varied_extent_list_never_asks_for_an_index_at_or_past_its_count`,
  `harness_varied_extent_list_lays_an_item_out_at_exactly_its_extent`,
  `harness_varied_extent_list_refuses_a_non_finite_extent`,
  `harness_varied_extent_list_of_zero_extents_builds_a_bounded_window`,
  `harness_varied_extent_list_with_an_endless_source_asks_only_its_window`,
  `harness_varied_extent_list_restores_a_deep_offset_without_walking_from_zero`; the existing
  list, grid and fixed-extent rows unchanged; a widget test
  `a_varied_extent_list_view_scrolls_to_an_exact_offset`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC0 **Size:** L

### RC9. Tree sliver

- **What:** W9. `TreeRowDelegate`, `TreeIndentation`, `TreeReveal`, the reveal map,
  `RenderTreeSliver`; `TreeSliver` with its node source, controller and roster. "Mapping
  decisions" records the departures from Flutter.
- **Owns:** `crates/flui-objects/src/sliver/sliver_tree/` (new),
  `crates/flui-objects/tests/harness/tree_sliver.rs` (new),
  `crates/flui-widgets/src/scroll/tree_sliver/` (new) and its test module; what the walk needs
  in `virtualized_band.rs` to map the window through the reveal map.
- **Tests:** R6's seven harness rows and nine widget tests, as named there.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC8, RC1; not at the same time as RC38 **Size:** L

### RC10. Pinned and resizing child-sized headers

- **What:** W10, `RenderPinnedHeaderSliver`, `RenderSliverResizingHeader`, `HeaderFrame`,
  widgets `PinnedHeaderSliver` and `SliverResizingHeader`. The group rows of R7, over the new
  headers and the existing pinned one; a failing row is fixed in the group.
- **Owns:** `crates/flui-objects/src/sliver/child_sized_header.rs` (new),
  `crates/flui-objects/tests/harness/child_sized_header.rs` (new),
  `crates/flui-widgets/src/scroll/pinned_header_sliver.rs`, `sliver_resizing_header.rs` (new)
  and their test modules; `crates/flui-objects/src/sliver/sliver_main_axis_group.rs` and
  `sliver_cross_axis_group.rs` only if a group row fails.
- **Tests:** `harness_pinned_header_takes_its_extent_from_its_child`,
  `harness_pinned_header_taller_than_the_viewport_stays_finite`,
  `harness_resizing_header_never_paints_hits_or_publishes_its_prototypes`,
  `harness_resizing_header_reports_its_minimum_as_obstruction`,
  `harness_resizing_header_without_a_maximum_prototype_takes_its_childs_intrinsic_extent`,
  `harness_child_sized_header_publishes_its_extents_and_overlap`,
  `harness_a_pinned_header_in_a_group_leaves_with_its_group`,
  `harness_two_pinned_headers_in_one_group_stack`,
  `harness_a_pinned_header_in_a_cross_axis_group_sits_below_a_preceding_pinned_sliver`; widget
  tests `a_pinned_header_is_read_where_it_is_painted` and the child-sized case of
  `a_pinned_header_keeps_a_following_clip_at_its_edge` (when RC3 has landed).
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC1 **Size:** M

### RC11. Floating child-sized header

- **What:** W10, `RenderSliverFloatingHeader` and `SliverFloatingHeader`. The floating state
  machine moves out of `RenderSliverFloatingHeaderBase` into a private type in a file of its
  own, owned by both header families; public names do not change.
- **Owns:** `crates/flui-objects/src/sliver/sliver_persistent_header.rs`, a new
  `floating_header_state.rs` beside it, `child_sized_header.rs` (after RC10),
  `crates/flui-widgets/src/scroll/sliver_floating_header.rs` (new) and its test module.
- **Tests:** `harness_floating_header_returns_by_the_distance_scrolled_back`,
  `harness_floating_header_returns_under_a_preceding_pinned_sliver` (both scroll directions,
  four axis directions), `harness_floating_header_snaps_open_when_released_past_half`; widget
  test `a_floating_header_returns_on_a_scroll_of_its_nested_body`; the rows of
  `family_persistent_header` unchanged.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC10 **Size:** M

### RC12. Sliver layout builder and fractional padding

- **What:** W11. `SliverConstraintsCell`, `SliverLayoutDimensions`,
  `RenderSliverLayoutBuilder`, the sliver twin of the layout-builder element,
  `SliverLayoutBuilder` with its two constructors; `RenderSliverFractionalPadding`,
  `SliverFractionalPadding`, `PageView::pad_ends` and the page arithmetic at the ends.
- **Owns:** `crates/flui-objects/src/sliver/sliver_layout_builder.rs`,
  `sliver_fractional_padding.rs` (new), a new cell file beside
  `crates/flui-objects/src/layout/layout_constraints_cell.rs`, a new element file beside
  `crates/flui-view/src/element/layout_builder.rs`,
  `crates/flui-widgets/src/scroll/sliver_layout_builder.rs`, `sliver_fractional_padding.rs`
  (new), `page_view.rs`, `crates/flui-objects/tests/harness/sliver_layout_builder.rs` (new).
- **Tests:** `a_sliver_layout_builder_rebuilds_only_when_its_dimensions_change`,
  `a_sliver_layout_builder_of_constraints_rebuilds_on_a_scroll`,
  `a_sliver_layout_builder_adopts_its_childs_geometry`,
  `a_rebuild_requested_during_layout_is_delivered_and_the_next_one_too` (one table over both
  builders), `a_padded_page_view_centres_its_first_and_last_page`,
  `an_unpadded_page_view_leaves_no_blank_space`, `an_unpadded_page_view_reaches_its_last_page`,
  `a_page_overlapping_the_viewport_edge_is_hit_where_it_is_painted`;
  `harness_sliver_fractional_padding_pads_by_the_viewport_fraction`,
  `harness_sliver_constraints_report_the_logical_scroll_direction`,
  `harness_sliver_layout_builder_passes_its_dimensions_to_its_cell`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC2 **Size:** M

### RC13. Records for what stays covered by a different design

- **What:** W18. Six entries in "Mapping decisions" of `crates/flui-objects/ARCHITECTURE.md`,
  from the table "Covered by a different design" of [design.md](design.md), each citing the
  tests that hold the FLUI form. For the nested-scroll entry: each of R18's six behaviours gets
  its ADR-0209 section and a test by name, a new test where the behaviour holds and none names
  it, or an entry in `docs/gaps.md`.
- **Owns:** those six paragraphs; new cases in the nested-scroll test module of
  `crates/flui-widgets/tests/`; the gap entries it adds.
- **Tests:** the ones it adds; every test it cites is found by `rg` under its name.
- **Checks:** the `checks` job's document steps; nextest for `flui-widgets` if a test is added.
- **Depends:** — **Size:** S

### RC14. ADR: tap classification

- **What:** the ADR of W12, in the shape of `docs/adr/README.md`, with the options of
  [design.md](design.md): the typed event and its phases and origins, the registry in the
  binding and the members cached on the contact's route, the capability, consuming through the
  arena, liveness without a navigator observer, the activation route through `flui-runtime`
  and `flui-semantics` with its stated limit, and whether a down that stops a fling is a tap.
  Its first step settles the two W12 points of [design.md](design.md), "Unverified".
- **Owns:** one new file in `docs/adr/`.
- **Checks:** the `checks` job's document steps, the ADR-number step; a design read.
- **Depends:** — **Size:** M

### RC15. Tap regions, the pointer route

- **What:** W12 as RC14 decides it, without the accessibility route: `TapRegionEvent`,
  `TapPhase`, `TapOrigin`, `TapRegionRegistry` and its ids; the members collected on the down
  and cached on `CachedPointerRoute`; the arena claim; the `tap_regions()` capability and its
  wiring; `RenderTapRegion`; `TapRegion`; `PopupRoute::tap_region_group`.
- **Owns:** a new module `crates/flui-interaction/src/pointer/tap_region.rs`, the collection in
  `crates/flui-interaction/src/binding/mod.rs`; the capability in
  `crates/flui-view/src/context/build_context.rs` and its implementations; the wiring in
  `crates/flui-runtime/src/presentation/assembly.rs`;
  `crates/flui-objects/src/interaction/tap_region.rs` (new),
  `crates/flui-objects/tests/harness/tap_region.rs` (new);
  `crates/flui-widgets/src/interaction/tap_region.rs` (new) and its test module; the builder on
  `PopupRoute` in `crates/flui-widgets/src/navigator/page_route.rs` and its use in
  `popup_menu.rs`.
- **Tests:** `a_tap_on_an_overlay_above_a_region_is_outside_it`,
  `a_tap_inside_a_group_member_is_inside_every_member`,
  `a_tap_inside_a_popup_is_inside_its_opener`,
  `a_tap_on_an_empty_part_of_a_region_is_inside_it`,
  `a_tap_on_a_second_field_is_outside_the_first`,
  `a_drag_that_starts_outside_reports_down_and_up_but_no_tap`,
  `a_secondary_button_outside_is_reported_with_its_button`,
  `a_consumed_outside_tap_reaches_no_detector`,
  `a_region_on_a_covered_page_hears_no_outside_tap`,
  `a_region_on_a_branch_that_is_not_shown_hears_no_outside_tap`,
  `an_up_is_classified_against_the_path_of_its_down`,
  `a_panicking_outside_callback_leaves_the_registry_usable`; a harness row
  `harness_tap_region_marks_its_hit_entry_and_joins_the_hits_below`.
- **Checks:** clippy and nextest for `flui-interaction`, `flui-objects`, `flui-view`,
  `flui-runtime`, `flui-widgets`.
- **Depends:** RC14, RC0 **Size:** L

### RC32. Tap regions, accessibility activations

- **What:** W12's activation route: the two accessors on `SemanticsActionInvocation`; in
  `dispatch_semantics_action`, the walk of the target's render ancestors, one
  `TapRegionEvent` with `TapOrigin::Semantics`, and no `invoke()` when a consuming region was
  outside.
- **Owns:** `crates/flui-semantics/src/owner.rs` (the accessors),
  `crates/flui-runtime/src/presentation/accessibility.rs`; new cases in the tap-region test
  module of `flui-widgets` and in `flui-runtime`'s tests.
- **Tests:** `a_screen_reader_activation_is_classified_by_ancestry`,
  `a_screen_reader_activation_is_classified_once`,
  `a_consumed_outside_activation_does_not_run_its_action`.
- **Checks:** clippy and nextest for `flui-semantics`, `flui-runtime`, `flui-widgets`.
- **Depends:** RC15 **Size:** M

## Tier A, gated or waiting on a gated task

### RC16. ADR: filters and group blend as paint effects

- **What:** the ADR of W13 and W26: field or scope; the order opacity, group blend, filter,
  clip, transform; a filter as a function of the size; bounds and rounding; the group layer
  for blend and mask, with its measured cost; what a filter over a native view does until W17
  decides.
- **Owns:** one new file in `docs/adr/`. **Depends:** — **Size:** M

### RC17. Subtree filters — gated

- **What:** W13 as RC16 decides it; closes the gap "No widget filters its subtree's colours or
  pixels".
- **Frozen-crate files:** `crates/flui-rendering/src/traits/paint_effects.rs` and the place
  effects are lowered to the scene (or `context/paint.rs` if RC16 chooses scopes); readback
  rows in `flui-engine`'s tests.
- **Owns:** those, `crates/flui-objects/src/proxy/color_filtered.rs`, `image_filtered.rs`
  (new), `crates/flui-objects/tests/harness/filters.rs` (new),
  `crates/flui-widgets/src/paint/color_filtered.rs`, `image_filtered.rs` (new), the gap entry.
- **Tests:** a readback row per filter kind, `a_filtered_subtree_is_hit_where_it_is_laid_out`,
  `a_filtered_subtree_at_an_offset_paints_at_that_offset`,
  `a_blurred_subtree_at_a_fractional_scroll_offset_repaints_without_an_edge`,
  `a_disabled_image_filter_pushes_no_layer`,
  `a_filter_change_does_not_repaint_the_child` (if RC16 chooses the effect field).
- **Checks:** clippy and nextest for `flui-rendering`, `flui-objects`, `flui-widgets`,
  `flui-engine` (readback suites at one test thread).
- **Depends:** RC16, RC0 **Size:** M

### RC18. Image inversion and filter quality — gated

- **What:** W7, the gated half.
- **Frozen-crate files:** `crates/flui-painting/src/paint/image.rs` (a composed
  `ColorFilter`), `display_list/payload.rs` (`filter_quality` on `ImageRegionPlacement`),
  `image_placement.rs`; the image sampler in `flui-engine`.
- **Owns:** those and `render_image.rs`, `image.rs` after RC7.
- **Tests:** `an_inverted_tinted_image_inverts_after_the_tint`,
  `an_upscaled_image_is_sampled_by_its_filter_quality`; the size assertions on `DrawOp` and
  `RecordedOp` unchanged.
- **Checks:** clippy and nextest for `flui-painting`, `flui-engine`, `flui-objects`,
  `flui-widgets`.
- **Depends:** RC7, RC16 **Size:** M

### RC19. ADR: inline boxes

- **What:** the ADR of W14: the span tree, the placeholder's identity and its two size forms,
  where measurement sits, intrinsics and dry layout, no implicit scaling, the semantics order
  and label, one U+FFFC per placeholder in the editing value. Its first step locates the
  editing value's type and offset unit ([design.md](design.md), "Unverified").
- **Owns:** one new file in `docs/adr/`. **Depends:** — **Size:** M

### RC20. Span children and inline-box shaping — gated

- **What:** W14 in `flui-painting`: `TextSpan.children` becomes `Vec<InlineSpan>`;
  `PlaceholderId` and the two size forms of `PlaceholderSpan`; a placeholder is an inline box
  to Parley and one position in the text; `TextPainter` takes placeholder dimensions by id and
  returns box rectangles by id, and answers a missing dimension with a typed error. Every user
  of the old child type and of `PlaceholderSpan`'s fields in the workspace moves in this
  change.
- **Frozen-crate files:** `crates/flui-painting/src/typography/text_spans.rs`,
  `text_painter/measure.rs` and the painter's layout entry, `parley_text/shape.rs`.
- **Owns:** those, and each call site the compiler names (expected in `flui-objects/src/text/`
  and `flui-widgets/src/text/`).
- **Tests:** in `crates/flui-painting/tests/`: `a_placeholder_takes_one_text_position`,
  `a_nested_placeholder_reserves_its_box_on_the_line`,
  `a_truncated_placeholder_has_no_box_and_a_later_one_keeps_its_own`,
  `a_baseline_placeholder_sits_on_the_baseline`,
  `placeholders_in_right_to_left_text_are_ordered_with_their_runs`,
  `a_justified_line_moves_its_placeholders`,
  `a_painter_without_a_placeholders_dimensions_returns_an_error`,
  `text_without_placeholders_shapes_as_before` (existing snapshots unchanged; a changed
  snapshot is justified in the pull request or is the defect).
- **Checks:** clippy and nextest for `flui-painting` and every crate with a moved call site;
  `cargo heli test --doc -p flui-painting`.
- **Depends:** RC19 **Size:** L

### RC21. Paragraph inline children

- **What:** W14 in `flui-objects` and `flui-widgets`: `InlineBoxes` over `ShownChildren`,
  `RenderParagraph` with `Arity = Variable`, the span builder, the multi-child element for
  `RichText`, `Text::rich`.
- **Owns:** `crates/flui-objects/src/text/inline_boxes.rs` (new), `paragraph.rs`,
  `crates/flui-objects/tests/harness/inline_children.rs` (new),
  `crates/flui-widgets/src/text/` (the span builder, `rich_text.rs`, `text.rs`), an element in
  `flui-view` if the multi-child render view needs one.
- **Tests:** R13's nine harness rows and five widget tests, as named there.
- **Checks:** clippy and nextest for `flui-objects`, `flui-widgets`, `flui-view`.
- **Depends:** RC20, RC1 **Size:** L

### RC22. Editable inline children

- **What:** W14 for `RenderEditable`, on `InlineBoxes`: the caret, selection and deletion rules
  of R14, and the typed refusal of a span tree whose text differs from the editing value.
- **Owns:** `crates/flui-objects/src/text/editable.rs` and `text/editable/`, its harness rows;
  the editing controller's check in `crates/flui-widgets/src/text/`.
- **Tests:** R14's eight rows, as named there.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets` (the editable-text
  suites).
- **Depends:** RC21 **Size:** M

### RC23. ADR: typed lazy key

- **What:** the ADR of W16: the request strategy, retain set and pinned set generic over a
  key; one owner of the key-to-child mapping, with the swap and the keep-alive removal; the
  three options.
- **Owns:** one new file in `docs/adr/`. **Depends:** — **Size:** M

### RC24. The request strategy takes a key type — gated

- **What:** W16's contract. The lazy slivers keep `usize` and their behaviour; the change is
  proven by their existing rows staying green.
- **Frozen-crate files:** `crates/flui-rendering/src/context/layout.rs` (the request and
  retain entries) and the erased context behind them.
- **Owns:** those and `crates/flui-view/src/element/sliver_adaptor.rs`.
- **Tests:** the rows of `family_sliver_lists` and the lazy-list widget suites unchanged;
  in `flui-rendering`'s tests, `a_keyed_request_builds_the_child_for_its_key`,
  `a_retain_set_drops_only_the_children_outside_it`,
  `two_keyed_children_moved_onto_each_other_are_swapped`.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-view`, `flui-objects`,
  `flui-widgets`.
- **Depends:** RC23 **Size:** L

### RC25. Two-axis viewport

- **What:** W16: `Vicinity`, `TwoAxisLayout`, `RenderTwoAxisViewport<L>`, the uniform-cell
  layout with pinned rows and columns, `TwoAxisScrollView`. The first step of the widget is the
  test that begins one contact on two scroll owners.
- **Owns:** `crates/flui-objects/src/sliver/viewport/two_axis.rs` (new),
  `crates/flui-objects/tests/harness/two_axis_viewport.rs` (new), a keyed element in
  `flui-view`, `crates/flui-widgets/src/scroll/two_axis_scroll_view.rs` (new) and its test
  module.
- **Tests:** R16's ten harness rows and four widget tests, as named there.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC24, RC1 **Size:** L

### RC26. ADR: a sliver's per-child transform, and the wheel

- **What:** the ADR of W15 and of the seam of [design.md](design.md), written after RC23 is
  accepted, because a wheel keyed by a signed index on the typed-key seam is one of its
  options. It works the reverse group's arithmetic for the looping wheel and names the
  fallback.
- **Owns:** one new file in `docs/adr/`. **Depends:** RC23 **Size:** M

### RC31. The sliver per-child transform seam — gated

- **What:** [design.md](design.md), "A sliver states a per-child paint transform":
  `RenderSliver::apply_paint_transform` and its blanket forwarding; the sliver hit-test
  context's transform reaching the driver; the input-region and presented-node walks visiting
  a twice-painted child once.
- **Frozen-crate files:** `crates/flui-rendering/src/traits/render_sliver.rs` and the blanket
  forwarding to `RenderObject`; `protocol/sliver_protocol.rs` (`push_transform`,
  `pop_transform`); `pipeline/owner/input_regions.rs`, `presented.rs`.
- **Owns:** those and their tests in `flui-rendering`.
- **Tests:** `a_sliver_without_the_hook_maps_its_children_as_before`,
  `a_hit_under_a_transformed_sliver_child_carries_its_transform`,
  `a_child_painted_twice_reports_its_regions_once`; every sliver row of the harness unchanged.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-objects`, `flui-widgets`.
- **Depends:** RC26. The third part is a defect fix and may land alone before the freeze lifts.
  **Size:** M

### RC27. The wheel sliver

- **What:** W15: `WheelGeometry` and its parameter newtypes, `RenderSliverWheel` on
  `ShownChildren` and the seam.
- **Owns:** `crates/flui-objects/src/sliver/wheel/` (new),
  `crates/flui-objects/tests/harness/wheel.rs` (new).
- **Tests:** R15's nine harness rows, as named there.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`.
- **Depends:** RC31, RC1 **Size:** L

### RC28. The wheel widget

- **What:** `ListWheelScrollView`, its controller, `WheelSnap`, the mouse-wheel step, the
  looping wheel in the form RC26 decides.
- **Owns:** `crates/flui-widgets/src/scroll/list_wheel/` (new) and its test module.
- **Tests:** `a_looping_wheel_scrolls_past_its_last_item_to_its_first`,
  `a_wheel_settles_on_an_item`, `a_wheel_reports_its_selected_item_once_per_change`,
  `a_wheel_child_reads_the_selected_item_in_its_first_build`,
  `one_notch_of_a_mouse_wheel_moves_one_item`, `a_tap_on_a_wheel_item_reaches_its_detector`.
- **Checks:** clippy and nextest for `flui-widgets`.
- **Depends:** RC27 **Size:** M

### RC29. ADR: native views

- **What:** the ADR of W17, after the owner decides that native views enter the contract and
  for which hosts. It answers each row of R17's table.
- **Owns:** one new file in `docs/adr/`; the gap entry "No native views inside the UI" is
  rewritten to point at it. **Depends:** owner decision **Size:** L

### RC30. Native views — gated

- **What:** not broken down. When RC29 is accepted this row is replaced by its tasks, the
  texture-backed path first; runs owed on devices become rows of
  `crates/flui-native/platform-parity.toml`.
- **Depends:** RC29

## Tier B, beyond Flutter

### RC33. Anchored placement

- **What:** W23. `AnchoredPlacement`, `PlacementSide`, `Placed`, the one layout delegate over
  them. `MenuPlacement` and `ToolbarLayout` are deleted; the popup menu and the selection
  toolbar configure the delegate.
- **Owns:** `crates/flui-widgets/src/overlay/placement.rs` (new) and its test module; the
  placement code of `crates/flui-widgets/src/popup_menu.rs` and
  `crates/flui-widgets/src/text/selection_menu/toolbar.rs`.
- **Tests:** `a_placement_takes_its_preferred_side_when_it_fits`,
  `a_placement_falls_back_in_order`, `a_placement_shifts_to_stay_inside`,
  `a_placement_does_not_flip_back_and_forth_at_the_threshold`, `a_placement_mirrors_in_rtl`,
  `a_placement_of_a_child_larger_than_the_area_is_finite`; the popup menu's and the selection
  toolbar's placement tests unchanged. `rg 'MenuPlacement|ToolbarLayout'` finds nothing.
- **Checks:** clippy and nextest for `flui-widgets`; clippy and nextest for
  `packages/flui-material` and `packages/flui-cupertino` (users of the menu and the toolbar).
- **Depends:** — **Size:** M

### RC34. First that fits

- **What:** W24. `RenderFirstThatFits`, `FitAxes`, `FirstThatFits`. The first step settles
  whether a mounted child may be skipped by a layout ([design.md](design.md), "Unverified").
- **Owns:** `crates/flui-objects/src/layout/first_that_fits.rs` (new),
  `crates/flui-objects/tests/harness/first_that_fits.rs` (new),
  `crates/flui-widgets/src/layout/first_that_fits.rs` (new).
- **Tests:** `harness_first_that_fits_shows_the_first_alternative_that_fits`,
  `harness_first_that_fits_falls_back_to_its_last_alternative`,
  `harness_first_that_fits_hides_the_others_from_hits_and_semantics`,
  `harness_first_that_fits_on_one_axis_ignores_the_other`,
  `harness_first_that_fits_dry_layout_makes_the_same_choice`.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC1 **Size:** S

### RC35. A wrap that stops

- **What:** W22. `layout/leading_run.rs`; `RenderToolbarItems` calls it; `RenderWrap` gains
  `max_runs`, the indicator and `WrapSplit`; `Wrap` gains its two builder methods. The sentence
  in `toolbar_items.rs` about what a wrap cannot do is rewritten.
- **Owns:** `crates/flui-objects/src/layout/leading_run.rs` (new), `wrap.rs` (after RC1),
  `toolbar_items.rs`; `crates/flui-objects/tests/harness/wrap_overflow.rs` (new);
  `crates/flui-widgets/src/wrap/`.
- **Tests:** `harness_wrap_stops_after_its_maximum_runs`,
  `harness_wrap_places_its_indicator_at_the_end_of_the_last_run`,
  `harness_wrap_hides_its_indicator_when_everything_fits`,
  `harness_wrap_reports_the_number_of_hidden_children`,
  `harness_wrap_split_is_stable_when_the_indicator_changes_width`,
  `harness_wrap_hidden_children_are_not_hit_or_published`,
  `harness_wrap_without_a_maximum_lays_out_as_before`; the rows of `RenderToolbarItems`
  unchanged.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC1 **Size:** M

### RC36. Tracks

- **What:** W21, the replacement: `layout/tracks.rs` with `TrackSize` and `resolve`.
  `TableColumnWidth` is deleted and every user moves to `TrackSize` (`rg 'TableColumnWidth'`
  across `crates/`, `packages/`, `docs/` and each `ARCHITECTURE.md` finds nothing);
  `RenderTable` calls `resolve`. No behaviour change.
- **Owns:** `crates/flui-objects/src/layout/tracks.rs` (new), `table_column.rs` (deleted),
  `table.rs`; `crates/flui-widgets/src/layout/table.rs`; each document that names the old
  type.
- **Tests:** the rows of `RenderTable` and the `Table` widget tests unchanged; the mutation run
  over `resolve` is caught by them, or the task adds the row that catches it.
- **Checks:** clippy and nextest for `flui-objects`, `flui-widgets` and each crate the rename
  reaches.
- **Depends:** — **Size:** M

### RC41. Grid

- **What:** W21. `RenderGrid`, `GridParentData`, `Grid`, `GridCell`. The first step defines the
  parent-data type in `flui-objects` and mounts one child ([design.md](design.md),
  "Unverified"); if that cannot work the task stops and is marked gated here.
- **Owns:** `crates/flui-objects/src/layout/grid.rs` (new),
  `crates/flui-objects/tests/harness/grid.rs` (new),
  `crates/flui-widgets/src/layout/grid.rs` (new) and its test module.
- **Tests:** R21's nine rows, as named there.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC36, RC1, RC0 **Size:** L

### RC37. Observers

- **What:** W19. `GeometryObserver`, `VisibilityObserver`, `ObservedGeometry`,
  `ObservedVisibility`.
- **Owns:** `crates/flui-widgets/src/observe/` (new) and its test module.
- **Tests:** `a_geometry_observer_reports_a_size_change_once`,
  `a_geometry_observer_reports_nothing_when_nothing_changed`,
  `a_visibility_observer_reports_the_fraction_a_viewport_leaves`,
  `a_visibility_observer_reports_zero_for_an_offstage_subtree`,
  `a_visibility_observer_under_a_rotation_reports_its_bounding_fraction`,
  `an_observer_that_changes_nothing_requests_no_frame`,
  `an_observer_callback_that_resizes_its_subtree_settles`,
  `a_panicking_observer_leaves_the_others_reporting`.
- **Checks:** clippy and nextest for `flui-widgets`.
- **Depends:** — **Size:** M

### RC38. Sticky rows

- **What:** W20. `StickyRows`, `sliver/sticky_rows.rs`, the mode on `RenderSliverList` and
  `RenderSliverVariedExtentList`, `.sticky_rows(..)` on their widgets and on `ListView`.
- **Owns:** `crates/flui-objects/src/sliver/sticky_rows.rs` (new), `sliver_list.rs`,
  `sliver_varied_extent_list.rs` (after RC8), `virtualized_band.rs`;
  `crates/flui-objects/tests/harness/sticky_rows.rs` (new);
  `crates/flui-widgets/src/scroll/sliver_list.rs`, `sliver_varied_extent_list.rs`,
  `list_view.rs`.
- **Tests:** `harness_sticky_row_stays_at_the_leading_edge_within_its_section`,
  `harness_sticky_row_is_pushed_off_by_the_next_header`,
  `harness_sticky_row_is_hit_before_the_row_it_covers`,
  `harness_sticky_row_reports_its_extent_as_obstruction`,
  `harness_sticky_row_works_in_all_four_axis_directions`; widget tests
  `revealing_a_row_stops_below_the_sticky_header`,
  `a_sticky_header_is_read_before_its_sections_rows`,
  `a_list_of_ten_thousand_sections_builds_only_its_window`.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC8, RC1; not at the same time as RC9 **Size:** M

### RC39. Item effects

- **What:** W25. `ItemEffect`, `ItemProgress`, `ItemAppearance`, `sliver/item_effect.rs`; the
  optional effect on the four lazy slivers and `.item_effect(..)` on their widgets.
- **Owns:** `crates/flui-objects/src/sliver/item_effect.rs` (new), the effect's field and call
  in `sliver_list.rs`, `sliver_fixed_extent_list.rs`, `sliver_varied_extent_list.rs`,
  `sliver_grid.rs`; `crates/flui-objects/tests/harness/item_effect.rs` (new); the builder
  methods in `crates/flui-widgets/src/scroll/`.
- **Tests:** `harness_item_effect_scales_an_item_at_the_viewport_edge`,
  `harness_item_effect_is_hit_through_its_transform`,
  `harness_item_effect_leaves_the_scroll_extent_unchanged`,
  `harness_item_effect_with_a_non_finite_transform_hides_the_item`,
  `harness_a_sliver_without_an_effect_maps_its_children_as_before`; widget test
  `scrolling_a_list_with_an_item_effect_rebuilds_nothing`.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC31, RC38 **Size:** M

### RC40. Blend group and mask — gated

- **What:** W26 as RC16 decides it: the group layer, its lowering, the blend field of
  `PaintEffects`, `RenderBlendGroup`, `RenderMask`, `BlendGroup`, `Mask`.
- **Frozen-crate files:** a new layer file in `crates/flui-layer/src/` and its registration;
  the lowering in `flui-engine`'s layer renderer; `crates/flui-rendering/src/traits/
  paint_effects.rs` and where effects are lowered to the scene.
- **Owns:** those, `crates/flui-objects/src/proxy/blend_group.rs`, `mask.rs` (new),
  `crates/flui-objects/tests/harness/blend_group.rs` (new),
  `crates/flui-widgets/src/paint/blend_group.rs`, `mask.rs` (new).
- **Tests:** `a_blend_group_blends_as_a_whole_with_what_is_behind_it`,
  `children_of_a_blend_group_do_not_blend_with_the_backdrop`,
  `a_mask_multiplies_its_content_by_the_masks_alpha`,
  `a_mask_at_a_fractional_offset_leaves_no_edge`, `a_masked_subtree_is_hit_where_its_content_is`,
  `a_mask_child_is_not_hit_or_published`; a perf scenario held by
  `flui_testing::perf::Baselines` for one group.
- **Checks:** clippy and nextest for `flui-layer`, `flui-engine`, `flui-rendering`,
  `flui-objects`, `flui-widgets` (readback suites at one test thread).
- **Depends:** RC16, RC17 **Size:** L

## Validation evidence

Empty until a task lands. Each task records here the final names of its tests, the commands it
ran with their results, and the pull request.
