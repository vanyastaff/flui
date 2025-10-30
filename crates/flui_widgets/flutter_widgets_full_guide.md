# 📚 Полный справочник виджетов Flutter

## 🎨 Layout Widgets (Виджеты размещения)

### 1. Basic Layout (Базовое размещение)

#### Container
```
📦 Container (изнутри → наружу)
  └─ Align (alignment)
      └─ Padding (padding)
          └─ DecoratedBox (decoration)
              └─ ConstrainedBox (constraints)
                  └─ Transform (transform)
                      └─ Padding (margin)
                          └─ Child Widget
```

**Параметры:**
- `alignment` - выравнивание ребенка
- `padding` - внутренние отступы
- `decoration` - фон, границы, тени
- `constraints` - ограничения размера
- `margin` - внешние отступы
- `transform` - трансформация
- `child` - дочерний виджет

#### SizedBox
```
📦 SizedBox
  └─ ConstrainedBox (width/height)
      └─ Child Widget (или пусто)
```

**Параметры:**
- `width` - фиксированная ширина
- `height` - фиксированная высота
- `child` - дочерний виджет (опционально)

**Варианты:**
- `SizedBox.expand()` - занимает все доступное пространство
- `SizedBox.shrink()` - нулевой размер
- `SizedBox.square()` - квадрат

#### Padding
```
📦 Padding
  └─ Padding (изнутри)
      └─ Child Widget
```

**Параметры:**
- `padding` - EdgeInsets (all, symmetric, only, fromLTRB)
- `child` - дочерний виджет

#### Center
```
📦 Center
  └─ Align (alignment: Alignment.center)
      └─ Child Widget
```

**Параметры:**
- `widthFactor` - множитель ширины от ребенка
- `heightFactor` - множитель высоты от ребенка
- `child` - дочерний виджет

#### Align
```
📦 Align
  └─ CustomSingleChildLayout
      └─ Child Widget (позиционирован)
```

**Параметры:**
- `alignment` - позиция (Alignment.topLeft, center, etc.)
- `widthFactor` - множитель ширины
- `heightFactor` - множитель высоты
- `child` - дочерний виджет

#### FittedBox
```
📦 FittedBox
  └─ Transform (scale для подгонки)
      └─ ClipRect (если clipBehavior != none)
          └─ Child Widget (масштабирован)
```

**Параметры:**
- `fit` - BoxFit (fill, contain, cover, fitWidth, fitHeight, none, scaleDown)
- `alignment` - выравнивание после подгонки
- `clipBehavior` - обрезка краев

#### AspectRatio
```
📦 AspectRatio
  └─ ConstrainedBox (поддерживает aspectRatio)
      └─ Child Widget
```

**Параметры:**
- `aspectRatio` - соотношение сторон (width/height)
- `child` - дочерний виджет

#### ConstrainedBox
```
📦 ConstrainedBox
  └─ Constraints (min/max width/height)
      └─ Child Widget
```

**Параметры:**
- `constraints` - BoxConstraints
- `child` - дочерний виджет

#### LimitedBox
```
📦 LimitedBox
  └─ ConstrainedBox (только если родитель unbounded)
      └─ Child Widget
```

**Параметры:**
- `maxWidth` - макс. ширина если родитель unbounded
- `maxHeight` - макс. высота если родитель unbounded
- `child` - дочерний виджет

#### FractionallySizedBox
```
📦 FractionallySizedBox
  └─ Align (alignment)
      └─ ConstrainedBox (размер = родитель × factor)
          └─ Child Widget
```

**Параметры:**
- `widthFactor` - процент от ширины родителя (0.0-1.0)
- `heightFactor` - процент от высоты родителя (0.0-1.0)
- `alignment` - выравнивание
- `child` - дочерний виджет

#### Baseline
```
📦 Baseline
  └─ CustomSingleChildLayout (выравнивание по baseline)
      └─ Child Widget
```

**Параметры:**
- `baseline` - расстояние от верха
- `baselineType` - alphabetic или ideographic
- `child` - дочерний виджет

#### OverflowBox
```
📦 OverflowBox
  └─ Игнорирует constraints родителя
      └─ Child Widget (может выходить за границы)
```

**Параметры:**
- `minWidth`, `maxWidth` - новые constraints
- `minHeight`, `maxHeight` - новые constraints
- `alignment` - выравнивание
- `child` - дочерний виджет

#### SizedOverflowBox
```
📦 SizedOverflowBox
  └─ Фиксированный размер + дети могут overflow
      └─ Child Widget
```

**Параметры:**
- `size` - фиксированный размер виджета
- `alignment` - выравнивание ребенка
- `child` - дочерний виджет

#### Offstage
```
📦 Offstage
  └─ (рендерит, но не показывает если offstage=true)
      └─ Child Widget (invisible)
```

**Параметры:**
- `offstage` - если true, не показывается
- `child` - дочерний виджет

#### Visibility
```
📦 Visibility
  └─ Conditional rendering
      └─ Child Widget (или replacement)
```

**Параметры:**
- `visible` - показывать или нет
- `replacement` - виджет при invisible
- `maintainState` - сохранять state
- `maintainAnimation` - сохранять анимации
- `maintainSize` - сохранять размер
- `maintainSemantics` - сохранять семантику
- `maintainInteractivity` - сохранять интерактивность
- `child` - дочерний виджет

---

### 2. Flex Layout (Гибкое размещение)

#### Row
```
📦 Row (Horizontal Flex)
  └─ RenderFlex (direction: Axis.horizontal)
      ├─ Child 1 (с FlexParentData)
      ├─ Child 2 (с FlexParentData)
      └─ Child N (с FlexParentData)
```

**Параметры:**
- `mainAxisAlignment` - выравнивание по главной оси
- `crossAxisAlignment` - выравнивание по поперечной оси
- `mainAxisSize` - max или min
- `verticalDirection` - down или up
- `textDirection` - ltr или rtl
- `textBaseline` - alphabetic или ideographic
- `children` - список виджетов

**MainAxisAlignment:**
- `start` - в начале
- `end` - в конце
- `center` - по центру
- `spaceBetween` - равномерно, без отступов по краям
- `spaceAround` - равномерно, с половинными отступами по краям
- `spaceEvenly` - равномерно, с полными отступами по краям

**CrossAxisAlignment:**
- `start` - в начале
- `end` - в конце
- `center` - по центру
- `stretch` - растянуть
- `baseline` - по базовой линии текста

#### Column
```
📦 Column (Vertical Flex)
  └─ RenderFlex (direction: Axis.vertical)
      ├─ Child 1 (с FlexParentData)
      ├─ Child 2 (с FlexParentData)
      └─ Child N (с FlexParentData)
```

**Параметры:** Те же что у Row

#### Flexible
```
📦 Flexible
  └─ FlexParentData (flex factor, fit)
      └─ Child Widget (может расширяться)
```

**Параметры:**
- `flex` - фактор гибкости (по умолчанию 1)
- `fit` - FlexFit.tight или FlexFit.loose
- `child` - дочерний виджет

#### Expanded
```
📦 Expanded = Flexible(fit: FlexFit.tight)
  └─ FlexParentData (flex factor, fit: tight)
      └─ Child Widget (заполняет доступное место)
```

**Параметры:**
- `flex` - фактор расширения (по умолчанию 1)
- `child` - дочерний виджет

#### Spacer
```
📦 Spacer = Expanded(child: SizedBox.shrink())
  └─ Пустое пространство с flex factor
```

**Параметры:**
- `flex` - фактор расширения (по умолчанию 1)

#### Flex
```
📦 Flex (Generic flex container)
  └─ RenderFlex (direction настраивается)
      └─ Children (с FlexParentData)
```

**Параметры:**
- `direction` - Axis.horizontal или Axis.vertical
- Остальные как у Row/Column

---

### 3. Stack Layout (Наложение слоями)

#### Stack
```
📦 Stack
  └─ RenderStack
      ├─ Child 1 (внизу, с StackParentData)
      ├─ Child 2 (выше, с StackParentData)
      └─ Child N (сверху, с StackParentData)
```

**Параметры:**
- `alignment` - выравнивание не-positioned детей
- `fit` - StackFit.loose, expand, passthrough
- `clipBehavior` - обрезка overflow
- `textDirection` - для directional alignment
- `children` - список виджетов (порядок = Z-order)

