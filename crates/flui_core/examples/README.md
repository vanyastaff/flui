# flui-core Examples

Примеры и руководства по созданию виджетов с использованием View API.

## 📚 Содержание

### Руководства

- **[WIDGET_GUIDE.md](WIDGET_GUIDE.md)** - Полное пошаговое руководство по созданию виджетов
  - Основы View API
  - Простые и сложные виджеты
  - Hooks (use_signal, use_memo, use_effect)
  - Оптимизация производительности
  - Типичные ошибки и их решения

### Примеры кода

- **[widget_examples.rs](widget_examples.rs)** - Коллекция примеров виджетов
  - SimpleText - простой stateless виджет
  - Counter - stateful виджет с hooks
  - ComputedDisplay - вычисляемые значения
  - LoggingWidget - side effects
  - Container - виджет с детьми
  - ConditionalWidget - условный рендеринг
  - FormWidget - формы с валидацией
  - ListWidget - динамические списки

## 🚀 Быстрый старт

### 1. Простейший виджет

```rust
use flui_core::{BuildContext, View, Element, ChangeFlags};

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    text: String,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl View for Label {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Создаём render element
        todo!("Create text render element")
    }

    fn rebuild(self, prev: &Self, _state: &mut Self::State, element: &mut Self::Element) -> ChangeFlags {
        if self.text != prev.text {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}
```

### 2. Виджет с состоянием

```rust
use flui_core::hooks::use_signal;

#[derive(Debug, Clone)]
pub struct Counter {
    initial: i32,
}

impl View for Counter {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let count = use_signal(ctx, self.initial);

        // Клонируем для closure
        let count_clone = count.clone();

        // Создаём UI с кнопкой
        // Button::new("Increment", move |_| {
        //     count_clone.update(|n| n + 1);
        // })

        todo!()
    }
}
```

## 📖 Изучение по порядку

1. **Начните с [WIDGET_GUIDE.md](WIDGET_GUIDE.md)**
   - Прочитайте разделы по порядку
   - Начните с "Простой виджет"
   - Затем перейдите к "Виджет с состоянием"

2. **Изучите примеры в [widget_examples.rs](widget_examples.rs)**
   - Посмотрите на структуру каждого примера
   - Обратите внимание на комментарии
   - Попробуйте модифицировать примеры

3. **Прочитайте документацию по hooks**
   - `../src/hooks/RULES.md` - Правила использования hooks
   - `../src/hooks/signal.rs` - Документация Signal
   - `../src/hooks/memo.rs` - Документация Memo

## 🎯 Ключевые концепции

### View Trait

```rust
pub trait View: 'static {
    type Element: ViewElement;
    type State: 'static;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State);
    fn rebuild(self, prev: &Self, state: &mut Self::State, element: &mut Self::Element) -> ChangeFlags;
    fn teardown(&self, state: &mut Self::State, element: &mut Self::Element) {}
}
```

### Hooks

- **use_signal** - Реактивное состояние
- **use_memo** - Вычисляемые значения (кэшируются)
- **use_effect** - Побочные эффекты (логирование, API calls)

### Правила Hooks

1. ✅ Всегда вызывать в одинаковом порядке
2. ❌ Никогда не вызывать условно
3. ✅ Клонировать signals для closures
4. ✅ Использовать memo для дорогих вычислений

## 💡 Паттерны

### Stateless Widget

```rust
#[derive(Debug, Clone, PartialEq)]
struct MyWidget { props: Props }

impl View for MyWidget {
    // Нет hooks, только props
}
```

### Stateful Widget

```rust
impl View for MyWidget {
    fn build(self, ctx: &mut BuildContext) -> _ {
        let state = use_signal(ctx, initial);
        // ...
    }
}
```

### Container Widget

```rust
struct Container {
    children: Vec<Box<dyn View<...>>>,
}

impl Container {
    pub fn child(mut self, child: impl View + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }
}
```

### Computed Values

```rust
let input = use_signal(ctx, 10);
let doubled = use_memo(ctx, |_| input.get() * 2);
```

### Side Effects

```rust
use_effect(ctx, move || {
    println!("State changed!");
    None // or Some(Box::new(|| cleanup))
});
```

## ⚠️ Типичные ошибки

### ❌ Условный hook

```rust
if condition {
    use_signal(ctx, 0);  // ОШИБКА!
}
```

### ✅ Правильно

```rust
let signal = use_signal(ctx, 0);
if condition {
    signal.set(10);
}
```

### ❌ Забыли оптимизировать rebuild

```rust
fn rebuild(...) -> ChangeFlags {
    element.mark_dirty();
    ChangeFlags::NEEDS_BUILD  // Всегда пересобирает!
}
```

### ✅ Правильно

```rust
fn rebuild(self, prev: &Self, ...) -> ChangeFlags {
    if self != *prev {
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    } else {
        ChangeFlags::NONE  // Пропускает пересборку
    }
}
```

## 🔗 Связанная документация

### В этом крейте

- `src/hooks/RULES.md` - Правила использования hooks
- `src/view/view.rs` - View trait исходный код
- `src/element/lifecycle.rs` - Lifecycle диаграммы

### Внешние ресурсы

- [React Hooks](https://react.dev/reference/react) - Похожая концепция hooks
- [Flutter Widgets](https://flutter.dev/docs/development/ui/widgets) - Похожая архитектура

## 🎓 Упражнения

Попробуйте создать:

1. **TodoItem** - виджет для элемента todo-списка
   - Checkbox для completed
   - Text для описания
   - Button для удаления

2. **ToggleButton** - кнопка с двумя состояниями
   - use_signal для состояния on/off
   - Разные стили для on/off

3. **ProgressBar** - индикатор прогресса
   - Props: value (0.0-1.0)
   - Анимация прогресса

4. **SearchBox** - поле поиска с фильтрацией
   - TextField для ввода
   - use_signal для query
   - use_memo для filtered results

## 📬 Вопросы?

Если что-то непонятно:

1. Проверьте [WIDGET_GUIDE.md](WIDGET_GUIDE.md)
2. Изучите примеры в [widget_examples.rs](widget_examples.rs)
3. Прочитайте `src/hooks/RULES.md`
4. Посмотрите исходный код существующих виджетов

Happy coding! 🚀
