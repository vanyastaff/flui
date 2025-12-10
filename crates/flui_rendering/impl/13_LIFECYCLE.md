# Flutter RenderObject Lifecycle

This document details the complete lifecycle of a RenderObject from creation to disposal.

## Lifecycle States

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    RenderObject Lifecycle                                │
│                                                                         │
│   ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐      │
│   │ Created  │ ──► │ Attached │ ──► │ Laid Out │ ──► │ Painted  │      │
│   │          │     │          │     │          │     │          │      │
│   └──────────┘     └────┬─────┘     └────┬─────┘     └────┬─────┘      │
│        │                │                │                │            │
│        │                │                │                │            │
│        │                ▼                ▼                ▼            │
│        │           ┌──────────┐     ┌──────────┐     ┌──────────┐      │
│        │           │ Detached │ ◄── │ Needs    │ ◄── │ Needs    │      │
│        │           │          │     │ Layout   │     │ Paint    │      │
│        │           └────┬─────┘     └──────────┘     └──────────┘      │
│        │                │                                              │
│        │                ▼                                              │
│        │           ┌──────────┐                                        │
│        └─────────► │ Disposed │                                        │
│                    └──────────┘                                        │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## State Descriptions

### Created
- RenderObject instance exists but is not in any tree
- `attached = false`
- `owner = null`
- Cannot participate in layout/paint

### Attached
- Part of a render tree with a PipelineOwner
- `attached = true`
- `owner != null`
- Ready for layout/paint/semantics

### Laid Out
- Layout has been performed
- `_needsLayout = false`
- Has valid size/geometry
- May still need paint

### Painted
- Paint has been performed
- `_needsPaint = false`
- Visual representation is current
- Ready for display

### Needs Layout
- Layout is invalid
- `_needsLayout = true`
- Size may have changed
- Automatically needs paint too

### Needs Paint
- Only paint is invalid (layout still valid)
- `_needsPaint = true`
- `_needsLayout = false`
- Size is unchanged

### Detached
- Removed from tree but not disposed
- `attached = false`
- Can be re-attached (e.g., GlobalKey reparenting)
- Resources still held

### Disposed
- All resources released
- Cannot be reused
- Layers, pictures, images freed

## Core Lifecycle Methods

### attach(PipelineOwner owner)

Called when added to a tree with a PipelineOwner:

```dart
@override
void attach(PipelineOwner owner) {
  super.attach(owner);
  
  // Register dirty state with new owner
  if (_needsLayout && _relayoutBoundary != null) {
    _needsLayout = false;
    markNeedsLayout();
  }
  if (_needsCompositingBitsUpdate) {
    _needsCompositingBitsUpdate = false;
    markNeedsCompositingBitsUpdate();
  }
  if (_needsPaint && _layerHandle.layer != null) {
    _needsPaint = false;
    markNeedsPaint();
  }
  if (_needsSemanticsUpdate && _semanticsConfiguration.isSemanticBoundary) {
    _needsSemanticsUpdate = false;
    markNeedsSemanticsUpdate();
  }
}
```

### detach()

Called when removed from tree:

```dart
@override
void detach() {
  super.detach();
  // owner is now null
  // Remove from dirty lists (handled by PipelineOwner)
}
```

### adoptChild(RenderObject child)

Called by parent when adding a child:

```dart
@override
void adoptChild(RenderObject child) {
  setupParentData(child);
  markNeedsLayout();
  markNeedsCompositingBitsUpdate();
  markNeedsSemanticsUpdate();
  super.adoptChild(child);
}
```

### dropChild(RenderObject child)

Called by parent when removing a child:

```dart
@override
void dropChild(RenderObject child) {
  child._cleanRelayoutBoundary();
  child.parentData!.detach();
  child.parentData = null;
  super.dropChild(child);
  markNeedsLayout();
  markNeedsCompositingBitsUpdate();
  markNeedsSemanticsUpdate();
}
```

### dispose()

Release all resources:

```dart
@override
void dispose() {
  _layerHandle.layer = null; // Releases layer
  super.dispose();
}
```

## Dirty Flag Transitions

### markNeedsLayout()

```dart
void markNeedsLayout() {
  if (_needsLayout) return;
  
  if (_relayoutBoundary == null) {
    _needsLayout = true;
    if (parent != null) {
      markParentNeedsLayout();
    }
    return;
  }
  
  if (_relayoutBoundary != this) {
    markParentNeedsLayout();
  } else {
    _needsLayout = true;
    if (owner != null) {
      owner!._nodesNeedingLayout.add(this);
      owner!.requestVisualUpdate();
    }
  }
}
```

### markNeedsPaint()

```dart
void markNeedsPaint() {
  if (_needsPaint) return;
  
  _needsPaint = true;
  
  if (isRepaintBoundary && _wasRepaintBoundary) {
    if (owner != null) {
      owner!._nodesNeedingPaint.add(this);
      owner!.requestVisualUpdate();
    }
  } else if (parent != null) {
    parent!.markNeedsPaint();
  } else {
    // Root of tree
    if (owner != null) {
      owner!.requestVisualUpdate();
    }
  }
}
```

