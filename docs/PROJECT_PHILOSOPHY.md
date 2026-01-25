# FLUI Project Philosophy

> **Дата**: 2026-01-24  
> **Автор**: Vanya (владелец проекта)  
> **Цель**: Формулировка философии и целей проекта FLUI

---

## 🎯 Основная концепция

**FLUI = Flutter подход + Rust экосистема + лучшее из других библиотек**

Мы создаем **новую библиотеку** для Rust, изучая и заимствуя лучшее из существующих решений:

### Источники вдохновения:

1. **Flutter** (`.flutter/` в проекте)
   - ✅ **Подход и удобство** - main inspiration
   - ✅ **Множество реализаций** - проверенные паттерны
   - ✅ **Декларативный API** - простой и понятный
   - ✅ **Three-tree architecture** - View → Element → Render
   - ✅ **Widget system** - композиция компонентов
   - ✅ **Hot reload** - быстрая разработка
   
2. **GPUI** (`.gpui/` в проекте)
   - ✅ **Написан на Rust** - нативная экосистема
   - ✅ **Современный стиль** - Rust idioms
   - ⚠️ **Сложен** - особенно для не-web разработчиков
   - ⚠️ **Понятен для HTML/CSS разработчиков** - но не для всех
   - ⚠️ **Множество недоделок** - незавершенная система
   - ⚠️ **Не полностью формулирован** - нечеткая архитектура
   
3. **Другие источники**:
   - Xilem (Linebender) - реактивные паттерны
   - Iced - Elm architecture
   - Druid - Lens pattern

---

## 📐 Философия FLUI

### Что мы берем от Flutter:

#### 1. **Декларативный подход**
```dart
// Flutter style
Container(
  padding: EdgeInsets.all(10),
  child: Text("Hello"),
)
```

```rust
// FLUI style (цель)
Container::new()
    .padding(EdgeInsets::all(10.0))
    .child(Text::new("Hello"))
```

**Почему**: Понятно, читаемо, композируется

#### 2. **Three-tree architecture**
```
Widget Tree (immutable) → Element Tree (mutable) → RenderObject Tree (layout/paint)
```

**Почему**: Проверено на практике, разделение ответственности

#### 3. **Lifecycle и Bindings**
- WidgetsBinding (build phase)
- RenderingBinding (layout/paint)
- GestureBinding (events)
- SchedulerBinding (frames)

**Почему**: Четкое разделение фаз, легко тестировать

#### 4. **Widget catalog**
- Stateless/Stateful widgets
- Layout widgets (Row, Column, Stack)
- Material/Cupertino components

**Почему**: Богатая библиотека готовых компонентов

---

### Что мы берем от GPUI:

#### 1. **Rust idioms**
```rust
// Type-safe unit system
Point<Pixels>  vs  Point<DevicePixels>

// Typestate pattern
Node<Unmounted> → Node<Mounted>

// Associated types
trait Element {
    type LayoutState: 'static;
    type PrepaintState: 'static;
}
```

**Почему**: Compile-time safety, zero-cost abstractions

#### 2. **Modern patterns**
- Arc<RwLock<T>> для sharing
- Callback registry для decoupling
- #[track_caller] для debugging
- Phase tracking для safety

**Почему**: Production-ready patterns from Zed editor

#### 3. **Platform abstraction**
```rust
pub trait Platform: Send + Sync {
    fn run(&self, on_ready: Box<dyn FnOnce()>);
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
    // ...
}
```

**Почему**: Чистая абстракция, легко добавлять платформы

---

### Что мы НЕ берем от GPUI:

#### ❌ **Сложность для не-web разработчиков**
GPUI ориентирован на HTML/CSS мышление:
```rust
// GPUI style - сложно без web background
div()
    .flex()
    .flex_row()
    .items_center()
    .gap_2()
    .px_4()
```

**FLUI подход**: Flutter-style API, понятный без web знаний:
```rust
// FLUI style - понятно всем
Row::new()
    .spacing(8.0)
    .padding(EdgeInsets::horizontal(16.0))
    .children(vec![...])
```

#### ❌ **Недоделки и нечеткая архитектура**
GPUI имеет много незавершенных частей и не всегда понятную структуру.

