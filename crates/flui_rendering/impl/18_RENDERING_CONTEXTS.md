# Rendering Contexts

This document describes the context objects used during different phases of rendering:
painting, hit testing, and layout.

## Overview

Flutter uses several context objects to carry state during rendering phases:

| Context | Phase | Purpose |
|---------|-------|---------|
| `PaintingContext` | Paint | Canvas access, layer management, compositing |
| `HitTestResult` | Hit Test | Track hit path, transform stack |
| `BoxHitTestResult` | Hit Test | Box-specific coordinate transforms |
| `SliverHitTestResult` | Hit Test | Sliver-specific axis transforms |
| `Constraints` | Layout | Parent → Child layout requirements |

## 1. PaintingContext

The `PaintingContext` provides a painting surface and manages compositing layers.

### Flutter API

```dart
class PaintingContext extends ClipContext {
  // Canvas access (may change during child painting!)
  Canvas get canvas;
  PictureRecorder get recorder;
  Rect get estimatedBounds;
  
  // Paint child at offset
  void paintChild(RenderObject child, Offset offset);
  
  // Push effects (create compositing layers)
  void pushClipRect(bool needsCompositing, Offset offset, Rect clipRect, 
                    PaintingContextCallback painter);
  void pushClipRRect(bool needsCompositing, Offset offset, RRect clipRRect,
                     PaintingContextCallback painter);
  void pushClipPath(bool needsCompositing, Offset offset, Path clipPath,
                    PaintingContextCallback painter);
  void pushOpacity(Offset offset, int alpha, PaintingContextCallback painter);
  void pushTransform(bool needsCompositing, Offset offset, Matrix4 transform,
                     PaintingContextCallback painter);
  void pushColorFilter(Offset offset, ColorFilter colorFilter,
                       PaintingContextCallback painter);
  
  // Layer management
  void addLayer(Layer layer);
  void appendLayer(Layer layer);
  PaintingContext createChildContext(ContainerLayer childLayer, Rect bounds);
  
  // Recording control
  void stopRecordingIfNeeded();
  
  // Optimization hints
  void setIsComplexHint();   // Complex painting, cache result
  void setWillChangeHint();  // Will change next frame, don't cache
}
```

### Key Design Points

1. **Canvas Can Change**: During `paintChild()`, the canvas reference may change
   due to compositing layers. Never store canvas reference across child painting.

2. **Push Pattern**: `push*` methods create new compositing layers when
   `needsCompositing` is true, otherwise apply directly to canvas.

3. **Recording Model**: Uses `PictureRecorder` to record draw operations
   into a `Picture` for later playback.

### Rust Adaptation

```rust
/// Context for paint phase operations.
pub struct PaintContext<'a> {
    /// Current canvas surface
    canvas: &'a mut Canvas,
    
    /// Layer stack for compositing
    layer_stack: Vec<LayerHandle>,
    
    /// Bounds estimation for debugging
    estimated_bounds: Rect,
    
    /// Complexity hints
    is_complex: bool,
    will_change: bool,
}

impl<'a> PaintContext<'a> {
    /// Paint a child render object at offset.
    /// 
    /// # Important
    /// The canvas reference may become invalid after this call
    /// due to compositing layer changes.
    pub fn paint_child(&mut self, child: &impl RenderObject, offset: Offset) {
        // ...
    }
    
    /// Push a clip rect, creating compositing layer if needed.
    pub fn push_clip_rect<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        clip_rect: Rect,
        painter: F,
    ) where
        F: FnOnce(&mut PaintContext),
    {
        if needs_compositing {
            // Create ClipRectLayer
            let layer = ClipRectLayer::new(clip_rect);
            self.layer_stack.push(layer.handle());
            
            let mut child_ctx = self.create_child_context(layer);
            painter(&mut child_ctx);
            
            self.layer_stack.pop();
        } else {
            // Apply clip directly to canvas
            self.canvas.save();
            self.canvas.clip_rect(clip_rect + offset);
            painter(self);
            self.canvas.restore();
        }
    }
    
    /// Push opacity effect.
    pub fn push_opacity<F>(&mut self, offset: Offset, alpha: u8, painter: F)
    where
        F: FnOnce(&mut PaintContext),
    {
        let layer = OpacityLayer::new(alpha, offset);
        // ...
    }
    
    /// Push transform.
    pub fn push_transform<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        transform: Mat4,
        painter: F,
    ) where
        F: FnOnce(&mut PaintContext),
    {
        // ...
    }
    
    /// Add a compositing layer directly.
    pub fn add_layer(&mut self, layer: impl Into<Layer>) {
        // ...
    }
    
    /// Mark painting as complex (hint to cache).
    pub fn set_complex_hint(&mut self) {
        self.is_complex = true;
    }
    
    /// Mark painting as changing (hint not to cache).
    pub fn set_will_change_hint(&mut self) {
        self.will_change = true;
    }
}
```

