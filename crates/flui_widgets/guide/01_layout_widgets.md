# 📦 Layout Widgets (Виджеты размещения)

## 1. Basic Layout (Базовое размещение)

### Container
```
📦 Container (изнутри → наружу)
  └─ Align (alignment) -> RenderPositionedBox
      └─ Padding (padding) -> RenderPadding
          └─ DecoratedBox (decoration) -> RenderDecoratedBox
              └─ ConstrainedBox (constraints) -> RenderConstrainedBox
                  └─ Transform (transform) -> RenderTransform
                      └─ Padding (margin) -> RenderPadding
                          └─ Child Widget
```

**RenderObject:** Комбинация нескольких RenderObject (см. выше)

**Параметры:**
- `alignment` - выравнивание ребенка
- `padding` - внутренние отступы
- `decoration` - фон, границы, тени
- `constraints` - ограничения размера
- `margin` - внешние отступы
- `transform` - трансформация
- `child` - дочерний виджет

---

### SizedBox
```
📦 SizedBox
  └─ ConstrainedBox (width/height) -> RenderConstrainedBox
      └─ Child Widget (или пусто)
```

**RenderObject:** `RenderConstrainedBox`

**Параметры:**
- `width` - фиксированная ширина
- `height` - фиксированная высота
- `child` - дочерний виджет (опционально)

**Варианты:**
- `SizedBox.expand()` - занимает все доступное пространство
- `SizedBox.shrink()` - нулевой размер
- `SizedBox.square()` - квадрат

---

### Padding
```
📦 Padding
  └─ Padding (изнутри) -> RenderPadding
      └─ Child Widget
```

**RenderObject:** `RenderPadding`

**Параметры:**
- `padding` - EdgeInsets (all, symmetric, only, fromLTRB)
- `child` - дочерний виджет

---

### Center
```
📦 Center
  └─ Align (alignment: Alignment.center) -> RenderPositionedBox
      └─ Child Widget
```

**RenderObject:** `RenderPositionedBox`

**Параметры:**
- `widthFactor` - множитель ширины от ребенка
- `heightFactor` - множитель высоты от ребенка
- `child` - дочерний виджет

---

### Align
```
📦 Align
  └─ CustomSingleChildLayout -> RenderPositionedBox
      └─ Child Widget (позиционирован)
```

**RenderObject:** `RenderPositionedBox`

**Параметры:**
- `alignment` - позиция (Alignment.topLeft, center, etc.)
- `widthFactor` - множитель ширины
- `heightFactor` - множитель высоты
- `child` - дочерний виджет

---

### FittedBox
```
📦 FittedBox
  └─ Transform (scale для подгонки) -> RenderFittedBox
      └─ ClipRect (если clipBehavior != none)
          └─ Child Widget (масштабирован)
```

**RenderObject:** `RenderFittedBox`

**Параметры:**
- `fit` - BoxFit (fill, contain, cover, fitWidth, fitHeight, none, scaleDown)
- `alignment` - выравнивание после подгонки
- `clipBehavior` - обрезка краев

---

### AspectRatio
```
📦 AspectRatio
  └─ ConstrainedBox (поддерживает aspectRatio) -> RenderAspectRatio
      └─ Child Widget
```

**RenderObject:** `RenderAspectRatio`

**Параметры:**
- `aspectRatio` - соотношение сторон (width/height)
- `child` - дочерний виджет

---

### ConstrainedBox
```
📦 ConstrainedBox
  └─ Constraints (min/max width/height) -> RenderConstrainedBox
      └─ Child Widget
```

**RenderObject:** `RenderConstrainedBox`

**Параметры:**
- `constraints` - BoxConstraints
- `child` - дочерний виджет

---

### LimitedBox
```
📦 LimitedBox
  └─ ConstrainedBox (только если родитель unbounded) -> RenderLimitedBox
      └─ Child Widget
```

**RenderObject:** `RenderLimitedBox`

**Параметры:**
- `maxWidth` - макс. ширина если родитель unbounded
- `maxHeight` - макс. высота если родитель unbounded
- `child` - дочерний виджет

---

### FractionallySizedBox
```
📦 FractionallySizedBox
  └─ Align (alignment) -> RenderFractionallySizedOverflowBox
      └─ ConstrainedBox (размер = родитель × factor)
          └─ Child Widget
```

**RenderObject:** `RenderFractionallySizedOverflowBox`

**Параметры:**
- `widthFactor` - процент от ширины родителя (0.0-1.0)
- `heightFactor` - процент от высоты родителя (0.0-1.0)
- `alignment` - выравнивание
- `child` - дочерний виджет

---