#### Positioned
```
📦 Positioned (только внутри Stack!)
  └─ StackParentData (top/left/right/bottom)
      └─ Child Widget (позиционирован абсолютно)
```

**Параметры:**
- `left` - отступ слева
- `top` - отступ сверху
- `right` - отступ справа
- `bottom` - отступ снизу
- `width` - ширина (нельзя с left+right)
- `height` - высота (нельзя с top+bottom)
- `child` - дочерний виджет

**Варианты:**
- `Positioned.fill()` - на весь Stack
- `Positioned.directional()` - с учетом textDirection
- `Positioned.fromRect()` - из Rect
- `Positioned.fromRelativeRect()` - из RelativeRect

#### PositionedDirectional
```
📦 PositionedDirectional (учитывает textDirection)
  └─ Positioned (auto-converts start/end)
      └─ Child Widget
```

**Параметры:**
- `start` - отступ от начала (left для LTR)
- `end` - отступ от конца (right для LTR)
- `top`, `bottom`, `width`, `height` - как у Positioned
- `child` - дочерний виджет

#### IndexedStack
```
📦 IndexedStack (показывает только один child)
  └─ RenderIndexedStack
      ├─ Child 0 (visible если index=0)
      ├─ Child 1 (visible если index=1)
      └─ Child N (visible если index=N)
```

**Параметры:**
- `index` - индекс видимого ребенка
- `alignment` - выравнивание
- `sizing` - StackFit (loose, expand, passthrough)
- `children` - список виджетов

---

### 4. Multi-Child Layout (Множественные дети)

#### Wrap
```
📦 Wrap (Flow-like layout)
  └─ RenderWrap
      ├─ Row/Column 1: [Child 1, Child 2, ...]
      ├─ Row/Column 2: [Child N, ...]
      └─ Row/Column M: [...]
```

**Параметры:**
- `direction` - Axis.horizontal или vertical
- `alignment` - WrapAlignment для главной оси
- `spacing` - отступ между детьми на одной линии
- `runAlignment` - WrapAlignment между линиями
- `runSpacing` - отступ между линиями
- `crossAxisAlignment` - выравнивание по cross-axis
- `textDirection` - для directional alignment
- `verticalDirection` - down или up
- `clipBehavior` - обрезка overflow
- `children` - список виджетов

#### Flow
```
📦 Flow (Custom positioned children)
  └─ RenderFlow
      └─ FlowDelegate (custom positioning logic)
          └─ Children (позиционированы delegate)
```

**Параметры:**
- `delegate` - FlowDelegate (определяет позиции)
- `children` - список виджетов
- `clipBehavior` - обрезка overflow

#### ListBody
```
📦 ListBody (Simple vertical/horizontal list)
  └─ RenderListBody
      ├─ Child 1 (positioned sequentially)
      ├─ Child 2
      └─ Child N
```

**Параметры:**
- `mainAxis` - Axis.vertical или horizontal
- `reverse` - реверс порядка
- `children` - список виджетов

#### Table
```
📦 Table
  └─ RenderTable
      ├─ TableRow 1: [TableCell 1, TableCell 2, ...]
      ├─ TableRow 2: [TableCell 1, TableCell 2, ...]
      └─ TableRow N: [...]
```

**Параметры:**
- `children` - список TableRow
- `columnWidths` - Map<int, TableColumnWidth>
- `defaultColumnWidth` - ширина колонок по умолчанию
- `textDirection` - для directional layout
- `border` - TableBorder
- `defaultVerticalAlignment` - TableCellVerticalAlignment
- `textBaseline` - для baseline alignment

#### TableRow
```
📦 TableRow (только внутри Table!)
  └─ List<Widget> (TableCells)
```

**Параметры:**
- `decoration` - BoxDecoration для строки
- `children` - список виджетов (ячеек)

#### TableCell
```
📦 TableCell (обертка с настройками для ячейки)
  └─ TableCellParentData
      └─ Child Widget
```

**Параметры:**
- `verticalAlignment` - TableCellVerticalAlignment
- `child` - дочерний виджет

#### CustomMultiChildLayout
```
📦 CustomMultiChildLayout
  └─ RenderCustomMultiChildLayoutBox
      └─ MultiChildLayoutDelegate (custom logic)
          └─ Children (с LayoutId)
```

**Параметры:**
- `delegate` - MultiChildLayoutDelegate
- `children` - список виджетов с LayoutId

#### LayoutId
```
📦 LayoutId (метка для child в CustomMultiChildLayout)
  └─ MultiChildLayoutParentData (id)
      └─ Child Widget
```

**Параметры:**
- `id` - Object (любой ключ)
- `child` - дочерний виджет

---

## 🎬 Scrolling Widgets (Прокрутка)

### SingleChildScrollView
```
📦 SingleChildScrollView
  └─ Scrollable
      └─ Viewport
          └─ ClipRect
              └─ ScrollableBox
                  └─ Child Widget (scrollable)
```

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

### ListView
```
📦 ListView (Scrollable list)
  └─ Scrollable
      └─ Viewport
          └─ SliverList
              ├─ Child 1 (lazy loaded)
              ├─ Child 2
              └─ Child N
```

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

#### ListView.builder
```
📦 ListView.builder
  └─ Scrollable
      └─ Viewport
          └─ SliverList
              └─ SliverChildBuilderDelegate
                  └─ itemBuilder(context, index) (ленивая загрузка)
```

**Параметры:**
- `itemBuilder` - Widget Function(BuildContext, int)
- `itemCount` - количество элементов (optional)
- Остальные как у ListView

#### ListView.separated
```
📦 ListView.separated
  └─ Scrollable
      └─ Viewport
          └─ SliverList
              ├─ Item 1
              ├─ Separator 1
              ├─ Item 2
              ├─ Separator 2
              └─ ...
```

**Параметры:**
- `itemBuilder` - Widget Function(BuildContext, int)
- `separatorBuilder` - Widget Function(BuildContext, int)
- `itemCount` - количество элементов (required)
- Остальные как у ListView

### GridView
```
📦 GridView (Scrollable grid)
  └─ Scrollable
      └─ Viewport
          └─ SliverGrid
              ├─ [Child 1, Child 2, Child 3, ...]
              ├─ [Child 4, Child 5, Child 6, ...]
              └─ [...]
```

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

#### GridView.count
```
📦 GridView.count (Fixed column count)
  └─ SliverGridDelegateWithFixedCrossAxisCount
      └─ Grid с фиксированным количеством колонок
```

**Параметры:**
- `crossAxisCount` - количество колонок/рядов
- `mainAxisSpacing` - отступ по главной оси
- `crossAxisSpacing` - отступ по поперечной оси
- `childAspectRatio` - соотношение сторон ячейки
- `children` - список виджетов

#### GridView.extent
```
📦 GridView.extent (Fixed cell size)
  └─ SliverGridDelegateWithMaxCrossAxisExtent
      └─ Grid с фиксированным размером ячеек
```

**Параметры:**
- `maxCrossAxisExtent` - макс. размер по поперечной оси
- `mainAxisSpacing`, `crossAxisSpacing`, `childAspectRatio`
- `children` - список виджетов

### CustomScrollView
```
📦 CustomScrollView (Sliver-based scroll)
  └─ Scrollable
      └─ Viewport
          ├─ Sliver 1 (SliverAppBar, SliverList, etc.)
          ├─ Sliver 2
          └─ Sliver N
```

**Параметры:**
- `slivers` - список Sliver виджетов
- `scrollDirection`, `reverse`, `controller`, `primary`, `physics`, `shrinkWrap`

**Популярные Slivers:**
- `SliverAppBar` - коллапсирующий app bar
- `SliverList` - список
- `SliverGrid` - сетка
- `SliverToBoxAdapter` - обычный виджет в sliver
- `SliverFillRemaining` - заполняет оставшееся место
- `SliverPadding` - padding для sliver
- `SliverPersistentHeader` - sticky header

### PageView
```
📦 PageView (Paginated scroll)
  └─ Scrollable (pageSnapping)
      └─ Viewport
          ├─ Page 1
          ├─ Page 2
          └─ Page N
```

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

### ListWheelScrollView
```
📦 ListWheelScrollView (3D wheel effect)
  └─ Scrollable
      └─ ListWheelViewport
          └─ RenderListWheelViewport
              ├─ Child 1 (3D transformed)
              ├─ Child 2 (центральный)
              └─ Child N (3D transformed)
```

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

