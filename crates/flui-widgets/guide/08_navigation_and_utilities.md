# 🧭 Navigation Widgets (Навигация)

## Navigator
```
📦 Navigator (Navigation stack)
  └─ Overlay (routes stack) -> RenderTheater
      ├─ Route 1 (bottom)
      ├─ Route 2
      └─ Route N (top)
```

**RenderObject:** `RenderTheater` (для Overlay)

**Параметры:**
- `pages` - List<Page> (declarative navigation)
- `onPopPage` - bool Function(Route, dynamic)
- `initialRoute` - String
- `onGenerateRoute` - Route Function(RouteSettings)
- `onGenerateInitialRoutes` - List<Route> Function(String)
- `onUnknownRoute` - Route Function(RouteSettings)
- `transitionDelegate` - TransitionDelegate
- `observers` - List<NavigatorObserver>
- `reportsRouteUpdateToEngine` - bool
- `clipBehavior` - Clip
- `requestFocus` - bool
- `restorationScopeId` - String

**Методы:**
```dart
Navigator.of(context).push(Route)
Navigator.of(context).pop([result])
Navigator.of(context).pushNamed(String)
Navigator.of(context).pushReplacement(Route)
Navigator.of(context).pushAndRemoveUntil(Route, RoutePredicate)
Navigator.of(context).popUntil(RoutePredicate)
Navigator.of(context).canPop()
Navigator.of(context).maybePop([result])
```

---

## MaterialApp
```
📦 MaterialApp (Material app root)
  └─ WidgetsApp
      └─ Navigator -> RenderTheater
          └─ Material Design theming
              └─ Routes
```

**RenderObject:** Комбинация RenderObject из Navigator и routes

**Параметры (основные):**
- `home` - Widget (главная страница)
- `routes` - Map<String, WidgetBuilder>
- `initialRoute` - String
- `onGenerateRoute` - Route Function(RouteSettings)
- `onGenerateInitialRoutes` - List<Route> Function(String)
- `onUnknownRoute` - Route Function(RouteSettings)
- `navigatorObservers` - List<NavigatorObserver>
- `builder` - Widget Function(BuildContext, Widget?)
- `title` - String (app title)
- `onGenerateTitle` - String Function(BuildContext)
- `color` - Color (primary color for OS)
- `theme` - ThemeData
- `darkTheme` - ThemeData
- `highContrastTheme` - ThemeData
- `highContrastDarkTheme` - ThemeData
- `themeMode` - ThemeMode
- `locale` - Locale
- `localizationsDelegates` - List<LocalizationsDelegate>
- `localeResolutionCallback` - Locale Function(...)
- `supportedLocales` - List<Locale>
- `debugShowMaterialGrid` - bool
- `showPerformanceOverlay` - bool
- `checkerboardRasterCacheImages` - bool
- `checkerboardOffscreenLayers` - bool
- `showSemanticsDebugger` - bool
- `debugShowCheckedModeBanner` - bool
- `shortcuts` - Map<ShortcutActivator, Intent>
- `actions` - Map<Type, Action>
- `restorationScopeId` - String
- `scrollBehavior` - ScrollBehavior

---

## CupertinoApp
```
📦 CupertinoApp (iOS-style app root)
  └─ WidgetsApp
      └─ Navigator -> RenderTheater
          └─ Cupertino theming
              └─ Routes
```

**RenderObject:** Комбинация RenderObject из Navigator и routes

**Параметры:** Похожи на MaterialApp, но с iOS-специфичными:
- `theme` - CupertinoThemeData
- остальные как у MaterialApp

---

## PageRouteBuilder
```
📦 PageRouteBuilder (Custom route transition)
  └─ PageRoute
      └─ Custom transition animation
          └─ pageBuilder(context, animation, secondaryAnimation)
```

**RenderObject:** RenderObject создается в pageBuilder

**Параметры:**
- `pageBuilder` - Widget Function(BuildContext, Animation, Animation) (required)
- `transitionsBuilder` - Widget Function(BuildContext, Animation, Animation, Widget)
- `transitionDuration` - Duration
- `reverseTransitionDuration` - Duration
- `opaque` - bool
- `barrierDismissible` - bool
- `barrierColor` - Color
- `barrierLabel` - String
- `maintainState` - bool
- `fullscreenDialog` - bool

---

