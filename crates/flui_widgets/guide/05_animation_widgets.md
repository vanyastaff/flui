# 🎬 Animation Widgets (Анимированные виджеты)

## AnimatedContainer
```
📦 AnimatedContainer
  └─ ImplicitlyAnimatedWidget
      └─ Animates Container properties -> Various RenderObjects
          └─ Container (с анимированными параметрами)
```

**RenderObject:** Комбинация RenderObject из Container (RenderPadding, RenderDecoratedBox, etc.)

**Параметры:**
- Все параметры Container
- `duration` - Duration
- `curve` - Curve
- `onEnd` - VoidCallback

---

## AnimatedPadding
```
📦 AnimatedPadding
  └─ ImplicitlyAnimatedWidget
      └─ Animates padding -> RenderAnimatedPadding
          └─ Padding (с анимированным padding)
```

**RenderObject:** `RenderPadding` (с анимацией)

**Параметры:**
- `padding` - EdgeInsets (target)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedAlign
```
📦 AnimatedAlign
  └─ ImplicitlyAnimatedWidget
      └─ Animates alignment -> RenderAnimatedAlign
          └─ Align (с анимированным alignment)
```

**RenderObject:** `RenderPositionedBox` (с анимацией)

**Параметры:**
- `alignment` - AlignmentGeometry (target)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedPositioned
```
📦 AnimatedPositioned (только в Stack!)
  └─ ImplicitlyAnimatedWidget
      └─ Animates position -> Stack's RenderStack
          └─ Positioned (с анимированными left/top/right/bottom)
```

**RenderObject:** Не создает свой RenderObject (модифицирует StackParentData в анимации)

**Параметры:**
- `left`, `top`, `right`, `bottom`, `width`, `height` (target)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedOpacity
```
📦 AnimatedOpacity
  └─ ImplicitlyAnimatedWidget
      └─ Animates opacity -> RenderAnimatedOpacity
          └─ Opacity (с анимированной opacity)
```

**RenderObject:** `RenderAnimatedOpacity`

**Параметры:**
- `opacity` - double (target 0.0-1.0)
- `duration`, `curve`, `onEnd`
- `alwaysIncludeSemantics`
- `child` - дочерний виджет

---

## AnimatedRotation
```
📦 AnimatedRotation
  └─ ImplicitlyAnimatedWidget
      └─ Animates rotation -> RenderTransform (с анимацией)
          └─ Transform.rotate (с анимированным углом)
```

**RenderObject:** `RenderTransform` (с анимацией)

**Параметры:**
- `turns` - double (0.0 = 0°, 0.5 = 180°, 1.0 = 360°)
- `alignment` - Alignment
- `filterQuality` - FilterQuality
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedScale
```
📦 AnimatedScale
  └─ ImplicitlyAnimatedWidget
      └─ Animates scale -> RenderTransform (с анимацией)
          └─ Transform.scale (с анимированным scale)
```

**RenderObject:** `RenderTransform` (с анимацией)

**Параметры:**
- `scale` - double (target scale)
- `alignment` - Alignment
- `filterQuality` - FilterQuality
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedSlide
```
📦 AnimatedSlide
  └─ ImplicitlyAnimatedWidget
      └─ Animates offset -> RenderFractionalTranslation (с анимацией)
          └─ FractionalTranslation (с анимированным offset)
```

**RenderObject:** `RenderFractionalTranslation` (с анимацией)

**Параметры:**
- `offset` - Offset (fractional offset, 1.0 = size)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedDefaultTextStyle
```
📦 AnimatedDefaultTextStyle
  └─ ImplicitlyAnimatedWidget
      └─ Animates text style
          └─ DefaultTextStyle (с анимированным style)
```

**RenderObject:** `RenderParagraph` (для детей, с анимацией стиля)

**Параметры:**
- `style` - TextStyle (target)
- `textAlign` - TextAlign
- `softWrap` - bool
- `overflow` - TextOverflow
- `maxLines` - int
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedPhysicalModel
```
📦 AnimatedPhysicalModel
  └─ ImplicitlyAnimatedWidget
      └─ Animates physical properties -> RenderAnimatedPhysicalModel
          └─ PhysicalModel (с анимацией)
```

