# render-object-coverage: design, Flutter parity (W1–W18)

- **Status:** Approved by the owner on 2026-11-25 (the decisions in design.md stand as recommended); tasks in progress, see tasks.md
- **Date:** 2026-11-25
- **Base:** `main` @ `fcab477`
- **Overview, shared seams, ADR list, decisions:** [design.md](design.md); requirements:
  [requirements.md](requirements.md); tasks: [tasks.md](tasks.md)

One section per work item. The conventions of [design.md](design.md) hold: existing code was
read at `fcab477`, a new name is marked *proposed* where it first appears, and "the shown
answer", "the offset index" and "the sliver transform seam" are the seams of that file.

## W1. The harness reads its families from more than one file

`crates/flui-objects/tests/render_object_harness.rs` is one test target (`autotests = false`).
Its guard `catalog_covers_every_render_object_name` reads the root with
`include_str!("render_object_harness.rs")` and the family files through `FAMILY_FILES`, and
counts a name only inside a row. Most tasks of this spec add rows; with one file they would all
own it.

- **Shape.** Family modules under `crates/flui-objects/tests/harness/`, each mounted from the
  root by two consecutive column-0 lines, `#[path = "harness/<family>.rs"]` and `mod <family>;`,
  as `harness_snapshot.rs` is. `FAMILY_FILES` is `family_files![a, b]`: the macro takes the names
  alone and expands each to `(stringify!(name), include_str!("harness/<name>.rs"))`, so the text
  the guard counts is the file the name mounts. The expansion also imports the module
  (`use self::<name> as _`): a compile-time backstop, held by the compiler, for a mount the text
  check reads as present but the compiler does not build at the top level (a pair written at
  column 0 inside a multi-line string, a macro invocation's body or a `#[rustfmt::skip]` item;
  rule 3). A row has no such backstop. No test reaches the import: a case that must not compile
  needs a compile-fail suite, which `flui-objects` does not have, and a trybuild target for one
  macro defined inside a test file would need the macro moved out of it. Existing rows stay
  where they are; only new families use files of their own.
- **What a row looks like in a family file.** The root's `family_*` table calls it, so it is
  `pub(super) fn harness_*`; the function the root calls in `harness_snapshot.rs` is `pub fn`
  for the same reason. A row nothing calls is dead code under `-D warnings`.