## 2. HitTestResult

Base class for recording hit test results with transform tracking.

### Flutter API

```dart
class HitTestResult {
  /// Unmodifiable list of hit entries in order
  Iterable<HitTestEntry> get path;
  
  /// Add an entry to the hit path
  void add(HitTestEntry entry);
  
  /// Push transform for subsequent entries
  void pushTransform(Matrix4 transform);
  void pushOffset(Offset offset);
  
  /// Pop most recent transform
  void popTransform();
}

class HitTestEntry<T extends HitTestTarget> {
  final T target;
  Matrix4 get transform;  // Global to local transform
}
```

### Rust Adaptation

```rust
/// Result of a hit test, tracking all targets hit and their transforms.
pub struct HitTestResult {
    /// Path of hit entries
    path: Vec<HitTestEntry>,
    
    /// Stack of transforms (global → local)
    transform_stack: Vec<Mat4>,
    
    /// Current combined transform
    current_transform: Mat4,
}

/// Entry in hit test result path.
pub struct HitTestEntry {
    /// Target that was hit
    pub target: RenderNodeId,
    
    /// Transform from global to local coordinates
    pub transform: Mat4,
}

impl HitTestResult {
    pub fn new() -> Self {
        Self {
            path: Vec::new(),
            transform_stack: Vec::new(),
            current_transform: Mat4::IDENTITY,
        }
    }
    
    /// Add a hit target to the path.
    pub fn add(&mut self, target: RenderNodeId) {
        self.path.push(HitTestEntry {
            target,
            transform: self.current_transform,
        });
    }
    
    /// Push a transform onto the stack.
    pub fn push_transform(&mut self, transform: Mat4) {
        self.transform_stack.push(self.current_transform);
        self.current_transform = self.current_transform * transform;
    }
    
    /// Push an offset transform.
    pub fn push_offset(&mut self, offset: Offset) {
        self.push_transform(Mat4::from_translation(offset.extend(0.0)));
    }
    
    /// Pop the most recent transform.
    pub fn pop_transform(&mut self) {
        if let Some(prev) = self.transform_stack.pop() {
            self.current_transform = prev;
        }
    }
    
    /// Get the hit path (immutable).
    pub fn path(&self) -> &[HitTestEntry] {
        &self.path
    }
    
    /// Check if any targets were hit.
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}
```

## 3. BoxHitTestResult

Specialized hit test result for box layout with coordinate transformations.

### Flutter API

```dart
class BoxHitTestResult extends HitTestResult {
  /// Test child with paint offset applied
  bool addWithPaintOffset({
    Offset? offset,
    required Offset position,
    required BoxHitTest hitTest,
  });
  
  /// Test child with full transform matrix
  bool addWithPaintTransform({
    Matrix4? transform,
    required Offset position,
    required BoxHitTest hitTest,
  });
  
  /// Test child with raw transform (no inversion)
  bool addWithRawTransform({
    Matrix4? transform,
    required Offset position,
    required BoxHitTest hitTest,
  });
  
  /// Test child with manual position management
  bool addWithOutOfBandPosition({
    Offset? paintOffset,
    Matrix4? paintTransform,
    Matrix4? rawTransform,
    required BoxHitTestWithOutOfBandPosition hitTest,
  });
}

typedef BoxHitTest = bool Function(BoxHitTestResult result, Offset position);
```