**RenderObject:** `RenderPhysicalModel` (с анимацией)

**Параметры:**
- `color` - Color (target)
- `shadowColor` - Color
- `elevation` - double
- `shape` - BoxShape
- `borderRadius` - BorderRadius
- `animateColor`, `animateShadowColor`
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

---

## AnimatedSwitcher
```
📦 AnimatedSwitcher (Cross-fade children)
  └─ Stack -> RenderStack
      ├─ Old child (fade out)
      └─ New child (fade in)
```

**RenderObject:** `RenderStack` + `RenderAnimatedOpacity` для каждого child

**Параметры:**
- `child` - текущий виджет (меняется по key)
- `duration` - Duration
- `reverseDuration` - Duration (для обратной анимации)
- `switchInCurve` - Curve (для нового child)
- `switchOutCurve` - Curve (для старого child)
- `transitionBuilder` - Widget Function(Widget, Animation<double>)
- `layoutBuilder` - Widget Function(Widget?, List<Widget>)

---

## AnimatedCrossFade
```
📦 AnimatedCrossFade (Cross-fade between two children)
  └─ Stack -> RenderStack
      ├─ firstChild (показывается если CrossFadeState.showFirst)
      └─ secondChild (показывается если CrossFadeState.showSecond)
```

**RenderObject:** `RenderStack` + `RenderAnimatedOpacity`

**Параметры:**
- `firstChild` - виджет 1
- `secondChild` - виджет 2
- `crossFadeState` - CrossFadeState (showFirst/showSecond)
- `duration` - Duration
- `reverseDuration` - Duration
- `firstCurve`, `secondCurve`, `sizeCurve` - Curve
- `alignment` - Alignment
- `layoutBuilder` - Widget Function(Widget, Key, Widget, Key)

---

## Hero
```
📦 Hero (Shared element transition)
  └─ Navigator transition координация
      └─ Child Widget (flies между screens)
```

**RenderObject:** Использует RenderObject ребенка + overlay для transition

**Параметры:**
- `tag` - Object (уникальный id для shared element)
- `child` - виджет для transition
- `createRectTween` - RectTween Function(Rect?, Rect?)
- `flightShuttleBuilder` - Widget Function(...)
- `placeholderBuilder` - Widget Function(...)
- `transitionOnUserGestures` - анимация при gesture navigation

---

## AnimatedBuilder
```
📦 AnimatedBuilder (Explicit animation)
  └─ Animation<T> listener
      └─ builder(context, child) (rebuild on animation)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `animation` - Listenable (обычно Animation)
- `builder` - Widget Function(BuildContext, Widget? child)
- `child` - Widget (cached, не rebuilds)

---

## AnimatedWidget
```
📦 AnimatedWidget (Base for explicit animations)
  └─ Abstract base class
      └─ Subclass implements build(context)
```

**RenderObject:** RenderObject создается в build() subclass

**Параметры:**
- `listenable` - Listenable (обычно Animation)

**Применение:** Наследовать для custom animated widgets

---

## TweenAnimationBuilder
```
📦 TweenAnimationBuilder<T> (Tween-based animation)
  └─ ImplicitlyAnimatedWidget
      └─ Tween<T>.animate(AnimationController)
          └─ builder(context, value, child)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `tween` - Tween<T>
- `duration` - Duration
- `curve` - Curve
- `builder` - Widget Function(BuildContext, T value, Widget? child)
- `child` - Widget (cached)
- `onEnd` - VoidCallback

---

## Transition Widgets (для явных анимаций)

### FadeTransition
```
📦 FadeTransition
  └─ AnimatedWidget -> RenderAnimatedOpacity
      └─ Opacity (controlled by animation)
```

**RenderObject:** `RenderAnimatedOpacity`

**Параметры:**
- `opacity` - Animation<double>
- `alwaysIncludeSemantics` - bool
- `child` - Widget

---

### SlideTransition
```
📦 SlideTransition
  └─ AnimatedWidget -> RenderFractionalTranslation
      └─ FractionalTranslation (controlled by animation)
```

**RenderObject:** `RenderFractionalTranslation`

**Параметры:**
- `position` - Animation<Offset>
- `transformHitTests` - bool
- `textDirection` - TextDirection
- `child` - Widget

