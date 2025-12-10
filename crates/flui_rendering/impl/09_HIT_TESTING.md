# Flutter Hit Testing Protocol

This document details Flutter's hit testing mechanism based on RenderObject and RenderBox implementations.

## Overview

Hit testing determines which render objects are located at a given position. It's used for:
- Pointer event dispatch (touch, mouse, stylus)
- Gesture recognition
- Accessibility target identification

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Hit Testing Flow                                  │
│                                                                         │
│   User Touch/Click                                                      │
│         │                                                               │
│         ▼                                                               │
│   ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐       │
│   │ GestureArena │ ← │ HitTestResult │ ← │ RenderObject.hitTest│       │
│   └─────────────┘    └──────────────┘    └─────────────────────┘       │
│         │                   │                      │                    │
│         ▼                   ▼                      ▼                    │
│   Event Dispatch      Path of hit           Tree traversal              │
│   to winner           objects               (front-to-back)             │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Core Types

### HitTestResult

Accumulates render objects hit during testing:

```dart
class HitTestResult {
  final List<HitTestEntry> path;
  
  void add(HitTestEntry entry);
  void pushTransform(Matrix4 transform);
  void pushOffset(Offset offset);
  void popTransform();
}
```

### HitTestEntry

Single entry in hit test result:

```dart
class HitTestEntry<T extends HitTestTarget> {
  final T target;
  final Matrix4 transform;
  
  Offset localPosition(Offset position);
}
```

### BoxHitTestEntry / SliverHitTestEntry

Protocol-specific entries with additional data:

```dart
class BoxHitTestEntry extends HitTestEntry<RenderBox> {
  final Offset localPosition;
}

class SliverHitTestEntry extends HitTestEntry<RenderSliver> {
  final double mainAxisPosition;
  final double crossAxisPosition;
}
```

## RenderBox Hit Testing

### Main Method

```dart
bool hitTest(BoxHitTestResult result, {required Offset position}) {
  // Transform position if needed
  
  if (_size!.contains(position)) {
    // Test children first (front to back in paint order)
    if (hitTestChildren(result, position: position) || 
        hitTestSelf(position)) {
      result.add(BoxHitTestEntry(this, position));
      return true;
    }
  }
  return false;
}
```

### Children Testing

```dart
bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
  // Default: no children, return false
  return false;
}

// For single child:
bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
  return child?.hitTest(result, position: position - child!.offset) ?? false;
}

// For multiple children (back to front):
bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
  for (final child in children.reversed) {
    final bool isHit = result.addWithPaintOffset(
      offset: child.offset,
      position: position,
      hitTest: (result, transformed) => child.hitTest(result, position: transformed),
    );
    if (isHit) return true;
  }
  return false;
}
```

### Self Testing

```dart
bool hitTestSelf(Offset position) {
  // Override to handle hits on this object itself
  // Default: false (transparent to hits)
  return false;
}
```

## Hit Test Behavior

Flutter defines several hit test behaviors via `HitTestBehavior` enum:

```dart
enum HitTestBehavior {
  /// Defers to children; only hit if child is hit
  deferToChild,
  
  /// Opaque: absorbs all hits within bounds
  opaque,
  
  /// Translucent: registers hits but allows pass-through to siblings
  translucent,
}
```

Implementation in GestureDetector/Listener:

```dart
bool hitTest(BoxHitTestResult result, {required Offset position}) {
  bool hitTarget = false;
  
  if (size.contains(position)) {
    hitTarget = hitTestChildren(result, position: position) || 
                hitTestSelf(position);
    
    if (hitTarget || behavior == HitTestBehavior.translucent) {
      result.add(BoxHitTestEntry(this, position));
    }
  }
  
  return hitTarget || behavior == HitTestBehavior.opaque;
}
```

## Transform Handling

Hit testing with transforms uses matrix operations:

