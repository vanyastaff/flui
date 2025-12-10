# Flutter Relayout and Repaint Boundaries

This document details Flutter's boundary optimization system for layout and paint.

## Overview

Boundaries are a critical performance optimization in Flutter's rendering system. They prevent unnecessary propagation of dirty flags up the tree:

- **Relayout Boundary**: Isolates layout changes to a subtree
- **Repaint Boundary**: Isolates paint changes to a subtree

## Relayout Boundaries

### Purpose

When `markNeedsLayout()` is called, it normally propagates up to the root. A relayout boundary stops this propagation, limiting the scope of relayout work.

### Visual Example

```
Without Relayout Boundary:
┌────────────────────────────────────┐
│ Root (dirty)                       │
│   └── Parent (dirty)               │
│         └── Container (dirty)      │
│               └── Text (changed)   │  ← Layout change here
└────────────────────────────────────┘
   All ancestors marked dirty!

With Relayout Boundary:
┌────────────────────────────────────┐
│ Root (clean)                       │
│   └── Parent (clean)               │
│         └── Container [BOUNDARY]   │  ← Relayout stops here
│               └── Text (dirty)     │  ← Layout change here
└────────────────────────────────────┘
   Only subtree needs relayout!
```

### Conditions for Relayout Boundary

A RenderObject becomes its own relayout boundary when ANY of these is true:

```dart
bool get _relayoutBoundary {
  return sizedByParent ||              // Size depends only on constraints
         constraints.isTight ||         // Constraints specify exact size
         !attached ||                   // Not in tree
         parent is! RenderObject ||     // Root of render tree
         !_needsLayout && !_needsPaint; // Already clean (optimization)
}
```

Simplified:

1. **`sizedByParent = true`**: Object's size is determined solely by parent constraints
2. **`constraints.isTight`**: Parent gave exact size (no flexibility)
3. **`parentUsesSize = false`**: Parent doesn't use child's computed size

### Implementation

```dart
void markNeedsLayout() {
  if (_needsLayout) {
    return; // Already dirty
  }
  
  if (_relayoutBoundary != this) {
    // Propagate to parent
    markParentNeedsLayout();
  } else {
    // We ARE the boundary - register with pipeline owner
    _needsLayout = true;
    if (owner != null) {
      owner!._nodesNeedingLayout.add(this);
      owner!.requestVisualUpdate();
    }
  }
}

void markParentNeedsLayout() {
  _needsLayout = true;
  final RenderObject parent = this.parent! as RenderObject;
  if (!_doingThisLayoutWithCallback) {
    parent.markNeedsLayout();
  }
}
```

### Layout with Boundary Optimization

```dart
void layout(Constraints constraints, {bool parentUsesSize = false}) {
  // Determine if this is a relayout boundary
  final bool isRelayoutBoundary = !parentUsesSize || 
                                   sizedByParent || 
                                   constraints.isTight;
  
  final RenderObject? relayoutBoundary = isRelayoutBoundary 
      ? this 
      : parent!._relayoutBoundary;
  
  // Skip layout if constraints haven't changed and we're clean
  if (!_needsLayout && 
      constraints == _constraints &&
      relayoutBoundary == _relayoutBoundary) {
    return;
  }
  
  _constraints = constraints;
  _relayoutBoundary = relayoutBoundary;
  
  if (sizedByParent) {
    performResize();
  }
  
  performLayout();
  _needsLayout = false;
  markNeedsPaint();
}
```

## Repaint Boundaries

### Purpose

When `markNeedsPaint()` is called, it propagates up to the nearest repaint boundary. Objects at repaint boundaries get their own compositing layer, enabling efficient partial repaints.

### Visual Example

```
Layer Tree Structure:

ContainerLayer (root)
├── PictureLayer (background)
├── OffsetLayer [REPAINT BOUNDARY - ListView]
│   ├── PictureLayer (visible items 0-5)
│   └── PictureLayer (visible items 6-10)
├── PictureLayer (header, footer)
└── OffsetLayer [REPAINT BOUNDARY - Overlay]
    └── PictureLayer (popup content)

When item 3 changes:
- Only "visible items 0-5" PictureLayer is recreated
- All other layers are reused from cache
```

### Enabling Repaint Boundary

```dart
@override
bool get isRepaintBoundary => true; // Override to enable

// Or use RepaintBoundary widget:
RepaintBoundary(
  child: ExpensiveWidget(),
)
```

### Implementation

```dart
void markNeedsPaint() {
  if (_needsPaint) {
    return; // Already dirty
  }
  
  _needsPaint = true;
  
  if (isRepaintBoundary) {
    // We ARE the boundary - register with pipeline owner
    if (owner != null) {
      owner!._nodesNeedingPaint.add(this);
      owner!.requestVisualUpdate();
    }
  } else if (parent is RenderObject) {
    // Propagate to parent
    (parent! as RenderObject).markNeedsPaint();
  } else {
    // Root of tree
    if (owner != null) {
      owner!.requestVisualUpdate();
    }
  }
}
```

