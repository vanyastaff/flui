# 🎬 Scrolling Widgets (Прокрутка)

## SingleChildScrollView
```
📦 SingleChildScrollView
  └─ Scrollable -> RenderPointerListener
      └─ Viewport -> RenderViewport
          └─ ClipRect -> RenderClipRect
              └─ ScrollableBox
                  └─ Child Widget (scrollable)
```

**RenderObject:** `RenderViewport` + `RenderSliverToBoxAdapter`

**Параметры:**
- `scrollDirection` - Axis.vertical или horizontal
- `reverse` - реверс направления
- `padding` - EdgeInsets
- `primary` - использовать primary scroll controller
- `physics` - ScrollPhysics
- `controller` - ScrollController
- `dragStartBehavior` - DragStartBehavior
- `clipBehavior` - Clip
- `child` - дочерний виджет

---

## ListView
```
📦 ListView (Scrollable list)
  └─ Scrollable -> RenderPointerListener
      └─ Viewport -> RenderViewport
          └─ SliverList -> RenderSliverList
              ├─ Child 1 (lazy loaded)
              ├─ Child 2
              └─ Child N
```

**RenderObject:** `RenderViewport` + `RenderSliverList`

**Параметры:**
- `scrollDirection` - направление прокрутки
- `reverse` - реверс
- `controller` - ScrollController
- `primary` - primary controller
- `physics` - ScrollPhysics
- `shrinkWrap` - подгонка под контент
- `padding` - EdgeInsets
- `itemExtent` - фиксированная высота элементов
- `prototypeItem` - элемент для измерения высоты
- `children` - список виджетов

**Варианты:**
- `ListView()` - обычный список
- `ListView.builder()` - ленивый с builder
- `ListView.separated()` - с разделителями
- `ListView.custom()` - с custom SliverChildDelegate

---

### ListView.builder
```
📦 ListView.builder
  └─ Scrollable -> RenderPointerListener
      └─ Viewport -> RenderViewport
          └─ SliverList -> RenderSliverList
              └─ SliverChildBuilderDelegate
                  └─ itemBuilder(context, index) (ленивая загрузка)
```

**RenderObject:** `RenderViewport` + `RenderSliverList`

**Параметры:**
- `itemBuilder` - Widget Function(BuildContext, int)
- `itemCount` - количество элементов (optional)
- Остальные как у ListView

---

### ListView.separated
```
📦 ListView.separated
  └─ Scrollable -> RenderPointerListener
      └─ Viewport -> RenderViewport
          └─ SliverList -> RenderSliverList
              ├─ Item 1
              ├─ Separator 1
              ├─ Item 2
              ├─ Separator 2
              └─ ...
```

**RenderObject:** `RenderViewport` + `RenderSliverList`

**Параметры:**
- `itemBuilder` - Widget Function(BuildContext, int)
- `separatorBuilder` - Widget Function(BuildContext, int)
- `itemCount` - количество элементов (required)
- Остальные как у ListView

---

## GridView
```
📦 GridView (Scrollable grid)
  └─ Scrollable -> RenderPointerListener
      └─ Viewport -> RenderViewport
          └─ SliverGrid -> RenderSliverGrid
              ├─ [Child 1, Child 2, Child 3, ...]
              ├─ [Child 4, Child 5, Child 6, ...]
              └─ [...]
```

**RenderObject:** `RenderViewport` + `RenderSliverGrid`

**Параметры:**
- `gridDelegate` - SliverGridDelegate (определяет сетку)
- `scrollDirection` - направление прокрутки
- `reverse` - реверс
- `controller` - ScrollController
- `primary` - primary controller
- `physics` - ScrollPhysics
- `shrinkWrap` - подгонка
- `padding` - EdgeInsets
- `children` - список виджетов

**Варианты:**
- `GridView.count()` - фиксированное количество колонок
- `GridView.extent()` - фиксированный размер ячейки
- `GridView.builder()` - ленивая загрузка
- `GridView.custom()` - custom delegate

---

### GridView.count
```
📦 GridView.count (Fixed column count)
  └─ SliverGridDelegateWithFixedCrossAxisCount
      └─ Grid с фиксированным количеством колонок
```

**RenderObject:** `RenderViewport` + `RenderSliverGrid`

**Параметры:**
- `crossAxisCount` - количество колонок/рядов
- `mainAxisSpacing` - отступ по главной оси
- `crossAxisSpacing` - отступ по поперечной оси
- `childAspectRatio` - соотношение сторон ячейки
- `children` - список виджетов

---

### GridView.extent
```
📦 GridView.extent (Fixed cell size)
  └─ SliverGridDelegateWithMaxCrossAxisExtent
      └─ Grid с фиксированным размером ячеек
```

**RenderObject:** `RenderViewport` + `RenderSliverGrid`

**Параметры:**
- `maxCrossAxisExtent` - макс. размер по поперечной оси
- `mainAxisSpacing`, `crossAxisSpacing`, `childAspectRatio`
- `children` - список виджетов

