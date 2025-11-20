# Flutter RenderObject Optional Child Analysis

## Research Date: 2025-01-18

### Question
Should FLUI's single-child render objects use `Render<Single>` or `Render<Optional>`?

### Flutter Analysis

#### Constructor Signatures (from api.flutter.dev)

All single-child RenderBox classes in Flutter have **optional child**:

```dart
RenderProxyBox([RenderBox? child])
RenderPadding({required EdgeInsetsGeometry padding, RenderBox? child})
RenderOpacity({double opacity = 1.0, RenderBox? child})
RenderTransform({required Matrix4 transform, RenderBox? child})
RenderConstrainedBox({required BoxConstraints additionalConstraints, RenderBox? child})
RenderLimitedBox({double maxWidth = double.infinity, double maxHeight = double.infinity, RenderBox? child})
RenderPositionedBox({RenderBox? child, AlignmentGeometry alignment = Alignment.center})
```

#### Runtime Behavior

**RenderProxyBox.performLayout():**
```dart
void performLayout() {
  size = (child?..layout(constraints, parentUsesSize: true))?.size ?? 
          computeSizeForNoChild(constraints);
}
```

**RenderProxyBox.paint():**
```dart
void paint(PaintingContext context, Offset offset) {
  final RenderBox? child = this.child;
  if (child == null) {
    return;  // Gracefully handles null child
  }
  context.paintChild(child, offset);
}
```

### Why Optional?

1. **Lifecycle flexibility** - RenderObject created first, child attached later
2. **Detached state** - Child can be temporarily removed during rebuild/animation
3. **SizedBox use case** - `SizedBox(width: 10)` without child = spacer
4. **Graceful degradation** - RenderObject can render even without child

### FLUI Decision

**All 46 single-child render objects should migrate to `Render<Optional>`, NOT `Render<Single>`**

This matches Flutter's semantics exactly:
- ✅ Child can be null at construction
- ✅ Child can be null at runtime
- ✅ Layout/paint handle null child gracefully
- ✅ Enables spacer patterns (SizedBox, Container)

### Migration Plan

**Leaf (0 children) → `Render<Leaf>`:**
- RenderEmpty
- RenderImage
- RenderTexture
- RenderParagraph
- RenderEditableLine
- RenderPlaceholder
- RenderErrorBox

**Single-child (0-1 child) → `Render<Optional>`:**
- All 46 current "Single" render objects
- Use `ctx.children.get()` → `Option<ElementId>`
- Use `ctx.children.is_some()` for checks
- Use `ctx.children.map(|child| ...)` for conditional operations

**Multi-child (N children) → `Render<Variable>`:**
- RenderFlex
- RenderStack
- RenderWrap
- RenderGrid
- RenderTable
- etc. (13 total)

### References

- https://api.flutter.dev/flutter/rendering/RenderProxyBox-class.html
- https://api.flutter.dev/flutter/rendering/RenderPadding-class.html
- https://api.flutter.dev/flutter/rendering/RenderOpacity-class.html
- https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/rendering/proxy_box.dart
