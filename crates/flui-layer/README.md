# flui-layer

Compositor layer tree for FLUI — the fourth tree in FLUI's 5-tree architecture.

```
View → Element → Render → Layer → Semantics
```

## Overview

The paint walk in `flui-rendering` emits one `LayerTree` per frame; it is frozen into a `Scene`,
stamped into a `SceneSnapshot`, moved by value to the raster side, and lowered to GPU work by
`flui-engine`.

```
RenderObject::paint  ──►  LayerTree  ──►  Scene  ──►  SceneSnapshot  ──►  flui-engine
  (flui-rendering)       (this crate)                 (raster boundary)     (wgpu)
```

## Layer types

| Kind | Variants |
|------|----------|
| Leaf | `PictureLayer` (sealed drawing commands), `CanvasLayer` (a live recorder), `TextureLayer`, `PlatformViewLayer`, `PerformanceOverlayLayer` |
| Clip | `ClipRectLayer`, `ClipRRectLayer`, `ClipSuperellipseLayer`, `ClipPathLayer` |
| Transform | `OffsetLayer`, `TransformLayer` |
| Effect | `OpacityLayer`, `ColorFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `BackdropFilterLayer` |
| Link | `LeaderLayer`, `FollowerLayer` |
| Annotation | `AnnotatedRegionLayer` |

`Layer::local_translation()` is the one place the set of translating variants is written down;
the engine's pushes and the follower resolver's chain sums both read it.

## Usage

### Building a tree

```rust
use flui_layer::{ClipRectLayer, Layer, LayerTree, OffsetLayer, PictureLayer, Scene};
use flui_foundation::geometry::Rect;
use flui_painting::paint::Clip;

let mut tree = LayerTree::new();
let root = tree.insert_root(Layer::from(OffsetLayer::zero()));
let clip = tree.push_child(
    root,
    Layer::from(ClipRectLayer::new(
        Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        Clip::AntiAlias,
    )),
);
tree.push_child(clip, Layer::from(PictureLayer::default()));

let scene = Scene::new(tree); // frozen: no `&mut` path back to the tree
assert_eq!(scene.root(), Some(root));
```

The tree is append-only: `push_child` mints the child's id in the call that links it, so a cycle
or a doubly-parented node cannot be expressed and no walker needs a guard.

### Scene building

```rust
use flui_layer::{LayerTree, OpacityLayer, PictureLayer, SceneBuilder};
use flui_painting::DisplayList;
use flui_foundation::geometry::Offset;

let mut tree = LayerTree::new();
let mut builder = SceneBuilder::new(&mut tree);
builder.push_offset(Offset::ZERO);
builder.push(OpacityLayer::new(0.9));
builder.add_picture(DisplayList::new());
builder.pop(); // opacity
builder.pop(); // offset
let root = builder.build();
assert!(root.is_some());
```

### Linked layers

For tooltips, dropdowns, and overlays that follow other content:

```rust
use flui_layer::{FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer, resolve_follower_offset};
use flui_foundation::geometry::{Offset, Size};
use flui_painting::Alignment;

let link = LayerLink::new();
let mut tree = LayerTree::new();
let root = tree.insert_root(Layer::from(flui_layer::OffsetLayer::zero()));
tree.push_child(
    root,
    Layer::from(LeaderLayer::with_offset(
        link,
        Size::new(100.0, 30.0),
        Offset::new(40.0, 10.0),
    )),
);
// Hang 5 px below the leader's bottom-center.
let follower = FollowerLayer::new(link)
    .with_leader_anchor(Alignment::BOTTOM_CENTER)
    .with_follower_anchor(Alignment::TOP_CENTER)
    .with_target_offset(Offset::new(0.0, 5.0))
    .with_size(Size::new(60.0, 20.0));
let follower_id = tree.push_child(root, Layer::from(follower));

let resolved = resolve_follower_offset(&tree, follower_id);
assert_eq!(resolved, Some(Offset::new(60.0, 45.0)));
```

The tree indexes every leader by link as it is pushed; at render time
`resolve_follower_offset` walks both ancestor chains to their common ancestor and returns the
offset the renderer applies at the follower's tree position.

## Features

- `testing` — the `testing::inspect` walkers (`structure`, `clip_rects`, `first_picture_bounds`,
  `diagnostics_tree`, …). `flui-rendering`'s render harness re-exports them.

```toml
[dev-dependencies]
flui-layer = { git = "https://github.com/vanyastaff/flui", features = ["testing"] }
```

This crate isn't published to crates.io; depend on it via git or path — see
the [flui facade's README](../../README.md).

## Design

1. **Canonical IDs** — `LayerId` from `flui-foundation`, 1-based over a 0-based `Vec`
2. **Inherent walks** — `LayerTree` answers `ancestors`, `lowest_common_ancestor` and (for the
   `testing` walkers) `descendants` itself; no generic tree trait sits in between
3. **Separation** — layer types here, GPU lowering in `flui-engine`
4. **Single owner, value-moved** — built on the paint side, frozen into a `Scene`, rendered on the
   raster side; no lock, no `Arc`
5. **Behavior is pinned by tests**; [`ARCHITECTURE.md`](ARCHITECTURE.md) explains the notable
   choices
