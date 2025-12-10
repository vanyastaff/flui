# PaintingContext Architecture

`PaintingContext` is the abstraction through which render objects paint themselves and their children.

## Class Structure

```
┌───────────────────────────────────────────────────────────────────────┐
│                         PaintingContext                                │
│                    extends ClipContext                                 │
├───────────────────────────────────────────────────────────────────────┤
│ LAYER MANAGEMENT                                                       │
│   - _containerLayer: ContainerLayer (final)                           │
│   - _currentLayer: PictureLayer?                                      │
│   - _recorder: PictureRecorder?                                       │
│   - _canvas: Canvas?                                                  │
├───────────────────────────────────────────────────────────────────────┤
│ BOUNDS                                                                 │
│   - estimatedBounds: Rect (final)                                     │
├───────────────────────────────────────────────────────────────────────┤
│ PUBLIC GETTERS                                                         │
│   - canvas: Canvas (auto-starts recording)                            │
│   - recorder: PictureRecorder                                         │
└───────────────────────────────────────────────────────────────────────┘
```

## Recording Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Canvas Recording Flow                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   ┌───────────────┐                                                 │
│   │ Access canvas │ (get canvas property)                           │
│   └───────┬───────┘                                                 │
│           │                                                         │
│           ▼                                                         │
│   ┌───────────────┐                                                 │
│   │ _canvas null? │                                                 │
│   └───────┬───────┘                                                 │
│           │                                                         │
│     yes   │   no                                                    │
│     ┌─────┴─────┐                                                   │
│     ▼           ▼                                                   │
│ ┌─────────────────┐  ┌─────────────────┐                           │
│ │ _startRecording │  │  Return _canvas │                           │
│ │                 │  └─────────────────┘                           │
│ │ 1. Create       │                                                 │
│ │    PictureLayer │                                                 │
│ │ 2. Create       │                                                 │
│ │    PictureRecorder│                                               │
│ │ 3. Create Canvas│                                                 │
│ │ 4. Append layer │                                                 │
│ │    to container │                                                 │
│ └─────────────────┘                                                 │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Layer Stack Operations

### paintChild()

