# Render-object test harness (`flui_rendering::testing`)

Headless integration testing for `RenderBox` and `RenderSliver` types through the
**real** [`PipelineOwner`](../src/pipeline/owner.rs) pipeline — no mocks, no GPU,
no window.

## Enabling

The module is **off by default** so it never lands in release builds.

| Consumer | How to enable |
|----------|----------------|
| This crate's own tests / benches / examples | Automatic via the self `dev-dependency` with `features = ["testing"]` |
| Downstream crates | `flui-rendering = { features = ["testing"] }` |
| `render_inspector` example | `cargo run -p flui-rendering --example render_inspector --features testing` |

```bash
cargo test -p flui-rendering
cargo test -p flui-rendering --test render_object_harness
cargo test -p flui-rendering --test harness_animation
```

## Design

```text
TreeNode spec  →  RenderTester::mount  →  run_layout | run_frame
                                              ↓
                                         LayoutRun | FrameRun
                                              ↓
                                         Probe (offset, hit, diagnostics, …)
```

1. **Describe** a tree with [`box_node`](../src/testing/tree.rs) /
   [`sliver_node`](../src/testing/tree.rs), optional [`ParentDataSeed`](../src/testing/parent_data.rs),
   and [`TreeNode::label`](../src/testing/tree.rs).
2. **Drive** the production pipeline at the depth you need.
3. **Inspect** through the shared [`Probe`](../src/testing/inspect.rs) trait and,
   for proxy query contracts, [`BoxQueryRun`](../src/testing/queries.rs).
4. **Animate** with multi-frame helpers on [`FrameRun`](../src/testing/harness.rs).

Diagnostics dumps combine each render object's **user config** (via
[`Diagnosticable`](../src/traits/mod.rs)) with **committed runtime** fields
(`offset`, `size`, sliver `geometry`) layered on by the pipeline. Property names
use **snake_case**.

## Core API

### Tree builders

| Item | Role |
|------|------|
| `box_node(obj)` | Box-protocol render object node |
| `sliver_node(obj)` | Sliver-protocol render object node |
| `TreeNode::child` / `children` | Nesting |
| `TreeNode::label("name")` | Register label for `run.id("name")` |
| `TreeNode::with_parent_data_seed` | Stack / flex / sliver parent metadata |
| `TreeNode::with_stack_parent_data` | Shorthand for stack positioning |
| `TreeNode::with_flex_parent_data` | Shorthand for flex factors |

### `RenderTester`

| Method | Returns | Purpose |
|--------|---------|---------|
| `mount(spec)` | `Self` | Configure from a `TreeNode` |
| `with_constraints(c)` | `Self` | Root box constraints |
| `with_size(size)` | `Self` | Tight root size |
| `run_layout()` | `LayoutRun` | Layout phase only (geometry / offsets) |
| `run_frame()` | `FrameRun` | Full frame (layout → compositing → paint) |

### `LayoutRun`

| Method | Purpose |
|--------|---------|
| `root()` | Root `RenderId` |
| `owner()` / `owner_mut()` | Escape hatch to `PipelineOwner<Layout>` |
| `update::<T>(id, edit)` | Mutate + `mark_needs_layout` (Box or Sliver) |
| `update_paint::<T>(id, edit)` | Mutate + paint-dirty (no layout pass here) |
| `mark_needs_paint(id)` | Paint-dirty only |
| `relayout()` | Re-run layout after `update` |
| [`BoxQueryRun`](../src/testing/queries.rs) | Intrinsics / dry layout / dry baseline (see below) |

Implements [`Probe`](../src/testing/inspect.rs) and [`BoxQueryRun`](../src/testing/queries.rs).

### `FrameRun`