### Rust Adaptation

```rust
/// Hit test result specialized for box layout.
pub struct BoxHitTestResult {
    inner: HitTestResult,
}

impl BoxHitTestResult {
    pub fn new() -> Self {
        Self { inner: HitTestResult::new() }
    }
    
    /// Wrap an existing result.
    pub fn wrap(result: HitTestResult) -> Self {
        Self { inner: result }
    }
    
    /// Test child with paint offset applied.
    /// 
    /// Transforms `position` by subtracting `offset` before passing to child.
    pub fn add_with_paint_offset<F>(
        &mut self,
        offset: Option<Offset>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let offset = offset.unwrap_or(Offset::ZERO);
        let transformed_position = position - offset;
        
        if offset != Offset::ZERO {
            self.inner.push_offset(offset);
        }
        
        let hit = hit_test(self, transformed_position);
        
        if offset != Offset::ZERO {
            self.inner.pop_transform();
        }
        
        hit
    }
    
    /// Test child with paint transform applied.
    /// 
    /// Inverts transform to convert position to child's coordinate space.
    pub fn add_with_paint_transform<F>(
        &mut self,
        transform: Option<Mat4>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let Some(transform) = transform else {
            return hit_test(self, position);
        };
        
        // Invert transform to go from parent to child coordinates
        let Some(inverse) = transform.try_inverse() else {
            // Non-invertible transform means child is not visible
            return false;
        };
        
        let transformed = inverse.transform_point3(position.extend(0.0));
        let child_position = Offset::new(transformed.x, transformed.y);
        
        self.inner.push_transform(transform);
        let hit = hit_test(self, child_position);
        self.inner.pop_transform();
        
        hit
    }
    
    /// Test child with raw transform (no inversion needed).
    pub fn add_with_raw_transform<F>(
        &mut self,
        transform: Option<Mat4>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let Some(transform) = transform else {
            return hit_test(self, position);
        };
        
        // Position already in child's coordinate space
        self.inner.push_transform(transform);
        let hit = hit_test(self, position);
        self.inner.pop_transform();
        
        hit
    }
    
    /// Add entry to the path.
    pub fn add(&mut self, target: RenderNodeId) {
        self.inner.add(target);
    }
    
    /// Get underlying result.
    pub fn into_inner(self) -> HitTestResult {
        self.inner
    }
}
```

## 4. SliverHitTestResult

Specialized hit test result for sliver layout with axis-relative coordinates.

### Flutter API

```dart
class SliverHitTestResult extends HitTestResult {
  /// Test child with axis-relative offsets
  bool addWithAxisOffset({
    required double paintOffset,
    required double mainAxisOffset,
    required double crossAxisOffset,
    required double mainAxisPosition,
    required double crossAxisPosition,
    required SliverHitTest hitTest,
  });
}

typedef SliverHitTest = bool Function(
  SliverHitTestResult result,
  double mainAxisPosition,
  double crossAxisPosition,
);
```

### Rust Adaptation

```rust
/// Hit test result specialized for sliver layout.
pub struct SliverHitTestResult {
    inner: HitTestResult,
}

impl SliverHitTestResult {
    pub fn new() -> Self {
        Self { inner: HitTestResult::new() }
    }
    
    pub fn wrap(result: HitTestResult) -> Self {
        Self { inner: result }
    }
    
    /// Test child with axis-relative offset transformation.
    pub fn add_with_axis_offset<F>(
        &mut self,
        paint_offset: f64,
        main_axis_offset: f64,
        cross_axis_offset: f64,
        main_axis_position: f64,
        cross_axis_position: f64,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut SliverHitTestResult, f64, f64) -> bool,
    {
        let transformed_main = main_axis_position - main_axis_offset;
        let transformed_cross = cross_axis_position - cross_axis_offset;
        
        // Push offset transform (axis direction depends on scroll direction)
        // This is simplified - full impl needs scroll direction
        self.inner.push_offset(Offset::new(cross_axis_offset, main_axis_offset));
        
        let hit = hit_test(self, transformed_main, transformed_cross);
        
        self.inner.pop_transform();
        
        hit
    }
    
    pub fn add(&mut self, target: RenderNodeId) {
        self.inner.add(target);
    }
    
    pub fn into_inner(self) -> HitTestResult {
        self.inner
    }
}
```

