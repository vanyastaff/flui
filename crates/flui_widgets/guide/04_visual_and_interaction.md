# 🎨 Visual Effects Widgets (Визуальные эффекты)

## Opacity
```
📦 Opacity
  └─ RenderOpacity
      └─ Child Widget (transparent)
```

**RenderObject:** `RenderOpacity`

**Параметры:**
- `opacity` - double (0.0 - 1.0)
- `alwaysIncludeSemantics` - сохранять semantics
- `child` - дочерний виджет

---

## Transform
```
📦 Transform
  └─ RenderTransform
      └─ Matrix4 transformation
          └─ Child Widget (transformed)
```

**RenderObject:** `RenderTransform`

**Параметры:**
- `transform` - Matrix4
- `origin` - Offset (центр трансформации)
- `alignment` - Alignment (центр трансформации)
- `transformHitTests` - трансформировать hit tests
- `filterQuality` - FilterQuality
- `child` - дочерний виджет

**Варианты:**
- `Transform()` - custom Matrix4
- `Transform.rotate()` - поворот
- `Transform.translate()` - смещение
- `Transform.scale()` - масштабирование

---

### Transform.rotate
```
📦 Transform.rotate
  └─ Matrix4 (rotation) -> RenderTransform
      └─ Child Widget (rotated)
```

**RenderObject:** `RenderTransform`

**Параметры:**
- `angle` - double (в радианах)
- `origin`, `alignment`, `transformHitTests`, `filterQuality`
- `child` - дочерний виджет

---

### Transform.translate
```
📦 Transform.translate
  └─ Matrix4 (translation) -> RenderTransform
      └─ Child Widget (offset)
```

**RenderObject:** `RenderTransform`

**Параметры:**
- `offset` - Offset
- `transformHitTests`, `filterQuality`
- `child` - дочерний виджет

---

### Transform.scale
```
📦 Transform.scale
  └─ Matrix4 (scale) -> RenderTransform
      └─ Child Widget (scaled)
```

**RenderObject:** `RenderTransform`

**Параметры:**
- `scale` - double (uniform scale)
- `scaleX`, `scaleY` - double (non-uniform)
- `origin`, `alignment`, `transformHitTests`, `filterQuality`
- `child` - дочерний виджет

---

## RotatedBox
```
📦 RotatedBox (90° increments only)
  └─ RenderRotatedBox
      └─ Child Widget (rotated 0/90/180/270°)
```

**RenderObject:** `RenderRotatedBox`

**Параметры:**
- `quarterTurns` - int (0, 1, 2, 3, ...)
- `child` - дочерний виджет

---

## ClipRect
```
📦 ClipRect (Rectangular clip)
  └─ RenderClipRect
      └─ Child Widget (clipped to bounds)
```

**RenderObject:** `RenderClipRect`

**Параметры:**
- `clipper` - CustomClipper<Rect> (optional)
- `clipBehavior` - Clip (hardEdge, antiAlias, antiAliasWithSaveLayer)
- `child` - дочерний виджет

---

## ClipRRect
```
📦 ClipRRect (Rounded rectangular clip)
  └─ RenderClipRRect
      └─ Child Widget (clipped with rounded corners)
```

**RenderObject:** `RenderClipRRect`

**Параметры:**
- `borderRadius` - BorderRadius
- `clipper` - CustomClipper<RRect> (optional)
- `clipBehavior` - Clip
- `child` - дочерний виджет

---

## ClipOval
```
📦 ClipOval (Oval/circular clip)
  └─ RenderClipOval
      └─ Child Widget (clipped to oval)
```

**RenderObject:** `RenderClipOval`

**Параметры:**
- `clipper` - CustomClipper<Rect> (optional)
- `clipBehavior` - Clip
- `child` - дочерний виджет

---

## ClipPath
```
📦 ClipPath (Custom path clip)
  └─ RenderClipPath
      └─ CustomClipper<Path>
          └─ Child Widget (clipped to custom path)
```

**RenderObject:** `RenderClipPath`

**Параметры:**
- `clipper` - CustomClipper<Path> (required)
- `clipBehavior` - Clip
- `child` - дочерний виджет

---

## BackdropFilter
```
📦 BackdropFilter (Blur/filter backdrop)
  └─ RenderBackdropFilter
      └─ ImageFilter
          └─ Child Widget (поверх filtered backdrop)
```

**RenderObject:** `RenderBackdropFilter`

**Параметры:**
- `filter` - ImageFilter (blur, matrix)
- `blendMode` - BlendMode
- `child` - дочерний виджет

---

## DecoratedBox
```
📦 DecoratedBox
  └─ RenderDecoratedBox
      └─ Decoration (background, border, shadow)
          └─ Child Widget
```

**RenderObject:** `RenderDecoratedBox`

**Параметры:**
- `decoration` - Decoration (BoxDecoration, ShapeDecoration, etc.)
- `position` - DecorationPosition (background, foreground)
- `child` - дочерний виджет

---

