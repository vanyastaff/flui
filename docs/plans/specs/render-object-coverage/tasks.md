# render-object-coverage: tasks

- **Status:** Not started — awaiting owner approval of requirements and design
- **Date:** 2026-11-25
- **Base:** `claude/project-thread-03lo7m` @ `fcab477`
- **Design:** [design.md](design.md) (work items W1–W18); [requirements.md](requirements.md)
  (R1–R18, gates G1–G3).

## Execution rules

- **One task, one branch, one pull request, one writer per file set** (AGENTS.md, "Working
  here"). "Owns" below is the file set; a task that finds it needs a file another open task
  owns stops and says so here.
- **Shared one-line files.** A task that adds a render object or a widget adds one line to
  each of: `crates/flui-objects/src/lib.rs` and the family's `mod.rs`; the `RENDER_OBJECT_TYPES`
  list and, for a new family, the `mod` line, guard entry and `family_*` test in
  `crates/flui-objects/tests/render_object_harness.rs`; `crates/flui-widgets/src/lib.rs`, the
  family's `mod.rs` and `crates/flui-widgets/tests/main.rs`. An entry in "Mapping decisions" of
  `crates/flui-objects/ARCHITECTURE.md` is its own paragraph. These edits are not ownership;
  they are rebased mechanically.
- **Checks of the crates touched, in the foreground, one at a time** (builds queue through
  heli): `cargo fmt --all`, then for each crate `cargo heli clippy -p <crate> --all-targets --
  -D warnings` and `cargo heli nextest run -p <crate>`. A task that changes a public API also
  runs its dependents. Whoever coordinates runs the workspace-wide checks once.
- **Every behaviour has a test that fails without it** ("Before each push", item 10): the
  task's rows are run with the change reverted, and the mutation run over its own lines goes
  to the review.
- **A consumer-visible change adds `changelog.d/<branch-slug>.md`.**
- **Gated tasks** (G1) are marked *gated* with the frozen-crate files they need. None starts
  while ADR-0304 or ADR-0305 has a bullet in
  [gaps.md](../../../gaps.md#accepted-adr-sections-not-built-yet), unless the owner writes an
  exception beside the task here.
- **An ADR task is a design change:** it is read with the design lenses before it merges
  (AGENTS.md, "Before a merge"), and the code tasks it blocks do not start until it is
  accepted.
- **Flutter's tracker first.** A task that pins a defect from [requirements.md](requirements.md)
  searches the tracker for the behaviour and adds what it finds to the test table; the spec
  read the source only.

## Status

| Task | Content | Requirements | Depends on | Gated | Status |
|------|---------|--------------|------------|:-----:|--------|
| RC0 | Harness reads family modules; `docs/testing.md` drops the copied count | R1 | — | | not started |
| RC1 | `OverflowClip`: one owner of the overflow answer for six objects | R2 | RC0 | | not started |
| RC2 | Sliver pass-through base; six proxies move to it | R3, R4 | RC0 | | not started |
| RC3 | `MaxPaintRect`, `RenderSliverClip<S>`, `SliverClipRect`, `SliverClipRRect` | R3 | RC2 | | not started |
| RC4 | `RenderDecoratedSliver`, `DecoratedSliver` | R4 | RC3 | | not started |
| RC5 | `BlockSemantics`, `SliverVisibility` | R9 | — | | not started |
| RC6 | `RenderOverflowBar`, `OverflowBar` | R9 | RC0 | | not started |
| RC7 | `RenderImage` colour, blend mode and opacity | R10 | RC0 | | not started |
| RC8 | `RenderSliverVariedExtentList`, `SliverVariedExtentList`, the `ListView` constructor | R5 | RC0 | | not started |
| RC9 | `RenderTreeSliver`, `TreeSliver` | R6 | RC8 | | not started |
| RC10 | `RenderPinnedHeaderSliver`, `RenderSliverResizingHeader` and their widgets | R7 | RC0 | | not started |
| RC11 | `RenderSliverFloatingHeader`, `SliverFloatingHeader`; the floating state leaves `sliver_persistent_header.rs` | R7 | RC10 | | not started |
| RC12 | `RenderSliverLayoutBuilder`, `SliverLayoutBuilder`, `PageView::pad_ends` | R8 | RC2 | | not started |
| RC13 | "Mapping decisions" entries for what stays covered by a different design | R18 | — | | not started |
| RC14 | ADR: the gesture binding classifies a tap against the regions on its hit path | R12 | — | | not started |
| RC15 | `TapRegionRegistry`, `RenderTapRegion`, `TapRegionSurface`, `TapRegion` | R12 | RC14, RC0 | | not started |
| RC16 | ADR: a subtree filter is a paint effect | R11 | — | | not started |
| RC17 | The filter effect, `RenderColorFiltered`, `RenderImageFiltered` and their widgets | R11 | RC16, RC0 | yes | not started |
| RC18 | `RenderImage` inversion and filter quality | R10 | RC7, RC16 | yes | not started |
| RC19 | ADR: a paragraph lays out the boxes its spans embed | R13, R14 | — | | not started |
| RC20 | Span children are `InlineSpan`s; placeholders shape as inline boxes | R13 | RC19 | yes | not started |
| RC21 | `RenderParagraph` inline children, the span builder, `Text::rich` and `RichText` | R13 | RC20, RC0 | | not started |
| RC22 | `RenderEditable` inline children | R14 | RC21 | | not started |
| RC23 | ADR: lazy children are requested by a typed key | R16 | — | | not started |
| RC24 | The request strategy, its element, retain set and pinned set take a key type | R16 | RC23 | yes | not started |
| RC25 | `RenderTwoAxisViewport`, the uniform-cell layout, `TwoAxisScrollView` | R16 | RC24, RC1 | | not started |
| RC26 | ADR: a list wheel is a sliver, and a sliver states a per-child paint transform | R15 | RC23 | | not started |
| RC27 | The sliver paint-transform hook and `RenderSliverWheel` | R15 | RC26, RC0 | yes | not started |
| RC28 | `ListWheelScrollView`, its controller and physics, the looping wheel | R15 | RC27 | | not started |
| RC29 | ADR: a native view is a scene node the host places | R17 | owner decision | | not started |
| RC30 | Native views, broken into tasks when RC29 is accepted | R17 | RC29 | yes | not started |

## Order and parallelism

1. **RC0 first.** It is the only task every object-adding task depends on.
2. **Then, in parallel** (file sets are disjoint): RC1, RC2, RC5, RC6, RC7, RC8, RC10, RC13,
   and the ADR tasks RC14, RC16, RC19, RC23, which touch only `docs/adr/`.
3. **After RC2:** RC3 and RC12 in parallel, then RC4. **After RC8:** RC9. **After RC10:** RC11
   (both edit the header family). **After RC14:** RC15.
4. **Gated, in this order once the freeze lifts or an exception is written:** RC17, then RC18;
   RC20, RC21, RC22; RC24, RC25; RC26 is written after RC23 is accepted, then RC27, RC28.
   RC17, RC20, RC24 and RC27 each edit `flui-rendering` or `flui-painting`; no two of them run
   at once unless their frozen-crate files differ (RC20 is `flui-painting` only, so it may run
   beside RC17 or RC24).
5. **RC29 waits for the owner.**

The cheapest tasks that are `flui-objects` alone, or `flui-objects` with a thin widget, are RC1,
RC2, RC6 and RC7, then RC4; RC5 is `flui-widgets` alone.

## Ungated tasks

### RC0. The harness reads its families from more than one file

- **What:** W1. The guard `catalog_covers_every_render_object_name` reads the root file and
  every listed family file; a family module that is declared and not listed fails it. No
  existing row moves. `docs/testing.md` links `RENDER_OBJECT_TYPES` instead of saying "81".
- **Owns:** `crates/flui-objects/tests/render_object_harness.rs` (the guard and the module
  list), `crates/flui-objects/tests/harness/` (new), the harness paragraph of
  `docs/testing.md`.
- **Tests:** the two guards pass; by hand, a row placed in a family module and then deleted
  makes `catalog_covers_every_render_object_name` fail (recorded in the pull request).
- **Checks:** `cargo heli clippy -p flui-objects --all-targets -- -D warnings`,
  `cargo heli nextest run -p flui-objects`; the `checks` job's document steps (typos, lychee,
  unmounted-tests).
- **Depends:** — **Size:** S

### RC1. One owner for the overflow answer

- **What:** W2. `OverflowClip` replaces `has_visual_overflow` in `RenderStack`,
  `RenderIndexedStack`, `RenderWrap`, `RenderFittedBox`, `RenderAnimatedSize` and
  `RenderConstraintsTransformBox`; each reports it from `describe_approximate_paint_clip`. The
  first step traces how input regions take a reported clip ([design.md](design.md),
  "Unverified"); if they do not read it, the task says so here before going further. The gap
  entry "An overflow clip cuts off paint but not semantics or input regions" leaves
  `docs/gaps.md`.
- **Owns:** `crates/flui-objects/src/layout/overflow_clip.rs` (new), `stack.rs`, `wrap.rs`,
  `fitted_box.rs`, `animated_size.rs`, `constraints_transform_box.rs`;
  `crates/flui-objects/tests/harness/overflow_clip.rs` (new); that entry of `docs/gaps.md`.
- **Tests:** `harness_<object>_cuts_semantics_and_regions_at_its_overflow_clip` for the six
  objects (one table), `harness_overflow_answer_survives_a_layout_that_fails`,
  `harness_no_overflow_reports_no_clip`.
- **Checks:** clippy and nextest for `flui-objects`; nextest for `flui-widgets` (the objects'
  widgets have tests there).
- **Depends:** RC0 **Size:** M

### RC2. The sliver pass-through base

- **What:** W3, the pass-through half: `sliver/pass_through.rs`; the six pass-through slivers
  call the shared rules. No behaviour change. `MaxPaintRect` is not built here: it is
  `pub(crate)` and its first caller is RC3, which brings it.
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
  `crates/flui-widgets/src/scroll/sliver_clip.rs` (new),
  `crates/flui-widgets/tests/sliver_clip.rs` (new).
- **Tests:** R3's four harness rows, each over the four axis directions; widget test
  `a_pinned_header_keeps_a_following_clip_at_its_edge` (with a persistent header; RC10 adds the
  child-sized one to the same table).
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC2 **Size:** M

### RC4. Decorated sliver

- **What:** W5.
- **Owns:** `crates/flui-objects/src/sliver/sliver_decorated.rs` (new),
  `crates/flui-objects/tests/harness/sliver_decorated.rs` (new),
  `crates/flui-widgets/src/scroll/decorated_sliver.rs` (new).
- **Tests:** R4's three rows; a widget row in the existing sliver test module that mounts
  `DecoratedSliver` over a lazy list.
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
  `crates/flui-widgets/src/layout/overflow_bar.rs` (new).
- **Tests:** `harness_overflow_bar_wraps_to_a_column_when_its_row_does_not_fit`,
  `harness_overflow_bar_intrinsics_match_its_layout`,
  `harness_overflow_bar_orders_its_column_by_direction`,
  `harness_overflow_bar_mirrors_its_row_in_rtl`.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC0 **Size:** S

### RC7. Image colour and opacity

- **What:** W7, the ungated half: `color`, `color_blend_mode`, `opacity` on `RenderImage` and
  `Image`.
- **Owns:** `crates/flui-objects/src/image/render_image.rs`,
  `crates/flui-widgets/src/image/image.rs`, the image rows of the harness (a new
  `tests/harness/image_paint.rs`).
- **Tests:** `an_image_is_tinted_by_its_colour_and_blend_mode`,
  `an_image_is_faded_by_its_opacity`, `an_image_of_zero_opacity_paints_nothing`; the readback
  samples are chosen so the unset property gives a different pixel.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`; the `image` test
  targets of `flui-widgets`.
- **Depends:** RC0 **Size:** S

### RC8. Varied-extent list

- **What:** W8. `ItemExtentDelegate`, `ItemExtentDimensions`, `ExtentIndex`,
  `RenderSliverVariedExtentList`; the alias in `flui-view`; `SliverVariedExtentList` and the
  `ListView` constructor. What the fixed-extent list and this one share moves to
  `virtualized_band.rs`. "Mapping decisions" records that the dimensions omit the scroll
  offset.
- **Owns:** `crates/flui-objects/src/sliver/item_extent.rs`, `extent_index.rs`,
  `sliver_varied_extent_list.rs` (new), `virtualized_band.rs`, `sliver_fixed_extent_list.rs`
  (only what is extracted); `crates/flui-objects/tests/harness/varied_extent.rs` (new);
  the alias line in `crates/flui-view/src/element/sliver_adaptor.rs`;
  `crates/flui-widgets/src/scroll/sliver_varied_extent_list.rs` (new), `list_view.rs`.
- **Tests:** R5's four rows; the existing fixed-extent rows unchanged; a widget test
  `a_varied_extent_list_view_scrolls_to_an_exact_offset`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC0 **Size:** L

### RC9. Tree sliver

- **What:** W9. `TreeRowDelegate`, `TreeIndentation`, `RenderTreeSliver`; `TreeSliver` with its
  node type and controller. The first step reads `sliver/placement.rs` to decide whether the
  reveal uses `Placement` or a reveal fraction in the `Config`, and records the answer in
  "Mapping decisions".
- **Owns:** `crates/flui-objects/src/sliver/sliver_tree.rs` (new),
  `crates/flui-objects/tests/harness/tree_sliver.rs` (new), the alias in `flui-view`,
  `crates/flui-widgets/src/scroll/tree_sliver/` (new) and its test module.
- **Tests:** R6's three harness rows and two widget tests;
  `a_second_toggle_during_a_reveal_keeps_rows_in_order`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC8 **Size:** L

### RC10. Pinned and resizing child-sized headers

- **What:** W10, `RenderPinnedHeaderSliver` and `RenderSliverResizingHeader`, widgets
  `PinnedHeaderSliver` and `SliverResizingHeader`.
- **Owns:** `crates/flui-objects/src/sliver/child_sized_header.rs` (new),
  `crates/flui-objects/tests/harness/child_sized_header.rs` (new),
  `crates/flui-widgets/src/scroll/pinned_header_sliver.rs`, `sliver_resizing_header.rs` (new);
  an element in `crates/flui-view/src/element/` only if the three slots need one.
- **Tests:** `harness_pinned_header_takes_its_extent_from_its_child`,
  `harness_pinned_header_taller_than_the_viewport_stays_finite`,
  `harness_resizing_header_never_paints_its_prototypes`,
  `harness_resizing_header_reports_its_minimum_as_obstruction`,
  `harness_resizing_header_without_a_maximum_prototype_measures_its_child`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-widgets`, and `flui-view` if touched.
- **Depends:** RC0 **Size:** M

### RC11. Floating child-sized header

- **What:** W10, `RenderSliverFloatingHeader` and `SliverFloatingHeader`. The floating state
  machine moves out of `RenderSliverFloatingHeaderBase` into a private type in a file of its
  own, owned by both header families; public names do not change.
- **Owns:** `crates/flui-objects/src/sliver/sliver_persistent_header.rs`, a new
  `floating_header_state.rs` beside it, `child_sized_header.rs` (after RC10),
  `crates/flui-widgets/src/scroll/sliver_floating_header.rs` (new).
- **Tests:** `harness_floating_header_returns_by_the_distance_scrolled_back`,
  `harness_floating_header_snaps_open_when_released_past_half`; the rows of
  `family_persistent_header` unchanged.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC10 **Size:** M

### RC12. Sliver layout builder

- **What:** W11. `SliverConstraintsCell`, `RenderSliverLayoutBuilder`, the element (generic
  over the protocol, or a sliver twin: the task reads `element/layout_builder.rs` and keeps
  one rebuild rule), `SliverLayoutBuilder`, `PageView::pad_ends` by composition.
- **Owns:** `crates/flui-objects/src/sliver/sliver_layout_builder.rs` (new),
  `crates/flui-objects/src/layout/layout_constraints_cell.rs`,
  `crates/flui-view/src/element/layout_builder.rs`,
  `crates/flui-widgets/src/scroll/sliver_layout_builder.rs` (new), `page_view.rs`,
  `crates/flui-objects/tests/harness/sliver_layout_builder.rs` (new).
- **Tests:** R8's three widget tests; a harness row
  `harness_sliver_layout_builder_passes_its_constraints_to_its_cell`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC2 **Size:** M

### RC13. Records for what stays covered by a different design

- **What:** W18. Four entries in "Mapping decisions" of
  `crates/flui-objects/ARCHITECTURE.md`, from the table "Covered by a different design" of
  [design.md](design.md), each citing the tests that hold the FLUI form.
- **Owns:** those four paragraphs.
- **Tests:** none; every test it cites is found by `rg` under its name.
- **Checks:** the `checks` job's document steps.
- **Depends:** — **Size:** S

### RC14. ADR: tap classification

- **What:** the ADR of W12, in the shape of `docs/adr/README.md`, with the three options of
  [design.md](design.md) and the questions listed under "Unverified" answered from the code
  (claiming a pointer for a registry, the route of an accessibility activation, how a widget
  learns its page is current).
- **Owns:** one new file in `docs/adr/`.
- **Checks:** the `checks` job's document steps, the ADR-number step; a design read.
- **Depends:** — **Size:** S

### RC15. Tap regions

- **What:** W12 as RC14 decides it.
- **Owns:** a new module in `crates/flui-interaction/src/` for the registry and its ids, the
  metadata read in `crates/flui-interaction/src/binding/mod.rs`,
  `crates/flui-objects/src/interaction/tap_region.rs` (new),
  `crates/flui-objects/tests/harness/tap_region.rs` (new),
  `crates/flui-widgets/src/interaction/tap_region.rs` (new) and its test module.
- **Tests:** R12's five widget tests; a harness row
  `harness_tap_region_marks_its_hit_entry_and_joins_the_hits_below`;
  `a_panicking_outside_callback_leaves_the_registry_usable`.
- **Checks:** clippy and nextest for `flui-interaction`, `flui-objects`, `flui-widgets`.
- **Depends:** RC14, RC0 **Size:** L

## Gated tasks

### RC16. ADR: subtree filter

- **What:** the ADR of W13: field or scope, and the nesting order.
- **Owns:** one new file in `docs/adr/`. **Depends:** — **Size:** S

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

- **What:** the ADR of W14: the span tree, where measurement sits, intrinsics and dry layout,
  no implicit scaling, the semantics order.
- **Owns:** one new file in `docs/adr/`. **Depends:** — **Size:** M

### RC20. Span children and inline-box shaping — gated

- **What:** W14 in `flui-painting`: `TextSpan.children` becomes `Vec<InlineSpan>`; a
  placeholder is an inline box to Parley and one position in the text; `TextPainter` takes
  placeholder dimensions and returns box rectangles. Every user of the old child type in the
  workspace moves in this change.
- **Frozen-crate files:** `crates/flui-painting/src/typography/text_spans.rs`,
  `text_painter/measure.rs` and the painter's layout entry, `parley_text/shape.rs`.
- **Owns:** those, and each call site the compiler names (expected in `flui-objects/src/text/`
  and `flui-widgets/src/text/`).
- **Tests:** in `crates/flui-painting/tests/`: `a_placeholder_takes_one_text_position`,
  `a_nested_placeholder_reserves_its_box_on_the_line`,
  `a_truncated_placeholder_has_no_box`, `a_baseline_placeholder_sits_on_the_baseline`,
  `text_without_placeholders_shapes_as_before` (existing snapshots unchanged; a changed
  snapshot is justified in the pull request or is the defect).
- **Checks:** clippy and nextest for `flui-painting` and every crate with a moved call site;
  `cargo heli test --doc -p flui-painting`.
- **Depends:** RC19 **Size:** L

### RC21. Paragraph inline children

- **What:** W14 in `flui-objects` and `flui-widgets`: `InlineBoxes`, `RenderParagraph` with
  `Arity = Variable`, the span builder, the multi-child element for `RichText`, `Text::rich`.
- **Owns:** `crates/flui-objects/src/text/inline_boxes.rs` (new), `paragraph.rs`,
  `crates/flui-objects/tests/harness/inline_children.rs` (new),
  `crates/flui-widgets/src/text/` (the span builder, `rich_text.rs`, `text.rs`), an element in
  `flui-view` if the multi-child render view needs one.
- **Tests:** R13's four harness rows and two widget tests;
  `harness_paragraph_without_inline_children_lays_out_as_before`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-widgets`, `flui-view`.
- **Depends:** RC20, RC0 **Size:** L

### RC22. Editable inline children

- **What:** W14 for `RenderEditable`, on `InlineBoxes`.
- **Owns:** `crates/flui-objects/src/text/editable.rs`, its harness rows.
- **Tests:** R14's two rows.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets` (the editable-text
  suites).
- **Depends:** RC21 **Size:** M

### RC23. ADR: typed lazy key

- **What:** the ADR of W16: the request strategy, retain set and pinned set generic over a
  key; the three options.
- **Owns:** one new file in `docs/adr/`. **Depends:** — **Size:** M

### RC24. The request strategy takes a key type — gated

- **What:** W16's contract. The lazy slivers keep `usize` and their behaviour; the change is
  proven by their existing rows staying green.
- **Frozen-crate files:** `crates/flui-rendering/src/context/layout.rs` (the request and
  retain entries) and the erased context behind them.
- **Owns:** those and `crates/flui-view/src/element/sliver_adaptor.rs`.
- **Tests:** the rows of `family_sliver_lists` and the lazy-list widget suites unchanged;
  in `flui-rendering`'s tests, `a_keyed_request_builds_the_child_for_its_key` and
  `a_retain_set_drops_only_the_children_outside_it`.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-view`, `flui-objects`,
  `flui-widgets`.
- **Depends:** RC23 **Size:** L

### RC25. Two-axis viewport

- **What:** W16: `Vicinity`, `TwoAxisLayout`, `RenderTwoAxisViewport<L>`, the uniform-cell
  layout, `TwoAxisScrollView`.
- **Owns:** `crates/flui-objects/src/sliver/viewport/two_axis.rs` (new),
  `crates/flui-objects/tests/harness/two_axis_viewport.rs` (new), a keyed element in
  `flui-view`, `crates/flui-widgets/src/scroll/two_axis_scroll_view.rs` (new).
- **Tests:** R16's four rows; widget tests `a_diagonal_drag_scrolls_both_axes` and
  `a_two_axis_view_builds_no_child_outside_its_cache`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC24, RC1 **Size:** L

### RC26. ADR: the wheel

- **What:** the ADR of W15, written after RC23 is accepted, because a wheel keyed by a signed
  index on the typed-key seam is one of its options. It answers the two unverified points
  (one child painted twice; a sliver's origin in an anchored viewport) from the code.
- **Owns:** one new file in `docs/adr/`. **Depends:** RC23 **Size:** M

### RC27. The wheel sliver — gated

- **What:** W15: the per-child paint-transform hook on `RenderSliver`, `WheelGeometry` and its
  parameter newtypes, `RenderSliverWheel`.
- **Frozen-crate files:** `crates/flui-rendering/src/traits/render_sliver.rs` and the blanket
  forwarding to `RenderObject`.
- **Owns:** that hook, `crates/flui-objects/src/sliver/wheel/` (new),
  `crates/flui-objects/tests/harness/wheel.rs` (new), the alias in `flui-view`.
- **Tests:** R15's three harness rows; `harness_wheel_semantics_rect_follows_its_transform`;
  in `flui-rendering`'s tests, `a_sliver_without_the_hook_maps_its_children_as_before`.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-objects`, `flui-view`.
- **Depends:** RC26, RC0 **Size:** L

### RC28. The wheel widget

- **What:** `ListWheelScrollView`, its fixed-extent controller and physics, the looping wheel
  in the form RC26 decides.
- **Owns:** `crates/flui-widgets/src/scroll/list_wheel/` (new) and its test module.
- **Tests:** `a_looping_wheel_scrolls_past_its_last_item_to_its_first`,
  `a_wheel_settles_on_an_item`, `a_wheel_reports_its_selected_item_once_per_change`.
- **Checks:** clippy and nextest for `flui-widgets`.
- **Depends:** RC27 **Size:** M

### RC29. ADR: native views

- **What:** the ADR of W17, after the owner decides that native views enter the contract and
  for which hosts.
- **Owns:** one new file in `docs/adr/`; the gap entry "No native views inside the UI" is
  rewritten to point at it. **Depends:** owner decision **Size:** L

### RC30. Native views — gated

- **What:** not broken down. When RC29 is accepted this row is replaced by its tasks, the
  texture-backed path first; runs owed on devices become rows of
  `crates/flui-native/platform-parity.toml`.
- **Depends:** RC29

## Validation evidence

Empty until a task lands. Each task records here the final names of its tests, the commands it
ran with their results, and the pull request.
