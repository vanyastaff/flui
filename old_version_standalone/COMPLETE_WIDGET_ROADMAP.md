# nebula-ui Complete Widget Implementation Roadmap 🏗️

**Goal**: Реализовать ВСЕ строительные блоки из Flutter, чтобы в nebula-parameter и других крейтах можно было легко собирать UI без борьбы с layout'ом.

**Дата**: 2025-10-16

---

## 📊 Статистика Реализации

| Категория | Всего | Реализовано | % |
|-----------|-------|-------------|---|
| **Базовые виджеты** | 10 | 0 | 0% |
| **Layout виджеты** | 18 | 0 | 0% |
| **Scroll виджеты** | 8 | 0 | 0% |
| **Sliver виджеты** | 6 | 0 | 0% |
| **Animation виджеты** | 12 | 0 | 0% |
| **Input & Interaction** | 8 | 0 | 0% |
| **Text & Rich Content** | 6 | 0 | 0% |
| **Painting & Effects** | 10 | 0 | 0% |
| **Focus & Navigation** | 9 | 0 | 0% |
| **Platform-specific** | 3 | 0 | 0% |
| **TYPES (уже есть)** | 50+ | 50+ | 100% ✅ |
| **CONTROLLERS (уже есть)** | 7 | 7 | 100% ✅ |
| **ВСЕГО ВИДЖЕТОВ** | **90** | **0** | **0%** |

---

## 🎯 ПРИОРИТЕТЫ ДЛЯ nebula-parameter-ui

### **P0 - КРИТИЧНЫЕ** (Без них вообще ничего не сделать)
Нужны для базового отображения параметров:

1. **Container** - основа всех виджетов
2. **Text** - отображение текста
3. **Row/Column** - раскладка элементов
4. **Padding** - отступы
5. **Spacer** - пространство между элементами

### **P1 - ОЧЕНЬ ВАЖНЫЕ** (Для интерактивности)
Нужны для работы с параметрами:

6. **GestureDetector** - клики, наведение
7. **TextField** (EditableText) - ввод текста
8. **Checkbox** - галочки
9. **Slider** - ползунки
10. **Dropdown/Select** - выбор из списка
11. **Button** - кнопки

### **P2 - ВАЖНЫЕ** (Для удобства)
Улучшают UX:

12. **ScrollView** - прокрутка длинных списков
13. **Stack** - наложение элементов
14. **Align/Center** - выравнивание
15. **Expanded/Flexible** - гибкая раскладка
16. **ListView** - списки
17. **Opacity** - прозрачность
18. **Transform** - трансформации

### **P3 - ЖЕЛАТЕЛЬНЫЕ** (Для красоты)
Анимации и эффекты:

19. **AnimatedContainer** - анимированные изменения
20. **AnimatedOpacity** - плавное появление/скрытие
21. **ClipRRect** - скругленные углы
22. **DecoratedBox** - фоновые эффекты
23. **CustomPaint** - кастомное рисование

---

## 🏗️ ФАЗЫ РЕАЛИЗАЦИИ

### **ФАЗА 1: FOUNDATION** (Неделя 1-2) - 15 виджетов
**Цель**: Можно собрать базовый UI для параметров

#### Базовые виджеты (5)
- [ ] **Container** - основа (200 LOC)
  - Decoration (color, border, shadow, radius)
  - Padding
  - Size constraints
  - Child widget

- [ ] **Text** - текст (150 LOC)
  - TextStyle support
  - Alignment
  - Max lines
  - Overflow handling

- [ ] **Image** - изображения (100 LOC)
  - Asset loading
  - Network loading
  - Size/fit modes

- [ ] **Spacer** - пустое пространство (50 LOC)
  - Flexible spacing
  - Fixed spacing

- [ ] **SizedBox** - фиксированный размер (30 LOC)
  - Width/height constraints

#### Layout виджеты (6)
- [ ] **Row** - горизонтальный layout (200 LOC)
  - MainAxisAlignment
  - CrossAxisAlignment
  - MainAxisSize

- [ ] **Column** - вертикальный layout (200 LOC)
  - То же что Row, но вертикально

- [ ] **Padding** - виджет padding'а (80 LOC)
  - EdgeInsets support
  - Child wrapper

