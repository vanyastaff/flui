# Xilem Architecture Deep Dive & Flui Redesign

## 🔍 Ключевые открытия из Xilem

### 1. Архитектура View Trait

```rust
pub trait View<State, Action, Context: ViewPathTracker>: ViewMarker + 'static {
    type Element: ViewElement;
    type ViewState;

    fn build(&self, ctx: &mut Context, app_state: &mut State)
        -> (Self::Element, Self::ViewState);

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState,
               ctx: &mut Context, element: Mut<'_, Self::Element>,
               app_state: &mut State);

    fn teardown(&self, view_state: &mut Self::ViewState,
                ctx: &mut Context, element: Mut<'_, Self::Element>);

    fn message(&self, view_state: &mut Self::ViewState,
               message: &mut MessageContext,
               element: Mut<'_, Self::Element>,
               app_state: &mut State) -> MessageResult<Action>;
}
```

**Ключевые особенности:**

1. **НЕТ иерархии подтрейтов** - Нет `StatelessWidget`, `StatefulWidget`, и т.д.
2. **Unified trait** - Один трейт для всех видов виджетов
3. **ViewState** - Внутреннее состояние view (не публичное API)
4. **Element** - "Retained tree" - постоянное дерево виджетов
5. **Generic параметры** - `State`, `Action`, `Context` делают trait гибким

### 2. ViewSequence - Как Xilem избегает конфликтов

```rust
pub trait ViewSequence<State, Action, Context, Element>: 'static {
    type SeqState;
    const ELEMENTS_COUNT: Count;

    fn seq_build(&self, ctx: &mut Context,
                 elements: &mut AppendVec<Element>,
                 app_state: &mut State) -> Self::SeqState;
    // ...
}

// ✅ Единственный blanket impl!
impl<State, Action, Context, V, Element>
    ViewSequence<State, Action, Context, Element> for V
where
    V: View<State, Action, Context> + ViewMarker,
    Element: SuperElement<V::Element, Context>,
{
    // Реализация для одиночного View
}
```

**Почему это работает:**

- ViewSequence - это ОТДЕЛЬНЫЙ трейт (не супертрейт View)
- View → ViewSequence - только ОДНО направление
- Нет множественных blanket impl с пересекающимися bounds
- ViewSequence также реализован для Vec, Option, кортежей - но напрямую, не через blanket impl

### 3. Конкретные реализации (Button)

```rust
pub struct Button<F, V> {
    child: V,
    callback: F,
    disabled: bool,
}

impl<F, V> ViewMarker for Button<F, V> {}

impl<F, V, State, Action> View<State, Action, ViewCtx> for Button<F, V>
where
    V: WidgetView<State, Action>,
    F: Fn(&mut State, Option<PointerButton>) -> MessageResult<Action> + ...
{
    type Element = Pod<widgets::Button>;
    type ViewState = V::ViewState; // ← Состояние дочернего view!

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State)
        -> (Self::Element, Self::ViewState)
    {
        // 1. Строим дочерний view
        let (child, child_state) = ctx.with_id(BUTTON_CONTENT_VIEW_ID, |ctx| {
            View::build(&self.child, ctx, app_state)
        });

        // 2. Создаем Masonry widget
        let pod = ctx.create_pod(widgets::Button::new(child.new_widget));

        (pod, child_state)
    }

    fn rebuild(&self, prev: &Self, state: &mut Self::ViewState, ...) {
        // Сравнение с prev для incremental updates
        if prev.disabled != self.disabled {
            element.ctx.set_disabled(self.disabled);
        }
        // Rebuild дочернего view
        View::rebuild(&self.child, &prev.child, state, ctx, ...);
    }
}
```

**Паттерны:**

1. **View = struct с данными** - не trait object
2. **Прямая реализация View** - каждый конкретный тип
3. **ViewState хранит состояние детей** - композиция через associated type
4. **build возвращает Element** - retained widget tree

---

## 🆚 Сравнение: Flutter vs Xilem vs Flui

### Flutter (Dart)

