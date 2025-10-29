# `impl Widget + use<>` Pattern для сокращения кода

## 🎯 Проблема: Verbose возвращаемые типы

### Без `impl Widget`:

```rust
// ❌ Нужно указывать конкретный тип
fn my_button() -> Widget {
    Widget::stateless(Button::new("Click", || {}))
}

// ❌ Требует boxing
fn conditional_widget(show: bool) -> Widget {
    if show {
        Widget::stateless(Text::new("Hello"))
    } else {
        Widget::stateless(Placeholder::new())
    }
}
```

### С `impl Widget`:

```rust
// ✅ Короче!
fn my_button() -> impl Widget {
    Button::new("Click", || {})
}

// ❌ Но это не скомпилируется с enum!
// Потому что Widget - это enum, не trait
```

**Проблема:** `impl Widget` работает только если `Widget` - это **trait**, но у нас `Widget` - это **enum**!

---

## 🤔 Как это работает в Xilem?

### Xilem использует trait View:

```rust
// View - это TRAIT
pub trait View<State, Action, Context> { ... }

// impl View возвращает анонимный тип
fn button(label: &str) -> impl WidgetView<AppState> + use<> {
    button(label, |state| state.count += 1)
}

// Компилятор знает конкретный тип:
// impl WidgetView<AppState> = Button<impl Fn(&mut AppState)>
```

**Ключ:** `impl Trait` работает с **trait'ами**, не с **enum'ами**!

---

## 💡 Решения для Flui

### Вариант 1: Widget как Trait (для builder functions)

```rust
// Создаём trait для builder API
pub trait IntoWidget {
    fn into_widget(self) -> Widget;
}

// Blanket impl для всех типов
impl<T: StatelessWidget + 'static> IntoWidget for T {
    fn into_widget(self) -> Widget {
        Widget::stateless(self)
    }
}

impl<T: StatefulWidget + 'static> IntoWidget for T {
    fn into_widget(self) -> Widget {
        Widget::stateful(self)
    }
}

// Теперь можем использовать impl IntoWidget
fn my_button() -> impl IntoWidget {
    Button::new("Click", || {})
}

// Usage
let widget = my_button().into_widget();
```

**Плюсы:**
- ✅ `impl IntoWidget` короче чем конкретный тип
- ✅ Flexibility

**Минусы:**
- ❌ Нужно вызывать `.into_widget()`
- ❌ Не так чисто как Xilem

---

### Вариант 2: Builder Functions возвращают конкретные типы

```rust
// Просто возвращаем конкретный тип
pub fn text(content: impl Into<String>) -> Text {
    Text::new(content)
}

pub fn button(
    label: impl Into<String>,
    on_press: impl Fn() + 'static,
) -> Button {
    Button::new(label, on_press)
}

pub fn column(children: Vec<Widget>) -> Column {
    Column::new(children)
}

// Usage
fn my_ui() -> Widget {
    column(vec![
        text("Hello").into(),  // ← .into() для Widget
        button("Click", || {}).into(),
    ]).into()
}
```

**Плюсы:**
- ✅ Конкретный тип (хорошо для type inference)
- ✅ Просто понять

**Минусы:**
- ❌ Нужен `.into()` для Widget
- ❌ Verbose в больших UI

---

### Вариант 3: Гибридный подход (Type Alias + use<>)

```rust
// Type alias для частых паттернов
pub type StatelessWidgetFn = impl IntoWidget + use<>;

// Helper trait
pub trait IntoWidget {
    fn into_widget(self) -> Widget;
}

// Builder functions
pub fn text(content: impl Into<String>) -> impl IntoWidget + use<> {
    Text::new(content)
}

pub fn button(
    label: impl Into<String>,
    on_press: impl Fn() + 'static,
) -> impl IntoWidget + use<> {
    Button::new(label, on_press)
}

// Composable functions
fn my_button() -> impl IntoWidget + use<> {
    button("Click", || {})
}

fn my_ui() -> impl IntoWidget + use<> {
    column(vec![
        text("Hello").into_widget(),
        my_button().into_widget(),
    ])
}

// Final conversion
let widget: Widget = my_ui().into_widget();
```

**Плюсы:**
- ✅ `impl IntoWidget + use<>` - короткая запись
- ✅ Composable functions
- ✅ Type inference работает

**Минусы:**
- ❌ Всё равно нужен `.into_widget()` в конце
- ❌ Дополнительный trait

---

### Вариант 4: Macro для автоматического `.into()`

```rust
// Macro упрощает создание UI
macro_rules! ui {
    ($widget:expr) => {
        $widget.into()
    };
}

// Или более мощный:
macro_rules! column {
    ($($child:expr),* $(,)?) => {
        Column::new(vec![
            $(ui!($child)),*
        ])
    };
}

// Usage
fn my_ui() -> Widget {
    column![
        text("Hello"),         // ← Автоматически .into()
        button("Click", || {}),
    ].into()
}
```

**Плюсы:**
- ✅ Чистый синтаксис
- ✅ Автоматическая конвертация

