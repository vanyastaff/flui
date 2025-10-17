# Nebula-UI Architecture Roadmap
## Полный план реализации Flutter-like архитектуры

> **Цель**: Построить полную Flutter-подобную архитектуру БЕЗ написания конкретных виджетов.
> Сосредоточиться на архитектурных компонентах, чтобы виджеты потом можно было писать правильно.

---

## ✅ ЭТАП 0: Завершено (613 тестов)

### Базовый фундамент
- [x] **Widget trait** - базовый trait для всех виджетов ([widget.rs](src/widgets/widget.rs))
- [x] **Element trait** - базовый trait для элементов ([framework.rs](src/widgets/framework.rs))
- [x] **ElementTree** - управление деревом элементов
- [x] **BuildContext** - доступ к дереву и сервисам
- [x] **ElementId** - уникальные идентификаторы
- [x] **BoxConstraints** - система layout constraints ([box_constraints.rs](src/rendering/box_constraints.rs))
- [x] **RenderObject trait** - базовый trait для layout/paint ([render_object.rs](src/rendering/render_object.rs))
- [x] **RenderBox** - базовая реализация box protocol
- [x] **RenderProxyBox** - проброс layout к child
- [x] **ComponentElement** - для StatelessWidget
- [x] **StatefulElement** - для StatefulWidget (базовая структура)
- [x] **SingleChildElement** - для виджетов с одним child
- [x] **MultiChildElement** - для виджетов с несколькими children

---

## 🔨 ЭТАП 1: Завершение Element System (Приоритет: Высокий)

### 1.1 Интеграция Widget ↔ Element
**Цель**: Связать Widget и Element так, чтобы widgets могли создавать правильные elements.

**Задачи**:
- [ ] Реализовать `Widget::create_element()` для всех базовых типов
  - StatelessWidget → ComponentElement
  - StatefulWidget → StatefulElement
  - RenderObjectWidget → RenderObjectElement

- [ ] Добавить метод `Element::widget()` для доступа к текущему виджету
  - Хранить widget в element
  - Возвращать &dyn Widget

- [ ] Реализовать `can_update_widget()` полностью
  - Проверка TypeId
  - Проверка Key (если есть)
  - Использование в update логике

**Файлы**:
- `src/widgets/framework.rs` - дополнить Element implementations
- `src/widgets/widget.rs` - добавить default impl для create_element()

**Тесты**: +10-15 тестов

---

### 1.2 Lifecycle Methods для Element
**Цель**: Полный жизненный цикл элемента с правильными callbacks.

**Задачи**:
- [ ] Реализовать `ComponentElement::rebuild()` полностью
  - Вызов `StatelessWidget::build()`
  - Создание/обновление child element
  - Применение can_update_widget logic

- [ ] Реализовать `StatefulElement::rebuild()` полностью
  - Вызов `State::build()`
  - Управление child element
  - Сохранение state между rebuilds

- [ ] Добавить `Element::visit_children()` реализацию
  - Для ComponentElement (1 child)
  - Для StatefulElement (1 child)
  - Для SingleChildElement (1 child)
  - Для MultiChildElement (N children)

- [ ] Реализовать `Element::parent()` и `Element::depth()`
  - Хранить parent_id в каждом element
  - Вычислять depth для dirty sorting

**Файлы**:
- `src/widgets/framework.rs` - завершить ComponentElement, StatefulElement

**Тесты**: +15-20 тестов

---

### 1.3 ElementTree Management
**Цель**: Полноценное управление деревом элементов с dirty tracking.

**Задачи**:
- [ ] Реализовать `ElementTree::rebuild_dirty()` полностью
  - Сортировка по depth (parent перед child)
  - Вызов rebuild() только на dirty elements
  - Очистка dirty list

- [ ] Добавить `ElementTree::mount_root()`
  - Создание root element из widget
  - Вызов mount() на root
  - Сохранение в tree

- [ ] Реализовать `ElementTree::unmount_subtree()`
  - Рекурсивный unmount всех children
  - Вызов dispose() на state
  - Очистка из elements HashMap

- [ ] Добавить `ElementTree::depth_of()`
  - Подсчет глубины элемента в дереве
  - Для сортировки при rebuild