- [ ] **Center** - центрирование (50 LOC)
  - Horizontal/vertical centering

- [ ] **Align** - выравнивание (100 LOC)
  - Alignment support
  - FractionalOffset support

- [ ] **Stack** - абсолютное позиционирование (150 LOC)
  - Children stacking
  - Positioned children
  - Alignment

#### Input виджеты (4)
- [ ] **GestureDetector** - жесты (300 LOC) ⭐
  - onTap, onDoubleTap
  - onLongPress
  - onPanStart/Update/End
  - onScaleStart/Update/End

- [ ] **MouseRegion** - мышь (100 LOC)
  - onEnter, onExit, onHover

- [ ] **InkWell** - material ripple (150 LOC)
  - Tap feedback
  - Hover effect

- [ ] **Listener** - низкоуровневые события (80 LOC)
  - PointerDown/Up/Move

**Итого Фаза 1**: ~1,940 LOC

---

### **ФАЗА 2: INTERACTION** (Неделя 3-4) - 18 виджетов
**Цель**: Полноценные формы с вводом данных

#### Text Input (4)
- [ ] **TextField** - текстовое поле (400 LOC) ⭐⭐
  - Decoration
  - Controller
  - Validation
  - onChanged callback
  - Prefix/suffix icons

- [ ] **EditableText** - редактируемый текст (300 LOC)
  - Cursor
  - Selection
  - TextEditingController

- [ ] **TextFormField** - поле с валидацией (200 LOC)
  - Form integration
  - Validators

- [ ] **SelectableText** - выделяемый текст (150 LOC)
  - Text selection
  - Copy support

#### Selection виджеты (5)
- [ ] **Checkbox** - галочка (120 LOC)
  - Checked/unchecked state
  - Tristate support
  - onChanged callback

- [ ] **Radio** - радиокнопка (120 LOC)
  - Group support
  - Value selection

- [ ] **Switch** - переключатель (150 LOC)
  - On/off state
  - Animation

- [ ] **Slider** - ползунок (200 LOC)
  - Min/max/value
  - Divisions
  - Label

- [ ] **DropdownButton** - выпадающий список (250 LOC)
  - Items
  - Selected value
  - Custom builder

#### Scrolling (4)
- [ ] **SingleChildScrollView** - прокрутка (200 LOC)
  - Vertical/horizontal
  - ScrollController
  - Physics

- [ ] **ListView** - список (300 LOC) ⭐
  - Builder pattern
  - Separator
  - Lazy loading

- [ ] **GridView** - сетка (300 LOC)
  - Grid delegate
  - Builder pattern

- [ ] **Scrollbar** - полоса прокрутки (150 LOC)
  - Auto-hide
  - Thumb dragging

#### Layout Advanced (5)
- [ ] **Expanded** - расширяемый виджет (80 LOC)
  - Flex factor
  - Fill available space

- [ ] **Flexible** - гибкий виджет (100 LOC)
  - Flex factor
  - FlexFit (tight/loose)

- [ ] **Wrap** - перенос виджетов (200 LOC)
  - Direction
  - Spacing/RunSpacing
  - Alignment

- [ ] **ConstrainedBox** - ограничения размера (80 LOC)
  - BoxConstraints

- [ ] **AspectRatio** - соотношение сторон (100 LOC)
  - Aspect ratio constraint

**Итого Фаза 2**: ~3,380 LOC

---

### **ФАЗА 3: ADVANCED INTERACTION** (Неделя 5-6) - 12 виджетов
**Цель**: Drag & drop, focus, сложные взаимодействия

#### Drag & Drop (3)
- [ ] **Draggable** - перетаскиваемый виджет (300 LOC) ⭐
  - Data payload
  - Feedback widget
  - onDragStarted/End

- [ ] **DragTarget** - цель для drop (250 LOC)
  - onWillAccept
  - onAccept
  - Builder for highlight

- [ ] **Dismissible** - свайп для удаления (200 LOC)
  - Direction
  - onDismissed
  - Background widget

#### Focus (4)
- [ ] **Focus** - фокус виджет (150 LOC)
  - FocusNode
  - onFocusChange
  - Auto focus

- [ ] **FocusScope** - область фокуса (200 LOC)
  - Focus tree
  - Traversal policy