**Минусы:**
- ❌ Macros сложнее понять
- ❌ Плохие error messages

---

## 🎨 Что делает Xilem подробнее

### Xilem code:

```rust
// View trait (обобщённый)
pub trait View<State, Action, Context>: ViewMarker + 'static {
    type Element: ViewElement;
    type ViewState;

    fn build(&self, ctx: &mut Context, state: &mut State)
        -> (Self::Element, Self::ViewState);
}

// Button - конкретный тип
pub struct Button<F, V> {
    child: V,
    callback: F,
}

// Button реализует View
impl<F, V, State, Action> View<State, Action, ViewCtx> for Button<F, V>
where
    V: WidgetView<State, Action>,
    F: Fn(&mut State, Option<PointerButton>) -> MessageResult<Action> + 'static,
{
    // ...
}

// Helper function возвращает impl View
pub fn button<State, Action, V>(
    child: V,
    callback: impl Fn(&mut State) -> Action + 'static,
) -> impl WidgetView<State, Action> + use<State, Action, V>
//     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
//     Анонимный тип который реализует WidgetView
where
    V: WidgetView<State, Action>,
{
    Button {
        child,
        callback: move |state, button| {
            MessageResult::Action(callback(state))
        },
    }
}

// Composable
fn my_button() -> impl WidgetView<AppState> + use<> {
    button("Click", |state: &mut AppState| {
        state.count += 1;
    })
}

fn app_logic(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    flex_column((
        label(format!("Count: {}", state.count)),
        my_button(),
    ))
}
```

**Почему это работает:**
1. `View` - **trait** (не enum)
2. `impl View` возвращает конкретный тип (Button<F, V>)
3. `+ use<>` указывает, что тип не захватывает lifetime
4. Composition через generics

---

## 🔄 Как адаптировать для Flui?

### Вариант A: Dual API (enum + trait)

```rust
// Widget enum для runtime
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
    RenderObject(Box<dyn RenderObjectWidget>),
}

// Trait для compile-time composition
pub trait IntoWidget: 'static {
    fn into_widget(self) -> Widget;
}

// Blanket impl
impl<T: StatelessWidget> IntoWidget for T {
    fn into_widget(self) -> Widget {
        Widget::stateless(self)
    }
}

// Builder functions возвращают impl IntoWidget
pub fn text(content: impl Into<String>) -> impl IntoWidget + use<> {
    Text::new(content)
}

pub fn button(
    label: impl Into<String>,
    on_press: impl Fn() + 'static,
) -> impl IntoWidget + use<> {
    Button::new(label, on_press)
}

pub fn column(
    children: impl IntoIterator<Item = impl IntoWidget>,
) -> impl IntoWidget + use<> {
    Column::new(
        children.into_iter()
            .map(|c| c.into_widget())
            .collect()
    )
}

// Composable functions
fn counter_button(count: i32) -> impl IntoWidget + use<> {
    button(format!("Count: {}", count), move || {
        println!("Clicked!");
    })
}

fn my_ui(count: i32) -> impl IntoWidget + use<> {
    column([
        text("Counter App"),
        counter_button(count),
    ])
}

// Usage
fn main() {
    let widget: Widget = my_ui(0).into_widget();
    // Use widget...
}
```

**Преимущества:**
- ✅ `impl IntoWidget + use<>` короче чем конкретные типы
- ✅ Composable functions
- ✅ Type inference работает
- ✅ Widget enum для runtime
- ✅ No `.into()` в каждой строчке

**Недостатки:**
- ❌ Всё равно `.into_widget()` в конце
- ❌ Два API (enum и trait)

---

### Вариант B: Generic Widget Container

```rust
// Generic контейнер для compile-time
pub struct WidgetBuilder<W> {
    inner: W,
}

impl<W: IntoWidget> WidgetBuilder<W> {
    pub fn new(widget: W) -> Self {
        Self { inner: widget }
    }

    pub fn build(self) -> Widget {
        self.inner.into_widget()
    }
}

// Builder functions
pub fn text(content: impl Into<String>) -> WidgetBuilder<Text> {
    WidgetBuilder::new(Text::new(content))
}

pub fn button(
    label: impl Into<String>,
    on_press: impl Fn() + 'static,
) -> WidgetBuilder<Button> {
    WidgetBuilder::new(Button::new(label, on_press))
}

// Chainable API
impl<W: IntoWidget> WidgetBuilder<W> {
    pub fn key(self, key: Key) -> Self {
        // Set key...
        self
    }

    pub fn tooltip(self, text: &str) -> WidgetBuilder<Tooltip<W>> {
        WidgetBuilder::new(Tooltip::new(self.inner, text))
    }
}

// Usage
fn my_ui() -> Widget {
    column(vec![
        text("Hello")
            .tooltip("Greeting")
            .build(),

        button("Click", || {})
            .key(Key::from_str("my_button"))
            .build(),
    ])
}
```

**Преимущества:**
- ✅ Chainable API
- ✅ Type-safe
- ✅ Чистый синтаксис

**Недостатки:**
- ❌ `.build()` в конце каждого widget'а
- ❌ Boilerplate для каждой builder function

