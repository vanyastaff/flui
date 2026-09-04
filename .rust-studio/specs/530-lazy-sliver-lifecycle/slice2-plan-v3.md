# Slice 2 plan v3 — one element-side lifecycle for list / fixed-extent / grid

Supersedes v2 after its review (spec §12). Lands after slice 3 (#828) as PRs A–D:
A = step 4 (#829, stacked on #828), B = step 1, C = steps 2–3, D = steps 5–7.

## Step 1 — the walk lays out everything it positions; stale residents never reach paint
- `virtualized_band.rs` 4a': widened-band children that are already attached are laid out
  (`layout_box_child`) and fed to `set_measured`, exactly like step 4; only absent ones are requested.
- Hit-test keeps the layout-time `attached_child_count` snapshot (every multi-child box object
  uses the same pattern): with evict-before-paint, the post-frame service only APPENDS children
  (`insert_child_render_object` appends; relocation never moves render slots) — an appended,
  not-yet-laid-out child is zero-sized and invisible to the snapshot, which is harmless. The
  live-count accessor is not needed once the ordering hole is closed; recorded here, not built.
- **Evict-before-paint on the deferral path** (`owner/layout_builder.rs`): when the lazy-band budget
  trips, apply the last pass's retain bands as an evict-only service (`SparseChildren::retain_band`
  through a `ChildManager::evict_outside(first, last)` entry) and run one more layout pass, then
  `run_frame`. No resident outside the committed band exists at paint, hit-test, or semantics time.
- Test: budget 0 + a scroll jump past the residents → the frame's display lists carry no head
  row's colour (`painted_rect_colors` walks the layer tree; red without the evict: 45 stale
  rows painted). Harness: `harness_sliver_list_lays_out_attached_children_the_widening_pulls_into_band`
  (55 px vs 230 px). Lesson: the evict-only pass must LEAVE the build requests queued — taking
  them cost the deferred band a frame (`…exhausted_pass_budget_defers_the_rest…` went red).
- ALT-1 (pipeline "positioned this pass" stamp) recorded as the eventual class fix (ADR follow-up).

## Step 2 — `RenderSliverFixedExtentList` on the request strategy
- `ParentData = SliverMultiBoxAdaptorParentData`; `new(item_extent, item_count)`, `item_count()`,
  `set_item_count()`.
- Flutter index math (`sliver_fixed_extent_list.dart:326-495`) for first/last/geometry;
  `hasVisualOverflow` from `targetLastIndexForPaint` (recorded).
- Emits `emit_retain_band` every pass (zero-count → `(0,0)`, window past the end → `(first,first)`).
- **No `scroll_offset_correction`.** Builder `None` ⇒ manager clamp ⇒ `count × extent` ⇒ viewport
  clamp. Widget test asserts the clamp chain's END: after the source shrinks below the offset,
  `ScrollController::pixels()` clamps to `count × extent − viewport`.
- Unbounded window: sentinel guard as the lazy grid; recorded: shrink-wrap in an unbounded parent
  materialises all N (Flutter-equal); `usize::MAX` count with a finite window reports an absurd
  finite extent where Flutter binary-searches the end (growable count follow-up).
- Harness rewrite for the ~20 scaffold sites (`(extent, count)` + seeded parent data),
  `flui-rendering/tests/sliver_fixed_extent_list.rs`, `harness_self_test.rs`, `render_inspector.rs`.

## Step 3 — `RenderSliverGrid` = today's lazy grid
- Rename; delete the eager grid. `SliverGridParentData` folded into `SliverMultiBoxAdaptorParentData`
  (`cross_axis_offset`); one downcast target in `append_sparse_sliver_children`;
  `not_parent_data.stderr` regenerated. Keep the `laid_out_band` diagnostics name where the
  harness pins read it; band convention half-open `[first, last)` everywhere.
- The eager grid's four stale-tile pins become the step-1 widget-tier deferral tests (the gate moved
  from the object to the frame); documented in the ADR.