- [ ] **FocusTraversalGroup** - группа навигации (150 LOC)
  - Tab order
  - Policy

- [ ] **AutofillGroup** - автозаполнение (180 LOC)
  - Form autofill

#### Pointer Control (3)
- [ ] **IgnorePointer** - игнорировать события (50 LOC)
  - Disable interaction

- [ ] **AbsorbPointer** - поглощать события (50 LOC)
  - Prevent hit testing below

- [ ] **InteractiveViewer** - zoom/pan (400 LOC) ⭐
  - Pan gesture
  - Zoom gesture
  - Transformations

#### Navigation (2)
- [ ] **Navigator** - навигация (500 LOC) ⭐⭐
  - Route stack
  - Push/pop
  - Route transitions

- [ ] **Overlay** - наложение (250 LOC)
  - OverlayEntry
  - Floating widgets

**Итого Фаза 3**: ~2,680 LOC

---

### **ФАЗА 4: ANIMATION & EFFECTS** (Неделя 7-8) - 18 виджетов
**Цель**: Красивые анимации и эффекты

#### Animated Widgets (8)
- [ ] **AnimatedContainer** - анимированный container (200 LOC)
  - Animated properties
  - Duration/curve

- [ ] **AnimatedOpacity** - анимированная прозрачность (100 LOC)
  - Fade in/out

- [ ] **AnimatedPadding** - анимированный padding (100 LOC)

- [ ] **AnimatedAlign** - анимированное выравнивание (100 LOC)

- [ ] **AnimatedPositioned** - анимированная позиция (120 LOC)

- [ ] **AnimatedSize** - анимированный размер (120 LOC)

- [ ] **AnimatedSwitcher** - смена виджета с анимацией (200 LOC)

- [ ] **AnimatedBuilder** - кастомная анимация (150 LOC)

#### Transitions (6)
- [ ] **FadeTransition** - переход с затуханием (80 LOC)

- [ ] **ScaleTransition** - переход с масштабом (80 LOC)

- [ ] **SlideTransition** - переход со слайдом (100 LOC)

- [ ] **RotationTransition** - переход с вращением (80 LOC)

- [ ] **SizeTransition** - переход с изменением размера (100 LOC)

- [ ] **PositionedTransition** - переход позиции (100 LOC)

#### Effects (4)
- [ ] **Opacity** - прозрачность (50 LOC)

- [ ] **Transform** - трансформация (150 LOC)
  - Rotate/scale/translate
  - Origin/alignment

- [ ] **RotatedBox** - повернутый виджет (80 LOC)

- [ ] **FractionalTranslation** - частичный сдвиг (80 LOC)

**Итого Фаза 4**: ~1,890 LOC

---

### **ФАЗА 5: PAINTING & GRAPHICS** (Неделя 9-10) - 10 виджетов
**Цель**: Кастомное рисование и визуальные эффекты

#### Custom Painting (2)
- [ ] **CustomPaint** - кастомное рисование (500 LOC) ⭐⭐⭐
  - Painter interface
  - Canvas API
  - Hit testing

- [ ] **CustomPainter** - интерфейс рисования (300 LOC)
  - paint() method
  - shouldRepaint()

#### Clipping (4)
- [ ] **ClipRect** - прямоугольное обрезание (80 LOC)

- [ ] **ClipRRect** - обрезание со скруглением (100 LOC)

- [ ] **ClipOval** - овальное обрезание (80 LOC)

- [ ] **ClipPath** - обрезание по пути (150 LOC)

#### Visual Effects (4)
- [ ] **DecoratedBox** - декорированный box (120 LOC)
  - BoxDecoration support

- [ ] **BackdropFilter** - фильтр фона (150 LOC)
  - Blur effect
  - Image filters

- [ ] **ShaderMask** - маска с шейдером (200 LOC)
  - Gradient mask
  - Blend modes

- [ ] **ColorFiltered** - фильтр цвета (100 LOC)
  - ColorFilter
  - BlendMode support

**Итого Фаза 5**: ~1,780 LOC

---

### **ФАЗА 6: ADVANCED SCROLLING** (Неделя 11-12) - 14 виджетов
**Цель**: Продвинутая прокрутка для сложных UI

#### Scroll Views (4)
- [ ] **CustomScrollView** - кастомная прокрутка (400 LOC)
  - Sliver support
  - Scroll controller

