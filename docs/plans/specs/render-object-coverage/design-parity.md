# render-object-coverage: design, Flutter parity (W1–W18)

- **Status:** Not started — awaiting owner approval of requirements and design
- **Date:** 2026-11-25
- **Base:** `claude/project-thread-03lo7m` @ `fcab477`
- **Overview, shared seams, ADR list, decisions:** [design.md](design.md); requirements:
  [requirements.md](requirements.md); tasks: [tasks.md](tasks.md)

One section per work item. The conventions of [design.md](design.md) hold: existing code was
read at `fcab477`, a new name is marked *proposed* where it first appears, and "the shown
answer", "the offset index" and "the sliver transform seam" are the seams of that file.

## W1. The harness reads its families from more than one file

`crates/flui-objects/tests/render_object_harness.rs` is one test target of 15,905 lines
(`autotests = false`). Its guard `catalog_covers_every_render_object_name` reads the file with
`include_str!("render_object_harness.rs")` and counts a name only inside the body of a
`fn harness_*` (the text up to the first `\n}\n`). Most tasks of this spec add rows; with one
file they all own it.

- **Shape.** Family modules under `crates/flui-objects/tests/harness/` *(proposed)*, mounted
  from the root at column 0 with `#[path = "harness/<family>.rs"]` and `mod <family>;`, as
  `harness_snapshot.rs` is mounted today. One `const FAMILY_FILES` beside the `mod` lines
  lists `(name, include_str!(..))` pairs. Existing rows stay where they are; only new families
  use files of their own.
- **What a row looks like in a family file.** The root's `family_*` table calls it, so it is
  `pub(super) fn harness_*`; the function the root calls in `harness_snapshot.rs` is `pub fn`
  for the same reason. A row nothing calls is dead code under `-D warnings`.
- **The guard, as three rules in one function** `rows_naming(sources, type_name)` that the
  guard tests call with the real sources:
  1. *A row* starts at a line at column 0 that matches `fn harness_` after an optional
     visibility (`pub`, `pub(crate)`, `pub(super)`) and ends at the first `\n}\n`. The
     present split on `"\nfn harness_"` sees no `pub` row.
  2. *A name* counts only as a whole identifier: not preceded or followed by a letter, a
     digit or an underscore. A row that names `RenderSliverFloatingHeaderBase` then does not
     cover `RenderSliverFloatingHeader`. A type the present substring match covered only
     through a longer name gets its row in this change.
  3. *A file* counts only when it is mounted. `family_files_are_mounted_and_listed` reads the
     directory (`CARGO_MANIFEST_DIR/tests/harness`), and fails for a file that is not in
     `FAMILY_FILES`, for a listed file whose `#[path]` and `mod` lines are missing from the
     root at column 0, and for a listed name with no file. CI's unmounted-tests step globs
     `tests/*.rs` only, so this test is the rung for the subdirectory (ADR-0303: a test of the
     owning crate).
- **The guard is tested on text.** Three tests call `rows_naming` and the mount check on
  literal sources: `the_guard_counts_a_pub_row_of_a_family_file`,
  `the_guard_refuses_a_name_inside_a_longer_one`,
  `the_guard_refuses_a_family_file_that_is_not_mounted`. Each fails with the present matcher,
  so the change has a test that fails without it, before any family file exists.
- **What each later task still edits in the root.** One line in `RENDER_OBJECT_TYPES`, and the
  first task of a family adds its two mount lines, its `FAMILY_FILES` entry and its
  `family_*` test. These are one-line, order-insensitive edits; a rebase resolves them
  mechanically.