**Файлы**:
- `src/widgets/framework.rs` - ElementTree implementation

**Тесты**: +10-12 тестов

---

## 🔨 ЭТАП 2: Element ↔ RenderObject Integration (Приоритет: Высокий)

### 2.1 RenderObjectElement
**Цель**: Связать Element и RenderObject вместе.

**Задачи**:
- [ ] Создать `RenderObjectElement` trait/struct
  - Хранит RenderObject
  - Управляет lifecycle render object
  - Связь с Element tree

- [ ] Реализовать `SingleChildRenderObjectElement`
  - Для Container, Padding, Align, etc.
  - Создает RenderObject при mount
  - Обновляет RenderObject при update
  - Layout/Paint через RenderObject

- [ ] Реализовать `MultiChildRenderObjectElement`
  - Для Row, Column, Stack, etc.
  - Управляет несколькими children
  - Передает constraints детям

- [ ] Реализовать `LeafRenderObjectElement`
  - Для Text, Image, etc.
  - Нет children
  - Прямой paint

**Файлы**:
- `src/widgets/framework.rs` - новые element types
- `src/rendering/render_object.rs` - дополнить интеграцию

**Тесты**: +15-20 тестов

---

### 2.2 RenderObject Lifecycle
**Цель**: Правильный lifecycle для RenderObject с attach/detach.

**Задачи**:
- [ ] Добавить `RenderObject::attach()` и `detach()`
  - Вызов при mount/unmount element
  - Подключение к PipelineOwner
  - Регистрация для layout/paint

- [ ] Реализовать `RenderObject::mark_needs_layout()`
  - Пометить как dirty
  - Уведомить parent
  - Добавить в PipelineOwner queue

- [ ] Реализовать `RenderObject::mark_needs_paint()`
  - Пометить как dirty for paint
  - Уведомить repaint boundary
  - Запланировать repaint

- [ ] Добавить `RenderObject::perform_layout()`
  - Вызов layout() с constraints
  - Сохранение размера
  - Layout children

**Файлы**:
- `src/rendering/render_object.rs` - lifecycle methods

**Тесты**: +10-12 тестов

---

