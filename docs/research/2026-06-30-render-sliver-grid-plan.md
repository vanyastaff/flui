# RenderSliverGrid — implementation plan (oracle-verified)

Build-ready plan for the missing 2D grid sliver render object (#1 Business.1 gap).
Prior attempt stalled on a **false** blocker; this plan pins down the truth.

## Headline: no infra/protocol gap, no ADR needed
- **`position_child(index, Offset)` already takes a full 2-D offset** (`context/layout.rs:101`), stored on the child slot (`sliver_protocol.rs:587`), committed to `RenderState.offset` (`subtree_arena.rs:1115/1612`), consumed 2-D at paint (`paint.rs:305→312/318`, `origin + child_offset`) and 2-D in hit-test (`accessors.rs:383-386`, `position - child_offset`). Cross-axis placement is fully expressible end-to-end. `RenderSliverFixedExtentList` drives this exact path, always passing cross=0 (`sliver_fixed_extent_list.rs:117-126`).
- **Do NOT use `walk_virtualizer_band`.** The `Virtualizer` is a 1-D linear cumulative-extent model (`virtualization/mod.rs:506`); a grid puts `cross_axis_count` items on one main-axis row sharing a scroll offset — the linear model can't represent it, and the band positioner hardcodes cross=0. This is almost certainly where the prior attempt stalled. Like the oracle, derive per-index tile geometry arithmetically from the delegate's `SliverGridLayout` (child size is *forced*, never measured) — no virtualizer needed for the eager MVP.

## Deliverable 1 — `SliverGridLayout::compute_max_scroll_offset` (`crates/flui-rendering/src/delegates/sliver_grid_delegate.rs`)
Currently MISSING (grep count 0). Oracle `.flutter/.../rendering/sliver_grid.dart:257-266`:
```rust
pub fn compute_max_scroll_offset(&self, child_count: usize) -> f32 {
    if child_count == 0 { return 0.0; }
    let main_axis_count = ((child_count - 1) / self.cross_axis_count) + 1;
    let main_axis_spacing = self.main_axis_stride - self.child_main_axis_extent;
    self.main_axis_stride * main_axis_count as f32 - main_axis_spacing
}
```
Unit tests (stride=100, childMain=100, cross=2): (0)=0, (1)=100, (6)=300, (7)=400, (8)=400.

## Deliverable 2 — `grid_child_paint_offset` (`crates/flui-rendering/src/constraints/sliver_layout.rs`)
Beside `child_paint_offset` (`:10-30`, which returns cross=0). Same main-axis math, but the non-main slot carries `cross_axis_offset`: Vertical → `Offset::new(px(cross), px(main))`, Horizontal → `Offset::new(px(main), px(cross))`. Export from `constraints/mod.rs`. Pure math; unit-test both axes.

## Deliverable 3 — `RenderSliverGrid` (`crates/flui-objects/src/sliver/sliver_grid.rs`)
```rust
pub struct RenderSliverGrid { grid_delegate: Arc<dyn SliverGridDelegate>, child_count: usize }
```
- `type Arity = Variable; type ParentData = SliverGridParentData;` (`index` + `cross_axis_offset`; `cross_axis_offset` written for parity/inspection but NOT load-bearing — paint/hit read `RenderState.offset`).
- `set_grid_delegate`: on change, `should_relayout(old)` → mark needs-layout (oracle `:577-585`). `Arc` so `RenderSliver: Send+Sync+'static` holds. `Diagnosticable`/`Debug` (`finish_non_exhaustive`).
- `perform_layout` (mirror oracle `:594-728`, eager/windowed):
  1. `let c=*ctx.constraints(); self.child_count=ctx.child_count();`
  2. `let layout=self.grid_delegate.get_layout(c);`
  3. `let scroll_offset=c.scroll_offset+c.cache_origin; let target_end=scroll_offset+c.remaining_cache_extent;`
  4. `first=layout.get_min_child_index_for_scroll_offset(scroll_offset); last=layout.get_max_child_index_for_scroll_offset(target_end).min(child_count-1);`
  5. `scroll_extent=layout.compute_max_scroll_offset(child_count);`
  6. For in-band `i in first..=last` (skip `i>=child_count`): `so=get_scroll_offset_of_child(i)`, `co=get_cross_axis_offset_of_child(i)`, `main=layout.child_main_axis_extent`, `cross=layout.child_cross_axis_extent`; `ctx.layout_box_child(i, c.as_box_constraints(main, main, Some(cross)))` (tight); track `leading=get_scroll_offset_of_child(first)`, `trailing=max(trailing, so+main)`.
  7. Geometry: `paint_extent=self.calculate_paint_offset(&c, scroll_offset.min(leading), trailing)`; `cache_extent=self.calculate_cache_offset(&c, leading, trailing)`; `layout_extent=paint_extent`; `max_paint_extent=scroll_extent`; `hit_test_extent=paint_extent`; `has_visual_overflow = scroll_extent>paint_extent || c.scroll_offset>0.0 || c.overlap!=0.0` (oracle `:716-719`).
  8. Position pass: for in-band `i`, `ctx.position_child(i, grid_child_paint_offset(&c, &geometry, px(so), px(main), px(co)))`; write parent data via `ctx.child_parent_data_mut(i)` (index, layout_offset=so, cross_axis_offset=co).
  9. Return geometry.
- `paint`: `ctx.paint_children();` `hit_test`: reverse-walk in-band with `ctx.hit_test_child_at_layout_offset(slot)` (mirror `sliver_fixed_extent_list.rs:132-142`).
- Register: `mod sliver_grid; pub use sliver_grid::*;` in `sliver/mod.rs`; re-export via `flui-objects/src/lib.rs`.

## Feature un-gate (partial)
`sliver_grid_delegate` is behind `experimental-delegates` (`lib.rs:69-72`, gate comment says un-gate "until the companion render-objects (RenderSliverGrid) land"). Un-gate ONLY the grid delegate: `SliverGridDelegate`, `SliverGridDelegateWithFixedCrossAxisCount`, `SliverGridDelegateWithMaxCrossAxisExtent`, `SliverGridLayout` → default build + prelude. Keep the other 5 companion-less delegate modules gated. `SliverGridParentData` already ungated. `RenderSliverGrid` ships ungated in flui-objects. Public-surface addition to flui-rendering → flag `/api-review` (pre-agreed trigger, not a new decision).

## Deliverable 4 — tests (oracle-derived golden)
Catalog guard: add `RenderSliverGrid` to `RENDER_OBJECT_TYPES` + a `harness_render_sliver_grid_*` test in `crates/flui-objects/tests/render_object_harness.rs` (per `crates/flui-rendering/docs/TESTING.md`).
Golden integration test (`crates/flui-rendering/tests/sliver_grid.rs`, clone `tests/sliver_fixed_extent_list.rs` scaffold): vertical grid, `SliverGridDelegateWithFixedCrossAxisCount::new(2)`, no spacing, aspect 1.0, `cross_axis_extent=200` → 100×100 tiles, **8 children**, `scroll_offset=100, remaining_paint_extent=200, remaining_cache_extent=200`.
Expected (oracle-derived): first=2, last=5 → children 2,3,4,5 laid out (0,1,6,7 out of band); sizes 100×100; positions child2(0,0), 3(100,0), 4(0,100), 5(100,100); SliverGeometry scroll_extent=400, paint_extent=200, layout_extent=200, max_paint_extent=400, cache_extent=200, hit_test_extent=200, has_visual_overflow=true.
Additional: horizontal axis (cross on dy), RTL (mirrored cross), cross-spacing, hit-test 2-D subtraction, should_relayout on delegate swap.

## Risk ranking
1. MED — `perform_layout` core loop + geometry (every sub-call proven by RenderSliverFixedExtentList).
2. LOW-MED — feature un-gate / prelude split (needs /api-review; pre-agreed).
3-6. LOW — the two math helpers, paint/hit (verbatim mirror), tests (values above).
DEFERRED (HIGH) — lazy virtualized grid: grid-aware windowing over `build_and_layout_box_child`/`dispose_box_child`, NOT `walk_virtualizer_band`. Separate follow-up.