- [ ] **NestedScrollView** - вложенная прокрутка (500 LOC)
  - Header/body coordination

- [ ] **PageView** - страничная прокрутка (300 LOC)
  - PageController
  - Snap to page

- [ ] **ListWheelScrollView** - колесо прокрутки (350 LOC)
  - 3D effect
  - Item extent

#### Slivers (6)
- [ ] **SliverList** - sliver список (250 LOC)
  - Delegate pattern
  - Lazy building

- [ ] **SliverGrid** - sliver сетка (300 LOC)
  - Grid delegate

- [ ] **SliverAppBar** - sliver app bar (400 LOC)
  - Collapsing header
  - Pin/float

- [ ] **SliverPadding** - sliver padding (100 LOC)

- [ ] **SliverToBoxAdapter** - sliver адаптер (80 LOC)
  - Convert box child to sliver

- [ ] **SliverFillRemaining** - sliver заполнение (150 LOC)
  - Fill remaining space

#### Scroll Config (4)
- [ ] **ScrollConfiguration** - конфигурация прокрутки (200 LOC)
  - Physics
  - Behavior

- [ ] **GlowingOverscrollIndicator** - индикатор перепрокрутки (200 LOC)
  - Glow effect

- [ ] **ScrollNotification** - уведомления прокрутки (150 LOC)
  - Listener pattern

- [ ] **NotificationListener** - слушатель уведомлений (100 LOC)

**Итого Фаза 6**: ~3,480 LOC

---

### **ФАЗА 7: PLATFORM & UTILITIES** (Неделя 13-14) - 13 виджетов
**Цель**: Платформенные виджеты и утилиты

#### Builders (5)
- [ ] **Builder** - виджет строитель (50 LOC)
  - BuildContext access

- [ ] **LayoutBuilder** - layout строитель (150 LOC)
  - Constraints-based building

- [ ] **OrientationBuilder** - ориентация строитель (80 LOC)
  - Portrait/landscape

- [ ] **StreamBuilder** - stream строитель (200 LOC)
  - Reactive updates

- [ ] **FutureBuilder** - future строитель (150 LOC)
  - Async data loading

#### Platform (5)
- [ ] **SafeArea** - безопасная область (120 LOC)
  - System UI insets

- [ ] **MediaQuery** - медиа запросы (200 LOC)
  - Screen size
  - Device info

- [ ] **Theme** - тема виджет (150 LOC)
  - Theme data access

- [ ] **InheritedWidget** - наследуемый виджет (250 LOC)
  - Data propagation
  - Rebuild optimization

- [ ] **InheritedModel** - наследуемая модель (300 LOC)
  - Aspect-based updates

#### Utilities (3)
- [ ] **Hero** - hero переход (300 LOC)
  - Shared element transition

- [ ] **Placeholder** - плейсхолдер (50 LOC)
  - Debug widget

- [ ] **ErrorWidget** - виджет ошибки (100 LOC)
  - Error display

**Итого Фаза 7**: ~2,100 LOC

---

## 📈 ОБЩАЯ СТАТИСТИКА

### По фазам:
| Фаза | Виджетов | LOC | Недели |
|------|----------|-----|--------|
| Фаза 1: Foundation | 15 | ~1,940 | 1-2 |
| Фаза 2: Interaction | 18 | ~3,380 | 3-4 |
| Фаза 3: Advanced Interaction | 12 | ~2,680 | 5-6 |
| Фаза 4: Animation & Effects | 18 | ~1,890 | 7-8 |
| Фаза 5: Painting & Graphics | 10 | ~1,780 | 9-10 |
| Фаза 6: Advanced Scrolling | 14 | ~3,480 | 11-12 |
| Фаза 7: Platform & Utilities | 13 | ~2,100 | 13-14 |
| **ВСЕГО** | **100** | **~17,250** | **14** |

### Минимальный набор для nebula-parameter-ui (Фаза 1):
- 15 виджетов
- ~2,000 LOC
- 1-2 недели работы
- После этого уже можно собирать базовые формы параметров

### Полный набор для продакшена (Все фазы):
- 100 виджетов
- ~17,000 LOC
- 14 недель (~3.5 месяца)
- После этого полная совместимость с Flutter виджетами

