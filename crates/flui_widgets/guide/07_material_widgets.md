# 🎭 Material Design Widgets (Material компоненты)

## Scaffold
```
📦 Scaffold (Material page structure)
  └─ Material -> RenderPhysicalModel
      ├─ AppBar (top)
      ├─ Body (центр)
      ├─ BottomNavigationBar (bottom)
      ├─ FloatingActionButton (floating)
      ├─ Drawer (left)
      └─ EndDrawer (right)
```

**RenderObject:** Комбинация различных RenderObject для каждой части + `RenderScaffold`

**Параметры:**
- `appBar` - PreferredSizeWidget (обычно AppBar)
- `body` - Widget (главный контент)
- `floatingActionButton` - Widget
- `floatingActionButtonLocation` - FloatingActionButtonLocation
- `floatingActionButtonAnimator` - FloatingActionButtonAnimator
- `persistentFooterButtons` - List<Widget>
- `persistentFooterAlignment` - AlignmentDirectional
- `drawer` - Widget (left drawer)
- `endDrawer` - Widget (right drawer)
- `drawerScrimColor` - Color
- `backgroundColor` - Color
- `bottomNavigationBar` - Widget
- `bottomSheet` - Widget
- `resizeToAvoidBottomInset` - bool
- `primary` - bool
- `drawerDragStartBehavior` - DragStartBehavior
- `extendBody` - bool
- `extendBodyBehindAppBar` - bool
- `drawerEdgeDragWidth` - double
- `drawerEnableOpenDragGesture` - bool
- `endDrawerEnableOpenDragGesture` - bool
- `restorationId` - String

---

## AppBar
```
📦 AppBar (Material app bar)
  └─ Material (elevation, color) -> RenderPhysicalModel
      └─ SafeArea
          └─ FlexibleSpaceBar (optional)
              ├─ Leading (back button, hamburger)
              ├─ Title (text)
              └─ Actions (icons)
```

**RenderObject:** `RenderPhysicalModel` + `RenderFlex` для layout

**Параметры:**
- `leading` - Widget (левая иконка)
- `automaticallyImplyLeading` - bool
- `title` - Widget
- `actions` - List<Widget> (правые иконки)
- `flexibleSpace` - Widget
- `bottom` - PreferredSizeWidget (TabBar, etc.)
- `elevation` - double
- `scrolledUnderElevation` - double
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `shape` - ShapeBorder
- `backgroundColor` - Color
- `foregroundColor` - Color
- `iconTheme` - IconThemeData
- `actionsIconTheme` - IconThemeData
- `primary` - bool
- `centerTitle` - bool
- `excludeHeaderSemantics` - bool
- `titleSpacing` - double
- `toolbarOpacity` - double
- `bottomOpacity` - double
- `toolbarHeight` - double
- `leadingWidth` - double
- `toolbarTextStyle` - TextStyle
- `titleTextStyle` - TextStyle
- `systemOverlayStyle` - SystemUiOverlayStyle

---

## BottomNavigationBar
```
📦 BottomNavigationBar (Material bottom nav)
  └─ Material -> RenderPhysicalModel
      └─ Row -> RenderFlex
          ├─ BottomNavigationBarItem 1
          ├─ BottomNavigationBarItem 2
          └─ BottomNavigationBarItem N
```

**RenderObject:** `RenderPhysicalModel` + `RenderFlex`

**Параметры:**
- `items` - List<BottomNavigationBarItem> (required)
- `currentIndex` - int (selected index)
- `onTap` - void Function(int)
- `elevation` - double
- `type` - BottomNavigationBarType (fixed, shifting)
- `fixedColor` - Color (selected item color)
- `backgroundColor` - Color
- `iconSize` - double
- `selectedItemColor` - Color
- `unselectedItemColor` - Color
- `selectedIconTheme` - IconThemeData
- `unselectedIconTheme` - IconThemeData
- `selectedFontSize` - double
- `unselectedFontSize` - double
- `selectedLabelStyle` - TextStyle
- `unselectedLabelStyle` - TextStyle
- `showSelectedLabels` - bool
- `showUnselectedLabels` - bool
- `mouseCursor` - MouseCursor
- `enableFeedback` - bool
- `landscapeLayout` - BottomNavigationBarLandscapeLayout

