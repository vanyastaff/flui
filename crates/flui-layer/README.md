# flui-layer

Compositor layer tree for FLUI — the fourth tree in FLUI's 5-tree architecture.

```
View → Element → Render → Layer → Semantics
```

## Overview

Layers handle compositing and GPU optimization. They're created at repaint boundaries and cached for efficient rendering.

```
RenderObject (flui_rendering)
    │
    │ paint() generates Canvas OR pushes Layer
    ▼
Layer (this crate)
    │
    │ render() → CommandRenderer
    ▼
GPU Rendering (wgpu via flui_engine)
```

## Layer Types

### Leaf Layers
| Layer | Description |
|-------|-------------|
| `CanvasLayer` | Standard canvas drawing commands (mutable) |
| `PictureLayer` | Recorded drawing commands (immutable, for repaint boundaries) |
| `TextureLayer` | External GPU texture rendering (video, camera) |
| `PlatformViewLayer` | Native platform view embedding |
| `PerformanceOverlayLayer` | Performance statistics display |

### Clip Layers
| Layer | Description |
|-------|-------------|
| `ClipRectLayer` | Rectangular clipping |
| `ClipRRectLayer` | Rounded rectangle clipping |
| `ClipSuperellipseLayer` | iOS-style squircle clipping |
| `ClipPathLayer` | Arbitrary path clipping |

### Transform Layers
| Layer | Description |
|-------|-------------|
| `OffsetLayer` | Simple translation (optimized for repaint boundaries) |
| `TransformLayer` | Full 4x4 matrix transformation |

### Effect Layers
| Layer | Description |
|-------|-------------|
| `OpacityLayer` | Alpha blending |
| `ColorFilterLayer` | Color matrix transformation (grayscale, sepia, etc.) |
| `ImageFilterLayer` | Blur, dilate, erode effects |
| `ShaderMaskLayer` | GPU shader masking (gradient fades, vignettes) |
| `BackdropFilterLayer` | Backdrop filtering (frosted glass, blur) |

### Linking Layers
| Layer | Description |
|-------|-------------|
| `LeaderLayer` | Anchor point for linked positioning |
| `FollowerLayer` | Positions content relative to a leader |

### Annotation Layers
| Layer | Description |
|-------|-------------|
| `AnnotatedRegionLayer` | Metadata regions for system UI integration |

## Usage

```rust
use flui_layer::prelude::*;
use flui_types::geometry::Rect;
use flui_types::painting::Clip;

// Create a layer tree
let mut tree = LayerTree::new();

// Add a canvas layer
let canvas_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

// Add a clip layer as a child
let clip = ClipRectLayer::anti_alias(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
let clip_id = tree.insert_child(canvas_id, Layer::ClipRect(clip));

// Add transform with opacity
let offset_id = tree.push_offset(10.0, 20.0);
let opacity_id = tree.push_opacity(0.8);
```

### Scene Building

```rust
use flui_layer::{SceneBuilder, Scene};

let mut builder = SceneBuilder::new();

builder.push_offset(10.0, 20.0);
builder.push_opacity(0.9);
builder.add_canvas(canvas);
builder.pop(); // opacity
builder.pop(); // offset

let scene: Scene = builder.build();
```

### Layer Handles

Type-safe handles for retained layer references:

```rust
use flui_layer::{LayerHandle, OpacityLayer};

let handle: LayerHandle<OpacityLayer> = LayerHandle::new();
handle.set(OpacityLayer::new(0.5));

if let Some(layer) = handle.get() {
    println!("Opacity: {}", layer.alpha());
}
```

### Linked Layers

For tooltips, dropdowns, and overlays that follow other content:

```rust
use flui_layer::{LayerLink, LeaderLayer, FollowerLayer};

// Create a link
let link = LayerLink::new();

// Leader defines the anchor point
let leader = LeaderLayer::new(link.clone())
    .with_offset(100.0, 50.0);

// Follower positions relative to leader
let follower = FollowerLayer::new(link)
    .right_of(10.0)  // 10px to the right of leader
    .with_size(200.0, 100.0);
```

### Annotation Search

Find annotations at a specific point in the layer tree:

```rust
use flui_layer::{AnnotationResult, AnnotationSearchOptions};

let mut result: AnnotationResult<String> = AnnotationResult::new();
// ... populate during hit testing ...

for entry in result.entries() {
    println!("Found: {} at {:?}", entry.annotation, entry.local_position);
}
```

## Features

- `parallel` — Enable parallel layer operations via rayon

```toml
[dependencies]
flui-layer = { version = "0.1", features = ["parallel"] }
```

## Design Principles

1. **Canonical IDs** — Uses `LayerId` from `flui-foundation`
2. **Tree Traits** — Implements `TreeRead<LayerId>`, `TreeNav<LayerId>` from `flui-tree`
3. **Separation** — Layer types here, rendering in `flui_engine`
4. **Thread-safe** — All types are `Send + Sync`
5. **Flutter-compatible** — API mirrors Flutter's layer.dart where applicable

## Flutter Parity

This crate provides equivalent functionality to Flutter's `layer.dart`:

| Flutter | flui-layer |
|---------|------------|
| `Layer` | `Layer` enum |
| `ContainerLayer` | `LayerTree` with parent-child |
| `OffsetLayer` | `OffsetLayer` |
| `ClipRectLayer` | `ClipRectLayer` |
| `ClipRRectLayer` | `ClipRRectLayer` |
| `ClipRSuperellipseLayer` | `ClipSuperellipseLayer` |
| `ClipPathLayer` | `ClipPathLayer` |
| `OpacityLayer` | `OpacityLayer` |
| `ColorFilterLayer` | `ColorFilterLayer` |
| `ImageFilterLayer` | `ImageFilterLayer` |
| `BackdropFilterLayer` | `BackdropFilterLayer` |
| `TransformLayer` | `TransformLayer` |
| `LeaderLayer` | `LeaderLayer` |
| `FollowerLayer` | `FollowerLayer` |
| `AnnotatedRegionLayer` | `AnnotatedRegionLayer` |
| `TextureLayer` | `TextureLayer` |
| `PlatformViewLayer` | `PlatformViewLayer` |
| `PerformanceOverlayLayer` | `PerformanceOverlayLayer` |
| `ShaderMaskLayer` | `ShaderMaskLayer` |

## License

MIT OR Apache-2.0