## 5. Layout - No Context Needed

Flutter does NOT use a separate `LayoutContext` - constraints are passed directly
as method parameters and stored in the RenderObject itself.

### Flutter Model

```dart
// Constraints stored in object, passed as parameter
abstract class RenderObject {
  Constraints get constraints => _constraints!;
  Constraints? _constraints;
  
  void layout(Constraints constraints, {bool parentUsesSize = false}) {
    _constraints = constraints;
    performLayout();
  }
  
  // Override in subclass
  void performLayout();
}

class RenderBox extends RenderObject {
  @override
  BoxConstraints get constraints => super.constraints as BoxConstraints;
  
  Size get size => _size!;
  
  @override
  void performLayout() {
    // Use this.constraints directly
    child?.layout(constraints.loosen(), parentUsesSize: true);
    size = constraints.constrain(child!.size);
  }
}
```

### Rust Approach - Direct Parameters via Protocol

Instead of creating wrapper contexts, use Protocol associated types directly:

```rust
impl<P: Protocol, A: Arity> RenderNode<P, A, Attached> {
    /// Layout with constraints from Protocol.
    /// No LayoutContext wrapper needed.
    pub fn layout(mut self, constraints: P::Constraints) -> RenderNode<P, A, LaidOut> {
        self.constraints = Some(constraints.clone());
        let geometry = self.perform_layout(&constraints);
        // ... transition to LaidOut
    }
}

impl<P: Protocol, A: Arity> RenderNode<P, A, LaidOut> {
    /// Hit test with position/result from Protocol.
    /// No HitTestContext wrapper needed.
    pub fn hit_test(
        &self,
        result: &mut P::HitTestResult,
        position: P::HitTestPosition,
    ) -> bool {
        self.perform_hit_test(result, position)
    }
}
```

### Why No Context Wrappers?

1. **Flutter doesn't use them** - constraints are stored in object
2. **Protocol provides types** - `P::Constraints`, `P::HitTestResult`, `P::HitTestPosition`
3. **Simpler API** - fewer indirections
4. **Type safety** - Protocol ensures Box gets BoxConstraints, Sliver gets SliverConstraints

## Integration with RenderNode

How contexts integrate with the RenderNode typestate system:

```rust
impl<P: Protocol, A: Arity> RenderNode<P, A, Attached> {
    /// Perform layout with constraints directly (no wrapper context).
    pub fn layout(self, constraints: P::Constraints) -> RenderNode<P, A, LaidOut> {
        // Store constraints
        // Perform layout algorithm
        // Compute geometry
        // Return new state
    }
}

impl<P: Protocol, A: Arity> RenderNode<P, A, LaidOut> {
    /// Paint with PaintingContext (the only real context needed).
    pub fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Paint self
        // Paint children via ctx.paint_child()
    }
    
    /// Hit test with protocol-specific types.
    pub fn hit_test(
        &self,
        result: &mut P::HitTestResult,
        position: P::HitTestPosition,
    ) -> bool {
        // Test self
        // Test children with coordinate transforms
    }
}
```

## Summary

| Phase | Context Object | Why |
|-------|---------------|-----|
| Layout | None (direct params) | Constraints stored in node |
| Paint | `PaintingContext` | Canvas ownership, layer management |
| Hit Test | `*HitTestResult` | Transform stack, path recording |

### Key Patterns

1. **PaintingContext owns Canvas**: Canvas may change during child painting - needs context
2. **HitTestResult tracks transforms**: Stack-based transform management
3. **Layout has no context**: Constraints passed directly, stored in node
4. **Protocol provides types**: `P::Constraints`, `P::HitTestResult`, `P::HitTestPosition`
