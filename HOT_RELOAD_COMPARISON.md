# Hot Reload: Flutter vs Xilem vs Dioxus vs Flui

## 🔥 Что такое Hot Reload?

**Hot Reload** - возможность изменять код и **мгновенно видеть результат** без перезапуска приложения и **без потери состояния**.

### Flutter Hot Reload (эталон):

```dart
class Counter extends StatefulWidget {
  @override
  _CounterState createState() => _CounterState();
}

class _CounterState extends State<Counter> {
  int count = 42; // ← State сохраняется

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Text('Count: $count'),
        ElevatedButton(
          child: Text('Increment'), // ← Меняем на 'Add'
          onPressed: () => setState(() => count++),
        ),
      ],
    );
  }
}

// 1. Нажали кнопку несколько раз → count = 45
// 2. Изменили 'Increment' на 'Add'
// 3. Нажали Ctrl+S / cmd+S
// 4. UI обновился мгновенно, count = 45 (состояние сохранилось!)
```

**Ключевые возможности Flutter Hot Reload:**
- ⚡ **< 1 секунда** обновление
- 💾 **Сохраняет state** виджетов
- 🎯 **Сохраняет навигацию** (на какой странице были)
- 🔄 **Incremental compilation** (только изменённые файлы)
- 📱 **Работает на устройстве** (не только эмулятор)

---

## 📊 Сравнение фреймворков

| Фреймворк | Hot Reload | Скорость | Сохраняет State | Платформы | Статус |
|-----------|------------|----------|-----------------|-----------|--------|
| **Flutter** | ✅ Да | < 1s | ✅ Да | All | Production |
| **Dioxus** | ✅ Да | < 1s | ✅ Да | All | Production |
| **Xilem** | ❌ Нет | - | - | Desktop | Planned |
| **Slint** | ✅ Частично | ~2s | ❌ Нет | All | Production |
| **egui** | ❌ Нет | - | - | All | Production |
| **iced** | ❌ Нет | - | - | All | Production |
| **Flui** | 🎯 Цель | ? | ? | ? | Concept |

---

## 🔍 Детальный анализ

### 1. 🏆 Flutter (Dart) - Эталон

**Как работает:**

```
1. Разработчик меняет код
   ↓
2. Dart VM получает изменённый код
   ↓
3. VM инжектит новый код в работающее приложение
   ↓
4. Framework вызывает rebuild() для затронутых виджетов
   ↓
5. UI обновляется, state сохраняется
```

**Технические детали:**
- Dart VM поддерживает **hot code replacement**
- Dart - **JIT compiled** в dev mode
- Framework **сериализует state** перед reload
- После reload **восстанавливает state**
- Работает через **observatory protocol**

**Ограничения:**
- ❌ Не работает для изменений в `main()`
- ❌ Не работает для изменений в `initState()`
- ❌ Не работает для изменений в global variables
- ❌ Не работает для изменений в native code

**Производительность:**
- ⚡ ~500ms в среднем
- ⚡ ~200ms для маленьких изменений
- ⚡ ~1s для больших изменений

---

### 2. 🎨 Dioxus (Rust) - Лучший в Rust

**Статус:** ✅ **Работает!** (powered by Subsecond)

**Как работает:**

```rust
// До изменения
fn app(cx: Scope) -> Element {
    let mut count = use_state(cx, || 0);

    cx.render(rsx! {
        div {
            "Count: {count}"
            button {
                onclick: move |_| count += 1,
                "Increment" // ← Меняем на "Add"
            }
        }
    })
}

// После Ctrl+S:
// 1. Subsecond recompiles только изменённый код
// 2. Создаёт WASM patch
// 3. Инжектит patch в работающее приложение
// 4. Dioxus перерендерит компоненты
// 5. State сохранён! (count остался как был)
```

**Технология:**

- **Subsecond** - инструмент hot-reload для Rust
- **WASM binary patching** - патчит WASM на лету
- **Hot state reload** - сохраняет state через перекомпиляцию
- **Works everywhere** - Web, Desktop, Mobile