## Layout Protocol

### layout(constraints, {parentUsesSize})

```dart
void layout(Constraints constraints, {bool parentUsesSize = false}) {
  // 1. Determine relayout boundary
  final bool isRelayoutBoundary = !parentUsesSize || 
                                   sizedByParent || 
                                   constraints.isTight || 
                                   parent is! RenderObject;
  
  final RenderObject relayoutBoundary = isRelayoutBoundary 
      ? this 
      : (parent! as RenderObject)._relayoutBoundary!;
  
  // 2. Check if layout needed
  if (!_needsLayout && 
      constraints == _constraints && 
      relayoutBoundary == _relayoutBoundary) {
    return;
  }
  
  // 3. Store constraints and boundary
  _constraints = constraints;
  if (_relayoutBoundary != null && relayoutBoundary != _relayoutBoundary) {
    visitChildren(_cleanChildRelayoutBoundary);
  }
  _relayoutBoundary = relayoutBoundary;
  
  // 4. Perform layout
  if (sizedByParent) {
    performResize();
  }
  performLayout();
  
  // 5. Mark clean and trigger paint
  _needsLayout = false;
  markNeedsPaint();
}
```

## Paint Protocol

### _paintWithContext(context, offset)

```dart
void _paintWithContext(PaintingContext context, Offset offset) {
  // 1. Handle repaint boundaries with layers
  if (_needsLayout) return;
  
  RenderObject? debugLastActivePaint;
  
  _needsPaint = false;
  _needsCompositedLayerUpdate = false;
  
  try {
    paint(context, offset);
  } catch (e) {
    _reportException('paint', e);
  }
}
```

## Invariants

### Tree Invariants
1. Every attached RenderObject has an owner
2. Every RenderObject with a parent has parentData
3. Children's depth > parent's depth
4. Relayout boundary is always an ancestor (or self)

### Dirty Flag Invariants
1. If `_needsLayout`, then `_needsPaint`
2. If attached and dirty, registered with owner's dirty list
3. Relayout boundary must not be dirty when children need layout

### Layout Invariants
1. Constraints are immutable during layout
2. Size must satisfy constraints
3. Parent must call `layout()` on children

## FLUI Typestate Design

Using Rust's type system to enforce lifecycle at compile time:

```rust
/// Lifecycle state markers (zero-sized types)
pub mod state {
    pub struct Detached;
    pub struct Attached;
    pub struct LaidOut;
    pub struct Painted;
}

/// RenderNode with typestate
pub struct RenderNode<S> {
    render_object: Box<dyn RenderObject>,
    parent: Option<RenderId>,
    children: Vec<RenderId>,
    _state: PhantomData<S>,
}

/// State transitions as consuming methods
impl RenderNode<Detached> {
    pub fn attach(self, owner: &mut PipelineOwner) -> RenderNode<Attached> {
        // Transfer ownership, return new state
    }
}

impl RenderNode<Attached> {
    pub fn layout(self, constraints: BoxConstraints) -> RenderNode<LaidOut> {
        // Perform layout, return new state
    }
    
    pub fn detach(self) -> RenderNode<Detached> {
        // Remove from tree
    }
}

impl RenderNode<LaidOut> {
    pub fn paint(self, context: &mut PaintContext) -> RenderNode<Painted> {
        // Perform paint
    }
    
    pub fn mark_needs_layout(self) -> RenderNode<Attached> {
        // Invalidate layout
    }
}

impl RenderNode<Painted> {
    pub fn mark_needs_paint(self) -> RenderNode<LaidOut> {
        // Invalidate paint only
    }
    
    pub fn mark_needs_layout(self) -> RenderNode<Attached> {
        // Invalidate layout (implies paint)
    }
}
```

### Benefits of Typestate

1. **Compile-time safety**: Can't paint without layout
2. **Clear API**: Methods only available in valid states
3. **Documentation**: State encoded in types
4. **No runtime overhead**: PhantomData is zero-sized

### Challenges

1. **Storage**: Tree needs to store heterogeneous states
2. **Batching**: Pipeline owner needs to process multiple states
3. **Flexibility**: Some operations valid in multiple states

### Hybrid Approach

```rust
/// Runtime state for storage, typestate for API
pub enum AnyRenderNode {
    Detached(RenderNode<Detached>),
    Attached(RenderNode<Attached>),
    LaidOut(RenderNode<LaidOut>),
    Painted(RenderNode<Painted>),
}

impl AnyRenderNode {
    pub fn state(&self) -> LifecycleState {
        match self {
            Self::Detached(_) => LifecycleState::Detached,
            Self::Attached(_) => LifecycleState::Attached,
            Self::LaidOut(_) => LifecycleState::LaidOut,
            Self::Painted(_) => LifecycleState::Painted,
        }
    }
}
```

## Sources

- [RenderObject class - Flutter API](https://api.flutter.dev/flutter/rendering/RenderObject-class.html)
- [PipelineOwner class - Flutter API](https://api.flutter.dev/flutter/rendering/PipelineOwner-class.html)
- [Flutter Internals - Render Objects](https://flutter.megathink.com/data-model/render-objects)
