# Полный каталог Renderers из Flutter

> Систематизированный список всех рендереров из Flutter с категоризацией по типу дочерних элементов

## 📊 Сводная статистика

| Тип | Количество | Процент |
|-----|-----------|---------|
| **Leaf** (0 детей) | 9 | 11% |
| **Single** (1 ребенок) | 34 | 42% |
| **Container** (N детей) | 38 | 47% |
| **Всего** | **81** | **100%** |

---

## 🍃 LEAF RenderObjects (0 детей) - 9 штук

Рисуют контент напрямую, без дочерних элементов.

| # | RenderObject | Категория | Описание | Статус Flui |
|---|--------------|-----------|----------|-------------|
| 1 | **RenderParagraph** | Text | Многострочный текст | ✅ Реализовано |
| 2 | **RenderEditableLine** | Text | Редактируемая строка текста | ⏳ Планируется |
| 3 | **RenderImage** | Media | Растровое изображение | ✅ Реализовано |
| 4 | **RenderTexture** | Media | GPU текстура | ⏳ Планируется |
| 5 | **RenderErrorBox** | Debug | Красный бокс с ошибкой | ⏳ Планируется |
| 6 | **RenderPlaceholder** | Debug | Placeholder прямоугольник | ⏳ Планируется |
| 7 | **RenderPerformanceOverlay** | Debug | Performance метрики | ❌ Низкий приоритет |
| 8 | **RenderFittedBox** | Special | Масштабирует child по BoxFit | ✅ Реализовано |
| 9 | **RenderColoredBox** | Visual | Простой цветной прямоугольник | ✅ Реализовано |

**Примечание:** RenderFittedBox технически может быть Single-child, но часто используется как Leaf с одним виртуальным ребенком.

---

## 📦 SINGLE RenderObjects (1 ребенок) - 34 штуки

Один дочерний элемент. Базовый trait: **RenderSingleBox** (было RenderProxyBox).

### Layout Single-child (13)

| # | RenderObject | Описание | Override | Статус Flui |
|---|--------------|----------|----------|-------------|
| 1 | **RenderPadding** | Добавляет отступы | perform_layout | ✅ Реализовано |
| 2 | **RenderConstrainedBox** | Ограничения размера (min/max) | perform_layout | ✅ Реализовано |
| 3 | **RenderLimitedBox** | Ограничения для unbounded | perform_layout | ✅ Реализовано |
| 4 | **RenderAspectRatio** | Фиксированное соотношение сторон | perform_layout | ✅ Реализовано |
| 5 | **RenderFractionallySizedBox** | Размер как доля родителя | perform_layout | ✅ Реализовано |
| 6 | **RenderPositionedBox** | Align/Center внутри родителя | perform_layout | ✅ Реализовано |
| 7 | **RenderIntrinsicWidth** | Ширина = intrinsic width | perform_layout | ✅ Реализовано |
| 8 | **RenderIntrinsicHeight** | Высота = intrinsic height | perform_layout | ✅ Реализовано |
| 9 | **RenderBaseline** | Выравнивание по baseline | perform_layout | ✅ Реализовано |
| 10 | **RenderShiftedBox** | Базовый класс для shift | perform_layout | ⏳ Планируется |
| 11 | **RenderRotatedBox** | Поворот на 90°/180°/270° | perform_layout | ✅ Реализовано |
| 12 | **RenderSizedBox** | Фиксированный размер | perform_layout | ✅ Реализовано |
| 13 | **RenderSizedOverflowBox** | Размер != child размер | perform_layout | ✅ Реализовано |

### Visual Effects Single-child (13)