## MaterialPageRoute
```
📦 MaterialPageRoute<T> (Material transition)
  └─ PageRoute
      └─ Platform-specific transition
          └─ builder(context)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `builder` - Widget Function(BuildContext) (required)
- `settings` - RouteSettings
- `maintainState` - bool
- `fullscreenDialog` - bool
- `allowSnapshotting` - bool

---

## CupertinoPageRoute
```
📦 CupertinoPageRoute<T> (iOS transition)
  └─ PageRoute
      └─ iOS-style slide transition
          └─ builder(context)
```

**RenderObject:** RenderObject создается в builder

**Параметры:** Те же что у MaterialPageRoute + `title`

---

# 🔧 Utility Widgets (Утилиты)

## Builder
```
📦 Builder
  └─ Вызывает builder с новым BuildContext
      └─ builder(context)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `builder` - Widget Function(BuildContext) (required)

**Применение:** Получить BuildContext для доступа к InheritedWidget

---

## StatefulBuilder
```
📦 StatefulBuilder
  └─ StatefulWidget без отдельного класса
      └─ builder(context, setState)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `builder` - Widget Function(BuildContext, StateSetter) (required)

**Применение:** Локальный state без создания StatefulWidget

---

## LayoutBuilder
```
📦 LayoutBuilder
  └─ Rebuilds на изменение constraints
      └─ builder(context, constraints)
```

**RenderObject:** `RenderConstrainedLayoutBuilder`

**Параметры:**
- `builder` - Widget Function(BuildContext, BoxConstraints) (required)

**Применение:** Адаптивная верстка на основе доступного размера

---

## OrientationBuilder
```
📦 OrientationBuilder
  └─ Rebuilds на изменение ориентации
      └─ builder(context, orientation)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `builder` - Widget Function(BuildContext, Orientation) (required)

**Применение:** Разные layouts для portrait/landscape

---

## MediaQuery
```
📦 MediaQuery (Inherited screen info)
  └─ InheritedWidget
      └─ MediaQueryData (size, padding, orientation, etc.)
          └─ Child Widget
```

**RenderObject:** Не создает свой RenderObject (InheritedWidget)

**Параметры:**
- `data` - MediaQueryData (required)
- `child` - Widget (required)

**Доступ:**
```dart
MediaQuery.of(context).size
MediaQuery.of(context).padding
MediaQuery.of(context).viewInsets
MediaQuery.of(context).orientation
MediaQuery.of(context).devicePixelRatio
MediaQuery.of(context).platformBrightness
MediaQuery.of(context).textScaler
```

---

## SafeArea
```
📦 SafeArea (Avoid system UI)
  └─ Padding (system insets) -> RenderPadding
      └─ MediaQuery (updates insets)
          └─ Child Widget
```

**RenderObject:** `RenderPadding`

**Параметры:**
- `child` - Widget (required)
- `left` - bool (avoid left inset)
- `top` - bool (avoid top inset)
- `right` - bool (avoid right inset)
- `bottom` - bool (avoid bottom inset)
- `minimum` - EdgeInsets (минимальные отступы)
- `maintainBottomViewPadding` - bool

---

## Theme
```
📦 Theme (Inherited theme)
  └─ InheritedTheme
      └─ ThemeData (colors, typography, etc.)
          └─ Child Widget
```

**RenderObject:** Не создает свой RenderObject (InheritedWidget)

**Параметры:**
- `data` - ThemeData (required)
- `child` - Widget (required)

**Доступ:**
```dart
Theme.of(context).primaryColor
Theme.of(context).textTheme
Theme.of(context).appBarTheme
...
```

---

## InheritedWidget
```
📦 InheritedWidget (Data propagation)
  └─ Abstract base class
      └─ Efficient data sharing down tree
          └─ Child Widget (can access data)
```

**RenderObject:** Не создает RenderObject (не участвует в rendering)

**Применение:** Создать custom inherited widget для sharing data

**Методы:**
- `updateShouldNotify(covariant InheritedWidget oldWidget)` - bool

---

## InheritedTheme
```
📦 InheritedTheme (Theme propagation)
  └─ InheritedWidget
      └─ Theme data
          └─ Child Widget
```

**RenderObject:** Не создает RenderObject

**Применение:** Base для theme widgets

---