### Baseline
```
📦 Baseline
  └─ CustomSingleChildLayout (выравнивание по baseline) -> RenderBaseline
      └─ Child Widget
```

**RenderObject:** `RenderBaseline`

**Параметры:**
- `baseline` - расстояние от верха
- `baselineType` - alphabetic или ideographic
- `child` - дочерний виджет

---

### OverflowBox
```
📦 OverflowBox
  └─ Игнорирует constraints родителя -> RenderConstrainedOverflowBox
      └─ Child Widget (может выходить за границы)
```

**RenderObject:** `RenderConstrainedOverflowBox`

**Параметры:**
- `minWidth`, `maxWidth` - новые constraints
- `minHeight`, `maxHeight` - новые constraints
- `alignment` - выравнивание
- `child` - дочерний виджет

---

### SizedOverflowBox
```
📦 SizedOverflowBox
  └─ Фиксированный размер + дети могут overflow -> RenderSizedOverflowBox
      └─ Child Widget
```

**RenderObject:** `RenderSizedOverflowBox`

**Параметры:**
- `size` - фиксированный размер виджета
- `alignment` - выравнивание ребенка
- `child` - дочерний виджет

---

### Offstage
```
📦 Offstage
  └─ (рендерит, но не показывает если offstage=true) -> RenderOffstage
      └─ Child Widget (invisible)
```

**RenderObject:** `RenderOffstage`

**Параметры:**
- `offstage` - если true, не показывается
- `child` - дочерний виджет

---

### Visibility
```
📦 Visibility
  └─ Conditional rendering -> RenderOffstage/RenderSliverOffstage
      └─ Child Widget (или replacement)
```

**RenderObject:** `RenderOffstage` или другие в зависимости от параметров

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

## 2. Flex Layout (Гибкое размещение)

### Row
```
📦 Row (Horizontal Flex)
  └─ RenderFlex (direction: Axis.horizontal)
      ├─ Child 1 (с FlexParentData)
      ├─ Child 2 (с FlexParentData)
      └─ Child N (с FlexParentData)
```

**RenderObject:** `RenderFlex`

**Параметры:**
- `mainAxisAlignment` - выравнивание по главной оси
- `crossAxisAlignment` - выравнивание по поперечной оси
- `mainAxisSize` - max или min
- `verticalDirection` - down или up
- `textDirection` - ltr или rtl
- `textBaseline` - alphabetic или ideographic
- `children` - список виджетов

**MainAxisAlignment:**
- `start`, `end`, `center`, `spaceBetween`, `spaceAround`, `spaceEvenly`

**CrossAxisAlignment:**
- `start`, `end`, `center`, `stretch`, `baseline`

---

### Column
```
📦 Column (Vertical Flex)
  └─ RenderFlex (direction: Axis.vertical)
      ├─ Child 1 (с FlexParentData)
      ├─ Child 2 (с FlexParentData)
      └─ Child N (с FlexParentData)
```

**RenderObject:** `RenderFlex`

**Параметры:** Те же что у Row

---

### Flexible
```
📦 Flexible
  └─ FlexParentData (flex factor, fit)
      └─ Child Widget (может расширяться)
```

**RenderObject:** Не создает свой RenderObject (модифицирует ParentData)

**Параметры:**
- `flex` - фактор гибкости (по умолчанию 1)
- `fit` - FlexFit.tight или FlexFit.loose
- `child` - дочерний виджет

---

### Expanded
```
📦 Expanded = Flexible(fit: FlexFit.tight)
  └─ FlexParentData (flex factor, fit: tight)
      └─ Child Widget (заполняет доступное место)
```

**RenderObject:** Не создает свой RenderObject (модифицирует ParentData)

**Параметры:**
- `flex` - фактор расширения (по умолчанию 1)
- `child` - дочерний виджет

---

### Spacer
```
📦 Spacer = Expanded(child: SizedBox.shrink())
  └─ Пустое пространство с flex factor
```

**RenderObject:** `RenderConstrainedBox` (через SizedBox)

**Параметры:**
- `flex` - фактор расширения (по умолчанию 1)

---

### Flex
```
📦 Flex (Generic flex container)
  └─ RenderFlex (direction настраивается)
      └─ Children (с FlexParentData)
```

**RenderObject:** `RenderFlex`

**Параметры:**
- `direction` - Axis.horizontal или Axis.vertical
- Остальные как у Row/Column

---

## 3. Stack Layout (Наложение слоями)

### Stack
```
📦 Stack
  └─ RenderStack
      ├─ Child 1 (внизу, с StackParentData)
      ├─ Child 2 (выше, с StackParentData)
      └─ Child N (сверху, с StackParentData)
```

**RenderObject:** `RenderStack`

