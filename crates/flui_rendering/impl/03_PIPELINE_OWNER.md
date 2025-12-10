# PipelineOwner Architecture

The `PipelineOwner` manages the rendering pipeline and coordinates all phases of frame production.

## Class Structure

```
┌───────────────────────────────────────────────────────────────────────┐
│                          PipelineOwner                                 │
├───────────────────────────────────────────────────────────────────────┤
│ CALLBACKS                                                              │
│   - onNeedVisualUpdate: VoidCallback?                                 │
│   - onSemanticsOwnerCreated: VoidCallback?                            │
│   - onSemanticsUpdate: SemanticsUpdateCallback?                       │
│   - onSemanticsOwnerDisposed: VoidCallback?                           │
├───────────────────────────────────────────────────────────────────────┤
│ TREE MANAGEMENT                                                        │
│   - _rootNode: RenderObject?                                          │
│   - _children: Set<PipelineOwner>                                     │
│   - _manifold: PipelineManifold?                                      │
├───────────────────────────────────────────────────────────────────────┤
│ DIRTY LISTS                                                            │
│   - _nodesNeedingLayout: List<RenderObject>                           │
│   - _nodesNeedingCompositingBitsUpdate: List<RenderObject>            │
│   - _nodesNeedingPaint: List<RenderObject>                            │
│   - _nodesNeedingSemantics: Set<RenderObject>                         │
├───────────────────────────────────────────────────────────────────────┤
│ STATE                                                                  │
│   - _semanticsOwner: SemanticsOwner?                                  │
│   - _outstandingSemanticsHandles: int                                 │
│   - _shouldMergeDirtyNodes: bool                                      │
└───────────────────────────────────────────────────────────────────────┘
```

## Flush Methods Sequence

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Frame Production Flow                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   ┌───────────────────┐                                             │
│   │ requestVisualUpdate│ (triggered by markNeeds* methods)          │
│   └─────────┬─────────┘                                             │
│             │                                                       │
│             ▼                                                       │
│   ┌───────────────────────────────────────────────────────────┐     │
│   │                    FRAME BEGIN                             │     │
│   └───────────────────────────────────────────────────────────┘     │
│             │                                                       │
│             ▼                                                       │
│   ┌───────────────────┐                                             │
│   │   flushLayout()   │ Process _nodesNeedingLayout                 │
│   │                   │ Sort by depth (shallowest first)            │
│   │                   │ Call _layoutWithoutResize() on each         │
│   └─────────┬─────────┘                                             │
│             │                                                       │
│             ▼                                                       │
│   ┌───────────────────┐                                             │
│   │flushCompositing   │ Process _nodesNeedingCompositingBitsUpdate  │
│   │     Bits()        │ Sort by depth (shallowest first)            │
│   │                   │ Call _updateCompositingBits() on each       │
│   └─────────┬─────────┘                                             │
│             │                                                       │
│             ▼                                                       │
│   ┌───────────────────┐                                             │
│   │   flushPaint()    │ Process _nodesNeedingPaint                  │
│   │                   │ Sort by depth (DEEPEST first!)              │
│   │                   │ Call repaintCompositedChild/updateLayer     │
│   └─────────┬─────────┘                                             │
│             │                                                       │
│             ▼                                                       │
│   ┌───────────────────┐                                             │
│   │  flushSemantics() │ Process _nodesNeedingSemantics              │
│   │                   │ Sort by depth (shallowest first)            │
│   │                   │ Update accessibility tree                   │
│   └─────────┬─────────┘                                             │
│             │                                                       │
│             ▼                                                       │
│   ┌───────────────────────────────────────────────────────────┐     │
│   │                     FRAME END                              │     │
│   └───────────────────────────────────────────────────────────┘     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## flushLayout() Details

```dart
void flushLayout() {
  while (_nodesNeedingLayout.isNotEmpty) {
    final List<RenderObject> dirtyNodes = _nodesNeedingLayout;
    _nodesNeedingLayout = <RenderObject>[];
    
    // Sort shallowest first (parents before children)
    dirtyNodes.sort((a, b) => a.depth - b.depth);
    
    for (var i = 0; i < dirtyNodes.length; i++) {
      // Check for dirty node merging after layout callbacks
      if (_shouldMergeDirtyNodes) {
        _shouldMergeDirtyNodes = false;
        if (_nodesNeedingLayout.isNotEmpty) {
          _nodesNeedingLayout.addAll(dirtyNodes.getRange(i, dirtyNodes.length));
          break;
        }
      }
      
      final node = dirtyNodes[i];
      if (node._needsLayout && node.owner == this) {
        node._layoutWithoutResize();
      }
    }
  }
  
  // Recursively flush children PipelineOwners
  for (final child in _children) {
    child.flushLayout();
  }
}
```

### Layout Depth Order