```dart
void paintChild(RenderObject child, Offset offset) {
  if (child.isRepaintBoundary) {
    stopRecordingIfNeeded();   // Finish current Picture
    _compositeChild(child, offset);
  } else if (child._wasRepaintBoundary) {
    // Child lost repaint boundary status
    child._layerHandle.layer = null;  // Dispose old layer
    child._paintWithContext(this, offset);
  } else {
    child._paintWithContext(this, offset);
  }
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│                      paintChild() Flow                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Parent painting                                                    │
│        │                                                            │
│        ▼                                                            │
│  ┌───────────────────┐                                              │
│  │   paintChild()    │                                              │
│  └─────────┬─────────┘                                              │
│            │                                                        │
│            ├──────────────────────┐                                 │
│            │  isRepaintBoundary?  │                                 │
│            │                      │                                 │
│     YES    ▼               NO     ▼                                 │
│  ┌─────────────────┐    ┌─────────────────┐                        │
│  │stopRecording    │    │ Paint into same │                        │
│  │_compositeChild  │    │ context/canvas  │                        │
│  │                 │    │                 │                        │
│  │ Child gets own  │    │ Child draws     │                        │
│  │ layer subtree   │    │ directly        │                        │
│  └─────────────────┘    └─────────────────┘                        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### pushLayer()

```dart
void pushLayer(
  ContainerLayer childLayer,
  PaintingContextCallback painter,
  Offset offset, {
  Rect? childPaintBounds,
}) {
  // Remove old children from reused layer
  if (childLayer.hasChildren) {
    childLayer.removeAllChildren();
  }
  
  stopRecordingIfNeeded();
  appendLayer(childLayer);
  
  // Create child context for painting within the new layer
  final childContext = createChildContext(
    childLayer,
    childPaintBounds ?? estimatedBounds,
  );
  
  painter(childContext, offset);
  childContext.stopRecordingIfNeeded();
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│                       Layer Stack Example                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Initial:                   After pushClipRect:                     │
│                                                                     │
│  ContainerLayer             ContainerLayer                          │
│  └── PictureLayer           ├── PictureLayer (stopped)              │
│      └── [drawings]         │   └── [drawings before clip]          │
│                             └── ClipRectLayer                       │
│                                 └── PictureLayer (new)              │
│                                     └── [drawings inside clip]      │
│                                                                     │
│  After painting inside clip ends:                                   │
│                                                                     │
│  ContainerLayer                                                     │
│  ├── PictureLayer                                                   │
│  │   └── [drawings before clip]                                     │
│  ├── ClipRectLayer                                                  │
│  │   └── PictureLayer                                               │
│  │       └── [clipped drawings]                                     │
│  └── PictureLayer (new, if more drawing after)                      │
│      └── [drawings after clip]                                      │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Clip Methods

| Method | Layer Type | Purpose |
|--------|------------|---------|
| `pushClipRect` | `ClipRectLayer` | Rectangle clip |
| `pushClipRRect` | `ClipRRectLayer` | Rounded rectangle clip |
| `pushClipRSuperellipse` | `ClipRSuperellipseLayer` | Superellipse (squircle) clip |
| `pushClipPath` | `ClipPathLayer` | Arbitrary path clip |

### Compositing vs Direct Clip

```dart
ClipRectLayer? pushClipRect(
  bool needsCompositing,  // Key parameter!
  Offset offset,
  Rect clipRect,
  PaintingContextCallback painter, {
  Clip clipBehavior = Clip.hardEdge,
  ClipRectLayer? oldLayer,
}) {
  if (clipBehavior == Clip.none) {
    painter(this, offset);
    return null;
  }
  
  final offsetClipRect = clipRect.shift(offset);
  
  if (needsCompositing) {
    // Create/reuse a layer
    final layer = oldLayer ?? ClipRectLayer();
    layer
      ..clipRect = offsetClipRect
      ..clipBehavior = clipBehavior;
    pushLayer(layer, painter, offset, childPaintBounds: offsetClipRect);
    return layer;
  } else {
    // Direct canvas clip (more efficient)
    clipRectAndPaint(offsetClipRect, clipBehavior, offsetClipRect, 
      () => painter(this, offset));
    return null;
  }
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│               Compositing vs Direct Clipping                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  needsCompositing = FALSE:              needsCompositing = TRUE:    │
│  (Direct canvas operation)              (Layer-based)               │
│                                                                     │
│  PictureLayer                           ContainerLayer              │
│  └── Canvas                             ├── PictureLayer            │
│      ├── save()                         └── ClipRectLayer           │
│      ├── clipRect()                         └── PictureLayer        │
│      ├── [child drawings]                       └── [child drawings]│
│      └── restore()                                                  │
│                                                                     │
│  PROS: Single Picture                   PROS: Child can be          │
│        Better performance               repainted independently     │
│  CONS: Child repaint =                  CONS: More layers           │
│        Parent repaint                   (memory overhead)           │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Transform and Opacity

### pushTransform()

```dart
TransformLayer? pushTransform(
  bool needsCompositing,
  Offset offset,
  Matrix4 transform,
  PaintingContextCallback painter, {
  TransformLayer? oldLayer,
}) {
  final effectiveTransform = Matrix4.translationValues(offset.dx, offset.dy, 0.0)
    ..multiply(transform)
    ..translateByDouble(-offset.dx, -offset.dy, 0, 1);
    
  if (needsCompositing) {
    final layer = oldLayer ?? TransformLayer();
    layer.transform = effectiveTransform;
    pushLayer(layer, painter, offset,
      childPaintBounds: MatrixUtils.inverseTransformRect(
        effectiveTransform, estimatedBounds));
    return layer;
  } else {
    canvas
      ..save()
      ..transform(effectiveTransform.storage);
    painter(this, offset);
    canvas.restore();
    return null;
  }
}
```

### pushOpacity()

```dart
OpacityLayer pushOpacity(
  Offset offset,
  int alpha,  // 0 = transparent, 255 = opaque
  PaintingContextCallback painter, {
  OpacityLayer? oldLayer,
}) {
  // Opacity ALWAYS requires compositing (no direct canvas equivalent)
  final layer = oldLayer ?? OpacityLayer();
  layer
    ..alpha = alpha
    ..offset = offset;
  pushLayer(layer, painter, Offset.zero);
  return layer;
}
```

### pushColorFilter()

```dart
ColorFilterLayer pushColorFilter(
  Offset offset,
  ColorFilter colorFilter,
  PaintingContextCallback painter, {
  ColorFilterLayer? oldLayer,
}) {
  // Color filter ALWAYS requires compositing
  final layer = oldLayer ?? ColorFilterLayer();
  layer.colorFilter = colorFilter;
  pushLayer(layer, painter, offset);
  return layer;
}
```

## Repaint Boundary Handling

### repaintCompositedChild() (static)

```dart
static void repaintCompositedChild(
  RenderObject child, {
  bool debugAlsoPaintedParent = false,
}) {
  assert(child._needsPaint);
  _repaintCompositedChild(child, debugAlsoPaintedParent: debugAlsoPaintedParent);
}

static void _repaintCompositedChild(
  RenderObject child, {
  bool debugAlsoPaintedParent = false,
  PaintingContext? childContext,
}) {
  // Get or create the child's layer
  var childLayer = child._layerHandle.layer as OffsetLayer?;
  if (childLayer == null) {
    // First time - create layer
    final layer = child.updateCompositedLayer(oldLayer: null);
    child._layerHandle.layer = childLayer = layer;
  } else {
    // Reuse layer
    childLayer.removeAllChildren();
    child.updateCompositedLayer(oldLayer: childLayer);
  }
  
  child._needsCompositedLayerUpdate = false;
  
  // Create context and paint
  childContext ??= PaintingContext(childLayer, child.paintBounds);
  child._paintWithContext(childContext, Offset.zero);
  
  childContext.stopRecordingIfNeeded();
}
```

```
┌─────────────────────────────────────────────────────────────────────┐
│                  Repaint Boundary Flow                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Parent Layer                                                       │
│  └── PictureLayer                                                   │
│      └── [parent drawings]                                          │
│                                                                     │
│  When paintChild() encounters repaint boundary:                     │
│                                                                     │
│  1. stopRecordingIfNeeded()  - finish current Picture               │
│  2. Check if child needs paint                                      │
│  3. If needs paint: repaintCompositedChild()                        │
│  4. Append child's layer to parent                                  │
│                                                                     │
│  Result:                                                            │
│  Parent Layer                                                       │
│  ├── PictureLayer                                                   │
│  │   └── [parent drawings before child]                             │
│  └── OffsetLayer (child's layer)  <── painted independently        │
│      └── PictureLayer                                               │
│          └── [child drawings]                                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Layer Update Without Repaint

```dart
static void updateLayerProperties(RenderObject child) {
  // Called when only layer properties changed, not content
  assert(child.isRepaintBoundary && child._wasRepaintBoundary);
  assert(!child._needsPaint);
  assert(child._layerHandle.layer != null);

  final childLayer = child._layerHandle.layer! as OffsetLayer;
  child.updateCompositedLayer(oldLayer: childLayer);
  child._needsCompositedLayerUpdate = false;
}
```

## Composition Callbacks

```dart
VoidCallback addCompositionCallback(CompositionCallback callback) {
  return _containerLayer.addCompositionCallback(callback);
}
```

Allows render objects to be notified when their layer is composited.

## Rust Implementation Notes

```rust
pub struct PaintingContext {
    container_layer: Arc<ContainerLayer>,
    estimated_bounds: Rect,
    
    // Recording state (interior mutability)
    current_layer: RefCell<Option<PictureLayer>>,
    recorder: RefCell<Option<PictureRecorder>>,
    canvas: RefCell<Option<Canvas>>,
}

impl PaintingContext {
    pub fn canvas(&self) -> Ref<Canvas> {
        if self.canvas.borrow().is_none() {
            self.start_recording();
        }
        Ref::map(self.canvas.borrow(), |opt| opt.as_ref().unwrap())
    }
    
    pub fn paint_child(&self, child: &dyn RenderObject, offset: Offset) {
        if child.is_repaint_boundary() {
            self.stop_recording_if_needed();
            self.composite_child(child, offset);
        } else {
            child.paint_with_context(self, offset);
        }
    }
    
    pub fn push_clip_rect<F>(
        &self,
        needs_compositing: bool,
        offset: Offset,
        clip_rect: Rect,
        painter: F,
        clip_behavior: Clip,
        old_layer: Option<ClipRectLayer>,
    ) -> Option<ClipRectLayer>
    where
        F: FnOnce(&PaintingContext, Offset)
    {
        // Implementation similar to Dart
    }
}
```