| Method | Purpose |
|--------|---------|
| `painted()` | Whether the last frame produced a layer tree |
| `is_clean()` | No dirty nodes remain |
| `layer_tree()` | Last `LayerTree`, if any |
| `structure()` | Layer kind names (pre-order) |
| `picture_bounds()` | First picture layer bounds |
| `report()` / `pump()` | `FrameReport` snapshot; run another frame |
| `pump_frames(n)` | `n` consecutive frames, collect reports |
| `pump_idle_frames(n)` | Skip `n` settled frames (panics if anything paints) |
| `update::<T>(id, edit)` | Layout mutation (Box or Sliver) |
| `update_paint::<T>(id, edit)` | Paint mutation (+ compositing bits refresh) |
| `advance_layout::<T>(id, edit)` | `update` + `pump` |
| `advance_paint::<T>(id, edit)` | `update_paint` + `pump` |
| `simulate(ticks, \|t, run\| …)` | Per-tick callback then auto-`pump` |
| `opacity_alpha()` | First opacity layer alpha, if any |
| `has_picture_layer()` | Whether a picture layer exists |

Implements [`Probe`](../src/testing/inspect.rs) and [`BoxQueryRun`](../src/testing/queries.rs).

### `Probe` (shared inspection)

| Method | Purpose |
|--------|---------|
| `id("label")` / `try_id` | Resolve labeled node |
| `offset(id)` | Committed paint offset |
| `box_geometry(id)` | Committed box size |
| `sliver_geometry(id)` | Committed sliver geometry |
| `hit(x, y)` / `hit_first` | Hit-test path (leaf-first) |
| `diagnostics()` | Full render-tree diagnostics tree |
| `property(id, "name")` | Structured property lookup |
| `property_f64(id, "name")` | Parsed numeric property |
| `descendant_property("RenderFlex", "direction")` | Find by type name |
| `dump()` | Printable tree (for failure messages) |

### `BoxQueryRun` (intrinsics / dry probes)

Implemented on both [`LayoutRun`](../src/testing/harness.rs) and
[`FrameRun`](../src/testing/harness.rs). Queries use the production
[`PipelineOwner`](../src/pipeline/owner.rs) memoization path — they do **not**
require a prior layout pass, but you can call them after `run_layout` or
`run_frame` to assert proxy forwarding contracts.

| Method | Purpose |
|--------|---------|
| `intrinsic_dimension(id, dimension, extent)` | Raw intrinsic dispatch |
| `min_intrinsic_width(id, height)` | `computeMinIntrinsicWidth` |
| `max_intrinsic_width(id, height)` | `computeMaxIntrinsicWidth` |
| `min_intrinsic_height(id, width)` | `computeMinIntrinsicHeight` |
| `max_intrinsic_height(id, width)` | `computeMaxIntrinsicHeight` |
| `dry_layout(id, constraints)` | Flutter `getDryLayout` |
| `dry_baseline(id, constraints, baseline)` | Flutter `getDryBaseline` |

```rust
use flui_rendering::objects::{RenderColoredBox, RenderOpacity};
use flui_rendering::testing::{BoxQueryRun, RenderTester, box_node};
use flui_types::{Size, geometry::px};

let constraints = flui_rendering::constraints::BoxConstraints::new(
    px(0.0), px(200.0), px(0.0), px(200.0),
);
let mut run = RenderTester::mount(
    box_node(RenderOpacity::opaque())
        .child(box_node(RenderColoredBox::red(40.0, 40.0))),
)
.with_constraints(constraints)
.run_layout();

assert_eq!(run.min_intrinsic_width(run.root(), 100.0), 40.0);
assert_eq!(
    run.dry_layout(run.root(), constraints),
    Size::new(px(40.0), px(40.0)),
);
```

### Assertion helpers ([`assertions`](../src/testing/assertions.rs))

| Function | Purpose |
|----------|---------|
| `assert_properties(node, &[&str])` | Required config property names |
| `assert_descendant_properties(tree, type_name, required)` | Descendant config contract |
| `assert_has_committed_size(node)` | Runtime `size` present |
| `assert_has_committed_geometry(node)` | Runtime sliver `geometry` present |

### `FrameReport`

Snapshot returned by `pump` / `advance_*` / `simulate`: `painted`, `structure`
(with depth), `picture_bounds`, `dirty`. Implements `Display` for examples and
[`render_inspector`](../examples/render_inspector.rs).

## Examples

### Single frame — layout + paint

