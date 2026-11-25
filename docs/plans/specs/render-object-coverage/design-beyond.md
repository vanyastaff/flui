# render-object-coverage: design, beyond Flutter (W19–W26)

- **Status:** Not started — awaiting owner approval of requirements and design
- **Date:** 2026-11-25
- **Base:** `claude/project-thread-03lo7m` @ `fcab477`
- **Overview, shared seams, ADR list, decisions:** [design.md](design.md); requirements:
  [requirements.md](requirements.md) (R19–R26); tasks: [tasks.md](tasks.md)

Render-layer primitives Flutter's library does not have. Each was judged, and each adopted one
is designed to the depth of a parity item.

Statements about FLUI code were read at `fcab477`. Statements about Compose, SwiftUI, CSS, WinUI,
Qt and community packages are recollections, not checked against their sources or trackers
([design.md](design.md), "Unverified"). They choose what to test; nothing is built on one.

## The bar

A candidate is adopted when all four hold:

1. **Authors reach for it and Flutter makes them work around its absence**: a community package
   exists for it, or the catalogs rebuild it by hand more than once.
2. **Composition does not reach it without a known defect**: the result needs a parent that
   measures all children, or one answer that paint, hit testing and semantics share.
3. **It lands on a seam FLUI has or this spec builds anyway**, not on a new one of its own.
4. **Its cost and its pitfalls are known**, and it edits no frozen crate, or it shares a gated
   change another item already needs.

| Candidate | Verdict | Item | Reason in one line |
|-----------|---------|------|--------------------|
| Geometry and visibility observers | Adopt (visibility gated) | W19 | A leader watch already reports a change of placement; visibility needs two more fields on it; answers `SizeChangedLayoutNotifier` without notifications |
| Sticky rows in a lazy list; group-scoped pinning | Adopt | W20, R7 rows | The most asked-for list feature; rides the offset index and one index the lazy element keeps outside the band |
| Grid over a shared track module | Adopt (subset) | W21 | The largest layout primitive Flutter lacks; `RenderTable` already holds half the algorithm |
| Wrap that stops, with an indicator | Adopt | W22 | `RenderToolbarItems` already has the rule; the shown answer hides the rest |
| One placement policy for floating children | Adopt | W23 | Two bespoke delegates on `main`, with tooltip and dropdown still to come |
| First that fits | Adopt | W24 | Small, and the shown answer makes "the others are hidden" free |
| Per-item scroll effects | Adopt (gated by its seam) | W25 | Second user of the sliver transform seam the wheel needs anyway |
| Blend group and widget mask | Adopt (gated) | W26 | One layer kind, decided in the same ADR as the filters |
| Child-declared scroll snap | Defer | — | `SnapPolicy` is already an open trait with three implementors |
| Typed `Layout` trait | Defer | — | Rewrites how multi-child boxes are declared; its delegate lives in a frozen crate |
| Masonry sliver | Defer | — | Needs a lane index beside the offset index; do after the varied-extent list has users |
| Multi-slot build during layout | Defer | — | An ADR that generalises ADR-0017; `LayoutBuilder` covers most uses |
| Flex shrink, basis, wrap with grow | Defer | — | Wanted by web ports, blocks nothing |
| Sticky box in a plain scroll view | Defer | — | Needs the scroll offset in paint; the lazy form covers lists |
| Balanced text wrapping | Defer | — | After the inline-children seam settles `RenderParagraph` |
| Inset host for an overlaid bar | Defer | — | A composition over W19 first; a layout only if the one-frame lag shows |
| Split view | Defer | — | A catalog widget |
| Viewport-relative sizing | Defer | — | `LayoutBuilder` outside the scroll view answers it |
| Alignment guides, last baseline | Reject for now | — | A render-protocol change in a frozen crate |
| Public pluggable lazy layout | Reject | — | Exposes the request strategy; the shared walk is the internal form |
| Lazy wrap | Reject | — | Only worth it as a user of the item above |
| Overlapping slivers, animated paint extent | Reject | — | `DecoratedSliver`, groups and the tree's reveal cover the uses |
| Recycling by content type | Reject | — | No profile shows creation cost; recycling brings stale state |
| Constraint-solver layout | Reject | — | A package over a layout trait, not core |
| Lookahead layout, `content-visibility`, user shaders, subgrid and dense placement, multi-column and floats | Reject | — | "Rejected" below |

Each adopted item is independent of the other seven in what it offers. Striking one strikes its
requirement, its section here and its tasks. One piece is shared and outlives a strike: the
single commit of a lazy sliver's shown answer, which W20 and W25 both join
([design.md](design.md), "Seams more than one item uses").

## W19. Geometry and visibility observers

- **What exists.** `LeaderWatch`
  (`crates/flui-rendering/src/pipeline/owner/leader_watch.rs`): `PipelineCell::watch_leader`
  returns a watch on the leader that publishes under a `LayerLink`. The pipeline owner
  compares every live watch against each applied `SceneVersion` and, when the leader's
  `LeaderGeometry { size, origin }` changed, queues one post-frame delivery to the watch's
  listeners. A leader is a `RenderLeaderLayer` (`flui-objects`, `proxy/leader.rs`). The
  selection handles of both text widgets and a plugin hook listen to one. There is no public
  leader widget.