## ValueListenableBuilder
```
📦 ValueListenableBuilder<T> (Listen to ValueNotifier)
  └─ Rebuilds когда value changes
      └─ builder(context, value, child)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `valueListenable` - ValueListenable<T> (required)
- `builder` - Widget Function(BuildContext, T, Widget?) (required)
- `child` - Widget (cached, не rebuilds)

**Применение:** Reactive UI для ValueNotifier

---

## StreamBuilder
```
📦 StreamBuilder<T> (Listen to Stream)
  └─ Rebuilds на каждое событие stream
      └─ builder(context, snapshot)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `stream` - Stream<T>
- `initialData` - T
- `builder` - Widget Function(BuildContext, AsyncSnapshot<T>) (required)

**Применение:** Reactive UI для Stream

---

## FutureBuilder
```
📦 FutureBuilder<T> (Listen to Future)
  └─ Rebuilds когда future completes
      └─ builder(context, snapshot)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `future` - Future<T>
- `initialData` - T
- `builder` - Widget Function(BuildContext, AsyncSnapshot<T>) (required)

**Применение:** Loading states для async operations

---

## Form
```
📦 Form (Form validation)
  └─ FormState (validation, saving)
      └─ Children (FormField widgets)
```

**RenderObject:** Не создает свой RenderObject (управляет state)

**Параметры:**
- `child` - Widget (required)
- `onChanged` - VoidCallback
- `autovalidateMode` - AutovalidateMode
- `onWillPop` - Future<bool> Function()

**Доступ:**
```dart
Form.of(context).validate() -> bool
Form.of(context).save()
Form.of(context).reset()
```

---

## FormField
```
📦 FormField<T> (Form field base)
  └─ FormFieldState<T>
      └─ builder(state)
```

**RenderObject:** RenderObject создается в builder

**Параметры:**
- `builder` - Widget Function(FormFieldState<T>) (required)
- `onSaved` - void Function(T?)
- `validator` - String? Function(T?)
- `initialValue` - T
- `autovalidateMode` - AutovalidateMode
- `enabled` - bool
- `restorationId` - String

**Применение:** Base для custom form fields

---

## Focus
```
📦 Focus (Focus management)
  └─ FocusNode
      └─ Child Widget (focusable)
```

**RenderObject:** `RenderProxyBox` (или RenderObject ребенка)

**Параметры:**
- `child` - Widget (required)
- `focusNode` - FocusNode
- `autofocus` - bool
- `onFocusChange` - void Function(bool)
- `onKey` - KeyEventResult Function(FocusNode, KeyEvent)
- `onKeyEvent` - KeyEventResult Function(FocusNode, KeyEvent)
- `canRequestFocus` - bool
- `skipTraversal` - bool
- `descendantsAreFocusable` - bool
- `descendantsAreTraversable` - bool
- `includeSemantics` - bool
- `debugLabel` - String

---

## FocusScope
```
📦 FocusScope (Focus subtree)
  └─ Focus
      └─ FocusScopeNode (manages focus tree)
          └─ Child Widget
```

**RenderObject:** `RenderProxyBox` (или RenderObject ребенка)

**Параметры:** Те же что у Focus + `node` (FocusScopeNode)

---

## Semantics
```
📦 Semantics (Accessibility)
  └─ RenderSemantics
      └─ SemanticsNode (accessibility info)
          └─ Child Widget
```

**RenderObject:** `RenderSemantics`

**Параметры:**
- `child` - Widget
- `container` - bool
- `explicitChildNodes` - bool
- `excludeSemantics` - bool
- `enabled` - bool
- `checked` - bool
- `toggled` - bool
- `selected` - bool
- `button` - bool
- `slider` - bool
- `keyboardKey` - bool
- `link` - bool
- `header` - bool
- `textField` - bool
- `readOnly` - bool
- `focusable` - bool
- `focused` - bool
- `inMutuallyExclusiveGroup` - bool
- `obscured` - bool
- `multiline` - bool
- `scopesRoute` - bool
- `namesRoute` - bool
- `image` - bool
- `liveRegion` - bool
- `label` - String
- `value` - String
- `increasedValue` - String
- `decreasedValue` - String
- `hint` - String
- `textDirection` - TextDirection
- `sortKey` - SemanticsSortKey
- `onTap`, `onLongPress`, `onScrollLeft`, `onScrollRight`, `onScrollUp`, `onScrollDown`
- `onIncrease`, `onDecrease`, `onCopy`, `onCut`, `onPaste`, `onMoveCursorForwardByCharacter`, etc.

---

## ExcludeSemantics
```
📦 ExcludeSemantics (Hide from accessibility)
  └─ Semantics (excludeSemantics: true) -> RenderSemantics
      └─ Child Widget (hidden from screen readers)
