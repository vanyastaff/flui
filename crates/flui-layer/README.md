# flui-layer

FLUI's compositor scene: the retained scene a frame's paint changes by typed patches, the layer
types it is made of, its damage, and the package the raster side draws.

```
RenderObject::paint ──► SceneWriter ──► SceneVersion ──► SceneSnapshot ──► flui-engine
  (flui-rendering)      (this crate: patches, damage)     (raster boundary)   (wgpu)
```

## Overview

A `SceneOwner` keeps one persistent scene per presentation (ADR-0304). Each frame's paint
publishes the boundaries that repainted through its `SceneWriter`; the owner applies the patch
all-or-nothing, computes its damage from the footprint of what changed, and publishes an
immutable `SceneVersion` that shares every unchanged boundary with the version before it. A
`SceneSnapshot` carries that version, its frame stamp and its damage across to the raster
thread by value.

A boundary's content is a run of entries: drawing chunks (sealed `DisplayList`s), the effect
layers that group them (clip, transform, opacity, filters, shader masks, backdrops, leaders and
followers, annotated regions) and slots that place child boundaries.

`LayerTree` and `SceneBuilder` build a whole layer tree by hand, for a hot-reload plugin's image,
an example or the engine's GPU readback tests. `Scene::new` converts the tree into a version
(`SceneVersion::from_tree`), so the raster side walks one form.

## Layer types

| Kind | Variants |
|------|----------|
| Leaf | `PictureLayer` (sealed drawing commands, external textures among them), `PerformanceOverlayLayer` |
| Clip | `ClipRectLayer`, `ClipRRectLayer`, `ClipSuperellipseLayer`, `ClipPathLayer` |
| Transform | `OffsetLayer`, `TransformLayer` |
| Effect | `OpacityLayer`, `ColorFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `BackdropFilterLayer` |
| Link | `LeaderLayer`, `FollowerLayer` |
| Annotation | `AnnotatedRegionLayer` |

`Layer::local_translation()` is the one place the set of translating variants is written down;
the engine's pushes and the follower resolver's chain sums both read it.

## A hand-built tree

```rust
use flui_foundation::geometry::Offset;
use flui_layer::{PictureLayer, Scene, SceneBuilder};

let mut builder = SceneBuilder::new();
let offset = builder.push_offset(Offset::ZERO);
builder.push_opacity(0.5);
builder.add(PictureLayer::default());
builder.pop();
let tree = builder.build();
assert_eq!(tree.parent(offset), Some(tree.root()));

let scene = Scene::new(tree); // a version now: no `&mut` path back
```

A tree has one root for its whole life and is append-only: `push_child` mints the child's id in
the call that links it, so a cycle or a doubly-parented node cannot be expressed and no walker
needs a guard.

## Linked layers

A `LeaderLayer` and the `FollowerLayer`s that share its `LayerLink` place tooltips, menus and
selection handles against content painted elsewhere. At render time the follower's transform is
the leader's anchor carried across both ancestor chains, the leader's scale and rotation
included; `FollowerLayer::with_leader_clip(LeaderClip::Hide)` hides a follower while its
leader's anchor is clipped out (a selection handle whose end scrolled away). A version resolves
links as its flattened tree does (`a_version_resolves_links_and_annotations_as_its_flattened_tree_does`).

## Features

- `testing`: the `testing::inspect` walkers (`structure`, `clip_rects`, `first_picture_bounds`,
  …), the layer-tree differ that checks a patch's footprint (ADR-0304 §11), and
  `SceneVersion::flatten`. `flui-rendering`'s render harness re-exports them.

## Design

1. **Retained, shared, immutable versions:** an unchanged boundary is the same `Arc` in the next
   version, so a frame costs what changed, and the raster side reads a version without a lock.
2. **Damage from patches:** a frame's damage is the footprint of the nodes its patch changed,
   closed over what filters read, never which render objects repainted (ADR-0061, ADR-0304 §4).
3. **Canonical IDs:** `LayerId` from `flui-foundation`, 1-based; scene node ids are generational.
4. **Separation:** layer types and damage here, GPU lowering in `flui-engine`.

[`ARCHITECTURE.md`](ARCHITECTURE.md) explains the invariants and the notable choices.