---

## Drawer
```
📦 Drawer (Side panel)
  └─ Material (elevation) -> RenderPhysicalModel
      └─ ConstrainedBox (width) -> RenderConstrainedBox
          └─ Column (typically) -> RenderFlex
              ├─ DrawerHeader
              ├─ ListTile 1
              ├─ ListTile 2
              └─ ...
```

**RenderObject:** `RenderPhysicalModel` + `RenderConstrainedBox`

**Параметры:**
- `child` - Widget (drawer content)
- `backgroundColor` - Color
- `elevation` - double
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `shape` - ShapeBorder
- `width` - double
- `semanticLabel` - String

---

## Card
```
📦 Card
  └─ Material (elevation, shape, clipBehavior) -> RenderPhysicalModel
      └─ Child Widget
```

**RenderObject:** `RenderPhysicalModel`

**Параметры:**
- `child` - Widget
- `color` - Color
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `elevation` - double
- `shape` - ShapeBorder
- `borderOnForeground` - bool
- `margin` - EdgeInsets
- `clipBehavior` - Clip
- `semanticContainer` - bool

---

## ListTile
```
📦 ListTile (Material list item)
  └─ InkWell (ripple) -> RenderInkFeatures
      └─ Row -> RenderFlex
          ├─ Leading (icon/avatar)
          ├─ Column -> RenderFlex (title + subtitle)
          └─ Trailing (icon/widget)
```

**RenderObject:** `RenderInkFeatures` + `RenderFlex`

**Параметры:**
- `leading` - Widget
- `title` - Widget
- `subtitle` - Widget
- `trailing` - Widget
- `isThreeLine` - bool
- `dense` - bool
- `visualDensity` - VisualDensity
- `shape` - ShapeBorder
- `style` - ListTileStyle
- `selectedColor` - Color
- `iconColor` - Color
- `textColor` - Color
- `contentPadding` - EdgeInsets
- `enabled` - bool
- `onTap` - GestureTapCallback
- `onLongPress` - GestureLongPressCallback
- `mouseCursor` - MouseCursor
- `selected` - bool
- `focusColor`, `hoverColor` - Color
- `splashColor` - Color
- `focusNode` - FocusNode
- `autofocus` - bool
- `tileColor` - Color
- `selectedTileColor` - Color
- `enableFeedback` - bool
- `horizontalTitleGap` - double
- `minVerticalPadding` - double
- `minLeadingWidth` - double
- `titleAlignment` - ListTileTitleAlignment

---

## Dialog
```
📦 Dialog (Modal dialog)
  └─ Overlay
      └─ Barrier (scrim)
          └─ Material (rounded, elevated) -> RenderPhysicalModel
              └─ Padding -> RenderPadding
                  └─ Child Widget (dialog content)
```

**RenderObject:** `RenderPhysicalModel` + overlay

**Параметры:**
- `child` - Widget
- `backgroundColor` - Color
- `elevation` - double
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `insetPadding` - EdgeInsets
- `clipBehavior` - Clip
- `shape` - ShapeBorder
- `alignment` - AlignmentGeometry

**Показ:**
```dart
showDialog<T>(
  context: context,
  builder: (context) => Dialog(child: ...),
  barrierDismissible: bool,
  barrierColor: Color,
  barrierLabel: String,
  useSafeArea: bool,
  useRootNavigator: bool,
  routeSettings: RouteSettings,
  anchorPoint: Offset,
)
```

---