**Параметры:**
- `alignment` - выравнивание не-positioned детей
- `fit` - StackFit.loose, expand, passthrough
- `clipBehavior` - обрезка overflow
- `textDirection` - для directional alignment
- `children` - список виджетов (порядок = Z-order)

---

### Positioned
```
📦 Positioned (только внутри Stack!)
  └─ StackParentData (top/left/right/bottom)
      └─ Child Widget (позиционирован абсолютно)
```

**RenderObject:** Не создает свой RenderObject (модифицирует StackParentData)

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

---

### PositionedDirectional
```
📦 PositionedDirectional (учитывает textDirection)
  └─ Positioned (auto-converts start/end)
      └─ Child Widget
```

**RenderObject:** Не создает свой RenderObject (модифицирует StackParentData)

**Параметры:**
- `start` - отступ от начала (left для LTR)
- `end` - отступ от конца (right для LTR)
- `top`, `bottom`, `width`, `height` - как у Positioned
- `child` - дочерний виджет

---

### IndexedStack
```
📦 IndexedStack (показывает только один child)
  └─ RenderIndexedStack
      ├─ Child 0 (visible если index=0)
      ├─ Child 1 (visible если index=1)
      └─ Child N (visible если index=N)
```

**RenderObject:** `RenderIndexedStack`

**Параметры:**
- `index` - индекс видимого ребенка
- `alignment` - выравнивание
- `sizing` - StackFit (loose, expand, passthrough)
- `children` - список виджетов

---

## 4. Multi-Child Layout (Множественные дети)

### Wrap
```
📦 Wrap (Flow-like layout)
  └─ RenderWrap
      ├─ Row/Column 1: [Child 1, Child 2, ...]
      ├─ Row/Column 2: [Child N, ...]
      └─ Row/Column M: [...]
```

**RenderObject:** `RenderWrap`

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

---

### Flow
```
📦 Flow (Custom positioned children)
  └─ RenderFlow
      └─ FlowDelegate (custom positioning logic)
          └─ Children (позиционированы delegate)
```

**RenderObject:** `RenderFlow`

**Параметры:**
- `delegate` - FlowDelegate (определяет позиции)
- `children` - список виджетов
- `clipBehavior` - обрезка overflow

---

### ListBody
```
📦 ListBody (Simple vertical/horizontal list)
  └─ RenderListBody
      ├─ Child 1 (positioned sequentially)
      ├─ Child 2
      └─ Child N
```

**RenderObject:** `RenderListBody`

**Параметры:**
- `mainAxis` - Axis.vertical или horizontal
- `reverse` - реверс порядка
- `children` - список виджетов

---

### Table
```
📦 Table
  └─ RenderTable
      ├─ TableRow 1: [TableCell 1, TableCell 2, ...]
      ├─ TableRow 2: [TableCell 1, TableCell 2, ...]
      └─ TableRow N: [...]
```

**RenderObject:** `RenderTable`

**Параметры:**
- `children` - список TableRow
- `columnWidths` - Map<int, TableColumnWidth>
- `defaultColumnWidth` - ширина колонок по умолчанию
- `textDirection` - для directional layout
- `border` - TableBorder
- `defaultVerticalAlignment` - TableCellVerticalAlignment
- `textBaseline` - для baseline alignment

---

### TableRow
```
📦 TableRow (только внутри Table!)
  └─ List<Widget> (TableCells)
```

**RenderObject:** Не создает свой RenderObject

**Параметры:**
- `decoration` - BoxDecoration для строки
- `children` - список виджетов (ячеек)

---

### TableCell
```
📦 TableCell (обертка с настройками для ячейки)
  └─ TableCellParentData
      └─ Child Widget
```

**RenderObject:** Не создает свой RenderObject (модифицирует ParentData)

**Параметры:**
- `verticalAlignment` - TableCellVerticalAlignment
- `child` - дочерний виджет

---

### CustomMultiChildLayout
```
📦 CustomMultiChildLayout
  └─ RenderCustomMultiChildLayoutBox
      └─ MultiChildLayoutDelegate (custom logic)
          └─ Children (с LayoutId)
```

**RenderObject:** `RenderCustomMultiChildLayoutBox`

**Параметры:**
- `delegate` - MultiChildLayoutDelegate
- `children` - список виджетов с LayoutId

---

### LayoutId
```
📦 LayoutId (метка для child в CustomMultiChildLayout)
  └─ MultiChildLayoutParentData (id)
      └─ Child Widget
```

**RenderObject:** Не создает свой RenderObject (модифицирует ParentData)

**Параметры:**
- `id` - Object (любой ключ)
- `child` - дочерний виджет
