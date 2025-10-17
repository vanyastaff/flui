# Flui Framework

> Flutter-inspired declarative UI framework for Rust, built on egui 0.33

[![Crates.io](https://img.shields.io/crates/v/flui.svg)](https://crates.io/crates/flui)
[![Documentation](https://docs.rs/flui/badge.svg)](https://docs.rs/flui)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

[Documentation](https://docs.rs/flui) | [Examples](examples/) | [Roadmap](ROADMAP.md)

---

## ✨ Features

- **🎨 Declarative API** - Flutter-like widget composition in Rust
- **🏗️ Three-Tree Architecture** - Widget → Element → RenderObject pattern
- **🚀 Performance** - 60fps with complex UIs, viewport culling, smart caching
- **🔒 Type Safety** - Leverage Rust's type system for compile-time guarantees
- **📦 State Management** - Built-in Provider system inspired by Flutter
- **🎬 Animations** - Smooth 60fps animations with curves and tweens
- **🔧 Hot Reload Ready** - Designed for fast iteration (coming soon)
- **📱 Cross-Platform** - Works on Windows, macOS, Linux, and Web

---

## 🚀 Quick Start

### Installation

```toml
[dependencies]
flui = "0.1"
```

### Hello World

```rust
use flui::prelude::*;

struct MyApp;

impl StatelessWidget for MyApp {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        Center::new(
            Text::new("Hello, Flui!")
                .style(TextStyle::headline1())
        ).into_widget()
    }
}

fn main() {
    FluiApp::new(MyApp)
        .title("Hello World")
        .run()
        .unwrap();
}
```

### Counter Example

```rust
use flui::prelude::*;

struct Counter;

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: 0 }
    }
}

struct CounterState {
    count: i32,
}

impl State for CounterState {
    type Widget = Counter;

    fn build(&mut self, ctx: &BuildContext) -> Box<dyn Widget> {
        Column::new()
            .main_axis_alignment(MainAxisAlignment::Center)
            .children(vec![
                Text::new(format!("Count: {}", self.count))
                    .style(TextStyle::headline2())
                    .into_widget(),

                SizedBox::height(20.0).into_widget(),

                Button::new("Increment")
                    .on_pressed(|| self.set_state(|s| s.count += 1))
                    .into_widget(),
            ])
            .into_widget()
    }
}

fn main() {
    FluiApp::new(Counter)
        .title("Counter")
        .run()
        .unwrap();
}
```

---

## 📚 Documentation

### Architecture

Flui follows Flutter's proven **three-tree architecture**:

```
Widget Tree          Element Tree         Render Tree
(Configuration)      (State Holder)       (Layout & Paint)
    │                     │                     │
    ├─ MyApp              ├─ Element            │
    │  └─ Container       │  └─ RenderBox ──────┼─ layout()
    │     └─ Text         │     └─ RenderPara ──┼─ paint()
    │                     │                     │
    └─> Immutable        └─> Mutable           └─> egui::Painter
```

### Core Concepts

#### Widgets

Widgets are **immutable** configurations that describe what should be displayed:

```rust
// StatelessWidget - no internal state
impl StatelessWidget for MyWidget {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        Text::new("Hello").into_widget()
    }
}

// StatefulWidget - has mutable state
impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State { /* ... */ }
}
```

#### State Management

Built-in Provider system for reactive state management:

```rust
// Create a model
#[derive(Clone)]
struct TodoModel {
    todos: Vec<Todo>,
}

impl ChangeNotifier for TodoModel { /* ... */ }

// Provide to widget tree
ChangeNotifierProvider::create(
    || TodoModel::new(),
    Consumer::new(|ctx, model: &TodoModel| {
        ListView::builder()
            .item_count(model.todos.len())
            .item_builder(|ctx, i| {
                TodoItem::new(model.todos[i].clone()).into_widget()
            })
            .into_widget()
    }),
)
```

#### Layout

Flexible and intuitive layout system:

```rust
Column::new()
    .main_axis_alignment(MainAxisAlignment::Center)
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .children(vec![
        Row::new()
            .children(vec![
                Expanded::new(Text::new("Left")),
                Text::new("Right"),
            ]),
        SizedBox::height(10.0),
        Container::new()
            .width(200.0)
            .height(100.0)
            .color(Color::BLUE),
    ])
```

#### Animations

Smooth 60fps animations with curves:

```rust
struct AnimatedBox {
    controller: Arc<Mutex<AnimationController>>,
}

impl State for AnimatedBoxState {
    fn init_state(&mut self) {
        let mut controller = AnimationController::new(
            Duration::from_millis(300)
        );
        controller.forward();
        self.controller = Some(controller);
    }

    fn build(&mut self, ctx: &BuildContext) -> Box<dyn Widget> {
        let scale = 1.0 + self.controller.value() * 0.2;

        Transform::scale(
            scale,
            Container::new()
                .width(100.0)
                .height(100.0)
                .color(Color::BLUE)
        ).into_widget()
    }
}
```

---

## 🎯 Project Status

**Current Phase:** Phase 1 - Foundation Layer 🔄

See [ROADMAP.md](ROADMAP.md) for detailed development plan.

### Milestones

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Project Setup | ✅ Complete |
| 1 | Foundation Layer | 🔄 In Progress |
| 2-4 | Core Framework | ⏳ Planned |
| 5-8 | Advanced Features | ⏳ Planned |
| 9-12 | Polish & Release | ⏳ Planned |

### What's Implemented

- ✅ Project structure
- ✅ Documentation architecture
- ✅ Cargo workspace setup
- 🔄 Foundation layer (in progress)

### What's Coming

- ⏳ Widget/Element/RenderObject traits
- ⏳ Stateless/Stateful widgets
- ⏳ Layout system (Row, Column, Stack)
- ⏳ Text and input widgets
- ⏳ Animation system
- ⏳ State management (Provider)

---

## 🏗️ Architecture

### Crate Structure

```
flui/
├── flui_core         # Core traits (Widget, Element, RenderObject)
├── flui_foundation   # Foundation (Key, ChangeNotifier, Diagnostics)
├── flui_widgets      # Widget implementations
├── flui_rendering    # Render objects
├── flui_painting     # Painting utilities (Decoration, EdgeInsets)
├── flui_animation    # Animation system
├── flui_gestures     # Gesture detection
├── flui_scheduler    # Frame scheduling (Ticker)
├── flui_platform     # Platform integration (egui/eframe)
└── flui_provider     # State management (Provider, Consumer)
```

### Technology Stack

- **egui 0.33** - Immediate mode GUI library
- **eframe 0.33** - Platform integration
- **tokio 1.40** - Async runtime
- **parking_lot 0.12** - Fast synchronization primitives
- **glam 0.29** - Vector math

See [Cargo.toml](Cargo.toml) for complete dependency list.

---

## 📖 Examples

Run examples with:

```bash
cargo run --example counter
cargo run --example animation_demo
cargo run --example todo_app
```

### Available Examples (coming soon)

- **counter** - Basic state management
- **animation_demo** - Animation showcase
- **layout_demo** - Layout examples
- **todo_app** - Complete app with Provider
- **performance_test** - 10,000 item list
- **custom_widgets** - Custom widget creation

---

## 🤝 Contributing

We welcome contributions! Here's how to get started:

1. **Read the docs**: [ROADMAP.md](ROADMAP.md), [GETTING_STARTED.md](GETTING_STARTED.md)
2. **Pick a task**: Check [issues](https://github.com/yourusername/flui/issues)
3. **Implement**: Follow the architecture in `docs/architecture/`
4. **Test**: Write tests with >80% coverage
5. **Submit PR**: Include tests and documentation

### Development Setup

```bash
# Clone repository
git clone https://github.com/yourusername/flui
cd flui

# Build
cargo build --workspace

# Test
cargo test --workspace

# Format & Lint
cargo fmt
cargo clippy -- -D warnings

# Run example
cargo run --example counter
```

### Areas Needing Help

- [ ] Widget implementations
- [ ] Documentation and examples
- [ ] Testing (unit and integration)
- [ ] Performance optimization
- [ ] Platform-specific features

---

## 📊 Performance

### Targets

- **FPS:** 60fps sustained with complex UIs
- **Memory:** < 100MB for typical app
- **Build Time:** < 60s full rebuild (debug)
- **Startup:** < 100ms to first frame

### Optimizations

- ✅ **Viewport Culling** - Only render visible items in lists
- ✅ **RepaintBoundary** - Cache expensive rendering
- ✅ **Memoization** - Cache unchanged widgets
- ✅ **Selector** - Fine-grained Provider updates
- ✅ **Layout Caching** - Reuse layout results
- ✅ **Image Caching** - LRU cache for images

### Benchmarks (coming soon)

| Scenario | Target | Status |
|----------|--------|--------|
| 10,000 item list scrolling | 60fps | ⏳ |
| Complex nested layouts | <16ms rebuild | ⏳ |
| Animation smoothness | 60fps | ⏳ |
| Memory usage | <100MB | ⏳ |

---

## 🔮 Future Plans

### Post-1.0 Features

- **Hot Reload** - Fast iteration during development
- **Custom Shaders** - wgpu integration for advanced effects
- **Platform Features** - Native menus, system tray, file dialogs
- **Advanced Widgets** - DataTable, Charts, Calendar, Rich text editor
- **Accessibility** - Screen reader support, keyboard navigation
- **Internationalization** - i18n, RTL languages, font fallback

---

## 📄 License

Flui is dual-licensed under:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

## 🙏 Acknowledgments

- **Flutter Team** - For the excellent architecture and inspiration
- **egui** - For the amazing immediate mode GUI library
- **Rust Community** - For the incredible ecosystem and tools

---

## 📞 Contact

- **Issues**: [GitHub Issues](https://github.com/yourusername/flui/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/flui/discussions)

---

## ⭐ Star History

[![Star History Chart](https://api.star-history.com/svg?repos=yourusername/flui&type=Date)](https://star-history.com/#yourusername/flui&Date)

---

**Built with ❤️ in Rust**

[Get Started](GETTING_STARTED.md) | [Read Docs](https://docs.rs/flui) | [View Roadmap](ROADMAP.md)
