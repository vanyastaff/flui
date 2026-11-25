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

- **Shape.** Family modules under `crates/flui-objects/tests/harness/` *(proposed)*, declared
  from the root with `#[path]` as `harness_snapshot.rs` is today. The guard reads the root and
  each family file: one `const` beside the `mod` lines lists `(name, include_str!(..))` pairs,
  the texts are concatenated before the split into `fn harness_*` bodies, and a module that is
  declared but not listed fails the guard. Existing rows stay where they are; only new families
  use files of their own.
- **What each later task still edits in the root.** One line in `RENDER_OBJECT_TYPES`, and the
  first task of a family adds its `mod` line, its guard entry and its `family_*` test. These
  are one-line, order-insensitive edits; a rebase resolves them mechanically.
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

- **Shape.** The `shown` module of [design.md](design.md), "The shown answer", is built here
  with both types. The six objects each hold a `ChildClip`, call `commit` as the last statement
  of a successful layout, clip in `paint` only for `Some`, and return it from
  `describe_approximate_paint_clip` for every child. The bool field and its reset are deleted
  from the six in the same change. `RenderSliverCarousel` moves from its private `Shown` table
  to `ShownChildren`, with no behaviour change (its rows are the check), so the per-child form
  has a production user from the day it lands.
- **Why an overflow clip does not widen for semantics.** A viewport overrides
  `describe_semantics_clip` to keep its scroll cache reachable. An overflow is not a cache:
  content cut by the box is not reachable by scrolling, so it is cut for a screen reader too.
- **Hit testing** is unchanged and now stated: an overflowing child is hit only inside its
  parent's box, clipped or not (R2, #19445).
- **Breaks.** None in public API; the fields are private.
- **Alternatives** are those of the seam ([design.md](design.md)).
- **Mapping decisions to record:** the contract of the shown answer; the hit rule of an
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
  bounding rectangle.
- **Seams between neighbours.** R3's readback row draws two clipped slivers of one colour at
  fractional scroll offsets and samples the boundary row. If it shows a seam, the main-axis
  edges of the committed clip are snapped to device pixels in paint, where the device scale is
  known; layout stays in logical units.
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
  SliverMultiBoxAdaptorParentData`), paired with `pub type SliverVariedExtentList =
  SliverMultiBoxAdaptor<RenderSliverVariedExtentList>` in `flui-view`, as the four existing
  lazy slivers are. Its `Config` holds an `Rc<dyn ItemExtentDelegate>`.
- **The delegate** *(proposed, `sliver/item_extent.rs`, in `flui-objects`)*:

  ```rust
  pub trait ItemExtentDelegate {
      fn extent(&self, index: usize, dimensions: &ItemExtentDimensions) -> f64;
      fn should_relayout(&self, old: &dyn ItemExtentDelegate) -> bool;
  }
  ```

  the shape of `SliverGridDelegate`. `ItemExtentDimensions` *(proposed, `#[non_exhaustive]`)*
  has the viewport main-axis extent and the cross-axis extent, and deliberately not the scroll
  offset or the preceding scroll extent Flutter's `SliverLayoutDimensions` has: with them an
  answer changes on every scroll and cannot be kept.
- **Offsets.** The offset index of [design.md](design.md): a `Virtualizer` driven through
  `walk_virtualizer_band`. For each index the walk reaches that is not yet answered
  (`is_measured` is false), the list asks the delegate, validates the answer (finite, not
  negative; otherwise a layout error naming the index) and stores it with `set_measured`. A
  change of delegate (`should_relayout`) or of dimensions calls `invalidate_from(0)`; a change
  of count calls `set_count`.
- **The count.** `LazyMultiBoxRender::create` takes a `usize`, and an `ItemCount::Unknown`
  source that probes as endless arrives as `usize::MAX`. Three cases:

  | Count | First layout | Scroll extent |
  |-------|--------------|---------------|
  | Known, at most the eager limit | The delegate is asked for every index once | `Extent::Exact` from the first layout |
  | Known, above the limit | Asked as the window reaches an index; the rest estimated at the mean of the answers | `Extent::Estimated` until all are asked; an anchor correction keeps the visible items in place |
  | Unknown or endless | The same, and the window is bounded | Estimated |

  The eager limit is the constant the grid already has for an unbounded window
  (`MAX_UNBOUNDED_WINDOW_CHILDREN`, 1,000,000), moved to `virtualized_band.rs` so both read
  one value. An unbounded main axis uses `bounded_unbounded_window` as the measured list does,
  which is also the bound when every extent is zero.