```
┌─────────────────────────────────────────────────────────────────────┐
│  Why shallowest-first for layout?                                    │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  depth=0:  ┌──────────┐   Process first                            │
│            │   Root   │   ─────────────────────>                    │
│            └────┬─────┘                                             │
│  depth=1:       │                                                   │
│            ┌────┴────┐                                              │
│            ▼         ▼                                              │
│       ┌──────┐  ┌──────┐  Process second                           │
│       │  A   │  │  B   │  ─────────────────────>                    │
│       └──┬───┘  └──┬───┘                                            │
│  depth=2:│         │                                                │
│          ▼         ▼                                                │
│       ┌────┐    ┌────┐   Process last                              │
│       │ A1 │    │ B1 │   ─────────────────────>                     │
│       └────┘    └────┘                                              │
│                                                                     │
│  Parents must layout first to provide constraints to children!      │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## flushPaint() Details

```dart
void flushPaint() {
  final dirtyNodes = _nodesNeedingPaint;
  _nodesNeedingPaint = <RenderObject>[];

  // Sort DEEPEST first (children before parents)
  for (final node in dirtyNodes..sort((a, b) => b.depth - a.depth)) {
    if ((node._needsPaint || node._needsCompositedLayerUpdate) 
        && node.owner == this) {
      if (node._layerHandle.layer!.attached) {
        if (node._needsPaint) {
          PaintingContext.repaintCompositedChild(node);
        } else {
          PaintingContext.updateLayerProperties(node);
        }
      } else {
        node._skippedPaintingOnLayer();
      }
    }
  }
  
  // Recursively flush children PipelineOwners
  for (final child in _children) {
    child.flushPaint();
  }
}
```

### Paint Depth Order

```
┌─────────────────────────────────────────────────────────────────────┐
│  Why deepest-first for paint?                                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  depth=0:  ┌──────────┐   Process last                             │
│            │   Root   │   <─────────────────────                    │
│            └────┬─────┘                                             │
│  depth=1:       │                                                   │
│            ┌────┴────┐                                              │
│            ▼         ▼                                              │
│       ┌──────┐  ┌──────┐  Process second                           │
│       │  A   │  │  B   │  <─────────────────────                    │
│       └──┬───┘  └──┬───┘                                            │
│  depth=2:│         │                                                │
│          ▼         ▼                                                │
│       ┌────┐    ┌────┐   Process first                             │
│       │ A1 │    │ B1 │   <─────────────────────                     │
│       └────┘    └────┘                                              │
│                                                                     │
│  Children may be repaint boundaries. Painting them first ensures    │
│  their layers are ready when parents paint and compose them.        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## PipelineOwner Tree

PipelineOwners can form a hierarchy:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PipelineOwner Hierarchy                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│           ┌─────────────────────────┐                               │
│           │  PipelineManifold       │                               │
│           │  (binding/platform)     │                               │
│           └───────────┬─────────────┘                               │
│                       │ attach                                      │
│                       ▼                                             │
│           ┌─────────────────────────┐                               │
│           │  Root PipelineOwner     │  (main render tree)           │
│           │  - _rootNode            │                               │
│           └───────────┬─────────────┘                               │
│                       │ adoptChild                                  │
│         ┌─────────────┼─────────────┐                               │
│         ▼             ▼             ▼                               │
│    ┌──────────┐  ┌──────────┐  ┌──────────┐                        │
│    │ Child 1  │  │ Child 2  │  │ Child 3  │ (off-screen trees)     │
│    │PipelineO │  │PipelineO │  │PipelineO │                        │
│    └──────────┘  └──────────┘  └──────────┘                        │
│                                                                     │
│  Each PipelineOwner manages its own dirty lists.                    │
│  Parent flushes itself, then flushes all children.                  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Layout Callback Handling

The `_enableMutationsToDirtySubtrees` mechanism supports `invokeLayoutCallback`:

```dart
void _enableMutationsToDirtySubtrees(VoidCallback callback) {
  assert(_debugDoingLayout);
  bool? oldState;
  assert(() {
    oldState = _debugAllowMutationsToDirtySubtrees;
    _debugAllowMutationsToDirtySubtrees = true;
    return true;
  }());
  try {
    callback();
  } finally {
    _shouldMergeDirtyNodes = true;  // Signal to merge new dirty nodes
    assert(() {
      _debugAllowMutationsToDirtySubtrees = oldState!;
      return true;
    }());
  }
}
```

This allows `LayoutBuilder` and similar widgets to modify the tree during layout.

## PipelineManifold Interface

```dart
abstract class PipelineManifold implements Listenable {
  /// Whether semantics collection is enabled
  bool get semanticsEnabled;
  
  /// Request a visual update (schedule frame)
  void requestVisualUpdate();
}
```

## Rust Implementation Considerations

```rust
pub struct PipelineOwner {
    // Root of the render tree this owner manages
    root_node: Option<Arc<RwLock<dyn RenderObject>>>,
    
    // Dirty lists - use concurrent collections for parallel access
    nodes_needing_layout: Mutex<Vec<RenderObjectId>>,
    nodes_needing_compositing_bits_update: Mutex<Vec<RenderObjectId>>,
    nodes_needing_paint: Mutex<Vec<RenderObjectId>>,
    nodes_needing_semantics: Mutex<HashSet<RenderObjectId>>,
    
    // Callbacks
    on_need_visual_update: Option<Box<dyn Fn() + Send + Sync>>,
    
    // Child pipeline owners
    children: RwLock<HashSet<Arc<PipelineOwner>>>,
    
    // Parent manifold
    manifold: Option<Weak<dyn PipelineManifold>>,
}

impl PipelineOwner {
    pub fn flush_layout(&self) {
        // Sort by depth ascending
        // Process each dirty node
        // Handle layout callbacks
        // Recurse to children
    }
    
    pub fn flush_paint(&self) {
        // Sort by depth descending  
        // Process each dirty node
        // Recurse to children
    }
}
```

## Key Invariants

1. **Dirty nodes must be relayout boundaries** (for layout) or **repaint boundaries** (for paint)
2. **Child PipelineOwners must not dirty nodes in parent**
3. **No tree modifications between flushLayout start and flushSemantics end**
4. **Layout processes shallowest first, paint processes deepest first**
5. **Each flush method recursively calls children after processing own nodes**