### NestedScrollView
```
📦 NestedScrollView (Nested scrolling)
  └─ Координация между header и body scroll
      ├─ headerSliverBuilder (коллапсируется)
      └─ body (scrollable)
```

**Параметры:**
- `headerSliverBuilder` - Function(BuildContext, bool innerBoxIsScrolled)
- `body` - Widget (обычно scrollable)
- `controller`, `scrollDirection`, `reverse`, `physics`, etc.

### NotificationListener
```
📦 NotificationListener<T extends Notification>
  └─ Слушает notifications из дерева
      └─ Child Widget (источник notifications)
```

**Параметры:**
- `onNotification` - bool Function(T notification)
- `child` - дочерний виджет

**Популярные Notifications:**
- `ScrollNotification` (start, update, end, metrics)
- `SizeChangedLayoutNotification`
- `LayoutChangedNotification`
- `OverscrollNotification`

---

## ✏️ Text Widgets (Текстовые виджеты)

### Text
```
📦 Text
  └─ RichText
      └─ TextSpan (single style)
          └─ Rendered text
```

**Параметры:**
- `data` - String (текст)
- `style` - TextStyle
- `textAlign` - TextAlign
- `textDirection` - TextDirection
- `softWrap` - перенос строк
- `overflow` - TextOverflow (clip, fade, ellipsis, visible)
- `textScaler` - масштабирование текста
- `maxLines` - максимум строк
- `semanticsLabel` - метка для accessibility
- `textWidthBasis` - TextWidthBasis

**Варианты:**
- `Text()` - обычный текст
- `Text.rich()` - с TextSpan

### RichText
```
📦 RichText (Multi-style text)
  └─ RenderParagraph
      └─ TextSpan (tree of styled spans)
          ├─ TextSpan 1 (style 1)
          ├─ TextSpan 2 (style 2)
          └─ WidgetSpan (встроенный виджет)
```

**Параметры:**
- `text` - InlineSpan (TextSpan tree)
- `textAlign`, `textDirection`, `softWrap`, `overflow`, `maxLines`, etc.
- `textScaler` - масштабирование
- `strutStyle` - минимальная высота строки

### TextSpan
```
📦 TextSpan (Styled text fragment)
  └─ InlineSpan
      ├─ text: String (опционально)
      ├─ style: TextStyle (опционально)
      ├─ children: List<InlineSpan> (опционально)
      └─ recognizer: GestureRecognizer (опционально)
```

**Параметры:**
- `text` - текст этого span
- `style` - TextStyle для этого span
- `children` - вложенные InlineSpan
- `recognizer` - TapGestureRecognizer, etc.
- `semanticsLabel` - для accessibility
- `locale` - Locale
- `spellOut` - произносить побуквенно

### WidgetSpan
```
📦 WidgetSpan (Widget внутри RichText)
  └─ InlineSpan
      └─ Embedded Widget (baseline-aligned)
```

**Параметры:**
- `child` - Widget для встраивания
- `alignment` - PlaceholderAlignment
- `baseline` - TextBaseline
- `style` - TextStyle (для контекста)

### SelectableText
```
📦 SelectableText (Selectable text)
  └─ EditableText (readOnly: true)
      └─ Selectable RenderParagraph
```

**Параметры:**
- `data` - String
- `style` - TextStyle
- `textAlign`, `textDirection`, `maxLines`, etc.
- `cursorColor` - цвет курсора при выделении
- `showCursor` - показывать курсор
- `selectionControls` - кастомные controls
- `onSelectionChanged` - callback при выделении

**Варианты:**
- `SelectableText()`
- `SelectableText.rich()` - с TextSpan

### DefaultTextStyle
```
📦 DefaultTextStyle (Inherited text style)
  └─ InheritedTheme
      └─ Children (наследуют style)
```

**Параметры:**
- `style` - TextStyle по умолчанию
- `textAlign` - выравнивание по умолчанию
- `softWrap` - перенос по умолчанию
- `overflow` - overflow по умолчанию
- `maxLines` - maxLines по умолчанию
- `textWidthBasis` - basis по умолчанию
- `textHeightBehavior` - behavior по умолчанию
- `child` - дочерний виджет

### TextStyle
```
📦 TextStyle (Text styling data)
  └─ Immutable configuration
      ├─ Color (color, backgroundColor)
      ├─ Font (fontFamily, fontSize, fontWeight, fontStyle)
      ├─ Decoration (decoration, decorationColor, decorationStyle)
      ├─ Spacing (letterSpacing, wordSpacing, height)
      ├─ Shadows (shadows)
      └─ Features (fontFeatures, fontVariations)
```

**Параметры:**
- **Цвет:** `color`, `backgroundColor`
- **Шрифт:** `fontFamily`, `fontSize`, `fontWeight`, `fontStyle`
- **Декорация:** `decoration`, `decorationColor`, `decorationStyle`, `decorationThickness`
- **Межстрочный:** `height`, `leadingDistribution`
- **Межбуквенный:** `letterSpacing`, `wordSpacing`
- **Тени:** `shadows`
- **Продвинутое:** `fontFeatures`, `fontVariations`, `locale`, `overflow`

---

## 🖼️ Image Widgets (Изображения)

### Image
```
📦 Image
  └─ RawImage
      └─ ImageProvider (loads image)
          └─ RenderImage (paints image)
```

**Параметры:**
- `image` - ImageProvider
- `width`, `height` - размеры
- `fit` - BoxFit
- `alignment` - Alignment
- `repeat` - ImageRepeat
- `color` - tint color
- `colorBlendMode` - BlendMode
- `filterQuality` - FilterQuality
- `semanticLabel` - для accessibility
- `excludeFromSemantics` - исключить из semantics

**Варианты:**
- `Image.asset()` - из assets
- `Image.network()` - из URL
- `Image.file()` - из File
- `Image.memory()` - из Uint8List

#### Image.asset
```
📦 Image.asset (Asset image)
  └─ AssetImage (provider)
      └─ Load from bundle
```

**Параметры:**
- `name` - String (путь в assets)
- `bundle` - AssetBundle (optional)
- `package` - для package assets
- `width`, `height`, `fit`, `alignment`, etc.

#### Image.network
```
📦 Image.network (Network image)
  └─ NetworkImage (provider)
      └─ HTTP request + cache
```

**Параметры:**
- `src` - String (URL)
- `scale` - масштаб изображения
- `headers` - HTTP headers
- `width`, `height`, `fit`, `alignment`, etc.
- `loadingBuilder` - Widget при загрузке
- `errorBuilder` - Widget при ошибке

#### Image.file
```
📦 Image.file (File image)
  └─ FileImage (provider)
      └─ Load from filesystem
```

**Параметры:**
- `file` - File
- `scale` - масштаб
- `width`, `height`, `fit`, `alignment`, etc.

#### Image.memory
```
📦 Image.memory (Memory image)
  └─ MemoryImage (provider)
      └─ Decode from bytes
```

**Параметры:**
- `bytes` - Uint8List
- `scale` - масштаб
- `width`, `height`, `fit`, `alignment`, etc.

### RawImage
```
📦 RawImage (Low-level image)
  └─ RenderImage
      └─ dart:ui Image (already decoded)
```

**Параметры:**
- `image` - ui.Image (decoded)
- `width`, `height`, `fit`, `alignment`, `repeat`, `color`, `colorBlendMode`, `filterQuality`

### Icon
```
📦 Icon
  └─ RichText (uses icon font)
      └─ TextSpan (icon glyph)
```

**Параметры:**
- `icon` - IconData
- `size` - размер иконки
- `color` - цвет
- `semanticLabel` - для accessibility
- `textDirection` - для directional icons

### IconTheme
```
📦 IconTheme (Inherited icon theme)
  └─ InheritedTheme
      └─ Children (наследуют IconThemeData)
```

**Параметры:**
- `data` - IconThemeData (color, size, opacity)
- `child` - дочерний виджет

### ImageIcon
```
📦 ImageIcon (Image as icon)
  └─ Image с ShaderMask
      └─ ImageProvider (used as icon)
```

**Параметры:**
- `image` - ImageProvider
- `size` - размер
- `color` - цвет (tint)
- `semanticLabel` - для accessibility

---

## 🎨 Visual Effects Widgets (Визуальные эффекты)

### Opacity
```
📦 Opacity
  └─ RenderOpacity
      └─ Child Widget (transparent)
```