- **Document.** `docs/testing.md` says the manifest holds "all 81 exported render types"; the
  list has 106 entries. The sentence links the list and drops the number (AGENTS.md, "One
  source per fact").
- **Alternative: one test target per family.** Each target links the whole crate and its GPU
  test support again; `flui-widgets` moved the other way for that reason (its `Cargo.toml`
  comment on `autotests`). Rejected.
- **Alternative: leave one file.** Every task then lists the same file as owned, which "One
  writer per file set" forbids. Rejected.

## W2. The shown answer, starting with the overflow clip

`layout/stack.rs` (two objects), `wrap.rs`, `fitted_box.rs`, `animated_size.rs` and
`constraints_transform_box.rs` each keep `has_visual_overflow: bool`, reset it at the start of
`perform_layout`, and clip in `paint` when it is set and the clip behaviour is not `Clip::None`.
None overrides `describe_approximate_paint_clip`, so semantics and input regions are not cut.
Each also has a public `has_visual_overflow()` accessor, which `flui-testing` reads
(`crates/flui-testing/src/widgets.rs`, `has_visual_overflow`) and two documents of
`flui-rendering` name.

- **Shape.** The `shown` module of [design.md](design.md), "The shown answer", is built here.
  The six objects each hold an `OverflowClip`. Their layout computes "a child overflows" as a
  value and commits it on every `Ok` exit; `paint` clips only when `clip(behavior, size)` is
  `Some`, and `describe_approximate_paint_clip` returns the same value for every child. The
  bool field and its reset are deleted from the six in the same change.
- **The public accessor stays.** `has_visual_overflow()` on each object returns
  `OverflowClip::overflows()`. Its meaning is unchanged where a layout succeeds, and after a
  layout that fails it now reports the last committed answer instead of `false`. `flui-testing`
  and the documents that name the accessor stay true and are not edited.
- **`RenderSliverCarousel`** moves from its private `Shown` table to `ShownChildren`, its
  ADR-0265 pinned slot committed as announced, with no behaviour change (its rows are the
  check), so the per-child form has a production user from the day it lands.
  Its `describe_semantics_clip` keeps returning `SemanticsClip::Bounds` for an item's mask.
- **What an overflow clip does to semantics** is what any authored clip does
  ([design.md](design.md), "Which clip hook"): the six objects report through the paint-clip
  hook, as `RenderClip` does, and add no semantics-clip override. R2 holds that by comparison,
  not by a rule of its own: an object with an overflow clip publishes what the same children
  under a `RenderClipRect` of the box publish, alone and under a scrollable.
- **Hit testing** is unchanged and now stated: an overflowing child is hit only inside its
  parent's box, clipped or not (R2, #19445).
- **Breaks.** None in public API: the fields are private and the accessors stay. One visible
  change of behaviour: after a failed layout `has_visual_overflow()` keeps the last committed
  answer.
- **Alternatives** are those of the seam ([design.md](design.md)).
- **Mapping decisions to record:** the contract of the shown answer with its three states;
  the hit rule of an unclipped overflow.

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
  never recovered from two offsets and never replaced by the child's own size. A change of
  delegate (`should_relayout`), or of dimensions when `depends_on_dimensions` is true, calls
  `invalidate_from(0)`; a change of count calls `set_count`.
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
  most `EAGER_EXTENT_LIMIT` calls in the eager case, the window otherwise. That is the bound,
  and the documentation states it. A delegate that returns `false` from
  `depends_on_dimensions` (a table of fixed heights) is asked once per item for its life.
  Answers are not kept per dimensions.
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
  roster of rows, and one clock per running toggle. The render object keeps one thing across
  layouts, the offset index, and it is a cache of the roster's extents: every change to it is
  a roster edit delivered through the one channel below, or a reset. It holds no expansion
  state and no animation.
- **The roster** *(`flui-widgets`)*: the visible nodes in depth-first order, each with its
  depth. It is eager over *expanded* nodes only: a node's children are asked from the
  application's node source when the node expands, and row widgets are built lazily by the
  sliver's element, each under a key made from its node's id so that a row keeps its state
  when rows are inserted above it ([design.md](design.md), "Unverified"). While a block
  collapses its rows stay in the roster as ghosts; when the clock ends they leave it.
- **How the render object learns which rows changed.** The roster numbers its states
  (`RosterRevision` *(proposed)*, minted by `IdMint`) and keeps a bounded log of
  `RosterEdit { at, removed, inserted }` *(proposed)*, one per expand or per collapse that
  ended. The delegate hands the log on:

  ```rust
  pub trait TreeRowDelegate {
      fn extent(&self, index: usize, dimensions: &ItemExtentDimensions) -> f64;
      fn depth(&self, index: usize) -> usize;
      fn revision(&self) -> RosterRevision;
      /// The edits after `since`, oldest first, or `None` when the log no longer reaches it.
      fn edits_since(&self, since: RosterRevision) -> Option<Vec<RosterEdit>>;
      fn should_relayout(&self, old: &dyn TreeRowDelegate) -> bool;
  }
  ```

  The lazy element calls `set_item_count` before `update(config)`
  (`sliver_adaptor.rs`, `update_render_object`), so the tree's `set_item_count` only records
  the count. In `update` the object asks for the edits since the revision it last applied
  and applies each as `Virtualizer::splice(at, removed, inserted)`, in order, so several
  toggles in one build arrive as several splices. `None` (the log was outrun, or the tree
  was replaced) is a reset: `set_count` and `invalidate_from(0)`. After the edits the index's
  length must equal the recorded count; a mismatch is a reset with a diagnostic, never an
  index used at the wrong length. Rows on either side of an edit keep their extents, and the
  scroll anchor moves with its row.
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
  the reveal is a layout result; the roster's edits are the only way the index changes.
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
- **What the child can read.** `HeaderFrame` *(proposed)*: current, minimum and maximum extent
  and whether content is scrolled under the header (the overlap is positive or the header is
  shrunk). The widget's state owns it and hands it to the render object and, through
  `HeaderFrame::of(ctx)`, to the child. The object writes it when layout commits and marks it
  changed when a value differs.
- **How the child learns it changed.** A pinned header's "content is scrolled under it" flips
  on a scroll that lays out neither the header's child nor anything that repaints it, so a
  value written in layout reaches nobody. `HeaderFrame::listenable()` is a `ChangeNotifier`
  the widget's state notifies after the frame, through the after-frame helper
  ([design.md](design.md)), when the frame was marked changed; a child that paints from the
  frame listens, as a carousel item listens to `CarouselItemScope::listenable`. The
  notification is one frame late, which the documentation states. A header whose content must
  follow in the same frame uses the existing persistent header, which builds during layout.
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

  Every region that hears a contact's `Down` hears exactly one `Up` or one `Cancel` for it:
  `Cancel` when the platform cancels the contact or the presentation closes under it. `Tap`
  follows an `Up` whose contact never exceeded the hit slop of its pointer kind
  (`GestureSettings::exceeds_hit_slop(kind, delta)`), latched on the cached route by the first
  move that exceeds it, so a contact that leaves and returns is not a tap. `Tap` is kinematic
  and independent of the arena, so a region learns of a tap whoever won the contact. A drag
  reports `Down` and `Up` and no `Tap` (#138190). Every button is reported (#136706).
  `TapOrigin` is `#[non_exhaustive]` because a host action without a contact (a status-bar
  tap, #177992) gets a variant of its own when a host first produces one; no such action is
  delivered as a pointer at an invented position, and FLUI's hosts produce none today.
- **The registry** *(proposed, `flui-interaction`, `TapRegionRegistry`)*: an `Rc` the
  `BuildOwner` holds, as it holds the focus manager, and that the presentation hands to its
  `GestureBinding`. It holds the mounted regions (id, group, live, consumes outside taps,
  callbacks); `TapRegionId` and `TapRegionGroup` are minted by `IdMint`. It is state only the
  owner thread touches (`Rc`, `RefCell`); callbacks run through
  `flui_foundation::panic::commit` with the borrow released, and a callback that registers or
  removes a region during delivery gets a typed outcome, not a second borrow.
- **How a widget reaches it.** `LifecycleContext::tap_regions() -> Rc<TapRegionRegistry>`
  *(proposed, `flui-view`)*, infallible like `focus_manager()`: the registry exists wherever
  a build owner does, and without a gesture binding (a bare element test) regions register
  and hear nothing. `TapRegion` takes it in `create_state` (the state is born bound) and holds
  a guard whose `Drop` removes the region. There is no surface widget and no surface render
  object.
- **What a contact reports to.** On a down the binding hit-tests, resolves the route, drops the
  result and admits a `CachedPointerRoute`. In the same pass, before `drop(result)`, the
  registry takes the contact's *snapshot* and stores it on the cached route: the ids of the
  regions that are registered and live at that moment, and which of them are on the path.
  Every later event of the contact is delivered from the snapshot. A region mounted after the
  down is not in it and hears nothing of that contact; one unmounted after the down is
  skipped. The up and the tap are never classified against a second hit test, so a region
  that moved between down and up cannot flip the answer.
- **`RenderTapRegion`** *(proposed, `flui-objects`, `interaction/tap_region.rs`)*: a proxy box
  whose `metadata()` is its `TapRegionMember` and which joins the hits below it
  (`joins_hits_below`), so a region under another is still on the path. It adds itself to the
  path wherever the point is inside its box, whether or not its child takes the hit (#112924).
  Its widget's state publishes the render object's id to the registry on attach and detach,
  the way `RenderSubtreeAnchor` publishes its id into a `SubtreeAnchor`.
- **Groups.** A down inside any member of a group is inside every member. `PopupRoute` gains
  `.tap_region_group(group)` *(proposed)*, and the route wraps its page in a `TapRegion` of
  that group, so a menu pushed with `PageHandle::push_popup` is inside its opener (#128317).
- **Consuming an outside tap.** For a down outside a consuming region the registry joins the
  contact as a *hold* (`ArenaMembership::holding_contact`, ADR-0208 rule 4), between the hit
  test and `arena().close(pointer)`. The members that do not move, a tap or a long press, lose
  on its admission and later ones are refused, so no detector under the tap acts (#135327). A
  drag or a scroll still takes the contact from the hold, and the raw `Listener`s on the route
  are not cancelled, because nothing claimed the contact (rule 5): a selection drag in a field
  under an open menu still runs, and the menu is still told of the outside `Down`. The arena
  holds members weakly; the registry keeps the `Rc`.

  ADR-0208's rule 4 names one producer of a hold, a moving scrollable. The tap ADR's change
  rewrites that sentence for two producers and adds the row "consuming region, outside
  contact" to the ownership table that `assert_ownership_table` checks.
  - *Alternative: resolve the registry's member as accepted,* Flutter's form. Under rule 5 a
    claimed hand-off sends `Cancel { reason: Claimed }` to every other `Listener` on the
    route and makes every moving member lose, so a scroll that starts outside a menu is dead
    and a text field's selection drag stops. Rejected ("Decisions for the owner", 14).
- **Liveness.** A region is live while two things hold: the page it is on is the current one
  (`PageHandle::is_current()`, an inherited value the widget depends on, where there is a
  page), and its render object was presented by the last frame
  (`PipelineOwner::presented_placement(id)` is `Some`). The state reads the second after each
  frame through the after-frame helper ([design.md](design.md)) and writes the flag to its
  registry entry. A region in a branch that is not shown, under an offstage parent or behind a
  `Visibility` is therefore not live, with no rule per container, and a region is live one
  frame after it first paints, which is before a pointer can land on it. A group hears outside
  events while any member is live. No navigator observer is involved (PR #155297) and
  `TickerMode` is not read: it mutes animations, and a region under a paused ticker can still
  be on screen.
  - *Cost.* One `presented_placement` per mounted region per drawn frame; the documentation
    states it.
- **Accessibility activations: one function, every door.** An activation never reaches the
  binding. Today three kinds of caller resolve a request and invoke it:
  `PresentationState::dispatch_semantics_action`
  (`crates/flui-runtime/src/presentation/accessibility.rs`, reached by
  `UiRuntime::dispatch_accessibility` and by the development agent),
  `flui_testing::a11y::invoke_semantics_action` (behind `Harness::invoke_semantics_action`),
  and tests of `flui-widgets` that call `resolve_semantics_action` and `invoke()` themselves
  (in `catalog_disclosure.rs`, `catalog_slider.rs`, `scroll.rs` and `semantics.rs`). A rule
  written into the first is bypassed by the other two.
  - `flui-view` gains `deliver_semantics_action(owner, registry, request)` *(proposed, in a
    module of its own)*: it resolves the request, classifies it, tells the registry, and
    invokes or does not. `flui-runtime`'s dispatch and `flui-testing`'s door both call it.
    The tests that invoke by hand only to deliver an action move onto the harness door.
    Those that keep an invocation across a rebuild to show a stale one is safe test
    `invoke` itself and stay as they are.
  - The rung that keeps it the only door: `SemanticsActionInvocation::invoke` joins
    `disallowed-methods` in `clippy.toml`, and the one call inside
    `deliver_semantics_action` carries the `#[expect]` with its reason. Test code is exempt
    from that lint at the crate root (ADR-0303 §5), which is why the delivering tests are
    moved by hand in the same change.
  - `flui-semantics`: `SemanticsActionInvocation` exposes its action and the id of its node
    *(proposed accessors; today only `invoke` is public)*.
  - *Classification is by render ancestry, in both directions.* The function walks from the
    target's render object to the root (`PipelineOwner::render_tree()`, `RenderTree::parent`)
    and collects the `TapRegionMember`s it passes: the target is inside those regions. A live
    region whose own render object has the target as an ancestor (a region below a node that
    merged its descendants) also counts as inside, found by walking up from each live region's
    published render id. Every other live region is outside.
  - *Which actions count.* One exhaustive `match` over `flui_protocol::semantics::
    SemanticsAction`, which is not `#[non_exhaustive]`, so a new variant does not compile
    until it is classed. Recommended to the ADR: an action that acts on its target is a tap
    (`Click`, `ShowContextMenu`, `Expand`, `Collapse`, `Increment`, `Decrement`, `SetValue`,
    `SetNumericValue`, `SetTextSelection`, `ReplaceSelectedText`, `Focus`, `CustomAction`);
    one that only moves the view or the reader's attention is not (`ScrollIntoView`, the four
    scroll actions, `SetScrollOffset`, `ShowTooltip`, `HideTooltip`, `Blur`) and is invoked
    without classification.
  - For a tap, the registry is handed one `TapRegionEvent` with `TapPhase::Tap` and
    `TapOrigin::Semantics`, delivers it once, and answers whether a consuming region was
    outside; if so the action is not invoked. No pointer is synthesised (PR #183093).
  - *A keyboard activation is not classified.* It has no position. Keeping the keys inside a
    popup is its route's focus scope; the ADR states that a consuming region does not stop
    Enter on a control outside it that still has focus.
- **Crates.** `flui-interaction` (event, registry, the snapshot in the binding),
  `flui-objects` (`RenderTapRegion`), `flui-view` (the capability, the one activation door),
  `flui-runtime` (wiring), `flui-semantics` (the two accessors), `flui-testing` (its door),
  `flui-widgets` (`TapRegion`, the popup's group). None is frozen.
- **A fling stopped by a down** (#144221†). Recommended in the ADR: a contact on which the
  scrollable admitted its hold (rule 4) reports `Down` and `Up` and no `Tap`
  ([design.md](design.md), "Unverified", for how the registry sees it).
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
- **Alternative: classify an activation inside `dispatch_semantics_action`.** One host's
  door; the harness and any test that invokes an action pass it by. Rejected.

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
  - *One owner of "value and placeholders agree".* The field builds the span tree from the
    controller's value: it splits the text at each U+FFFC and asks an inline-child builder
    *(proposed)* for the child of the n-th one. The application supplies no second text, so
    a span tree whose text differs from the value cannot be made (#107432), and there is no
    check to run and nothing to refuse.
  - *A U+FFFC that did not come from the application.* The controller's write points are
    where text enters the value (`insert_str`, `insert_str_at`, `set_text`, an IME commit).
    Each removes U+FFFC from incoming text, so a paste or an IME cannot add a placeholder. A
    placeholder enters through one method, `insert_placeholder` *(proposed)*, and a restored
    value through a constructor that states it carries placeholders. A field with no
    inline-child builder draws a U+FFFC that is already in its value as the replacement
    glyph, as today.
  - *An obscured field.* `RenderEditable` is given masked text and "never sees the source
    characters" (`editable.rs`); the mask maps one character to one obscuring character. A
    placeholder is masked like any character, one position for one position, and its child
    is not built ("Decisions for the owner", 15).
  - A controller that turns typed text into chips, what a chip serialises to, and keeping an
    application's own list of chips in step with deletions are catalog work on top of this.
- **ADR.** The span tree change, the ordinal rule, the placement of measurement, the scaling
  rule and the editing value's rules (who builds the spans, where a U+FFFC may enter, the
  obscured field) are cross-crate and hard to reverse.
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
  `WheelSnap`)* beside `PageSnap`. A mouse wheel's notch is a step, not a release, and goes
  through `DetentSteps` ([design.md](design.md), "One settle rule … and one step rule"): the
  wheel's implementor steps from the item the previous step was heading for, so two quick
  notches move two items and one notch moves one whatever the host's line height (#125291†).
  A touchpad's pixel stream scrolls freely and settles at its end. The controller is the
  fixed-extent form of `ScrollController`. Bringing a tapped item to the centre (#24089†) is
  the catalog's use of the tap that R15's test shows reaching the item.
- **Alternative: a box viewport as in Flutter.** It needs lazy children on the box layout
  context and a second lazy element: a second form of the request strategy, in a frozen crate.
  With W16's typed key a wheel keyed by a signed index becomes possible there, and looping
  becomes simple; the ADR weighs this against the sliver.
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
  motion. An axis whose content fits takes none and blocks nothing (#144982). Table and tree
  views are layouts on top, outside this spec.
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
  (ADR-0017) settles it in the frame (#147292†).
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
