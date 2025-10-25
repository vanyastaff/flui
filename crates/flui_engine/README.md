# FLUI Engine

Backend-agnostic rendering engine for FLUI. This crate implements the typed Layer/Scene/Compositor architecture described in [idea.md](../../idea.md).

## Architecture

```
RenderObject.paint() → Layer
                        │
                        ▼
                  Scene Builder
                        │
                        ▼
                   Layer Tree
                        │
                        ▼
                   Compositor
                        │
                        ▼
                 Painter (backend)
                   │         │
                   ▼         ▼
                egui     wgpu/skia
```

## Features

### Layer System

Composable scene graph nodes:
- **ContainerLayer**: Holds multiple child layers
- **OpacityLayer**: Applies opacity to children
- **TransformLayer**: Applies transforms (translate, rotate, scale)
- **ClipLayer**: Clips children to a region
- **PictureLayer**: Leaf layer with actual drawing commands

### Scene Management

- Build and manage complete frames of rendering
- Track metadata (bounds, layer count, frame number)
- Support for viewport sizing and resizing

### Compositor

- Orchestrates layer traversal and painting
- Layer culling (skip off-screen layers)
- Performance tracking
- Optimization hooks

### Backend Abstraction

- **Surface**: Rendering target (window, buffer)
- **Frame**: Single frame of rendering
- **Painter**: Backend-agnostic drawing API
- **Backend**: Platform provider (egui, wgpu, skia)

## Usage

### Basic Example

```rust
use flui_engine::{
    Scene, Compositor, PictureLayer, Paint,
};
use flui_types::{Size, Rect};

// Create a scene
let mut scene = Scene::new(Size::new(800.0, 600.0));

// Add a layer
let mut picture = PictureLayer::new();
picture.draw_rect(
    Rect::from_xywh(100.0, 100.0, 200.0, 150.0),
    Paint {
        color: [1.0, 0.0, 0.0, 1.0], // Red
        ..Default::default()
    }
);
scene.add_layer(Box::new(picture));

// Composite to painter
let mut compositor = Compositor::new();
compositor.composite(&scene, &mut painter);
```

### Complex Scene

```rust
use flui_engine::{
    Scene, OpacityLayer, TransformLayer, PictureLayer,
};
use flui_types::Offset;

let mut scene = Scene::new(viewport_size);

// Create a picture layer
let mut picture = PictureLayer::new();
picture.draw_circle(center, radius, paint);

// Wrap in opacity
let with_opacity = OpacityLayer::new(Box::new(picture), 0.5);

// Wrap in transform
let transformed = TransformLayer::translate(
    Box::new(with_opacity),
    Offset::new(100.0, 50.0)
);

scene.add_layer(Box::new(transformed));
```

## Running Examples

```bash
# Full pipeline demo (egui)
cargo run --example full_pipeline --features egui

# Layer demo
cargo run --example layer_demo --features egui
```

## Feature Flags

- `egui` (default): Enable egui backend
- `wgpu`: Enable wgpu backend (future)
- `skia`: Enable skia backend (future)

## Integration with FLUI

This engine integrates with FLUI's typed RenderObject system:

1. **RenderObject** creates **Layers** during paint phase
2. **RenderPipeline** builds **Scene** from layers
3. **Compositor** renders **Scene** to **Surface**
4. **Painter** translates to platform (egui/wgpu/skia)

See [idea.md](../../idea.md) Chapter 6 for detailed architecture.

## Benefits

### Compile-Time Safety

- Type-safe layer composition
- No runtime casts or type checking
- Generic optimization opportunities

### Backend Agnostic

- Easy to add new backends
- Same scene works on all platforms
- Export to different formats (SVG, PNG, etc.)

### Performance

- Layer culling (skip off-screen content)
- Potential for caching and reuse
- Batching opportunities
- Offscreen rendering support

### Flexibility

- Effects can be composed (opacity + transform + clip)
- Custom layers easy to add
- Debug visualization support
- Frame capture and analysis

## Testing

```bash
cargo test
cargo test --features egui
```

## License

MIT OR Apache-2.0