---

### ScaleTransition
```
📦 ScaleTransition
  └─ AnimatedWidget -> RenderTransform
      └─ Transform.scale (controlled by animation)
```

**RenderObject:** `RenderTransform`

**Параметры:**
- `scale` - Animation<double>
- `alignment` - Alignment
- `filterQuality` - FilterQuality
- `child` - Widget

---

### RotationTransition
```
📦 RotationTransition
  └─ AnimatedWidget -> RenderTransform
      └─ Transform.rotate (controlled by animation)
```

**RenderObject:** `RenderTransform`

**Параметры:**
- `turns` - Animation<double>
- `alignment` - Alignment
- `filterQuality` - FilterQuality
- `child` - Widget

---

### SizeTransition
```
📦 SizeTransition
  └─ AnimatedWidget -> RenderAnimatedSize
      └─ Size (controlled by animation)
```

**RenderObject:** `RenderAnimatedSize`

**Параметры:**
- `sizeFactor` - Animation<double>
- `axis` - Axis
- `axisAlignment` - double
- `child` - Widget

---

### PositionedTransition
```
📦 PositionedTransition (только в Stack!)
  └─ AnimatedWidget
      └─ Positioned (controlled by animation)
```

**RenderObject:** Модифицирует StackParentData

**Параметры:**
- `rect` - Animation<RelativeRect>
- `child` - Widget

---

### DecoratedBoxTransition
```
📦 DecoratedBoxTransition
  └─ AnimatedWidget -> RenderDecoratedBox
      └─ DecoratedBox (controlled by animation)
```

**RenderObject:** `RenderDecoratedBox`

**Параметры:**
- `decoration` - Animation<Decoration>
- `position` - DecorationPosition
- `child` - Widget

---

### AlignTransition
```
📦 AlignTransition
  └─ AnimatedWidget -> RenderPositionedBox
      └─ Align (controlled by animation)
```

**RenderObject:** `RenderPositionedBox`

**Параметры:**
- `alignment` - Animation<AlignmentGeometry>
- `widthFactor` - double
- `heightFactor` - double
- `child` - Widget

---

### DefaultTextStyleTransition
```
📦 DefaultTextStyleTransition
  └─ AnimatedWidget
      └─ DefaultTextStyle (controlled by animation)
```

**RenderObject:** `RenderParagraph` (для детей)

**Параметры:**
- `style` - Animation<TextStyle>
- `textAlign` - TextAlign
- `softWrap` - bool
- `overflow` - TextOverflow
- `maxLines` - int
- `child` - Widget

---

## AnimationController

Не является виджетом, но важен для явных анимаций:

```dart
AnimationController(
  duration: Duration,
  reverseDuration: Duration,
  lowerBound: double,
  upperBound: double,
  value: double,
  vsync: TickerProvider, // обычно this для StatefulWidget with TickerProviderStateMixin
)
```

**Методы:**
- `forward()` - запустить анимацию вперед
- `reverse()` - запустить анимацию назад
- `repeat()` - повторять анимацию
- `reset()` - сбросить в начало
- `stop()` - остановить анимацию
- `animateTo(value)` - анимировать до значения
- `animateBack(value)` - анимировать назад к значению

---

## Tween

Не является виджетом, но определяет интерполяцию:

```dart
Tween<T>(begin: T, end: T)
ColorTween(begin: Color, end: Color)
SizeTween(begin: Size, end: Size)
RectTween(begin: Rect, end: Rect)
IntTween(begin: int, end: int)
```

**Методы:**
- `animate(Animation)` - создать Animation<T>
- `chain(Animatable)` - цепочка трансформаций
- `transform(double t)` - вычислить значение

---

## Curves

Стандартные кривые анимации:

- `linear` - линейная
- `easeIn`, `easeOut`, `easeInOut` - ease
- `fastOutSlowIn` - Material Design стандарт
- `bounceIn`, `bounceOut`, `bounceInOut` - отскок
- `elasticIn`, `elasticOut`, `elasticInOut` - эластичность
- `decelerate` - замедление
- `fastLinearToSlowEaseIn` - быстрый старт
- Custom: `Cubic(a, b, c, d)`, `Interval(begin, end)`
