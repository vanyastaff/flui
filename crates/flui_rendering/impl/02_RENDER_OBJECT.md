# RenderObject Architecture

The `RenderObject` is the core abstraction for layout and painting in Flutter's rendering layer.

## Class Diagram

```
┌───────────────────────────────────────────────────────────────────────┐
│                           RenderObject                                 │
├───────────────────────────────────────────────────────────────────────┤
│ IDENTITY & TREE                                                        │
│   - parent: RenderObject?                                             │
│   - _depth: int                                                       │
│   - _owner: PipelineOwner?                                            │
│   - debugCreator: Object?                                             │
├───────────────────────────────────────────────────────────────────────┤
│ LAYOUT STATE                                                           │
│   - parentData: ParentData?                                           │
│   - _constraints: Constraints?                                        │
│   - _needsLayout: bool                                                │
│   - _isRelayoutBoundary: bool?                                        │
│   - _doingThisLayoutWithCallback: bool                                │
├───────────────────────────────────────────────────────────────────────┤
│ PAINT STATE                                                            │
│   - _needsPaint: bool                                                 │
│   - _needsCompositedLayerUpdate: bool                                 │
│   - _needsCompositing: bool                                           │
│   - _needsCompositingBitsUpdate: bool                                 │
│   - _wasRepaintBoundary: bool                                         │
│   - _layerHandle: LayerHandle<ContainerLayer>                         │
├───────────────────────────────────────────────────────────────────────┤
│ SEMANTICS                                                              │
│   - _semantics: _RenderObjectSemantics                                │
└───────────────────────────────────────────────────────────────────────┘
```

## Lifecycle

```
                    ┌──────────────────┐
                    │   Constructor    │
                    │ (sets initial    │
                    │  dirty flags)    │
                    └────────┬─────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │    adoptChild    │ <- parent calls this
                    │ setupParentData  │
                    │    attach        │
                    └────────┬─────────┘
                             │
              ┌──────────────┴──────────────┐
              │                             │
              ▼                             ▼
     ┌─────────────────┐          ┌─────────────────┐
     │ scheduleInitial │          │  markNeeds*()   │
     │    Layout       │          │  methods        │
     └────────┬────────┘          └────────┬────────┘
              │                             │
              └──────────────┬──────────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │     layout()     │
                    │   performLayout  │
                    │   performResize  │
                    └────────┬─────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │     paint()      │
                    │  paintWithContext│
                    └────────┬─────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │    dropChild     │
                    │    detach        │
                    └────────┬─────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │    dispose()     │
                    │ (cleanup layers) │
                    └──────────────────┘
```

## Layout Protocol

### Entry Point: `layout()`

```dart
void layout(Constraints constraints, {bool parentUsesSize = false})
```

```
┌──────────────────────────────────────────────────────────────────────┐
│                          layout() Flow                                │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  1. Validate constraints                                             │
│                                                                      │
│  2. Determine if relayout boundary:                                  │
│     _isRelayoutBoundary = !parentUsesSize                            │
│                         || sizedByParent                             │
│                         || constraints.isTight                       │
│                         || parent == null                            │
│                                                                      │
│  3. Early return if:                                                 │
│     - !_needsLayout && constraints == _constraints                   │
│                                                                      │
│  4. If sizedByParent:                                                │
│     - Call performResize()                                           │
│                                                                      │
│  5. Call performLayout()                                             │
│                                                                      │
│  6. markNeedsSemanticsUpdate()                                       │
│                                                                      │
│  7. _needsLayout = false                                             │
│                                                                      │
│  8. markNeedsPaint()                                                 │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### `_layoutWithoutResize()`

Called for relayout boundaries that don't need constraint changes:

```dart
void _layoutWithoutResize() {
  // Only called on relayout boundaries
  assert(_isRelayoutBoundary ?? false);
  performLayout();
  markNeedsSemanticsUpdate();
  _needsLayout = false;
  markNeedsPaint();
}
```

## Relayout Boundary Determination

A render object becomes a relayout boundary when:

```
_isRelayoutBoundary = 
    !parentUsesSize        // Parent doesn't use child's size
    || sizedByParent       // Size determined solely by constraints
    || constraints.isTight // Only one possible size
    || parent == null      // Root node