**FLUI подход**: 
- ✅ Четкая документация каждого решения (ADR)
- ✅ Завершенная архитектура перед реализацией
- ✅ Тесты для каждой фичи
- ✅ Примеры использования

#### ❌ **Слишком много магии**
GPUI использует много скрытых механизмов.

**FLUI подход**: Explicit is better than implicit (Rust philosophy)

---

## 🎨 Стиль FLUI API

### Принципы:

1. **Flutter-like naming**
   ```rust
   // Flutter names
   Container, Row, Column, Text, Padding
   StatelessWidget, StatefulWidget
   BuildContext, State
   
   // FLUI equivalent
   Container, Row, Column, Text, Padding
   StatelessView, StatefulView
   BuildContext, ViewState
   ```

2. **Rust idioms**
   ```rust
   // Builder pattern with bon
   #[builder]
   pub struct Container {
       padding: Option<EdgeInsets>,
       margin: Option<EdgeInsets>,
       child: Option<Box<dyn View>>,
   }
   
   // Usage
   Container::builder()
       .padding(EdgeInsets::all(10.0))
       .child(Text::new("Hello"))
       .build()
   ```

3. **Type safety**
   ```rust
   // Compile-time unit checking
   fn set_position(point: Point<Pixels>) { }  // Logical pixels
   fn draw_at(point: Point<DevicePixels>) { } // Physical pixels
   
   // Can't mix!
   // set_position(device_point); // ❌ Compile error
   ```

4. **Explicit lifecycle**
   ```rust
   impl View for MyWidget {
       fn create_element(&self) -> Box<dyn Element> {
           Box::new(StatelessElement::new(self))
       }
   }
   
   // Element lifecycle visible
   element.mount(parent, slot);
   element.update(new_widget);
   element.unmount();
   ```

---

## 🏗️ Архитектурные решения

### Что уже реализовано правильно:

#### ✅ **Foundation layer** (от Flutter)
- flui_types - базовые типы с Unit system
- flui-foundation - ID system, Keys, Notifications
- flui-tree - Arity system, Tree traits

#### ✅ **Platform abstraction** (от GPUI)
- Единый Platform trait
- WindowsPlatform (native Win32)
- WinitPlatform (cross-platform)
- HeadlessPlatform (testing)

#### ✅ **Bindings pattern** (от Flutter)
- WidgetsBinding (build)
- RenderingBinding (layout/paint)
- GestureBinding (events)
- SchedulerBinding (frames)

#### ✅ **Type safety** (от Rust/GPUI)
- Generic Unit system (Pixels vs DevicePixels)
- Arity system (Leaf, Single, Optional, Variable)
- Typestate pattern (Mounted/Unmounted)

### Что делаем дальше:

#### 🔄 **V2 Enhancements** (от GPUI)
- Associated types для Element state
- Three-phase lifecycle (request_layout → prepaint → paint)
- Pipeline phase tracking
- Source location tracking

#### 🆕 **V3 Reactive patterns** (от Xilem/Iced/Druid)
- Lens pattern (type-safe data access)
- Elm architecture (message-based updates)
- Adapt nodes (component composition)

#### 📦 **Widget library** (от Flutter)
- Material widgets
- Layout widgets
- Animation widgets
- Rich text widgets

---

## 📚 Как мы учимся

### Процесс изучения:

1. **Читаем source code**:
   ```
   .flutter/src/widgets/    ← Flutter widgets
   .flutter/src/rendering/  ← RenderObjects
   .gpui/src/               ← GPUI implementation
   ```

2. **Анализируем паттерны**:
   - Что работает хорошо?
   - Что можно улучшить?
   - Как адаптировать для Rust?

3. **Документируем решения**:
   - ADR (Architecture Decision Records)
   - Примеры кода
   - Обоснование выбора

4. **Итеративная реализация**:
   - Phase 1 → Phase 7 (base)
   - V2 enhancements (GPUI patterns)
   - V3 reactive (Xilem/Iced/Druid)

### Приоритеты при выборе:

1. **Простота** > Complexity
   - Flutter-style API понятнее GPUI
   
2. **Type safety** > Flexibility
   - Rust compile-time checking
   
3. **Явность** > Магия
   - Explicit lifecycle, no hidden behavior
   
4. **Проверенность** > Новизна
   - Flutter patterns работают 10+ лет
   
