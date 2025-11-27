# Flutter Reference Map: Effects Objects

This document maps each FLUI effect object to its Flutter equivalent with links to source code and API documentation.

## Flutter Source Locations

**Base Path:** https://github.com/flutter/flutter/tree/master/packages/flutter/lib/src/rendering

**Key Files:**
- `proxy_box.dart` - Most effect objects (RenderProxyBox subclasses)
- `custom_paint.dart` - RenderCustomPaint
- `shifted_box.dart` - Some positioned effects

## Object Mapping

### 1. RenderOpacity

**FLUI:** `crates/flui_rendering/src/objects/effects/opacity.rs`
**Flutter:** `proxy_box.dart` - RenderOpacity (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderOpacity-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L1339

**Expected Behavior:**
```dart
class RenderOpacity extends RenderProxyBox {
  // Layout: Pass-through (inherited from RenderProxyBox)

  // Paint: Three paths
  // 1. opacity == 0: Don't paint child
  // 2. opacity == 1: Paint child directly (no layer)
  // 3. 0 < opacity < 1: Use OpacityLayer

  @override
  void paint(PaintingContext context, Offset offset) {
    if (child == null) return;
    if (_alpha == 0) return; // Fast path
    if (_alpha == 255) { // Fast path
      context.paintChild(child!, offset);
      return;
    }
    // Use layer for partial opacity
    context.pushOpacity(offset, _alpha, super.paint);
  }

  @override
  bool get alwaysNeedsCompositing => _alpha > 0 && _alpha < 255;
}
```

**FLUI Status:** ✅ Needs verification

---

### 2. RenderTransform

**FLUI:** `crates/flui_rendering/src/objects/effects/transform.rs`
**Flutter:** `proxy_box.dart` - RenderTransform (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderTransform-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L1806

**Expected Behavior:**
```dart
class RenderTransform extends RenderProxyBox {
  // Layout: Pass-through (inherited from RenderProxyBox)

  // Paint: Apply transform
  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      final Matrix4 transform = _effectiveTransform;
      final Offset? childOffset = MatrixUtils.getAsTranslation(transform);
      if (childOffset == null) {
        // Use layer for non-translation transforms
        layer = context.pushTransform(
          needsCompositing,
          offset,
          transform,
          super.paint,
          oldLayer: layer as TransformLayer?,
        );
      } else {
        // Optimization for pure translation
        super.paint(context, offset + childOffset);
      }
    }
  }

  // Hit testing uses inverse transform
  @override
  bool hitTest(BoxHitTestResult result, { required Offset position }) {
    return hitTestChildren(result, position: position);
  }

  @override
  bool hitTestChildren(BoxHitTestResult result, { required Offset position }) {
    final Matrix4? inverse = Matrix4.tryInvert(getTransformTo(null));
    if (inverse == null) return false;
    final Offset transformed = MatrixUtils.transformPoint(inverse, position);
    return super.hitTestChildren(result, position: transformed);
  }
}
```

**FLUI Status:** ✅ Needs verification

---

### 3. RenderAnimatedOpacity

**FLUI:** `crates/flui_rendering/src/objects/effects/animated_opacity.rs`
**Flutter:** `proxy_box.dart` - RenderAnimatedOpacity (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderAnimatedOpacity-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L1530

**Expected Behavior:**
```dart
class RenderAnimatedOpacity extends RenderProxyBox {
  // Similar to RenderOpacity but:
  // - alwaysNeedsCompositing can be true even at opacity 1.0 during animation
  // - This avoids layer creation/destruction during animation

  @override
  bool get alwaysNeedsCompositing =>
    (child != null && _alpha > 0 && _alpha < 255) ||
    (_currentlyNeedsCompositing && _opacity.isAnimating);

  // Paint: Same as RenderOpacity
}
```

**FLUI Status:** ✅ Needs verification

---

### 4. RenderClipRect

**FLUI:** `crates/flui_rendering/src/objects/effects/clip_rect.rs`
**Flutter:** `proxy_box.dart` - RenderClipRect (extends _RenderCustomClip)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderClipRect-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L878

**Expected Behavior:**
```dart
class RenderClipRect extends _RenderCustomClip<Rect> {
  // Layout: Pass-through

  // Paint: Clip to rect
  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      if (clipBehavior != Clip.none) {
        layer = context.pushClipRect(
          needsCompositing,
          offset,
          _clip!,
          super.paint,
          clipBehavior: clipBehavior,
          oldLayer: layer as ClipRectLayer?,
        );
      } else {
        context.paintChild(child!, offset);
      }
    }
  }

  @override
  Rect get _defaultClip => Offset.zero & size;
}
```

**FLUI Status:** ✅ Needs verification

---

### 5. RenderClipRRect

**FLUI:** `crates/flui_rendering/src/objects/effects/clip_rrect.rs`
**Flutter:** `proxy_box.dart` - RenderClipRRect (extends _RenderCustomClip)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderClipRRect-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L957

**Expected Behavior:**
```dart
class RenderClipRRect extends _RenderCustomClip<RRect> {
  // Similar to RenderClipRect but with rounded corners

  @override
  RRect get _defaultClip => borderRadius.toRRect(Offset.zero & size);

  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      if (clipBehavior != Clip.none) {
        layer = context.pushClipRRect(
          needsCompositing,
          offset,
          _clip!.outerRect,
          _clip!,
          super.paint,
          clipBehavior: clipBehavior,
          oldLayer: layer as ClipRRectLayer?,
        );
      } else {
        context.paintChild(child!, offset);
      }
    }
  }
}
```

**FLUI Status:** ✅ Needs verification

---

### 6. RenderClipOval

**FLUI:** `crates/flui_rendering/src/objects/effects/clip_oval.rs`
**Flutter:** `proxy_box.dart` - RenderClipOval (extends _RenderCustomClip)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderClipOval-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L1023

**Expected Behavior:**
```dart
class RenderClipOval extends _RenderCustomClip<Rect> {
  @override
  Rect get _defaultClip => Offset.zero & size;

  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      if (clipBehavior != Clip.none) {
        layer = context.pushClipPath(
          needsCompositing,
          offset,
          _clip!,
          _getClipPath(),
          super.paint,
          clipBehavior: clipBehavior,
          oldLayer: layer as ClipPathLayer?,
        );
      } else {
        context.paintChild(child!, offset);
      }
    }
  }

  Path _getClipPath() {
    final Path path = Path()..addOval(_clip!);
    return path;
  }
}
```

**FLUI Status:** ✅ Needs verification

---

### 7. RenderClipPath

**FLUI:** `crates/flui_rendering/src/objects/effects/clip_path.rs`
**Flutter:** `proxy_box.dart` - RenderClipPath (extends _RenderCustomClip)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderClipPath-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L1095

**Expected Behavior:**
```dart
class RenderClipPath extends _RenderCustomClip<Path> {
  // Uses custom Clipper delegate

  @override
  Path get _defaultClip {
    final Path path = Path();
    path.addRect(Offset.zero & size);
    return path;
  }

  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      if (clipBehavior != Clip.none) {
        layer = context.pushClipPath(
          needsCompositing,
          offset,
          Offset.zero & size,
          _clip!,
          super.paint,
          clipBehavior: clipBehavior,
          oldLayer: layer as ClipPathLayer?,
        );
      } else {
        context.paintChild(child!, offset);
      }
    }
  }
}
```

**FLUI Status:** ✅ Needs verification

---

### 8. RenderDecoratedBox

**FLUI:** `crates/flui_rendering/src/objects/effects/decorated_box.rs`
**Flutter:** `proxy_box.dart` - RenderDecoratedBox (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderDecoratedBox-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L468

**Expected Behavior:**
```dart
class RenderDecoratedBox extends RenderProxyBox {
  // Layout: Pass-through

  // Paint: Decoration before or after child based on position
  @override
  void paint(PaintingContext context, Offset offset) {
    if (_decoration != null) {
      if (_position == DecorationPosition.background) {
        // Paint decoration first
        _painter ??= _decoration!.createBoxPainter(markNeedsPaint);
        _painter!.paint(context.canvas, offset, configuration);
      }
    }
    super.paint(context, offset); // Paint child
    if (_decoration != null) {
      if (_position == DecorationPosition.foreground) {
        // Paint decoration last
        _painter ??= _decoration!.createBoxPainter(markNeedsPaint);
        _painter!.paint(context.canvas, offset, configuration);
      }
    }
  }
}
```

**FLUI Status:** ✅ Needs verification

---

### 9. RenderPhysicalModel

**FLUI:** `crates/flui_rendering/src/objects/effects/physical_model.rs`
**Flutter:** `proxy_box.dart` - RenderPhysicalModel (extends _RenderPhysicalModelBase)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderPhysicalModel-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L2242

**Expected Behavior:**
```dart
class RenderPhysicalModel extends _RenderPhysicalModelBase<RRect> {
  // Paint: Draws shadow, clips to shape

  @override
  void paint(PaintingContext context, Offset offset) {
    if (child == null) return;

    final RRect offsetRRect = _clipRRect.shift(offset);
    final Rect offsetBounds = offsetRRect.outerRect;

    // Draw shadow
    final Path path = Path()..addRRect(offsetRRect);
    final Paint paint = Paint()
      ..color = shadowColor
      ..maskFilter = MaskFilter.blur(BlurStyle.normal, elevation);
    context.canvas.drawPath(path, paint);

    // Clip and paint child
    final bool needsCompositing = alwaysNeedsCompositing || elevation != 0.0;
    layer = context.pushClipRRect(
      needsCompositing,
      offset,
      offsetBounds,
      offsetRRect,
      super.paint,
      clipBehavior: clipBehavior,
      oldLayer: layer as ClipRRectLayer?,
    );
  }
}
```

**FLUI Status:** ✅ Needs verification

---

### 10. RenderPhysicalShape

**FLUI:** `crates/flui_rendering/src/objects/effects/physical_shape.rs`
**Flutter:** `proxy_box.dart` - RenderPhysicalShape (extends _RenderPhysicalModelBase)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderPhysicalShape-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L2356

**Expected Behavior:**
```dart
class RenderPhysicalShape extends _RenderPhysicalModelBase<Path> {
  // Similar to RenderPhysicalModel but with custom path
  // Uses Clipper delegate
}
```

**FLUI Status:** ✅ Needs verification

---

### 11. RenderOffstage

**FLUI:** `crates/flui_rendering/src/objects/effects/offstage.rs`
**Flutter:** `proxy_box.dart` - RenderOffstage (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderOffstage-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L3367

**Expected Behavior:**
```dart
class RenderOffstage extends RenderProxyBox {
  // Layout: Always layout child (even if offstage)
  @override
  void performLayout() {
    if (child != null) {
      child!.layout(constraints, parentUsesSize: !offstage);
      size = offstage ? Size.zero : child!.size;
    } else {
      size = constraints.smallest;
    }
  }

  // Paint: Don't paint if offstage
  @override
  void paint(PaintingContext context, Offset offset) {
    if (!offstage && child != null) {
      super.paint(context, offset);
    }
  }

  // Hit test: Don't hit test if offstage
  @override
  bool hitTest(BoxHitTestResult result, { required Offset position }) {
    return !offstage && super.hitTest(result, position: position);
  }
}
```

**FLUI Status:** ⚠️ IMPORTANT - Has custom layout, NOT a pure proxy!

---

### 12. RenderRepaintBoundary

**FLUI:** `crates/flui_rendering/src/objects/effects/repaint_boundary.rs`
**Flutter:** `proxy_box.dart` - RenderRepaintBoundary (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderRepaintBoundary-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L679

**Expected Behavior:**
```dart
class RenderRepaintBoundary extends RenderProxyBox {
  // Layout: Pass-through

  @override
  bool get isRepaintBoundary => true;

  // Paint: Creates compositing layer automatically
  // (handled by PaintingContext when isRepaintBoundary is true)
}
```

**FLUI Status:** ✅ Needs verification

---

### 13. RenderShaderMask

**FLUI:** `crates/flui_rendering/src/objects/effects/shader_mask.rs`
**Flutter:** `proxy_box.dart` - RenderShaderMask (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderShaderMask-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L2731

**Expected Behavior:**
```dart
class RenderShaderMask extends RenderProxyBox {
  // Layout: Pass-through

  // Paint: Apply shader with blend mode
  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      layer = context.pushShaderMask(
        offset,
        Offset.zero & size,
        shaderCallback,
        super.paint,
        blendMode,
        oldLayer: layer as ShaderMaskLayer?,
      );
    }
  }

  @override
  bool get alwaysNeedsCompositing => child != null;
}
```

**FLUI Status:** ✅ Needs verification

---

### 14. RenderBackdropFilter

**FLUI:** `crates/flui_rendering/src/objects/effects/backdrop_filter.rs`
**Flutter:** `proxy_box.dart` - RenderBackdropFilter (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderBackdropFilter-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L2824

**Expected Behavior:**
```dart
class RenderBackdropFilter extends RenderProxyBox {
  // Layout: Pass-through

  // Paint: Apply backdrop filter
  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      layer = context.pushBackdropFilter(
        offset,
        filter,
        super.paint,
        oldLayer: layer as BackdropFilterLayer?,
      );
    }
  }

  @override
  bool get alwaysNeedsCompositing => child != null;
}
```

**FLUI Status:** ✅ Needs verification

---

### 15. RenderCustomPaint

**FLUI:** `crates/flui_rendering/src/objects/effects/custom_paint.rs`
**Flutter:** `custom_paint.dart` - RenderCustomPaint (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderCustomPaint-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/custom_paint.dart#L121

**Expected Behavior:**
```dart
class RenderCustomPaint extends RenderProxyBox {
  // Layout: Can have custom layout if no child
  @override
  void performLayout() {
    if (child != null) {
      child!.layout(constraints, parentUsesSize: true);
      size = child!.size;
    } else {
      size = constraints.constrain(preferredSize ?? Size.zero);
    }
  }

  // Paint: Background painter, child, foreground painter
  @override
  void paint(PaintingContext context, Offset offset) {
    // Paint background
    if (painter != null) {
      _paintWithPainter(context.canvas, offset, painter!);
    }
    // Paint child
    super.paint(context, offset);
    // Paint foreground
    if (foregroundPainter != null) {
      _paintWithPainter(context.canvas, offset, foregroundPainter!);
    }
  }
}
```

**FLUI Status:** ⚠️ IMPORTANT - Has custom layout when no child!

---

### 16. RenderAnimatedSize

**FLUI:** `crates/flui_rendering/src/objects/effects/animated_size.rs`
**Flutter:** `proxy_box.dart` - RenderAnimatedSize (extends RenderProxyBox)

**Flutter API:** https://api.flutter.dev/flutter/rendering/RenderAnimatedSize-class.html
**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L3567

**Expected Behavior:**
```dart
class RenderAnimatedSize extends RenderProxyBox {
  // Layout: Custom - animates between sizes
  @override
  void performLayout() {
    if (child == null || constraints.isTight) {
      _hasVisualOverflow = false;
      size = _sizeTween.end = constraints.smallest;
      child?.layout(constraints);
      return;
    }

    child!.layout(constraints, parentUsesSize: true);

    // Animate size
    final Size childSize = child!.size;
    if (_sizeTween.end != childSize) {
      _sizeTween.begin = size;
      _sizeTween.end = childSize;
      _controller.forward(from: 0.0);
    }

    size = constraints.constrain(_sizeTween.evaluate(_animation));
    _hasVisualOverflow = size.width < childSize.width || size.height < childSize.height;
  }

  // Paint: Clip if has overflow
  @override
  void paint(PaintingContext context, Offset offset) {
    if (child != null && _hasVisualOverflow && clipBehavior != Clip.none) {
      final Rect rect = Offset.zero & size;
      layer = context.pushClipRect(
        needsCompositing,
        offset,
        rect,
        super.paint,
        clipBehavior: clipBehavior,
        oldLayer: layer as ClipRectLayer?,
      );
    } else {
      super.paint(context, offset);
    }
  }
}
```

**FLUI Status:** ⚠️ CRITICAL - Complex custom layout with animation!

---

### 17. RenderVisibility (if exists)

**FLUI:** `crates/flui_rendering/src/objects/effects/visibility.rs` (?)
**Flutter:** No direct equivalent - Visibility is a widget that uses RenderOffstage

**Note:** Need to check if this exists in FLUI or if it's the same as RenderOffstage.

---

### 18. Clip Base (_RenderCustomClip)

**FLUI:** `crates/flui_rendering/src/objects/effects/clip_base.rs`
**Flutter:** `proxy_box.dart` - _RenderCustomClip (abstract base for clip objects)

**Flutter Source:** https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart#L763

**Expected Behavior:**
```dart
abstract class _RenderCustomClip<T> extends RenderProxyBox {
  // Common clip behavior
  // - Manages clipper delegate
  // - Handles clipBehavior
  // - Manages clip path/rect/rrect calculation

  T? get _defaultClip;

  @override
  void paint(PaintingContext context, Offset offset) {
    // Implemented by subclasses (ClipRect, ClipRRect, etc.)
  }
}
```

**FLUI Status:** ✅ Needs verification

---

## Critical Findings

### Objects with Custom Layout (NOT pure proxies):

1. **RenderOffstage** - Custom layout (size = Size.zero when offstage)
2. **RenderCustomPaint** - Custom layout when no child
3. **RenderAnimatedSize** - Complex custom layout with animation

These should **NOT** use simple proxy pattern!

### Pure Proxy Objects (should use RenderBox<Single>):

All others are pure proxies - layout passes through, only paint/hit-test differs.

## Next Steps

1. ✅ Verify each object uses correct base trait
2. ✅ Check layout implementations match Flutter
3. ✅ Check paint implementations match Flutter
4. ✅ Add tests for edge cases