- **The guard** is `rows_naming(root, families, type_name)`, which the tests call with the real
  sources and with literal text:
  1. *A row* starts at a column-0 line that opens `fn harness_` under any visibility (`pub`,
     `pub(crate)`, `pub(super)`) and that the compiler builds (`compiled_at_top_level`, the
     rule a mount pair also meets, rule 3). A line's code (`code_of`) is its text before the
     first `//`, without the block comments that open and close on it. A row whose opening
     line's code ends with `}` (a one-line row such as `fn harness_x() {} // to come` or
     `fn harness_x() {} /* to come */`) is that line alone; any other row is its opening line
     and the lines after it up to, not including, the first that starts at column 0 with `}`.
     When a signature leaves no room for its `{`, rustfmt puts the brace on the next line, at
     column 0 when the signature fits its line and after one space when the signature itself
     is longer; either line belongs to the row. Line ends are read without `\r`, so a CRLF
     source ends its rows where an LF source does.
  2. *A name* counts only as a whole identifier (`names_whole_identifier`) in the code of a
     row's lines: not preceded or followed by a letter, a digit or an underscore, and not in a
     comment (a row's lines below a block comment left open are not read). A row that names
     `RenderSliverFloatingHeaderBase` then does not cover `RenderSliverFloatingHeader`, and
     `// RenderX is covered elsewhere` covers nothing. A name inside a string literal counts:
     by convention a row that names a type in a string, such as an assertion message,
     exercises it.
  3. *A file* counts only when it is mounted (`is_mounted`): its two mount lines are consecutive
     at column 0 and the first is compiled (`compiled_at_top_level`). The rule, shared with
     rows, refuses an item whose nearest code above (blank lines skipped) does not end an item
     (`;` or `}`) or is an attribute (starts with `#[`, a one-line `#[attr] item;` included),
     and every item below the first line that opens a block comment which does not close on
     that line (`first_block_comment`; an opener with another inside it before its closer is
     read as open, since block comments nest); the catalog's failure names that line. An item
     on the opener's own line counts, read up to the opener; mount lines are matched whole, so
     a mount line never carries one.
     *Limits of a text guard*, which the root and the family files keep to. Column 0 is read
     as the top level of the file: rustfmt, which CI's format check holds, indents the items
     inside an item, but not the contents of a string literal, a macro invocation's body or a
     `#[rustfmt::skip]` item, so a row or mount line written at column 0 inside one of those is
     indistinguishable from code; literal guard input is written on one line with `\n`
     escapes. Strings are not told apart from code: a `/*` inside a string that its line does
     not close reads as an opener (every row below stops counting, and the catalog names the
     line), so a string spells it through `BLOCK_COMMENT_OPENER`; a `//` inside a string reads
     as a line comment, which refuses the item right below and hides a block comment opened
     after it on the line. The rule that keeps both out: no string in a code line of the root or
     a family file holds a `/*` its line does not close, and none holds `//` followed later on
     its line by a block comment.
     The directory is `FAMILY_DIRECTORY`, which `mount_lines` and `family_directory()` read
     and the default arm of `family_files!` spells as a literal for `include_str!`
     (`the_family_macro_reads_the_family_directory` holds the two equal).
     `family_files_are_mounted_and_listed` reads the directory (`family_directory()`: the
     run's `CARGO_MANIFEST_DIR` first, so a relocated nextest archive finds the sources, the
     compile-time path as the fallback) and fails for a file not in `FAMILY_FILES`, a listed
     name with no file, and a listed file that is not mounted. A directory that does not exist
     is the empty set (git stores no empty directory); one that cannot be listed for another
     reason fails with its path; the stems are sorted. CI's unmounted-tests step globs
     `tests/*.rs` only, so this test is the rung for the subdirectory (ADR-0303: a test of the
     owning crate).
- **The guard is tested on text**, so the change has tests that fail without it before any
  family file exists: `the_guard_counts_a_pub_row_of_a_family_file` (visibilities, CRLF),
  `the_guard_ends_a_row_at_its_own_end` (one-line rows, with and without a line or block
  comment after them, a closing `}` with a comment or trailing whitespace, both of rustfmt's
  brace lines below a wrapped signature, a name only in a comment against a name in a string),
  `the_guard_skips_a_row_the_compiler_does_not_build` (an attribute over it, a block comment
  opened at a line's start, after code or nested, against one closed on its line and a row on
  the opener's own line), `the_guard_refuses_a_name_inside_a_longer_one`,
  `the_guard_refuses_a_family_file_that_is_not_mounted` (every refused placement, the block
  comments among them, and each message of `mount_problems`),
  `the_mount_check_takes_a_missing_directory_as_no_files`,
  `the_mount_check_refuses_a_directory_it_cannot_list` (their scratch directories are
  `tempfile::tempdir()`, removed with their contents on drop), `the_mount_check_sorts_the_stems`
  (over an unsorted list, through `rs_stems`), `a_family_entry_is_the_file_its_name_mounts`
  (the pair the macro expands to; the import is the compiler's, above),
  `the_family_macro_reads_the_family_directory` and
  `coverage_map_lists_every_render_object_type` (the header's coverage map names each type of
  `RENDER_OBJECT_TYPES` once, so the map is checked against the list instead of copied).
- **What each later task still edits in the root.** One line in `RENDER_OBJECT_TYPES`, and the
  first task of a family adds its two mount lines, its name in `family_files![..]` and its
  `family_*` test. These are one-line, order-insensitive edits; a rebase resolves them
  mechanically.
- **Document.** `docs/testing.md` and the crate README link `RENDER_OBJECT_TYPES` and carry no
  count of it (AGENTS.md, "One source per fact").

## W2. The shown answer, starting with the overflow clip

`layout/stack.rs` (two objects), `wrap.rs`, `fitted_box.rs`, `animated_size.rs` and
`constraints_transform_box.rs` each keep `has_visual_overflow: bool`, reset it at the start of
`perform_layout`, and clip in `paint` when it is set and the clip behaviour is not `Clip::None`.
None overrides `describe_approximate_paint_clip`, so semantics and input regions are not cut.
Each also has a public `has_visual_overflow()` accessor, which `flui-testing` reads
(`crates/flui-testing/src/widgets.rs`, `has_visual_overflow`) and two documents of
`flui-rendering` name.

- **Shape, first task.** The `shown` module of [design.md](design.md), "The shown answer", is
  built here with `OverflowClip`. The six objects each hold one. Their `lay_out` returns the
  size with "a child overflows" as a value, and `perform_layout` is `begin`, `lay_out`,
  `commit`; `paint` clips only when `clip(behavior, size)` is `Some`, and
  `describe_approximate_paint_clip` returns the same value for every child. The bool field
  and its reset are deleted from the six in the same change.
- **The public accessor stays.** `has_visual_overflow()` on each object returns the record's
  bool. Its meaning is unchanged where a layout succeeds; after a layout that fails it
  reports `true`, the state `begin` wrote, where today it reports whatever the failed pass
  reached. `flui-testing` and the documents that name the accessor stay true and are not
  edited.
- **Shape, second task.** `ShownChildren` lands with every lazy sliver `main` has:
  `RenderSliverCarousel` moves from its private `Shown` table, and `RenderSliverList`,
  `RenderSliverFixedExtentList` and `RenderSliverGrid` stop asking
  `PinnedChildren::is_pinned_slot` in `paint` and `hit_test`. Each announces the slots
  `PinnedChildren::lay_out` returns; `slots`, `is_pinned_slot` and `clear_slots` are deleted.
  No behaviour changes (their rows and ADR-0265's tests are the check). The carousel's
  `describe_semantics_clip` keeps returning `SemanticsClip::Bounds` for an item's mask.
- **What an overflow clip does to semantics** is what any authored clip does
  ([design.md](design.md), "Which clip hook"): the six objects report through the paint-clip
  hook, as `RenderClip` does, and add no semantics-clip override. R2 holds that by comparison,
  not by a rule of its own: an object with an overflow clip publishes what the same children
  under a `RenderClipRect` of the box publish, alone and under a scrollable.
- **Hit testing** is unchanged and now stated: an overflowing child is hit only inside its
  parent's box, clipped or not (R2, #19445).
- **Breaks.** None in public API: the fields are private, the accessors stay and
  `PinnedChildren` is private to the `sliver` module. One visible change of behaviour: after a failed layout
  `has_visual_overflow()` is `true` and a lazy sliver's children are neither hit nor
  published until a layout succeeds.
- **Alternatives** are those of the seam ([design.md](design.md)).
- **Mapping decisions to record:** the contract of the shown answer with its three states
  (rewriting "Lazy slivers lay out a pinned child outside the band"); the hit rule of an
  unclipped overflow.

## W3. The sliver pass-through base and the maximum paint rectangle

Six slivers pass layout through to one child by hand (`sliver_semantics.rs`,
`sliver_ignore_pointer.rs`, `sliver_opacity.rs`, `sliver_offstage.rs`,
`sliver_animated_opacity.rs`, `sliver_constrained_cross_axis.rs`): `Arity = Single`,
`ParentData = SliverPhysicalParentData`, `SliverGeometry::ZERO` without a child, otherwise the
child's geometry, and a hit test at the child's layout offset. Five lay the child out under
their own constraints; `sliver_constrained_cross_axis.rs` rewrites `cross_axis_extent` for the
child and sets `geometry.cross_axis_extent`. W4, W5 and W11 add five more.

- **Shape.** A private module `sliver/pass_through.rs` *(proposed)* with the two rules as
  functions the proxies call from their own `perform_layout` and `hit_test`. The layout
  function takes the constraints to give the child as a parameter, so the cross-axis proxy
  passes its narrowed ones and adjusts the returned geometry itself.
- **`MaxPaintRect`** *(proposed, `pub(crate)`, `sliver/max_paint_rect.rs`)*: the rectangle W4
  and W5 need, computed from the constraints and the committed geometry at the end of layout.
  Its rule, transcribed from Flutter's `RenderSliver.getMaxPaintRect`
  (`rendering/sliver.dart`):
  - empty for zero geometry;
  - main-axis length is the geometry's maximum paint extent, and where that is infinite,
    `constraints.scroll_offset + geometry.cache_extent + constraints.cache_origin` (the
    geometry's cache extent, which is finite), so the rectangle is finite;
  - the leading edge is at minus the scroll offset clamped to
    `0 ..= scroll_extent - max_scroll_obstruction_extent`;
  - the cross-axis length is `geometry.cross_axis_extent` where the geometry states one, and
    the constraints' cross-axis extent otherwise;
  - for the axis directions left and up (after the growth direction is applied) the rectangle
    is flipped about the paint extent.
- **Migration.** The six existing proxies move to the functions in the same change, with no
  behaviour change (their harness rows are the check).
- **Alternative: a generic `RenderSliverProxy<B>` with a behaviour trait,** as `RenderClip<S>`
  is generic over its shape. The six objects differ in which hooks they override (paint
  effects, semantics, hit testing, cross-axis constraints), so the behaviour trait would
  re-declare most of `RenderSliver` with defaults, and every public name would become an alias
  the export guard must special-case. Rejected: two shared rules do not pay for a second trait.
- **Alternative: leave them hand-written.** Eleven copies of the childless rule. Rejected.

## W4. Sliver clips

- **Shape.** `RenderSliverClip<S: ClipGeometry>` *(proposed, `sliver/sliver_clip.rs`)*, reusing
  the sealed `ClipGeometry` of `proxy/clip.rs`, with aliases `RenderSliverClipRect =
  RenderSliverClip<Rect<f64>>` and `RenderSliverClipRRect = RenderSliverClip<RRect>`, the shape
  parameters `RenderClipRect` and `RenderClipRRect` use. `Arity = Single`, `ParentData =
  SliverPhysicalParentData`. `ClipOverlapBehavior { None, FollowEdge, PreserveShape }`
  *(proposed, `#[non_exhaustive]`)* beside it; the default is `FollowEdge`, as in Flutter. The
  generic root joins the three the export guard already excludes.
- **Layout.** Pass-through (W3), then the clip is committed as the object's shown answer: the
  shape built for `MaxPaintRect`, with its leading edge moved to the overlap origin when the
  behaviour is not `None`. Flutter's `getClipOriginForOverlap(inside_extent)`, transcribed:
  with `flexible = max(0, inside_extent - max_scroll_obstruction_extent)`, the origin is
  `flexible - scroll_offset` clamped to `-min(flexible, scroll_offset) ..= max(0, overlap)`.
  `inside_extent` is the clip's main-axis extent for `FollowEdge` and, for `PreserveShape` on a
  rounded clip, the extent of its middle rectangle (the part between the corners), which is
  what makes the corners slide instead of being sheared. The moved shape keeps its radii under
  both. For a rectangle the two behave alike. The behaviour tests of R3 are the contract, not
  this formula.
- **Paint.** `ctx.with_clip_rect` or `with_clip_rrect` around `paint_child`, from the committed
  shape; nothing when the child is invisible or the clip is empty.
- **Hit test.** A point outside the committed shape is rejected before the child is asked (the
  rounded shape's containment, not its bounding rectangle).
- **Semantics, regions.** `describe_approximate_paint_clip` returns the committed shape's
  bounding rectangle, and the seam's rule for an authored clip follows
  ([design.md](design.md), "Which clip hook").
- **Seams between neighbours.** R3's readback row draws two clipped slivers of one colour at
  fractional scroll offsets and samples the boundary row. The object does not snap: ADR-0098
  §6 puts snapping in `flui-engine`, in raster-target coordinates, on edges and only under a
  translation and a positive axis-aligned scale, and a render object knows none of that. A
  seam the row shows is a defect of the engine's clip snapping (two clips that share a
  logical edge must snap it to the same device row); it is fixed there, as a defect fix the
  freeze admits, or listed in `docs/gaps.md` with the failing row.
- **Widgets.** `SliverClipRect::new(sliver)` and `SliverClipRRect::new(border_radius, sliver)`
  with `.clip_behavior(..)` and `.overlap_behavior(..)`
  (`crates/flui-widgets/src/scroll/`). Flutter's custom `clipper` is not offered for slivers in
  this spec: `RenderClip<S>` has one for boxes, and no report asks for the sliver form.
- **Shape versus rectangle.** `ClipGeometry::default_for_size` builds a shape at the origin; the
  sliver clip builds it for the clip's size and applies it under the clip's origin. If that
  needs a method on `ClipGeometry`, the trait is in `flui-objects` and sealed, so the method is
  added there.
- **Alternative: two hand-written objects,** as Flutter has. They differ only in the shape;
  `RenderClip<S>` already made that a parameter. Rejected.
- **Alternative: a boolean `clip_overlap`,** the form of Flutter's first proposal (PR #179003).
  It cannot say what a rounded corner does at the moving edge. Rejected.

## W5. Decorated sliver

- **Shape.** `RenderDecoratedSliver` *(proposed, `sliver/sliver_decorated.rs`)* with a
  `BoxDecoration`, a `DecorationPosition` (the enum of `proxy/decorated_box.rs`) and
  `anti_alias`, pass-through layout, and a paint that calls `decoration.paint(ctx.canvas(),
  rect, DecorationPaintOptions::with_anti_alias(..))` over `MaxPaintRect`, before or after the
  child. Hit testing and semantics are the child's.
- **Why `MaxPaintRect` and not the paint extent.** The rectangle starts at minus the scroll
  offset and has the whole scroll extent, so a rounded corner is at the sliver's real end
  (#107498), and the overlap and both axes are in its rule. A pinned child changes the
  geometry's obstruction, which the clamp of the leading edge reads (#179801).
- **Shadows.** The decoration paints inside the viewport's clip. The scroll view's
  `clip_behavior` is the caller's switch (R4, #156819); the object adds none.
- **Widget.** `DecoratedSliver::new(decoration, sliver).position(..)`.
- **No alternative considered:** the object is `RenderDecoratedBox` with a different rectangle.

## W6. Composition widgets and the overflow bar

- **`BlockSemantics`** *(proposed, `crates/flui-widgets/src/semantics/`)* builds a `Semantics`
  with `block_previous_siblings`. No render object: ADR-0226 made the barrier a property of the
  configuration, and `RenderSemanticsAnnotations` carries it. The sentences "no
  `BlockSemantics`" in `crates/flui-widgets/src/navigator/page_route.rs`,
  `crates/flui-widgets/tests/modal_route.rs` and `packages/flui-material/src/drawer.rs` are
  corrected in the same change. Removing the barrier republishes the earlier siblings; R9's
  restore test holds that no announcement is made for nodes that were live before.
- **`SliverVisibility`** *(proposed, `scroll/sliver_visibility.rs`)* composes `SliverOffstage`,
  `SliverOpacity` (whose render object has `always_include_semantics`), `SliverIgnorePointer`
  and a replacement sliver, with the builder methods `Visibility` has (`maintain_state`,
  `maintain_animation`, `maintain_size`, `maintain_semantics`, `maintain_interactivity`).
  Flutter has a private render object for it; FLUI has every part as a sliver proxy already.
- **`RenderOverflowBar`** *(proposed, `layout/overflow_bar.rs`)*, `Arity = Variable`,
  `ParentData = BoxParentData` (what `RenderWrap` uses). Children are laid out unconstrained in
  width; if their widths plus spacing fit the maximum width they form a row aligned by the
  main-axis alignment, otherwise a column with `overflow_spacing`, `overflow_alignment` and a
  vertical direction. Intrinsics follow the same branch (the row's when it fits at that width),
  and the dry layout is the layout without placement. No baseline. Widget `OverflowBar`.
  Flutter keeps this object private; FLUI exports it because `flui-widgets` cannot reach a
  private type of another crate.
- **A flex child under an overflow bar** (#188272). The parent data type is `BoxParentData`, so
  a widget that contributes `FlexParentData` (`Expanded`, `Flexible`, `Spacer`) meets the
  composition diagnostic `flui-view` raises when a `ParentDataView`'s type differs from what
  its render parent stores (`crates/flui-view/src/tree/element_tree/mounting.rs`, "the same
  failure in debug and release"), naming the widget and the parent it belongs under. Nothing
  is added; R9's test pins it for the bar.
- **Alternative: `RenderWrap` with one run and a fallback direction.** A wrap breaks a row into
  several runs; the bar switches the whole axis. Two rules in one object. Rejected. The wrap
  that stops (W22) and the bar stay separate for the same reason.

## W7. `RenderImage` paint properties

`RenderImage::paint` calls `self.placement.paint(ctx.canvas(), rect, image, natural, None,
None)`; the two `None`s are `filter: Option<ColorFilter>` and `paint: Option<&Paint>`. `Image`
has the builders `fit`, `alignment`, `repeat`, `center_slice`, `placement`, `width`, `height`
and `gapless_playback`, and none for colour, blend mode, opacity, inversion or filter quality.

- **Ungated half.** `color: Option<Color>` with `color_blend_mode` becomes `ColorFilter::Mode`
  in the `filter` argument; `opacity` becomes the alpha of the `Paint` argument. Both are
  setters on `RenderImage` that mark paint, and builder methods on `Image`. The default blend
  mode is source-in, as in Flutter, so a tint keeps the image's transparent pixels (#12990);
  the builder's documentation names the modes that fill them.
- **Gated half.** `invert_colors` must apply after the tint. `ColorFilter` has `Mode`, `Matrix`
  and the two gamma variants and no composition, and a tint in an arbitrary blend mode followed
  by an inversion is not one `Matrix`. `filter_quality` is carried by `DrawOp::Texture` but not
  by the image ops, and the engine samples images linearly. Both need `flui-painting` (a
  composed `ColorFilter`, and `filter_quality` on the `#[non_exhaustive]`
  `ImageRegionPlacement` payload, which keeps `DrawOp` within its asserted size) and
  `flui-engine` (the sampler). They ship together after the freeze, so `RenderImage` never has
  a property that works alone and fails in combination.
- **Intrinsics before the image resolves** (#32116): R10's row runs first; if `RenderImage`
  answers a non-finite intrinsic today, the fix is in the ungated half.
- **Centre slice in a small destination** (#27827): the first step of the ungated task runs the
  case against `Image::center_slice` and records the result under the task; a defect found is
  fixed there or listed in `docs/gaps.md`.
- **Alternative for inversion: wrap the image in W13's subtree filter.** It allocates a layer
  for what is a per-draw colour operation. Rejected unless the composed `ColorFilter` is
  refused.
- **Breaks.** None; new setters.

## W8. Varied-extent list

- **Shape.** `RenderSliverVariedExtentList` *(proposed, `sliver/sliver_varied_extent_list.rs`)*
  joins `LazyMultiBoxRender` (`Arity = Variable`, `ParentData =
  SliverMultiBoxAdaptorParentData`). In `flui-view` that is not a line: each of the four lazy
  slivers has its `LazyMultiBoxRender` implementation and its configuration type as a block
  of `crates/flui-view/src/element/sliver_adaptor.rs` (2,159 lines). The new list's block and
  its `pub type SliverVariedExtentList = SliverMultiBoxAdaptor<RenderSliverVariedExtentList>`
  go in a file of their own beside it. Its configuration holds an
  `Rc<dyn ItemExtentDelegate>`.
- **The delegate** *(proposed, `sliver/item_extent.rs`, in `flui-objects`)*:

  ```rust
  pub trait ItemExtentDelegate {
      fn extent(&self, index: usize, dimensions: &ItemExtentDimensions) -> f64;
      /// False when `extent` ignores the dimensions, so a resize asks nothing again.
      fn depends_on_dimensions(&self) -> bool { true }
      fn should_relayout(&self, old: &dyn ItemExtentDelegate) -> bool;
  }
  ```

  the shape of `SliverGridDelegate`. `ItemExtentDimensions` *(proposed, `#[non_exhaustive]`)*
  has the viewport main-axis extent and the cross-axis extent, and deliberately not the scroll
  offset or the preceding scroll extent Flutter's `SliverLayoutDimensions` has: with them an
  answer changes on every scroll and cannot be kept.
- **Offsets and layout.** The offset index of [design.md](design.md): a `Virtualizer` driven
  through `walk_virtualizer_band` with the authored extent source. For an index the walk
  reaches that the index has not been told, the source asks the delegate, validates the
  answer (finite, not negative; otherwise a layout error naming the index) and stores it. The
  child is laid out tight on the main axis at the stored extent, read back with `extent_of`,
  never recovered from two offsets and never replaced by the child's own size.
- **Every writer of the index** ([design.md](design.md), "One owner, every writer"). A change
  of delegate (`should_relayout`), or of dimensions when `depends_on_dimensions` is true, is
  `invalidate_from(0)`. The lazy element's `set_item_count` is the count's one source, and
  the walk's `set_count` at the start of a pass repeats it. An index whose builder answers
  `ChildLayout::NoChild` ends the list there, as it does for `SliverList`: the count the
  element reports next is the truth. The adaptive default estimate applies only to items
  not yet asked. The source re-stores the first visible item each pass, which hands the
  walk's anchor to the index and changes no extent.
- **Readers.** The list is born on `ShownChildren`: its band children are parts and its
  ADR-0265 pinned slots are announced. It has no `is_pinned_slot` call to remove.
- **The count, and how much one layout asks.** `LazyMultiBoxRender::create` takes a `usize`,
  and an `ItemCount::Unknown` source that probes as endless arrives as `usize::MAX`.

  | Count | A layout asks | Scroll extent |
  |-------|---------------|---------------|
  | Known, at most `EAGER_EXTENT_LIMIT` | Every index not yet answered, so all of them in the first layout and after an invalidation | `Extent::Exact` from that layout on |
  | Known, above the limit | Its window; the rest are estimated at the mean of the answers | `Extent::Estimated` until all are asked; an anchor correction keeps the visible items in place |
  | Unknown or endless | The same, and the window is bounded | Estimated |

  `EAGER_EXTENT_LIMIT` *(proposed, in `item_extent.rs`)* is a budget of delegate calls in one
  layout. It is a constant of this list, set by a perf scenario held by
  `flui_testing::perf::Baselines` (ADR-0396), 10,000 until the scenario exists. It is not
  `MAX_UNBOUNDED_WINDOW_CHILDREN`: that constant (1,000,000, `sliver_grid.rs`) bounds a window
  whose end is unbounded, its documentation says it is "deliberately NOT a 'large grids are
  too slow' budget", and it stays where it is with that meaning.
- **What a resize costs.** With a delegate that depends on the dimensions, a change of
  dimensions invalidates every answer, and each layout of a continuous resize asks again: at
  most `EAGER_EXTENT_LIMIT` calls in the eager case, the window otherwise. That is the bound
  the limit exists to set: the perf scenario sizes it so that one eager layout fits its
  stated share of a frame, and a resize frame is one such layout. A delegate that returns
  `false` from `depends_on_dimensions` (a table of fixed heights) is asked once per item for
  its life.
  - *Alternative: ask only the window while the dimensions change and fill when they
    settle.* Cheaper frames, but the fill needs a layout nothing asks for once the resize
    stops, and the extent is estimated for as long as the user drags. Rejected while the
    limit holds the cost.
  - *Alternative: keep the stale answers and refine the window.* Rows outside the window
    then sit at extents authored for other dimensions, and the "exact" extent is wrong.
    Rejected.
- **An unbounded main axis** uses `bounded_unbounded_window` as the measured list does, which
  is also the bound when every extent is zero.
- **Why the eager case exists.** With every extent known, the scroll extent is exact, the
  scrollbar thumb does not move under the user, and a jump to an offset lands exactly. That is
  why a caller picks this list over `SliverList`. Above the limit the list says in its
  documentation that its extent is provisional.
- **What the walk already does.** The window, child requests (`request_child_build`), the
  retain band (`emit_retain_band`), pinned children (`PinnedChildren`, ADR-0265) and the
  placement hand-off. The fixed-extent list keeps its own arithmetic walk; nothing is
  extracted from it.
- **Placement.** `set_placement` is implemented by `RenderSliverList` and `RenderSliverGrid`
  only. The varied-extent list implements it too, through the walk's `placed` output, so an
  animated list can use authored extents.
- **Widgets.** `SliverVariedExtentList` (builder and delegate) and a `ListView` constructor
  beside `ListView::builder`.
- **Mapping decisions to record:** the two extent sources of the walk (the record "A lazy
  list records only measured extents" is rewritten); the delegate's dimensions omit the
  scroll offset; the three count cases and the eager limit.
- **Alternative: lay the child out loose and compare its size with the answer.** The walk
  unchanged, and an authored extent that a child can contradict on every layout. Rejected.
- **Alternative: keep Flutter's dimensions and walk from zero.** O(n) per layout (#144390).
  Rejected.
- **Alternative: accept only a known count.** Simple and always exact, and it refuses the
  endless feed a varied-extent list is often wanted for (PR #178021). Rejected.
- **Alternative: a second index of prefix sums.** [design.md](design.md), "The offset index".
  Rejected.

## W9. Tree sliver

Flutter's `RenderTreeSliver` extends the varied-extent list, keeps a map of active animations
keyed by a parent index, applies the animation's offset in its index arithmetic and pushes a
clip layer per animation in paint. Its tracker (R6) is a list of the orders of events that map
gets wrong.

- **Shape.** `RenderTreeSliver` *(proposed, `sliver/sliver_tree/`)* joins `LazyMultiBoxRender`
  (its implementation and configuration in a file of their own in `flui-view`, as W8's) and
  holds the offset index of W8. Flutter extends the list by inheritance; here both objects
  drive the same walk with the authored extent source, the tree's adding the indent.
- **One owner.** The widget's state owns the expansion state of every node, the flattened
  roster of rows, and one clock per running toggle. The render object owns one thing across
  layouts, the offset index, a cache of the extents of the roster snapshot it holds. It holds
  no expansion state and no animation, and "Every writer of the index" below is the whole
  list of what changes it.
- **The roster** *(`flui-widgets`)*: the visible nodes in depth-first order, each with its
  depth. It is eager over *expanded* nodes only: a node's children are asked from the
  application's node source when the node expands, and row widgets are built lazily by the
  sliver's element, each under a key made from its node's id so that a row keeps its state
  when rows are inserted above it ([design.md](design.md), "Unverified"). While a block
  collapses its rows stay in the roster as ghosts; when the clock ends they leave it.
- **What the render object reads is a snapshot.** The delegate is a value of the roster at
  one revision, and its answers never change for its life: an edit makes the roster's next
  value (the rows are shared and copied once per edit while a snapshot is held, the cost a
  `Vec` insert already has) and the object reads the one it holds until `update` hands it
  the next. No answer can come from a revision the index has not applied.

  ```rust
  /// One revision of a tree's visible rows. An implementor is immutable.
  pub trait TreeRowDelegate {
      fn len(&self) -> usize;
      fn extent(&self, index: usize, dimensions: &ItemExtentDimensions) -> f64;
      fn depends_on_dimensions(&self) -> bool { true }
      fn depth(&self, index: usize) -> usize;
      fn revision(&self) -> RosterRevision;
      /// The edits after `since`, oldest first; `None` when `since` is of another roster or
      /// the log no longer reaches it.
      fn edits_since(&self, since: RosterRevision) -> Option<Vec<RosterEdit>>;
  }
  ```

  `RosterRevision { roster: RosterId, edit }` *(proposed)* is typed per roster: `RosterId`
  comes from a private `AtomicIdMint`, as `OverlayEntryId` does
  (`crates/flui-widgets/src/overlay/entry.rs`), and the edit number from the roster's own
  `IdMint`, so a replaced roster cannot answer for the old one's number. A changed extent
  rule, node source or controller is a new roster, started in `did_update_view`; there is no
  `should_relayout`, and one channel carries every reason to reset. The log is bounded and
  holds `RosterEdit { at, removed, inserted }` *(proposed)*, one per expand or per collapse
  that ended.
- **Every writer of the index** ([design.md](design.md), "One owner, every writer").
  1. `update(config)`. Edits since the applied revision are applied as
     `Virtualizer::splice`, in order, so several toggles in one build are several splices,
     and each returned correction joins the sliver's pending scroll correction: the row at
     the top of the window stays where it is. `None` is a reset: `set_count(len)` and
     `invalidate_from(0)`. Afterwards the index's length must equal the snapshot's `len()`;
     a mismatch is a reset with a diagnostic, never an index used at the wrong length.
  2. The authored source, in the walk: `set_measured` for a row not yet asked, and the
     re-store of the first visible row that hands over the anchor.
  3. A change of dimensions when `depends_on_dimensions` is true: `invalidate_from(0)`.
  4. The walk's `set_count` at the start of a pass. The tree passes its snapshot's `len()`,
     which `update` has already made the index's length, so it changes nothing.
  5. The lazy element's `set_item_count`, its clamp included. The tree ignores it: the
     count's one source is the snapshot, which the same build produced.
  6. The walk's truncation on `ChildLayout::NoChild`. The tree's row builder is total over
     the snapshot it was built with, so the answer is a defect: the band ends there for the
     pass with a diagnostic, and the next pass starts again from `len()`.
  7. The adaptive default estimate, which moves only rows not yet asked.
- **What the render object is given.** Its `Config` holds:
  - `Rc<dyn TreeRowDelegate>` *(proposed)*, above. The lazy family's parent data type is
    fixed by the trait, so a row's depth cannot live there as Flutter's
    `TreeSliverNodeParentData.depth` does;
  - `TreeIndentation` *(proposed)*: a newtype over logical pixels per level with `try_new`
    refusing a negative or non-finite value. Flutter's `TreeSliverIndentationType` (standard,
    none, custom) becomes two constants and the constructor;
  - a bounded list of `TreeReveal { rows: Range<usize>, progress }` *(proposed)*: the rows of
    one animating block and the animation whose value in `0.0 ..= 1.0` says how much of the
    block is uncovered. The object samples `progress` in layout, the way the animated proxies
    sample theirs (`crates/flui-objects/src/animation_sample.rs`), and is marked for layout
    when it ticks.
- **The reveal map.** `RevealMap` *(proposed, private)* is built at the start of layout from
  the reveals and the offset index. For a reveal with rows `a..b` and sampled value `t`, let
  `E` be the block's extent (the sum of its rows, with each nested reveal counted at its own
  uncovered extent). From that one value:
  - the block's layout extent is `t · E`: rows after it are placed `(1 − t) · E` earlier, and
    the scroll extent shrinks by the same amount;
  - the block's rows are shifted by `−(1 − t) · E`, so they slide out from under the parent
    row;
  - the block's clip is the main-axis range from its start (the trailing edge of the row before
    `a`, after the shifts of enclosing blocks) to `start + t · E`.

  Reveals are laminar: two blocks are disjoint or one contains the other, because each is a
  subtree. A row's shift is the sum over the blocks that contain it and its clip the
  intersection. The map is piecewise linear, so the scroll window is mapped back through it
  before the walk and the placed rows forward after it, in O(reveals) per row.
- **No sentinel.** A reveal is addressed by its row range. Nothing in the object names a parent
  index, and where a row may be absent the type is `Option` (PR #188626: `parentIndex == 0`
  meant two things).
- **Layout.** A row is laid out with the cross-axis extent left after its indent and placed at
  the cross-axis start plus the indent, so in a right-to-left context it indents from the
  right. The indent and the reveal shift are the row's committed paint offset; the clips are
  committed through `ShownChildren`. All four axis directions work.
- **Paint, hit test, semantics** read the shown answer: a row under a reveal clip takes no hit
  where it is clipped, and a row wholly outside it is not shown (PR #194172). The reveal clip
  is in the sliver's paint coordinates, which already include the scroll offset, because it is
  derived from committed row positions.
- **Toggles.** A toggle with no duration changes the roster and creates no reveal (#153889†). A
  second toggle of a node whose reveal runs reverses its clock; the roster does not change
  until the clock ends at zero. A collapse of a parent over an animating child adds an
  enclosing reveal; when the parent's clock ends, the roster drops the block and the owner
  drops every reveal inside its range. Replacing the tree drops every reveal. A cap bounds the
  running reveals; a toggle past it completes the oldest at once.
- **Cost.** A toggle at row `i` is one splice, O(log n) plus the runs it touches, and the
  delegate is asked only for the rows it added, as the window reaches them
  ([design.md](design.md), "The offset index"). The tree waits on that fix.
- **Semantics.** The widget wraps each row in a `Semantics` carrying level, position, set size
  and expanded state; the render object adds none.
- **Widget.** `TreeSliver` *(proposed)* with a node source (children on demand), a controller
  for expand and collapse, a row builder and the indentation. Keyboard traversal follows the
  focusable list (ADR-0265).
- **Departures from Flutter to record in "Mapping decisions":** a row is laid out in the
  extent left after its indent (Flutter lays it out at the full extent and shifts it, so it
  overflows the trailing edge by its indent); all four axis directions; children on demand;
  the reveal is a layout result; the delegate is a snapshot, and the seven writers of the
  index.
- **Alternative: the render object diffs the old and the new roster.** No log, and an O(n)
  comparison per build over rows that are answers of a delegate, not values the object holds.
  Rejected.
- **Alternative: `invalidate_from(i)` on a toggle.** [design.md](design.md), "The offset
  index". Rejected.
- **Alternative: the lazy family's `Placement`** (the slide the animated list uses). It moves
  paint only: the scroll extent would jump at the toggle while rows slide. A row that enters
  the band appears at its place, so revealed rows need a displacement each, and a row that
  leaves the roster has nothing left to slide. Rejected.
- **Alternative: Flutter's `activeAnimations` map inside the render object.** Two owners of the
  expansion state, and the orders of events of R6's table. Rejected.
- **Alternative: a column of nested lists.** Not lazy across nodes. Rejected.

## W10. Child-sized headers

The four persistent headers of `sliver_persistent_header.rs` take explicit minimum and maximum
extents and share `PersistentHeaderCore`; the floating ones are
`RenderSliverFloatingHeaderBase<M: FloatingHeaderMode>`. Flutter's three newer headers are the
same three behaviours with extents measured from children.

- **Shape.** Three objects in a new module `sliver/child_sized_header.rs` *(proposed)*, reusing
  the core's geometry rules with a measured extent in place of the authored one:
  - `RenderPinnedHeaderSliver` *(proposed)*: `Single`. The child is laid out with an unbounded
    main axis; its main-axis size is the scroll extent, the maximum paint extent and the
    scroll obstruction; paint origin is the overlap; paint extent is the child's extent capped
    by the remaining paint extent minus the overlap.
  - `RenderSliverResizingHeader` *(proposed)*: `Variable`, the child first and then the
    prototypes its configuration says are present. The child is laid out between the minimum
    and `max(minimum, maximum - shrink)`; the scroll obstruction is the minimum (#162661).
  - `RenderSliverFloatingHeader` *(proposed)*: `Single`, the floating state of
    `RenderSliverFloatingHeaderBase` (effective scroll offset, snap through
    `FloatingHeaderSnapConfiguration` and `SnapCommand`) with the child's extent as the maximum
    and zero as the minimum.
- **The resizing header's maximum.** With a maximum prototype it is the prototype's extent.
  Without one it is the child's maximum intrinsic extent on the main axis, asked through the
  sliver layout context's `box_child_intrinsic`, which `sliver_fill_remaining.rs` already uses.
  A query that returns an error makes the layout fail with a typed error that names the
  remedy: a maximum prototype. A child that builds during layout does not return one:
  `RenderLayoutBuilder`'s intrinsics answer zero and report through `tracing::error!`, so the
  header would take its minimum as its maximum. The widget's documentation says such a child
  needs a maximum prototype, and an answer below the minimum is raised to it.
  - *Alternative: a probing layout* (lay the child out unbounded, read its size, lay it out
    again under the real constraint). `measure_box_child` is a real layout, so this works with
    no new seam; it lays the child out twice on every pass and commits a size the child's
    descendants see between the two. Rejected while the intrinsic query answers.
  - *Alternative: a dry-layout entry on the sliver layout context.* A frozen-crate addition for
    one caller. Rejected.
- **Prototypes are not shown.** They are laid out and have no part in the header's
  `ShownChildren`, so they are not painted, not hit and not published (#153014).
- **The element.** No slot-typed multi-child sliver element exists. The widget is a
  `Variable`-arity sliver render view whose children reconcile densely, as `SliverMainAxisGroup`
  is; a dense list has no slot tag, so the render object learns "has a minimum prototype" and
  "has a maximum prototype" from its configuration in `update_render_object`. `flui-view` is
  not touched. The flags and the children are two records of one fact, so layout checks them
  first: the number of children must be one plus the prototypes the flags name, and a
  difference is a typed layout error that states both numbers. No child is read by a
  position the flags imply before that check.
- **What the child can read.** `HeaderFrame` *(proposed)*: the minimum and maximum extent and
  whether content is scrolled under the header (the overlap is positive or the header is
  shrunk). The current extent is not in it: it is the child's own constraint, which a child
  reads in the same frame with a `LayoutBuilder`. `HeaderFrame::of(ctx)` is an inherited
  value, so a child that reads it depends on it like any other.
- **How the child learns it changed.** A pinned header's "content is scrolled under it" flips
  on a scroll that lays out neither the header's child nor anything that repaints it. The
  header is the owner that knows: it writes the frame to the record it shares with the
  widget's state when layout commits, and calls the state's wake when the value differs
  from the last ([design.md](design.md), "A value a frame decided reaches a widget on a
  change"). The state rebuilds with the new value and the dependents of `HeaderFrame::of`
  rebuild with it; a scroll that changes nothing wakes nobody. The child follows one frame
  late, which the documentation states. A header whose content must follow in the same
  frame uses the existing persistent header, which builds during layout.
- **Overlap arithmetic.** The floating header's return under a preceding pinned sliver is where
  Flutter's review found a `min` for a `max` (PR #151145). The geometry is written once in the
  shared state machine, and R7's row runs it in both scroll directions and all four axis
  directions.
- **Groups.** `RenderSliverMainAxisGroup` already pulls a pinned child back inside the group's
  remaining extent (its module documentation), so a header leaves with its group (#178808).
  R7's group rows run the three new headers and the existing pinned one in a main-axis group
  and in a cross-axis group after a pinned sliver; a failing row is fixed in the group, which
  is an unfrozen file of `flui-objects`.
- **Reading order.** A pinned header is painted over the content it overlaps and published as
  a sibling before or after it. R7's test asserts the order a screen reader meets; if it fails,
  the header's node takes its place through ADR-0226's reading order, with no new mechanism.
- **What moves.** The floating state machine is lifted out of
  `RenderSliverFloatingHeaderBase` into a private type both families own, in its own file;
  `sliver_persistent_header.rs` shrinks, and its public names do not change.
- **Widgets.** `PinnedHeaderSliver::new(child)`, `SliverResizingHeader::new(child)` with
  `.min_extent_prototype(..)` and `.max_extent_prototype(..)`, `SliverFloatingHeader::new(child)`
  with `.snap_mode(..)`.
- **Alternative: add a "measure the child" mode to the existing persistent headers.** They are
  `Single` and rebuilt during layout through `HeaderShrinkCell`; prototypes need more children
  and no rebuild. One object would carry two arities' worth of rules. Rejected.

## W11. Sliver layout builder and fractional padding

- **Shape.** `RenderSliverLayoutBuilder` *(proposed, `sliver/sliver_layout_builder.rs`)*, a
  pass-through sliver (W3) that, before laying out its child, hands what it was built for to a
  cell implementing `BuildDuringLayoutCell`, the seam of ADR-0017 that `RenderLayoutBuilder`
  and `LayoutConstraintsCell` use for boxes. `SliverConstraintsCell` *(proposed)* sits beside
  `LayoutConstraintsCell`.
- **Element.** `flui_view::element::LayoutBuilder` hard-codes the box protocol, its constraints,
  its cell and its render object. The sliver form is a twin element with its own cell; the
  registry of cells and the fixpoint between layout passes are reused unchanged
  ([design.md](design.md), "One list of listeners"). Making the box element generic over
  protocol, constraints, cell and render object is the larger change for the same behaviour.
- **What triggers a rebuild.** Two constructors, one cell:
  - `SliverLayoutBuilder::new(|ctx, dimensions| ..)` rebuilds when `SliverLayoutDimensions`
    *(proposed: cross-axis extent, viewport main-axis extent, axis direction, growth
    direction)* changes. A scroll does not change it.
  - `SliverLayoutBuilder::of_constraints(|ctx, constraints| ..)` rebuilds when the
    `SliverConstraints` change, which is on every scroll step. Its documentation says so
    (#168064).

  The cell's `needs_build` compares the value the form was built for.
- **A rebuild requested during layout.** Build never runs inside a layout pass: a cell is
  serviced between passes, and `service_layout_builders` rebuilds the dirty elements under a
  builder with it. `commit` clears the cell's edge for the value it built for and nothing else,
  so an element dirtied while the pass ran is still dirty when the fixpoint looks again, and it
  is rebuilt in that frame or the next (Flutter PR #193331 loses it). R8's test holds this for
  both builders with one table: a child that requests a rebuild from inside the layout the
  builder caused, twice in a row.
- **`RenderSliverFractionalPadding`** *(proposed, `sliver/sliver_fractional_padding.rs`)*: pads
  its sliver child at both main-axis ends by a fraction of the viewport's main-axis extent,
  computed in layout from the constraints. `PageView` gains `pad_ends(bool)` (default true) and
  wraps its `SliverFillViewport` in one with the fraction `(1 − viewport_fraction) / 2` clamped
  at zero; the sentence in `page_view.rs` that lists centring as missing is corrected.
- **Why not the builder for `pad_ends`.** A page view scrolls on every frame of a swipe;
  Flutter built it on the builder and reverted for the cost (PR #37024†). The padding is a
  function of the constraints and needs no build.
- **Page arithmetic.** With `pad_ends(false)` the last pages cannot be scrolled to their
  centred offset. The page for an offset is clamped so that the maximum scroll extent maps to
  the last page, and a jump to a page clamps its target to the scroll range (#115755).
  `ViewportFraction::try_new` already refuses a fraction that is not finite and positive
  (#95710).
- **`user_scroll_direction`** (#79250). `SliverConstraints::user_scroll_direction` exists; R8's
  row pins that it is the logical direction in all four axis directions, with no code expected
  to change.
- **Alternative: compose the padding from `SliverLayoutBuilder` and `SliverPadding`.** No new
  object, and a build per resize; acceptable for one caller, but it puts a build-during-layout
  fixpoint under every page view. Rejected.

## W12. Tap regions

Flutter's `RenderTapRegionSurface` wraps the application, caches the hit-test result of each
pointer, and on a down or up event compares it with a registry of `RenderTapRegion`s. FLUI's
hit path is owned by the driver, a `PointerTarget` is data, and a render object's handler does
not see the path. What a render object can do is mark its entry: `RenderBox::metadata()`
returns a `HitMetadata`, and the gesture binding already reads one kind of it on the path
(`FirstClickTarget`, read by `first_click_route` in
`crates/flui-interaction/src/binding/mod.rs`).

The mechanism is the tap ADR's to decide ([design.md](design.md), "ADRs this spec needs").
This section is its brief: the shape this spec holds fixed, and the scenarios the ADR answers
with a type or a named test before it is accepted. The tasks build what the ADR accepts.

### What the spec holds fixed

- **The event** *(proposed, `flui-interaction`)*:

  ```rust
  #[non_exhaustive]
  pub struct TapRegionEvent {
      pub phase: TapPhase,
      pub buttons: PointerButtons,
      pub origin: TapOrigin,
  }

  #[non_exhaustive]
  pub enum TapPhase { Down, Up, Cancel, Tap }

  #[non_exhaustive]
  pub enum TapOrigin { Pointer(PointerKind), Semantics }
  ```

  A region that hears a contact's `Down` hears exactly one `Up` or one `Cancel` for it,
  whatever ends the contact. `Tap` follows an `Up` whose contact never exceeded the hit slop
  of its pointer kind (`GestureSettings::exceeds_hit_slop(kind, delta)`), latched by the
  first move that exceeds it, so a contact that leaves and returns is not a tap. `Tap` is
  kinematic and independent of the arena: a region learns of a tap whoever won the contact.
  A drag reports `Down` and `Up` and no `Tap` (#138190). Every button is reported (#136706).
  A host action without a contact (a status-bar tap, #177992) gets a `TapOrigin` variant when
  a host first produces one; none is delivered as a pointer at an invented position.
- **One registry, in the binding's presentation** *(proposed, `flui-interaction`,
  `TapRegionRegistry`)*: an `Rc` the `BuildOwner` holds, as it holds the focus manager, and
  hands to its `GestureBinding`. `LifecycleContext::tap_regions()` *(proposed, `flui-view`)*
  is infallible like `focus_manager()`; without a gesture binding regions register and hear
  nothing. `TapRegion` takes it in `create_state` and holds a guard whose `Drop` removes the
  region. An entry is an identity (`TapRegionId`, `TapRegionGroup`, both from the registry's
  `IdMint`) and a `Weak` to the state's cell; the callbacks, the consume flag and the group
  are read from the cell at delivery. State only the owner thread touches (`Rc`, `RefCell`);
  callbacks run through `flui_foundation::panic::commit` with the borrow released, and a
  callback that registers or removes a region during delivery gets a typed outcome. There is
  no surface widget and no surface render object.
- **`RenderTapRegion`** *(proposed, `flui-objects`, `interaction/tap_region.rs`)*: a proxy box
  whose `metadata()` is its `TapRegionMember` and which joins the hits below it
  (`joins_hits_below`), so a region under another is still on the path. It is on the path
  wherever the point is inside its box, whether or not its child takes the hit (#112924).
- **Inside or outside is decided once, from the hit path of the down.** The registry takes a
  snapshot when the binding has hit-tested the down, and every later event of the contact is
  delivered from it. A region mounted after the down hears nothing of that contact; one
  unmounted after it is skipped. Nothing is classified against a second hit test, so a
  region that moved between down and up cannot flip the answer.
- **Groups.** A down inside any member of a group is inside every member. `PopupRoute` gains
  `.tap_region_group(group)` *(proposed)*, and the route wraps its page in a `TapRegion` of
  that group, so a menu pushed with `PageHandle::push_popup` is inside its opener (#128317).
  A region's group is readable below it (`TapRegionGroup::maybe_of(ctx)` *(proposed)*).
- **Consuming an outside tap is a hold.** For a down outside a consuming region the registry
  joins the contact as a hold (`ArenaMembership::holding_contact`), between the hit test and
  `arena().close(pointer)`. The members that do not move lose on its admission and later
  ones are refused, so no detector under the tap acts (#135327). A drag or a scroll still
  takes the contact from the hold, and the raw `Listener`s on the route are not cancelled,
  because nothing claimed the contact (ADR-0208 §2 rule 5): a selection drag in a field
  under an open menu still runs. This makes a second producer of a hold, which is the part
  of ADR-0208 §2 rule 4 the tap ADR supersedes ([design.md](design.md)).
  - *Alternative: resolve the registry's member as accepted,* Flutter's form. A claimed
    hand-off sends `Cancel { reason: Claimed }` to every other `Listener` on the route and
    makes every moving member lose. Rejected ("Decisions for the owner", 14).
- **An accessibility activation passes one door.** An activation never reaches the binding.
  One function resolves the request, classifies it, tells the registry and invokes or does
  not (`deliver_semantics_action` *(proposed)*, recommended in `flui-view`, which owns the
  `BuildOwner` that holds the registry). `PresentationState::dispatch_semantics_action`
  (`crates/flui-runtime/src/presentation/accessibility.rs`), which the hosts and the
  development agent reach, calls it. `SemanticsActionInvocation::invoke` joins
  `disallowed-methods` in `clippy.toml`, and the one call inside the door carries the
  `#[expect]` with its reason. Classification is by render ancestry in both directions: the
  regions passed on the way from the target's render object to the root, and a live region
  whose own render object has the target as an ancestor (a region below a merged node).
  `SemanticsActionInvocation` already exposes `node_id()` and `action()`. No pointer is
  synthesised (PR #183093). A keyboard activation is not classified: it has no position, and
  keeping keys inside a popup is its route's focus scope.

### What the ADR answers

Each is a failure the first design allowed. The ADR names, for each, the type or the test
that rules it out; R12 lists the tests.

1. **Every action has a class.** `SemanticsAction` is `#[non_exhaustive]` (the `vocabulary!`
   macro of `flui-protocol` emits it), so a `match` in another crate needs a wildcard and a
   new action falls through it unclassed. Recommended: the class is a method beside the enum
   in `flui-protocol`, where a `match` without a wildcard compiles; an action that acts on
   its target is a tap (`Click`, `ShowContextMenu`, `Expand`, `Collapse`, `Increment`,
   `Decrement`, `SetValue`, `SetNumericValue`, `SetTextSelection`, `ReplaceSelectedText`,
   `Focus`, `CustomAction`) and one that only moves the view or the reader's attention is
   not. Otherwise a test over `SemanticsAction::ALL`.
2. **A stopped activation says so.** `dispatch_semantics_action` returns `Ok(())` whatever
   happened and the agent then reports `EffectKind::Ran`, so a consumed action reads as run;
   mapped to an error the agent's wildcard arm would report `NodeNotFound`. The ADR adds one
   typed outcome for "a consuming region stopped it" that the dispatch returns and the agent
   reports as itself, which changes the act schema of `flui-protocol`. It also fixes the
   order under a panic: every region is told, the action is invoked unless consumed, and the
   first panic resumes afterwards (`FirstPanic`).
3. **The tests use the doors applications have.** ADR-0400 deletes `Harness` and `LaidOut`,
   so nothing new lands on `Harness::invoke_semantics_action` and no test moves onto it. An
   activation is tested through the runtime's dispatch and through `Tester`'s semantic path;
   `Tester` has no verb that performs an action on a located node today
   ([design.md](design.md), "Unverified"). `flui-testing`'s existing door calls the one
   function for as long as it exists. The widget tests that resolve and invoke by hand are
   test code, exempt from the lint (ADR-0303 §5), and move with ADR-0400's migration.
4. **Liveness is one rule, per group, at the snapshot.** A popup covers its opener, whose
   page is then not current (`PageHandle::is_current()`), while the opener's region is in
   the popup's group and must still count. Recommended: at the snapshot the registry asks
   each entry's cell whether its render object is presented
   (`PipelineOwner::presented_placement`) and its page, where it has one, is current; a
   group is live when any member is, and every member of a live group hears. No stored flag
   and no per-frame read. `TickerMode` is not read: it mutes animations, and a region under
   a paused ticker can still be on screen.
5. **The snapshot sees the whole path, and every end of a contact reaches it.** The binding
   prunes the hit path for a first click (`first_click_route` keeps the first-click target
   and drops its ancestors) before the route is resolved, so membership read after the
   pruning puts a region outside a contact that is inside it. It also ends contacts itself:
   a second `Down` on a live pointer supersedes the first, the pointer cap evicts the
   oldest, and the platform, a lost capture, a removed device or a closing presentation
   cancel. The ADR takes membership from the result the hit test returned, says whether a
   click that only activates the window reports at all, and delivers the terminal event
   from the one place a cached route is removed, with a row per cause.
6. **A field's own overlays are inside it.** Selection handles, the selection toolbar and
   the magnifier are overlay entries (`text/editable_text/handles.rs`,
   `text/selection_menu/toolbar.rs`), outside the field's render subtree; a press on a handle
   would dismiss the field it belongs to. The field reads its ambient group and its overlay
   entries join it.
7. **A rebuilt region uses what it was rebuilt with.** Nothing is copied at `create_state`
   ("Rules every item follows"); the ADR's test changes the callback and the consume flag
   between two contacts.
8. **A fling stopped by a down** (#144221†). Recommended: a contact on which the scrollable
   admitted its hold reports `Down` and `Up` and no `Tap` ([design.md](design.md),
   "Unverified", for how the registry sees it).

- **Crates.** `flui-interaction` (event, registry, the snapshot in the binding),
  `flui-objects` (`RenderTapRegion`), `flui-view` (the capability, the door),
  `flui-runtime` (wiring, the outcome), `flui-protocol` (the class, the act schema),
  `flui-sdk` (the per-name re-exports of `flui_sdk::interaction`, so a package can name the
  event and the group), `flui-testing`, `flui-widgets` (`TapRegion`, the popup's group, the
  field's overlays). None is frozen.
- **Alternative: a surface render object that caches hit results,** as Flutter has. It needs
  the path inside a pointer handler, which the protocol does not carry
  (`crates/flui-rendering/ARCHITECTURE.md`, "The hit-test path is driver-owned"), so it means a
  frozen-crate change. Rejected.
- **Alternative: a global pointer listener that tests the region's rectangle.** Wrong under an
  overlay, a clip or a transform: R12's first test. Rejected.
- **Alternative: liveness from a navigator observer,** Flutter's fix. It misses shell routes
  and branch switches (PR #155297's own thread) and adds a second copy of "which page is
  current". Rejected.
- **Alternative: a tap from the arena's winner** instead of the kinematic rule. A region would
  then hear no tap when a detector under the pointer won, which is the case a region exists
  for. Rejected.

## W13. Subtree filters

`ColorFilterLayer` and `ImageFilterLayer` exist in `flui-layer` and `flui-engine` lowers both.
Paint has no way to ask for one: `FragmentScope` has opacity, clip, shader mask, backdrop
filter, annotated region, leader and follower scopes, and `PaintEffects` has opacity, clip and
transform.

- **Shape.** `PaintEffects` gains a filter, as its own documentation anticipates ("a future
  filter or mask (once those fields exist)") and as `crates/flui-rendering/ARCHITECTURE.md`
  plans under "Deferred producers" (`ImageFiltered`: "a field; `enabled` → `PAINT`").
  `RenderColorFiltered` and `RenderImageFiltered` *(proposed, `proxy/`)* are proxies whose
  `paint_effects(size)` returns it; hit testing and layout are the child's. An effect is pure
  in the object and its size, so the retained scene can patch a filter change without
  repainting the child.
- **A filter is a function of the size.** `paint_effects` takes the size and not the offset,
  so a filter built from the box (a matrix about its centre) cannot depend on where the box is
  painted (PR #73613).
- **Order.** Today the order is opacity, clip, transform, outermost first
  (`crates/flui-rendering/src/traits/paint_effects.rs` and `ARCHITECTURE.md`: "opacity, clip,
  transform, in that fixed nesting order"), and a node that needs another order uses a child
  node, "never by reordering the fields". Recommended to the ADR, outermost first: the group
  composite, the clip, the filter, the transform. The three existing fields keep their
  relative order.
  - *The composite is one step.* `ARCHITECTURE.md` already plans the group blend as an
    "additive field on the `#[non_exhaustive]` struct" `PaintOpacity`: a group is composited
    once, with an alpha and a blend mode. Opacity and blend are therefore not ordered against
    each other, and a half-transparent multiply group is one offscreen group, not two (W26).
  - *The clip bounds the filter.* A blur's output is cut by the object's own clip, as a
    clipped blurred element is in CSS and a `graphicsLayer` with `clip` in Compose
    (recollection; [design.md](design.md), "Unverified"). The layer and damage bounds of a
    filtered node then never exceed its clip. An author who wants the blur to spill wraps the
    filter around the clip, in a parent node.
- **Records the change owns.** The sentences that state the order and the two "Deferred
  producers" rows (`ImageFiltered`; "`blend` on `PaintOpacity`") in
  `crates/flui-rendering/ARCHITECTURE.md` and the documentation of `PaintEffects` in
  `traits/paint_effects.rs` are rewritten by the task that adds each field, so no record
  teaches the three-field order after it has four.
- **Bounds.** A blurred subtree's damage and layer bounds grow by the filter's reach. They are
  computed in the same space and with the same rounding as the offset the layer is painted at
  (#105674); R11's fractional-offset row runs over the retained scene's damage.
- **A native view under a filter** is refused or defined by W17's ADR (PR #188542); until it
  exists there is nothing to filter.
- **Alternative: `PaintContext::with_color_filter` and `with_image_filter` scopes,** like
  `with_shader_mask` and `with_backdrop_filter`. Consistent with the two effects that exist as
  scopes, and free in nesting, but a scope is inside the object's paint, so a change of filter
  repaints the subtree's fragment.
- **Gate.** Either form edits `flui-rendering`; the readback rows are engine tests.
- **Widgets.** `ColorFiltered::new(filter, child)`, `ImageFiltered::new(filter, child)` with
  `.enabled(bool)`.

## W14. Inline children

Today `RenderParagraph` and `RenderEditable` are `Arity = Leaf`. `InlineSpan` is
`Text(Arc<TextSpan>) | Placeholder(PlaceholderSpan)`, but `TextSpan.children` is
`Vec<TextSpan>`, so a placeholder can only be the root. `PlaceholderSpan` carries an authored
width and height. Measurement emits U+FFFC for a placeholder and shapes it as a glyph;
`ParagraphSpec` has no inline boxes, and `parley_text/shape.rs` drops
`PositionedLayoutItem::InlineBox`. `PlaceholderDimensions` exists as a type and nothing
consumes it. Parley is 0.13.0 and supports inline boxes.

The seam is designed once and both text objects use it.

- **Span tree (`flui-painting`, breaking).** `TextSpan.children` becomes `Vec<InlineSpan>`.
  `PlaceholderSpan` keeps its alignment and baseline and states its size as one of two forms
  *(proposed)*: a fixed width and height, as today, or `Measured`, with no payload, whose size
  the owner of the child supplies at layout. Every constructor and walker of the tree in the
  workspace moves in the same change (`rg 'children' crates/flui-painting/src/typography`,
  then the callers the compiler names).
- **What ties a child to its placeholder.** Its ordinal: the n-th `Measured` placeholder of
  the span tree in depth-first order belongs to the paragraph's n-th child. The ordinal is
  derived by the one walk of the tree and stored nowhere, so it has no lifetime to manage: a
  span tree rebuilt on every build names the same children, `PlaceholderSpan` keeps its
  `PartialEq` and its `serde` form, and a child's state follows ordinary reconciliation (its
  key, or its position among the children). `PlaceholderOrdinal` *(proposed)* is the newtype
  the painter's two lists are indexed by.
  - *Alternative: an id minted per placeholder when the span tree is built.* The tree is
    rebuilt each build, so either the id changes each build and every child loses its state,
    or something outside the tree must remember ids across builds: a second owner. An id in
    the span also has no honest equality or serialised form. Rejected.
- **The two lists are the same length by construction.** The paragraph refuses, with a typed
  layout error naming both numbers, a span tree whose count of `Measured` placeholders
  differs from its number of children. The painter returns one entry per `Measured`
  placeholder, `Some(rectangle)` or `None` for one cut by truncation, so a later child is
  found at its own ordinal and not at the count of boxes before it (#35869). A child's
  semantics node is attached by its child slot, so a child that publishes none shifts
  nobody (#69787).
- **Shaping (`flui-painting`).** A placeholder keeps one position in the text (U+FFFC stays in
  the logical string, so offsets, selection and published text runs do not shift), is not
  shaped as a glyph, and is passed to Parley as an inline box with its size and baseline.
  `TextPainter` takes a slice of `PlaceholderDimensions` in ordinal order before layout and
  returns the rectangles in the same order after it. `PlaceholderDimensions` exists today,
  is listed in `typography/README.md` and has no consumer; this is its consumer, and the
  README line is corrected to say what it carries. A painter laid out without dimensions for
  a measured placeholder returns a typed error; it never asserts (#49883).
- **`InlineBoxes`** *(proposed, `pub(crate)`, `crates/flui-objects/src/text/inline_boxes.rs`)*:
  the one owner of the relation between child slots and placeholder ordinals. After the text
  is laid out it commits the placed children to a `ShownChildren`; paint, hit testing,
  `apply_paint_transform`, `visits_child_for_semantics` and the dry measurements read that
  answer. There is no nullable offset in parent data as in Flutter's `TextParentData`, and no
  loop that stops at the first child without one.
- **Layout.** Two passes inside one `perform_layout`: each child is laid out under a loose
  constraint whose maximum width is the paragraph's; for `PlaceholderAlignment::Baseline` (and
  only then) its baseline is read; the text is laid out with those dimensions; children are
  placed at their rectangles. `Arity` becomes `Variable`, `ParentData` the box offset data. The
  rectangles are the ones the shaper returns after bidi reordering and alignment, so a child
  moves with a justified, centred or right-to-left line (#50009, #54400); R13's rows are the
  check that Parley does this ([design.md](design.md), "Unverified").
- **Intrinsics and dry layout.** Children are measured dry (`compute_dry_layout`,
  `compute_dry_baseline`); the paragraph's intrinsic widths include their widths, and its
  intrinsic height for a width is the height of the text laid out at that width with the
  children's dry sizes, over every line (#27293†).
- **Paint and hit test.** Text first, then the shown children in order.
- **Semantics.** The paragraph's node keeps its text; shown children's nodes are its children,
  attached by child slot, and take their place by ADR-0226's reading order. The spoken label is
  assembled in reading order with each child's content where its placeholder stands, never the
  replacement character (#111931). Selection treats a placeholder as one position, and copied
  text omits it.
- **No implicit scaling.** Flutter wraps a `WidgetSpan`'s child in a render object that scales
  it by a linear factor of the text scaler, which scales text inside it twice (#126962).
  ADR-0183 made text sizing non-linear, so there is no one factor. An inline child reads the
  ambient sizing itself, like any other widget.
- **Widgets (`flui-widgets`).** `InlineSpan` lives in `flui-painting` and cannot hold a view.
  A span builder *(proposed)* produces the span tree and the child list together, appending a
  child each time it emits a `Measured` placeholder (`Text::rich` and `RichText` take its
  result), so the two counts agree by construction and the paragraph's check is for a span
  tree made by hand. `RichText` mounts a multi-child element; with no inline child it has no
  children and behaves as today.
- **`RenderEditable`.** The same `InlineBoxes`. The editing value's text holds one U+FFFC per
  placeholder, so the model, the shaper and the platform's text input count the same positions:
  one scalar, one grapheme, one caret stop, one deletion. `TextEditingController`
  (`crates/flui-widgets/src/text/controller.rs`) holds the text as UTF-8 and the selection as
  byte offsets on character boundaries, so a placeholder is three bytes there and a caret
  never stands inside them. The caret beside a placeholder takes the line's height and can
  stand after a trailing one; the selection highlight is painted after the inline children it
  covers (#30688).
  - *One owner: the editing value.* In a field an ordinal is not an identity: after a
    deletion or a restored value the n-th placeholder is another chip. The value is one type,
    `EditingText` *(proposed, working name, beside `controller.rs`)*: the text and one
    `PlaceholderId` per U+FFFC, in order, with ids from the controller's own `IdMint`. It is
    changed by two methods and has no other way in: `replace(range, &str)`, which removes
    U+FFFC from the incoming text and drops the ids of the placeholders inside the range, and
    `insert_placeholder(at) -> PlaceholderId`. A value made from a plain string has no
    placeholders.
  - *Both holders of the text are that type.* The controller's stored text
    (`ControllerInner.text`) and the input session's document (`Doc.text`,
    `text_store.rs`) are `String`s today, and a session writes its whole text back
    (`SessionWrite`, `write_session`). With both an `EditingText`, the session's edits, which
    already arrive as range replacements (`TextStoreEdit::replace`), keep every placeholder
    outside the range with its id, and the write-back carries text and ids together. No path
    strips placeholders from a whole value, so a composition beside a chip cannot delete it,
    and a paste or an IME cannot add one. `insert_str`, `insert_str_at`, `set_text` and
    `backspace` become calls of `replace`.
  - *Who builds the spans.* The field builds the span tree from the value: it splits the
    text at each U+FFFC and asks an inline-child builder *(proposed)* for the child of that
    `PlaceholderId`, and keys the child by it, so a chip keeps its state when another is
    deleted before it. The application supplies no second text, so a span tree whose text
    differs from the value cannot be made (#107432). A read-only paragraph has no edits and
    keeps ordinals. A field with no inline-child builder draws a U+FFFC as the replacement
    glyph, as today. The field has no undo stack today; one that lands restores values, and
    a value carries its ids.
  - *An obscured field.* `RenderEditable` is given masked text and "never sees the source
    characters" (`editable.rs`); the mask maps one character to one obscuring character. A
    placeholder is masked like any character, one position for one position, and its child
    is not built ("Decisions for the owner", 15).
  - A controller that turns typed text into chips, what a chip serialises to, and keeping an
    application's own list of chips in step with deletions are catalog work on top of this.
- **ADR.** The span tree change, the ordinal rule, the placement of measurement, the scaling
  rule and the editing value's type (who builds the spans, the two methods that change it,
  the obscured field) are cross-crate and hard to reverse.
- **Alternative: ordinals in a field, with U+FFFC stripped at each write point.** A session
  write is the whole value, so stripping it deletes the field's own placeholders, and an
  ordinal names a different chip after any deletion. Rejected.
- **Alternative: a separate widgets-layer span tree** that lowers to the painting one. Two
  trees to keep in step. Rejected.
- **Alternative: a list of boxes and a list of nodes zipped with the children,** Flutter's
  form. The lists differ in length when a placeholder has no box or a child has no node, and
  every Flutter defect of R13's first two rows is that difference. Rejected: both of FLUI's
  lists have one entry per placeholder, present or `None`.
- **Alternative: the editing value holds the replaced text** (a mention's full name) and the
  shaper maps ranges to placeholders. Two offset spaces and a map between them on every caret
  move; Flutter's #107432 is open on exactly this. Rejected.
- **Alternative: lay text out around floats in the widget layer** (a `Wrap` of text runs and
  widgets). Loses line breaking across runs, bidi and selection. Rejected.

## W15. List wheel

Flutter's `RenderListWheelViewport` is a box that is its own viewport: it owns a scroll offset,
creates and destroys children during layout through a child manager, and paints each under a
cylinder transform that it saves into parent data for hit testing. FLUI's lazy children exist
only for slivers: `request_child_build` and `emit_retain_band` are on the sliver layout
context, and the element is `SliverMultiBoxAdaptor`.

- **Shape: a sliver.** `RenderSliverWheel` *(proposed, `sliver/wheel/`)* joins
  `LazyMultiBoxRender` and sits alone in an ordinary viewport built by `ListWheelScrollView`.
  It depends on the sliver transform seam ([design.md](design.md)) and edits no frozen crate
  itself.
  - *Extents.* Like the carousel, whose scroll extent is its strip plus the viewport: the wheel
    reports `(count - 1) * item_extent + viewport_extent` and places item `i` so that its
    centre is at the viewport's centre when the scroll offset is `i * item_extent`. The scroll
    range is then exactly `0 ..= (count - 1) * item_extent` with no anchor. The viewport's
    centre is `viewport_main_axis_extent / 2`; the sliver's own origin in the viewport is
    `viewport_main_axis_extent - remaining_paint_extent` in the forward group.
  - *Geometry.* `WheelGeometry` *(proposed)* owns the arithmetic: the item at an offset, the
    offset of an item, and for an item its angle on the cylinder and its transform. Its
    parameters are validated newtypes (`try_new`): diameter ratio, perspective (above 0, at
    most 0.01, Flutter's bounds), squeeze, off-axis fraction, magnification, off-centre
    opacity. An unbounded cross axis or a transform that is not finite is a layout error
    (#31650).
  - *Layout.* Children in the window are requested, laid out at the item extent and the full
    cross-axis extent, and for each the object computes its transform, its opacity and its
    clip and commits them as `ShownChildren`. Items beyond a quarter turn are laid out and not
    shown. The transform therefore exists after the first layout, before any paint (#126491).
  - *Paint, hit test, semantics* read the shown answer; the semantics rectangle goes through
    the sliver's paint-transform hook.
  - *Magnifier.* The magnification is a continuous function of an item's distance from the
    centre across the band's edge, not a step (#131544). An item that crosses the edge is
    shown in two parts, each under its clip; the seam's third change makes it one set of
    regions.
  - *Along either axis* (#25074†): the geometry is written in main-axis and cross-axis terms.
- **The selected item** is `WheelGeometry`'s answer for the scroll offset. Before the first
  layout it is the controller's initial item, so a child that reads it in its first build gets
  a defined value (PR #194859); a negative initial item and a looping wheel are in R15's test.
- **A change of count or delegate** marks the sliver for layout through the lazy element's
  `update`, as for every lazy sliver (#58144); R15's row pins it.
- **Looping.** The item count is a `usize`, so the wheel cannot use negative indices as
  Flutter does. Recommended: two wheel slivers around the viewport's `center`, with the
  viewport's `anchor` at 0.5. The forward one shows items `0, 1, 2, …` modulo the count and
  the reverse one `-1, -2, …`; `RenderViewport` already has `center`, `anchor` and a reverse
  growth group whose scroll extent counts down. `WheelGeometry` owns the half-item shift
  between the two halves' offsets ([design.md](design.md), "Unverified").
  - *Alternative: one sliver with a very large count and an origin in its middle,* re-centred
    while the wheel is idle. No second sliver; the scroll offset loses precision far from the
    origin and the re-centre is an extra rule. The fallback if the reverse group's arithmetic
    does not work out.
- **Scrolling.** A released drag or fling settles through a `SnapPolicy` *(proposed:
  `WheelSnap`)* beside `PageSnap`. A mouse wheel's notch is a step, not a release:
  `WheelSnap` implements `SnapPolicy::step` ([design.md](design.md), "One settle rule for a
  scrollable, and a notch that moves one item"), and the scrollable's one `WheelMotion`
  holds where the wheel is heading, so two quick notches move two items and one notch moves
  one whatever the host's line height (#125291†). What counts as a notch on each host is
  that section's table; a touchpad's pixel stream, and every browser packet, scrolls freely
  and settles at its end. The controller is the fixed-extent form of `ScrollController`. Bringing a tapped item to the centre (#24089†) is
  the catalog's use of the tap that R15's test shows reaching the item.
- **Alternative: a box viewport as in Flutter.** It needs lazy children on the box layout
  context and a second lazy element: a second form of the request strategy, in a frozen crate.
  With W16's typed key a wheel keyed by a signed index becomes possible there, and looping
  becomes simple; the ADR weighs this against the sliver with the costs of
  [design.md](design.md), "Decisions for the owner", 2.
- **Alternative: an eager wheel** (all children built). Fine for a 12-item picker, wrong for a
  date wheel of thousands. Rejected as the only form.

## W16. The two-axis viewport

Flutter's `RenderTwoDimensionalViewport` is abstract: it owns two offsets, asks a delegate for
the child at a `ChildVicinity`, and leaves `layoutChildSequence` to a subclass. FLUI's request
strategy is keyed by `usize` and retains one contiguous band.

- **The contract (ADR, frozen crate).** Lazy children are requested by a typed key. The
  request strategy and its element are made generic over the key: the lazy slivers use a
  `usize`, the two-axis viewport a `Vicinity { column, row }` *(proposed)*. The retain set
  becomes a set the key type describes: a band for `usize`, a small list of vicinity
  rectangles for a grid (the visible region plus pinned rows and columns). Pinned children
  (ADR-0265) take the key type too.
- **One owner of the mapping.** The keyed element holds the only map from key to child, with
  kept-alive and pinned children in it. Reconciliation of one layout's requests is a single
  pass over that map: a child whose widget key moved to another vicinity is re-slotted, and
  when the target vicinity holds a live child the two are swapped, never overwritten (#141101).
  A kept-alive child that leaves the retain set and whose lease is released is removed in the
  same pass, and a query for a child that has no geometry returns `None` (#138977).
- **Shape.** `RenderTwoAxisViewport<L: TwoAxisLayout>` *(proposed, `flui-objects`,
  `sliver/viewport/two_axis.rs`)*, a `RenderBox`. `TwoAxisLayout` *(proposed)* is the policy
  Flutter leaves to a subclass: given the viewport size, both offsets and the cache extent, it
  requests children by vicinity, lays each out, places it, and returns the content extent on
  each axis and the paint order. The viewport reports the extents to its two scroll positions
  (`apply_content_dimensions`) and commits the shown answer: the children in paint order and
  one clip only when a child extends past the viewport and the clip behaviour is not
  `Clip::None` (#149907†, #134858†). One policy ships with it so the object has a harness row
  and a widget: a grid of uniform cells with pinned leading and trailing rows and columns.
- **Hit testing** is the reverse of the committed paint order, so a pinned cell, painted last,
  is hit first on any side and with either axis reversed (#186876, #135386). Flutter walks its
  child map.
- **Cache.** The cached region extends by the cache extent on all four sides of the visible
  one, whatever the scroll direction (#157350†).
- **Cost.** The policy's work is proportional to the cells in the region and the pinned cells
  in view, not to the content's extent (#161421): the uniform grid computes its visible range
  by division, and pinned rows and columns are ranges, not per-cell lookups.
- **Widget.** `TwoAxisScrollView` *(proposed)*, which owns two scroll positions and one
  two-axis drag (`DragSlot::pan()`), and gives each axis that can scroll its component of the
  motion. An axis whose content fits takes none and blocks nothing (#144982).
  `DiagonalDrag::{Free, Locked}` *(proposed)* says what a drag off the axes does: `Free`
  moves both, and `Locked` gives a contact to the axis that led when it left the slop, which
  is what a table wants. It is a filter on the deltas after both owners began, so the
  contract below is the same for both. Table and tree views are layouts on top, outside
  this spec.
- **One contact, two owners, begun together or not at all.** Each axis has a `ScrollOwner`,
  and one owner's `begin_contact` resumes its `FirstPanic` before it returns
  (`crates/flui-widgets/src/scroll/scroll_owner.rs`), so two calls in a row leave the second
  owner unbegun, still flinging, when a listener of the first panics. The widget begins the
  contact through one entry *(proposed, in `scroll_owner.rs`)* that calls
  `begin_contact_with` on both owners with one sequence id and one `FirstPanic`, and resumes
  it after both have begun. `begin_contact_with` returns `None` when an owner cannot begin;
  the entry then ends the sequence the other owner began (`ScrollEndReason::Cancelled`) and
  reports that no contact began, so the drag moves neither axis. Updates and the end of the
  contact go to both owners under the same rule: both are told, then the first panic is
  resumed. No code begins one contact on two owners today ([design.md](design.md),
  "Unverified").
- **A resize or a jump mid-frame** invalidates the region; the request strategy's fixpoint
  (ADR-0017) settles it in the frame within ADR-0308's bound, and a frame that reaches the
  bound paints what it has, logs, and goes on in the next (#147292†).
- **Alternative: a second request seam for boxes,** leaving the sliver one alone. Two
  implementations of the build-during-layout rule, the retain rule and the pinned rule.
  Rejected.
- **Alternative: map a vicinity to a `usize` row-major and use today's seam.** No frozen-crate
  change, but the column count must be known and fixed, and one contiguous band between the
  first and last visible cell retains every column of every row between them. Rejected for
  wide tables; acceptable as a stopgap only if the owner wants a table before the freeze ends.
- **Alternative: a lazy column of lazy rows sharing one horizontal position.** A viewport per
  row, no shared column geometry, no pinned column. Rejected.

## W17. Native views

Not designed here; the gap says it needs an owner decision. What the ADR must settle is R17's
table. The shape this spec recommends it consider:

- **One render object.** `RenderPlatformView` *(proposed)* for every host, not one per OS as in
  Flutter: OS types stay in `flui-native` (ADR-0082, ADR-0151), and the object holds a typed
  id from the contract crate.
- **Placement is scene data.** Paint records a placement node (id, rectangle, clip, transform)
  in the retained scene. After the scene is applied the host reads the placements and moves the
  OS views in the same turn. Layout never asks the native side for anything, so no geometry is
  requested before the first layout or at zero size (#190833, #20456): a view that is not laid
  out has no placement node, and a placement of zero area is not sent.
- **No placement means hidden.** A view whose frame holds no placement node for it (scrolled
  out of a lazy list, offstage, under a page that is not painted, not yet laid out) is hidden
  by the host in the turn that applies that frame. The host keeps no placement from an
  earlier frame, so a view cannot stay on screen at a stale rectangle. The placements move
  with the frame that is presented: the ADR states what a host does with them when the
  present fails, so the OS view and the pixels around it never come from different frames.
- **A host that cannot embed a view says so.** Creating a native view on a host without
  support, the headless host included, returns a typed refusal the widget shows as its
  fallback; it is not a panic and not an empty box. The rule is the contract crate's, so
  every host answers the same way.
- **Resize.** A size change is one placement, applied with the frame that shows it. For a
  texture-backed view the host resizes the texture and the placement carries the size the
  texture was drawn for, so a frame never stretches content drawn for another size (#139230).
  The ADR states the cost of a resize per host and whether a view inside a scroller is
  resized or clipped while it scrolls (#31243).
- **Two compositions.** A view rendered into a texture rides `RenderTexture` and
  `TextureRegistry`, which exist; a view composited by the OS needs the placement node and a
  rule for what the GPU surface draws under it and for order and clipping under a scroller
  (#164899†). The texture path is the first stage.
- **Input and semantics.** Which side wins a contact is a rule of the gesture arena, stated
  per host (#53490, #49084†); who owns focus and the IME (#19718†); how a foreign accessibility
  tree joins the published one.
- **`RenderWebImage`** is the web host's case of the same seam (an `<img>` element for an image
  the canvas may not read) and waits for it, with rows over every fit and alignment
  (PR #171916†) and release of the element on disposal (PR #193408†).

## W18. The record of what stays covered by a different design

A document change. "Mapping decisions" of `crates/flui-objects/ARCHITECTURE.md` gains the six
entries of [design.md](design.md), "Covered by a different design", each citing by name the
tests that hold the FLUI form. For the nested-scroll entry the task reads ADR-0209 and the
tests of `crates/flui-widgets/src/scroll/nested/` and `collapsing_header.rs` against R18's six
behaviours, and for each either cites the section and the test, adds the test where the
behaviour holds and no test names it, or adds an entry to `docs/gaps.md` where the connection
does not define it. It designs nothing: a behaviour ADR-0209 does not define becomes a gap for
that ADR's owner, not a decision of this spec.