- **Why the eager case exists.** With every extent known, the scroll extent is exact, the
  scrollbar thumb does not move under the user, and a jump to an offset lands exactly. That is
  why a caller picks this list over `SliverList`. A million cheap calls once is the price;
  above it, the list says in its documentation that its extent is provisional.
- **Layout, paint, hit test.** `walk_virtualizer_band` already does the window walk, child
  requests (`request_child_build`), the retain band (`emit_retain_band`), pinned children
  (`PinnedChildren`, ADR-0265) and the placement hand-off. The fixed-extent list keeps its own
  arithmetic walk; nothing is extracted from it.
- **Placement.** `set_placement` is implemented by `RenderSliverList` and `RenderSliverGrid`
  only. The varied-extent list implements it too, through the walk's `placed` output, so an
  animated list can use authored extents.
- **Widgets.** `SliverVariedExtentList` (builder and delegate) and a `ListView` constructor
  beside `ListView::builder`.
- **Mapping decisions to record:** the delegate's dimensions omit the scroll offset; the three
  count cases.
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
  and holds the offset index of W8. Flutter extends the list by inheritance; here both objects
  drive the same walk.
- **One owner.** The widget's state owns the expansion state of every node, the flattened
  roster of rows, and one clock per running toggle. The render object owns nothing that
  outlives a layout: it is handed the roster's answers and the running reveals and derives
  positions and clips from them.
- **The roster** *(`flui-widgets`)*: the visible nodes in depth-first order, each with its
  depth. It is eager over *expanded* nodes only: a node's children are asked from the
  application's node source when the node expands, and row widgets are built lazily by the
  sliver's element. While a block collapses its rows stay in the roster as ghosts; when the
  clock ends they leave it.
- **What the render object is given.** Its `Config` holds:
  - `Rc<dyn TreeRowDelegate>` *(proposed)*: `extent(index, dimensions) -> f64`,
    `depth(index) -> usize`, `should_relayout`. The lazy family's parent data type is fixed by
    the trait, so a row's depth cannot live there as Flutter's `TreeSliverNodeParentData.depth`
    does;
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
- **Cost.** A toggle at row `i` calls `invalidate_from(i)` on the index and re-asks the rows
  after `i`; each answer is a read of the roster ([design.md](design.md), "The offset index").
- **Semantics.** The widget wraps each row in a `Semantics` carrying level, position, set size
  and expanded state; the render object adds none.
- **Widget.** `TreeSliver` *(proposed)* with a node source (children on demand), a controller
  for expand and collapse, a row builder and the indentation. Keyboard traversal follows the
  focusable list (ADR-0265).
- **Departures from Flutter to record in "Mapping decisions":** a row is laid out in the
  extent left after its indent (Flutter lays it out at the full extent and shifts it, so it
  overflows the trailing edge by its indent); all four axis directions; children on demand;
  the reveal is a layout result.
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
  A child that cannot answer an intrinsic query makes the layout fail with a typed error that
  names the remedy: a maximum prototype.
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
  not touched.
- **What the child can read.** `HeaderFrame` *(proposed)*: current, minimum and maximum extent
  and whether content is scrolled under the header (the overlap is positive or the header is
  shrunk). The object writes it when layout commits; the child reads it in paint or after, the
  contract `CarouselItemFrames` has in `sliver/carousel/`. A header that must *rebuild* from it
  uses the existing persistent header, which builds during layout.
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
  pub enum TapPhase { Down, Up, Tap }

  #[non_exhaustive]
  pub enum TapOrigin { Pointer(PointerKind), Semantics }
  ```

  `Tap` is delivered with the up of a contact that was not cancelled and whose up lies within
  the touch slop of its down. It is kinematic and independent of the arena, so a region learns
  of a tap whoever won the contact. A drag reports `Down` and `Up` and no `Tap` (#138190). Every
  button is reported (#136706). `TapOrigin` is `#[non_exhaustive]` because a host action
  without a contact (a status-bar tap, #177992) gets a variant of its own when a host first
  produces one; no such action is delivered as a pointer at an invented position, and FLUI's
  hosts produce none today.
- **The registry** *(proposed, `flui-interaction`, `TapRegionRegistry`)*: owned by the
  presentation's `GestureBinding`, holding the mounted regions (id, group, live, consumes
  outside taps, callbacks); `TapRegionId` and `TapRegionGroup` minted by `IdMint`. It is state
  only the owner thread touches (`Rc`, `RefCell`); callbacks run through
  `flui_foundation::panic::commit` with the borrow released, and a callback that registers or
  removes a region during delivery gets a typed outcome, not a second borrow.