### Layer Management

Repaint boundaries manage their own layers:

```dart
// Layer handle for compositing
ContainerLayer? _layer;

void _paintWithContext(PaintingContext context, Offset offset) {
  if (isRepaintBoundary) {
    // Create/update our own layer
    _layer = context.pushLayer(
      _layer as OffsetLayer? ?? OffsetLayer(),
      _paintWithoutLayer,
      offset,
    );
  } else {
    _paintWithoutLayer(context, offset);
  }
  
  _needsPaint = false;
}
```

## Compositing Bits

The `needsCompositing` flag tracks whether any descendant needs a compositing layer:

```dart
bool _needsCompositingBitsUpdate = false;

void markNeedsCompositingBitsUpdate() {
  if (_needsCompositingBitsUpdate) return;
  
  _needsCompositingBitsUpdate = true;
  
  if (parent is RenderObject) {
    final RenderObject parent = this.parent! as RenderObject;
    if (parent._needsCompositingBitsUpdate) return;
    
    if (!isRepaintBoundary && !parent.isRepaintBoundary) {
      parent.markNeedsCompositingBitsUpdate();
    }
  }
  
  owner?._nodesNeedingCompositingBitsUpdate.add(this);
}

void _updateCompositingBits() {
  if (!_needsCompositingBitsUpdate) return;
  
  final bool oldNeedsCompositing = _needsCompositing;
  _needsCompositing = false;
  
  visitChildren((child) {
    child._updateCompositingBits();
    if (child.needsCompositing) {
      _needsCompositing = true;
    }
  });
  
  if (isRepaintBoundary || alwaysNeedsCompositing) {
    _needsCompositing = true;
  }
  
  if (oldNeedsCompositing != _needsCompositing) {
    markNeedsPaint();
  }
  
  _needsCompositingBitsUpdate = false;
}
```

## Best Practices

### When to Use Repaint Boundaries

**Good candidates:**
- Scrollable content (ListView items)
- Animations that change frequently
- Complex drawings that rarely change
- Isolated interactive areas

**Avoid for:**
- Simple, cheap-to-paint widgets
- Deeply nested widgets (layer overhead)
- Widgets that always repaint with parent

### When to Use Relayout Boundaries

**Automatic cases:**
- Fixed-size containers
- Widgets with tight constraints
- `SizedBox` with explicit dimensions

**Manual optimization:**
- Override `sizedByParent` when layout doesn't depend on children

## FLUI Implementation Considerations

### Boundary Flags in RenderState

```rust
bitflags! {
    pub struct RenderFlags: u32 {
        const NEEDS_LAYOUT = 1 << 0;
        const NEEDS_PAINT = 1 << 1;
        const NEEDS_COMPOSITING = 1 << 2;
        
        // Boundary flags
        const IS_RELAYOUT_BOUNDARY = 1 << 3;
        const IS_REPAINT_BOUNDARY = 1 << 4;
        const SIZED_BY_PARENT = 1 << 5;
        
        // Compositing
        const ALWAYS_NEEDS_COMPOSITING = 1 << 6;
        const DESCENDANT_NEEDS_COMPOSITING = 1 << 7;
    }
}
```

### Relayout Boundary Calculation

```rust
impl<P: Protocol> RenderState<P> {
    pub fn is_relayout_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_RELAYOUT_BOUNDARY)
    }
    
    pub fn compute_relayout_boundary(&self, constraints: &P::Constraints) -> bool {
        self.flags.contains(RenderFlags::SIZED_BY_PARENT) ||
        constraints.is_tight() ||
        !self.parent_uses_size
    }
}
```

### Smart Dirty Propagation

```rust
pub fn mark_needs_layout(&self, id: RenderId, tree: &mut impl RenderTree) {
    if self.flags.needs_layout() {
        return; // Early exit optimization
    }
    
    self.flags.set(RenderFlags::NEEDS_LAYOUT);
    
    if self.is_relayout_boundary() {
        // Register with pipeline owner
        tree.register_needs_layout(id);
    } else if let Some(parent_id) = tree.parent(id) {
        // Propagate to parent
        if let Some(parent_state) = tree.render_state(parent_id) {
            parent_state.mark_needs_layout(parent_id, tree);
        }
    }
}
```

### Layer Handle Integration

```rust
/// Handle to compositing layer for repaint boundaries
pub struct LayerHandle {
    layer_id: Option<LayerId>,
}

impl LayerHandle {
    pub fn ensure_layer(&mut self, layer_tree: &mut LayerTree) -> LayerId {
        if let Some(id) = self.layer_id {
            id
        } else {
            let id = layer_tree.create_offset_layer();
            self.layer_id = Some(id);
            id
        }
    }
}
```

## Sources

- [RenderObject class - Flutter API](https://api.flutter.dev/flutter/rendering/RenderObject-class.html)
- [Flutter Architectural Overview](https://docs.flutter.dev/resources/architectural-overview)
- [RepaintBoundary class](https://api.flutter.dev/flutter/widgets/RepaintBoundary-class.html)