- **Shape** *(proposed, `crates/flui-widgets/src/observe/`)*. Both observers wrap their child
  in a leader of their own and hold a watch on it, taken in `create_state`:
  - `GeometryObserver::new(on_change, child)` reports `ObservedGeometry { size, origin }`:
    the child's size and its top-left corner in the window, what the watch reports today. It
    needs nothing below `flui-widgets`.
  - `VisibilityObserver::new(on_change, child)` reports `ObservedVisibility { fraction,
    visible_rect }` when the fraction crosses one of its thresholds: `.thresholds(..)`, a
    short validated list in `0.0 ..= 1.0`, by default the one crossing between nothing shown
    and something shown; `.margin(..)` grows or shrinks the clip the fraction is taken
    against, so a list can start loading before a row appears. This is the report an
    intersection observer gives, and it cannot fire once per frame of a scroll unless the
    application asks for a threshold per frame.
- **What the visibility observer waits for.** The fraction needs the transform of the
  leader's chain and the clip that chain resolves, and `LeaderGeometry` carries neither. The
  scene already resolves both for a follower (ADR-0276's `LeaderClip::Hide`). One gated
  change adds them to `LeaderGeometry`, which is `#[non_exhaustive]`, with a read on
  `SceneVersion`, and rewrites the record "A follower's build hears its leader's geometry
  after paint" ([design.md](design.md), "Decisions for the owner", 17). `GeometryObserver`
  does not wait.
- **Delivery.** The watch's listener holds a `Weak` to the state's cell and reads the current
  `on_change` there, so a rebuilt observer calls what it was rebuilt with, and runs it through
  `flui_foundation::panic::commit`. The watch clears its queued mark before its listeners run,
  so a callback that panics is contained and the next change is delivered. Dropping the watch
  with the state ends delivery. An observer whose child does not move costs nothing per
  frame.
- **The fraction.** The child's box is mapped through the chain's transform; the fraction is
  the area of that shape's bounding rectangle inside the clip, grown by the margin, over the
  bounding rectangle's area. Under a rotation it is therefore a bounding fraction, which the
  report's documentation says. A leader that is not in the scene (offstage, not painted)
  reports zero. Siblings painted over the child are not counted: the documentation says "not
  clipped away", never "seen".
- **A hidden window.** A presentation that is minimised or hidden draws no frame, so no
  scene is applied and the last report stands. The documentation says so and names what to
  combine it with, the application's lifecycle state (`flui_scheduler::AppLifecycleState`).
- **Feedback.** A callback that changes layout causes a frame, and the frame's scene a new
  comparison. An application whose callback converges settles; one that does not is a loop
  of its own making, one report per frame, never a spin inside a frame.
- **No render object.** Nothing is added to `flui-objects`.
- **Replaced forms.** None in code. It is FLUI's answer for `SizeChangedLayoutNotifier`
  (requirements, "What is not here"): a typed callback, not a notification.
- **Not included.** The rectangle in a named ancestor's space (an application subtracts two
  reports), throttling, dwell time ("half visible for a second") and occlusion: policy an
  application builds on the report.
- **Pitfalls elsewhere (recollection).** Compose's `onGloballyPositioned` callbacks that set
  state and loop; the `visibility_detector` package's 500 ms timer, which leaks into tests; a
  fraction that ignores occlusion read as "seen".