## AlertDialog
```
📦 AlertDialog (Material alert dialog)
  └─ Dialog -> RenderPhysicalModel
      └─ Column -> RenderFlex
          ├─ Icon (optional)
          ├─ Title
          ├─ Content
          └─ Actions (buttons)
```

**RenderObject:** `RenderPhysicalModel` + `RenderFlex`

**Параметры:**
- `icon` - Widget
- `title` - Widget
- `titlePadding` - EdgeInsets
- `titleTextStyle` - TextStyle
- `content` - Widget
- `contentPadding` - EdgeInsets
- `contentTextStyle` - TextStyle
- `actions` - List<Widget> (buttons)
- `actionsPadding` - EdgeInsets
- `actionsAlignment` - MainAxisAlignment
- `actionsOverflowDirection` - VerticalDirection
- `actionsOverflowButtonSpacing` - double
- `buttonPadding` - EdgeInsets
- `backgroundColor`, `elevation`, `shadowColor`, `surfaceTintColor`
- `semanticLabel` - String
- `insetPadding` - EdgeInsets
- `clipBehavior` - Clip
- `shape` - ShapeBorder
- `alignment` - AlignmentGeometry
- `scrollable` - bool

---

## SnackBar
```
📦 SnackBar (Temporary message)
  └─ Material (bottom sheet) -> RenderPhysicalModel
      └─ Row -> RenderFlex
          ├─ Content (text/widget)
          └─ Action (button)
```

**RenderObject:** `RenderPhysicalModel` + `RenderFlex`

**Параметры:**
- `content` - Widget (required)
- `backgroundColor` - Color
- `elevation` - double
- `margin` - EdgeInsets
- `padding` - EdgeInsets
- `width` - double
- `shape` - ShapeBorder
- `behavior` - SnackBarBehavior (fixed, floating)
- `action` - SnackBarAction
- `duration` - Duration
- `animation` - Animation<double>
- `onVisible` - VoidCallback
- `dismissDirection` - DismissDirection
- `clipBehavior` - Clip

**Показ:**
```dart
ScaffoldMessenger.of(context).showSnackBar(
  SnackBar(content: Text('...'))
)
```

---

## BottomSheet
```
📦 BottomSheet (Bottom panel)
  └─ Material (elevation, rounded top) -> RenderPhysicalShape
      └─ Padding -> RenderPadding
          └─ Child Widget (sheet content)
```

**RenderObject:** `RenderPhysicalShape` + overlay

**Параметры:**
- `onClosing` - VoidCallback (required)
- `builder` - WidgetBuilder (required)
- `backgroundColor` - Color
- `elevation` - double
- `shape` - ShapeBorder
- `clipBehavior` - Clip
- `constraints` - BoxConstraints
- `enableDrag` - bool
- `showDragHandle` - bool
- `dragHandleColor` - Color
- `dragHandleSize` - Size

**Показ:**
```dart
showModalBottomSheet<T>(
  context: context,
  builder: (context) => Widget,
  backgroundColor: Color,
  elevation: double,
  shape: ShapeBorder,
  clipBehavior: Clip,
  constraints: BoxConstraints,
  barrierColor: Color,
  isScrollControlled: bool,
  useRootNavigator: bool,
  isDismissible: bool,
  enableDrag: bool,
  showDragHandle: bool,
  useSafeArea: bool,
  routeSettings: RouteSettings,
  transitionAnimationController: AnimationController,
  anchorPoint: Offset,
)
```

---

## CircularProgressIndicator
```
📦 CircularProgressIndicator (Spinning circle)
  └─ CustomPaint -> RenderCustomPaint
      └─ Animated circular arc
```

**RenderObject:** `RenderCustomPaint`

**Параметры:**
- `value` - double? (0.0-1.0, null = indeterminate)
- `backgroundColor` - Color
- `color` - Color (foreground)
- `valueColor` - Animation<Color?>
- `strokeWidth` - double
- `strokeAlign` - double
- `strokeCap` - StrokeCap
- `semanticsLabel` - String
- `semanticsValue` - String