```dart
abstract class Widget {
  Element createElement();
}

abstract class StatelessWidget extends Widget {
  Widget build(BuildContext context);
}

abstract class StatefulWidget extends Widget {
  State createState();
}

class MyWidget extends StatelessWidget {
  @override
  Widget build(BuildContext context) => Text("Hello");
}
```

**Особенности:**
- 3 уровня иерархии: Widget → Stateless/Stateful → Concrete
- createElement() - фабричный метод
- Runtime polymorphism (динамический dispatch)
- Простой API для пользователей

### Xilem (Rust)

```rust
// Нет иерархии!
pub struct Button<F, V> { ... }

impl<F, V, State, Action> View<State, Action, ViewCtx> for Button<F, V> {
    type Element = Pod<widgets::Button>;
    type ViewState = V::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State)
        -> (Self::Element, Self::ViewState) { ... }
}
```

**Особенности:**
- Flat hierarchy - нет подтрейтов
- Каждый view - конкретная структура
- Compile-time полиморфизм (generic parameters)
- ViewState для внутреннего состояния
- View tree - короткоживущий, Element tree - долгоживущий

### Flui (текущая)

```rust
pub trait Widget: Debug + 'static {
    type Element;
    // ...
}

pub trait StatelessWidget { ... }
pub trait StatefulWidget { ... }

// ❌ Пытаемся сделать blanket impl (не компилируется!)
impl<W: StatelessWidget> Widget for W { ... }
impl<W: StatefulWidget> Widget for W { ... }
```

**Проблемы:**
- Пытается скопировать Flutter иерархию в Rust
- Blanket impl конфликтуют из-за coherence rules
- Rust не может доказать, что StatelessWidget и StatefulWidget взаимоисключающие

---

## 🎯 Предложение для Flui: Xilem-inspired Design

### Вариант A: Полная переработка (Xilem Style)

**Архитектура:**