- **Alternative: read `presented_placement` after every frame and compare.** Ungated, and a
  read per observer on every frame of every animation, with its own rules for a panic, a
  disposed state and a rebuilt callback ([design.md](design.md), "A value a frame decided
  reaches a widget on a change"). Rejected.
- **Alternative: a render proxy that records its size into a cell in layout,** like
  `LayoutConstraintsCell`. Cheap for size; it cannot see a position that changes without its
  own layout (a scroll). Rejected.
- **Alternative: build the notification layer** (`NotificationListener`, senders). It is the
  gap "No scroll notifications", which is to build on `ScrollObserverScope`
  (`scroll/scroll_observer.rs`, module documentation); geometry does not bubble. Rejected here.

## W20. Sticky rows in a lazy list

- **What exists.** `RenderSliverList` runs on `walk_virtualizer_band`; the geometry reports a
  scroll obstruction. A section per sliver in a `SliverMainAxisGroup` already works (R7's
  group rows) and costs a sliver per section. `PinnedChildren` (ADR-0265) keeps a lazy child
  built outside the band, and it is not the mechanism for a stuck header: by ADR-0265 §2 a
  pinned child is neither painted nor hit and its extent enters nothing. The lazy element
  keeps a child outside the band through two checks that read the children a keep-alive
  lease holds (`crates/flui-view/src/element/sparse_children.rs`): the reconcile carries a
  keyless resident outside the band over without rebuilding it unless it is held, and
  `retain_band` exempts the held children from eviction. A build request for an index
  outside the band is skipped (`sliver_adaptor.rs`), so a header whose row has left the band
  is evicted today and cannot be asked back.
- **Shape.** A mode of the two lists on the shared walk (`RenderSliverList`,
  `RenderSliverVariedExtentList`), in a module of its own, `sliver/sticky_rows.rs` *(proposed)*:
  - `StickyRows` *(proposed)*: the header indices, strictly increasing, built by
    `StickyRows::try_new` and shared by `Rc`. The header at or before an index is a binary
    search.
  - After the walk places the band, the module finds the header `h` for the first visible row
    and the next header `n`. Its stuck position is its natural offset, raised to the leading
    edge of the viewport (after any overlap), and lowered so that it ends where `n` begins.
    That position is committed as the child's paint offset in layout.
  - *One set of children kept outside the band.* `LazyMultiBoxRender` gains
    `kept_outside_band(&self) -> Option<usize>` *(proposed, default `None`)*: the one index
    the last layout wants built though it lies outside the band. Once per service pass the
    manager forms the kept set from the held children and that index, and resolves it to
    the resident elements before the reconcile; an index with no resident (after a jump)
    stays an index. The reconcile, the eviction and the build admission, which today skips
    every request outside the band, read that set and nothing else. A stuck
    header whose builder changed is therefore rebuilt like a row in the band, and a keyed
    header that the reconcile moves to another index is the same kept element afterwards and
    is not evicted there.
  - *One rule lays the stuck header out.* In a pass where `h` lies outside the band the
    sliver lays out its pinned children without `h`, whether or not `h` also holds a lease
    (it has focus), and the sticky rule alone lays `h` out, positions it, stores its extent
    in the index with the anchor correction `set_measured` returns, and adds its part. The
    order is the one commit of [design.md](design.md): band, pinned, sticky, effect. A header
    that is both stuck and focused is shown, which includes published; when another header
    takes over it is a pinned child again for as long as its lease lasts. ADR-0265 §2 speaks
    of pinned children outside the band and keeps its text: for a list with sticky rows the
    band is the contiguous band and the kept index, which the record in
    `crates/flui-view/ARCHITECTURE.md` states.
  - The stuck header is the last part of the list's pass, so it paints over the rows, is hit
    before them and is read where it is painted.
  - The list's geometry reports the stuck header's extent as its scroll obstruction, and the
    list's own answer for "the offset that reveals row `i`" subtracts it.
- **Placement animations.** The stuck header takes no part in the list's `Placement` track: its
  offset is the sticky rule's, so an insert above it does not slide it.
- **Limits, stated in the documentation:** one level pins; the fixed-extent list and the grid
  keep walks of their own and are not covered.
- **Widgets.** `SliverList` and `SliverVariedExtentList` gain `.sticky_rows(..)`; `ListView`'s
  builders pass it through. The lists' configurations in `sliver_adaptor.rs` carry the
  `Rc<StickyRows>`, so `ListConfig` stops being `Copy`.
- **Crates.** `flui-objects`, `flui-view` (the trait method, the manager's kept set and its
  three readers, the configurations), `flui-widgets`. No frozen crate. The kept set is
  recorded in `crates/flui-view/ARCHITECTURE.md`.
- **Alternative: make the header an ADR-0265 pinned child.** It would change that ADR's rule
  for one caller: a pinned child would sometimes paint and take hits. Rejected.
- **Alternative: honour the kept index in eviction and admission only.** The reconcile then
  freezes a keyless stuck header at its old content and evicts a keyed one that moved.
  Rejected.
- **Replaced forms.** None; the group form stays for sections that are slivers of their own.
- **Pitfalls elsewhere (recollection).** Compose's `scrollToItem` landing an item behind the
  sticky header; sticky headers flickering with item placement animations; SwiftUI pinning one
  level only.
- **Alternative: a separate `RenderSliverSectionList`.** A third measured lazy list for one
  feature, with the offset index, the retain band and the pinned rule again. Rejected.
- **Alternative: only the group form.** Ten thousand sections are ten thousand slivers, and an
  index across sections is the application's arithmetic. Kept for what it is good at; not the
  answer for a contacts list.

## W21. Tracks and a grid

- **What exists.** `RenderTable` (`layout/table.rs`) sizes columns with `compute_column_widths`
  and `grow_and_shrink_column_widths` from `TableColumnWidth`: fixed, flex, intrinsic with an
  optional flex, fraction of the available width, and the maximum and minimum of two. It has no
  spans and no placement.
- **The track module** *(proposed, `layout/tracks.rs`)*:
  - `TrackSize` is `TableColumnWidth` under the name its field uses, with the same six
    variants and the same `serde` derive. `TableColumnWidth` is deleted; the migration is the
    rename, and every user moves in the same change: `RenderTable`, `Table` and their tests;
    the re-exports in `crates/flui-objects/src/lib.rs`, `crates/flui-widgets/src/lib.rs` and
    `crates/flui-sdk/src/lib.rs` (`flui_sdk::rendering::objects`), with the two lines of
    `crates/flui-sdk/tests/surface.rs` that pin the old name; and
    `packages/flui-material/src/data_table.rs` with its tests, whose public
    `column_width(TableColumnWidth)` becomes `column_width(TrackSize)`. The facade's surface
    changes by one name, which the changelog fragment states with its replacement.
  - `resolve(tracks, available, gap, query) -> RenderResult<Vec<f64>>` is the table's width
    algorithm moved (`extent_for_spec`, `compute_column_widths`,
    `grow_and_shrink_column_widths`): it is already generic over one closure that answers a
    child's intrinsic size. `RenderTable`'s rows are the proof that nothing changed.
  - `minmax(min, 1fr)` of CSS is `TrackSize::max(min, TrackSize::Flex(1.0))` as the table
    already evaluates the combinator: the width is the larger of the two and the flex is
    carried over.
- **`RenderGrid`** *(proposed, `layout/grid.rs`)*, `Arity = Variable`:
  - *Configuration.* Column tracks, row tracks, the size of tracks added implicitly (content
    by default), a gap per axis, the default alignment in a cell, the text direction.
  - *Per child.* `GridParentData` *(proposed)*: an optional start column and row, a span on
    each axis (at least one), an optional alignment. `GridCell` is the `ParentDataView` that
    writes it.
  - *Placement.* Children that name a cell are placed first; two that overlap are a layout
    error that names both (layering is `Stack`'s job, inside a cell). The rest take the next
    free cell in row order, a spanning one the next free run. No dense packing: the visual
    order never departs from the child order, which is the reading and focus order.
  - *Sizing.* Columns first: a content track takes the widest single-span child's intrinsic
    width; a spanning child's excess over its tracks and gaps is shared equally among the
    tracks it spans that can grow (content or flex), or among all of them when none can; then
    `resolve` distributes the free width. Rows second, with column widths known: a content row
    takes the tallest child at its resolved width. Flex tracks on an unbounded axis are a
    layout error.
  - *A flex track has no content floor.* `TrackSize::Flex` is the table's: a child wider than
    its share overflows its cell and the grid clips it through its `OverflowClip`. The floor is
    asked for explicitly, `Intrinsic { flex: Some(1.0) }`. CSS's implicit `min-width: auto` is
    the opposite default and its best-known surprise.
  - *Intrinsics and dry layout* run the same resolution over the children's intrinsic and dry
    sizes.
- **Not included, and why.** Subgrid (doubles the algorithm); named areas (sugar over explicit
  cells); dense packing (reading order); baseline alignment across a row (a second pass over a
  row's children; added when a caller needs it); overlapping cells.
- **Widgets.** `Grid::new(children).columns(..).rows(..).gap(..)`, `GridCell::new(child)
  .column(..).row(..).column_span(..).row_span(..)`.
- **Crates.** `flui-objects`, `flui-widgets`. `GridParentData` is defined in `flui-objects`:
  `ParentData` is a public trait and a child's offset is stored through `position_child`, not in
  the parent data. Every parent-data type on `main` lives in `flui-rendering`, so this is the
  first one outside it ([design.md](design.md), "Unverified"); if it cannot be, the type is one
  new file in `flui-rendering/src/parent_data/` and the grid's task becomes gated.
- **Later users.** A lazy table on W16's viewport resolves its column widths with `resolve`.
- **Pitfalls elsewhere (recollection).** CSS: a share with a content minimum that overflows; a
  row height that depends on a width resolved in the wrong pass; dense placement and reading
  order. SwiftUI: a cell that asks for infinite width stretches every column.
- **Alternative: keep `TableColumnWidth` and add a `TrackSize` beside it.** Two types with one
  meaning and two resolvers to keep equal. Rejected.
- **Alternative: add spans to `RenderTable`.** A table is rows of cells with borders and a
  per-cell vertical alignment; placement by cell and row tracks are a different object.
  Rejected.
- **Alternative: the full CSS algorithm** (min-content and max-content contributions, intrinsic
  size keywords, fit-content). Most of its length serves features left out above. Rejected for
  the first form; `TrackSize` is `#[non_exhaustive]`.

## W22. A wrap that stops and shows what is left over

- **What exists.** `RenderWrap` (`layout/wrap.rs`, 929 lines) lays every child out in runs.
  `RenderToolbarItems` (`layout/toolbar_items.rs`) treats its last child as an overflow button,
  takes the longest leading run that fits with the button held back, hides the left-over
  children from paint, hits and semantics, and writes the count to a `ToolbarItemsSplit` the
  widget reads after the frame. Its documentation says a `RenderWrap` cannot do this.
- **Shape.**
  - The run rule moves to `layout/leading_run.rs` *(proposed, private)*: given the children's
    main-axis sizes, the spacing, the available extent and a reserve, the number that fit.
    `RenderToolbarItems` calls it and its rows stay green.
  - `RenderWrap` gains `max_runs: Option<NonZeroUsize>` and `indicator: bool` (the last child
    is the indicator, the toolbar's convention). With both set and children left over, the
    last allowed run is split by the rule with the indicator's main-axis size as the reserve,
    and the indicator is placed after it.
  - The hidden children and, when everything fits, the indicator have no part in the wrap's
    `ShownChildren`.
  - Each layout writes a `WrapSplit` *(proposed: shown, hidden)*, the form of
    `ToolbarItemsSplit`.
- **Who relabels the indicator.** A value written in layout wakes nobody:
  `ToolbarItemsSplit` is an `Rc<Cell<..>>` with no notifier, and the toolbar's widget
  rebuilds only when its controller does. `Wrap` with an indicator is therefore stateful. Its
  state owns the `WrapSplit` and installs a wake on it; the wrap calls the wake when the
  hidden count a layout commits differs from the last
  ([design.md](design.md), "A value a frame decided reaches a widget on a change"), and the
  next frame builds the indicator with the new count. A layout that changes nothing wakes
  nobody, and a `Wrap` without an indicator stays stateless and has no wake.
- **The loop, and the rule that ends it.** The indicator's label depends on the hidden count,
  and the count depends on the indicator's size. `RenderWrap` holds a reserve and the key it
  was taken under: the constraints, the wrap's configuration, and the main-axis sizes of the
  children before the indicator, in order. The indicator's own size is not in the key. While
  the key is unchanged the reserve is the wider of the indicator now and the stored reserve,
  so a relabel can only widen it; when the key changes the reserve starts again from the
  indicator's size. A wider reserve hides the same number of children or more, so under one
  key the hidden count only grows and is bounded by the number of children: the relabelling
  ends, after at most one rebuild per child and one in the usual case (a label that grew by
  a digit). A label that narrows as more is hidden ("99+") leaves the reserve where it was
  and ends the same way. Each step is a frame, so the label is one frame behind the split
  while it settles and never wrong about a split that has settled. The split is stable at
  the price of a little room, as the toolbar's is, which holds back "the wider of the button
  now and the button of the last collapsed layout".
- **Documents.** The module documentation of `toolbar_items.rs` ("That is what a `RenderWrap`,
  an offstage child or a layout delegate cannot do") and the mapping decision "A toolbar row
  hides…" in `crates/flui-objects/ARCHITECTURE.md` ("An offstage child, a `RenderWrap` or a
  layout delegate cannot hide a child by what layout decides") are rewritten in the same
  change: a wrap with `max_runs` now can, and the toolbar remains for its expanded forms.
- **The objects stay separate.** The toolbar's expanded state (a column, more columns, runs) is
  its own; `RenderOverflowBar` switches the whole axis. They share the run rule and the shown
  answer, not a type.
- **Widgets.** `Wrap` gains `.max_runs(..)` and `.overflow_indicator(|hidden| ..)`.
- **Crates.** `flui-objects`, `flui-widgets`.
- **Pitfalls elsewhere (recollection).** Compose's `FlowRow` overflow lays the indicator out
  once with a count and accepts a one-step lie.
- **Alternative: build the indicator during layout** (ADR-0017), so its label is exact in the
  frame. A build-during-layout fixpoint under every chip group; the post-frame label is one
  frame late on a change of the count and never wrong in the split. Rejected.
- **Alternative: a `max_items` limit.** It is a slice of the children, which the application
  can take before building. Not offered.

## W23. One placement policy for a floating child

- **What exists.** Two private single-child layout delegates: `MenuPlacement`
  (`crates/flui-widgets/src/popup_menu.rs`), which loosens the constraints, flips across the
  anchor and then clamps, so a menu that fits on neither side covers its anchor; and
  `ToolbarLayout` (`crates/flui-widgets/src/text/selection_menu/toolbar.rs`), which goes over
  or under its anchors with a gap. A layout delegate's methods take `&self`, and a delegate
  is made anew on each build. No tooltip, dropdown or autocomplete exists yet; each would be
  a third.
- **Shape** *(proposed, `crates/flui-widgets/src/overlay/placement.rs`)*:

  ```rust
  #[non_exhaustive]
  pub enum PlacementSide { Above, Below, Start, End }

  /// What happens when the child fits on no listed side.
  #[non_exhaustive]
  pub enum NoFit {
      /// The child keeps its size and may cover the anchor.
      Overlap,
      /// The child is limited to the roomiest listed side, so it always fits there.
      Shrink,
  }

  pub struct AnchoredPlacement { /* sides in order, alignment, gap, margin, no_fit */ }

  #[non_exhaustive]
  pub struct Placed {
      pub offset: Offset,
      pub side: PlacementSide,
      pub covers_anchor: bool,
      pub anchor_in_area: bool,
  }

  impl AnchoredPlacement {
      /// The room beside the anchor on one side.
      pub fn room(&self, anchor: Rect, area: Rect, side: PlacementSide) -> Size;
      /// The largest size the child may take, before a side is known.
      pub fn limit(&self, anchor: Rect, area: Rect) -> Size;
      pub fn place(
          &self,
          anchor: Rect,
          child: Size,
          area: Rect,
          direction: TextDirection,
          current: Option<PlacementSide>,
      ) -> Placed;
  }
  ```

  Pure functions, and one `SingleChildLayoutDelegate` over them.
- **The order of the rules, fixed.** A delegate must constrain its child before it knows the
  child's size, and the side depends on that size, so the limit cannot depend on the side
  chosen. `limit` is therefore one of two side-independent sizes: under `NoFit::Shrink`, the
  room of the roomiest listed side, so whatever the child's size it fits there; under
  `NoFit::Overlap`, the area less its margin. Then `place` takes the first listed side whose
  room holds the child, keeping `current` while the child still fits on it. Then the result
  is shifted along the side to stay inside the area less its margin.
- **When no side fits.** Only under `Overlap`. `place` takes the listed side with the most
  room, clamps the child into the area and returns `covers_anchor: true`; the result is
  finite and inside the area whenever the child is no larger than it. That is what the popup
  menu does today, now stated and reported. A caller that must never cover its anchor (a
  tooltip, an autocomplete under a field) asks for `Shrink` and gives its child a scrolling
  body.
- **Stability, and where the side is remembered.** `current` is kept while the child still
  fits on it, so an anchor at the threshold does not flip on every frame. The delegate cannot
  hold it: it is rebuilt each build. The popup's state owns a `PlacementMemory` *(proposed,
  an `Rc<Cell<Option<PlacementSide>>>`)* created in `create_state` and hands it to each
  build's delegate, which reads it before and writes it after `place`. It is state only the
  owner thread touches, written in layout with no user code run, and it takes no part in the
  delegate's `should_relayout`. It lives as long as the popup, so a new popup starts with no
  side.
- **The area.** Every caller gets it from one function, `PlacementArea::of(ctx)` *(proposed)*:
  the overlay's rectangle less the `MediaQuery` view insets and padding (the keyboard, a
  cutout), which makes the popup rebuild when the keyboard moves. The caller's margin is
  applied on top. No popup subtracts an inset by hand.
- **Edge cases.** A child larger than the area is placed at the area's start and the result is
  finite. An anchor outside the area gives `anchor_in_area: false`, and the caller hides or
  keeps the child.
- **Replaced forms.** `MenuPlacement` and `ToolbarLayout` are deleted in the same change; the
  popup menu configures the delegate with `NoFit::Overlap` and its present sides, the
  selection toolbar with its over-and-under order and gap, and their placement tests stay as
  they are. `packages/flui-cupertino/src/navigation_toolbar.rs` has a type of its own named
  `ToolbarLayout`, a multi-child delegate for a navigation bar; it is another concept and
  stays.
- **Crates.** `flui-widgets` only. No render object.
- **Name.** `Placement` in `flui-objects` is the slide of a lazy list's item, a different
  concept ([design.md](design.md), "Decisions for the owner", 11).
- **Pitfalls elsewhere (recollection).** Popup libraries' flip oscillation and their rules
  against it; the order of size limit and flip; anchors scrolled out of view.
- **Alternative: a limit per side, and a second layout of the child once the side is known.**
  Exact room on the chosen side, and a child laid out twice whose size on the second pass can
  move it to another side. Rejected.
- **Alternative: a render object that reads the anchor through a leader and follower.**
  `Leader` and `Follower` place at composite time, after layout, so the child's size limit
  cannot depend on the side. The policy is a layout input. Rejected.

## W24. First that fits

- **Shape.** `RenderFirstThatFits` *(proposed, `layout/first_that_fits.rs`)*, `Arity =
  Variable`, with `FitAxes { Horizontal, Vertical, Both }` *(proposed)*.
- **Layout: each child once per pass, under one set of constraints.** Every alternative but
  the last is laid out, in order, under the *probe constraints*: the incoming ones loosened,
  with the maximum lifted on the chosen axes. The first whose size is finite and within the
  incoming maximum is the choice, and it is shown at that size; it is not laid out again. The
  object's size is that size constrained by the incoming constraints, with the child at the
  start. The last alternative is never probed: when no earlier one fits it is laid out under
  the incoming constraints, once.
- **Why not probe and then lay out again.** A child that builds during layout
  (`LayoutBuilder`) is rebuilt for each distinct set of constraints it is laid out under.
  Two layouts per pass under two sets rebuild it twice a pass, and the build-during-layout
  fixpoint sees its constraints change on every pass. With one set per child per pass the
  fixpoint settles as it does for any parent, inside ADR-0308's bound. The documentation
  states the consequence: a
  `LayoutBuilder` inside an alternative that is not the last sees the lifted maximum on the
  chosen axes, not the incoming one.
- **A probe is a real layout.** A dry layout would be cheaper and cannot answer for a child
  that builds during layout: `RenderLayoutBuilder::compute_dry_layout` returns zero and
  reports through `tracing::error!`.
- **Shown.** Only the choice is in `ShownChildren`. The others stay mounted, so their state
  survives a switch; they are not painted, not hit and not published.
- **Alternatives after the choice** are not laid out in that pass
  ([design.md](design.md), "Unverified"); if the pipeline does not accept a mounted child that
  a layout skips, they are laid out under the incoming constraints, which costs a layout each
  and changes nothing visible.
- **Intrinsics** are the first alternative's; the dry layout makes the same choice from the
  children's dry sizes. With an alternative that builds during layout both inherit
  `RenderLayoutBuilder`'s zero answer and its report, as any parent of one does; the
  documentation says a parent that asks for intrinsics or a dry layout needs alternatives
  that can answer.
- **Documentation states:** an alternative that fills its constraints has no finite size when
  the limit is lifted and never fits; it belongs last.
- **Widget.** `FirstThatFits::new(children).axes(..)`.
- **Crates.** `flui-objects`, `flui-widgets`.
- **Replaced forms.** None. `OverflowBar` is the special case "a row, else a column of the same
  children"; this object chooses between different children.
- **Pitfalls elsewhere (recollection).** SwiftUI's `ViewThatFits` instantiates every
  alternative and tests the ideal size, so a flexible one always fits.
- **Alternative: `LayoutBuilder` and a comparison in the application.** The application must
  know each alternative's size without building it. That is the case the object exists for.
- **Alternative: build only the choice.** State is lost on every switch, and the choice needs a
  build during layout. Offered by `LayoutBuilder` already.

## W25. Per-item scroll effects

- **What exists.** `RenderSliverCarousel` computes a clip per item in layout. The wheel (W15)
  computes a transform per item. Both are single objects; a list has no way to ask.
- **Shape** *(proposed, `sliver/item_effect.rs`)*:

  ```rust
  pub trait ItemEffect {
      fn appearance(&self, progress: ItemProgress) -> ItemAppearance;
      fn should_relayout(&self, old: &dyn ItemEffect) -> bool;
  }

  /// Where the item lies in the viewport: 0.0 at the leading edge, 1.0 at the trailing one.
  #[non_exhaustive]
  pub struct ItemProgress { pub leading: f64, pub trailing: f64 }

  #[non_exhaustive]
  pub struct ItemAppearance { pub transform: Option<Matrix4>, pub opacity: Option<f32> }
  ```

  `ItemProgress::centre()` gives the signed distance of the item's centre from the viewport's,
  in viewport extents, for effects written about the centre. One definition; the two
  conventions other frameworks split over are both derived from it.
- **Where it runs.** The four lazy slivers take an optional `Rc<dyn ItemEffect>`. After a
  layout places the band, the effect is evaluated for each placed child as one step of the
  single function that builds the sliver's parts ([design.md](design.md), "One commit per
  layout"): band, pinned, sticky rule, effect, one commit. The effect does not commit, and it is not
  applied to a stuck header, whose part the sticky rule has already placed over the rows; a
  header that scaled or faded with the row under it would no longer cover it. A scroll lays
  the sliver out and rebuilds nothing. The transform is about the item's own box; the item's
  layout is untouched, so scroll extent and positions are the same with and without an
  effect.
- **Hit testing and semantics** go through the sliver transform seam
  ([design.md](design.md)): the item is hit and published through the transform it is painted
  with. This is why the item is gated by its dependency and edits no frozen crate itself.
- **Cost.** A transform is a canvas transform. An opacity below one costs a layer for that
  item, which the documentation says; an effect that returns no opacity costs none.
- **Not included.** A blur per item: a filter is a paint effect of the item's own subtree
  (W13). The carousel keeps its own geometry; moving it onto `ItemEffect` is not in this spec.
- **Widgets.** `.item_effect(..)` on `SliverList`, `SliverFixedExtentList`,
  `SliverVariedExtentList`, `SliverGrid` and the `ListView` and `GridView` builders.
- **Pitfalls elsewhere (recollection).** Hit testing against the untransformed rectangle;
  effects driven by a scroll listener that rebuilds every frame.
- **Alternative: a widget per item that reads the scroll position.** A rebuild or a repaint
  listener per item per frame, and a transform the list's hit test does not know. Rejected.

## W26. A blend group and a widget mask

- **What exists.** `flui-layer` has `OpacityLayer`, `ColorFilterLayer`, `ImageFilterLayer`,
  clips and transforms. Only `ShaderMaskLayer` and `BackdropFilterLayer` carry a blend mode; no
  layer composites an isolated group with one. `ShaderMask` masks by a shader, not by a widget.
- **One primitive.** A group layer *(proposed, `flui-layer`)*: its children are drawn into an
  isolated group, and the group is composited with a blend mode. A mask is two of them: the
  content in a source-over group, and inside it, after the content, the mask child in a
  destination-in group.
- **Shape.** The group's blend mode is a field of `PaintOpacity`, the shape
  `crates/flui-rendering/ARCHITECTURE.md` plans ("`blend` on `PaintOpacity`": an "additive
  field on the `#[non_exhaustive]` struct"), so a node's group is composited once with an
  alpha and a mode ([design-parity.md](design-parity.md), W13, "Order"). A node that asks
  for a mode other than source-over, or for isolation, gets a group even at full opacity.
  - `RenderBlendGroup` *(proposed, `proxy/blend_group.rs`)*: a proxy whose `paint_effects`
    returns the mode. Under an opacity on the same node the group fades and blends as one;
    an opacity in an ancestor composites the already-blended result.
  - `RenderMask` *(proposed, `proxy/mask.rs`)*: two children, content first. Its own effects
    ask for an isolated source-over group; inside it the mask child is painted after the
    content in a destination-in group. The mask child is laid out tight at the content's
    size. It is painted, in its group, and is neither hit nor published: the object's
    `hit_test` asks only the content and `visits_child_for_semantics` is false for the mask.
- **No layer when there is nothing to composite:** an empty or fully transparent group pushes
  none.
- **Gate.** `flui-layer` (the layer), `flui-engine` (lowering it to an offscreen group),
  `flui-rendering` (the field on `PaintOpacity`, and the records that state the composite:
  the "Deferred producers" row and the documentation of `PaintEffects`). It follows the
  subtree filters, which add the first new field.
- **Widgets.** `BlendGroup::new(mode, child)`, `Mask::new(content, mask)`.
- **Cost.** An offscreen group per use; unmeasured ([design.md](design.md), "Unverified"). The
  ADR measures it before the item is accepted.
- **Pitfalls elsewhere (recollection).** A blend mode outside an isolated group blends with
  whatever is behind it; clear and destination-in are silently wrong without an offscreen
  group; anti-aliasing seams at a mask's edge at fractional offsets (R26's row).
- **Alternative: `PaintContext::with_blend_group` scopes.** As for the filters: a scope is
  inside the object's paint, so a change of mode repaints the subtree.
- **Alternative: compose a mask from `ShaderMask` and a snapshot of the mask widget.** The
  snapshot is an image made in an earlier frame. Rejected.

## Deferred

Not built by this spec; each has the condition under which it returns.

| Candidate | Why not now | Returns when |
|-----------|-------------|--------------|
| Child-declared scroll snap | `SnapPolicy` is an open trait with three implementors, so a settle rule is already pluggable. A child-declared target for lazy items needs "the snap offset of item `i`" from the offset index | A third scrollable needs targets declared by its children |
| Typed `Layout` trait with a cache and per-child values | It replaces `CustomMultiChildLayout`, whose delegate lives in `flui-rendering` (frozen) and has two production users (`packages/flui-material/src/scaffold.rs`, `packages/flui-cupertino/src/navigation_toolbar.rs`). The rule it would enforce for hidden children is the shown answer's | The freeze lifts; the shown answer's record is the result type it returns |
| Masonry sliver | An item's lane depends on every earlier extent: a lane index beside the offset index, stable in both scroll directions | The varied-extent list and sticky rows have shown the walk's shape holds for a second index |
| Multi-slot build during layout | Generalises ADR-0017 from one slot to keyed slots: an ADR, a `flui-view` element, a bound on passes | A catalog widget needs "build as many as fit" and `LayoutBuilder` cannot give it |
| Flex shrink and basis; wrap with grow | Wanted by code ported from the web; nothing is blocked. `flex.rs` is 1,166 lines, so it is a module beside it | After the grid, which removes the commonest reason to want it |
| A sticky box in a plain scroll view | Needs the scroll offset at paint time or a relayout per scroll step for one box | A caller needs it outside a lazy list |
| Balanced text wrapping | A width search over paragraph layouts; `paragraph.rs` is about to change under W14 | W14 has landed |
| Inset host for an overlaid bar | A composition over W19 with a one-frame lag on first layout | The lag is seen; then a two-child layout, small |
| Split view | A catalog widget over flex and a drag handle | The desktop catalog needs it |
| Viewport-relative sizing | A value published at build is one layout behind; `LayoutBuilder` outside the scroll view answers it | — |

## Rejected

| Candidate | Reason |
|-----------|--------|
| Alignment guides, last baseline | A second alignment query on the render protocol, in `flui-rendering`. Not a change this spec may make for a feature no caller has asked for |
| Public pluggable lazy layout | It would publish the request strategy, the retain band and the pinned rule as API. `walk_virtualizer_band` is that seam, kept `pub(crate)`, and W8, W9 and W20 are its users |
| Lazy wrap | Runs of unknown width over lazy children need the item above. No caller |
| Overlapping slivers; animated paint extent | A background behind a list is `DecoratedSliver`; a pinned overlay is a header in a group; an animated extent is the tree's reveal, which is not exported as a wrapper until a second caller exists |
| Recycling by content type | Creating an element and a render object is cheap here, and recycling keeps state that belongs to another item. Measure first |
| Constraint-solver layout | Chains, barriers and bias are a specification of their own; a package over a layout trait |
| Lookahead and shared-element layout as a render primitive | Doubles layout; a slide between two observed rectangles (W19) and `RenderAnimatedTransform` reach most of it as a package |
| `content-visibility` | Lazy lists are FLUI's answer; remembered sizes make the scroll extent jump, and skipped content must still be published |
| User shaders on a subtree | A WGSL surface with the uniformity rule of AGENTS.md and a portability matrix, for a niche. After the paint-effect ADR settles |
| Subgrid, named areas, dense placement | W21, "Not included" |
| Multi-column text, floats, exclusions | Fragmentation across boxes; neither constraints-down layout nor the text shaper offers it |
| A container-query render object | `LayoutBuilder` is it |