## Step 4 — delete the render-owned seam and the deferred-mutation queue
- Delete `RenderSliverListLazy`, `OffBandDisposal`, the walk's fallback/dispose closures,
  `build_and_layout_box_child` (grid's resident layout → `layout_box_child`), `dispose_box_child`,
  `PendingBuild`/`pending_builds`, `ChildLayout::Ready` (the enum keeps `Scheduled`/`NoChild`/
  `Unwired`; `request_child_build`'s return type narrows accordingly — public flui-rendering change),
  `pipeline/deferred.rs` + `defer_*` + `apply_deferred_mutation`, `LogicalIndexParentData` and its
  port-check entry; move the `!Send` rationale in `subtree_arena.rs`.
- Retarget: `lazy_sliver_list_child_build_contract.rs` (9 tests → deleted; the request contract is
  covered by `lazy_list.rs` + harness), `attach_detach_lifecycle.rs` deferred tests, the two
  sliver-level benches (→ `RenderSliverList` with seeded residents), `harness_snapshot.rs`'s two
  snapshots, trybuild `not_a_view.stderr`, `workspace-layers.toml`, doc mentions, ROADMAP citations.
- ADR-0003 amendment: adopts rejected alternative (c) knowingly — whole-pass granularity inside the
  in-frame fixpoint; a mid-pass backend later is a breaking change to `SliverLayoutCtxErased`.

## Step 5 — one adaptor element
- As v2 (`LazyMultiBoxRender`, `SliverMultiBoxAdaptor<R>`, one behavior/manager, typed wrappers
  keeping `SliverList::{new, separated, list}`, `SliverFixedExtentList`, `SliverGrid`).
- Extend the existing `SliverList::list` delegate with the key map (hash bucket + `key_eq`, built on
  first lookup, tied to the `Rc`'s identity); `should_rebuild = !Rc::ptr_eq(old, new)` for list
  delegates, whose render update therefore does not force `LAYOUT` (builder delegates keep it).

## Step 6 — widgets
- `ListView::new` / `GridView::count|extent` onto the list delegate. Gate list with dispositions:
  `scroll_controller_test.rs:129` (capture the first mounted child after the jump),
  `sliver_fixed_extent_list_test.rs:398` (un-ignore; rewrite its doc + manifest), the two
  `grid_view_interaction_test.rs` suspects (assert on-stage tiles, not "all mounted"). Keyed
  `StatefulView` reorder test on `ListView::new`; `setDidUnderflow` look-ahead pin.

## Step 7 — record (ADR-0053)
Deletions, clamp contract, evict-before-paint, ALT-1 as the follow-up, `_replaceMovedChildren`
everywhere, `hasVisualOverflow` formulas, `semanticBounds` fallback dropped, keep-alive → follow-up,
shrink-wrap/`usize::MAX` divergences.

## PR C design notes (one adaptor element) — from reading `sliver_adaptor.rs` (1967 lines)
The two managers and two behaviors differ only in the render-object type (`RenderSliverList` /
`RenderSliverGridLazy`, reached by `downcast_render_object[_mut]` in `item_count` /
`clamp_render_item_count`) and the view config (`item_extent_estimate: f32` / `grid_delegate`).
- `pub trait LazyMultiBoxRender: RenderSliver<Arity = Variable, ParentData = SliverMultiBoxAdaptorParentData> + 'static`
  with `type Config: Clone + Debug`, `fn create(config: &Config, item_count) -> Self`,
  `fn update(&mut self, config: &Config) -> RenderUpdateImpact`, `fn item_count(&self)`,
  `fn set_item_count(&mut self, n) -> RenderUpdateImpact`. Impls in flui-view for both objects
  (orphan rule: trait here, types in flui-objects). Fixed-extent joins in PR D.
- `pub struct SliverMultiBoxAdaptor<R> { config: R::Config, item_count, builder, find_index_by_key }`;
  `pub type SliverList = SliverMultiBoxAdaptor<RenderSliverList>` keeps `new` / `separated` /
  `list` / `find_index_by_key` as inherent impls on the concrete instantiation;
  `pub type SliverGridLazy = SliverMultiBoxAdaptor<RenderSliverGridLazy>` keeps `new`.
  `update_render_object` = `set_item_count | update(config) | LAYOUT` (the unconditional LAYOUT is
  load-bearing for `needs_resident_refresh`; PR D relaxes it for identity-comparable list delegates).
- One `SliverAdaptorManager<R>` (today's list manager + `PhantomData<R>`), one
  `SliverAdaptorBehavior<R>`, `impl<R> RenderElementBase<Variable> for Element<SliverMultiBoxAdaptor<R>, Variable, SliverAdaptorBehavior<R>>`.
  `create_element` boxes into `ElementKind::RenderVariable`. `hosts_sparse_children` true.
- Widgets keep calling `SliverList::new` / `SliverGridLazy::new` (`list_view.rs:253`, `grid_view.rs:257`).
- Tests: the module's unit tests (`service_returns_*`, constructors) move onto the generic types;
  `lazy_grid.rs` / `lazy_list.rs` are the behaviour pins and must not change.

## PR D design notes (fixed-extent on the request strategy, list delegate, widgets)
- Flutter floor read 2026-09-03 (`sliver_fixed_extent_list.dart:326-500`, `scroll_delegate.dart:633-795`):
  `firstIndex = getMinChildIndexForScrollOffset(scrollOffset + cacheOrigin)`, `targetLastIndex` from
  `scrollOffset + remainingCacheExtent` (null when infinite → lay out to the end), garbage outside
  `[first, last]`, leading-insert failure → `scrollOffsetCorrection = index × extent` (FLUI: clamp
  contract instead, recorded), trailing-insert failure → `estimatedMaxScrollOffset = index × extent`,
  `hasVisualOverflow = lastIndex >= targetLastIndexForPaint || scrollOffset > 0`,
  `setDidUnderflow(estimatedMax == trailing)`.
- `SliverChildListDelegate`: `_keyToIndex` filled lazily with a cursor (`_keyToIndex[null]`), keys
  unsalted before lookup, `shouldRebuild = children != oldDelegate.children` (identity). FLUI:
  `SliverList::list`'s `Rc<Vec<BoxedView>>` gets a `OnceCell`-style map keyed by `key_hash` with
  `key_eq` inside the bucket, and `should_rebuild = !Rc::ptr_eq`; the adaptor's render update drops
  the unconditional `LAYOUT` only for that delegate (builder delegates stay opaque).
- Widgets: `ListView::new(extent, children)` → `SliverFixedExtentList` over the list delegate
  (`list_view.rs:269`); `GridView::count/extent` → grid over the list delegate (`grid_view.rs:278`);
  `SliverFixedExtentList` view moves to the adaptor (`sliver_fixed_extent_list.rs`).
- Un-ignore `sliver_fixed_extent_list_offscreen_children_are_not_built_on_initial_window_pin`
  (its doc + manifest + ROADMAP Cross.H entry rewritten); gate list from spec §12 #14.