**Параметры:**
- `opacity` - double (0.0 - 1.0)
- `alwaysIncludeSemantics` - сохранять semantics
- `child` - дочерний виджет

### Transform
```
📦 Transform
  └─ RenderTransform
      └─ Matrix4 transformation
          └─ Child Widget (transformed)
```

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

#### Transform.rotate
```
📦 Transform.rotate
  └─ Matrix4 (rotation)
      └─ Child Widget (rotated)
```

**Параметры:**
- `angle` - double (в радианах)
- `origin`, `alignment`, `transformHitTests`, `filterQuality`
- `child` - дочерний виджет

#### Transform.translate
```
📦 Transform.translate
  └─ Matrix4 (translation)
      └─ Child Widget (offset)
```

**Параметры:**
- `offset` - Offset
- `transformHitTests`, `filterQuality`
- `child` - дочерний виджет

#### Transform.scale
```
📦 Transform.scale
  └─ Matrix4 (scale)
      └─ Child Widget (scaled)
```

**Параметры:**
- `scale` - double (uniform scale)
- `scaleX`, `scaleY` - double (non-uniform)
- `origin`, `alignment`, `transformHitTests`, `filterQuality`
- `child` - дочерний виджет

### RotatedBox
```
📦 RotatedBox (90° increments only)
  └─ RenderRotatedBox
      └─ Child Widget (rotated 0/90/180/270°)
```

**Параметры:**
- `quarterTurns` - int (0, 1, 2, 3, ...)
- `child` - дочерний виджет

### ClipRect
```
📦 ClipRect (Rectangular clip)
  └─ RenderClipRect
      └─ Child Widget (clipped to bounds)
```

**Параметры:**
- `clipper` - CustomClipper<Rect> (optional)
- `clipBehavior` - Clip (hardEdge, antiAlias, antiAliasWithSaveLayer)
- `child` - дочерний виджет

### ClipRRect
```
📦 ClipRRect (Rounded rectangular clip)
  └─ RenderClipRRect
      └─ Child Widget (clipped with rounded corners)
```

**Параметры:**
- `borderRadius` - BorderRadius
- `clipper` - CustomClipper<RRect> (optional)
- `clipBehavior` - Clip
- `child` - дочерний виджет

### ClipOval
```
📦 ClipOval (Oval/circular clip)
  └─ RenderClipOval
      └─ Child Widget (clipped to oval)
```

**Параметры:**
- `clipper` - CustomClipper<Rect> (optional)
- `clipBehavior` - Clip
- `child` - дочерний виджет

### ClipPath
```
📦 ClipPath (Custom path clip)
  └─ RenderClipPath
      └─ CustomClipper<Path>
          └─ Child Widget (clipped to custom path)
```

**Параметры:**
- `clipper` - CustomClipper<Path> (required)
- `clipBehavior` - Clip
- `child` - дочерний виджет

### BackdropFilter
```
📦 BackdropFilter (Blur/filter backdrop)
  └─ RenderBackdropFilter
      └─ ImageFilter
          └─ Child Widget (поверх filtered backdrop)
```

**Параметры:**
- `filter` - ImageFilter (blur, matrix)
- `blendMode` - BlendMode
- `child` - дочерний виджет

### DecoratedBox
```
📦 DecoratedBox
  └─ RenderDecoratedBox
      └─ Decoration (background, border, shadow)
          └─ Child Widget
```

**Параметры:**
- `decoration` - Decoration (BoxDecoration, ShapeDecoration, etc.)
- `position` - DecorationPosition (background, foreground)
- `child` - дочерний виджет

### ColorFiltered
```
📦 ColorFiltered (Color filter)
  └─ RenderColorFiltered
      └─ ColorFilter
          └─ Child Widget (with color filter)
```

**Параметры:**
- `colorFilter` - ColorFilter (mode, matrix, etc.)
- `child` - дочерний виджет

### ShaderMask
```
📦 ShaderMask (Gradient mask)
  └─ RenderShaderMask
      └─ Shader
          └─ Child Widget (masked by shader)
```

**Параметры:**
- `shaderCallback` - Shader Function(Bounds)
- `blendMode` - BlendMode
- `child` - дочерний виджет

### RepaintBoundary
```
📦 RepaintBoundary (Isolate repaints)
  └─ RenderRepaintBoundary
      └─ Child Widget (в отдельном layer)
```

**Параметры:**
- `child` - дочерний виджет

**Применение:** Оптимизация - ребенок перерисовывается независимо

---

## 🖱️ Interaction Widgets (Интерактивность)

### GestureDetector
```
📦 GestureDetector
  └─ RenderPointerListener
      └─ Gesture Arena (recognizers)
          └─ Child Widget (interactive)
```

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

### InkWell
```
📦 InkWell (Material ripple effect)
  └─ Material (required ancestor!)
      └─ InkResponse
          └─ Ripple animation on tap
              └─ Child Widget
```

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

### InkResponse
```
📦 InkResponse (Customizable InkWell)
  └─ Material (required!)
      └─ Ripple + Highlight
          └─ Child Widget
```

**Параметры:** Те же что у InkWell + дополнительные:
- `containedInkWell` - ограничить ripple bounds
- `highlightShape` - BoxShape
- `radius` - радиус ripple
- `splashFactory` - InteractiveInkFeatureFactory

### Listener
```
📦 Listener (Raw pointer events)
  └─ RenderPointerListener
      └─ Child Widget (receives pointer events)
```

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

### MouseRegion
```
📦 MouseRegion (Mouse events)
  └─ RenderMouseRegion
      └─ Child Widget (mouse-aware)
```

**Параметры:**
- `onEnter` - PointerEnterEvent
- `onExit` - PointerExitEvent
- `onHover` - PointerHoverEvent
- `cursor` - MouseCursor
- `opaque` - блокировать события для родителей
- `child` - дочерний виджет

### AbsorbPointer
```
📦 AbsorbPointer (Block pointer events)
  └─ RenderAbsorbPointer
      └─ Child Widget (не получает события)
```

**Параметры:**
- `absorbing` - bool (если true, блокирует события)
- `ignoringSemantics` - игнорировать semantics
- `child` - дочерний виджет

### IgnorePointer
```
📦 IgnorePointer (Ignore pointer events)
  └─ RenderIgnorePointer
      └─ Child Widget (пропускает события дальше)
```

**Параметры:**
- `ignoring` - bool (если true, игнорирует события)
- `ignoringSemantics` - игнорировать semantics
- `child` - дочерний виджет

**Отличие от AbsorbPointer:** IgnorePointer пропускает события к виджетам позади, AbsorbPointer - нет

### Draggable
```
📦 Draggable<T> (Draggable widget)
  └─ GestureDetector (drag detection)
      ├─ child (when not dragging)
      └─ feedback (dragging overlay)
```

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

### LongPressDraggable
```
📦 LongPressDraggable<T> (Long press to drag)
  └─ Draggable (delay: long press duration)
      └─ ...
```

**Параметры:** Те же что у Draggable + `hapticFeedbackOnStart`

### DragTarget
```
📦 DragTarget<T> (Drop zone)
  └─ MetaData
      └─ Builder (candidateData, rejectedData)
          └─ Child Widget (rendered by builder)
```

**Параметры:**
- `builder` - Widget Function(BuildContext, List<T?> candidateData, List<dynamic> rejectedData)
- `onWillAcceptWithDetails` - bool Function(DragTargetDetails<T>)
- `onAcceptWithDetails` - void Function(DragTargetDetails<T>)
- `onLeave` - void Function(T?)
- `onMove` - void Function(DragTargetDetails<T>)
- `hitTestBehavior` - HitTestBehavior

### Dismissible
```
📦 Dismissible (Swipe to dismiss)
  └─ GestureDetector (drag)
      └─ SlideTransition
          ├─ background (показывается при swipe)
          └─ child (dismissable widget)
```

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

### InteractiveViewer
```
📦 InteractiveViewer (Pan, zoom)
  └─ GestureDetector
      └─ Transform (panEnabled, scaleEnabled)
          └─ Child Widget (zoomable)
```

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

### Scrollbar
```
📦 Scrollbar (Visual scrollbar)
  └─ RawScrollbar
      └─ Scrollable (required child!)
          └─ ScrollView + thumb overlay
```

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

## 🎬 Animation Widgets (Анимированные виджеты)

