# Animation Support in RenderObject

This document describes how RenderObject must support animations. Note that the animation system itself (Animation, AnimationController, Tween, etc.) lives in a separate crate. This document focuses on what the rendering layer needs to provide.

## Overview

RenderObjects don't drive animations themselves - they **react** to animation value changes. The widget layer calls setters on RenderObjects via `updateRenderObject()`, and those setters call `markNeedsPaint()` or `markNeedsLayout()` to trigger visual updates.

```
Widget Layer                    Rendering Layer (flui_rendering)
        │                                    │
        │  animation.value changes           │
        │  widget rebuilds                   │
        │                                    │
        │  updateRenderObject(render)        │
        │ ─────────────────────────────────► │
        │                                    │
        │  render.set_opacity(new_value)     │
        │ ─────────────────────────────────► │
        │                                    │ mark_needs_paint()
        │                                    │      │
        │                                    │      ▼
        │                                    │ PipelineOwner schedules repaint
        │                                    │      │
        │                                    │      ▼
        │                                    │ paint() called with new value
```

## Key Patterns

### 1. Animation-Aware Property Setters

When a property can be animated, its setter must:
1. Check if the value actually changed (avoid unnecessary work)
2. Call `markNeedsPaint()` if only visual appearance changes
3. Call `markNeedsLayout()` if the change affects layout

```rust
// Rust example
impl RenderOpacity {
    pub fn set_opacity(&mut self, value: f32) {
        if (self.opacity - value).abs() < f32::EPSILON {
            return;  // No change, skip update
        }
        self.opacity = value;
        
        // Opacity change doesn't affect layout, only paint
        self.mark_needs_paint();
        
        // Also update composited layer if using layer-based optimization
        self.update_composited_layer(needs_compositing: true);
    }
}
```

### 2. RenderAnimatedOpacity Pattern

Flutter provides `RenderAnimatedOpacity` which accepts `Animation<double>` instead of a static opacity value. This pattern:
- Stores the animation reference
- Listens to animation changes via attach/detach lifecycle
- Automatically calls `markNeedsPaint()` when animation value changes

```dart
// Flutter pattern (Dart)
class RenderAnimatedOpacity extends RenderProxyBox {
  Animation<double> _opacity;
  
  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    _opacity.addListener(markNeedsPaint);  // Listen to animation
  }
  
  @override
  void detach() {
    _opacity.removeListener(markNeedsPaint);  // Stop listening
    super.detach();
  }
  
  @override
  void paint(PaintingContext context, Offset offset) {
    // Use _opacity.value during paint
    context.pushOpacity(offset, (_opacity.value * 255).round(), super.paint);
  }
}
```

### 3. Rust Translation

In Rust, we can use callbacks or channels instead of Dart's listener pattern:

```rust
/// Render object that applies animated opacity to its child.
pub struct RenderAnimatedOpacity<Child> {
    child: Child,
    opacity: f32,  // Current opacity value (0.0 - 1.0)
    
    // Optional: callback to unsubscribe from animation
    animation_subscription: Option<Box<dyn FnOnce()>>,
    
    // Layer handle for opacity compositing
    opacity_layer: Option<OpacityLayerHandle>,
    
    // Whether to include child semantics even when fully transparent
    always_include_semantics: bool,
}

impl<Child> RenderAnimatedOpacity<Child> {
    /// Called by animation system when value changes.
    pub fn update_opacity(&mut self, value: f32) {
        if (self.opacity - value).abs() < f32::EPSILON {
            return;
        }
        
        let was_visible = self.opacity > 0.0;
        let is_visible = value > 0.0;
        
        self.opacity = value;
        
        // Mark for repaint
        self.mark_needs_paint();
        
        // If visibility changed, may need semantics update
        if was_visible != is_visible && !self.always_include_semantics {
            self.mark_needs_semantics_update();
        }
    }
}
```

## Layer-Based Optimization

### Why Use Layers for Animation?

Without layers, an opacity change requires:
1. Repaint the parent
2. Repaint this object
3. Repaint all children

With an OpacityLayer:
1. Only update the layer's opacity property
2. GPU composites the cached child texture with new opacity

This is **critical** for 60fps animations.

### Composited Layers for Animation

RenderObjects that support animation should use composited layers:

| Animated Property | Layer Type | Purpose |
|-------------------|------------|---------|
| Opacity | OpacityLayer | Avoid repainting subtree |
| Transform | TransformLayer | Hardware-accelerated transforms |
| Clip | ClipRectLayer, ClipPathLayer | Efficient clip updates |

```rust
impl RenderAnimatedOpacity {
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.opacity == 0.0 {
            return;  // Fully transparent, skip paint entirely
        }
        
        if self.opacity == 1.0 {
            // Fully opaque, no layer needed
            self.child.paint(context, offset);
            return;
        }
        
        // Use OpacityLayer for partial transparency
        context.push_opacity(
            offset,
            (self.opacity * 255.0) as u8,
            |ctx, off| self.child.paint(ctx, off),
        );
    }
}
```

## Transform Animation Support

### RenderTransform Requirements

The `RenderTransform` object must support animated transforms:

