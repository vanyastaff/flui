# Action в Xilem: Полное объяснение

## 🎯 Что такое Action?

`Action` в Xilem - это **generic параметр типа**, который представляет собой **сообщения/события**, которые view может отправлять вверх по дереву к родительским view.

Это часть **Elm Architecture** паттерна: `Model → View → Update`.

---

## 📊 Сигнатура View trait

```rust
pub trait View<State, Action, Context: ViewPathTracker>: ViewMarker + 'static {
    type Element: ViewElement;
    type ViewState;

    fn build(&self, ctx: &mut Context, app_state: &mut State)
        -> (Self::Element, Self::ViewState);

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState,
               ctx: &mut Context, element: Mut<'_, Self::Element>,
               app_state: &mut State);

    fn message(&self, view_state: &mut Self::ViewState,
               message: &mut MessageContext,
               element: Mut<'_, Self::Element>,
               app_state: &mut State) -> MessageResult<Action>;
    //                                                   ^^^^^^ Возвращает Action!
}
```

**Три generic параметра:**

1. **`State`** - Тип состояния приложения (модель данных)
2. **`Action`** - Тип сообщений, которые view может генерировать
3. **`Context`** - Контекст для построения view (обычно `ViewCtx`)

---

## 🔄 MessageResult<Action>

```rust
pub enum MessageResult<Action> {
    /// Действие для родительского view
    Action(Action),

    /// View запрашивает rebuild (например, после async операции)
    RequestRebuild,

    /// Событие не повлияло на состояние
    Nop,

    /// View, к которому шло сообщение, больше не существует
    Stale,
}
```

**Когда view обрабатывает событие (например, клик кнопки), он может:**

1. Вернуть `Action(action)` - родитель должен обработать это действие
2. Вернуть `RequestRebuild` - нужен rebuild без изменения state
3. Вернуть `Nop` - ничего не делать
4. Вернуть `Stale` - сообщение пришло к несуществующему view

---

## 💡 Примеры использования

### Пример 1: Простой счетчик (без Action)

```rust
#[derive(Default)]
struct AppState {
    count: i32,
}

fn app_logic(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    flex_column((
        label(format!("Count: {}", state.count)),
        button("Increment", |state: &mut AppState| {
            state.count += 1;
            // Нет Action! Callback напрямую изменяет state
        }),
    ))
}
```

**Здесь:**
- `Action = ()` (unit type)
- Callback изменяет `state` напрямую
- Простой случай, нет нужды в Action

---

### Пример 2: Elm Architecture (с Action)

```rust
#[derive(Default)]
struct AppState {
    count: i32,
}

// Определяем Action enum
enum CountMessage {
    Increment,
    Decrement,
    Reset,
}

// Дочерний view, который генерирует CountMessage
fn counter_view<T: 'static>(count: i32) -> impl WidgetView<T, CountMessage> {
    //                                                        ^^^^^^^^^^^^ Action type!
    flex_column((
        label(format!("Count: {count}")),
        text_button("+", |_| CountMessage::Increment),
        text_button("-", |_| CountMessage::Decrement),
        text_button("reset", |_| CountMessage::Reset),
    ))
}

// Родительский view, который обрабатывает CountMessage
fn app_logic(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    map_action(
        counter_view(state.count),
        |state: &mut AppState, message: CountMessage| {
            // Обработчик Action от дочернего view
            match message {
                CountMessage::Increment => state.count += 1,
                CountMessage::Decrement => state.count -= 1,
                CountMessage::Reset => state.count = 0,
            }
        },
    )
}
```

**Здесь:**
- `counter_view` имеет `Action = CountMessage`
- `counter_view` **не изменяет state напрямую**
- Вместо этого он **возвращает сообщение** (Action)
- Родитель **получает сообщение** и **обновляет state**
- Это **Elm Architecture** паттерн!

---

### Пример 3: Модульные компоненты с разными Action

```rust
#[derive(Default)]
struct AppState {
    counter: i32,
    text: String,
}

// Компонент 1: Counter с своим Action
enum CounterAction {
    Increment,
    Decrement,
}

fn counter_component(count: i32) -> impl WidgetView<i32, CounterAction> {
    flex_row((
        text_button("+", |_| CounterAction::Increment),
        label(format!("{count}")),
        text_button("-", |_| CounterAction::Decrement),
    ))
}

// Компонент 2: Text input с своим Action
enum TextInputAction {
    Changed(String),
    Clear,
}

fn text_input_component(text: &str) -> impl WidgetView<String, TextInputAction> {
    flex_row((
        textbox(text, |text, new_text| {
            *text = new_text.clone();
            TextInputAction::Changed(new_text)
        }),
        text_button("Clear", |_| TextInputAction::Clear),
    ))
}

// Главный view объединяет оба компонента
fn app_logic(state: &mut AppState) -> impl WidgetView<AppState> + use<> {
    flex_column((
        // Используем lens для работы с частью state
        map_action(
            lens(counter_component, |state: &mut AppState| &mut state.counter),
            |state: &mut AppState, action: CounterAction| {
                match action {
                    CounterAction::Increment => state.counter += 1,
                    CounterAction::Decrement => state.counter -= 1,
                }
            },
        ),

        map_action(
            lens(text_input_component, |state: &mut AppState| &mut state.text),
            |state: &mut AppState, action: TextInputAction| {
                match action {
                    TextInputAction::Changed(new_text) => state.text = new_text,
                    TextInputAction::Clear => state.text.clear(),
                }
            },
        ),
    ))
}
```