---

## CustomScrollView
```
📦 CustomScrollView (Sliver-based scroll)
  └─ Scrollable -> RenderPointerListener
      └─ Viewport -> RenderViewport
          ├─ Sliver 1 (SliverAppBar, SliverList, etc.)
          ├─ Sliver 2
          └─ Sliver N
```

**RenderObject:** `RenderViewport` + различные RenderSliver*

**Параметры:**
- `slivers` - список Sliver виджетов
- `scrollDirection`, `reverse`, `controller`, `primary`, `physics`, `shrinkWrap`

**Популярные Slivers:**
- `SliverAppBar` -> `RenderSliverFloatingPersistentHeader`
- `SliverList` -> `RenderSliverList`
- `SliverGrid` -> `RenderSliverGrid`
- `SliverToBoxAdapter` -> `RenderSliverToBoxAdapter`
- `SliverFillRemaining` -> `RenderSliverFillRemaining`
- `SliverPadding` -> `RenderSliverPadding`
- `SliverPersistentHeader` -> `RenderSliverPersistentHeader`

---

## PageView
```
📦 PageView (Paginated scroll)
  └─ Scrollable (pageSnapping) -> RenderPointerListener
      └─ Viewport -> RenderViewport
          ├─ Page 1
          ├─ Page 2
          └─ Page N
```

**RenderObject:** `RenderViewport` + `RenderSliverFillViewport`

**Параметры:**
- `scrollDirection` - Axis.horizontal или vertical
- `reverse` - реверс
- `controller` - PageController
- `physics` - ScrollPhysics
- `pageSnapping` - snap к страницам
- `onPageChanged` - callback при смене страницы
- `children` - список страниц

**Варианты:**
- `PageView()` - обычный
- `PageView.builder()` - ленивый
- `PageView.custom()` - custom delegate

---

## ListWheelScrollView
```
📦 ListWheelScrollView (3D wheel effect)
  └─ Scrollable -> RenderPointerListener
      └─ ListWheelViewport -> RenderListWheelViewport
          ├─ Child 1 (3D transformed)
          ├─ Child 2 (центральный)
          └─ Child N (3D transformed)
```

**RenderObject:** `RenderListWheelViewport`

**Параметры:**
- `itemExtent` - высота каждого элемента (required)
- `diameterRatio` - отношение диаметра к viewport
- `perspective` - эффект перспективы
- `offAxisFraction` - смещение от оси
- `useMagnifier` - увеличение центрального элемента
- `magnification` - степень увеличения
- `squeeze` - сжатие элементов
- `controller` - FixedExtentScrollController
- `physics` - ScrollPhysics
- `children` - список виджетов

---

## NestedScrollView
```
📦 NestedScrollView (Nested scrolling)
  └─ Координация между header и body scroll
      ├─ headerSliverBuilder (коллапсируется)
      └─ body (scrollable)
```

**RenderObject:** `RenderViewport` + различные RenderSliver* для header/body

**Параметры:**
- `headerSliverBuilder` - Function(BuildContext, bool innerBoxIsScrolled)
- `body` - Widget (обычно scrollable)
- `controller`, `scrollDirection`, `reverse`, `physics`, etc.

---

## NotificationListener
```
📦 NotificationListener<T extends Notification>
  └─ Слушает notifications из дерева
      └─ Child Widget (источник notifications)
```

**RenderObject:** `RenderProxyBox` (не модифицирует rendering)

**Параметры:**
- `onNotification` - bool Function(T notification)
- `child` - дочерний виджет

**Популярные Notifications:**
- `ScrollNotification` (start, update, end, metrics)
- `SizeChangedLayoutNotification`
- `LayoutChangedNotification`
- `OverscrollNotification`

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

---

## Sliver Widgets (для CustomScrollView)

### SliverAppBar
**RenderObject:** `RenderSliverFloatingPersistentHeader` или `RenderSliverPinnedPersistentHeader`

### SliverList
**RenderObject:** `RenderSliverList`

### SliverGrid
**RenderObject:** `RenderSliverGrid`

### SliverToBoxAdapter
**RenderObject:** `RenderSliverToBoxAdapter`

### SliverFillRemaining
**RenderObject:** `RenderSliverFillRemaining`

### SliverPadding
**RenderObject:** `RenderSliverPadding`

### SliverPersistentHeader
**RenderObject:** `RenderSliverPersistentHeader`

### SliverFixedExtentList
**RenderObject:** `RenderSliverFixedExtentList`

### SliverPrototypeExtentList
**RenderObject:** `RenderSliverPrototypeExtentList`

### SliverOpacity
**RenderObject:** `RenderSliverOpacity`

### SliverIgnorePointer
**RenderObject:** `RenderSliverIgnorePointer`

### SliverOffstage
**RenderObject:** `RenderSliverOffstage`
