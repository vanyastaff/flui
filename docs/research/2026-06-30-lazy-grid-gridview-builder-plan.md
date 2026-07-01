# Lazy 2D grid (RenderSliverGridLazy → GridView.builder) — plan

Build-ready, oracle-verified. Enables `GridView.builder` (the common large/infinite
grid). No infra gap, no ADR.

## Headline (de-risking)
The **element half is grid-agnostic and reused wholesale**; only a small
delegate-windowed render object is new. `GridView.builder` goes through the
**element-owned request strategy** (the path `ListView.builder` uses), NOT the
render-owned `build_and_layout` primitive and NOT `walk_virtualizer_band` (the
Virtualizer is 1-D; a grid shares one scroll offset across `cross_axis_count`
tiles). Scroll extent is delegate-DETERMINISTIC (`compute_max_scroll_offset`), so
no virtualizer/estimate is needed.

Correct analog: **`RenderSliverList`** (request-strategy, element-owned), NOT
`RenderSliverListLazy` (render-owned, wired to no widget, produces render objects
directly). The lazy grid = `RenderSliverList`'s request/retain plumbing + the eager
`RenderSliverGrid`'s delegate window/geometry.

**Sharp edge (routed around, no ADR):** the lazy-build backend hardcodes child
parent-data as `SliverMultiBoxAdaptorParentData` (`sliver_protocol.rs:1085-1087`,
`sparse_children.rs:215`). So `RenderSliverGridLazy` uses
`type ParentData = SliverMultiBoxAdaptorParentData` (like `RenderSliverList`),
recomputing cross-axis offset from the delegate each pass — safe because the eager
grid already documents `cross_axis_offset` in parent data is non-load-bearing
(paint/hit read `RenderState.offset`).

## Step 1 — RenderSliverGridLazy (`crates/flui-objects/src/sliver/sliver_grid_lazy.rs`)
Model on `RenderSliverList` (request/retain) + eager `RenderSliverGrid` (window/geometry).
```rust
pub struct RenderSliverGridLazy {
    grid_delegate: Arc<dyn SliverGridDelegate>,
    item_count: usize,
    logical_to_slot: BTreeMap<usize, usize>,  // rebuilt each pass from pd.index
    attached_child_count: usize,               // &self hit-test reverse walk
}
// Arity = Variable; ParentData = SliverMultiBoxAdaptorParentData
```
`perform_layout` (oracle sliver_grid.dart:594-728, request-strategy):
1. `c = *ctx.constraints()`; `item_count==0 → SliverGeometry::ZERO`.
2. `layout = grid_delegate.get_layout(c)`.
3. window: `so=c.scroll_offset+c.cache_origin; end=so+c.remaining_cache_extent; first=layout.get_min_child_index_for_scroll_offset(so); last=layout.get_max_child_index_for_scroll_offset(end).min(item_count-1)`.
4. reconcile `logical_to_slot` from `ctx.child_parent_data(slot).index` for all slots.
5. tile = `c.as_box_constraints(child_main, child_main, Some(child_cross))` (tight).
6. window loop `for i in first..=last`: resident (slot in map) → `ctx.build_and_layout_box_child(slot, i, tile, &mut |_| None)`; absent → `ctx.request_child_build(i)`.
7. `scroll_extent = layout.compute_max_scroll_offset(item_count)` (deterministic).
8. geometry (eager parity): leading/trailing from delegate; `calculate_paint_offset`/`calculate_cache_offset`; `has_visual_overflow = scroll_extent>paint_extent || c.scroll_offset>0 || c.overlap!=0`.
9. position resident slots: `ctx.position_child(slot, grid_child_paint_offset(&c,&geom,px(so_i),px(child_main),px(co_i)))`; write `pd.index=i; pd.layout_offset=so_i`.
10. `attached_child_count = ctx.child_count()`.
11. **`ctx.emit_retain_band(first, last+1)`** (half-open) — evicts off-window tiles element-side.
`paint` → `ctx.paint_children()`; `hit_test` → reverse walk via `ctx.hit_test_child_at_layout_offset`.
Register in `sliver/mod.rs` + `flui-objects/src/lib.rs`.

## Step 2 — element wiring (`crates/flui-view/src/element/sliver_adaptor.rs`)
The `SliverListAdaptorManager` + `SparseChildren` are grid-agnostic → reused.
Preferred: generalize `SliverListAdaptorBehavior`/`Manager` into a
`LazySliverAdaptorBehavior<V: RenderView<Protocol=SliverProtocol>>` carrying the
builder; add a `SliverGridLazy` view (grid_delegate + item_count + builder) whose
`create_render_object` → `RenderSliverGridLazy::new(...)` and `on_mount` registers
the same manager. `has_children()=false`, empty `visit_child_views`, RenderVariable
element, distinct `view_type_id`. Fallback: parallel `SliverGridAdaptorBehavior`
module (duplicates the lifecycle — only if the generic bound fights the type system).

## Step 3 — GridView::builder (`crates/flui-widgets/src/scroll/grid_view.rs`)
`GridView::builder(grid_delegate, item_count, builder: Fn(usize)->Option<BoxedView>+Send+Sync+'static)`
(+ optional `count_builder`/`extent_builder` conveniences). Store a
`SliverChildBuilderDelegate` (reuse from sliver_list.rs). In `build`, branch: builder
arm → `SliverGridLazy::new(...)`; eager arm → `SliverGrid::new(...)`. No item-extent
arg (extent from delegate). Re-export `SliverGridLazy`.

## Step 4 — feature-gating: none (ships ungated like eager RenderSliverGrid). /api-review (pre-agreed trigger).

## Step 5 — tests
- Unit (sliver_grid_lazy.rs): construction, set_item_count/set_grid_delegate, window math vs a fixed SliverGridLayout (Direct-context-safe).
- Harness catalog: add "RenderSliverGridLazy" to RENDER_OBJECT_TYPES + a table row + harness_render_sliver_grid_lazy_* (per crates/flui-rendering/docs/TESTING.md).
- Lazy-window e2e (`crates/flui-widgets/tests/lazy_grid.rs`, clone lazy_list.rs): GridView::builder, settle via laid.tick(); assert (1) only visible+cache window built (<< item_count, ≥ visible tiles), (2) oracle 2-D positions ((0,0),(100,0),(0,100),(100,100)), (3) disposal-on-scroll (built set shifts, count bounded, ABA-safe), (4) quiescence (3rd tick builds 0), (5) None-at-K caps.

## Risk: LOW-MED (perform_layout reconcile+position bookkeeping) + LOW-MED (element generic extraction). No DEFERRED/HIGH, no ADR — the prior "HIGH complexity lazy grid" concern dissolves (no virtualizer needed; element adaptor already exists + is grid-agnostic).