**Варианты:**
- `CircularProgressIndicator()` - Material Design
- `CircularProgressIndicator.adaptive()` - платформо-специфичный

---

## LinearProgressIndicator
```
📦 LinearProgressIndicator (Horizontal bar)
  └─ CustomPaint -> RenderCustomPaint
      └─ Animated rectangle
```

**RenderObject:** `RenderCustomPaint`

**Параметры:**
- `value` - double? (0.0-1.0, null = indeterminate)
- `backgroundColor` - Color
- `color` - Color
- `valueColor` - Animation<Color?>
- `minHeight` - double
- `semanticsLabel` - String
- `semanticsValue` - String
- `borderRadius` - BorderRadius

---

## Tooltip
```
📦 Tooltip (Hover/long-press tooltip)
  └─ GestureDetector (long press detection) -> RenderPointerListener
      └─ Overlay entry (при показе)
          └─ Material (tooltip bubble) -> RenderPhysicalModel
              └─ Text (message)
```

**RenderObject:** `RenderPointerListener` + overlay с `RenderPhysicalModel`

**Параметры:**
- `message` - String (текст tooltip)
- `richMessage` - InlineSpan (rich text)
- `height` - double
- `padding` - EdgeInsets
- `margin` - EdgeInsets
- `verticalOffset` - double
- `preferBelow` - bool
- `excludeFromSemantics` - bool
- `decoration` - Decoration
- `textStyle` - TextStyle
- `textAlign` - TextAlign
- `waitDuration` - Duration
- `showDuration` - Duration
- `exitDuration` - Duration
- `enableFeedback` - bool
- `triggerMode` - TooltipTriggerMode
- `child` - Widget

---

## Chip
```
📦 Chip (Material chip)
  └─ Material (rounded) -> RenderPhysicalModel
      └─ InkWell (ripple) -> RenderInkFeatures
          └─ Row -> RenderFlex
              ├─ Avatar (optional)
              ├─ Label
              └─ Delete button (optional)
```

**RenderObject:** `RenderPhysicalModel` + `RenderInkFeatures` + `RenderFlex`

**Параметры:**
- `avatar` - Widget (leading icon/avatar)
- `label` - Widget (text)
- `labelStyle` - TextStyle
- `labelPadding` - EdgeInsets
- `deleteIcon` - Widget
- `onDeleted` - VoidCallback
- `deleteIconColor` - Color
- `deleteButtonTooltipMessage` - String
- `side` - BorderSide
- `shape` - OutlinedBorder
- `clipBehavior` - Clip
- `focusNode` - FocusNode
- `autofocus` - bool
- `backgroundColor` - Color
- `padding` - EdgeInsets
- `visualDensity` - VisualDensity
- `materialTapTargetSize` - MaterialTapTargetSize
- `elevation` - double
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `iconTheme` - IconThemeData

**Варианты:**
- `Chip()` - базовый
- `InputChip()` - для ввода
- `ChoiceChip()` - выбор (radio-like)
- `FilterChip()` - фильтр (checkbox-like)
- `ActionChip()` - действие (button-like)

---

## Badge
```
📦 Badge (Notification badge)
  └─ Stack -> RenderStack
      ├─ Child Widget
      └─ Positioned (badge dot/label) -> RenderPhysicalModel
```

**RenderObject:** `RenderStack` + `RenderPhysicalModel` для badge

**Параметры:**
- `child` - Widget
- `label` - Widget (text/number)
- `isLabelVisible` - bool
- `backgroundColor` - Color
- `textColor` - Color
- `smallSize` - double
- `largeSize` - double
- `textStyle` - TextStyle
- `padding` - EdgeInsets
- `alignment` - AlignmentGeometry
- `offset` - Offset

---

## TabBar
```
📦 TabBar (Material tabs)
  └─ Material -> RenderPhysicalModel
      └─ Row (tabs) + Indicator -> RenderFlex + RenderDecoratedBox
          ├─ Tab 1
          ├─ Tab 2
          └─ Tab N
```