### AnimatedContainer
```
📦 AnimatedContainer
  └─ ImplicitlyAnimatedWidget
      └─ Animates Container properties
          └─ Container (с анимированными параметрами)
```

**Параметры:**
- Все параметры Container
- `duration` - Duration
- `curve` - Curve
- `onEnd` - VoidCallback

### AnimatedPadding
```
📦 AnimatedPadding
  └─ ImplicitlyAnimatedWidget
      └─ Animates padding
          └─ Padding (с анимированным padding)
```

**Параметры:**
- `padding` - EdgeInsets (target)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedAlign
```
📦 AnimatedAlign
  └─ ImplicitlyAnimatedWidget
      └─ Animates alignment
          └─ Align (с анимированным alignment)
```

**Параметры:**
- `alignment` - AlignmentGeometry (target)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedPositioned
```
📦 AnimatedPositioned (только в Stack!)
  └─ ImplicitlyAnimatedWidget
      └─ Animates position
          └─ Positioned (с анимированными left/top/right/bottom)
```

**Параметры:**
- `left`, `top`, `right`, `bottom`, `width`, `height` (target)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedOpacity
```
📦 AnimatedOpacity
  └─ ImplicitlyAnimatedWidget
      └─ Animates opacity
          └─ Opacity (с анимированной opacity)
```

**Параметры:**
- `opacity` - double (target 0.0-1.0)
- `duration`, `curve`, `onEnd`
- `alwaysIncludeSemantics`
- `child` - дочерний виджет

### AnimatedRotation
```
📦 AnimatedRotation
  └─ ImplicitlyAnimatedWidget
      └─ Animates rotation
          └─ Transform.rotate (с анимированным углом)
```

**Параметры:**
- `turns` - double (0.0 = 0°, 0.5 = 180°, 1.0 = 360°)
- `alignment` - Alignment
- `filterQuality` - FilterQuality
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedScale
```
📦 AnimatedScale
  └─ ImplicitlyAnimatedWidget
      └─ Animates scale
          └─ Transform.scale (с анимированным scale)
```

**Параметры:**
- `scale` - double (target scale)
- `alignment` - Alignment
- `filterQuality` - FilterQuality
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedSlide
```
📦 AnimatedSlide
  └─ ImplicitlyAnimatedWidget
      └─ Animates offset
          └─ FractionalTranslation (с анимированным offset)
```

**Параметры:**
- `offset` - Offset (fractional offset, 1.0 = size)
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedDefaultTextStyle
```
📦 AnimatedDefaultTextStyle
  └─ ImplicitlyAnimatedWidget
      └─ Animates text style
          └─ DefaultTextStyle (с анимированным style)
```

**Параметры:**
- `style` - TextStyle (target)
- `textAlign` - TextAlign
- `softWrap` - bool
- `overflow` - TextOverflow
- `maxLines` - int
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedPhysicalModel
```
📦 AnimatedPhysicalModel
  └─ ImplicitlyAnimatedWidget
      └─ Animates physical properties
          └─ PhysicalModel (с анимацией)
```

**Параметры:**
- `color` - Color (target)
- `shadowColor` - Color
- `elevation` - double
- `shape` - BoxShape
- `borderRadius` - BorderRadius
- `animateColor`, `animateShadowColor`
- `duration`, `curve`, `onEnd`
- `child` - дочерний виджет

### AnimatedSwitcher
```
📦 AnimatedSwitcher (Cross-fade children)
  └─ Stack
      ├─ Old child (fade out)
      └─ New child (fade in)
```

**Параметры:**
- `child` - текущий виджет (меняется по key)
- `duration` - Duration
- `reverseDuration` - Duration (для обратной анимации)
- `switchInCurve` - Curve (для нового child)
- `switchOutCurve` - Curve (для старого child)
- `transitionBuilder` - Widget Function(Widget, Animation<double>)
- `layoutBuilder` - Widget Function(Widget?, List<Widget>)

### AnimatedCrossFade
```
📦 AnimatedCrossFade (Cross-fade between two children)
  └─ Stack
      ├─ firstChild (показывается если CrossFadeState.showFirst)
      └─ secondChild (показывается если CrossFadeState.showSecond)
```

**Параметры:**
- `firstChild` - виджет 1
- `secondChild` - виджет 2
- `crossFadeState` - CrossFadeState (showFirst/showSecond)
- `duration` - Duration
- `reverseDuration` - Duration
- `firstCurve`, `secondCurve`, `sizeCurve` - Curve
- `alignment` - Alignment
- `layoutBuilder` - Widget Function(Widget, Key, Widget, Key)

### Hero
```
📦 Hero (Shared element transition)
  └─ Navigator transition координация
      └─ Child Widget (flies между screens)
```

**Параметры:**
- `tag` - Object (уникальный id для shared element)
- `child` - виджет для transition
- `createRectTween` - RectTween Function(Rect?, Rect?)
- `flightShuttleBuilder` - Widget Function(...)
- `placeholderBuilder` - Widget Function(...)
- `transitionOnUserGestures` - анимация при gesture navigation

### AnimatedBuilder
```
📦 AnimatedBuilder (Explicit animation)
  └─ Animation<T> listener
      └─ builder(context, child) (rebuild on animation)
```

**Параметры:**
- `animation` - Listenable (обычно Animation)
- `builder` - Widget Function(BuildContext, Widget? child)
- `child` - Widget (cached, не rebuilds)

### AnimatedWidget
```
📦 AnimatedWidget (Base for explicit animations)
  └─ Abstract base class
      └─ Subclass implements build(context)
```

**Параметры:**
- `listenable` - Listenable (обычно Animation)

**Применение:** Наследовать для custom animated widgets

### TweenAnimationBuilder
```
📦 TweenAnimationBuilder<T> (Tween-based animation)
  └─ ImplicitlyAnimatedWidget
      └─ Tween<T>.animate(AnimationController)
          └─ builder(context, value, child)
```

**Параметры:**
- `tween` - Tween<T>
- `duration` - Duration
- `curve` - Curve
- `builder` - Widget Function(BuildContext, T value, Widget? child)
- `child` - Widget (cached)
- `onEnd` - VoidCallback

---

## 📝 Input Widgets (Виджеты ввода)

### TextField
```
📦 TextField
  └─ EditableText
      └─ RenderEditable
          ├─ InputDecoration (border, label, hint, etc.)
          └─ Text input + cursor
```

**Параметры (основные):**
- `controller` - TextEditingController
- `focusNode` - FocusNode
- `decoration` - InputDecoration
- `keyboardType` - TextInputType
- `textInputAction` - TextInputAction
- `textCapitalization` - TextCapitalization
- `style` - TextStyle
- `textAlign` - TextAlign
- `textDirection` - TextDirection
- `readOnly` - bool
- `obscureText` - bool (для паролей)
- `autocorrect` - bool
- `maxLines` - int (null = unlimited)
- `minLines` - int
- `expands` - bool
- `maxLength` - int
- `onChanged` - void Function(String)
- `onSubmitted` - void Function(String)
- `onEditingComplete` - VoidCallback
- `enabled` - bool
- `cursorColor` - Color
- `keyboardAppearance` - Brightness
- `scrollPadding` - EdgeInsets
- `enableInteractiveSelection` - bool
- `buildCounter` - Widget? Function(...)

### TextFormField
```
📦 TextFormField (Form-integrated TextField)
  └─ FormField<String>
      └─ TextField
          └─ Validation + save/restore
```

**Параметры:** Те же что у TextField + дополнительные:
- `initialValue` - String
- `validator` - String? Function(String?)
- `onSaved` - void Function(String?)
- `autovalidateMode` - AutovalidateMode
- `restorationId` - String

### Checkbox
```
📦 Checkbox
  └─ Material (checkbox shape + ripple)
      └─ Checkmark animation
```

**Параметры:**
- `value` - bool? (null = indeterminate)
- `onChanged` - void Function(bool?)
- `tristate` - bool (разрешить null)
- `activeColor` - Color (checked color)
- `checkColor` - Color (checkmark color)
- `fillColor` - MaterialStateProperty<Color?>
- `focusColor`, `hoverColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `splashRadius` - double
- `materialTapTargetSize` - MaterialTapTargetSize
- `visualDensity` - VisualDensity
- `focusNode` - FocusNode
- `autofocus` - bool
- `shape` - OutlinedBorder
- `side` - BorderSide
- `isError` - bool

### CheckboxListTile
```
📦 CheckboxListTile (ListTile + Checkbox)
  └─ MergeSemantics
      └─ ListTile
          └─ Checkbox
