# Flui Widget Design: Xilem-inspired без Action

## 🎯 Цель

Создать Widget trait, который:
- ✅ **Компилируется** (нет coherence конфликтов)
- ✅ **Прост** для пользователей (Flutter-like API)
- ✅ **Без Action** (на первом этапе)
- ✅ **Производительный** (incremental updates)
- ✅ **Гибкий** (можно добавить Action позже)

---

## 📐 Архитектура

### Ключевая идея из Xilem:

**Два дерева:**
1. **View Tree** (короткоживущее) - пользовательский код
2. **Element Tree** (долгоживущее) - retained widgets

```
User Code:              Framework:
┌─────────────┐        ┌──────────────┐
│  View Tree  │───────▶│ Element Tree │
│ (temporary) │ diff   │  (retained)  │
└─────────────┘        └──────────────┘
     Text                  TextElement
     Column                ColumnElement
     Button                ButtonElement
```

---

## 🏗️ Core Design

### 1. Widget Trait (Xilem-inspired)

```rust
// ========== Marker Trait (для coherence) ==========

/// Marker trait для всех типов, которые могут быть Widget
pub trait WidgetMarker {}

// ========== Core Widget Trait ==========

/// Основной trait для всех widgets
///
/// View Tree vs Element Tree:
/// - View Tree: короткоживущие, создаются при каждом rebuild
/// - Element Tree: долгоживущие, обновляются инкрементально
pub trait Widget: WidgetMarker + Debug + 'static {
    /// Тип element в retained tree
    type Element: WidgetElement;

    /// Внутреннее состояние widget'а (не публичное API)
    ///
    /// Это может включать:
    /// - Состояние дочерних widgets
    /// - Кэш для оптимизации
    /// - Runtime данные
    type WidgetState;

    /// Создать начальный element и state
    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState);

    /// Обновить element на основе diff с prev
    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        element: ElementMut<'_, Self::Element>,
    );

    /// Очистка при удалении widget из дерева
    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        element: ElementMut<'_, Self::Element>,
    ) {
        // Default: ничего не делаем
        let _ = (widget_state, ctx, element);
    }
}
```

---

## 🎨 Concrete Implementations

### Пример 1: Text (Stateless)

```rust
/// Простой текстовый widget
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    data: String,
    style: TextStyle,
}

impl Text {
    pub fn new(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            style: TextStyle::default(),
        }
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }
}

// ========== Widget Implementation ==========

impl WidgetMarker for Text {}

impl Widget for Text {
    type Element = TextElement;
    type WidgetState = (); // ← Нет состояния!

    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState) {
        let element = TextElement::new(&self.data, self.style.clone());
        ctx.register_element(&element);
        (element, ())
    }

    fn rebuild(
        &self,
        prev: &Self,
        _widget_state: &mut Self::WidgetState,
        _ctx: &mut BuildContext,
        mut element: ElementMut<'_, Self::Element>,
    ) {
        // Инкрементальные обновления
        if prev.data != self.data {
            element.set_text(&self.data);
        }
        if prev.style != self.style {
            element.set_style(self.style.clone());
        }
    }

    fn teardown(
        &self,
        _widget_state: &mut Self::WidgetState,
        _ctx: &mut BuildContext,
        _element: ElementMut<'_, Self::Element>,
    ) {
        // Text не требует очистки
    }
}
```

---

### Пример 2: Button (с callback)