**Что работает:**
- ✅ UI изменения (текст, стили, layout)
- ✅ Logic изменения (event handlers, callbacks)
- ✅ State сохраняется
- ✅ Работает на всех платформах
- ✅ Incremental compilation

**Ограничения:**
- ❌ Не работает для изменений в `main()`
- ❌ Не работает для struct definitions (нужен full restart)
- ❌ Медленнее Flutter (~1-2s vs ~500ms)

**Пример:**

```rust
use dioxus::prelude::*;

fn main() {
    // Launch with hot reload enabled
    dioxus_desktop::launch_cfg(
        app,
        dioxus_desktop::Config::new()
            .with_hot_reload(true)
    );
}

fn app(cx: Scope) -> Element {
    let mut count = use_state(cx, || 0);

    cx.render(rsx! {
        div {
            style: "padding: 20px",
            h1 { "Counter: {count}" }
            button {
                onclick: move |_| count += 1,
                "Increment"
            }
        }
    })
}

// Меняем "Increment" на "Add" → Ctrl+S → мгновенно видим изменения!
```

---

### 3. 🦀 Xilem - НЕТ hot reload (пока)

**Статус:** ❌ **Не реализовано** (но запланировано)

**План Xilem:**

```
Два процесса:

┌─────────────────────┐
│   App Process       │  ← Hot reload здесь
│  (app logic)        │
│  - View tree        │
│  - State            │
│  - Быстрая          │
│    перекомпиляция   │
└──────────┬──────────┘
           │ IPC
           │ (View tree)
           ↓
┌──────────┴──────────┐
│  Display Process    │  ← Long-lived
│  (widgets)          │
│  - Element tree     │
│  - Rendering        │
│  - Не перезапускается│
└─────────────────────┘
```

**Как это будет работать:**

1. **App Process** (легковесный):
   ```rust
   // Этот файл
   fn app_logic(data: &mut AppData) -> impl WidgetView<AppData> {
       flex_column((
           label(format!("Count: {}", data.count)),
           button("Increment", |data| data.count += 1),
       ))
   }
   ```

2. При изменении:
   - App Process **перекомпилируется** (~100-500ms)
   - **Отправляет новое View tree** в Display Process через IPC
   - Display Process **обновляет Element tree**
   - **State сохраняется** в Display Process

**Проблемы этого подхода:**

- ❌ **IPC overhead** (межпроцессное взаимодействие)
- ❌ **Сложная сериализация** View tree
- ❌ **Сложная отладка** (два процесса)
- ❌ **Не работает на Web** (нет процессов в браузере)
- ❌ **Пока не реализовано**

**Почему Xilem не может как Dioxus:**

- Rust = **AOT compiled** (не JIT как Dart)
- Нет встроенного hot code replacement в Rust
- Нужна перекомпиляция → медленнее
- Subsecond работает хорошо для Dioxus, но не интегрирован в Xilem

---

### 4. 🎯 Slint - Частичный hot reload

**Статус:** ✅ Работает, но ограниченно

**Что работает:**
- ✅ Изменения в `.slint` файлах (UI description)
- ✅ Live preview в редакторе
- ⚡ Мгновенное обновление (~100ms)

**Что НЕ работает:**
- ❌ Изменения в Rust коде (logic)
- ❌ State НЕ сохраняется
- ❌ Нужен restart для Rust изменений

**Пример:**

```slint
// ui.slint - это hot reloadится
export component MainWindow {
    property <int> count: 0;

    VerticalLayout {
        Text { text: "Count: \{count}"; } // ← Меняем текст
        Button {
            text: "Increment"; // ← Hot reload работает
            clicked => { count += 1; }
        }
    }
}
```

```rust
// main.rs - это НЕ hot reloadится
slint::include_modules!();

fn main() {
    let ui = MainWindow::new();

    // Логика в Rust требует restart
    ui.on_button_clicked(|| {
        println!("Clicked!");
    });

    ui.run();
}
```

---