5. **Документация** > Code
   - Каждое решение объяснено

---

## 🎯 Целевая аудитория

### Кто будет использовать FLUI:

1. **Rust разработчики** без web background
   - Flutter-style API понятен без HTML/CSS
   - Нет необходимости знать flex/grid

2. **Flutter разработчики**, переходящие на Rust
   - Знакомые концепции (Widget, State, BuildContext)
   - Похожий API

3. **Desktop приложения**
   - Windows, macOS, Linux
   - Native performance (wgpu)

4. **Embedded UI** (будущее)
   - Игры
   - Инструменты разработчика
   - Kiosk applications

### Кто НЕ целевая аудитория (пока):

- Web разработчики (для них есть Dioxus, Leptos)
- Mobile-first (для этого сам Flutter лучше)
- Immediate mode UI (для этого egui)

---

## 🚀 Успех проекта = ?

### Критерии успеха:

1. **API проще GPUI**, понятен без web знаний
2. **Архитектура четче GPUI**, все задокументировано
3. **Ecosystem богаче GPUI**, больше виджетов
4. **Production-ready**, как Flutter
5. **Type-safe**, как Rust должен быть

### Как мы это достигаем:

- ✅ Изучаем лучшие практики (Flutter, GPUI, Xilem, Iced, Druid)
- ✅ Берем только то, что работает
- ✅ Адаптируем под Rust idioms
- ✅ Документируем каждое решение
- ✅ Пишем тесты для всего
- ✅ Создаем примеры

---

## 📖 Для контрибьюторов

### Если вы хотите помочь:

1. **Прочитайте**:
   - `PROJECT_PHILOSOPHY.md` (этот файл)
   - `ARCHITECTURE_OVERVIEW.md`
   - `docs/plans/ARCHITECTURE_DECISIONS.md`

2. **Изучите source code**:
   - `.flutter/` - как делает Flutter
   - `.gpui/` - паттерны из GPUI
   - `crates/` - текущая реализация

3. **Следуйте стилю**:
   - Flutter naming
   - Rust idioms
   - Type safety first
   - Explicit over implicit

4. **Документируйте решения**:
   - Почему выбрали этот подход?
   - Какие альтернативы рассмотрели?
   - Примеры использования

---

## 🎓 Философия в коде

### Пример: Container widget

**Flutter** (Dart):
```dart
Container(
  padding: EdgeInsets.all(10),
  margin: EdgeInsets.symmetric(horizontal: 20),
  decoration: BoxDecoration(
    color: Colors.blue,
    borderRadius: BorderRadius.circular(8),
  ),
  child: Text("Hello"),
)
```

**GPUI** (Rust):
```rust
div()
    .p_2()  // padding (CSS-like)
    .mx_4() // margin horizontal (Tailwind-like)
    .bg(blue())
    .rounded_lg()
    .child(div().child("Hello"))
```

**FLUI** (Rust) - наша цель:
```rust
Container::builder()
    .padding(EdgeInsets::all(px(10.0)))
    .margin(EdgeInsets::symmetric(horizontal: px(20.0)))
    .decoration(BoxDecoration::builder()
        .color(Color::BLUE)
        .border_radius(BorderRadius::circular(px(8.0)))
        .build())
    .child(Text::new("Hello"))
    .build()
```

**Почему FLUI лучше**:
- ✅ Понятно без web знаний (не нужно знать CSS)
- ✅ Explicit (явные типы и названия)
- ✅ Type-safe (px() для pixels)
- ✅ Builder pattern (bon crate)
- ✅ Похоже на Flutter (легко переключиться)

---

## 🌟 Итого

**FLUI** = **Flutter** (подход) + **Rust** (type safety) + **GPUI** (production patterns) - (сложность GPUI)

Мы создаем библиотеку, которая:
- ✅ **Проста** как Flutter
- ✅ **Безопасна** как Rust
- ✅ **Надежна** как production code
- ✅ **Понятна** всем (не только web devs)
- ✅ **Документирована** полностью

**Цель**: Стать **de-facto стандартом** для desktop UI на Rust.

---

**Документ живой** - обновляется по мере развития проекта.

**Последнее обновление**: 2026-01-24  
**Автор**: Vanya (владелец проекта) + Claude (анализ)