```dart
// In parent, testing child with transform:
bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
  return result.addWithPaintTransform(
    transform: _transform,
    position: position,
    hitTest: (result, position) => child!.hitTest(result, position: position),
  );
}

// addWithPaintTransform implementation:
bool addWithPaintTransform({
  required Matrix4? transform,
  required Offset position,
  required BoxHitTest hitTest,
}) {
  if (transform == null) {
    return hitTest(this, position);
  }
  
  final Matrix4 inverse = Matrix4.tryInvert(transform);
  if (inverse == null) {
    // Transform is not invertible, can't hit test
    return false;
  }
  
  pushTransform(transform);
  final Offset localPosition = MatrixUtils.transformPoint(inverse, position);
  final bool isHit = hitTest(this, localPosition);
  popTransform();
  
  return isHit;
}
```

## RenderSliver Hit Testing

Slivers use axis-aligned coordinates:

```dart
bool hitTest(
  SliverHitTestResult result, {
  required double mainAxisPosition,
  required double crossAxisPosition,
}) {
  if (mainAxisPosition >= 0.0 && 
      mainAxisPosition < geometry!.hitTestExtent &&
      crossAxisPosition >= 0.0 && 
      crossAxisPosition < constraints.crossAxisExtent) {
    
    if (hitTestChildren(result, 
        mainAxisPosition: mainAxisPosition, 
        crossAxisPosition: crossAxisPosition) ||
        hitTestSelf(mainAxisPosition: mainAxisPosition, 
                    crossAxisPosition: crossAxisPosition)) {
      result.add(SliverHitTestEntry(
        this,
        mainAxisPosition: mainAxisPosition,
        crossAxisPosition: crossAxisPosition,
      ));
      return true;
    }
  }
  return false;
}
```

## Event Dispatch After Hit Test

```dart
void handleEvent(PointerEvent event, HitTestEntry entry) {
  // Called for each object in hit test path
  // entry contains local position information
  
  // Example: Listener widget
  if (event is PointerDownEvent) {
    onPointerDown?.call(event);
  }
}
```

## FLUI Implementation Considerations

### Rust Trait Design

```rust
/// Hit test result accumulator
pub struct HitTestResult<Id> {
    path: Vec<HitTestEntry<Id>>,
    transforms: Vec<Transform2D>,
}

/// Entry in hit test result
pub struct HitTestEntry<Id> {
    target: Id,
    local_position: Offset,
    transform: Transform2D,
}

/// Hit testing behavior
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HitTestBehavior {
    DeferToChild,
    Opaque,
    Translucent,
}

/// Trait for hit testable objects
pub trait HitTestable {
    type Id;
    
    fn hit_test(&self, result: &mut HitTestResult<Self::Id>, position: Offset) -> bool;
    fn hit_test_self(&self, position: Offset) -> bool { false }
}

/// Box-specific hit testing
pub trait BoxHitTestable: HitTestable {
    fn hit_test_children(
        &self, 
        result: &mut HitTestResult<Self::Id>, 
        position: Offset
    ) -> bool {
        false
    }
}
```

### Transform Stack

```rust
impl<Id> HitTestResult<Id> {
    pub fn push_transform(&mut self, transform: Transform2D) {
        self.transforms.push(transform);
    }
    
    pub fn pop_transform(&mut self) {
        self.transforms.pop();
    }
    
    pub fn add_with_transform<F>(
        &mut self,
        transform: Transform2D,
        position: Offset,
        hit_test: F,
    ) -> bool 
    where
        F: FnOnce(&mut Self, Offset) -> bool,
    {
        let Some(inverse) = transform.try_inverse() else {
            return false;
        };
        
        self.push_transform(transform);
        let local_pos = inverse.transform_point(position);
        let hit = hit_test(self, local_pos);
        self.pop_transform();
        
        hit
    }
}
```

## Performance Optimizations

1. **Bounds Check First**: Always check `size.contains(position)` before expensive operations
2. **Early Return**: Return immediately when hit found (unless translucent)
3. **Skip Invisible**: Don't test objects with `visible: false` or zero opacity
4. **Spatial Index**: For many children, consider spatial partitioning (quadtree, R-tree)
5. **Cache Inverse**: Pre-compute inverse transforms when possible

## Sources

- [RenderObject class - Flutter API](https://api.flutter.dev/flutter/rendering/RenderObject-class.html)
- [RenderBox class - Flutter API](https://api.flutter.dev/flutter/rendering/RenderBox-class.html)
- [Flutter Internals - Render Objects](https://flutter.megathink.com/data-model/render-objects)