## ColorFiltered
```
📦 ColorFiltered (Color filter)
  └─ RenderColorFiltered
      └─ ColorFilter
          └─ Child Widget (with color filter)
```

**RenderObject:** `RenderColorFiltered`

**Параметры:**
- `colorFilter` - ColorFilter (mode, matrix, etc.)
- `child` - дочерний виджет

---

## ShaderMask
```
📦 ShaderMask (Gradient mask)
  └─ RenderShaderMask
      └─ Shader
          └─ Child Widget (masked by shader)
```

**RenderObject:** `RenderShaderMask`

**Параметры:**
- `shaderCallback` - Shader Function(Bounds)
- `blendMode` - BlendMode
- `child` - дочерний виджет

---

## RepaintBoundary
```
📦 RepaintBoundary (Isolate repaints)
  └─ RenderRepaintBoundary
      └─ Child Widget (в отдельном layer)
```

**RenderObject:** `RenderRepaintBoundary`

**Параметры:**
- `child` - дочерний виджет

**Применение:** Оптимизация - ребенок перерисовывается независимо

---

# 🖱️ Interaction Widgets (Интерактивность)

## GestureDetector
```
📦 GestureDetector
  └─ RenderPointerListener
      └─ Gesture Arena (recognizers)
          └─ Child Widget (interactive)
```

**RenderObject:** `RenderPointerListener` (если behavior != deferToChild) или `RenderProxyBox`

**Параметры (основные):**
- **Tap:** `onTap`, `onTapDown`, `onTapUp`, `onTapCancel`, `onDoubleTap`, `onLongPress`
- **Pan:** `onPanStart`, `onPanUpdate`, `onPanEnd`, `onPanCancel`
- **Scale:** `onScaleStart`, `onScaleUpdate`, `onScaleEnd`
- **Drag:** `onVerticalDragStart/Update/End`, `onHorizontalDragStart/Update/End`
- **Force Press:** `onForcePressStart`, `onForcePressPeak`, `onForcePressUpdate`, `onForcePressEnd`
- **Secondary Tap:** `onSecondaryTap`, `onSecondaryTapDown`, `onSecondaryTapUp`
- **Tertiary Tap:** `onTertiaryTapDown`, `onTertiaryTapUp`
- **Behavior:** `behavior` - HitTestBehavior
- **Exclude:** `excludeFromSemantics`
- `child` - дочерний виджет

---

## InkWell
```
📦 InkWell (Material ripple effect)
  └─ Material (required ancestor!)
      └─ InkResponse -> RenderInkFeatures
          └─ Ripple animation on tap
              └─ Child Widget
```

**RenderObject:** `RenderInkFeatures` (из Material ancestor)

**Параметры:**
- `onTap`, `onDoubleTap`, `onLongPress`
- `onTapDown`, `onTapCancel`, `onTapUp`
- `onHighlightChanged`, `onHover`
- `mouseCursor` - MouseCursor
- `splashColor` - цвет ripple
- `highlightColor` - цвет highlight
- `borderRadius` - BorderRadius (для ripple)
- `customBorder` - ShapeBorder
- `enableFeedback` - haptic feedback
- `excludeFromSemantics`
- `child` - дочерний виджет

---

## InkResponse
```
📦 InkResponse (Customizable InkWell)
  └─ Material (required!)
      └─ Ripple + Highlight -> RenderInkFeatures
          └─ Child Widget
```

**RenderObject:** `RenderInkFeatures` (из Material ancestor)

**Параметры:** Те же что у InkWell + дополнительные:
- `containedInkWell` - ограничить ripple bounds
- `highlightShape` - BoxShape
- `radius` - радиус ripple
- `splashFactory` - InteractiveInkFeatureFactory

---

## Listener
```
📦 Listener (Raw pointer events)
  └─ RenderPointerListener
      └─ Child Widget (receives pointer events)
```

**RenderObject:** `RenderPointerListener`

**Параметры:**
- `onPointerDown` - PointerDownEvent
- `onPointerMove` - PointerMoveEvent
- `onPointerUp` - PointerUpEvent
- `onPointerCancel` - PointerCancelEvent
- `onPointerHover` - PointerHoverEvent
- `onPointerEnter` - PointerEnterEvent
- `onPointerExit` - PointerExitEvent
- `onPointerSignal` - PointerSignalEvent (scroll wheel)
- `behavior` - HitTestBehavior
- `child` - дочерний виджет

---

## MouseRegion
```
📦 MouseRegion (Mouse events)
  └─ RenderMouseRegion
      └─ Child Widget (mouse-aware)
```

**RenderObject:** `RenderMouseRegion`

**Параметры:**
- `onEnter` - PointerEnterEvent
- `onExit` - PointerExitEvent
- `onHover` - PointerHoverEvent
- `cursor` - MouseCursor
- `opaque` - блокировать события для родителей
- `child` - дочерний виджет

---

## AbsorbPointer
```
📦 AbsorbPointer (Block pointer events)
  └─ RenderAbsorbPointer
      └─ Child Widget (не получает события)
```

**RenderObject:** `RenderAbsorbPointer`