### 5. ❌ egui & iced - Нет hot reload

**egui:**
- Immediate mode GUI
- Нет официального hot reload
- Можно использовать `cargo-watch` для auto-restart
- State теряется при restart

**iced:**
- Retained mode GUI
- Нет hot reload
- Можно использовать `cargo-watch`
- State теряется

---

## 🎯 Flui: Как сделать лучше всех?

### Проблема Rust:

**Rust = AOT compiled** → нет встроенного hot code replacement

**Решения:**

### Вариант 1: Как Dioxus (Subsecond)

```rust
// Используем Subsecond
[dependencies]
flui = "0.1"
subsecond = "0.1"

// Интеграция
fn main() {
    flui::launch_with_hot_reload(app);
}

fn app() -> impl Widget {
    let count = use_state(|| 0);

    Column::new(vec![
        Text::new(format!("Count: {}", count)).into(),
        Button::new("Add", move || *count += 1).into(),
    ])
}
```

**Плюсы:**
- ✅ Работает уже сейчас
- ✅ Cross-platform (Web, Desktop, Mobile)
- ✅ Сохраняет state
- ✅ Incremental compilation

**Минусы:**
- ❌ Зависимость от Subsecond
- ❌ ~1-2s (медленнее Flutter)
- ❌ Не работает для всех изменений

---

### Вариант 2: Как Slint (DSL)

```rust
// .flui файл (DSL)
widget MainView {
    state count: i32 = 0;

    Column {
        Text { text: "Count: {count}" }
        Button {
            label: "Add", // ← Hot reload работает
            on_click: || count += 1
        }
    }
}

// Rust код
fn main() {
    flui::launch(MainView::new());
}
```

**Плюсы:**
- ✅ Мгновенный hot reload DSL
- ✅ Можно иметь live preview в редакторе
- ✅ Простая сериализация

**Минусы:**
- ❌ Новый язык для изучения
- ❌ Rust логика НЕ hot reloadится
- ❌ Меньше гибкости чем Rust

---

### Вариант 3: Гибрид (Лучший?)

```rust
// Hot reloadable view (декларативный)
#[hot_reload]
fn counter_view(count: i32, on_increment: impl Fn()) -> impl Widget {
    Column::new(vec![
        Text::new(format!("Count: {}", count)).into(),
        Button::new("Add", on_increment).into(), // ← Hot reload
    ])
}

// Business logic (не hot reload, но это OK)
fn main() {
    let mut count = 0;

    flui::launch(move || {
        counter_view(count, || count += 1)
    });
}
```

**Как работает:**

1. `#[hot_reload]` macro генерирует:
   - Сериализацию view definition
   - IPC код для передачи изменений
   - State preservation logic

2. При изменении view функции:
   - Перекомпилируется только view код
   - Отправляется новое определение
   - Framework обновляет UI
   - State сохраняется

**Плюсы:**
- ✅ Hot reload для UI кода
- ✅ Нормальный Rust (не DSL)
- ✅ Быстрая компиляция view functions
- ✅ State сохраняется

**Минусы:**
- ❌ Business logic не hot reloadится
- ❌ Нужна сложная инфраструктура

---

### Вариант 4: Interpreter-based (как Flutter/Dart)

**Идея:** Интерпретировать Rust в dev mode

```rust
// Flui компилирует view code в bytecode
#[flui::interpreted]
fn app() -> impl Widget {
    let count = use_state(|| 0);

    Column::new(vec![
        Text::new(format!("Count: {}", count)).into(),
        Button::new("Add", move || *count += 1).into(),
    ])
}

// В dev mode: компилируется в bytecode, интерпретируется
// В prod mode: нормальная AOT компиляция
```

**Плюсы:**
- ✅ True hot reload (как Flutter)
- ✅ Instant feedback (< 500ms)
- ✅ Сохраняет state
- ✅ Работает для большинства изменений

**Минусы:**
- ❌ Нужен Rust interpreter (огромная работа!)
- ❌ Медленнее в dev mode
- ❌ Два режима компиляции (сложность)