```rust
pub struct RenderTransform<Child> {
    child: Child,
    transform: Matrix4,  // 4x4 transformation matrix
    origin: Option<Offset>,  // Transform origin (local coordinates)
    alignment: Option<Alignment>,  // Alignment-based origin
    transform_hit_tests: bool,  // Whether hit testing respects transform
}

impl<Child> RenderTransform<Child> {
    /// Set the transform matrix. Called by widget layer via updateRenderObject.
    pub fn set_transform(&mut self, matrix: Matrix4) {
        // Note: no getter because Matrix4 is mutable and we can't track external changes
        self.transform = matrix;
        self.mark_needs_paint();
    }
    
    /// Convenience methods for incremental modifications
    pub fn rotate_z(&mut self, radians: f32) {
        self.transform = self.transform * Matrix4::rotation_z(radians);
        self.mark_needs_paint();
    }
    
    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        self.transform = self.transform * Matrix4::scale(x, y, z);
        self.mark_needs_paint();
    }
    
    pub fn translate(&mut self, offset: Vec3) {
        self.transform = self.transform * Matrix4::translation(offset);
        self.mark_needs_paint();
    }
    
    pub fn set_identity(&mut self) {
        self.transform = Matrix4::IDENTITY;
        self.mark_needs_paint();
    }
}
```

### Transform and Hit Testing

When `transform_hit_tests` is true, the inverse transform must be applied to hit test positions:

```rust
impl<Child> RenderTransform<Child> {
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if !self.transform_hit_tests {
            return self.child.hit_test(result, position);
        }
        
        // Transform position into child's coordinate space
        let inverse = self.transform.try_inverse()?;
        let transformed_position = inverse.transform_point(position);
        
        self.child.hit_test(result, transformed_position)
    }
}
```

## Repaint Boundary for Animations

### When to Use isRepaintBoundary

A RenderObject should set `is_repaint_boundary = true` when:
1. It wraps frequently animated content
2. The animation only affects this subtree
3. The subtree is complex enough that caching is worthwhile

```rust
impl RenderAnimatedOpacity {
    fn is_repaint_boundary(&self) -> bool {
        // Always create a repaint boundary for animated opacity
        // This allows opacity changes without repainting siblings
        true
    }
}
```

### Trade-offs

| With Repaint Boundary | Without Repaint Boundary |
|----------------------|-------------------------|
| Extra layer memory overhead | No extra memory |
| Fast subtree updates | Entire tree repaints |
| Better for complex children | Better for simple children |
| GPU compositing | CPU compositing |

## Attach/Detach Lifecycle for Animations

RenderObjects must properly manage animation subscriptions:

```rust
impl RenderAnimatedOpacity {
    fn attach(&mut self, owner: &PipelineOwner) {
        // Register with pipeline owner first
        self.base.attach(owner);
        
        // Then start listening to animation
        if let Some(animation) = &self.animation {
            animation.add_listener(|| self.mark_needs_paint());
        }
    }
    
    fn detach(&mut self) {
        // Stop listening to animation first
        if let Some(animation) = &self.animation {
            animation.remove_listener();
        }
        
        // Then detach from pipeline
        self.base.detach();
    }
}
```

## Semantics and Animation

### alwaysIncludeSemantics

When an object is fully transparent (opacity = 0), should it still be accessible?

```rust
pub struct RenderAnimatedOpacity {
    // When true, child semantics are included regardless of opacity
    // Critical for screen readers during fade animations
    always_include_semantics: bool,
}

impl RenderAnimatedOpacity {
    fn visit_children_for_semantics(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if self.opacity > 0.0 || self.always_include_semantics {
            visitor(&self.child);
        }
    }
}
```

## FLUI Implementation Requirements

### Required Traits/Methods

For animation support, RenderObject must provide:

```rust
pub trait RenderObject {
    /// Mark this object as needing repaint (visual change only)
    fn mark_needs_paint(&mut self);
    
    /// Mark this object as needing layout (size/position change)
    fn mark_needs_layout(&mut self);
    
    /// Mark this object as needing semantics update
    fn mark_needs_semantics_update(&mut self);
    
    /// Whether this object is a repaint boundary
    fn is_repaint_boundary(&self) -> bool { false }
    
    /// Lifecycle: called when attached to render tree
    fn attach(&mut self, owner: &PipelineOwner);
    
    /// Lifecycle: called when detached from render tree
    fn detach(&mut self);
}
```

### Animation-Ready RenderObjects

FLUI should provide these animation-aware render objects:

| RenderObject | Animated Property | Layer |
|--------------|-------------------|-------|
| `RenderAnimatedOpacity` | opacity: f32 | OpacityLayer |
| `RenderAnimatedTransform` | transform: Matrix4 | TransformLayer |
| `RenderAnimatedScale` | scale: f32 | TransformLayer |
| `RenderAnimatedRotation` | angle: f32 | TransformLayer |
| `RenderAnimatedSlide` | offset: Offset | TransformLayer |
| `RenderAnimatedClipRect` | clip: Rect | ClipRectLayer |

### Performance Guidelines

1. **Use layers for complex subtrees**: If child subtree is expensive to paint, use composited layers
2. **Avoid layout during animation**: Prefer transform/opacity which only need repaint
3. **Batch property updates**: If multiple properties change, update all before marking dirty
4. **Check for actual changes**: Always compare new value to old before marking dirty

## Source Reference

Based on analysis of:
- [RenderAnimatedOpacity class](https://api.flutter.dev/flutter/rendering/RenderAnimatedOpacity-class.html)
- [RenderAnimatedOpacityMixin mixin](https://api.flutter.dev/flutter/rendering/RenderAnimatedOpacityMixin-mixin.html)
- [RenderTransform class](https://api.flutter.dev/flutter/rendering/RenderTransform-class.html)
- [markNeedsPaint method](https://api.flutter.dev/flutter/rendering/RenderObject/markNeedsPaint.html)
