# ADR-0053: One element-owned lazy child lifecycle for every multi-box sliver

- **Status:** Accepted (2026-09-03)
- **Date:** 2026-09-03
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-view/src/element/sliver_adaptor.rs` (`LazyMultiBoxRender`,
  `SliverMultiBoxAdaptor<R>`, one `ChildManager`, one behavior, one element),
  `crates/flui-objects/src/sliver/` (`RenderSliverList`, `RenderSliverFixedExtentList`,
  the lazy grid, the shared band walk), `crates/flui-widgets/src/scroll/`.
- **Related:** [ADR-0003](ADR-0003-virtualization-core-and-reentrant-build.md) (+ amendment),
  [ADR-0017](ADR-0017-build-during-layout-callback-seam.md) (+ amendment),
  [ADR-0051](ADR-0051-anchor-stationary-scroll-correction.md),
  [ADR-0052](ADR-0052-lazy-sliver-child-identity-and-recovery.md); issue #530.

## Context

Flutter has one `SliverMultiBoxAdaptorElement` and one `RenderSliverMultiBoxAdaptor` base under
`SliverList`, `SliverFixedExtentList`, and `SliverGrid` (`widgets/sliver.dart`,
`rendering/sliver_multi_box_adaptor.dart`). FLUI reached the same family through three
generations of code that coexisted: a fully eager fixed-extent list and eager grid (every child
attached as a dense child, `O(n)` per frame, no residency), a render-owned lazy list that built its
own children mid-layout through a deferred-mutation queue (never wired to a view), and the
request-strategy list and lazy grid whose children the element tree builds between layout passes.
Two copies of the adaptor element existed for the last two, differing only in the render type
reached by downcast and in one config field. `ListView::new(children)` and
`GridView::count(children)` — the static constructors most apps reach for first — were the eager
objects, so their materialised child count was unbounded, and the parity pin for Flutter's
"offscreen children are not built" stayed `#[ignore]`.

### Market

- **Flutter:** one element (`SliverMultiBoxAdaptorElement`) over a `SliverChildDelegate`
  (`build(index)`, `findIndexByKey`, `estimatedChildCount`, `shouldRebuild`); the render side
  varies (`RenderSliverList`, `RenderSliverFixedExtentBoxAdaptor`, `RenderSliverGrid`) over one
  `RenderSliverBoxChildManager` protocol (`createChild`, `removeChild`, `didAdoptChild`,
  `setDidUnderflow`). The behavioural reference here.
- **Jetpack Compose:** `LazyLayout` takes one `LazyLayoutItemProvider` (`itemCount`,
  `Item(index)`, `getKey(index)`, `getIndex(key)`) and every lazy container (`LazyColumn`,
  `LazyVerticalGrid`, `LazyStaggeredGrid`, `Pager`) is a measure policy over that one provider —
  the same split as decision 1: one provider contract, several layouts.
- **SwiftUI:** `List`/`LazyVStack` are keyed by `Identifiable` data with no index protocol exposed;
  no reusable contract to borrow.
- **Rust:** Xilem's `virtual_scroll` and Iced's `lazy` widgets virtualise one list each with no
  shared child-lifecycle protocol between list and grid; GPUI's `uniform_list` is a fixed-extent
  list over an item-count and a render callback, with no grid sibling. None offers a contract a
  third layout can join.

## Decision

1. **One adaptor element, generic over the render object.** `LazyMultiBoxRender` (`Config`,
   `create`, `update`, `item_count`, `set_item_count`, `KIND`) is the whole contract a multi-box
   sliver owes the element tree; `SliverMultiBoxAdaptor<R>` carries `{ config, item_count,
   builder, find_index_by_key }` and is constructed through `with_config`, so a render object
   outside `flui-view` joins by implementing the trait; one `ChildManager`, one behavior, one
   element. `SliverList`, `SliverGrid` (the transitional `SliverGridLazy` name retired once the
   eager grid went) and `SliverFixedExtentList` are type aliases with their own constructors.