| # | RenderObject | Описание | Override | Статус Flui |
|---|--------------|----------|----------|-------------|
| 14 | **RenderOpacity** | Прозрачность (0.0-1.0) | paint_with_child | ✅ Реализовано |
| 15 | **RenderAnimatedOpacity** | Анимированная прозрачность | paint_with_child | ✅ Реализовано |
| 16 | **RenderTransform** | Матричные трансформации | paint_with_child | ✅ Реализовано |
| 17 | **RenderClipRect** | Обрезка прямоугольником | paint_with_child | ✅ Реализовано |
| 18 | **RenderClipRRect** | Обрезка скругл. прямоуг. | paint_with_child | ✅ Реализовано |
| 19 | **RenderClipOval** | Обрезка овалом | paint_with_child | ✅ Реализовано |
| 20 | **RenderClipPath** | Обрезка произвольным путем | paint_with_child | ✅ Реализовано |
| 21 | **RenderDecoratedBox** | Background/Border/Shadow | paint_with_child | ✅ Реализовано |
| 22 | **RenderPhysicalModel** | Material elevation/shadow | paint_with_child | ✅ Реализовано |
| 23 | **RenderPhysicalShape** | Custom shape elevation | paint_with_child | ⏳ Планируется |
| 24 | **RenderBackdropFilter** | Blur фон за виджетом | paint_with_child | ✅ Реализовано |
| 25 | **RenderShaderMask** | Shader маска | paint_with_child | ✅ Реализовано |
| 26 | **RenderRepaintBoundary** | Отдельный paint layer | paint_with_child | ✅ Реализовано |
| 27 | **RenderOffstage** | Скрывает child (не рисует) | paint_with_child | ✅ Реализовано |
| 28 | **RenderVisibility** | Показывает/скрывает child | paint_with_child | ✅ Реализовано |

### Interaction Single-child (4)

| # | RenderObject | Описание | Override | Статус Flui |
|---|--------------|----------|----------|-------------|
| 28 | **RenderPointerListener** | Pointer события | hit_test | ✅ Реализовано |
| 29 | **RenderIgnorePointer** | Пропускает hit tests | hit_test | ✅ Реализовано |
| 30 | **RenderAbsorbPointer** | Блокирует hit tests | hit_test | ✅ Реализовано |
| 31 | **RenderMouseRegion** | Mouse enter/exit/hover | hit_test | ✅ Реализовано |

### Special Single-child (4)

| # | RenderObject | Описание | Override | Статус Flui |
|---|--------------|----------|----------|-------------|
| 32 | **RenderCustomPaint** | Кастомная отрисовка | paint | ✅ Реализовано |
| 33 | **RenderMetaData** | Метаданные для родителя | - | ✅ Реализовано |
| 34 | **RenderAnnotatedRegion** | Metadata для system UI | - | ✅ Реализовано |
| 35 | **RenderBlockSemantics** | Блокирует семантику | - | ✅ Реализовано |
| 36 | **RenderExcludeSemantics** | Исключает семантику | - | ✅ Реализовано |
| 37 | **RenderMergeSemantics** | Объединяет семантику | - | ✅ Реализовано |

---

## 📚 CONTAINER RenderObjects (N детей) - 38 штук

Множество дочерних элементов. Базовый trait: **RenderContainerBox**.

### Layout Container (12)

| # | RenderObject | Описание | Layout Algorithm | Статус Flui |
|---|--------------|----------|------------------|-------------|
| 1 | **RenderFlex** | Row/Column (linear + flex) | Linear flex layout | ✅ Реализовано |
| 2 | **RenderStack** | Positioned слои | Absolute positioning | ✅ Реализовано |
| 3 | **RenderIndexedStack** | Показывает child по index | Index selection | ✅ Реализовано |
| 4 | **RenderWrap** | Wrap с переносом строк | Flow with wrapping | ✅ Реализовано |
| 5 | **RenderFlow** | Custom layout delegate | Custom delegate | ⏳ Планируется |
| 6 | **RenderTable** | Табличный layout | Table algorithm | ⏳ Планируется |
| 7 | **RenderListBody** | Простой scrollable список | Linear list | ✅ Реализовано |
| 8 | **RenderGrid** | Grid layout (CSS Grid) | Grid algorithm | ⏳ Планируется |
| 9 | **RenderListWheelViewport** | 3D wheel picker | Wheel positioning | ⏳ Планируется |
| 10 | **RenderCupertinoContextMenu** | iOS context menu | Stack-based | ⏳ Планируется |
| 11 | **RenderCustomMultiChildLayoutBox** | Custom multi-child layout | Custom delegate | ⏳ Планируется |
| 12 | **RenderTwoDimensionalViewport** | 2D scrolling (table/grid) | 2D viewport | ⏳ Планируется |