```rust
use flui_rendering::objects::{RenderColoredBox, RenderPadding};
use flui_rendering::testing::{RenderTester, Probe, box_node};
use flui_types::{Offset, Size, geometry::px};

let run = RenderTester::mount(
    box_node(RenderPadding::all(5.0))
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
)
.with_size(Size::new(px(200.0), px(200.0)))
.run_frame();

let child = run.id("child");
assert_eq!(run.offset(child), Offset::new(px(5.0), px(5.0)));
assert_eq!(run.structure(), vec!["Offset", "Picture"]);
assert!(run.painted());
```

### Layout only

```rust
let run = RenderTester::mount(/* … */).run_layout();
assert_eq!(run.box_geometry(run.root()), Size::new(px(200.0), px(200.0)));
```

### Stack positioned child (`ParentDataSeed`)

```rust
use flui_rendering::parent_data::StackParentData;
use flui_rendering::objects::{RenderColoredBox, RenderStack};
use flui_rendering::testing::{RenderTester, Probe, box_node};

let run = RenderTester::mount(
    box_node(RenderStack::new())
        .child(box_node(RenderColoredBox::red(80.0, 80.0)).label("base"))
        .child(
            box_node(RenderColoredBox::green(20.0, 20.0))
                .with_stack_parent_data(StackParentData::new().with_top(12.0).with_left(18.0))
                .label("positioned"),
        ),
)
.with_size(Size::new(px(120.0), px(120.0)))
.run_layout();

assert_eq!(run.hit_first(25.0, 20.0), Some(run.id("positioned")));
```

### Multi-frame layout animation

```rust
let mut run = RenderTester::mount(/* padding + child */).run_frame();
let pad = run.root();
let child = run.id("child");

run.simulate([0.0, 0.5, 1.0], |t, run| {
    let padding = 5.0 + 50.0 * t as f32;
    run.update::<RenderPadding>(pad, |p| {
        p.set_padding(EdgeInsets::all(px(padding)));
    });
});
assert_eq!(run.offset(child), Offset::new(px(55.0), px(55.0)));
```

### Paint-only (color / opacity)

```rust
// Color — assert via diagnostics config
run.advance_paint::<RenderColoredBox>(leaf, |b| {
    b.set_color([0.0, 1.0, 0.0, 1.0]);
});
assert_eq!(
    run.property(leaf, "color").as_deref(),
    Some("[0.0, 1.0, 0.0, 1.0]"),
);

// Opacity — assert via layer tree
run.advance_paint::<RenderOpacity>(fade, |o| o.set_opacity(0.5));
assert!(run.structure().contains(&"Opacity"));
assert!((run.opacity_alpha().unwrap() - 0.5).abs() < 0.01);
```

### `AnimationController` integration

See [`tests/harness_animation.rs`](../tests/harness_animation.rs): call
`ctrl.tick_at(t)` then `run.advance_layout` and assert `offset` /
`picture_bounds` each frame. Finish with `run.pump_idle_frames(2)` to prove
the pipeline settles.

### CI catalog

[`tests/render_object_harness.rs`](../tests/render_object_harness.rs) mounts every
exported `RenderBox` / `RenderSliver` type. `catalog_covers_every_render_object_name`
fails CI if a new render object lacks harness coverage.

## Cross-crate dependencies

| Crate | Re-use |
|-------|--------|
| [`flui_layer::testing`](../../flui-layer/docs/TESTING.md) | Layer-tree walkers (`structure`, `first_picture_bounds`, `first_opacity_alpha`) |
| [`flui_foundation`](../../flui-foundation/docs/TESTING.md) | `DiagnosticsNode` query API for structured assertions |
| [`flui_painting::testing`](../../flui-painting/docs/TESTING.md) | Display-list recording when testing paint in isolation |

## See also

- Workspace overview: [`docs/testing.md`](../../../docs/testing.md)
- Example binary: [`examples/render_inspector.rs`](../examples/render_inspector.rs)
- Production animation tests: [`tests/animation_pipeline.rs`](../tests/animation_pipeline.rs)