```

**Параметры:**
- `value`, `onChanged`, `tristate` - как у Checkbox
- `title` - Widget (главный текст)
- `subtitle` - Widget (подзаголовок)
- `secondary` - Widget (leading/trailing icon)
- `isThreeLine` - bool
- `dense` - bool
- `selected` - bool
- `controlAffinity` - ListTileControlAffinity
- `activeColor`, `checkColor`, `tileColor`, `selectedTileColor`
- `contentPadding` - EdgeInsets
- `enabled` - bool

### Radio
```
📦 Radio<T>
  └─ Material (radio button shape + ripple)
      └─ Filled circle animation
```

**Параметры:**
- `value` - T (значение этой радиокнопки)
- `groupValue` - T? (текущее выбранное значение)
- `onChanged` - void Function(T?)
- `toggleable` - bool (можно ли снять выбор)
- `activeColor` - Color
- `fillColor` - MaterialStateProperty<Color?>
- `focusColor`, `hoverColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `splashRadius` - double
- `materialTapTargetSize` - MaterialTapTargetSize
- `visualDensity` - VisualDensity
- `focusNode` - FocusNode
- `autofocus` - bool

### RadioListTile
```
📦 RadioListTile<T> (ListTile + Radio)
  └─ MergeSemantics
      └─ ListTile
          └─ Radio<T>
```

**Параметры:**
- `value`, `groupValue`, `onChanged`, `toggleable` - как у Radio
- `title`, `subtitle`, `secondary`, `isThreeLine`, `dense`, `selected` - как у CheckboxListTile
- `controlAffinity`, `activeColor`, `tileColor`, `selectedTileColor`, `contentPadding`, `enabled`

### Switch
```
📦 Switch
  └─ Material (track + thumb)
      └─ Slide animation
```

**Параметры:**
- `value` - bool
- `onChanged` - void Function(bool)
- `activeColor` - Color (thumb color when on)
- `activeTrackColor` - Color (track color when on)
- `inactiveThumbColor` - Color
- `inactiveTrackColor` - Color
- `activeThumbImage` - ImageProvider
- `inactiveThumbImage` - ImageProvider
- `thumbColor` - MaterialStateProperty<Color?>
- `trackColor` - MaterialStateProperty<Color?>
- `trackOutlineColor` - MaterialStateProperty<Color?>
- `thumbIcon` - MaterialStateProperty<Icon?>
- `materialTapTargetSize` - MaterialTapTargetSize
- `dragStartBehavior` - DragStartBehavior
- `focusColor`, `hoverColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `splashRadius` - double
- `focusNode` - FocusNode
- `autofocus` - bool

### SwitchListTile
```
📦 SwitchListTile (ListTile + Switch)
  └─ MergeSemantics
      └─ ListTile
          └─ Switch
```

**Параметры:**
- `value`, `onChanged` - как у Switch
- `title`, `subtitle`, `secondary`, `isThreeLine`, `dense`, `selected` - как у CheckboxListTile
- `controlAffinity`, `activeColor`, `activeTrackColor`, `inactiveThumbColor`, `inactiveTrackColor`
- `tileColor`, `selectedTileColor`, `contentPadding`, `enabled`

### Slider
```
📦 Slider
  └─ Material (track + thumb + overlay)
      └─ Gesture detection
```

**Параметры:**
- `value` - double (current value)
- `onChanged` - void Function(double)
- `onChangeStart` - void Function(double)
- `onChangeEnd` - void Function(double)
- `min` - double (default 0.0)
- `max` - double (default 1.0)
- `divisions` - int? (discrete steps)
- `label` - String (показывается над thumb)
- `activeColor` - Color
- `inactiveColor` - Color
- `thumbColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `mouseCursor` - MouseCursor
- `semanticFormatterCallback` - String Function(double)
- `focusNode` - FocusNode
- `autofocus` - bool

**Варианты:**
- `Slider()` - обычный
- `Slider.adaptive()` - платформо-специфичный

### RangeSlider
```
📦 RangeSlider (Two-thumb slider)
  └─ Material (track + 2 thumbs + overlays)
      └─ Gesture detection для обоих thumbs
```

**Параметры:**
- `values` - RangeValues (start, end)
- `onChanged` - void Function(RangeValues)
- `onChangeStart`, `onChangeEnd` - void Function(RangeValues)
- `min`, `max` - double
- `divisions` - int
- `labels` - RangeLabels (start label, end label)
- `activeColor`, `inactiveColor` - Color
- Остальные как у Slider

### DropdownButton
```
📦 DropdownButton<T>
  └─ InkWell (trigger)
      └─ Row
          ├─ Selected item
          └─ Down arrow icon
      └─ Overlay (popup menu)
          └─ DropdownMenuItem items
```

**Параметры:**
- `items` - List<DropdownMenuItem<T>>
- `value` - T? (selected value)
- `onChanged` - void Function(T?)
- `onTap` - VoidCallback
- `selectedItemBuilder` - List<Widget> Function(BuildContext)
- `hint` - Widget (показывается если value == null)
- `disabledHint` - Widget
- `elevation` - int
- `style` - TextStyle
- `icon` - Widget (down arrow)
- `iconDisabledColor`, `iconEnabledColor` - Color
- `iconSize` - double
- `isDense` - bool
- `isExpanded` - bool (заполнить ширину)
- `itemHeight` - double
- `focusColor` - Color
- `focusNode` - FocusNode
- `autofocus` - bool
- `dropdownColor` - Color
- `menuMaxHeight` - double
- `enableFeedback` - bool
- `alignment` - AlignmentGeometry
- `borderRadius` - BorderRadius
- `padding` - EdgeInsets

### DropdownMenuItem
```
📦 DropdownMenuItem<T>
  └─ Container
      └─ InkWell
          └─ Child Widget
```

**Параметры:**
- `value` - T
- `onTap` - VoidCallback
- `enabled` - bool
- `alignment` - AlignmentGeometry
- `child` - Widget

### DropdownButtonFormField
```
📦 DropdownButtonFormField<T>
  └─ FormField<T>
      └─ InputDecorator
          └─ DropdownButton<T>
```

**Параметры:** Те же что у DropdownButton + дополнительные:
- `decoration` - InputDecoration
- `validator` - String? Function(T?)
- `onSaved` - void Function(T?)
- `autovalidateMode` - AutovalidateMode

---

## 🔘 Button Widgets (Кнопки)

### TextButton
```
📦 TextButton (Material Design text button)
  └─ Material
      └─ InkWell (ripple)
          └─ Padding
              └─ Row
                  ├─ Icon (optional)
                  └─ Text
```

**Параметры:**
- `onPressed` - VoidCallback? (null = disabled)
- `onLongPress` - VoidCallback?
- `onHover` - void Function(bool)
- `onFocusChange` - void Function(bool)
- `style` - ButtonStyle
- `focusNode` - FocusNode
- `autofocus` - bool
- `clipBehavior` - Clip
- `child` - Widget

**Варианты:**
- `TextButton()` - стандартный
- `TextButton.icon()` - с иконкой

### ElevatedButton
```
📦 ElevatedButton (Material Design elevated button)
  └─ Material (elevation, shadow)
      └─ InkWell (ripple)
          └─ Padding
              └─ Row
                  ├─ Icon (optional)
                  └─ Text
```

**Параметры:** Те же что у TextButton

**Варианты:**
- `ElevatedButton()`
- `ElevatedButton.icon()`

### OutlinedButton
```
📦 OutlinedButton (Material Design outlined button)
  └─ Material (border)
      └─ InkWell (ripple)
          └─ Padding
              └─ Row
                  ├─ Icon (optional)
                  └─ Text
```

**Параметры:** Те же что у TextButton

**Варианты:**
- `OutlinedButton()`
- `OutlinedButton.icon()`

### IconButton
```
📦 IconButton (Icon button)
  └─ Material
      └─ InkWell (ripple)
          └─ Padding
              └─ Icon
```