**Параметры:**
- `absorbing` - bool (если true, блокирует события)
- `ignoringSemantics` - игнорировать semantics
- `child` - дочерний виджет

---

## IgnorePointer
```
📦 IgnorePointer (Ignore pointer events)
  └─ RenderIgnorePointer
      └─ Child Widget (пропускает события дальше)
```

**RenderObject:** `RenderIgnorePointer`

**Параметры:**
- `ignoring` - bool (если true, игнорирует события)
- `ignoringSemantics` - игнорировать semantics
- `child` - дочерний виджет

**Отличие от AbsorbPointer:** IgnorePointer пропускает события к виджетам позади, AbsorbPointer - нет

---

## Draggable
```
📦 Draggable<T> (Draggable widget)
  └─ GestureDetector (drag detection) -> RenderPointerListener
      ├─ child (when not dragging)
      └─ feedback (dragging overlay)
```

**RenderObject:** `RenderPointerListener` + overlay для feedback

**Параметры:**
- `child` - виджет для перетаскивания
- `feedback` - виджет во время drag
- `childWhenDragging` - виджет на месте оригинала
- `data` - T (данные для DragTarget)
- `axis` - Axis (ограничить направление)
- `dragAnchorStrategy` - позиция feedback
- `affinity` - Axis
- `maxSimultaneousDrags` - макс. одновременных drag
- `onDragStarted`, `onDragUpdate`, `onDraggableCanceled`, `onDragCompleted`, `onDragEnd`
- `ignoringFeedbackSemantics`, `ignoringFeedbackPointer`

---

## LongPressDraggable
```
📦 LongPressDraggable<T> (Long press to drag)
  └─ Draggable (delay: long press duration)
      └─ ...
```

**RenderObject:** `RenderPointerListener` + overlay для feedback

**Параметры:** Те же что у Draggable + `hapticFeedbackOnStart`

---

## DragTarget
```
📦 DragTarget<T> (Drop zone)
  └─ MetaData -> RenderMetaData
      └─ Builder (candidateData, rejectedData)
          └─ Child Widget (rendered by builder)
```

**RenderObject:** `RenderMetaData`

**Параметры:**
- `builder` - Widget Function(BuildContext, List<T?> candidateData, List<dynamic> rejectedData)
- `onWillAcceptWithDetails` - bool Function(DragTargetDetails<T>)
- `onAcceptWithDetails` - void Function(DragTargetDetails<T>)
- `onLeave` - void Function(T?)
- `onMove` - void Function(DragTargetDetails<T>)
- `hitTestBehavior` - HitTestBehavior

---

## Dismissible
```
📦 Dismissible (Swipe to dismiss)
  └─ GestureDetector (drag) -> RenderPointerListener
      └─ SlideTransition -> RenderSlideTransition
          ├─ background (показывается при swipe)
          └─ child (dismissable widget)
```

**RenderObject:** `RenderPointerListener` + `RenderSlideTransition`

**Параметры:**
- `key` - Key (required!)
- `child` - виджет для dismiss
- `background` - виджет за child (swipe right/down)
- `secondaryBackground` - виджет за child (swipe left/up)
- `direction` - DismissDirection
- `dismissThresholds` - Map<DismissDirection, double>
- `movementDuration` - Duration
- `crossAxisEndOffset` - double
- `dragStartBehavior` - DragStartBehavior
- `behavior` - HitTestBehavior
- `onResize`, `onUpdate`, `onDismissed`, `confirmDismiss`

---

## InteractiveViewer
```
📦 InteractiveViewer (Pan, zoom)
  └─ GestureDetector -> RenderPointerListener
      └─ Transform (panEnabled, scaleEnabled) -> RenderTransform
          └─ Child Widget (zoomable)
```

**RenderObject:** `RenderPointerListener` + `RenderTransform`

**Параметры:**
- `child` - виджет для zoom/pan
- `panEnabled` - разрешить pan
- `scaleEnabled` - разрешить zoom
- `constrained` - constraints от родителя
- `boundaryMargin` - EdgeInsets
- `minScale` - минимальный zoom
- `maxScale` - максимальный zoom
- `onInteractionStart`, `onInteractionUpdate`, `onInteractionEnd`
- `transformationController` - TransformationController
- `clipBehavior` - Clip

**Варианты:**
- `InteractiveViewer()` - стандартный
- `InteractiveViewer.builder()` - для больших viewport

---

## Scrollbar
```
📦 Scrollbar (Visual scrollbar)
  └─ RawScrollbar
      └─ Scrollable (required child!)
          └─ ScrollView + thumb overlay
```

**RenderObject:** `RenderMouseRegion` + `RenderIgnorePointer` для thumb

**Параметры:**
- `controller` - ScrollController
- `thumbVisibility` - всегда показывать thumb
- `trackVisibility` - показывать track
- `thickness` - толщина scrollbar
- `radius` - радиус скругления
- `interactive` - можно ли перетаскивать
- `scrollbarOrientation` - ScrollbarOrientation
- `child` - Scrollable виджет