```

**RenderObject:** `RenderSemantics`

**Параметры:**
- `excluding` - bool (default true)
- `child` - Widget

---

## MergeSemantics
```
📦 MergeSemantics (Merge child semantics)
  └─ Semantics (merges children) -> RenderMergeSemantics
      └─ Child Widget
```

**RenderObject:** `RenderMergeSemantics`

**Параметры:**
- `child` - Widget

---

## Placeholder
```
📦 Placeholder (Временный виджет)
  └─ LimitedBox -> RenderLimitedBox
      └─ CustomPaint (рисует X) -> RenderCustomPaint
```

**RenderObject:** `RenderLimitedBox` + `RenderCustomPaint`

**Параметры:**
- `color` - Color
- `strokeWidth` - double
- `fallbackWidth` - double
- `fallbackHeight` - double

---

# 📱 Platform-Specific Widgets (Платформо-специфичные)

## PlatformMenuBar
```
📦 PlatformMenuBar (Native menu bar)
  └─ Platform-specific menu
      └─ Menu items (desktop platforms)
```

**RenderObject:** Platform-specific (не использует Flutter rendering)

**Параметры:**
- `menus` - List<PlatformMenuItem> (required)

---

## SelectionArea
```
📦 SelectionArea (Text selection)
  └─ SelectionContainer -> RenderSelectionContainer
      └─ Child Widget (selectable content)
```

**RenderObject:** `RenderSelectionContainer`

**Параметры:**
- `child` - Widget (required)
- `focusNode` - FocusNode
- `selectionControls` - TextSelectionControls
- `contextMenuBuilder` - Widget Function(BuildContext, SelectableRegionState)
- `magnifierConfiguration` - TextMagnifierConfiguration
- `onSelectionChanged` - void Function(SelectedContent?)

---

## CupertinoNavigationBar
```
📦 CupertinoNavigationBar (iOS nav bar)
  └─ CupertinoSliverNavigationBar -> RenderSliverPersistentHeader
      ├─ Leading (back button)
      ├─ Middle (title)
      └─ Trailing (buttons)
```

**RenderObject:** `RenderSliverPersistentHeader`

**Параметры:**
- `leading` - Widget
- `middle` - Widget (title)
- `trailing` - Widget
- `backgroundColor` - Color
- `brightness` - Brightness
- `padding` - EdgeInsetsDirectional
- `border` - Border
- `transitionBetweenRoutes` - bool
- `heroTag` - Object
- `previousPageTitle` - String

---

## CustomPaint
```
📦 CustomPaint (Custom painting)
  └─ RenderCustomPaint
      └─ CustomPainter (foreground/background)
          └─ Child Widget (optional)
```

**RenderObject:** `RenderCustomPaint`

**Параметры:**
- `painter` - CustomPainter (background)
- `foregroundPainter` - CustomPainter (foreground)
- `size` - Size (preferred size)
- `isComplex` - bool (hint для caching)
- `willChange` - bool (hint для animation)
- `child` - Widget

**CustomPainter методы:**
- `paint(Canvas canvas, Size size)` - рисование
- `shouldRepaint(CustomPainter oldDelegate)` - bool

---

## SingleChildRenderObjectWidget

Абстрактный base class для виджетов с одним ребенком и custom RenderObject:

```dart
class MyWidget extends SingleChildRenderObjectWidget {
  @override
  RenderObject createRenderObject(BuildContext context) {
    return MyRenderObject();
  }
  
  @override
  void updateRenderObject(BuildContext context, MyRenderObject renderObject) {
    // Update properties
  }
}
```

---

## MultiChildRenderObjectWidget

Абстрактный base class для виджетов с несколькими детьми и custom RenderObject:

```dart
class MyWidget extends MultiChildRenderObjectWidget {
  @override
  RenderObject createRenderObject(BuildContext context) {
    return MyRenderObject();
  }
  
  @override
  void updateRenderObject(BuildContext context, MyRenderObject renderObject) {
    // Update properties
  }
}
```

---

## LeafRenderObjectWidget

Абстрактный base class для виджетов без детей и custom RenderObject:

```dart
class MyWidget extends LeafRenderObjectWidget {
  @override
  RenderObject createRenderObject(BuildContext context) {
    return MyRenderObject();
  }
  
  @override
  void updateRenderObject(BuildContext context, MyRenderObject renderObject) {
    // Update properties
  }
}
```