### Sliver Container (26)

**Sliver protocol** - специальный протокол для scrollable контента.

| # | RenderSliver | Описание | Статус Flui |
|---|--------------|----------|-------------|
| 13 | **RenderSliver** | Базовый trait для slivers | ⏳ Планируется |
| 14 | **RenderSliverList** | Scrollable список | ⏳ Планируется |
| 15 | **RenderSliverFixedExtentList** | Список с фикс. высотой | ⏳ Планируется |
| 16 | **RenderSliverVariedExtentList** | Список с разной высотой | ⏳ Планируется |
| 17 | **RenderSliverGrid** | Scrollable grid | ⏳ Планируется |
| 18 | **RenderSliverToBoxAdapter** | Box → Sliver адаптер | ⏳ Планируется |
| 19 | **RenderSliverPadding** | Padding для sliver | ⏳ Планируется |
| 20 | **RenderSliverFillViewport** | Заполняет viewport | ⏳ Планируется |
| 21 | **RenderSliverFillRemaining** | Заполняет остаток | ⏳ Планируется |
| 22 | **RenderSliverPersistentHeader** | Sticky header | ⏳ Планируется |
| 23 | **RenderSliverFloatingPersistentHeader** | Floating header | ⏳ Планируется |
| 24 | **RenderSliverPinnedPersistentHeader** | Pinned header | ⏳ Планируется |
| 25 | **RenderSliverAnimatedOpacity** | Анимир. прозрачность | ⏳ Планируется |
| 26 | **RenderSliverIgnorePointer** | Ignore pointer | ⏳ Планируется |
| 27 | **RenderSliverOffstage** | Скрывает sliver | ⏳ Планируется |
| 28 | **RenderSliverOpacity** | Прозрачность sliver | ⏳ Планируется |
| 29 | **RenderSliverCrossAxisGroup** | Cross-axis группировка | ⏳ Планируется |
| 30 | **RenderSliverMainAxisGroup** | Main-axis группировка | ⏳ Планируется |
| 31 | **RenderViewport** | Viewport для slivers | ⏳ Планируется |
| 32 | **RenderShrinkWrappingViewport** | Shrink-wrap viewport | ⏳ Планируется |
| 33 | **RenderAbstractViewport** | Абстрактный viewport | ⏳ Планируется |
| 34 | **RenderSliverMultiBoxAdaptor** | Базовый для списков | ⏳ Планируется |
| 35 | **RenderSliverEdgeInsetsPadding** | EdgeInsets padding | ⏳ Планируется |
| 36 | **RenderSliverCrossAxisPositioned** | Cross-axis позиционир. | ⏳ Планируется |
| 37 | **RenderSliverConstrainedCrossAxis** | Cross-axis constraints | ⏳ Планируется |
| 38 | **RenderSliverOverlapAbsorber** | Absorb overlap | ⏳ Планируется |

---

## 🎯 Распределение по категориям

### По функциональности

| Категория | Leaf | Single | Container | Всего |
|-----------|------|--------|-----------|-------|
| **Layout** | 1 | 13 | 12 | **26** |
| **Visual Effects** | 1 | 13 | 0 | **14** |
| **Interaction** | 0 | 4 | 0 | **4** |
| **Text** | 2 | 0 | 0 | **2** |
| **Media** | 2 | 0 | 0 | **2** |
| **Sliver** | 0 | 0 | 26 | **26** |
| **Debug/Special** | 3 | 4 | 0 | **7** |
| **Всего** | **9** | **34** | **38** | **81** |

### По приоритету для Flui

| Приоритет | Описание | Количество |
|-----------|----------|-----------|
| ✅ **Реализовано** | Готово и протестировано | **44** (54%) |
| 🔄 **В процессе** | Активная разработка | **0** (0%) |
| ⏳ **High Priority** | Layout + Visual + Text | **2** (2%) |
| ⏳ **Medium Priority** | Media + Advanced Layout | **4** (5%) |
| ⏳ **Low Priority** | Sliver + Debug + Special | **31** (39%) |