```

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Relayout Boundary Benefits                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  When child changes:        Without boundary   With boundary        │
│                                                                     │
│      Parent                     DIRTY              clean            │
│        │                          │                  │              │
│        ▼                          ▼                  ▼              │
│   ┌─────────┐               ┌─────────┐        ┌─────────┐         │
│   │ Child   │ DIRTY         │ Child   │ DIRTY  │ Child   │ DIRTY   │
│   │(boundary│────────>      │         │────────│(boundary│         │
│   │   =true)│               │         │        │   =true)│         │
│   └─────────┘               └─────────┘        └─────────┘         │
│                                                                     │
│  markNeedsLayout propagates up to relayout boundary only!           │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Paint Protocol

### Key Properties

| Property | Type | Purpose |
|----------|------|---------|
| `isRepaintBoundary` | bool | Whether this node creates its own layer |
| `alwaysNeedsCompositing` | bool | Force compositing (e.g., video) |
| `needsCompositing` | bool | Whether subtree needs compositing |
| `layer` | ContainerLayer? | The compositing layer (if any) |
| `paintBounds` | Rect | Estimated painting bounds |

### Repaint Boundary

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Repaint Boundary Benefits                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Without boundary:              With boundary:                       │
│                                                                     │
│    ┌──────────────┐              ┌──────────────┐                   │
│    │   Parent     │ REPAINT      │   Parent     │ clean             │
│    │   Layer      │              │   Layer      │                   │
│    │  ┌────────┐  │              │              │                   │
│    │  │ Child  │  │              └──────┬───────┘                   │
│    │  │(no own │  │                     │                           │
│    │  │ layer) │  │              ┌──────┴───────┐                   │
│    │  └────────┘  │              │ Child Layer  │ REPAINT           │
│    └──────────────┘              │(isRepaint    │                   │
│                                  │ Boundary)    │                   │
│  Child changes require           └──────────────┘                   │
│  repainting parent               Only child layer repaints          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### `updateCompositedLayer()`

Called for repaint boundaries to create/update their layer:

```dart
OffsetLayer updateCompositedLayer({required OffsetLayer? oldLayer}) {
  assert(isRepaintBoundary);
  return oldLayer ?? OffsetLayer();
}
```

## Tree Operations

### `adoptChild()`

```dart
void adoptChild(RenderObject child) {
  setupParentData(child);    // Initialize parent-specific data
  markNeedsLayout();         // Parent needs layout
  markNeedsCompositingBitsUpdate();
  markNeedsSemanticsUpdate();
  child._parent = this;
  if (attached) {
    child.attach(_owner!);   // Propagate owner
  }
  redepthChild(child);       // Update depth
}
```

### `dropChild()`

```dart
void dropChild(RenderObject child) {
  child.parentData!.detach();
  child.parentData = null;
  child._parent = null;
  if (attached) {
    child.detach();
  }
  markNeedsLayout();
  markNeedsCompositingBitsUpdate();
  markNeedsSemanticsUpdate();
}
```

## Transform Operations

### `getTransformTo()`

Computes the paint transform from this node to a target:

```
┌─────────────────────────────────────────────────────────────────────┐
│                      getTransformTo Algorithm                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. Find common ancestor by walking up from both nodes              │
│                                                                     │
│  2. Build fromPath: [this, parent, ..., ancestor]                   │
│     Build toPath:   [target, parent, ..., ancestor]                 │
│                                                                     │
│  3. Compute fromTransform by calling applyPaintTransform             │
│     down the fromPath                                               │
│                                                                     │
│  4. Compute toTransform by calling applyPaintTransform               │
│     down the toPath, then invert it                                 │
│                                                                     │
│  5. Return fromTransform * toTransform                              │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Debug Features

| Method | Purpose |
|--------|---------|
| `debugDoingThisLayout` | Check if currently in layout |
| `debugDoingThisPaint` | Check if currently in paint |
| `debugNeedsLayout` | Check layout dirty flag |
| `debugNeedsPaint` | Check paint dirty flag |
| `debugCanParentUseSize` | Parent's size dependency |
| `describeForError()` | Generate error diagnostics |

## Rust Implementation Notes

For FLUI, consider these Rust-specific patterns:

```rust
// Trait-based approach
pub trait RenderObject: Send + Sync {
    fn parent(&self) -> Option<&dyn RenderObject>;
    fn constraints(&self) -> &dyn Constraints;
    fn perform_layout(&mut self);
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
}

// Use interior mutability for dirty flags
pub struct RenderObjectState {
    needs_layout: Cell<bool>,
    needs_paint: Cell<bool>,
    constraints: RefCell<Option<BoxConstraints>>,
}

// Type-safe parent data with GATs
pub trait RenderObjectWithParentData {
    type ParentData: ParentData;
}
```
