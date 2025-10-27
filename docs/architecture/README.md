# FLUI Engine Documentation

**FLUI** (Flutter-inspired UI) - высокопроизводительный UI фреймворк на Rust, который берет лучшие идеи Flutter и адаптирует их под идиомы Rust для достижения **10x улучшения** по всем параметрам.

## 🎯 Цели Проекта

### Основные Цели

1. **Performance** - 3-5x быстрее Flutter
   - Zero GC pauses (детерминированная память)
   - Parallel layout/paint возможности
   - GPU-accelerated rendering
   - Aggressive caching и incremental updates

2. **Type Safety** - Compile-time гарантии
   - Arity system предотвращает runtime ошибки
   - Нет null pointer exceptions (Option<T>)
   - Нет data races (borrow checker)
   - Статическая типизация везде

3. **Developer Experience** - Эргономичный Rust-идиоматичный API
   - Fine-grained reactivity (Signals)
   - Прозрачная реактивность (автоматический tracking)
   - Builder patterns и fluent API
   - Минимальный boilerplate

4. **Enterprise Ready** - Готовность для production
   - Security audit friendly
   - Formal verification поддержка
   - Compliance и certifications
   - Large codebase support

## 🏗️ Архитектура

FLUI построен на трехслойной архитектуре:

```
┌─────────────────────────────────────┐
│   Widget Layer (Configuration)     │  ← Immutable, declarative
├─────────────────────────────────────┤
│   Element Layer (State Holders)    │  ← Mutable, lifecycle management
├─────────────────────────────────────┤
│ RenderObject Layer (Layout & Paint)│  ← Performance-critical rendering
└─────────────────────────────────────┘
           ↓
┌─────────────────────────────────────┐
│      Layer Tree (Compositing)       │  ← GPU-optimized layers
└─────────────────────────────────────┘
           ↓
┌─────────────────────────────────────┐
│    Render Backend (wgpu/egui)       │  ← Platform abstraction
└─────────────────────────────────────┘
```

## 📦 Структура Проекта

```
flui/
├── crates/
│   ├── flui_core/           # Ядро: Widget, Element, RenderObject
│   ├── flui_engine/         # Layers, Compositor, Paint
│   ├── flui_widgets/        # Стандартные виджеты
│   ├── flui_rendering/      # Render backend (wgpu, soft, egui)
│   ├── flui_types/          # Общие типы (Size, Color, etc)
│   ├── flui_reactive/       # Signal system (fine-grained reactivity)
│   ├── flui_animation/      # Анимации и transitions
│   ├── flui_gestures/       # Input handling и gesture recognition
│   ├── flui_platform/       # Platform integration (window, events)
│   └── flui_derive/         # Proc macros
│
├── examples/                # Примеры использования
├── docs/                    # Техническая документация (эта папка!)
└── benches/                 # Benchmarks
```

## 🚀 Ключевые Особенности

### 1. Type-Safe Arity System

```rust
// ✅ Compile-time проверка количества детей
impl RenderObject for RenderOpacity {
    type Arity = SingleArity;  // Ровно один ребенок
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();  // ✅ Компилятор гарантирует наличие!
        cx.layout_child(child, cx.constraints())
    }
}
```

### 2. Fine-Grained Reactivity

```rust
// ✅ Автоматический dependency tracking
pub struct CounterState {
    count: Signal<i32>,
}

impl State for CounterState {
    fn build(&mut self) -> BoxedWidget {
        column![
            // Только этот text rebuilds при изменении count!
            text(format!("Count: {}", self.count.get())),
            button("+").on_press_signal_inc(&self.count),
        ]
    }
}
```

### 3. Zero-Cost Abstractions

```rust
// Нет Box<dyn RenderObject> - все monomorphized
// Нет downcast - типы известны на compile-time
// Полная inline оптимизация для LLVM
```

### 4. GPU-Accelerated Rendering

- Direct wgpu integration
- Layer-based compositing
- GPU compute shaders для effects
- Cached rasterization

## 📊 Performance Goals

| Metric | Flutter | FLUI Goal | Improvement |
|--------|---------|-----------|-------------|
| Layout time | 16ms | 3-5ms | **3-5x** |
| Paint time | 8ms | 2-3ms | **2-4x** |
| Memory usage | 150MB | 50MB | **3x** |
| Cold start | 800ms | 200ms | **4x** |
| Bundle size | 15MB | 3MB | **5x** |
| GC pauses | 5-20ms | 0ms | **♾️** |

## 📚 Документация

Полная техническая документация организована следующим образом:

- **[SUMMARY.md](SUMMARY.md)** - навигация по всем главам
- **[01_architecture.md](01_architecture.md)** - общая архитектура
- **[02_widget_element_system.md](02_widget_element_system.md)** - Widget/Element система
- **[03_render_objects.md](03_render_objects.md)** - RenderObject и pipeline
- **[04_layout_engine.md](04_layout_engine.md)** - Layout constraints и cache
- **[05_layers_and_painters.md](05_layers_and_painters.md)** - Layer tree и compositing
- **[06_render_backend.md](06_render_backend.md)** - Render backends (wgpu, soft)
- **[07_input_and_events.md](07_input_and_events.md)** - Input handling
- **[08_frame_scheduler.md](08_frame_scheduler.md)** - Frame scheduling
- **[09_debug_and_devtools.md](09_debug_and_devtools.md)** - Debug tools
- **[10_future_extensions.md](10_future_extensions.md)** - Future plans

## 🎓 Начало Работы

### Установка

```toml
[dependencies]
flui_core = "0.1"
flui_widgets = "0.1"
flui_engine = "0.1"
```

### Hello World

```rust
use flui_core::*;
use flui_widgets::*;

fn main() {
    run_app(
        text("Hello, FLUI!")
            .size(24.0)
            .color(Color::BLUE)
    );
}
```

### Counter Example

```rust
use flui_core::*;
use flui_widgets::*;
use flui_reactive::Signal;

#[derive(Debug, Clone)]
pub struct Counter {
    initial: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;
    
    fn create_state(&self) -> Self::State {
        CounterState {
            count: Signal::new(self.initial),
        }
    }
}

#[derive(Debug)]
pub struct CounterState {
    count: Signal<i32>,
}

impl State for CounterState {
    type Widget = Counter;
    
    fn build(&mut self) -> BoxedWidget {
        Box::new(
            column![
                text(format!("Count: {}", self.count.get())),
                row![
                    button("−").on_press_signal_dec(&self.count),
                    button("+").on_press_signal_inc(&self.count),
                ],
            ]
        )
    }
}

fn main() {
    run_app(Counter { initial: 0 });
}
```

## 🤝 Contributing

FLUI - это open-source проект. Contributions приветствуются!

1. Fork repository
2. Create feature branch
3. Implement changes с tests
4. Submit pull request

См. [CONTRIBUTING.md](../CONTRIBUTING.md) для деталей.

## 📄 License

MIT License - см. [LICENSE](../LICENSE)

## 🔗 Ссылки

- **GitHub:** https://github.com/your-org/flui
- **Documentation:** https://docs.flui.dev
- **Discord:** https://discord.gg/flui
- **Crates.io:** https://crates.io/crates/flui

## 🙏 Acknowledgments

FLUI вдохновлен следующими проектами:

- **Flutter** - архитектура Widget/Element/RenderObject
- **Leptos** - fine-grained reactivity signals
- **Dioxus** - Rust-first подход к UI
- **Xilem** - reactive architecture research
- **Druid** - Rust UI framework pioneers

---

**Next:** [Начните с SUMMARY.md для навигации по документации](SUMMARY.md)