```rust
/// Button widget с callback
#[derive(Debug)]
pub struct Button<F> {
    label: String,
    on_pressed: F,
    enabled: bool,
}

impl<F> Button<F>
where
    F: Fn() + 'static,
{
    pub fn new(label: impl Into<String>, on_pressed: F) -> Self {
        Self {
            label: label.into(),
            on_pressed,
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

// ========== Widget Implementation ==========

impl<F> WidgetMarker for Button<F> {}

impl<F> Widget for Button<F>
where
    F: Fn() + 'static,
{
    type Element = ButtonElement;
    type WidgetState = EventHandlerId;

    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState) {
        let element = ButtonElement::new(&self.label);

        // Регистрируем event handler
        let handler_id = ctx.register_event_handler(
            element.id(),
            EventType::PointerDown,
            Box::new(|| {
                (self.on_pressed)();
            }),
        );

        (element, handler_id)
    }

    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        mut element: ElementMut<'_, Self::Element>,
    ) {
        // Обновляем label
        if prev.label != self.label {
            element.set_label(&self.label);
        }

        // Обновляем enabled state
        if prev.enabled != self.enabled {
            element.set_enabled(self.enabled);
        }

        // Обновляем callback (если изменился)
        // Note: сравнить Fn напрямую нельзя, поэтому всегда обновляем
        ctx.update_event_handler(
            *widget_state,
            Box::new(|| {
                (self.on_pressed)();
            }),
        );
    }

    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        _element: ElementMut<'_, Self::Element>,
    ) {
        // Удаляем event handler
        ctx.remove_event_handler(*widget_state);
    }
}
```

---

### Пример 3: Column (Container)

```rust
/// Вертикальный layout container
#[derive(Debug)]
pub struct Column<Children> {
    children: Children,
    spacing: f64,
}

impl<Children> Column<Children> {
    pub fn new(children: Children) -> Self {
        Self {
            children,
            spacing: 0.0,
        }
    }

    pub fn spacing(mut self, spacing: f64) -> Self {
        self.spacing = spacing;
        self
    }
}

// ========== Widget Implementation ==========

impl<Children> WidgetMarker for Column<Children> {}

// Для одиночного child
impl<Child> Widget for Column<Child>
where
    Child: Widget,
{
    type Element = FlexElement;
    type WidgetState = Child::WidgetState;

    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState) {
        // Строим дочерний widget
        let (child_element, child_state) = self.children.build(ctx);

        // Создаём flex container
        let mut element = FlexElement::new(Axis::Vertical);
        element.set_spacing(self.spacing);
        element.add_child(Box::new(child_element));

        (element, child_state)
    }

    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        mut element: ElementMut<'_, Self::Element>,
    ) {
        // Обновляем spacing
        if (prev.spacing - self.spacing).abs() > f64::EPSILON {
            element.set_spacing(self.spacing);
        }

        // Rebuild дочернего widget
        self.children.rebuild(
            &prev.children,
            widget_state,
            ctx,
            element.child_mut(0),
        );
    }

    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        mut element: ElementMut<'_, Self::Element>,
    ) {
        self.children.teardown(widget_state, ctx, element.child_mut(0));
    }
}

// Для tuple детей (2 элемента)
impl<C1, C2> Widget for Column<(C1, C2)>
where
    C1: Widget,
    C2: Widget,
{
    type Element = FlexElement;
    type WidgetState = (C1::WidgetState, C2::WidgetState);

    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState) {
        let (child1_element, child1_state) = self.children.0.build(ctx);
        let (child2_element, child2_state) = self.children.1.build(ctx);

        let mut element = FlexElement::new(Axis::Vertical);
        element.set_spacing(self.spacing);
        element.add_child(Box::new(child1_element));
        element.add_child(Box::new(child2_element));

        (element, (child1_state, child2_state))
    }

    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        mut element: ElementMut<'_, Self::Element>,
    ) {
        if (prev.spacing - self.spacing).abs() > f64::EPSILON {
            element.set_spacing(self.spacing);
        }

        // Rebuild первого child
        self.children.0.rebuild(
            &prev.children.0,
            &mut widget_state.0,
            ctx,
            element.child_mut(0),
        );

        // Rebuild второго child
        self.children.1.rebuild(
            &prev.children.1,
            &mut widget_state.1,
            ctx,
            element.child_mut(1),
        );
    }

    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        mut element: ElementMut<'_, Self::Element>,
    ) {
        self.children.0.teardown(&mut widget_state.0, ctx, element.child_mut(0));
        self.children.1.teardown(&mut widget_state.1, ctx, element.child_mut(1));
    }
}

// TODO: Реализовать для tuple с 3, 4, ... элементами через macro
```

---