```rust
// ========== Core Traits ==========

pub trait ViewMarker {}

pub trait Widget<State = (), Action = ()>: ViewMarker + 'static {
    /// Тип Element в retained tree
    type Element: WidgetElement;

    /// Внутреннее состояние widget'а (не публичное API!)
    type WidgetState;

    /// Создать начальный element и state
    fn build(&self, ctx: &mut BuildContext<'_>, state: &mut State)
        -> (Self::Element, Self::WidgetState);

    /// Обновить element на основе diff с prev
    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext<'_>,
        element: ElementMut<'_, Self::Element>,
        state: &mut State,
    );

    /// Очистка при удалении
    fn teardown(
        &self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext<'_>,
        element: ElementMut<'_, Self::Element>,
    );

    /// Обработка событий
    fn on_event(
        &self,
        widget_state: &mut Self::WidgetState,
        event: &Event,
        element: ElementMut<'_, Self::Element>,
        state: &mut State,
    ) -> EventResult<Action>;
}

// ========== Concrete Widgets ==========

// "Stateless" widget - просто struct с данными
#[derive(Debug, Clone)]
pub struct Text {
    data: String,
    style: TextStyle,
}

impl ViewMarker for Text {}

impl<State, Action> Widget<State, Action> for Text {
    type Element = TextElement;
    type WidgetState = (); // ← Нет состояния!

    fn build(&self, ctx: &mut BuildContext<'_>, _state: &mut State)
        -> (Self::Element, Self::WidgetState)
    {
        let element = TextElement::new(&self.data, self.style.clone());
        (element, ())
    }

    fn rebuild(
        &self,
        prev: &Self,
        _widget_state: &mut Self::WidgetState,
        _ctx: &mut BuildContext<'_>,
        mut element: ElementMut<'_, Self::Element>,
        _state: &mut State,
    ) {
        if prev.data != self.data {
            element.set_text(&self.data);
        }
        if prev.style != self.style {
            element.set_style(self.style.clone());
        }
    }

    fn teardown(&self, _: &mut (), _: &mut BuildContext<'_>, _: ElementMut<'_, Self::Element>) {}
    fn on_event(&self, _: &mut (), _: &Event, _: ElementMut<'_, Self::Element>, _: &mut State)
        -> EventResult<Action>
    {
        EventResult::Ignored
    }
}

// "Stateful" widget - ViewState хранит состояние
pub struct Counter {
    initial_count: i32,
}

impl ViewMarker for Counter {}

pub struct CounterState {
    count: i32,
    child_view_state: <Column<(Text, Button)> as Widget>::WidgetState,
}

impl<Action> Widget<(), Action> for Counter {
    type Element = ComponentElement;
    type WidgetState = CounterState;

    fn build(&self, ctx: &mut BuildContext<'_>, _state: &mut ())
        -> (Self::Element, Self::WidgetState)
    {
        let count = self.initial_count;

        let child_view = Column::new((
            Text::new(format!("Count: {}", count)),
            Button::new("Increment", |_state| {}),
        ));

        let (child_element, child_state) = child_view.build(ctx, _state);

        let element = ComponentElement::new(Box::new(child_element));
        let widget_state = CounterState {
            count,
            child_view_state: child_state,
        };

        (element, widget_state)
    }

    fn rebuild(
        &self,
        prev: &Self,
        widget_state: &mut Self::WidgetState,
        ctx: &mut BuildContext<'_>,
        mut element: ElementMut<'_, Self::Element>,
        state: &mut (),
    ) {
        // Rebuild child if count changed
        if widget_state.count != prev.initial_count {
            let child_view = self.build_child(widget_state.count);
            let prev_child_view = prev.build_child(widget_state.count);

            child_view.rebuild(
                &prev_child_view,
                &mut widget_state.child_view_state,
                ctx,
                element.child_mut(),
                state,
            );
        }
    }

    fn teardown(&self, widget_state: &mut Self::WidgetState, ctx: &mut BuildContext<'_>,
                mut element: ElementMut<'_, Self::Element>)
    {
        let child_view = self.build_child(widget_state.count);
        child_view.teardown(&mut widget_state.child_view_state, ctx, element.child_mut());
    }

    fn on_event(&self, _: &mut Self::WidgetState, _: &Event, _: ElementMut<'_, Self::Element>,
                _: &mut ()) -> EventResult<Action>
    {
        EventResult::Ignored
    }
}

impl Counter {
    fn build_child(&self, count: i32) -> Column<(Text, Button<()>)> {
        Column::new((
            Text::new(format!("Count: {}", count)),
            Button::new("Increment", move |_| {}),
        ))
    }
}

// Button с callback
pub struct Button<F> {
    label: String,
    on_press: F,
}

impl<F> ViewMarker for Button<F> {}

impl<State, Action, F> Widget<State, Action> for Button<F>
where
    F: Fn(&mut State) -> Action + 'static,
{
    type Element = ButtonElement;
    type WidgetState = ();

    fn build(&self, ctx: &mut BuildContext<'_>, _state: &mut State)
        -> (Self::Element, Self::WidgetState)
    {
        let element = ButtonElement::new(&self.label);
        ctx.register_event_handler(element.id(), /* ... */);
        (element, ())
    }

    fn rebuild(&self, prev: &Self, _: &mut (), _: &mut BuildContext<'_>,
               mut element: ElementMut<'_, Self::Element>, _: &mut State)
    {
        if prev.label != self.label {
            element.set_label(&self.label);
        }
    }

    fn teardown(&self, _: &mut (), _: &mut BuildContext<'_>, _: ElementMut<'_, Self::Element>) {}

    fn on_event(&self, _: &mut (), event: &Event, _: ElementMut<'_, Self::Element>,
                state: &mut State) -> EventResult<Action>
    {
        if let Event::PointerDown { .. } = event {
            EventResult::Action((self.on_press)(state))
        } else {
            EventResult::Ignored
        }
    }
}
```

**Преимущества:**

✅ **Компилируется** - нет blanket impl конфликтов
✅ **Гибкость** - generic параметры State и Action
✅ **Производительность** - compile-time полиморфизм
✅ **Проверенная архитектура** - успешно используется в Xilem
✅ **Incremental updates** - diff-based rebuild

**Недостатки:**

❌ **API отличается от Flutter** - более сложный для новичков
❌ **Больше boilerplate** - каждый widget требует полной impl
❌ **ViewState management** - нужно вручную управлять состоянием детей
❌ **Большие изменения** - придется переписать существующий код