---

## 📊 Сравнение подходов

| Подход | Verbosity | Type Safety | Xilem-like | Сложность |
|--------|-----------|-------------|------------|-----------|
| **Widget enum (текущий)** | ❌ Высокая | ✅ Да | ❌ Нет | ✅ Простой |
| **IntoWidget trait** | 🟡 Средняя | ✅ Да | 🟡 Частично | 🟡 Средняя |
| **WidgetBuilder** | 🟡 Средняя | ✅ Да | ❌ Нет | 🟡 Средняя |
| **Macros** | ✅ Низкая | 🟡 Частичная | ❌ Нет | ❌ Сложная |
| **Pure Xilem (View trait)** | ✅ Низкая | ✅ Да | ✅ Да | ❌ Очень сложная |

---

## 🎯 Рекомендация для Flui

### Комбинированный подход:

```rust
// ========== 1. Widget enum для runtime ==========
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
    RenderObject(Box<dyn RenderObjectWidget>),
}

// ========== 2. IntoWidget trait для builder functions ==========
pub trait IntoWidget: 'static {
    fn into_widget(self) -> Widget;
}

// Blanket impls
impl<T: StatelessWidget> IntoWidget for T {
    fn into_widget(self) -> Widget {
        Widget::stateless(self)
    }
}

// Widget enum также реализует IntoWidget (identity)
impl IntoWidget for Widget {
    fn into_widget(self) -> Widget {
        self
    }
}

// ========== 3. Builder functions с impl IntoWidget ==========
pub fn text(content: impl Into<String>) -> impl IntoWidget + use<> {
    Text::new(content)
}

pub fn button(
    label: impl Into<String>,
    on_press: impl Fn() + 'static,
) -> impl IntoWidget + use<> {
    Button::new(label, on_press)
}

pub fn column(
    children: impl IntoIterator<Item = impl IntoWidget>,
) -> impl IntoWidget + use<> {
    Column::new(
        children.into_iter()
            .map(|c| c.into_widget())
            .collect()
    )
}

// ========== 4. Composable UI functions ==========
fn counter_button(count: i32) -> impl IntoWidget + use<> {
    button(format!("Count: {}", count), move || {
        println!("Clicked!");
    })
}

fn counter_ui(count: i32) -> impl IntoWidget + use<> {
    column([
        text("Counter App"),
        counter_button(count),
    ])
}

// ========== 5. Usage ==========
fn main() {
    // Composable functions
    let ui = counter_ui(0);

    // Convert to Widget enum for framework
    let widget: Widget = ui.into_widget();

    // Run app
    run_app(widget);
}
```

### Преимущества этого подхода:

1. ✅ **Короткие типы**: `impl IntoWidget + use<>` вместо конкретных
2. ✅ **Composable**: функции можно вкладывать
3. ✅ **Type inference**: компилятор выводит типы
4. ✅ **Widget enum**: для runtime type erasure
5. ✅ **Xilem-like**: похожий API
6. ✅ **Flutter-like**: всё ещё Widget концепция

### Пример использования:

```rust
// Короткая функция
fn app() -> impl IntoWidget + use<> {
    column([
        text("Hello"),
        button("Click", || {}),
    ])
}

// Вложенная композиция
fn header() -> impl IntoWidget + use<> {
    row([
        text("Logo"),
        button("Menu", || {}),
    ])
}

fn body(content: &str) -> impl IntoWidget + use<> {
    text(content)
}

fn page() -> impl IntoWidget + use<> {
    column([
        header(),
        body("Welcome!"),
    ])
}

// Main
fn main() {
    let widget = page().into_widget();
    run_app(widget);
}
```

---

## 🎨 Сравнение синтаксиса

### Без `impl IntoWidget`:

```rust
// ❌ Verbose
fn my_ui() -> Widget {
    Widget::stateless(
        Column::new(vec![
            Widget::stateless(Text::new("Hello")),
            Widget::stateless(Button::new("Click", || {})),
        ])
    )
}
```

### С `impl IntoWidget`:

```rust
// ✅ Короче
fn my_ui() -> impl IntoWidget + use<> {
    column([
        text("Hello"),
        button("Click", || {}),
    ])
}
```

### Xilem для сравнения:

```rust
// Xilem
fn app_logic(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    flex_column((
        label("Hello"),
        button("Click", |state| state.count += 1),
    ))
}
```

---

## ✅ Вывод

**Да, можем использовать `impl Widget + use<>` паттерн!**

### План:

1. ✅ Оставляем **Widget enum** для runtime
2. ✅ Добавляем **IntoWidget trait** для builder API
3. ✅ Builder functions возвращают **`impl IntoWidget + use<>`**
4. ✅ Финальная конвертация через **`.into_widget()`**

### Результат:

- ✅ **Короткий код** (как Xilem)
- ✅ **Composable** (функции можно вкладывать)
- ✅ **Type-safe** (compile-time проверки)
- ✅ **Flutter-like** (Widget концепция сохранена)
- ✅ **Best of both worlds** 🚀

**Это отличная идея!** 💡