### Пример 4: StatefulWidget (с внутренним state)

```rust
/// Counter widget с внутренним состоянием
#[derive(Debug, Clone)]
pub struct Counter {
    initial_count: i32,
}

impl Counter {
    pub fn new(initial_count: i32) -> Self {
        Self { initial_count }
    }

    /// Вспомогательная функция для построения дочернего view
    fn build_child(&self, count: i32) -> Column<(Text, Button<impl Fn()>)> {
        // ПРОБЛЕМА: Как передать изменение count обратно?
        // Решение: использовать Rc<RefCell<_>> или другой паттерн
        Column::new((
            Text::new(format!("Count: {}", count)),
            Button::new("Increment", move || {
                // Как здесь обновить count?
                // Нужен доступ к WidgetState...
            }),
        ))
    }
}

// ========== Widget Implementation ==========

impl WidgetMarker for Counter {}

// ❌ Проблема: нужен способ обновить WidgetState из callback!
// Решение 1: Использовать Rc<RefCell<>>
// Решение 2: Использовать State handle
// Решение 3: Использовать Action (как в Xilem)

// Давайте используем Rc<RefCell<>> для примера:

use std::rc::Rc;
use std::cell::RefCell;

#[derive(Debug)]
pub struct CounterState {
    count: Rc<RefCell<i32>>,
    child_state: <Column<(Text, Button<impl Fn()>)> as Widget>::WidgetState,
}

// ❌ Это не скомпилируется из-за impl Fn() в типе...
// Нужно использовать type erasure или другой подход
```

**ПРОБЛЕМА:** Без Action сложно обновлять state из callbacks!

---

## 🔧 Решение: State Handle

Введём `StateHandle` для доступа к состоянию из callbacks:

```rust
/// Handle для обновления state widget'а
pub struct StateHandle<T> {
    inner: Rc<RefCell<T>>,
}

impl<T> StateHandle<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
        }
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.inner.borrow().clone()
    }

    pub fn set(&self, value: T) {
        *self.inner.borrow_mut() = value;
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut *self.inner.borrow_mut());
    }
}

impl<T> Clone for StateHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
```

Теперь используем StateHandle в Counter:

```rust
#[derive(Debug, Clone)]
pub struct Counter {
    initial_count: i32,
}

impl Counter {
    pub fn new(initial_count: i32) -> Self {
        Self { initial_count }
    }
}

// ========== Widget Implementation ==========

impl WidgetMarker for Counter {}

#[derive(Debug)]
pub struct CounterWidgetState {
    count: StateHandle<i32>,
    child_state: ((), EventHandlerId), // State для (Text, Button)
}

impl Widget for Counter {
    type Element = FlexElement;
    type WidgetState = CounterWidgetState;

    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState) {
        let count_handle = StateHandle::new(self.initial_count);
        let count_value = count_handle.get();

        // Строим child widgets
        let text = Text::new(format!("Count: {}", count_value));
        let button = {
            let count_handle = count_handle.clone();
            Button::new("Increment", move || {
                count_handle.update(|count| *count += 1);
                // TODO: Нужно запросить rebuild!
                // ctx.request_rebuild();
            })
        };

        let column = Column::new((text, button));
        let (element, child_state) = column.build(ctx);

        let widget_state = CounterWidgetState {
            count: count_handle,
            child_state,
        };

        (element, widget_state)
    }

    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        element: ElementMut<'_, Self::Element>,
    ) {
        let current_count = widget_state.count.get();

        // Если count изменился, rebuild детей
        if current_count != prev.initial_count {
            let text = Text::new(format!("Count: {}", current_count));
            let button = {
                let count_handle = widget_state.count.clone();
                Button::new("Increment", move || {
                    count_handle.update(|count| *count += 1);
                })
            };

            let column = Column::new((text, button));
            let prev_column = Column::new((
                Text::new(format!("Count: {}", prev.initial_count)),
                Button::new("Increment", || {}),
            ));

            column.rebuild(&prev_column, &mut widget_state.child_state, ctx, element);
        }
    }

    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        element: ElementMut<'_, Self::Element>,
    ) {
        let count = widget_state.count.get();
        let column = Column::new((
            Text::new(format!("Count: {}", count)),
            Button::new("Increment", || {}),
        ));
        column.teardown(&mut widget_state.child_state, ctx, element);
    }
}
```