**RenderObject:** `RenderPhysicalModel` + `RenderFlex` + `RenderDecoratedBox` (indicator)

**Параметры:**
- `tabs` - List<Widget> (required)
- `controller` - TabController
- `isScrollable` - bool
- `padding` - EdgeInsets
- `indicatorColor` - Color
- `automaticIndicatorColorAdjustment` - bool
- `indicatorWeight` - double
- `indicatorPadding` - EdgeInsets
- `indicator` - Decoration
- `indicatorSize` - TabBarIndicatorSize
- `labelColor` - Color
- `labelStyle` - TextStyle
- `labelPadding` - EdgeInsets
- `unselectedLabelColor` - Color
- `unselectedLabelStyle` - TextStyle
- `dragStartBehavior` - DragStartBehavior
- `overlayColor` - MaterialStateProperty<Color?>
- `mouseCursor` - MouseCursor
- `enableFeedback` - bool
- `onTap` - void Function(int)
- `physics` - ScrollPhysics
- `splashFactory` - InteractiveInkFeatureFactory
- `splashBorderRadius` - BorderRadius
- `tabAlignment` - TabAlignment

---

## TabBarView
```
📦 TabBarView (Swipeable tab content)
  └─ NotificationListener
      └─ PageView (tabs content) -> RenderViewport
          ├─ Tab 1 content
          ├─ Tab 2 content
          └─ Tab N content
```

**RenderObject:** `RenderViewport` + `RenderSliverFillViewport`

**Параметры:**
- `children` - List<Widget> (required)
- `controller` - TabController
- `physics` - ScrollPhysics
- `dragStartBehavior` - DragStartBehavior
- `viewportFraction` - double
- `clipBehavior` - Clip

---

## ExpansionTile
```
📦 ExpansionTile (Expandable list item)
  └─ ListTile (header)
      ├─ Leading (icon)
      ├─ Title + Subtitle
      └─ Trailing (expand icon)
      └─ AnimatedCrossFade
          └─ Children (expanded content)
```

**RenderObject:** Комбинация из ListTile + AnimatedCrossFade RenderObjects

**Параметры:**
- `leading` - Widget
- `title` - Widget (required)
- `subtitle` - Widget
- `trailing` - Widget (expand icon)
- `children` - List<Widget> (expanded content)
- `onExpansionChanged` - void Function(bool)
- `initiallyExpanded` - bool
- `maintainState` - bool
- `tilePadding` - EdgeInsets
- `expandedCrossAxisAlignment` - CrossAxisAlignment
- `expandedAlignment` - Alignment
- `childrenPadding` - EdgeInsets
- `backgroundColor` - Color
- `collapsedBackgroundColor` - Color
- `textColor` - Color
- `collapsedTextColor` - Color
- `iconColor` - Color
- `collapsedIconColor` - Color
- `shape` - ShapeBorder
- `collapsedShape` - ShapeBorder
- `clipBehavior` - Clip
- `controlAffinity` - ListTileControlAffinity

---

## Material
```
📦 Material (Base Material widget)
  └─ AnimatedPhysicalModel -> RenderPhysicalModel или RenderPhysicalShape
      └─ InkFeatures -> RenderInkFeatures
          └─ Child Widget
```

**RenderObject:** `RenderPhysicalModel` или `RenderPhysicalShape` + `RenderInkFeatures`

**Параметры:**
- `type` - MaterialType (canvas, card, circle, button, transparency)
- `elevation` - double
- `color` - Color
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `textStyle` - TextStyle
- `borderRadius` - BorderRadius
- `shape` - ShapeBorder
- `borderOnForeground` - bool
- `clipBehavior` - Clip
- `animationDuration` - Duration
- `child` - Widget

**Применение:** Базовый виджет для Material Design, предоставляет elevation, ink effects, etc.