**Здесь:**
- Каждый компонент имеет **свой собственный Action type**
- `map_action` преобразует Action дочернего view в изменения state
- `lens` дает компоненту доступ только к части state
- Это позволяет создавать **модульные, переиспользуемые компоненты**

---

## 🔧 map_action vs map_message

### `map_action` (проще)

```rust
map_action(
    child_view,
    |state: &mut State, action: ChildAction| {
        // Обработать action, обновить state
        // Автоматически возвращает MessageResult::Action(())
    },
)
```

**Используется когда:**
- Нужно просто обработать action от дочернего view
- Изменить state на основе action
- Автоматически генерирует `MessageResult`

### `map_message` (гибче)

```rust
map_message(
    child_view,
    |state: &mut State, message: MessageResult<ChildAction>| -> MessageResult<ParentAction> {
        match message {
            MessageResult::Action(action) => {
                // Обработать action
                MessageResult::Action(ParentAction::Something)
            }
            MessageResult::Nop => MessageResult::Nop,
            MessageResult::RequestRebuild => MessageResult::RequestRebuild,
            MessageResult::Stale => MessageResult::Stale,
        }
    },
)
```

**Используется когда:**
- Нужен полный контроль над `MessageResult`
- Хотите преобразовать `ChildAction` в `ParentAction`
- Нужно обработать `RequestRebuild`, `Nop`, `Stale` специальным образом

---

## 🎨 Паттерны использования Action

### Паттерн 1: No Action (прямое изменение)

```rust
button("Click", |state: &mut AppState| {
    state.count += 1;
    // Никакого Action, просто меняем state
})
```

**Когда использовать:**
- Простые случаи
- Локальные изменения
- Не нужна модульность

### Паттерн 2: Simple Action (enum messages)

```rust
enum Message {
    Increment,
    Decrement,
}

text_button("+", |_| Message::Increment)
```

**Когда использовать:**
- Модульные компоненты
- Elm Architecture
- Переиспользуемые view

### Паттерн 3: Complex Action (with data)

```rust
enum UserAction {
    Login { username: String, password: String },
    Logout,
    UpdateProfile { name: String, email: String },
}

text_button("Login", |state| {
    UserAction::Login {
        username: state.username.clone(),
        password: state.password.clone(),
    }
})
```

**Когда использовать:**
- Сложная бизнес-логика
- Нужно передать данные вверх
- Разделение presentation и business logic

---

## 🔄 Сравнение с Flutter

### Flutter Callbacks

```dart
class MyButton extends StatelessWidget {
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return ElevatedButton(
      onPressed: onPressed,  // ← Callback
      child: Text('Click'),
    );
  }
}

// Usage:
MyButton(onPressed: () {
  setState(() {
    count++;
  });
})
```

### Xilem Action

```rust
fn my_button<State, Action>(
    label: &str,
    on_press: impl Fn(&mut State) -> Action + 'static,
) -> impl WidgetView<State, Action> {
    button(label, on_press)  // ← Возвращает Action
}

// Usage:
map_action(
    my_button("Click", |_| CounterAction::Increment),
    |state, action| {
        match action {
            CounterAction::Increment => state.count += 1,
        }
    },
)
```

**Отличия:**

| Flutter | Xilem |
|---------|-------|
| Callback функции | Action messages |
| Прямой вызов | Message passing |
| `VoidCallback` | `MessageResult<Action>` |
| setState() | Автоматический rebuild |

---

## 💭 Для Flui: Стоит ли использовать Action?

### ✅ Плюсы Action паттерна:

1. **Модульность** - компоненты не зависят от конкретного State
2. **Переиспользуемость** - view можно использовать с разными State
3. **Тестируемость** - легко тестировать Action без UI
4. **Elm Architecture** - проверенный паттерн
5. **Type safety** - компилятор проверяет все Action

### ❌ Минусы Action паттерна:

1. **Сложнее для новичков** - нужно понимать Elm Architecture
2. **Больше boilerplate** - enum для Action, map_action
3. **Не всегда нужно** - для простых случаев overkill
4. **Отличается от Flutter** - Flutter использует callbacks

### 🎯 Рекомендация для Flui:

**Поддержите ОБА паттерна:**

```rust
// Паттерн 1: Flutter-like callbacks (для новичков)
pub trait Widget {
    fn build(&self, ctx: &BuildContext) -> Element;
}

button("Click", |state: &mut AppState| {
    state.count += 1;  // Прямое изменение
})

// Паттерн 2: Xilem-like Actions (для продвинутых)
pub trait Widget<State, Action = ()> {
    type Element;
    type WidgetState;

    fn build(&self, ctx: &mut BuildContext, state: &mut State)
        -> (Self::Element, Self::WidgetState);

    fn on_event(&self, ...) -> MessageResult<Action>;
}
```

**Начните с Flutter-like (проще), добавьте Action позже (для модульности).**

---

## 📚 Резюме

**Action в Xilem:**
- Generic параметр типа
- Представляет сообщения от view к родителю
- Часть Elm Architecture (Model-View-Update)
- Позволяет создавать модульные, переиспользуемые компоненты
- Альтернатива Flutter callbacks

**Три способа работы:**
1. **No Action** - прямое изменение state (проще)
2. **Simple Action** - enum messages (модульность)
3. **Complex Action** - данные в сообщениях (flexibility)

**Для Flui:**
- Начните без Action (как Flutter)
- Добавьте Action позже для модульности
- Поддержите оба паттерна для гибкости