---

## 🎯 РЕКОМЕНДУЕМАЯ СТРАТЕГИЯ

### Вариант A: Минимум сначала (РЕКОМЕНДУЮ)
1. **Фаза 1** (1-2 недели) - базовые виджеты
2. Используем в nebula-parameter-ui, тестируем
3. Выявляем что еще нужно срочно
4. **Фаза 2** (1-2 недели) - формы и интерактивность
5. Снова тестируем в реальном проекте
6. Дальше по потребности

### Вариант B: Все сразу
- Реализуем все 100 виджетов за 3-4 месяца
- Риск: может что-то не понадобится
- Плюс: полная библиотека сразу

### Вариант C: По запросу
- Реализуем только то, что нужно прямо сейчас
- Самый гибкий подход
- Минус: может быть хаотично

---

## 🚀 НЕМЕДЛЕННЫЕ ДЕЙСТВИЯ

### Сегодня:
1. Создать структуру `src/widgets/`
2. Определить базовый trait `Widget`
3. Начать с **Container** (самый базовый)

### Эта неделя:
1. Реализовать 5 базовых виджетов (Container, Text, Spacer, Row, Column)
2. Создать примеры использования
3. Интегрировать в nebula-parameter-ui для тестирования

### Следующая неделя:
1. GestureDetector + MouseRegion (интерактивность)
2. TextField + Checkbox + Slider (формы)
3. ScrollView + ListView (прокрутка)

---

## 📝 СТРУКТУРА МОДУЛЕЙ

```rust
nebula-ui/
└── src/
    └── widgets/
        ├── mod.rs              // Экспорты всех виджетов
        ├── widget.rs           // Базовый trait Widget
        │
        ├── primitives/         // Базовые виджеты
        │   ├── mod.rs
        │   ├── container.rs
        │   ├── text.rs
        │   ├── image.rs
        │   ├── spacer.rs
        │   └── sized_box.rs
        │
        ├── layout/             // Layout виджеты
        │   ├── mod.rs
        │   ├── row.rs
        │   ├── column.rs
        │   ├── stack.rs
        │   ├── padding.rs
        │   ├── align.rs
        │   ├── center.rs
        │   ├── expanded.rs
        │   ├── flexible.rs
        │   └── wrap.rs
        │
        ├── input/              // Input виджеты
        │   ├── mod.rs
        │   ├── gesture_detector.rs
        │   ├── mouse_region.rs
        │   ├── listener.rs
        │   ├── draggable.rs
        │   └── drag_target.rs
        │
        ├── forms/              // Form виджеты
        │   ├── mod.rs
        │   ├── text_field.rs
        │   ├── checkbox.rs
        │   ├── radio.rs
        │   ├── switch.rs
        │   ├── slider.rs
        │   └── dropdown.rs
        │
        ├── scrolling/          // Scroll виджеты
        │   ├── mod.rs
        │   ├── scroll_view.rs
        │   ├── list_view.rs
        │   ├── grid_view.rs
        │   ├── page_view.rs
        │   └── scrollbar.rs
        │
        ├── animation/          // Animation виджеты
        │   ├── mod.rs
        │   ├── animated_container.rs
        │   ├── animated_opacity.rs
        │   ├── transitions.rs
        │   └── animated_builder.rs
        │
        ├── painting/           // Painting виджеты
        │   ├── mod.rs
        │   ├── custom_paint.rs
        │   ├── decorated_box.rs
        │   ├── opacity.rs
        │   ├── transform.rs
        │   └── clips.rs
        │
        └── platform/           // Platform виджеты
            ├── mod.rs
            ├── safe_area.rs
            ├── media_query.rs
            └── builders.rs
```

---

## ✅ КРИТЕРИИ ГОТОВНОСТИ

Каждый виджет должен иметь:
- [ ] Документацию с примерами
- [ ] Тесты (минимум 3-5)
- [ ] Builder pattern для настройки
- [ ] Integration с существующими types
- [ ] Integration с controllers (где применимо)
- [ ] Пример использования в examples/

---

**Готовы начинать? С какой фазы стартуем?** 🚀

Рекомендую:
1. **Фаза 1** - чтобы быстро получить рабочие результаты в nebula-parameter-ui
2. Потом уже расширять по потребности