2. **Every multi-box sliver is on the request strategy.** The render object asks
   (`request_child_build`, `emit_retain_band`); the element tree builds between the frame's layout
   passes (ADR-0017's fixpoint, the lazy-band budget) and evicts by band. The eager fixed-extent
   list is rewritten on it with Flutter's index math; the eager grid is deleted in favour of the
   lazy one; the render-owned lazy list, the deferred-mutation queue and the
   `LogicalIndexParentData` stamp were deleted (ADR-0003 amendment).
3. **Clamp, not correction, for a shrinking source.** A builder answering `None` below the count
   clamps the render object's count; the next pass reports the real extent and the viewport clamps
   `pixels`. Flutter's `scrollOffsetCorrection` on a leading-insert failure has no trigger under
   the request strategy (the object cannot observe an insert failing mid-layout), so the
   fixed-extent list never emits one. Divergence: a non-monotone builder truncates at its first
   `None` where Flutter teleports. ADR-0051 keeps the measured list's anchor-stationary correction.
4. **The frame owns the committed band.** Paint, hit-test and semantics never see a resident
   outside the last committed band: the fixpoint evicts before it paints when the budget trips,
   and the walk lays out everything it positions (ADR-0017 amendment). The pipeline
   "positioned this pass" stamp is the recorded follow-up.
5. **Static children are a delegate, and delegates compare by identity.** `ListView::new` /
   `GridView::count|extent` hand their children to `StaticChildren` (Flutter's
   `SliverChildListDelegate`): built by index, keys preserved and salted through the per-item
   `RepaintBoundary` (ADR-0052), a lazily filled key → index map for keyed reconciliation. An
   update whose builder and key callback are the same `Rc` and whose count is unchanged does not
   refresh the residents (Flutter's `shouldRebuild`: `children != oldDelegate.children` for a
   list, always `true` for a builder — a fresh closure per build never compares equal). The
   adaptor's render update therefore no longer forces a relayout; a changed delegate marks it.

## Recorded divergences and floors

- `_replaceMovedChildren` is effectively always on (ADR-0052); `hasVisualOverflow` follows
  Flutter's `targetLastIndexForPaint` formula for the fixed-extent list.
- The precision tolerance of the fixed-extent index math is `f32`-scaled (`1e-3` px against
  Flutter's `1e-10` double); the reference's rounding regression tests are ported with nudges on
  the same side of the tolerance.
- Shrink-wrap in an unbounded parent materialises all N (Flutter-equal: `targetLastIndex == null`
  lays out to the end). A `usize::MAX` count with a finite window reports an absurd finite extent
  where Flutter's builder delegate with a null count binary-searches the end — growable count is a
  follow-up. Under an unbounded window a sentinel count is truncated as the lazy grid does.
- `semanticBounds` fallback, `addAutomaticKeepAlives`, `addSemanticIndexes`: not ported;
  follow-ups.
- The eager grid's stale-tile harness pins are replaced by the frame-level deferral tests: the
  gate moved from the object to the frame.

## Consequences

- One adaptor implementation instead of two (`sliver_adaptor.rs` −758 / +460 lines at the
  unification), and one door for a third layout.
- `ListView::new` / `GridView::count|extent` become bounded in materialised children; the
  `SliverFixedExtentList` parity pin is un-ignored.
- A downstream implementor of `LazyMultiBoxRender` owes the harness catalog entry and tests every
  render object owes (`crates/flui-rendering/docs/TESTING.md`).

## Status of the decisions

Decision 1 landed with the unification and decision 4 with ADR-0017's amendment. Decisions 2, 3
and 5 landed with the fixed-extent port (`RenderSliverFixedExtentList` on the request strategy,
`StaticChildren`, `ListView::new` over it, the un-ignored residency pin and the clamp-contract
ports of the two auto-correct cases). The grid is done too: the eager `RenderSliverGrid` and
`SliverGridParentData` are deleted, the request-strategy grid took the `RenderSliverGrid` /
`SliverGrid` names (the transitional `RenderSliverGridLazy` / `SliverGridLazy` names are gone),
and `GridView::count`/`GridView::extent` route over `StaticChildren` exactly as `ListView::new`
does. All five decisions have landed.