- **How a widget reaches it.** A capability on `LifecycleContext` *(proposed, `tap_regions()`,
  `flui-view`)*, wired where the presentation is assembled
  (`crates/flui-runtime/src/presentation/assembly.rs`), as the focus manager and the hit-test
  handle are. `TapRegion` takes it in `create_state` (the state is born bound) and holds a
  guard whose `Drop` removes the region. There is no surface widget and no surface render
  object.
- **The pointer route.** On a down the binding hit-tests, resolves the route, drops the result
  and admits a `CachedPointerRoute`. The registry's members are collected from the path in the
  same pass, before `drop(result)`, and stored on the cached route. The up and the tap of the
  contact read the members from the route: they are never classified against a second hit
  test, so a region that moved or unmounted between down and up cannot flip the answer.
- **`RenderTapRegion`** *(proposed, `flui-objects`, `interaction/tap_region.rs`)*: a proxy box
  whose `metadata()` is its `TapRegionMember` and which joins the hits below it
  (`joins_hits_below`), so a region under another is still on the path. It adds itself to the
  path wherever the point is inside its box, whether or not its child takes the hit (#112924).
- **Groups.** A down inside any member of a group is inside every member. `PopupRoute` gains
  `.tap_region_group(group)` *(proposed)*, and the route wraps its page in a `TapRegion` of
  that group, so a menu pushed with `PageHandle::push_popup` is inside its opener (#128317).
- **Consuming an outside tap.** Between the hit test and `arena().close(pointer)`, a registry
  told of a down outside a consuming region adds itself to the arena for that pointer and
  resolves as accepted. An eager winner among the members resolves at closure, so no detector
  under the tap acts (#135327). The arena holds members weakly; the registry keeps the `Rc`.
- **Liveness.** A region is live while `PageHandle::is_current()` holds for the page it is on
  (where there is one) and `TickerMode::is_enabled(ctx)` holds. Both are inherited values the
  region's widget depends on, so it rebuilds and updates its registry entry when either
  changes. A group hears outside events while any member is live. No navigator observer is
  involved, so a shell's branch switch is covered by the branch muting its subtree, the way
  `Visibility` and the flight seat already mute theirs (PR #155297).
- **Accessibility activations.** An activation never reaches the binding: the presentation's
  `dispatch_semantics_action` resolves the request with
  `PipelineOwner::resolve_semantics_action` and calls `invocation.invoke()`. The seam is
  there:
  - `flui-semantics`: `SemanticsActionInvocation` exposes its action and the render id of its
    node *(proposed accessors; today only `invoke` is public)*.
  - `flui-runtime`: before invoking a tap-like action, `dispatch_semantics_action` walks the
    target's render ancestors (`PipelineOwner::render_tree()`, `RenderTree::parent`,
    `render_object().metadata()`, all public today) and collects the `TapRegionMember`s. It
    hands the registry one `TapRegionEvent` with `TapPhase::Tap` and `TapOrigin::Semantics`.
    The registry classifies as it does a pointer's members and answers whether a consuming
    region was outside; if so the runtime does not call `invoke()`.
  - Once per activation by construction: the runtime is the only caller, and it does not
    synthesise a pointer (PR #183093).
  - The limit, stated in the ADR: a region whose render object lies below a node that merged
    its descendants is classified by the merged node's source render object
    ([design.md](design.md), "Unverified").
- **Crates.** `flui-interaction` (event, registry, the collection in the binding),
  `flui-objects` (`RenderTapRegion`), `flui-view` (the capability), `flui-runtime` (wiring and
  the activation route), `flui-semantics` (the two accessors), `flui-widgets` (`TapRegion`,
  the popup's group). None is frozen.
- **A fling stopped by a down** (#144221†). Recommended in the ADR: the scrollable that stops a
  fling on a down marks the contact, and the registry delivers `Down` and `Up` for it and no
  `Tap`.
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
  filter or mask (once those fields exist)"). `RenderColorFiltered` and `RenderImageFiltered`
  *(proposed, `proxy/`)* are proxies whose `paint_effects(size)` returns it; hit testing and
  layout are the child's. An effect is pure in the object and its size, so the retained scene
  can patch a filter change without repainting the child.
- **A filter is a function of the size.** `paint_effects` takes the size and not the offset,
  so a filter built from the box (a matrix about its centre) cannot depend on where the box is
  painted (PR #73613).
- **Order.** Today the order is opacity, clip, transform, and the documentation says a node
  that needs another order uses a child node, "never by reordering the fields". The ADR adds
  to the order without changing it: opacity, group blend (W26), filter, clip, transform. A blur
  is then not cut by the object's own clip and fades as a whole.
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
  *(proposed)*: a fixed width and height, as today, or `Measured(PlaceholderId)`, whose size
  the owner of the child supplies at layout. Every constructor and walker of the tree in the
  workspace moves in the same change (`rg 'children' crates/flui-painting/src/typography`,
  then the callers the compiler names).
- **Identity.** `PlaceholderId` *(proposed)* is minted per measured placeholder when the span
  tree is built and is what ties a child to its placeholder. Nothing is matched by position:
  a list of boxes shorter than the list of children (a truncated placeholder has no box) is
  then not an index error (#35869), and a child that publishes no semantics node does not
  shift the others (#69787).
- **Shaping (`flui-painting`).** A placeholder keeps one position in the text (U+FFFC stays in
  the logical string, so offsets, selection and published text runs do not shift), is not
  shaped as a glyph, and is passed to Parley as an inline box with its size and baseline.
  `TextPainter` takes the measured dimensions by id before layout and returns, by id, the box
  rectangle after it, or nothing for a placeholder cut by truncation. A painter laid out
  without dimensions for a measured placeholder returns a typed error; it never asserts
  (#49883).
- **`InlineBoxes`** *(proposed, `pub(crate)`, `crates/flui-objects/src/text/inline_boxes.rs`)*:
  the one owner of the relation between child slots and placeholder ids. After the text is
  laid out it commits the placed children to a `ShownChildren`; paint, hit testing,
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
  attached by id, and take their place by ADR-0226's reading order. The spoken label is
  assembled in reading order with each child's content where its placeholder stands, never the
  replacement character (#111931). Selection treats a placeholder as one position, and copied
  text omits it.
- **No implicit scaling.** Flutter wraps a `WidgetSpan`'s child in a render object that scales
  it by a linear factor of the text scaler, which scales text inside it twice (#126962).
  ADR-0183 made text sizing non-linear, so there is no one factor. An inline child reads the
  ambient sizing itself, like any other widget.
- **Widgets (`flui-widgets`).** `InlineSpan` lives in `flui-painting` and cannot hold a view.
  A span builder *(proposed)* produces the span tree and the child list keyed by
  `PlaceholderId` together (`Text::rich` and `RichText` take its result), so every measured
  placeholder has exactly one child by construction. `RichText` mounts a multi-child element;
  with no inline child it has no children and behaves as today.
- **`RenderEditable`.** The same `InlineBoxes`. The editing value's text holds one U+FFFC per
  placeholder, so the model, the shaper and the platform's text input count the same positions:
  one scalar, one grapheme, one caret stop, one deletion. `RenderEditable` refuses a span tree
  whose logical text differs from the editing value with a typed error, instead of drawing a
  caret in the wrong place (#107432). The caret beside a placeholder takes the line's height
  and can stand after a trailing one; the selection highlight is painted after the inline
  children it covers (#30688). A controller that turns typed text into chips, and what a chip
  serialises to, is catalog work on top of this.
- **ADR.** The span tree change, the placeholder's identity, the placement of measurement, the
  scaling rule and the editing value's rule are cross-crate and hard to reverse.
- **Alternative: a separate widgets-layer span tree** that lowers to the painting one. Two
  trees to keep in step. Rejected.
- **Alternative: children passed beside the span tree by index.** The two can disagree, and
  every Flutter defect of R13's first two rows is that disagreement. Rejected.
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
- **Scrolling.** The settle is a `SnapPolicy` *(proposed: `WheelSnap`)* beside `PageSnap`; one
  notch of a mouse wheel is a step to the next item's offset through the same policy (#125291†).
  The controller is the fixed-extent form of `ScrollController`. Bringing a tapped item to the
  centre (#24089†) is the catalog's use of the tap that R15's test shows reaching the item.
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
  motion. An axis whose content fits takes none and blocks nothing (#144982). Both positions
  begin the contact under one sequence id (`ScrollOwner::begin_contact`), which no code does
  today ([design.md](design.md), "Unverified"). Table and tree views are layouts on top,
  outside this spec.
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