---

## 📊 Сравнение подходов для Flui

| Подход | Скорость | Complexity | Реалистичность | State | Рекомендация |
|--------|----------|------------|----------------|-------|--------------|
| **Subsecond (Dioxus)** | ~1-2s | Средняя | ✅ Высокая | ✅ Да | ⭐⭐⭐⭐ |
| **DSL (Slint)** | < 500ms | Средняя | ✅ Высокая | ❌ Нет | ⭐⭐⭐ |
| **Гибрид (macro)** | ~1s | Высокая | 🟡 Средняя | ✅ Да | ⭐⭐⭐⭐ |
| **Interpreter** | < 500ms | ❌ Очень высокая | ❌ Низкая | ✅ Да | ⭐⭐ |
| **Два процесса (Xilem)** | ~500ms | Высокая | 🟡 Средняя | ✅ Да | ⭐⭐⭐ |

---

## 🏆 Может ли Flui быть лучше Xilem в hot reload?

### ✅ **ДА!** Если:

1. **Интегрировать Subsecond** (как Dioxus)
   - Xilem: ❌ Не реализовано
   - Flui: ✅ Может использовать сейчас

2. **Фокус на DX (Developer Experience)**
   - Сделать hot reload приоритетом #1
   - Xilem фокусируется на архитектуре

3. **Поддержка DSL** (опционально)
   - Для instant hot reload UI
   - Rust для бизнес-логики

4. **Better tooling**
   - Интеграция с VS Code/IntelliJ
   - Live preview в редакторе
   - Hot reload indicators

---

## 💡 Рекомендация для Flui

### Фаза 1: Используем Subsecond (быстрый старт)

```rust
// Просто интегрируем Subsecond
#[cfg(debug_assertions)]
flui::enable_hot_reload();

fn app() -> impl Widget {
    // Ваш код
}
```

**Преимущества:**
- ✅ Работает уже сейчас
- ✅ Минимум работы
- ✅ Как Dioxus (проверенное решение)

---

### Фаза 2: Улучшаем DX

```rust
// Better error messages
#[hot_reload]
fn view() -> impl Widget {
    // Если ошибка компиляции:
    // → Показываем в UI
    // → Указываем строку
    // → Предлагаем fix
}

// Live preview
// cargo flui preview view.rs
// → Открывает window с live preview
```

---

### Фаза 3: Опциональный DSL

```flui
// view.flui (опционально, для hot UI)
view CounterView(count: i32) {
    Column {
        Text("Count: {count}")
        Button("Add") { on_click: increment }
    }
}
```

```rust
// main.rs (бизнес-логика)
fn main() {
    let mut count = 0;
    flui::launch(CounterView::new(count, || count += 1));
}
```

---

## 📝 Итоговое сравнение

| Фреймворк | Hot Reload | Скорость | Сложность реализации | Для Flui |
|-----------|------------|----------|----------------------|----------|
| **Flutter** | 🏆 Отлично | < 500ms | ❌ Impossible (JIT Dart) | Эталон |
| **Dioxus** | ✅ Хорошо | ~1s | ✅ Можем скопировать | ⭐⭐⭐⭐⭐ |
| **Xilem** | ❌ Нет | - | 🟡 Планируется | ✅ Можем быть лучше! |
| **Slint** | 🟡 Частично | < 500ms | ✅ Реализуемо | ⭐⭐⭐ |

---

## 🎯 Вывод

**Flui МОЖЕТ быть лучше Xilem в hot reload:**

1. ✅ Xilem не имеет hot reload (пока)
2. ✅ Flui может интегрировать Subsecond (как Dioxus)
3. ✅ Можно сделать лучший DX
4. ✅ Можно добавить DSL для instant hot reload
5. ✅ Это реальное преимущество!

**Это БОЛЬШОЕ преимущество для Flutter разработчиков!**

Hot reload - это то, что делает Flutter таким продуктивным. Если Flui сможет предложить hot reload на уровне Dioxus или лучше, это будет **killer feature**! 🔥