### 2.3 ParentData System
**Цель**: Данные для позиционирования детей (Flutter's ParentData).

**Задачи**:
- [ ] Создать `ParentData` trait
  - Базовый trait для данных о позиции child
  - Используется RenderObject для позиционирования

- [ ] Реализовать `BoxParentData`
  - Offset для позиционирования child
  - Используется в RenderBox

- [ ] Реализовать `FlexParentData`
  - flex: Option<f32> для Flexible/Expanded
  - fit: FlexFit для stretch/loose

- [ ] Интегрировать в RenderObject
  - setup_parent_data() метод
  - Доступ через child.parent_data()

**Файлы**:
- `src/rendering/parent_data.rs` - новый файл
- `src/rendering/render_object.rs` - интеграция

**Тесты**: +8-10 тестов

---

## 🔨 ЭТАП 3: State Management Foundation (Приоритет: Высокий)

### 3.1 State Lifecycle
**Цель**: Полный lifecycle для StatefulWidget's State.

**Задачи**:
- [ ] Реализовать `State::init_state()` callback
  - Вызов при первом mount
  - Инициализация state
  - Подписка на streams/futures

- [ ] Реализовать `State::did_update_widget()`
  - Вызов при update с новым widget
  - Сравнение old vs new
  - Обновление подписок if needed

- [ ] Реализовать `State::did_change_dependencies()`
  - Вызов при изменении InheritedWidget
  - Повторная подписка на зависимости
  - Запрос rebuild если нужно

- [ ] Реализовать `State::deactivate()` и `activate()`
  - Вызов при перемещении в дереве
  - Временное отключение listeners

- [ ] Реализовать `State::dispose()`
  - Вызов при unmount
  - Очистка resources (timers, streams, etc.)
  - Отписка от всех listeners

**Файлы**:
- `src/widgets/framework.rs` - State lifecycle в StatefulElement

**Тесты**: +15-18 тестов

---

### 3.2 setState() Implementation
**Цель**: Правильная реализация `setState()` для State.

**Задачи**:
- [ ] Реализовать `State::set_state()`
  - Принимает closure для изменения state
  - Вызывает mark_needs_build()
  - Проверка на disposed state

- [ ] Добавить проверки безопасности
  - Нельзя вызывать в dispose()
  - Нельзя вызывать в build() (warning)
  - Нельзя вызывать после unmount

- [ ] Интегрировать с BuildContext
  - context.mark_dirty() вызывает rebuild
  - Добавление в ElementTree.dirty list

**Файлы**:
- `src/widgets/framework.rs` - State trait и StatefulElement

**Тесты**: +8-10 тестов

---

### 3.3 GlobalKey System
**Цель**: Доступ к State и RenderObject через GlobalKey.

**Задачи**:
- [ ] Создать `GlobalKey<T>` struct
  - Generic over State type
  - Уникальный идентификатор
  - Регистрация в BuildOwner

- [ ] Реализовать `GlobalKey::current_state()`
  - Поиск state по ключу
  - Возврат Option<&T>
  - Проверка типа

- [ ] Реализовать `GlobalKey::current_context()`
  - Доступ к BuildContext элемента
  - Для вызова методов

- [ ] Добавить в BuildOwner registry
  - HashMap<GlobalKeyId, (ElementId, *State)>
  - Регистрация при mount
  - Удаление при unmount

**Файлы**:
- `src/core/key.rs` - дополнить GlobalKey
- `src/widgets/framework.rs` - интеграция в BuildOwner

**Тесты**: +10-12 тестов

---

## 🔨 ЭТАП 4: InheritedWidget System (Приоритет: Высокий)

### 4.1 InheritedWidget Trait
**Цель**: Передача данных вниз по дереву эффективно.

**Задачи**:
- [ ] Создать `InheritedWidget` trait
  - Extends Widget
  - `update_should_notify(old: &Self) -> bool` метод
  - Хранение данных

- [ ] Создать `InheritedElement`
  - Хранит dependents: HashSet<ElementId>
  - `register_dependent(id)` метод
  - `unregister_dependent(id)` метод
  - Уведомление dependents при update

- [ ] Реализовать update logic
  - Сравнение old vs new widget
  - Вызов update_should_notify()
  - Пометка dependents как dirty

**Файлы**:
- `src/widgets/inherited.rs` - новый файл
- `src/widgets/framework.rs` - InheritedElement

**Тесты**: +12-15 тестов

---

### 4.2 BuildContext::dependOnInheritedWidgetOfExactType()
**Цель**: API для доступа к InheritedWidget из потомков.

**Задачи**:
- [ ] Реализовать `BuildContext::depend_on_inherited<T>()`
  - Поиск вверх по дереву
  - Поиск InheritedWidget of type T
  - Регистрация как dependent

- [ ] Добавить `BuildContext::find_ancestor_element_of_exact_type<T>()`
  - Общий метод поиска вверх
  - Используется internally

- [ ] Реализовать dependency tracking
  - Элемент регистрируется как зависимый
  - При update InheritedWidget → rebuild dependent
  - Очистка dependencies при unmount

**Файлы**:
- `src/widgets/framework.rs` - BuildContext methods

**Тесты**: +10-12 тестов

---

### 4.3 InheritedModel и InheritedNotifier
**Цель**: Более гранулярный контроль dependencies.

**Задачи**:
- [ ] Создать `InheritedModel<T>` trait
  - Extends InheritedWidget
  - `update_should_notify_dependent(aspect)` метод
  - Для частичных обновлений

- [ ] Создать `InheritedNotifier<T: Listenable>`
  - Автоматическая подписка на ChangeNotifier
  - Rebuild при notifyListeners()

- [ ] Реализовать aspect-based dependencies
  - Зависимость только от части данных
  - Более эффективные updates

**Файлы**:
- `src/widgets/inherited.rs` - InheritedModel, InheritedNotifier

**Тесты**: +8-10 тестов

---

## 🔨 ЭТАП 5: BuildOwner & Pipeline (Приоритет: Средний)

### 5.1 BuildOwner
**Цель**: Глобальное управление build процессом.

**Задачи**:
- [ ] Создать `BuildOwner` struct
  - Управление build scope
  - GlobalKey registry
  - FocusManager
  - InactiveElements pool

- [ ] Реализовать `BuildOwner::build_scope()`
  - Принимает callback
  - Перестройка всех dirty elements
  - Сортировка по depth

- [ ] Добавить frame budget
  - Лимит времени на rebuild (16ms для 60fps)
  - Defer rebuilds если превышен
  - Приоритизация critical rebuilds

- [ ] Реализовать inactive elements pool
  - Переиспользование elements
  - Deactivate вместо unmount
  - Реактивация при reparenting

**Файлы**:
- `src/widgets/build_owner.rs` - новый файл

**Тесты**: +15-18 тестов

---

### 5.2 PipelineOwner
**Цель**: Управление layout и paint pipeline.

**Задачи**:
- [ ] Создать `PipelineOwner` struct
  - Layout queue
  - Paint queue
  - Semantics queue

- [ ] Реализовать `flush_layout()`
  - Layout всех dirty render objects
  - От корня к листьям
  - Вызов performLayout()

- [ ] Реализовать `flush_paint()`
  - Paint всех dirty render objects
  - Учет RepaintBoundary
  - Вызов paint() с Painter

- [ ] Реализовать `flush_semantics()`
  - Обновление semantics tree
  - Для accessibility

**Файлы**:
- `src/rendering/pipeline_owner.rs` - новый файл

**Тесты**: +12-15 тестов

---

### 5.3 SchedulerBinding Integration
**Цель**: Интеграция с egui's frame loop.

**Задачи**:
- [ ] Создать `SchedulerBinding`
  - Координация фреймов
  - Запланированные callbacks
  - Microtask queue

- [ ] Реализовать phases
  - **Idle** → ничего не происходит
  - **Build** → rebuild dirty widgets
  - **Layout** → flush layout queue
  - **Paint** → flush paint queue
  - **Post-frame** → callbacks после paint

- [ ] Добавить `schedule_frame()`
  - Запрос следующего frame
  - Вызов egui::Context::request_repaint()

- [ ] Реализовать `add_post_frame_callback()`
  - Callbacks после текущего frame
  - Для измерений после layout

**Файлы**:
- `src/platform/scheduler.rs` - новый файл

**Тесты**: +10-12 тестов

---

## 🔨 ЭТАП 6: Animation System (Приоритет: Средний)

### 6.1 Ticker System
**Цель**: Frame callbacks для animations.

**Задачи**:
- [ ] Создать `Ticker` struct
  - Frame callback на каждый vsync
  - Start/stop управление
  - Elapsed time tracking

- [ ] Создать `TickerProvider` trait
  - createTicker() метод
  - Mixin для State
  - Автоматический dispose

- [ ] Реализовать `SingleTickerProviderStateMixin`
  - Для State с одной анимацией
  - Один ticker per State
  - Auto-dispose при unmount

- [ ] Реализовать `TickerProviderStateMixin`
  - Для State с несколькими анимациями
  - Multiple tickers
  - Tracking всех tickers

**Файлы**:
- `src/animation/ticker.rs` - новый файл
- См. `docs/architecture/nebula_ticker_mixin.rs`

**Тесты**: +12-15 тестов

---

### 6.2 AnimationController
**Цель**: Контроллер для управления анимациями.

**Задачи**:
- [ ] Создать `AnimationController` struct
  - value: f64 (0.0 to 1.0)
  - duration: Duration
  - status: AnimationStatus
  - Два типа listeners (value + status)

- [ ] Реализовать control methods
  - `forward()` - запустить вперед
  - `reverse()` - запустить назад
  - `reset()` - сбросить к началу
  - `stop()` - остановить
  - `repeat()` - зациклить

- [ ] Реализовать listeners
  - `add_listener()` - на изменение value
  - `add_status_listener()` - на изменение status
  - `notify_listeners()` - уведомление

- [ ] Интеграция с Ticker
  - Tick callback обновляет value
  - Вычисление прогресса (elapsed / duration)
  - Уведомление listeners

**Файлы**:
- `src/animation/animation_controller.rs` - новый файл
- См. `docs/architecture/nebula_anim_controller.rs`
- См. `docs/architecture/nebula_anim_summary.txt`

**Тесты**: +15-18 тестов

---

### 6.3 Curves & Tweens
**Цель**: Easing functions и interpolation.

**Задачи**:
- [ ] Создать `Curve` trait
  - `transform(t: f64) -> f64` метод
  - Easing functions

- [ ] Реализовать standard curves
  - Linear
  - EaseIn, EaseOut, EaseInOut
  - FastOutSlowIn (Material)
  - Elastic, Bounce curves

- [ ] Создать `Tween<T>` struct
  - begin и end values
  - `lerp(t: f64) -> T` метод
  - Generic over type

- [ ] Реализовать `CurvedAnimation`
  - Применяет Curve к AnimationController
  - Возвращает curved value

- [ ] Реализовать `Animation<T>`
  - Применяет Tween к controller
  - value() возвращает T

**Файлы**:
- `src/animation/curves.rs`
- `src/animation/tween.rs`

**Тесты**: +12-15 тестов

---

## 🔨 ЭТАП 7: Gesture System (Приоритет: Низкий)

### 7.1 Hit Testing
**Цель**: Определение какой виджет получил клик/touch.

**Задачи**:
- [ ] Создать `HitTestResult` struct
  - Список попавших render objects
  - Path от корня к target

- [ ] Реализовать `RenderObject::hit_test()`
  - Проверка bounds
  - Рекурсивный вызов для children
  - Добавление в HitTestResult

- [ ] Реализовать `RenderObject::hit_test_self()`
  - Проверка только этого object
  - Без children

- [ ] Добавить `HitTestBehavior` enum
  - Opaque - всегда accepts
  - Translucent - pass through to child
  - Deferring - defer to child

**Файлы**:
- `src/rendering/hit_test.rs` - новый файл

**Тесты**: +10-12 тестов

---

### 7.2 Gesture Recognizers
**Цель**: Распознавание жестов (tap, drag, pinch, etc.).

**Задачи**:
- [ ] Создать `GestureRecognizer` trait
  - `add_pointer(event)` метод
  - `accept_gesture()` и `reject_gesture()`
  - State machine для gestures

- [ ] Реализовать `TapGestureRecognizer`
  - onTapDown, onTapUp, onTap callbacks
  - Timeout для double tap

- [ ] Реализовать `DragGestureRecognizer`
  - onStart, onUpdate, onEnd callbacks
  - Velocity calculation
  - Direction constraints

- [ ] Реализовать `ScaleGestureRecognizer`
  - Pinch to zoom/rotate
  - Multi-touch support

- [ ] Gesture Arena для конфликтов
  - Несколько recognizers конкурируют
  - Winner takes gesture

**Файлы**:
- `src/gestures/recognizer.rs` - новый файл
- `src/gestures/tap.rs`
- `src/gestures/drag.rs`
- `src/gestures/scale.rs`
- `src/gestures/arena.rs`

**Тесты**: +20-25 тестов

---

### 7.3 GestureDetector Widget
**Цель**: High-level API для gesture detection.

**Задачи**:
- [ ] Создать `GestureDetector` widget
  - Wrapper вокруг recognizers
  - Простой callback API
  - Behavior контроль

- [ ] Реализовать `RenderPointerListener`
  - RenderObject для pointer events
  - Dispatch к recognizers
  - Hit test behavior

**Файлы**:
- `src/widgets/gesture_detector.rs` - новый файл

**Тесты**: +8-10 тестов

---

## 🔨 ЭТАП 8: Platform Integration (Приоритет: Высокий)

### 8.1 NebulaApp Entry Point
**Цель**: Простой entry point для приложений.

**Задачи**:
- [ ] Создать `NebulaApp` struct
  - home: Box<dyn Widget> - root widget
  - title: String
  - theme: Theme
  - debug banners

- [ ] Реализовать `NebulaApp::run()`
  - Создание eframe application
  - Инициализация ElementTree
  - Интеграция с egui main loop

- [ ] Создать `NebulaAppState`
  - Внутреннее состояние app
  - ElementTree instance
  - PipelineOwner instance
  - Frame counter

**Файлы**:
- `src/platform/app.rs` - новый файл

**Тесты**: +5-8 тестов (integration)

---

### 8.2 Main Loop Integration
**Цель**: Интеграция three-tree с egui rendering.

**Задачи**:
- [ ] Реализовать `eframe::App::update()`
  - **Phase 1: Build** - rebuild dirty elements
  - **Phase 2: Layout** - flush layout queue
  - **Phase 3: Paint** - paint render objects
  - Request repaint if dirty

- [ ] Интеграция constraints
  - Получение размера от egui
  - Создание root BoxConstraints
  - Передача root render object

- [ ] Paint к egui::Painter
  - Преобразование Offset → egui::Pos2
  - Преобразование Size → egui::Vec2
  - Вызов egui drawing primitives

**Файлы**:
- `src/platform/app.rs` - update() implementation

**Тесты**: +8-10 integration тестов

---

### 8.3 Debug Tools
**Цель**: Инструменты для отладки и диагностики.

**Задачи**:
- [ ] Debug Banner
  - "DEBUG" label в углу
  - Показ в debug mode
  - Переключаемый

- [ ] Performance Overlay
  - FPS counter
  - Build time
  - Layout time
  - Paint time

- [ ] Widget Inspector
  - Highlight widget on hover
  - Show widget tree
  - Show properties

**Файлы**:
- `src/platform/debug.rs` - новый файл

**Тесты**: +5-8 тестов

---

## 🔨 ЭТАП 9: Optimization Features (Приоритет: Низкий)

### 9.1 RepaintBoundary
**Цель**: Кеширование paint для оптимизации.

**Задачи**:
- [ ] Создать `RepaintBoundary` widget
  - Marks boundary для repaint
  - Кеширует painted result

- [ ] Реализовать `RenderRepaintBoundary`
  - Separate layer для painting
  - Cache layer until marked dirty
  - Reduces repaints

**Файлы**:
- `src/widgets/repaint_boundary.rs` - новый файл

**Тесты**: +8-10 тестов

---

### 9.2 Viewport Culling
**Цель**: Не рендерить виджеты вне viewport.

**Задачи**:
- [ ] Реализовать `Viewport` struct
  - Visible area
  - Scroll offset
  - Bounds

- [ ] Добавить culling в RenderObject
  - Проверка visibility перед paint
  - Skip если вне viewport

- [ ] Интеграция с ScrollView
  - Передача viewport bounds
  - Dynamic child creation

**Файлы**:
- `src/rendering/viewport.rs` - новый файл

**Тесты**: +10-12 тестов

---

### 9.3 Layout Caching
**Цель**: Кеширование layout результатов.

**Задачи**:
- [ ] Добавить layout cache в RenderObject
  - Сохранять последние constraints
  - Сохранять вычисленный size
  - Reuse если constraints не изменились

- [ ] Intrinsic size caching
  - Кеш для intrinsic width/height
  - Invalidate при изменении

**Файлы**:
- `src/rendering/render_object.rs` - дополнить caching

**Тесты**: +8-10 тестов

---

## 📊 Итоговая статистика по этапам

| Этап | Компонентов | Тестов | Приоритет | Сложность |
|------|-------------|--------|-----------|-----------|
| 0. Завершено | 12 | 613 | - | - |
| 1. Element System | 10 | +35-47 | 🔴 Высокий | 🟡 Средняя |
| 2. Element↔Render | 8 | +33-42 | 🔴 Высокий | 🔴 Высокая |
| 3. State Management | 8 | +33-40 | 🔴 Высокий | 🟡 Средняя |
| 4. InheritedWidget | 6 | +30-37 | 🔴 Высокий | 🟡 Средняя |
| 5. BuildOwner | 6 | +37-45 | 🟡 Средний | 🔴 Высокая |
| 6. Animation | 8 | +39-48 | 🟡 Средний | 🟡 Средняя |
| 7. Gestures | 8 | +38-47 | 🟢 Низкий | 🟡 Средняя |
| 8. Platform | 6 | +18-26 | 🔴 Высокий | 🟡 Средняя |
| 9. Optimization | 6 | +26-32 | 🟢 Низкий | 🟢 Низкая |
| **ИТОГО** | **66** | **+289-364** | - | - |

**Финальная цель**: ~900-980 тестов (613 + 289-364)

---

## 🎯 Рекомендуемый порядок реализации

### Фаза 1: Критический путь (2-3 недели)
1. **ЭТАП 1**: Element System (высокий приоритет, средняя сложность)
2. **ЭТАП 2**: Element↔Render Integration (высокий приоритет, высокая сложность)
3. **ЭТАП 3**: State Management (высокий приоритет, средняя сложность)
4. **ЭТАП 8**: Platform Integration (высокий приоритет, средняя сложность)

**Результат**: Минимально работающая система для создания виджетов.

### Фаза 2: Расширенные возможности (1-2 недели)
5. **ЭТАП 4**: InheritedWidget (высокий приоритет, средняя сложность)
6. **ЭТАП 5**: BuildOwner & Pipeline (средний приоритет, высокая сложность)

**Результат**: Полноценная система для production виджетов.

### Фаза 3: Animations & Advanced (1-2 недели)
7. **ЭТАП 6**: Animation System (средний приоритет, средняя сложность)
8. **ЭТАП 7**: Gesture System (низкий приоритет, средняя сложность)

**Результат**: Интерактивные и анимированные виджеты.

### Фаза 4: Оптимизация (опционально)
9. **ЭТАП 9**: Optimization Features (низкий приоритет, низкая сложность)

**Результат**: Производительность для больших приложений.

---

## 🔑 Ключевые принципы

### 1. **Architecture First**
- НЕ писать конкретные виджеты сейчас
- Сосредоточиться на архитектурных traits и systems
- Виджеты будут простыми после правильной архитектуры

### 2. **Test-Driven**
- Каждый компонент должен иметь тесты
- Минимум 5-10 тестов на компонент
- Integration тесты для сложных взаимодействий

### 3. **Incremental**
- Реализовывать этапами
- Каждый этап должен компилироваться
- Можно использовать TODO комментарии для будущего

### 4. **Documentation**
- Документировать каждый trait и struct
- Примеры использования в doc comments
- README для каждого модуля

---

## 📚 Справочные материалы

### Документация
- [docs/architecture/nebula_arch_p1.txt](docs/architecture/nebula_arch_p1.txt) - Foundation & Structure
- [docs/architecture/nebula_arch_p2.txt](docs/architecture/nebula_arch_p2.txt) - Core Traits
- [docs/architecture/nebula_arch_p3.txt](docs/architecture/nebula_arch_p3.txt) - Widget Framework
- [docs/architecture/nebula_arch_p4.txt](docs/architecture/nebula_arch_p4.txt) - Rendering & Animation
- [docs/architecture/nebula_arch_p5.txt](docs/architecture/nebula_arch_p5.txt) - Controllers & Provider
- [docs/architecture/nebula_arch_p6.txt](docs/architecture/nebula_arch_p6.txt) - Optimizations

### Примеры кода
- [docs/architecture/nebula_anim_controller.rs](docs/architecture/nebula_anim_controller.rs) - AnimationController
- [docs/architecture/nebula_ticker_mixin.rs](docs/architecture/nebula_ticker_mixin.rs) - Ticker Mixin

---

## ✅ Чеклист готовности к написанию виджетов

После завершения всех этапов, мы сможем уверенно писать виджеты:

- [ ] Widget trait полностью работает
- [ ] Element lifecycle полностью реализован
- [ ] RenderObject интегрирован с Element
- [ ] State management работает (setState, dispose)
- [ ] InheritedWidget для dependency injection
- [ ] BuildOwner управляет rebuilds
- [ ] AnimationController для анимаций
- [ ] GestureDetector для interactions
- [ ] NebulaApp::run() запускает приложение
- [ ] Debug tools для отладки
- [ ] ~900 тестов проходят

**Тогда**: Любой виджет (Container, Text, Button, TextField) будет просто обёрткой над правильной архитектурой! 🚀

---

**Статус**: План готов к реализации
**Следующий шаг**: ЭТАП 1.1 - Интеграция Widget ↔ Element