---


## 🚀 Roadmap для Flui

### Phase 1: Core (✅ 100% Complete)
- [x] RenderPadding
- [x] RenderOpacity
- [x] RenderTransform
- [x] RenderClipRect/RRect/Oval/Path
- [x] RenderDecoratedBox
- [x] RenderConstrainedBox
- [x] RenderAspectRatio
- [x] RenderLimitedBox
- [x] RenderFractionallySizedBox
- [x] RenderPositionedBox
- [x] RenderFlex
- [x] RenderStack
- [x] RenderIndexedStack
- [x] RenderOffstage
- [x] RenderPointerListener
- [x] RenderIgnorePointer
- [x] RenderAbsorbPointer
- [x] RenderMouseRegion
- [x] RenderParagraph
- [x] RenderBaseline
- [x] RenderIntrinsicWidth/Height
- [x] RenderRotatedBox
- [x] RenderSizedBox
- [x] RenderSizedOverflowBox
- [x] RenderAnimatedOpacity
- [x] RenderPhysicalModel
- [x] RenderBackdropFilter
- [x] RenderShaderMask
- [x] RenderRepaintBoundary
- [x] RenderVisibility
- [x] RenderWrap
- [x] RenderListBody
- [x] RenderCustomPaint
- [x] RenderMetaData
- [x] RenderAnnotatedRegion
- [x] RenderFittedBox
- [x] RenderColoredBox
- [x] RenderBlockSemantics
- [x] RenderExcludeSemantics
- [x] RenderMergeSemantics

### Phase 2: Essential (⏳ In Progress)
- [x] RenderImage
- [ ] RenderPhysicalShape (custom shape elevation)
- [ ] RenderShiftedBox (base class for shift operations)

### Phase 3: Advanced (⏳ Future)
- [ ] RenderTable
- [ ] RenderGrid
- [ ] RenderFlow
- [ ] RenderEditableLine (editable text)
- [ ] RenderTexture (GPU texture)
- [ ] RenderErrorBox (debug error box)
- [ ] RenderPlaceholder (debug placeholder)

### Phase 4: Sliver (⏳ Future)
- [ ] RenderSliver базовый trait
- [ ] RenderSliverList
- [ ] RenderSliverGrid
- [ ] RenderViewport
- [ ] RenderSliverPadding

---

## 📊 Статистика по статусу

| Статус | Layout | Visual | Interaction | Text | Media | Sliver | Special | Всего |
|--------|--------|--------|-------------|------|-------|--------|---------|-------|
| ✅ Готово | 16 | 14 | 4 | 1 | 1 | 0 | 8 | **44** |
| ⏳ Планируется | 9 | 1 | 0 | 1 | 1 | 26 | 0 | **37** |
| **Всего** | **25** | **15** | **4** | **2** | **2** | **26** | **8** | **82** |

---

## 🔍 Примечания

### Различия между Single и Container

**Single (RenderSingleBox):**
- Ровно 1 child или None
- Простое управление: `Option<Box<dyn DynRenderObject>>`
- Обычно forwarding к child с модификацией

**Container (RenderContainerBox):**
- 0..N детей
- Управление: `Vec<Box<dyn DynRenderObject>>`
- Сложная логика layout для всех детей

### Sliver vs Box

**Box protocol:**
- Фиксированный размер
- BoxConstraints (min/max width/height)
- Используется для большинства UI

**Sliver protocol:**
- Переменный размер вдоль scroll axis
- SliverConstraints (scrollOffset, remainingSpace)
- Используется для scrollable контента

### Leaf оптимизации

Leaf RenderObject'ы могут быть оптимизированы:
- Нет `visit_children` overhead
- Нет `hit_test_children` overhead
- Прямое рисование в `paint_leaf`
- Intrinsic размеры из контента

---

**Последнее обновление:** Октябрь 2024
**Источник:** Flutter rendering library + анализ документации
**Всего типов:** 81 RenderObject