**Параметры:**
- `onPressed` - VoidCallback?
- `icon` - Widget
- `iconSize` - double
- `visualDensity` - VisualDensity
- `padding` - EdgeInsets
- `alignment` - AlignmentGeometry
- `splashRadius` - double
- `color` - Color
- `focusColor`, `hoverColor`, `highlightColor`, `splashColor`, `disabledColor` - Color
- `mouseCursor` - MouseCursor
- `focusNode` - FocusNode
- `autofocus` - bool
- `tooltip` - String
- `enableFeedback` - bool
- `constraints` - BoxConstraints
- `style` - ButtonStyle
- `isSelected` - bool
- `selectedIcon` - Widget

### FloatingActionButton
```
📦 FloatingActionButton (FAB)
  └─ Material (circular elevation)
      └─ InkWell (ripple)
          └─ Padding
              └─ Icon или Text
```

**Параметры:**
- `onPressed` - VoidCallback?
- `tooltip` - String
- `foregroundColor` - Color (icon/text color)
- `backgroundColor` - Color
- `focusColor`, `hoverColor`, `splashColor` - Color
- `elevation` - double
- `focusElevation`, `hoverElevation`, `highlightElevation`, `disabledElevation` - double
- `shape` - ShapeBorder
- `clipBehavior` - Clip
- `focusNode` - FocusNode
- `autofocus` - bool
- `materialTapTargetSize` - MaterialTapTargetSize
- `mini` - bool (small FAB)
- `mouseCursor` - MouseCursor
- `child` - Widget
- `heroTag` - Object (для Hero transition)

**Варианты:**
- `FloatingActionButton()` - обычный
- `FloatingActionButton.extended()` - с текстом
- `FloatingActionButton.small()` - маленький
- `FloatingActionButton.large()` - большой

### CupertinoButton
```
📦 CupertinoButton (iOS-style button)
  └─ GestureDetector
      └─ Opacity (при нажатии)
          └─ DecoratedBox (опционально)
              └─ Padding
                  └─ Child Widget
```

**Параметры:**
- `onPressed` - VoidCallback?
- `child` - Widget
- `padding` - EdgeInsets
- `color` - Color (background)
- `disabledColor` - Color
- `minSize` - double
- `pressedOpacity` - double
- `borderRadius` - BorderRadius
- `alignment` - AlignmentGeometry

**Варианты:**
- `CupertinoButton()`
- `CupertinoButton.filled()` - с фоном

---

## 🎭 Material Design Widgets (Material компоненты)

### Scaffold
```
📦 Scaffold (Material page structure)
  └─ Material
      ├─ AppBar (top)
      ├─ Body (центр)
      ├─ BottomNavigationBar (bottom)
      ├─ FloatingActionButton (floating)
      ├─ Drawer (left)
      └─ EndDrawer (right)
```

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

### AppBar
```
📦 AppBar (Material app bar)
  └─ Material (elevation, color)
      └─ SafeArea
          └─ FlexibleSpaceBar (optional)
              ├─ Leading (back button, hamburger)
              ├─ Title (text)
              └─ Actions (icons)
```

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

### BottomNavigationBar
```
📦 BottomNavigationBar (Material bottom nav)
  └─ Material
      └─ Row
          ├─ BottomNavigationBarItem 1
          ├─ BottomNavigationBarItem 2
          └─ BottomNavigationBarItem N
```

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

### Drawer
```
📦 Drawer (Side panel)
  └─ Material (elevation)
      └─ ConstrainedBox (width)
          └─ Column (typically)
              ├─ DrawerHeader
              ├─ ListTile 1
              ├─ ListTile 2
              └─ ...
```

**Параметры:**
- `child` - Widget (drawer content)
- `backgroundColor` - Color
- `elevation` - double
- `shadowColor` - Color
- `surfaceTintColor` - Color
- `shape` - ShapeBorder
- `width` - double
- `semanticLabel` - String

### Card
```
📦 Card
  └─ Material (elevation, shape, clipBehavior)
      └─ Child Widget
```

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

### ListTile
```
📦 ListTile (Material list item)
  └─ InkWell (ripple)
      └─ Row
          ├─ Leading (icon/avatar)
          ├─ Column (title + subtitle)
          └─ Trailing (icon/widget)
```

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

### Dialog
```
📦 Dialog (Modal dialog)
  └─ Overlay
      └─ Barrier (scrim)
          └─ Material (rounded, elevated)
              └─ Padding
                  └─ Child Widget (dialog content)
```

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

### AlertDialog
```
📦 AlertDialog (Material alert dialog)
  └─ Dialog
      └─ Column
          ├─ Icon (optional)
          ├─ Title
          ├─ Content
          └─ Actions (buttons)
```

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

### SnackBar
```
📦 SnackBar (Temporary message)
  └─ Material (bottom sheet)
      └─ Row
          ├─ Content (text/widget)
          └─ Action (button)
```

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

### BottomSheet
```
📦 BottomSheet (Bottom panel)
  └─ Material (elevation, rounded top)
      └─ Padding
          └─ Child Widget (sheet content)
```

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

### CircularProgressIndicator
```
📦 CircularProgressIndicator (Spinning circle)
  └─ CustomPaint
      └─ Animated circular arc
```

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

### LinearProgressIndicator
```
📦 LinearProgressIndicator (Horizontal bar)
  └─ CustomPaint
      └─ Animated rectangle
```

**Параметры:**
- `value` - double? (0.0-1.0, null = indeterminate)
- `backgroundColor` - Color
- `color` - Color
- `valueColor` - Animation<Color?>
- `minHeight` - double
- `semanticsLabel` - String
- `semanticsValue` - String
- `borderRadius` - BorderRadius

### Tooltip
```
📦 Tooltip (Hover/long-press tooltip)
  └─ GestureDetector (long press detection)
      └─ Overlay entry (при показе)
          └─ Material (tooltip bubble)
              └─ Text (message)
```

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

### Chip
```
📦 Chip (Material chip)
  └─ Material (rounded)
      └─ InkWell (ripple)
          └─ Row
              ├─ Avatar (optional)
              ├─ Label
              └─ Delete button (optional)
```

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

### Badge
```
📦 Badge (Notification badge)
  └─ Stack
      ├─ Child Widget
      └─ Positioned (badge dot/label)
```

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

### TabBar
```
📦 TabBar (Material tabs)
  └─ Material
      └─ Row (tabs) + Indicator
          ├─ Tab 1
          ├─ Tab 2
          └─ Tab N
```

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

### TabBarView
```
📦 TabBarView (Swipeable tab content)
  └─ NotificationListener
      └─ PageView (tabs content)
          ├─ Tab 1 content
          ├─ Tab 2 content
          └─ Tab N content
```

**Параметры:**
- `children` - List<Widget> (required)
- `controller` - TabController
- `physics` - ScrollPhysics
- `dragStartBehavior` - DragStartBehavior
- `viewportFraction` - double
- `clipBehavior` - Clip

### ExpansionTile
```
📦 ExpansionTile (Expandable list item)
  └─ ListTile (header)
      ├─ Leading (icon)
      ├─ Title + Subtitle
      └─ Trailing (expand icon)
      └─ AnimatedCrossFade
          └─ Children (expanded content)
```

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

## 🧭 Navigation Widgets (Навигация)

### Navigator
```
📦 Navigator (Navigation stack)
  └─ Overlay (routes stack)
      ├─ Route 1 (bottom)
      ├─ Route 2
      └─ Route N (top)
```

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

### MaterialApp
```
📦 MaterialApp (Material app root)
  └─ WidgetsApp
      └─ Navigator
          └─ Material Design theming
              └─ Routes
```

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

### CupertinoApp
```
📦 CupertinoApp (iOS-style app root)
  └─ WidgetsApp
      └─ Navigator
          └─ Cupertino theming
              └─ Routes
```

**Параметры:** Похожи на MaterialApp, но с iOS-специфичными:
- `theme` - CupertinoThemeData
- остальные как у MaterialApp

### PageRouteBuilder
```
📦 PageRouteBuilder (Custom route transition)
  └─ PageRoute
      └─ Custom transition animation
          └─ pageBuilder(context, animation, secondaryAnimation)
```

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

### MaterialPageRoute
```
📦 MaterialPageRoute<T> (Material transition)
  └─ PageRoute
      └─ Platform-specific transition
          └─ builder(context)
```

**Параметры:**
- `builder` - Widget Function(BuildContext) (required)
- `settings` - RouteSettings
- `maintainState` - bool
- `fullscreenDialog` - bool
- `allowSnapshotting` - bool