---

## 🚨 Проблемы этого подхода

### Проблема 1: Request Rebuild

```rust
Button::new("Increment", move || {
    count_handle.update(|count| *count += 1);
    // ❌ Как запросить rebuild?
    // Нужен доступ к framework runtime
})
```

**Решение:** BuildContext должен быть доступен в callbacks

```rust
pub struct BuildContext {
    // ...
    rebuild_notifier: Rc<dyn Fn()>,
}

impl BuildContext {
    pub fn request_rebuild(&self) {
        (self.rebuild_notifier)();
    }
}

// Usage:
Button::new_with_ctx("Increment", |ctx| {
    move || {
        count_handle.update(|count| *count += 1);
        ctx.request_rebuild(); // ← Запрашиваем rebuild
    }
})
```

### Проблема 2: Сложность rebuild

Каждый раз при rebuild нужно:
1. Пересоздавать child widgets
2. Сравнивать с предыдущими
3. Управлять WidgetState детей

**Это сложно и подвержено ошибкам!**

---

## 💭 Вывод: Action действительно нужен?

### Без Action (текущий подход):

```rust
// ❌ Сложно
// ❌ Нужен StateHandle
// ❌ Нужен request_rebuild
// ❌ Сложный rebuild logic
// ❌ Много boilerplate

let count_handle = StateHandle::new(0);
Button::new("Increment", {
    let count = count_handle.clone();
    let ctx = ctx.clone();
    move || {
        count.update(|c| *c += 1);
        ctx.request_rebuild();
    }
})
```

### С Action (Xilem подход):

```rust
// ✅ Проще
// ✅ Явные сообщения
// ✅ Автоматический rebuild
// ✅ Type safe
// ✅ Меньше boilerplate

enum CounterAction {
    Increment,
}

Button::new("Increment", |_| CounterAction::Increment)

// Родитель обрабатывает:
map_action(counter(), |state, action| {
    match action {
        CounterAction::Increment => state.count += 1,
    }
})
```

---

## 🎯 Рекомендация

**Для Flui нужен гибридный подход:**

### Вариант 1: Для простых случаев (без внутреннего state)

```rust
// Прямой доступ к app state
Button::new("Click", |state: &mut AppState| {
    state.count += 1;
})
```

### Вариант 2: Для модульных компонентов (с Action)

```rust
// Action для переиспользуемых компонентов
enum CounterAction { Increment, Decrement }

fn counter(count: i32) -> impl Widget<(), CounterAction> {
    Button::new("+", |_| CounterAction::Increment)
}
```

### Вариант 3: Для сложных stateful widgets

```rust
// Builder pattern с внутренним state
StatefulWidget::builder()
    .initial_state(|| CounterState { count: 0 })
    .build(|state| {
        Column::new((
            Text::new(format!("{}", state.count)),
            Button::new("+", move |state| state.count += 1),
        ))
    })
```

---

## 📝 Итоговый дизайн Widget trait

```rust
pub trait WidgetMarker {}

pub trait Widget: WidgetMarker + Debug + 'static {
    type Element: WidgetElement;
    type WidgetState;

    fn build(&self, ctx: &mut BuildContext) -> (Self::Element, Self::WidgetState);

    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        element: ElementMut<'_, Self::Element>,
    );

    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext,
        element: ElementMut<'_, Self::Element>,
    ) {
        let _ = (widget_state, ctx, element);
    }
}
```

**Особенности:**
- ✅ Нет blanket impl конфликтов
- ✅ Инкрементальные обновления через rebuild
- ✅ Можно добавить Action позже
- ✅ Гибкий WidgetState
- ❌ Требует реализации для каждого widget
- ❌ Stateful widgets сложнее без Action

**Следующий шаг:** Прототип с примерами использования!
