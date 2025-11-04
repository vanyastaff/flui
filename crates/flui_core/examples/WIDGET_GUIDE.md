# Widget Creation Guide

Полное руководство по созданию виджетов в flui-core с использованием View API.

## Содержание

1. [Основы View](#основы-view)
2. [Простой виджет](#простой-виджет)
3. [Виджет с состоянием](#виджет-с-состоянием)
4. [Виджет с детьми](#виджет-с-детьми)
5. [Оптимизация rebuild](#оптимизация-rebuild)
6. [Продвинутые паттерны](#продвинутые-паттерны)

---

## Основы View

View - это trait, который определяет как создаются виджеты в flui.

```rust
pub trait View: 'static {
    type Element: ViewElement;
    type State: 'static;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State);
    fn rebuild(self, prev: &Self, state: &mut Self::State, element: &mut Self::Element) -> ChangeFlags;
    fn teardown(&self, state: &mut Self::State, element: &mut Self::Element);
}
```

### Ключевые концепции

- **Element**: Render element, который отображается на экран
- **State**: Постоянное состояние между перестройками
- **build()**: Вызывается при первом создании виджета
- **rebuild()**: Вызывается когда виджет обновляется новыми данными
- **teardown()**: Вызывается когда виджет удаляется

---

## Простой виджет

Начнём с самого простого виджета - текстовой метки.

```rust
use flui_core::{BuildContext, View, Element, ChangeFlags};

/// Простая текстовая метка
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
        }
    }
}

impl View for Label {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // В реальной реализации здесь создаётся TextRenderElement
        // let render = TextRenderElement::new(self.text);
        // (Element::Render(render), ())

        todo!("Create render element")
    }

    // Оптимизация: пересобираем только если текст изменился
    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        if self.text != prev.text {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE  // Ничего не изменилось!
        }
    }
}
```

### Использование

```rust
let label = Label::new("Hello, World!");
```

---

## Виджет с состоянием

Виджет, который использует hooks для управления состоянием.

```rust
use flui_core::hooks::{use_signal, Signal};

/// Счётчик с кнопками
#[derive(Debug, Clone)]
pub struct Counter {
    initial: i32,
}

impl Counter {
    pub fn new(initial: i32) -> Self {
        Self { initial }
    }
}

impl View for Counter {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Создаём signal для хранения значения
        let count = use_signal(ctx, self.initial);

        // Клонируем signal для использования в замыканиях
        let count_inc = count.clone();
        let count_dec = count.clone();

        // Создаём UI:
        // - Текст с текущим значением
        // - Кнопка +1
        // - Кнопка -1

        // Column::new()
        //     .child(Label::new(format!("Count: {}", count.get())))
        //     .child(Button::new("Increment", move |_| {
        //         count_inc.update(|n| n + 1);
        //     }))
        //     .child(Button::new("Decrement", move |_| {
        //         count_dec.update(|n| n - 1);
        //     }))

        todo!("Build counter UI")
    }
}
```

### Ключевые моменты

1. **Hooks всегда вызываются в одном порядке**
2. **Signal клонируется для замыканий** (это дёшево - только Rc increment)
3. **use_signal() хранит состояние между рендерами**

---

## Виджет с детьми

Контейнер, который может содержать другие виджеты.

```rust
/// Вертикальный контейнер
#[derive(Debug, Clone)]
pub struct VBox {
    children: Vec<Box<dyn View<Element = Element, State = ()>>>,
    spacing: f32,
}

impl VBox {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            spacing: 8.0,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn child(mut self, child: impl View<Element = Element, State = ()> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children(
        mut self,
        children: impl IntoIterator<Item = impl View<Element = Element, State = ()> + 'static>
    ) -> Self {
        self.children.extend(
            children.into_iter().map(|c| Box::new(c) as Box<dyn View<Element = Element, State = ()>>)
        );
        self
    }
}

impl View for VBox {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Строим всех детей
        // let child_elements: Vec<_> = self.children
        //     .into_iter()
        //     .map(|child| child.build(ctx).0)
        //     .collect();

        // Создаём Column render element
        // let column = ColumnRenderElement::new()
        //     .spacing(self.spacing)
        //     .children(child_elements);

        todo!("Build VBox")
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Пересобираем если изменился spacing или количество детей
        if self.spacing != prev.spacing || self.children.len() != prev.children.len() {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            // Дети сами обработают свои изменения
            ChangeFlags::NONE
        }
    }
}
```

### Использование

```rust
VBox::new()
    .spacing(10.0)
    .child(Label::new("Title"))
    .child(Counter::new(0))
    .child(Label::new("Footer"))
```

---

## Оптимизация rebuild

### Паттерн 1: Сравнение props

```rust
fn rebuild(
    self,
    prev: &Self,
    _state: &mut Self::State,
    element: &mut Self::Element,
) -> ChangeFlags {
    // Проверяем изменились ли свойства
    if self.prop1 != prev.prop1 || self.prop2 != prev.prop2 {
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    } else {
        ChangeFlags::NONE  // Ничего не изменилось - пропускаем rebuild
    }
}
```

### Паттерн 2: PartialEq для автоматического сравнения

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct MyWidget {
    prop1: String,
    prop2: i32,
}

impl View for MyWidget {
    // ...

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Используем PartialEq для сравнения всех полей
        if self != *prev {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}
```

### Когда НЕ оптимизировать rebuild

- Виджет очень простой (быстрее пересобрать чем сравнить)
- Сравнение props дорогое (большие коллекции)
- Виджет редко меняется

---

## Продвинутые паттерны

### 1. Computed Values (Memo)

```rust
use flui_core::hooks::use_memo;

impl View for ExpensiveWidget {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let input = use_signal(ctx, 10);

        // Дорогое вычисление - выполняется только когда input меняется
        let result = use_memo(ctx, |_hook_ctx| {
            let val = input.get();
            println!("Computing expensive result...");
            expensive_computation(val)
        });

        // UI использует result
        todo!()
    }
}

fn expensive_computation(n: i32) -> i32 {
    // Симуляция дорогих вычислений
    std::thread::sleep(std::time::Duration::from_millis(100));
    n * 2
}
```

### 2. Side Effects

```rust
use flui_core::hooks::use_effect;

impl View for Logger {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let count = use_signal(ctx, 0);

        // Логируем каждое изменение
        use_effect(ctx, move || {
            println!("Count changed to: {}", count.get());

            // Cleanup function (опционально)
            Some(Box::new(|| {
                println!("Cleaning up effect");
            }))
        });

        todo!()
    }
}
```

### 3. Условный рендеринг

```rust
impl View for ConditionalWidget {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let show_details = use_signal(ctx, false);

        // ✅ ПРАВИЛЬНО: Всегда вызываем все hooks
        let details = use_signal(ctx, String::from("Details..."));

        // Условие применяем к VALUE, не к hook calls
        let content = if show_details.get() {
            details.get()
        } else {
            String::from("Hidden")
        };

        // ❌ НЕПРАВИЛЬНО: Условный hook call
        // if show_details.get() {
        //     let details = use_signal(ctx, String::from("Details"));  // БАГ!
        // }

        todo!()
    }
}
```

### 4. Списки с ключами

```rust
use flui_core::Key;

impl View for TodoList {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let todos = use_signal(ctx, vec![
            Todo { id: 1, text: "Task 1".into() },
            Todo { id: 2, text: "Task 2".into() },
        ]);

        // Маппим todos на виджеты с ключами
        // Column::new()
        //     .children(todos.get().iter().map(|todo| {
        //         TodoItem::new(todo.text.clone())
        //             .key(Key::from_u64(todo.id))  // Ключ для эффективных обновлений
        //     }))

        todo!()
    }
}

#[derive(Clone)]
struct Todo {
    id: u64,
    text: String,
}
```

### 5. Форма с валидацией

```rust
impl View for LoginForm {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Поля формы
        let email = use_signal(ctx, String::new());
        let password = use_signal(ctx, String::new());

        // Computed валидация
        let is_valid = use_memo(ctx, |_hook_ctx| {
            let email_val = email.get();
            let pass_val = password.get();

            email_val.contains('@') && pass_val.len() >= 8
        });

        // Эффект: показываем подсказку когда форма валидна
        use_effect(ctx, move || {
            if is_valid.get() {
                println!("✓ Form is valid!");
            }
            None
        });

        // VBox::new()
        //     .child(TextField::new("Email", email))
        //     .child(TextField::new("Password", password))
        //     .child(Button::new("Login")
        //         .enabled(is_valid.get()))

        todo!()
    }
}
```

---

## Чек-лист для создания виджета

### Структура

- [ ] Определить struct с нужными props
- [ ] Добавить `#[derive(Debug, Clone)]`
- [ ] Добавить `PartialEq` если нужна оптимизация rebuild
- [ ] Создать конструктор `new()`

### View trait

- [ ] Определить `type Element`
- [ ] Определить `type State` (или `()` если не нужно)
- [ ] Реализовать `build()`
- [ ] Реализовать `rebuild()` с оптимизацией
- [ ] Реализовать `teardown()` если нужна очистка

### Hooks

- [ ] Все hooks вызываются в ОДИНАКОВОМ порядке
- [ ] Hooks НЕ вызываются условно
- [ ] Signals клонируются для замыканий
- [ ] use_memo для дорогих вычислений
- [ ] use_effect для side effects

### Производительность

- [ ] rebuild() сравнивает props
- [ ] Возвращает ChangeFlags::NONE если ничего не изменилось
- [ ] use_memo для избежания пересчёта
- [ ] Keys для списков

---

## Примеры из жизни

См. `widget_examples.rs` для полных примеров:

1. **SimpleText** - простой виджет без состояния
2. **Counter** - stateful виджет с hooks
3. **ComputedDisplay** - использование use_memo
4. **LoggingWidget** - side effects с use_effect
5. **Container** - виджет с детьми
6. **ConditionalWidget** - условный рендеринг
7. **FormWidget** - форма с валидацией
8. **ListWidget** - динамические списки

---

## Частые ошибки

### ❌ Условный hook

```rust
// НЕПРАВИЛЬНО
if condition {
    let signal = use_signal(ctx, 0);  // Порядок hooks меняется!
}
```

### ✅ Правильно

```rust
// ПРАВИЛЬНО
let signal = use_signal(ctx, 0);
if condition {
    signal.set(10);  // Условие применяем к значению
}
```

### ❌ Забыли клонировать signal

```rust
// НЕПРАВИЛЬНО - signal moved
Button::new("Click", move |_| {
    count.update(|n| n + 1);  // count moved here
});
// count больше нельзя использовать!
```

### ✅ Правильно

```rust
// ПРАВИЛЬНО
let count_clone = count.clone();
Button::new("Click", move |_| {
    count_clone.update(|n| n + 1);
});
// count всё ещё доступен
```

---

## Дополнительные ресурсы

- `hooks/RULES.md` - Правила использования hooks
- `view/view.rs` - Исходный код View trait
- `element/lifecycle.rs` - Lifecycle диаграммы

Happy coding! 🚀