### CupertinoPageRoute
```
📦 CupertinoPageRoute<T> (iOS transition)
  └─ PageRoute
      └─ iOS-style slide transition
          └─ builder(context)
```

**Параметры:** Те же что у MaterialPageRoute + `title`

---

## 🔧 Utility Widgets (Утилиты)

### Builder
```
📦 Builder
  └─ Вызывает builder с новым BuildContext
      └─ builder(context)
```

**Параметры:**
- `builder` - Widget Function(BuildContext) (required)

**Применение:** Получить BuildContext для доступа к InheritedWidget

### StatefulBuilder
```
📦 StatefulBuilder
  └─ StatefulWidget без отдельного класса
      └─ builder(context, setState)
```

**Параметры:**
- `builder` - Widget Function(BuildContext, StateSetter) (required)

**Применение:** Локальный state без создания StatefulWidget

### LayoutBuilder
```
📦 LayoutBuilder
  └─ Rebuilds на изменение constraints
      └─ builder(context, constraints)
```

**Параметры:**
- `builder` - Widget Function(BuildContext, BoxConstraints) (required)

**Применение:** Адаптивная верстка на основе доступного размера

### OrientationBuilder
```
📦 OrientationBuilder
  └─ Rebuilds на изменение ориентации
      └─ builder(context, orientation)
```

**Параметры:**
- `builder` - Widget Function(BuildContext, Orientation) (required)

**Применение:** Разные layouts для portrait/landscape

### MediaQuery
```
📦 MediaQuery (Inherited screen info)
  └─ InheritedWidget
      └─ MediaQueryData (size, padding, orientation, etc.)
          └─ Child Widget
```

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

### SafeArea
```
📦 SafeArea (Avoid system UI)
  └─ Padding (system insets)
      └─ MediaQuery (updates insets)
          └─ Child Widget
```

**Параметры:**
- `child` - Widget (required)
- `left` - bool (avoid left inset)
- `top` - bool (avoid top inset)
- `right` - bool (avoid right inset)
- `bottom` - bool (avoid bottom inset)
- `minimum` - EdgeInsets (минимальные отступы)
- `maintainBottomViewPadding` - bool

### Theme
```
📦 Theme (Inherited theme)
  └─ InheritedTheme
      └─ ThemeData (colors, typography, etc.)
          └─ Child Widget
```

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

### InheritedWidget
```
📦 InheritedWidget (Data propagation)
  └─ Abstract base class
      └─ Efficient data sharing down tree
          └─ Child Widget (can access data)
```

**Применение:** Создать custom inherited widget для sharing data

**Методы:**
- `updateShouldNotify(covariant InheritedWidget oldWidget)` - bool

### InheritedTheme
```
📦 InheritedTheme (Theme propagation)
  └─ InheritedWidget
      └─ Theme data
          └─ Child Widget
```

**Применение:** Base для theme widgets

### ValueListenableBuilder
```
📦 ValueListenableBuilder<T> (Listen to ValueNotifier)
  └─ Rebuilds когда value changes
      └─ builder(context, value, child)
```

**Параметры:**
- `valueListenable` - ValueListenable<T> (required)
- `builder` - Widget Function(BuildContext, T, Widget?) (required)
- `child` - Widget (cached, не rebuilds)

**Применение:** Reactive UI для ValueNotifier

### StreamBuilder
```
📦 StreamBuilder<T> (Listen to Stream)
  └─ Rebuilds на каждое событие stream
      └─ builder(context, snapshot)
```

**Параметры:**
- `stream` - Stream<T>
- `initialData` - T
- `builder` - Widget Function(BuildContext, AsyncSnapshot<T>) (required)

**Применение:** Reactive UI для Stream

### FutureBuilder
```
📦 FutureBuilder<T> (Listen to Future)
  └─ Rebuilds когда future completes
      └─ builder(context, snapshot)
```

**Параметры:**
- `future` - Future<T>
- `initialData` - T
- `builder` - Widget Function(BuildContext, AsyncSnapshot<T>) (required)

**Применение:** Loading states для async operations

### Form
```
📦 Form (Form validation)
  └─ FormState (validation, saving)
      └─ Children (FormField widgets)
```

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

### FormField
```
📦 FormField<T> (Form field base)
  └─ FormFieldState<T>
      └─ builder(state)
```

**Параметры:**
- `builder` - Widget Function(FormFieldState<T>) (required)
- `onSaved` - void Function(T?)
- `validator` - String? Function(T?)
- `initialValue` - T
- `autovalidateMode` - AutovalidateMode
- `enabled` - bool
- `restorationId` - String

**Применение:** Base для custom form fields

### Focus
```
📦 Focus (Focus management)
  └─ FocusNode
      └─ Child Widget (focusable)
```

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

### FocusScope
```
📦 FocusScope (Focus subtree)
  └─ Focus
      └─ FocusScopeNode (manages focus tree)
          └─ Child Widget
```

**Параметры:** Те же что у Focus + `node` (FocusScopeNode)

### Semantics
```
📦 Semantics (Accessibility)
  └─ RenderSemantics
      └─ SemanticsNode (accessibility info)
          └─ Child Widget
```

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

### ExcludeSemantics
```
📦 ExcludeSemantics (Hide from accessibility)
  └─ Semantics (excludeSemantics: true)
      └─ Child Widget (hidden from screen readers)
```

**Параметры:**
- `excluding` - bool (default true)
- `child` - Widget

### MergeSemantics
```
📦 MergeSemantics (Merge child semantics)
  └─ Semantics (merges children)
      └─ Child Widget
```

**Параметры:**
- `child` - Widget

### Placeholder
```
📦 Placeholder (Временный виджет)
  └─ LimitedBox
      └─ CustomPaint (рисует X)
```

**Параметры:**
- `color` - Color
- `strokeWidth` - double
- `fallbackWidth` - double
- `fallbackHeight` - double

---

## 📱 Platform-Specific Widgets (Платформо-специфичные)

### PlatformMenuBar
```
📦 PlatformMenuBar (Native menu bar)
  └─ Platform-specific menu
      └─ Menu items (desktop platforms)
```

**Параметры:**
- `menus` - List<PlatformMenuItem> (required)

### SelectionArea
```
📦 SelectionArea (Text selection)
  └─ SelectionContainer
      └─ Child Widget (selectable content)
```

**Параметры:**
- `child` - Widget (required)
- `focusNode` - FocusNode
- `selectionControls` - TextSelectionControls
- `contextMenuBuilder` - Widget Function(BuildContext, SelectableRegionState)
- `magnifierConfiguration` - TextMagnifierConfiguration
- `onSelectionChanged` - void Function(SelectedContent?)

### CupertinoNavigationBar
```
📦 CupertinoNavigationBar (iOS nav bar)
  └─ CupertinoSliverNavigationBar
      ├─ Leading (back button)
      ├─ Middle (title)
      └─ Trailing (buttons)
```

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

## 🎯 Заключение

Этот справочник охватывает **200+ основных виджетов Flutter**, организованных по категориям:

1. **Layout Widgets** (35+) - Container, Row, Column, Stack, Wrap, etc.
2. **Scrolling Widgets** (10+) - ListView, GridView, CustomScrollView, etc.
3. **Text Widgets** (10+) - Text, RichText, TextField, etc.
4. **Image Widgets** (8+) - Image, Icon, etc.
5. **Visual Effects** (15+) - Opacity, Transform, ClipRRect, etc.
6. **Interaction** (15+) - GestureDetector, Draggable, etc.
7. **Animation** (20+) - AnimatedContainer, AnimatedOpacity, Hero, etc.
8. **Input** (20+) - TextField, Checkbox, Radio, Switch, Slider, etc.
9. **Buttons** (8+) - TextButton, ElevatedButton, IconButton, FAB, etc.
10. **Material Design** (30+) - Scaffold, AppBar, Card, Dialog, etc.
11. **Navigation** (8+) - Navigator, MaterialApp, routes, etc.
12. **Utility** (20+) - Builder, MediaQuery, Theme, Form, etc.
13. **Platform-Specific** (5+) - Cupertino widgets, etc.

Каждый виджет показан с:
- 📦 Внутренней структурой слоев (изнутри → наружу)
- Полным списком параметров
- Вариантами использования
- Примерами кода (где применимо)

---

**💡 Совет:** Используйте поиск (Ctrl+F) по этому документу для быстрого нахождения нужного виджета!

**📚 Источники:** Flutter SDK 3.x documentation