---

### Вариант B: Гибридный подход (Flui Flavored)

Сохраняем Flutter-like API, но используем derive macros:

```rust
// ========== Core Traits (упрощенные) ==========

pub trait Widget: Debug + 'static {
    type Element: WidgetElement;

    fn create_element(&self) -> Self::Element;
}

pub trait StatelessWidget {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget>;
}

pub trait StatefulWidget {
    type State;

    fn create_state(&self) -> Self::State;
    fn build(&self, ctx: &BuildContext, state: &Self::State) -> Box<dyn Widget>;
}

// ========== Usage with Derive ==========

#[derive(Debug, Clone, Widget)]  // ← Macro generates Widget impl
pub struct Text {
    data: String,
}

impl StatelessWidget for Text {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        // Implementation
    }
}

// Macro generates:
// impl Widget for Text {
//     type Element = ComponentElement<Self>;
//     fn create_element(&self) -> Self::Element {
//         ComponentElement::new(self)
//     }
// }

#[derive(Debug, Widget)]
pub struct Counter {
    initial_count: i32,
}

impl StatefulWidget for Counter {
    type State = i32;

    fn create_state(&self) -> i32 {
        self.initial_count
    }

    fn build(&self, ctx: &BuildContext, state: &i32) -> Box<dyn Widget> {
        Box::new(Column::new(vec![
            Box::new(Text::new(format!("Count: {}", state))),
            Box::new(Button::new("+")),
        ]))
    }
}
```

**Преимущества:**

✅ **Flutter-like API** - знакомо для Flutter разработчиков
✅ **Компилируется** - derive macro генерирует impl
✅ **Меньше изменений** - можно мигрировать постепенно
✅ **Проще для пользователей** - только derive + один trait impl

**Недостатки:**

❌ **Требует proc macros** - больше сложности в build
❌ **Type erasure** - Box<dyn Widget> убирает type safety
❌ **Runtime dispatch** - менее производительно
❌ **Менее гибко** - чем Xilem подход

---

## 📊 Рекомендации

### Для Flui v0.1 (MVP):

**Используйте Вариант B (Гибридный):**

1. Простой API для пользователей
2. Flutter-like ментальная модель
3. Быстрая миграция существующего кода
4. Derive macros решают coherence проблему

### Для Flui v1.0 (Production):

**Рассмотрите Вариант A (Xilem Style):**

1. Лучшая производительность
2. Больше type safety
3. Гибкость для сложных use cases
4. Проверенная архитектура

---

## 🔧 План миграции (Вариант B)

### Шаг 1: Создать derive macro

```rust
// В flui_derive/src/lib.rs
#[proc_macro_derive(Widget)]
pub fn derive_widget(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Detect which trait is implemented (StatelessWidget или StatefulWidget)
    // Generate appropriate Widget impl

    quote! {
        impl Widget for #name {
            type Element = ComponentElement<Self>;

            fn create_element(&self) -> Self::Element {
                ComponentElement::new(self.clone())
            }
        }
    }.into()
}
```

### Шаг 2: Обновить существующие widgets

```rust
// Было:
impl Widget for Text {
    type Element = ComponentElement<Self>;
    // ...
}

impl StatelessWidget for Text {
    // ...
}

// Стало:
#[derive(Widget)]
struct Text { ... }

impl StatelessWidget for Text {
    // ...
}
```

### Шаг 3: Добавить tests

```rust
#[test]
fn test_stateless_widget() {
    #[derive(Widget)]
    struct MyWidget;

    impl StatelessWidget for MyWidget {
        fn build(&self, _ctx: &BuildContext) -> Box<dyn Widget> {
            Box::new(Text::new("Hello"))
        }
    }

    let widget = MyWidget;
    let element = widget.create_element();
    // assertions...
}
```

---

## 📚 Дополнительные материалы

- [Xilem Architecture](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html)
- [Xilem GitHub](https://github.com/linebender/xilem)
- [Rust Orphan Rules](https://doc.rust-lang.org/reference/items/implementations.html#orphan-rules)
- [Trait Coherence](https://rust-lang.github.io/rfcs/2451-re-rebalancing-coherence.html)
