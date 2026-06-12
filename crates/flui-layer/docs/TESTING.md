# Layer-tree test harness (`flui_layer::testing`)

Declarative construction and structural inspection of [`LayerTree`](../src/layer_tree.rs)
for unit tests and for [`flui-rendering`](../../flui-rendering/docs/TESTING.md)'s
render harness (which re-exports the walkers as `inspect::layer_structure`, etc.).

## Enabling

| Consumer | How to enable |
|----------|----------------|
| This crate's own tests | `cfg(test)` |
| Downstream crates | `flui-layer = { features = ["testing"] }` |
| `flui-rendering` with `testing` | Forwarded automatically |

```bash
cargo test -p flui-layer
```

## Design

```text
LayerSpec  →  LayerTester::mount  →  structure / bounds / diagnostics
     ↑
  layer(OffsetLayer::…).child(…).label("canvas")
```

The free functions in [`inspect`](../src/testing/inspect.rs) are the **single
source of truth** for layer-tree walks. `flui-rendering` does not duplicate them.

## Core API

### Tree builder ([`spec`](../src/testing/spec.rs))

| Item | Role |
|------|------|
| `layer(value)` | Start a spec from any `Into<Layer>` (`OffsetLayer`, `PictureLayer`, …) |
| `LayerSpec::label("name")` | Register label for `tester.id("name")` |
| `LayerSpec::child` / `children` | Nesting |
| `mount(tree, spec)` | Low-level insert + wire children (used by `LayerTester`) |
| `LayerLabelRegistry` | Label → `LayerId` map |

### `LayerTester` ([`tester`](../src/testing/tester.rs))

| Method | Purpose |
|--------|---------|
| `mount(spec)` | Build a fresh `LayerTree` from a spec |
| `root()` | Root `LayerId` |
| `tree()` / `tree_mut()` | Underlying `LayerTree` |
| `id("label")` / `try_id` | Resolve labeled layer |
| `kind(id)` | Short kind name (`"Picture"`, `"Opacity"`, …) |
| `structure()` | Pre-order kind names |
| `structure_with_depth()` | Kinds paired with depth from root |
| `first_picture_bounds()` | First `Picture` layer bounds |
| `diagnostics()` | `Diagnosticable`-backed hierarchy |
| `dump()` | Printable diagnostics string |

### Free inspect functions ([`inspect`](../src/testing/inspect.rs))

Use these directly when you already hold a `LayerTree` (e.g. from
`FrameRun::layer_tree()` in the render harness).

| Function | Returns |
|----------|---------|
| `layer_kind(layer)` | `&'static str` kind name |
| `structure(tree)` | `Vec<&'static str>` pre-order kinds |
| `structure_with_depth(tree)` | `Vec<(usize, &'static str)>` |
| `first_picture_bounds(tree)` | `Option<Rect>` |
| `first_opacity_alpha(tree)` | `Option<f32>` — first `Opacity` layer |
| `has_picture_layer(tree)` | `bool` |
| `diagnostics_tree(tree)` | `Option<DiagnosticsNode>` |

## Examples

### Build and inspect structure

```rust
use flui_layer::testing::{LayerTester, layer};
use flui_layer::{CanvasLayer, OffsetLayer};
use flui_types::geometry::px;

let probe = LayerTester::mount(
    layer(OffsetLayer::new(flui_types::Offset::new(px(5.0), px(5.0))))
        .child(layer(CanvasLayer::new()).label("canvas")),
);

assert_eq!(probe.structure(), vec!["Offset", "Canvas"]);
assert_eq!(probe.kind(probe.id("canvas")), "Canvas");
```

### Walk a tree from the render harness

```rust
use flui_layer::testing::inspect;

let tree = run.layer_tree().expect("frame painted");
assert_eq!(inspect::structure(tree), vec!["Offset", "Picture"]);
assert!(inspect::has_picture_layer(tree));
```

### Opacity layer checks (animation tests)

```rust
use flui_layer::testing::inspect;

let alpha = inspect::first_opacity_alpha(tree);
// None when fully opaque (Flutter parity: no OpacityLayer at alpha 255)
// Some(0.5) when semi-transparent
```

### Diagnostics dump

```rust
let dump = probe.dump();
assert!(dump.contains("Offset"));
```

## See also

- Render harness (uses these walkers): [`flui-rendering/docs/TESTING.md`](../../flui-rendering/docs/TESTING.md)
- Workspace overview: [`docs/testing.md`](../../../docs/testing.md)
