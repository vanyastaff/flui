# render-object-coverage: tasks

- **Status:** In progress — requirements and design approved by the owner on 2026-11-25, the decisions in design.md as recommended; the table below records each task
- **Date:** 2026-11-25
- **Base:** `main` @ `fcab477`
- **Design:** [design.md](design.md) (shared seams, ADR list, decisions),
  [design-parity.md](design-parity.md) (W1–W18), [design-beyond.md](design-beyond.md)
  (W19–W26); [requirements.md](requirements.md) (R1–R26, gates G1–G3).

## Execution rules

- **One task, one branch, one pull request, one writer per file set** (AGENTS.md, "Working
  here"). "Owns" below is the file set; a task that finds it needs a file another open task
  owns stops and says so here.
- **Shared one-line files.** A task that adds a render object or a widget adds one line to
  each of: `crates/flui-objects/src/lib.rs` and the family's `mod.rs`; the `RENDER_OBJECT_TYPES`
  list and, for a new family, the two mount lines, its name in `family_files![..]` and the
  `family_*` test in `crates/flui-objects/tests/render_object_harness.rs`; the `mod` line of
  `crates/flui-view/src/element/mod.rs` for a new lazy sliver's file;
  `crates/flui-widgets/src/lib.rs`, the family's `mod.rs` and
  `crates/flui-widgets/tests/main.rs`. An entry in "Mapping decisions" of
  `crates/flui-objects/ARCHITECTURE.md` is its own paragraph. These edits are not ownership;
  they are rebased mechanically.
- **`crates/flui-view/src/element/sliver_adaptor.rs` is owned, not shared.** It holds the lazy
  element's manager and, as full blocks, the `LazyMultiBoxRender` implementation and the
  configuration of each of the four lazy slivers. A new lazy sliver (RC8, RC9, RC27) puts its
  block in a file of its own beside it and does not edit it. The tasks that do edit it are
  RC38, RC39 and RC24, one at a time, in that order where they meet. RC43 moves the four
  lazy slivers' readers in `flui-objects` and does not touch it.
- **A record a task makes untrue is that task's to rewrite.** A task that changes what a
  sentence of an `ARCHITECTURE.md`, a module's documentation or an ADR states owns that
  sentence and names it under "Owns".
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
  accepted. It merges `Proposed`. The change that accepts it writes the `Supersedes:` and
  `Superseded-by:` pair, leaves the superseded record's text as it is, and adds one bullet per
  unbuilt section under "Accepted ADR sections not built yet" of
  [gaps.md](../../../gaps.md#accepted-adr-sections-not-built-yet); each code task removes the
  bullets it builds, and the last writes `Implemented` on the record.
- **An ADR for a frozen pipeline waits with its code.** RC16, RC19, RC23, RC26 and RC29
  decide in `flui-layer`, `flui-rendering`, `flui-painting` or `flui-engine`. Each is written
  when its first code task may start: the freeze has lifted, or the owner has written an
  exception beside that task here. RC14 decides in no frozen crate and is written now.
- **The record first.** A task that pins a tracker row marked `†` in
  [requirements.md](requirements.md) reads the thread before writing the test and corrects the
  row. A task of R19–R26 checks the pitfalls it relies on against the other framework's source
  or tracker and replaces "recollection" in its requirement with what it found.
- **The shared row.** A task whose object keeps an `OverflowClip` or a `ShownChildren` adds
  its call of `harness_<object>_hits_and_publishes_a_child_only_where_it_paints`, with both
  builders; `every_keeper_of_a_shown_record_runs_the_shared_row` (RC1) fails the crate's
  tests until it does.
- **Runs owed on devices.** A task whose behaviour a host decides (a wheel's units, an IME, a
  pointer kind) adds the row to `crates/flui-native/platform-parity.toml`, restates
  `docs/platform-parity.md` from it, and names the platforms in its pull request. The tasks
  that owe one say so below.

## Status

| Task | Content | Req. | Crates | Depends on | Gated | Status |
|------|---------|------|--------|------------|:-----:|--------|
| RC0 | Harness reads family modules; the guard counts `pub` rows, whole names and mounted files; `docs/testing.md` drops the copied count | R1 | `flui-objects` (tests), docs | — | | in review |
| RC1 | The `shown` module with `OverflowClip`, the shared row and its guard; six overflow objects move onto it; the gap entry leaves | R2 | `flui-objects`, docs | RC0 | | not started |
| RC43 | `ShownChildren` and the sliver frame; the carousel, the list, the fixed-extent list and the grid move onto it; `PinnedChildren` returns its slots and keeps none | R2 | `flui-objects`, docs | RC1 | | not started |
| RC2 | Sliver pass-through base; six proxies move to it | R3, R4 | `flui-objects` | RC0 | | not started |
| RC3 | `MaxPaintRect`, `RenderSliverClip<S>`, `SliverClipRect`, `SliverClipRRect` | R3 | `flui-objects`, `flui-widgets` | RC43, RC2 | | not started |
| RC4 | `RenderDecoratedSliver`, `DecoratedSliver` | R4 | `flui-objects`, `flui-widgets` | RC3 | | not started |
| RC5 | `BlockSemantics`, `SliverVisibility` | R9 | `flui-widgets`, `flui-material` docs | — | | not started |
| RC6 | `RenderOverflowBar`, `OverflowBar` | R9 | `flui-objects`, `flui-widgets` | RC0 | | not started |
| RC7 | `RenderImage` colour, blend mode and opacity | R10 | `flui-objects`, `flui-widgets` | RC0 | | not started |
| RC8 | The walk takes its extent source; `RenderSliverVariedExtentList` with authored extents, `SliverVariedExtentList`, the `ListView` constructor | R5 | `flui-objects`, `flui-view`, `flui-widgets` | RC0, RC42, RC43 | | not started |
| RC9 | `RenderTreeSliver` with the reveal map and roster snapshots, `TreeSliver` and its roster | R6 | `flui-objects`, `flui-view`, `flui-widgets` | RC8, RC43, RC42 | | not started |
| RC10 | `RenderPinnedHeaderSliver`, `RenderSliverResizingHeader`, `HeaderFrame`, their widgets; group rows | R7 | `flui-objects`, `flui-widgets` | RC43 | | not started |
| RC11 | `RenderSliverFloatingHeader`, `SliverFloatingHeader`; the floating state leaves `sliver_persistent_header.rs` | R7 | `flui-objects`, `flui-widgets` | RC10 | | not started |
| RC12 | `RenderSliverLayoutBuilder` and its element, `RenderSliverFractionalPadding`, `PageView::pad_ends` | R8 | `flui-objects`, `flui-view`, `flui-widgets` | RC2 | | not started |
| RC13 | "Mapping decisions" entries for what stays covered by a different design; the nested-scroll behaviours against ADR-0209 | R18 | docs, `flui-widgets` (tests) | — | | not started |
| RC14 | ADR: a tap is classified against the regions on the path of its down, by the binding; it supersedes ADR-0208 §2 rule 4 in part | R12 | `docs/adr`, `docs/gaps.md` | — | | not started |
| RC15 | Tap regions, the pointer route: event, registry, snapshot, every end of a contact, hold, capability, group liveness, `RenderTapRegion`, `TapRegion`, the popup's group, the field's overlays, the `flui-sdk` re-exports | R12 | `flui-interaction`, `flui-objects`, `flui-view`, `flui-runtime`, `flui-widgets`, `flui-sdk`, `flui-testing` | RC14, RC0 | | not started |
| RC16 | ADR: a subtree's filter and its group blend are paint effects | R11, R26 | `docs/adr`, `docs/gaps.md` | — | with RC17 | not started |
| RC17 | The filter effect and the records of the effect order, `RenderColorFiltered`, `RenderImageFiltered` and their widgets | R11 | `flui-rendering`, `flui-engine` (tests), `flui-objects`, `flui-widgets` | RC16, RC0 | yes | not started |
| RC18 | `RenderImage` inversion and filter quality | R10 | `flui-painting`, `flui-engine`, `flui-objects`, `flui-widgets` | RC7, RC16 | yes | not started |
| RC19 | ADR: a paragraph lays out the boxes its spans embed, and an editing value holds its placeholders | R13, R14 | `docs/adr`, `docs/gaps.md` | — | with RC20 | not started |
| RC20 | Span children are `InlineSpan`s; a measured placeholder is tied to its child by its ordinal and shapes as an inline box | R13 | `flui-painting` and moved call sites | RC19 | yes | not started |
| RC21 | `RenderParagraph` inline children, the span builder, `Text::rich` and `RichText` | R13 | `flui-objects`, `flui-view`, `flui-widgets` | RC20, RC43 | | not started |
| RC22 | `RenderEditable` inline children; the editing value holds its placeholders' ids and is edited by range in the controller and the input session; the field builds its spans from it; the obscured field | R14 | `flui-objects`, `flui-widgets` | RC21 | | not started |
| RC23 | ADR: lazy children are requested by a typed key | R16 | `docs/adr`, `docs/gaps.md` | — | with RC24 | not started |
| RC24 | The request strategy, its element, retain set and pinned set take a key type | R16 | `flui-rendering`, `flui-view` | RC23 | yes | not started |
| RC25 | `RenderTwoAxisViewport`, the uniform-cell layout, `TwoAxisScrollView`, one contact on two owners | R16 | `flui-objects`, `flui-view`, `flui-widgets` | RC24, RC43 | | not started |
| RC26 | ADR: a sliver states a per-child paint transform, the list wheel is a sliver, and a notch is the snap policy's step | R15, R25 | `docs/adr`, `docs/gaps.md` | RC23 | with RC31 | not started |
| RC27 | `WheelGeometry`, `RenderSliverWheel`; the record's transform, opacity and two-part rule | R15 | `flui-objects`, `flui-view` | RC31, RC43 | | not started |
| RC28 | `ListWheelScrollView`, `WheelSnap`, `SnapPolicy::step` and the packet table in the scrollable, the looping wheel | R15 | `flui-widgets`, `flui-native` (the parity rows) | RC27 | | not started |
| RC29 | ADR: a native view is a scene node the host places | R17 | `docs/adr`, `docs/gaps.md` | owner decision | with RC30 | not started |
| RC30 | Native views, broken into tasks when RC29 is accepted | R17 | set by RC29 | RC29 | yes | not started |
| RC31 | The sliver per-child transform seam: the hook, the hit-test transform, a twice-painted child walked once | R15, R25 | `flui-rendering` | RC26 | yes | not started |
| RC32 | Tap regions, accessibility activations: one production door, a class for every action, classification by ancestry, the consuming stop and its typed outcome | R12 | `flui-protocol`, `flui-view`, `flui-runtime`, `flui-testing`, `flui-widgets` (tests) | RC15 | | not started |
| RC33 | `AnchoredPlacement`; the popup menu and the selection toolbar move onto it | R23 | `flui-widgets` | — | | not started |
| RC34 | `RenderFirstThatFits`, `FirstThatFits` | R24 | `flui-objects`, `flui-widgets` | RC43 | | not started |
| RC35 | The leading-run rule; `RenderWrap` with `max_runs`, an indicator and a keyed reserve; a stateful `Wrap` that relabels it on the split's wake | R22 | `flui-objects`, `flui-widgets` | RC43 | | not started |
| RC36 | The `tracks` module; `TableColumnWidth` becomes `TrackSize` in every crate and package; `RenderTable` moves onto it | R21 | `flui-objects`, `flui-widgets`, `flui-sdk`, `flui-material` | — | | not started |
| RC37 | `GeometryObserver`, a leader watch around its child | R19 | `flui-widgets` | — | | not started |
| RC44 | `LeaderGeometry` reports the leader chain's transform and clip; `VisibilityObserver` with thresholds and a margin | R19 | `flui-rendering`, `flui-layer`, `flui-widgets` | RC37 | yes | not started |
| RC38 | Sticky rows in `RenderSliverList` and `RenderSliverVariedExtentList`; the lazy element's one set kept outside the band | R20 | `flui-objects`, `flui-view`, `flui-widgets` | RC8, RC43 | | not started |
| RC39 | `ItemEffect` on the four lazy slivers | R25 | `flui-objects`, `flui-view`, `flui-widgets` | RC31, RC38 | | not started |
| RC40 | The group layer, the blend mode on `PaintOpacity`, `RenderBlendGroup`, `RenderMask`, `BlendGroup`, `Mask` | R26 | `flui-layer`, `flui-engine`, `flui-rendering`, `flui-objects`, `flui-widgets` | RC16, RC17 | yes | not started |
| RC41 | `RenderGrid`, `GridParentData`, `Grid`, `GridCell` | R21 | `flui-objects`, `flui-widgets` | RC36, RC1, RC0 | | not started |
| RC42 | `Virtualizer::splice`, which returns its anchor correction, and `extent_of`; the module documentation states what the code does | R5, R6 | `flui-rendering` | — | defect fix | not started |

"Defect fix" in the Gated column: the task edits a frozen crate and the freeze admits it as a
defect fix (AGENTS.md, "Build what is accepted before designing further"); the owner confirms
that reading for RC42 in [design.md](design.md), "Decisions for the owner", 12 and 13. RC3
edits `flui-engine` on the same ground only if its readback row shows a seam. "With RCn": an
ADR for a frozen pipeline, written when that code task may start ("Execution rules").

## Order and parallelism

1. **RC0 first.** Every task that adds a harness row depends on it.
2. **Then, in parallel** (file sets are disjoint): RC1, RC2, RC5, RC6, RC7, RC13, RC33, RC36,
   RC37, RC42, and the one ADR task that is written now, RC14.
3. **After RC1:** RC43, and RC41 once RC36 has landed. **After RC43:** RC34, and RC10 and
   RC35 in parallel (RC35 owns `wrap.rs` after RC1 leaves it). **After RC43 and RC2:** RC3,
   then RC4. **After RC2:** RC12. **After RC10:** RC11. **After RC14:** RC15, then RC32.
4. **After RC42 and RC43:** RC8. **After RC8:** RC9 and RC38 one after the other, in either
   order: both edit `virtualized_band.rs`.
5. **Gated, once the freeze lifts or an exception is written,** each ADR first and its code
   after it is accepted: RC16, then RC17, then RC18 and RC40; RC19, then RC20, then RC21 and
   RC22; RC23, then RC24, then RC25; RC26 after RC23, then RC31, then RC27 and RC28, and RC39
   after RC31 and RC38; RC44 after RC37. RC17, RC24, RC31, RC40 and RC44 each edit
   `flui-rendering`, and no two of them run at once unless their files in it differ (RC17 and
   RC40 share `traits/paint_effects.rs` and the same paragraphs of its `ARCHITECTURE.md`;
   RC24 is `context/layout.rs`; RC31 is `traits/render_sliver.rs`, the sliver hit-test
   context and two walks; RC44 is `pipeline/owner/leader_watch.rs` and one paragraph of the
   `ARCHITECTURE.md`). RC42 is `virtualization/` alone and meets none of them. RC20 and RC18
   are `flui-painting` with different files; RC40 and RC44 are `flui-layer` with different
   files.
6. **`sliver_adaptor.rs` has one writer at a time:** RC38, then RC39; RC24 before or after
   the pair, never between a branch and its merge.
7. **RC29 waits for the owner.**

Files edited by tasks that do not depend on each other are rebased, not owned twice:
`crates/flui-widgets/src/navigator/page_route.rs` (a doc sentence in RC5, the popup's group in
RC15), `crates/flui-widgets/src/popup_menu.rs` (the placement in RC33, passing the opener's
group in RC15) and `crates/flui-widgets/src/text/selection_menu/toolbar.rs` (the placement in RC33,
joining the field's group in RC15). Whoever lands second rebases.

The cheapest tasks are RC2, RC6, RC7 and RC33, then RC4 and RC34; RC5 and RC37 are
`flui-widgets` alone.

## Tier A, ungated

### RC0. The harness reads its families from more than one file

- **What:** W1. The guard's matching moves into one function, `rows_naming`, with W1's three
  rules: a row is a column-0 `fn harness_` under any visibility that the compiler builds at
  the top level, and ends at the next column-0 line (W1, rule 1); a type counts only as a whole
  identifier; a family file counts only when its name is in `family_files![..]` and it is
  mounted (W1, rule 3). `family_files_are_mounted_and_listed` reads `tests/harness/` and
  takes a directory that does not exist as the empty set, which is what this task leaves. A
  type
  the substring match covered only through a longer name gets its own row here. No existing
  row moves. `docs/testing.md` links `RENDER_OBJECT_TYPES` instead of saying "81".
- **Owns:** `crates/flui-objects/tests/render_object_harness.rs` (the guards and the module
  list), the harness paragraph of `docs/testing.md`, `crates/flui-objects/README.md` (its
  copied counts). It creates no directory:
  `crates/flui-objects/tests/harness/` appears with RC1's first family file.
- **Tests:** `the_guard_counts_a_pub_row_of_a_family_file`,
  `the_guard_ends_a_row_at_its_own_end`,
  `the_guard_skips_a_row_the_compiler_does_not_build`,
  `the_guard_refuses_a_name_inside_a_longer_one`,
  `the_guard_refuses_a_family_file_that_is_not_mounted`,
  `the_mount_check_takes_a_missing_directory_as_no_files`,
  `the_mount_check_lists_the_stems_in_order`,
  `the_mount_check_refuses_a_directory_it_cannot_list`, each over literal sources or a
  temporary directory and each failing with the matcher reverted;
  `a_family_entry_is_the_file_its_name_mounts` over the expansion of `family_files!`;
  `the_family_macro_reads_the_family_directory`;
  `family_files_are_mounted_and_listed`; the two existing guards pass.
- **Checks:** clippy and nextest for `flui-objects`; the `checks` job's document steps (typos,
  lychee, unmounted-tests).
- **Depends:** — **Size:** S

### RC1. The shown answer: the overflow record

- **What:** W2's first task and the seam of [design.md](design.md), "The shown answer".
  `crates/flui-objects/src/shown.rs` with `OverflowClip` (`begin`, `commit`, `clip`). It
  replaces the `has_visual_overflow` field in `RenderStack`, `RenderIndexedStack`,
  `RenderWrap`, `RenderFittedBox`, `RenderAnimatedSize` and `RenderConstraintsTransformBox`:
  each one's `perform_layout` is `begin`, its own `lay_out`, `commit`; each reports the clip
  from `describe_approximate_paint_clip` and keeps its public `has_visual_overflow()` as a
  read of the record. The generic shared row with its two builders and the guard that every
  keeper of a record runs it are added to the harness. The gap entry "An overflow clip cuts
  off paint but not semantics or input regions" leaves `docs/gaps.md`. "Mapping decisions"
  records the contract and the hit rule of an unclipped overflow.
- **Owns:** `crates/flui-objects/src/shown.rs` (new), `layout/stack.rs`, `wrap.rs`,
  `fitted_box.rs`, `animated_size.rs`, `constraints_transform_box.rs`;
  `crates/flui-objects/tests/harness/shown.rs` (new); that entry of `docs/gaps.md`. Not
  owned, because the accessors keep their meaning: `crates/flui-testing/src/widgets.rs` and
  the two documents of `flui-rendering` that name `has_visual_overflow`; the task's last step
  is `rg has_visual_overflow` and a reading of each hit against the code.
- **Tests:** R2's rows for the six overflow objects, as named there, the shared row for each,
  `every_keeper_of_a_shown_record_runs_the_shared_row` and
  `the_keeper_guard_finds_a_record_field_in_literal_source`.
- **Checks:** clippy and nextest for `flui-objects`; nextest for `flui-widgets` and
  `flui-testing` (the objects' widgets and the overflow probe have tests there).
- **Depends:** RC0 **Size:** M

### RC43. The shown answer: the per-child record and the lazy slivers `main` has

- **What:** W2's second task. `ShownChildren` and `ShownPass` as far as they have callers:
  parts with a slot and a clip, the announced slots, the sliver frame and its hit mapping,
  `begin` and `commit`. `PinnedChildren::lay_out` returns the slots it laid out; its `slots`
  field, `is_pinned_slot` and `clear_slots` are deleted. `RenderSliverCarousel` moves from
  its private `Shown` table, and `RenderSliverList`, `RenderSliverFixedExtentList` and
  `RenderSliverGrid` from `is_pinned_slot`, onto the record, their pinned slots announced. No
  behaviour changes. The record "Lazy slivers lay out a pinned child outside the band" of
  `crates/flui-objects/ARCHITECTURE.md` is rewritten for the three states. The last step is
  `rg 'is_pinned_slot|clear_slots' crates/ docs/adr/` with no hit left.
- **Owns:** the `ShownChildren` half of `crates/flui-objects/src/shown.rs`,
  `sliver/pinned.rs`, `sliver/carousel/render.rs`, the readers (`paint`, `hit_test`, the
  semantics visit, the clip hooks) and the `perform_layout` wrapper of `sliver_list.rs`,
  `sliver_fixed_extent_list.rs` and `sliver_grid.rs`; that paragraph of the
  `ARCHITECTURE.md`; its rows in `tests/harness/shown.rs`.
- **Tests:** R2's rows for the per-child record, as named there, with the failed-layout row,
  the shared row and the pinned-slot row run for each of the four slivers; their existing
  rows and ADR-0265's tests unchanged.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view` and `flui-widgets`.
- **Depends:** RC1; before RC8, which edits the walk's call in `sliver_list.rs` **Size:** M

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
- **If the readback row shows a seam:** the object is not changed. The defect is the engine's
  clip snapping (ADR-0098 §6); it is fixed in `flui-engine` in a pull request of its own, as
  a defect fix with the row as its test, or listed in `docs/gaps.md` with the row marked as
  the evidence, and this task lands without it.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`; the readback suite at
  one test thread.
- **Owed on devices:** the seam row runs on lavapipe; two adjacent clipped slivers at a
  fractional offset on each host's GPU backend are a row of `platform-parity.toml`.
- **Depends:** RC43 (the sliver frame), RC2 **Size:** M

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

- **What:** W6, the two compositions. `rg BlockSemantics` has four hits outside this spec.
  The three that say FLUI has no `BlockSemantics` are corrected. The fourth,
  `docs/adr/ADR-0020-transition-modal-route-seam.md` ("Not carried: … `BlockSemantics`"),
  lists what the modal route carries, which this task does not change, and stays.
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

### RC42. The offset index's edit in the middle — a defect fix in a frozen crate

- **What:** [design.md](design.md), "The offset index". `Virtualizer::splice(at, removed,
  inserted) -> Option<AnchorCorrection>` in O(log n) plus the runs touched, with the anchor
  moved with its item and the change of that item's offset returned;
  `Virtualizer::extent_of(index) -> Option<ItemExtent>`; the module documentation of
  `virtualization/mod.rs` corrected where it says a mid-list insert or delete goes through
  `set_count`. ADR-0003 decided both the edit and the public `ItemExtent`; the code and the
  document disagree with it, which is why the freeze admits the change ("Decisions for the
  owner", 12 and 13). If the owner declines 13, `extent_of` is dropped here and RC8 keeps its
  resident band's answers itself.
- **Frozen-crate files:** `crates/flui-rendering/src/virtualization/mod.rs`, `sumtree.rs`.
- **Owns:** those and their tests.
- **Tests:** in `flui-rendering`'s tests: `a_splice_keeps_the_extents_on_both_sides`,
  `a_splice_moves_the_anchor_with_its_item`,
  `a_splice_above_the_anchor_returns_its_correction` (and none for one below it),
  `a_splice_of_a_removed_anchor_lands_on_the_edit`,
  a property test of `splice` against the item-wise model of the test build,
  `extent_of_returns_what_set_measured_stored` (100.0 after extents that do not sum exactly);
  the existing virtualization tests and every lazy-list row of the harness unchanged.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-objects`, `flui-widgets`;
  `cargo heli test --doc -p flui-rendering`.
- **Depends:** — **Size:** M

### RC8. Varied-extent list

- **What:** W8 and the walk's half of "The offset index". `walk_virtualizer_band` takes a
  `BandExtents`; `MeasuredExtents` is today's rule and `RenderSliverList` passes it;
  `AuthoredExtents` asks, validates, stores and lays the child out tight at the stored
  extent, and re-stores the first visible item each pass so the index holds the walk's
  anchor. The list keeps `ShownChildren` from its first version, with its pinned slots
  announced. `ItemExtentDelegate`, `ItemExtentDimensions`, `EAGER_EXTENT_LIMIT` with its perf
  scenario, `RenderSliverVariedExtentList` with `set_placement`; its `LazyMultiBoxRender`
  block, configuration and alias in a new file of `flui-view`; `SliverVariedExtentList` and
  the `ListView` constructor. `MAX_UNBOUNDED_WINDOW_CHILDREN` stays in `sliver_grid.rs`
  untouched. The mapping decision "A lazy list records only measured extents" is rewritten
  for two extent sources; the dimensions without a scroll offset, the three count cases, the
  eager limit and the rule for each writer of the index (W8) are recorded.
- **Owns:** `crates/flui-objects/src/sliver/item_extent.rs`, `sliver_varied_extent_list.rs`
  (new), `virtualized_band.rs`, the one call of the walk in `sliver_list.rs`; that paragraph
  of `crates/flui-objects/ARCHITECTURE.md`;
  `crates/flui-objects/tests/harness/varied_extent.rs` (new);
  `crates/flui-view/src/element/sliver_varied_extent_list.rs` (new);
  `crates/flui-widgets/src/scroll/sliver_varied_extent_list.rs` (new), `list_view.rs`; the
  perf scenario's file.
- **Tests:** R5's rows as named there
  (`harness_varied_extent_list_places_items_at_the_sum_of_their_extents`,
  `…_gives_a_child_its_extent_whatever_the_child_wants`,
  `…_asks_the_delegate_once_per_item_between_invalidations`,
  `…_asks_at_most_its_budget_in_one_layout`,
  `…_with_fixed_answers_asks_nothing_again_on_a_resize`,
  `…_never_asks_for_an_index_at_or_past_its_count`,
  `…_lays_an_item_out_at_exactly_its_extent`, `…_refuses_a_non_finite_extent`,
  `…_of_zero_extents_builds_a_bounded_window`,
  `…_with_an_endless_source_asks_only_its_window`,
  `…_restores_a_deep_offset_without_walking_from_zero`,
  `…_publishes_its_pinned_child_without_painting_it`,
  `…_keeps_the_visible_items_in_place_when_items_above_are_first_asked`), the shared row,
  `harness_the_measured_list_lays_out_as_before_over_the_shared_walk`; the existing list,
  grid and fixed-extent rows unchanged; a widget test
  `a_varied_extent_list_view_scrolls_to_an_exact_offset`; the perf scenario held by
  `flui_testing::perf::Baselines`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC0, RC42, RC43 **Size:** L

### RC9. Tree sliver

- **What:** W9. `TreeRowDelegate` as an immutable snapshot with its length, revision and
  edits, `RosterEdit`, `RosterRevision` typed by `RosterId`, `TreeIndentation`, `TreeReveal`,
  the reveal map, `RenderTreeSliver` on the authored extent source with the indent as
  cross-axis offset and the seven writers of its index as W9 lists them; its block,
  configuration and alias in a new file of `flui-view`; `TreeSliver` with its node source,
  controller and roster, the roster's copy-on-write rows and edit log, a new roster in
  `did_update_view` when the extent rule, the source or the controller changes, rows keyed by
  node id. "Mapping decisions" records the departures from Flutter. Its first step settles
  whether the lazy element re-slots a keyed row whose index moved
  ([design.md](design.md), "Unverified").
- **Owns:** `crates/flui-objects/src/sliver/sliver_tree/` (new),
  `crates/flui-objects/tests/harness/tree_sliver.rs` (new),
  `crates/flui-view/src/element/sliver_tree.rs` (new),
  `crates/flui-widgets/src/scroll/tree_sliver/` (new) and its test module; what the walk needs
  in `virtualized_band.rs` to map the window through the reveal map and to place a child at a
  cross-axis offset.
- **Tests:** R6's harness rows and widget tests, as named there, and the shared row.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC8, RC43, RC42; not at the same time as RC38 **Size:** L

### RC10. Pinned and resizing child-sized headers

- **What:** W10, `RenderPinnedHeaderSliver`, `RenderSliverResizingHeader`, `HeaderFrame` as
  an inherited value the header's layout wakes its widget to rebuild when it changes,
  widgets `PinnedHeaderSliver` and `SliverResizingHeader`; the check of the prototype flags
  against the children. The group rows of R7, over the new headers and the existing pinned
  one; a failing row is fixed in the group.
- **Owns:** `crates/flui-objects/src/sliver/child_sized_header.rs` (new),
  `crates/flui-objects/tests/harness/child_sized_header.rs` (new),
  `crates/flui-widgets/src/scroll/pinned_header_sliver.rs`, `sliver_resizing_header.rs`,
  `header_frame.rs` (new) and their test modules;
  `crates/flui-objects/src/sliver/sliver_main_axis_group.rs` and `sliver_cross_axis_group.rs`
  only if a group row fails.
- **Tests:** R7's rows and widget tests, as named there, except those RC11 names; the
  shared row; the child-sized case of `a_pinned_header_keeps_a_following_clip_at_its_edge`
  when RC3 has landed.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC43 **Size:** M

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
  [design.md](design.md). It records what W12 holds fixed and answers W12's eight scenarios,
  each with the type or the named test that rules the failure out: a class for every action;
  the typed outcome of a stopped activation and the order under a panic; the doors tests
  use; liveness per group at the snapshot; the snapshot from the unpruned hit path and the
  terminal event for every end of a contact; a field's overlays in its group; configuration
  read at delivery; a down that stops a fling. It carries `Supersedes: ADR-0208 §2 rule 4
  only (who admits a hold)` and states the ownership row "consuming region, outside contact"
  itself. ADR-0208 is not edited in this task. Its first step settles the W12 points of
  [design.md](design.md), "Unverified", by reading
  `crates/flui-interaction/src/gesture/arena/` and the binding's teardown paths.
- **On acceptance** (the owner's change, or this task's last commit once the owner says
  so): ADR-0208 gets `Superseded-by:` for §2 rule 4 only and keeps its text, and
  `docs/gaps.md` gets one bullet per unbuilt section of the new ADR, the ownership row among
  them. RC15 and RC32 remove them.
- **Owns:** one new file in `docs/adr/`; the header line of
  `docs/adr/ADR-0208-gesture-ownership-table-and-composition.md`; its bullets in
  `docs/gaps.md`.
- **Checks:** the `checks` job's document steps, the ADR-number step; a design read.
- **Depends:** — **Size:** M

### RC15. Tap regions, the pointer route

- **What:** W12 as RC14 decides it, without the accessibility route: `TapRegionEvent`,
  `TapPhase`, `TapOrigin`, `TapRegionRegistry` and its ids, an entry being an identity and a
  `Weak` to its state's cell; the snapshot taken from the hit-test result of the down and
  kept on `CachedPointerRoute`, with the slop latch and the group liveness asked then; the
  terminal event delivered from the one place a cached route is removed; the hold a
  consuming region admits and its row in the ownership table; the registry on `BuildOwner`
  and the infallible `tap_regions()` capability; `RenderTapRegion`; `TapRegion` and
  `TapRegionGroup::maybe_of`; `PopupRoute::tap_region_group`; the field's handles and
  toolbar joining the field's group; the names re-exported from `flui_sdk::interaction`. The
  paragraph "A hold is an admission kind" of `crates/flui-interaction/docs/ARCHITECTURE.md`
  is rewritten for two producers, and the task removes its bullets from `docs/gaps.md`.
- **Owns:** a new module `crates/flui-interaction/src/pointer/tap_region.rs`, the collection
  and the route removal in `crates/flui-interaction/src/binding/mod.rs`, that paragraph of
  `crates/flui-interaction/docs/ARCHITECTURE.md`; the registry's field in
  `crates/flui-view/src/owner/build_owner.rs`, the capability in
  `crates/flui-view/src/context/build_context.rs` and its implementations; the wiring in
  `crates/flui-runtime/src/presentation/assembly.rs`;
  `crates/flui-objects/src/interaction/tap_region.rs` (new),
  `crates/flui-objects/tests/harness/tap_region.rs` (new);
  `crates/flui-widgets/src/interaction/tap_region.rs` (new) and its test module; the builder on
  `PopupRoute` in `crates/flui-widgets/src/navigator/page_route.rs` and its use in
  `popup_menu.rs`; the group of the overlay entries in
  `crates/flui-widgets/src/text/editable_text/handles.rs`,
  `text/selection_area/handles.rs` and `text/selection_menu/toolbar.rs`; the re-export lines
  of `crates/flui-sdk/src/lib.rs` and its surface test; the new row in
  `crates/flui-testing/src/ownership.rs`.
- **Tests:** R12's tests, as named there, except those RC32 names; a harness row
  `harness_tap_region_marks_its_hit_entry_and_joins_the_hits_below`; the ownership table's
  new row.
- **Checks:** clippy and nextest for `flui-interaction`, `flui-objects`, `flui-view`,
  `flui-runtime`, `flui-widgets`, `flui-sdk`, `flui-testing`.
- **Owed on devices:** an outside tap from touch, mouse and stylus on each host, and a first
  click on an inactive window on macOS, as rows of `platform-parity.toml`.
- **Depends:** RC14, RC0 **Size:** L

### RC32. Tap regions, accessibility activations

- **What:** W12's activation route as RC14 decides it. The class of every `SemanticsAction`,
  held where the ADR puts it (recommended: beside the enum in `flui-protocol`);
  `deliver_semantics_action`, with the two-way ancestry walk and one `TapRegionEvent` with
  `TapOrigin::Semantics`; the typed outcome of a stopped activation through
  `PresentationState::dispatch_semantics_action` and the agent's act outcome, with the act
  schema's expectation changed and justified in the pull request;
  `SemanticsActionInvocation::invoke` joins `disallowed-methods` with its one `#[expect]`.
  `flui_testing::a11y::invoke_semantics_action` calls the door and gains nothing. No test
  moves onto `Harness`: the widget tests that resolve and invoke by hand are test code,
  exempt from the lint, and move with ADR-0400's migration. A `Tester` verb for a semantic
  action is added here only with the agreement of the testing-platform spec's owner.
- **Owns:** a new module in `crates/flui-view/src/` for the door;
  `crates/flui-runtime/src/presentation/accessibility.rs` and the semantic arm of
  `crates/flui-runtime/src/ui_runtime/agent.rs`; the class and the outcome's variant in
  `crates/flui-protocol/src/` with its schema expectation; the one call in
  `crates/flui-testing/src/a11y.rs`; the sentence of `crates/flui-widgets/ARCHITECTURE.md`
  that names `PipelineOwner::resolve_semantics_action` as the route; the entry in
  `clippy.toml`; new cases in the tap-region test module of `flui-widgets` and in
  `flui-runtime`'s tests; its bullets in `docs/gaps.md`.
- **Tests:** `a_screen_reader_activation_is_classified_by_ancestry`,
  `an_activation_of_a_merged_node_above_a_region_is_inside_it`,
  `a_screen_reader_activation_is_classified_once`,
  `a_consumed_outside_activation_does_not_run_its_action`,
  `a_consumed_outside_activation_is_reported_as_consumed`,
  `a_panicking_region_callback_still_decides_the_activation`,
  `every_semantics_action_has_a_tap_class` (or the compile-time form),
  `a_scroll_action_outside_a_consuming_region_still_runs`. `rg '\.invoke\(\)'` over
  production code finds the door and no other caller of an invocation.
- **Checks:** clippy and nextest for `flui-protocol`, `flui-view`, `flui-runtime`,
  `flui-testing`, `flui-widgets`; workspace clippy once, for the new disallowed method.
- **Depends:** RC15 **Size:** M

## Tier A, gated or waiting on a gated task

### RC16. ADR: filters and group blend as paint effects

- **What:** the ADR of W13 and W26: field or scope; the order, recommended as the group
  composite (alpha and blend mode as one step, on `PaintOpacity`), the clip, the filter, the
  transform; a filter as a function of the size; bounds and rounding; the group layer for
  blend and mask, with its measured cost; what a filter over a native view does until W17
  decides. Its first step checks the prior art the order cites. Written when RC17 may start.
- **Owns:** one new file in `docs/adr/`; on acceptance its bullets in `docs/gaps.md`, which
  RC17, RC18 and RC40 remove. **Depends:** — **Size:** M

### RC17. Subtree filters — gated

- **What:** W13 as RC16 decides it; closes the gap "No widget filters its subtree's colours or
  pixels". The records that state the effect order are rewritten for four effects.
- **Frozen-crate files:** `crates/flui-rendering/src/traits/paint_effects.rs` (the field and
  the documentation of the nesting order) and the place effects are lowered to the scene (or
  `context/paint.rs` if RC16 chooses scopes); the sentences of
  `crates/flui-rendering/ARCHITECTURE.md` that state the order ("opacity, clip, transform, in
  that fixed nesting order") and the `ImageFiltered` row of "Deferred producers"; readback
  rows in `flui-engine`'s tests.
- **Owns:** those, `crates/flui-objects/src/proxy/color_filtered.rs`, `image_filtered.rs`
  (new), `crates/flui-objects/tests/harness/filters.rs` (new),
  `crates/flui-widgets/src/paint/color_filtered.rs`, `image_filtered.rs` (new), the gap entry.
- **Tests:** a readback row per filter kind, `a_filtered_subtree_is_hit_where_it_is_laid_out`,
  `a_filtered_subtree_at_an_offset_paints_at_that_offset`,
  `a_blurred_subtree_at_a_fractional_scroll_offset_repaints_without_an_edge`,
  `a_disabled_image_filter_pushes_no_layer`,
  `a_filter_change_does_not_repaint_the_child` and
  `a_node_with_a_clip_and_a_filter_cuts_the_filters_output` (if RC16 chooses the effect
  field). `rg 'opacity, clip, transform'` across `crates/` and `docs/` finds no sentence
  that gives three effects as the whole order.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-objects`, `flui-widgets`,
  `flui-engine` (readback suites at one test thread).
- **Owed on devices:** the readback rows run on lavapipe; a filtered subtree on each host's
  GPU backend is a row of `platform-parity.toml`.
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

- **What:** the ADR of W14: the span tree, the ordinal that ties a measured placeholder to
  its child in a paragraph, the placeholder's two size forms, where measurement sits,
  intrinsics and dry layout, no implicit scaling, the semantics order and label, and the
  editing value: one type that holds the text and an id per U+FFFC, changed only by a range
  replace and `insert_placeholder`, held by the controller and by the input session's
  document; the field building its spans from it; the obscured field. Written when RC20 may
  start.
- **Owns:** one new file in `docs/adr/`; on acceptance its bullets in `docs/gaps.md`, which
  RC20, RC21 and RC22 remove. **Depends:** — **Size:** M

### RC20. Span children and inline-box shaping — gated

- **What:** W14 in `flui-painting`: `TextSpan.children` becomes `Vec<InlineSpan>`; the two
  size forms of `PlaceholderSpan`, `Measured` without a payload, and `PlaceholderOrdinal`; a
  placeholder is an inline box to Parley and one position in the text; `TextPainter` takes a
  slice of `PlaceholderDimensions` in ordinal order and returns one `Option` of a rectangle
  per measured placeholder in the same order, and answers a missing dimension with a typed
  error. `PlaceholderDimensions` gets its consumer and its line in `typography/README.md`
  is corrected. `PlaceholderSpan` keeps `PartialEq` and its `serde` form. Every user of the
  old child type and of `PlaceholderSpan`'s fields in the workspace moves in this change.
- **Frozen-crate files:** `crates/flui-painting/src/typography/text_spans.rs`,
  `typography/README.md`, `text_painter/measure.rs` and the painter's layout entry,
  `parley_text/shape.rs`.
- **Owns:** those, and each call site the compiler names (expected in `flui-objects/src/text/`
  and `flui-widgets/src/text/`).
- **Tests:** in `crates/flui-painting/tests/`: `a_placeholder_takes_one_text_position`,
  `a_nested_placeholder_reserves_its_box_on_the_line`,
  `a_truncated_placeholder_has_no_box_and_a_later_one_keeps_its_own`,
  `a_baseline_placeholder_sits_on_the_baseline`,
  `placeholders_in_right_to_left_text_are_ordered_with_their_runs`,
  `a_justified_line_moves_its_placeholders`,
  `a_painter_without_a_placeholders_dimensions_returns_an_error`,
  `a_span_tree_rebuilt_with_the_same_shape_names_the_same_ordinals`,
  `text_without_placeholders_shapes_as_before` (existing snapshots unchanged; a changed
  snapshot is justified in the pull request or is the defect).
- **Checks:** clippy and nextest for `flui-painting` and every crate with a moved call site;
  `cargo heli test --doc -p flui-painting`.
- **Depends:** RC19 **Size:** L

### RC21. Paragraph inline children

- **What:** W14 in `flui-objects` and `flui-widgets`: `InlineBoxes` over `ShownChildren`,
  `RenderParagraph` with `Arity = Variable` and its check of the children against the
  measured placeholders, the span builder, the multi-child element for `RichText`,
  `Text::rich`.
- **Owns:** `crates/flui-objects/src/text/inline_boxes.rs` (new), `paragraph.rs`,
  `crates/flui-objects/tests/harness/inline_children.rs` (new),
  `crates/flui-widgets/src/text/` (the span builder, `rich_text.rs`, `text.rs`), an element in
  `flui-view` if the multi-child render view needs one.
- **Tests:** R13's ten harness rows and six widget tests, as named there.
- **Checks:** clippy and nextest for `flui-objects`, `flui-widgets`, `flui-view`.
- **Depends:** RC20, RC43 **Size:** L

### RC22. Editable inline children

- **What:** W14 for `RenderEditable`, on `InlineBoxes`: the caret, selection and deletion
  rules of R14. The editing value becomes the type RC19 decides (working name
  `EditingText`): the text with one `PlaceholderId` per U+FFFC, changed by `replace(range,
  &str)` and `insert_placeholder`. `ControllerInner.text` and the session document's
  `Doc.text` are both that type, `SessionWrite` carries it, and the controller's write
  methods become calls of `replace`. The field builds its span tree from the value, asks an
  inline-child builder for each `PlaceholderId` and keys the child by it. An obscured field
  masks a placeholder and builds no child.
- **Owns:** `crates/flui-objects/src/text/editable.rs` and `text/editable/`, its harness rows;
  `crates/flui-widgets/src/text/editing_text.rs` (new), `controller.rs` (the stored value and
  its write methods), `text_store.rs` (the document's text and the write-back),
  `crates/flui-widgets/src/text/editable_text/field_view.rs` and `obscuring.rs`, the
  inline-child builder beside them.
- **Tests:** R14's harness rows and widget tests, as named there; the existing
  text-store and controller suites unchanged.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets` (the editable-text
  suites).
- **Owed on devices:** composing and deleting beside a placeholder through each host's IME,
  as a row of `platform-parity.toml`.
- **Depends:** RC21 **Size:** M

### RC23. ADR: typed lazy key

- **What:** the ADR of W16: the request strategy, retain set and pinned set generic over a
  key; one owner of the key-to-child mapping, with the swap and the keep-alive removal; the
  three options. Written when RC24 may start.
- **Owns:** one new file in `docs/adr/`; on acceptance its bullets in `docs/gaps.md`, which
  RC24 and RC25 remove. **Depends:** — **Size:** M

### RC24. The request strategy takes a key type — gated

- **What:** W16's contract. The lazy slivers keep `usize` and their behaviour; the change is
  proven by their existing rows staying green.
- **Frozen-crate files:** `crates/flui-rendering/src/context/layout.rs` (the request and
  retain entries) and the erased context behind them.
- **Owns:** those, `crates/flui-view/src/element/sliver_adaptor.rs`, `sparse_children.rs` and
  the lazy slivers' files beside them. The set kept outside the band (RC38, if it has
  landed) becomes a set of keys the retain set names.
- **Tests:** the rows of `family_sliver_lists` and the lazy-list widget suites unchanged;
  in `flui-rendering`'s tests, `a_keyed_request_builds_the_child_for_its_key`,
  `a_retain_set_drops_only_the_children_outside_it`,
  `two_keyed_children_moved_onto_each_other_are_swapped`.
- **Checks:** clippy and nextest for `flui-rendering`, `flui-view`, `flui-objects`,
  `flui-widgets`.
- **Depends:** RC23; not at the same time as RC38 or RC39 (`sliver_adaptor.rs`) **Size:** L

### RC25. Two-axis viewport

- **What:** W16: `Vicinity`, `TwoAxisLayout`, `RenderTwoAxisViewport<L>`, the uniform-cell
  layout with pinned rows and columns, `TwoAxisScrollView` with `DiagonalDrag`. The first
  step of the widget is the entry in `scroll_owner.rs` that begins, updates and ends one
  contact on two owners under one `FirstPanic`, with its two tests.
- **Owns:** `crates/flui-objects/src/sliver/viewport/two_axis.rs` (new),
  `crates/flui-objects/tests/harness/two_axis_viewport.rs` (new), a keyed element in
  `flui-view`, `crates/flui-widgets/src/scroll/two_axis_scroll_view.rs` (new) and its test
  module; the two-owner entry in `crates/flui-widgets/src/scroll/scroll_owner.rs`.
- **Tests:** R16's harness rows and widget tests, as named there.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`, `flui-widgets`.
- **Depends:** RC24, RC43 **Size:** L

### RC26. ADR: a sliver's per-child transform, and the wheel

- **What:** the ADR of W15 and of the seam of [design.md](design.md), written after RC23 is
  accepted, because a wheel keyed by a signed index on the typed-key seam is one of its
  options. It weighs the two forms with the costs of [design.md](design.md), "Decisions for
  the owner", 2, works the reverse group's arithmetic for the looping wheel before it is
  accepted and names the fallback. It records the notch: `SnapPolicy::step` beside
  ADR-0209 §9's `target`, the one `WheelMotion` target, and the table of packets per host.
  Written when RC31 may start.
- **Owns:** one new file in `docs/adr/`; on acceptance its bullets in `docs/gaps.md`, which
  RC31, RC27 and RC28 remove. **Depends:** RC23 **Size:** M

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
  `ShownChildren` and the seam; its `LazyMultiBoxRender` block and configuration in a new
  file of `flui-view`. The record gains what the wheel is the first to use: a part's
  transform and opacity, `transform_of`, and the two-part rule.
- **Owns:** `crates/flui-objects/src/sliver/wheel/` (new), `crates/flui-objects/src/shown.rs`
  (after RC43), `crates/flui-objects/tests/harness/wheel.rs` (new),
  `crates/flui-view/src/element/sliver_wheel.rs` (new).
- **Tests:** R15's nine harness rows, as named there, and the shared row.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view`.
- **Depends:** RC31, RC43 **Size:** L

### RC28. The wheel widget

- **What:** `ListWheelScrollView`, its controller, `WheelSnap`, the looping wheel in the form
  RC26 decides, and the notch of [design.md](design.md), "One settle rule for a scrollable,
  and a notch that moves one item": the provided `SnapPolicy::step`, the split of
  `wheel_axis_delta` so the axis is chosen once, the packet-to-items function with the
  fraction carried on `WheelMotion`, and `WheelSnap`'s implementation. The record "Wheel
  precision selects local animation policy" of `crates/flui-widgets/ARCHITECTURE.md` is
  rewritten to the whole rule.
- **Owns:** `crates/flui-widgets/src/scroll/list_wheel/` (new) and its test module; the
  method in `crates/flui-widgets/src/scroll/nested/snap.rs`; `WheelMotion`, `wheel_claim`
  and `wheel_axis_delta` in `crates/flui-widgets/src/scroll/scrollable.rs`; that record of
  the `ARCHITECTURE.md`.
- **Tests:** R15's widget tests, as named there; the record's four tests
  (`notched_wheel_accumulates_distance_and_eases_out_in_150ms`,
  `precise_and_unknown_wheels_interrupt_synthetic_motion_once`,
  `replacing_or_unmounting_a_scrollable_retires_its_notched_motion`,
  `dragging_interrupts_notched_motion_and_windows_progress_independently`) unchanged.
- **Checks:** clippy and nextest for `flui-widgets`.
- **Owed on devices:** the packet each host sends for a mouse wheel, a high-resolution
  wheel and a touchpad, and that a browser's wheel scrolls a picker and settles, as rows of
  `platform-parity.toml`.
- **Depends:** RC27 **Size:** M

### RC29. ADR: native views

- **What:** the ADR of W17, after the owner decides that native views enter the contract and
  for which hosts. It answers each row of R17's table and R17's two rules: a view without a
  placement in a frame is hidden with that frame, and a host that cannot embed a view
  answers with a typed refusal, the headless host included. Written when the owner has
  decided and RC30's first task may start.
- **Owns:** one new file in `docs/adr/`; the gap entry "No native views inside the UI" is
  rewritten to point at it, with a bullet per unbuilt section on acceptance.
  **Depends:** owner decision **Size:** L

### RC30. Native views — gated

- **What:** not broken down. When RC29 is accepted this row is replaced by its tasks, the
  texture-backed path first; runs owed on devices become rows of
  `crates/flui-native/platform-parity.toml`.
- **Depends:** RC29

## Tier B, beyond Flutter

### RC33. Anchored placement

- **What:** W23. `AnchoredPlacement`, `PlacementSide`, `NoFit`, `Placed`, `PlacementMemory`,
  `PlacementArea::of`, the one layout delegate over them. `MenuPlacement` and the selection
  toolbar's `ToolbarLayout` are deleted; the popup menu configures the delegate with
  `NoFit::Overlap` and the selection toolbar with its over-and-under order, each holding a
  `PlacementMemory` in its state.
- **Owns:** `crates/flui-widgets/src/overlay/placement.rs` (new) and its test module; the
  placement code of `crates/flui-widgets/src/popup_menu.rs` and
  `crates/flui-widgets/src/text/selection_menu/toolbar.rs`.
- **Tests:** `a_placement_takes_its_preferred_side_when_it_fits`,
  `a_placement_falls_back_in_order`, `a_placement_shifts_to_stay_inside`,
  `a_placement_does_not_flip_back_and_forth_at_the_threshold`,
  `a_placement_keeps_its_side_across_a_rebuild`, `a_placement_mirrors_in_rtl`,
  `a_placement_of_a_child_larger_than_the_area_is_finite`,
  `a_placement_that_shrinks_fits_on_its_roomiest_side`,
  `a_placement_that_overlaps_reports_that_it_covers_its_anchor`,
  `a_placement_stays_above_the_keyboard`; the popup menu's and the selection toolbar's
  placement tests unchanged. `rg 'MenuPlacement'` finds nothing in the repository, and
  `rg 'ToolbarLayout' crates/` finds nothing; `packages/flui-cupertino/src/
  navigation_toolbar.rs` keeps its own type of that name, which is another concept.
- **Checks:** clippy and nextest for `flui-widgets`; clippy and nextest for
  `packages/flui-material` and `packages/flui-cupertino` (users of the menu and the toolbar).
- **Depends:** — **Size:** M

### RC34. First that fits

- **What:** W24. `RenderFirstThatFits`, `FitAxes`, `FirstThatFits`: each alternative laid out
  once per pass, the probe's size the shown size, the last alternative never probed. The
  first step settles whether a mounted child may be skipped by a layout
  ([design.md](design.md), "Unverified").
- **Owns:** `crates/flui-objects/src/layout/first_that_fits.rs` (new),
  `crates/flui-objects/tests/harness/first_that_fits.rs` (new),
  `crates/flui-widgets/src/layout/first_that_fits.rs` (new) and its test module.
- **Tests:** `harness_first_that_fits_shows_the_first_alternative_that_fits`,
  `harness_first_that_fits_lays_each_alternative_out_once_per_pass`,
  `harness_first_that_fits_falls_back_to_its_last_alternative`,
  `harness_first_that_fits_hides_the_others_from_hits_and_semantics`,
  `harness_first_that_fits_on_one_axis_ignores_the_other`,
  `harness_first_that_fits_dry_layout_makes_the_same_choice`; widget test
  `a_layout_builder_in_an_alternative_builds_once_per_pass_and_settles`; the shared row.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC43 **Size:** S

### RC35. A wrap that stops

- **What:** W22. `layout/leading_run.rs`; `RenderToolbarItems` calls it; `RenderWrap` gains
  `max_runs`, the indicator, the reserve with its key (the constraints, the wrap's
  configuration and the main-axis sizes of the children before the indicator) and
  `WrapSplit` with its wake; `Wrap` gains its two builder methods and, with an indicator, a
  state that installs the wake and rebuilds the indicator when the count changed
  ([design.md](design.md), "A value a frame decided reaches a widget on a change"). The two
  records
  that say a wrap cannot hide what does not fit are rewritten: the module documentation of
  `toolbar_items.rs` and the mapping decision "A toolbar row hides…".
- **Owns:** `crates/flui-objects/src/layout/leading_run.rs` (new), `wrap.rs` (after RC1),
  `toolbar_items.rs`; that paragraph of `crates/flui-objects/ARCHITECTURE.md`;
  `crates/flui-objects/tests/harness/wrap_overflow.rs` (new);
  `crates/flui-widgets/src/wrap/` and its test module.
- **Tests:** R22's rows and widget tests, as named there; the shared row; the rows of
  `RenderToolbarItems` unchanged. `rg 'cannot hide'` and `rg 'RenderWrap'` over the two
  records find no sentence that says a wrap cannot hide a child.
- **Checks:** clippy and nextest for `flui-objects` and `flui-widgets`.
- **Depends:** RC43 **Size:** M

### RC36. Tracks

- **What:** W21, the replacement: `layout/tracks.rs` with `TrackSize` and `resolve`.
  `TableColumnWidth` is deleted and every user moves to `TrackSize`; `RenderTable` calls
  `resolve`. No behaviour change. The pull request says why `flui-sdk`'s surface test
  changes: it pins a re-export by name, and the name is the thing replaced.
- **Owns:** `crates/flui-objects/src/layout/tracks.rs` (new), `table_column.rs` (deleted),
  `table.rs`; the table's lines of `crates/flui-objects/tests/render_object_harness.rs` (its
  import and its rows' uses of the type); `crates/flui-widgets/src/layout/table.rs` and
  `crates/flui-widgets/tests/table.rs`; the re-export in `crates/flui-sdk/src/lib.rs` and the
  two lines of `crates/flui-sdk/tests/surface.rs`; `packages/flui-material/src/data_table.rs`
  and `packages/flui-material/tests/data_table.rs`; each document that names the old type;
  the two unreleased fragments that name it, `changelog.d/widgets-one-path.md` and
  `changelog.d/claude-project-thread-wzs1nq.md`, corrected to the new name if they have not
  been released; its own fragment, which names the rename.
- **Tests:** the rows of `RenderTable`, the `Table` widget tests, `flui-sdk`'s surface test and
  the data table's tests green under the new name; the mutation run over `resolve` is caught
  by them, or the task adds the row that catches it. `rg 'TableColumnWidth'` across `crates/`,
  `packages/`, `docs/`, `changelog.d/` and each `ARCHITECTURE.md` finds it only in this spec,
  in the task's own fragment and in released entries of `CHANGELOG.md`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-widgets`, `flui-sdk`,
  `packages/flui-material`, and the facade's own tests.
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

### RC37. The geometry observer

- **What:** W19's ungated half. `GeometryObserver` and `ObservedGeometry`: a private leader
  around the child, a `LeaderWatch` taken in `create_state`, a listener that holds a `Weak`
  to the state's cell and delivers through `commit`. Its first step measures a few hundred
  observers in a list against the perf baselines ([design.md](design.md), "Unverified").
- **Owns:** `crates/flui-widgets/src/observe/` (new) and its test module.
- **Tests:** R19's tests, as named there, except those RC44 names.
- **Checks:** clippy and nextest for `flui-widgets`.
- **Depends:** — **Size:** S

### RC44. The visibility observer — gated

- **What:** W19's gated half. `LeaderGeometry` gains the leader chain's transform and its
  resolved clip, read from the applied `SceneVersion`, so a watch reports a change of either;
  `VisibilityObserver` and `ObservedVisibility` with thresholds and a margin. The record
  "A follower's build hears its leader's geometry after paint" of
  `crates/flui-rendering/ARCHITECTURE.md` is rewritten: what a watch reports, and why a
  watch and not a post-frame check, in terms that hold since ADR-0367. No ADR: the fields
  are additive on a `#[non_exhaustive]` struct and change no decision. Its first step shows
  that the clip a leader's chain resolves equals the clip `presented_placement` reports for
  the same node.
- **Frozen-crate files:** `crates/flui-rendering/src/pipeline/owner/leader_watch.rs`, that
  paragraph of `crates/flui-rendering/ARCHITECTURE.md`; the read in
  `crates/flui-layer/src/scene/version.rs`.
- **Owns:** those and `crates/flui-widgets/src/observe/visibility.rs` (new, after RC37) with
  its tests.
- **Tests:** in `flui-rendering`'s tests
  `a_leader_watch_reports_a_change_of_its_clip_and_transform` and the existing leader-watch
  tests unchanged; widget tests
  `a_visibility_observer_reports_the_fraction_a_viewport_leaves`,
  `a_visibility_observer_reports_only_when_a_threshold_is_crossed`,
  `a_visibility_observer_with_a_margin_reports_before_its_subtree_shows`,
  `a_visibility_observer_reports_zero_for_an_offstage_subtree`,
  `a_visibility_observer_under_a_rotation_reports_its_bounding_fraction`; the selection
  handles' suites unchanged.
- **Checks:** clippy and nextest for `flui-layer`, `flui-rendering`, `flui-widgets`,
  `flui-sdk` (the plugin hook that listens to a watch).
- **Depends:** RC37 **Size:** M

### RC38. Sticky rows

- **What:** W20. `StickyRows`, `sliver/sticky_rows.rs`, the mode on `RenderSliverList` and
  `RenderSliverVariedExtentList`, `.sticky_rows(..)` on their widgets and on `ListView`.
  `LazyMultiBoxRender::kept_outside_band`. The lazy element's manager forms one kept set
  per service pass, from the held children and that index, resolved to resident elements
  before the reconcile, and its reconcile, its eviction and its build admission read it. The
  function that builds a lazy sliver's pass in one order and commits once
  (`sliver/band_parts.rs`) is built here with its band, pinned and sticky steps; the pinned
  step leaves out the index the sticky rule takes.
- **Owns:** `crates/flui-objects/src/sliver/sticky_rows.rs`, `band_parts.rs` (new),
  `sliver_list.rs`, `sliver_varied_extent_list.rs` (after RC8), `virtualized_band.rs`;
  `crates/flui-objects/tests/harness/sticky_rows.rs` (new);
  `crates/flui-view/src/element/sliver_adaptor.rs` (the trait method, the manager's kept
  set and admission, `ListConfig`), `sparse_children.rs` (the reconcile and `retain_band`),
  `sliver_varied_extent_list.rs` (its configuration, after RC8); the paragraph of
  `crates/flui-view/ARCHITECTURE.md` on what the lazy element keeps built;
  `crates/flui-widgets/src/scroll/sliver_list.rs`, `sliver_varied_extent_list.rs`,
  `list_view.rs`.
- **Tests:** R20's rows and widget tests, as named there; the lazy-list and pinned-child
  suites of `flui-view` and `flui-widgets` unchanged.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view` and `flui-widgets`.
- **Depends:** RC8, RC43; not at the same time as RC9 (`virtualized_band.rs`) or RC24
  (`sliver_adaptor.rs`) **Size:** L

### RC39. Item effects

- **What:** W25. `ItemEffect`, `ItemProgress`, `ItemAppearance`, `sliver/item_effect.rs`; the
  effect as the last step of `band_parts.rs`, not applied to a stuck header; the optional
  effect on the four lazy slivers and `.item_effect(..)` on their widgets.
- **Owns:** `crates/flui-objects/src/sliver/item_effect.rs` (new), `band_parts.rs` (after
  RC38), `sliver_list.rs`, `sliver_varied_extent_list.rs`, `sliver_fixed_extent_list.rs`,
  `sliver_grid.rs`; `crates/flui-objects/tests/harness/item_effect.rs` (new); the
  configurations in `crates/flui-view/src/element/sliver_adaptor.rs` and
  `sliver_varied_extent_list.rs`; the builder methods in `crates/flui-widgets/src/scroll/`.
- **Tests:** `harness_item_effect_scales_an_item_at_the_viewport_edge`,
  `harness_item_effect_is_hit_through_its_transform`,
  `harness_item_effect_leaves_the_scroll_extent_unchanged`,
  `harness_item_effect_with_a_non_finite_transform_hides_the_item`,
  `harness_item_effect_leaves_a_stuck_header_in_place_over_its_rows`,
  `harness_the_fixed_extent_list_and_the_grid_without_an_effect_paint_and_hit_as_before`,
  `harness_a_sliver_without_an_effect_maps_its_children_as_before`; widget test
  `scrolling_a_list_with_an_item_effect_rebuilds_nothing`.
- **Checks:** clippy and nextest for `flui-objects`, `flui-view` and `flui-widgets`.
- **Depends:** RC31, RC38; not at the same time as RC24 **Size:** M

### RC40. Blend group and mask — gated

- **What:** W26 as RC16 decides it: the group layer, its lowering, the blend mode on
  `PaintOpacity`, `RenderBlendGroup`, `RenderMask`, `BlendGroup`, `Mask`. The records that
  state the composite are rewritten.
- **Frozen-crate files:** a new layer file in `crates/flui-layer/src/` and its registration;
  the lowering in `flui-engine`'s layer renderer; `crates/flui-rendering/src/traits/
  paint_effects.rs` and where effects are lowered to the scene; the "`blend` on
  `PaintOpacity`" row of "Deferred producers" in `crates/flui-rendering/ARCHITECTURE.md`.
- **Owns:** those, `crates/flui-objects/src/proxy/blend_group.rs`, `mask.rs` (new),
  `crates/flui-objects/tests/harness/blend_group.rs` (new),
  `crates/flui-widgets/src/paint/blend_group.rs`, `mask.rs` (new).
- **Tests:** `a_blend_group_blends_as_a_whole_with_what_is_behind_it`,
  `a_half_transparent_blend_group_composites_once_against_the_page`,
  `children_of_a_blend_group_do_not_blend_with_the_backdrop`,
  `a_mask_multiplies_its_content_by_the_masks_alpha`,
  `a_mask_at_a_fractional_offset_leaves_no_edge`, `a_masked_subtree_is_hit_where_its_content_is`,
  `a_mask_child_is_not_hit_or_published`; a perf scenario held by
  `flui_testing::perf::Baselines` for one group.
- **Checks:** clippy and nextest for `flui-layer`, `flui-engine`, `flui-rendering`,
  `flui-objects`, `flui-widgets` (readback suites at one test thread).
- **Owed on devices:** the readback rows run on lavapipe; a blend group and a mask on each
  host's GPU backend are rows of `platform-parity.toml`.
- **Depends:** RC16, RC17 **Size:** L

## Validation evidence

Empty until a task lands. Each task records here the final names of its tests, the commands it
ran with their results, and the pull request.
