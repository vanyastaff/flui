# Widget vs Controller - Architecture Guide

## ⚠️ Главная ошибка

```rust
// ❌ WRONG! Controller это НЕ Widget!
impl Widget for AnimationController {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        // self (move) - контроллер уничтожается после рендеринга!
        // Несовместимо с &mut self методами!
    }
}
```

**Почему это плохо:**
1. `ui(self, ...)` **консумирует** контроллер (move semantics)
2. Controller нужен `&mut self` для методов типа `forward()`, `reverse()`, `update()`
3. **Концептуально неправильно** - Controller это state management, не UI

---

## ✅ Правильная архитектура: Два отдельных трейта

### 1. Widget Trait (Immutable UI)

```rust
/// Widget trait - for immutable UI elements.
///
/// Widgets are created each frame and consumed during rendering.
pub trait Widget: Sized {
    /// Render the widget (consumes self).
    fn ui(self, ui: &mut egui::Ui) -> egui::Response;

    fn id(&self) -> Option<egui::Id> { None }
    fn validate(&self) -> Result<(), String> { Ok(()) }
    fn debug_name(&self) -> &'static str { std::any::type_name::<Self>() }
    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> { None }
}
```

**Характеристики:**
- **Ownership**: `self` (move semantics)
- **Lifetime**: Один фрейм
- **Pattern**: Declarative UI
- **bon?**: ✅ Да
- **Примеры**: Container, Row, Column, Text

**Использование:**
```rust
// Создаётся и консумируется каждый фрейм
Container::builder()
    .width(300.0)
    .color(Color::BLUE)
    .ui(ui);  // ← self moved here, Container уничтожен
```

---

### 2. Controller Trait (Mutable State)

```rust
/// Controller trait - for mutable state that persists across frames.
pub trait Controller {
    /// Update controller state (mutates self).
    fn update(&mut self, ctx: &egui::Context);

    /// Reset to initial state.
    fn reset(&mut self);

    /// Get debug name.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Check if active/animating.
    fn is_active(&self) -> bool { false }
}
```

**Характеристики:**
- **Ownership**: `&mut self` (borrow semantics)
- **Lifetime**: Множество фреймов
- **Pattern**: Imperative state management
- **bon?**: ❌ Нет
- **Примеры**: AnimationController, ScrollController, TabController

**Использование:**
```rust
// Живёт в App state, переиспользуется
struct MyApp {
    animation: AnimationController,  // ← Owned by App
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update controller (borrow)
        self.animation.update(ctx);  // ← &mut self

        // Use value in widgets
        let opacity = self.animation.value();
        Container::builder()
            .color(Color::from_rgba(255, 0, 0, (opacity * 255.0) as u8))
            .ui(ui);
    }
}
```

---

## 📊 Сравнительная таблица

| Аспект | Widget | Controller |
|--------|--------|------------|
| **Трейт метод** | `fn ui(self, ...)` | `fn update(&mut self, ...)` |
| **Ownership** | `self` (move) | `&mut self` (borrow) |
| **Lifetime** | Один фрейм | Множество фреймов |
| **Создаётся** | Каждый фрейм | Один раз |
| **Паттерн** | Declarative | Imperative |
| **bon builder?** | ✅ Да | ❌ Нет |
| **State** | Immutable | Mutable |
| **Цель** | Render UI | Manage state |
| **Примеры** | Container, Row, Column | AnimationController |

---

## 🎯 AnimationController - правильная реализация

### Определение

```rust
use super::Controller;

/// Animation controller for smooth UI transitions.
///
/// This is a Controller, NOT a Widget!
#[derive(Debug, Clone)]
pub struct AnimationController {
    value: f32,
    target: f32,
    duration: Duration,
    curve: AnimationCurve,
    state: AnimationState,
    start_time: Option<Instant>,
    start_value: f32,
}

impl AnimationController {
    pub fn new(duration: Duration) -> Self {
        Self {
            value: 0.0,
            target: 0.0,
            duration,
            curve: AnimationCurve::EaseInOut,
            state: AnimationState::Idle,
            start_time: None,
            start_value: 0.0,
        }
    }

    // Builder methods (опционально)
    pub fn with_curve(mut self, curve: AnimationCurve) -> Self {
        self.curve = curve;
        self
    }

    // ✅ &mut self методы - ПРАВИЛЬНО!
    pub fn forward(&mut self) {
        self.start_value = self.value;
        self.target = 1.0;
        self.state = AnimationState::Forward;
        self.start_time = Some(Instant::now());
    }

    pub fn reverse(&mut self) {
        self.start_value = self.value;
        self.target = 0.0;
        self.state = AnimationState::Reverse;
        self.start_time = Some(Instant::now());
    }

    pub fn tick(&mut self) -> f32 {
        if let Some(start_time) = self.start_time {
            let elapsed = start_time.elapsed().as_secs_f32();
            let progress = (elapsed / self.duration.as_secs_f32()).min(1.0);

            let curved = self.apply_curve(progress);
            self.value = self.start_value + (self.target - self.start_value) * curved;

            if progress >= 1.0 {
                self.value = self.target;
                self.state = AnimationState::Completed;
                self.start_time = None;
            }
        }

        self.value
    }

    // Getters (read-only)
    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn is_animating(&self) -> bool {
        matches!(self.state, AnimationState::Forward | AnimationState::Reverse)
    }
}
```

### Impl Controller (NOT Widget!)

```rust
impl Controller for AnimationController {
    fn update(&mut self, ctx: &egui::Context) {
        // Tick animation
        self.tick();

        // Request repaint if active
        if self.is_active() {
            ctx.request_repaint();
        }
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.state = AnimationState::Idle;
        self.start_time = None;
    }

    fn debug_name(&self) -> &'static str {
        "AnimationController"
    }

    fn is_active(&self) -> bool {
        self.is_animating()
    }
}
```

---

## 💡 Примеры использования

### Container (Widget) - создаётся каждый фрейм

```rust
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    egui::CentralPanel::default().show(ctx, |ui| {
        // Widget создаётся заново каждый фрейм
        Container::builder()
            .width(300.0)
            .color(Color::BLUE)
            .ui(ui);  // ← Консумируется здесь

        // Нельзя использовать снова - уже moved!
        // container.ui(ui);  // ❌ ERROR: value used after move
    });
}
```

### AnimationController (Controller) - переиспользуется

```rust
struct MyApp {
    // Controller живёт в App state
    fade_animation: AnimationController,
}

impl MyApp {
    fn new(_cc: &eframe::CreationContext) -> Self {
        Self {
            // Создаётся ОДИН РАЗ
            fade_animation: AnimationController::new(Duration::from_secs(2))
                .with_curve(AnimationCurve::EaseInOut),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update controller
        self.fade_animation.update(ctx);  // ← &mut borrow

        egui::CentralPanel::default().show(ctx, |ui| {
            // Используем значение из controller в widget
            let opacity = self.fade_animation.value();

            Container::builder()
                .width(300.0)
                .color(Color::from_rgba(100, 150, 255, (opacity * 255.0) as u8))
                .ui(ui);

            // Control buttons
            ui.horizontal(|ui| {
                if ui.button("Fade In").clicked() {
                    self.fade_animation.forward();  // ← &mut borrow
                }
                if ui.button("Fade Out").clicked() {
                    self.fade_animation.reverse();  // ← &mut borrow
                }
                if ui.button("Reset").clicked() {
                    self.fade_animation.reset();  // ← &mut borrow
                }
            });
        });
    }
}
```

---

## 🔄 Полный пример: Widget + Controller

```rust
use eframe::egui;
use nebula_ui::prelude::*;
use nebula_ui::controllers::{AnimationController, Controller};
use std::time::Duration;

fn main() -> eframe::Result {
    eframe::run_native(
        "Widget vs Controller Demo",
        eframe::NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

struct MyApp {
    // Controllers (mutable state, persist across frames)
    fade: AnimationController,
    scale: AnimationController,
    rotate: AnimationController,
}

impl MyApp {
    fn new(_cc: &eframe::CreationContext) -> Self {
        Self {
            fade: AnimationController::new(Duration::from_secs(1))
                .with_curve(AnimationCurve::EaseInOut),

            scale: AnimationController::new(Duration::from_millis(500))
                .with_curve(AnimationCurve::EaseOut),

            rotate: AnimationController::new(Duration::from_secs(2))
                .with_curve(AnimationCurve::Linear),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update all controllers
        self.fade.update(ctx);
        self.scale.update(ctx);
        self.rotate.update(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Widget vs Controller Demo");
            ui.add_space(20.0);

            // Get animated values from controllers
            let opacity = self.fade.value();
            let scale = 0.5 + self.scale.value() * 0.5;  // 0.5 to 1.0
            let rotation = self.rotate.value() * 360.0;  // 0 to 360 degrees

            // Create widget with animated properties
            // Widget is created FRESH each frame with new values
            Container::builder()
                .width(200.0)
                .height(200.0)
                .color(Color::from_rgba(100, 150, 255, (opacity * 255.0) as u8))
                .transform(Transform::scale(scale, scale)
                    .rotate_degrees(rotation))
                .ui(ui);

            ui.add_space(20.0);

            // Control buttons
            ui.horizontal(|ui| {
                if ui.button("Fade").clicked() {
                    if self.fade.value() < 0.5 {
                        self.fade.forward();
                    } else {
                        self.fade.reverse();
                    }
                }

                if ui.button("Scale").clicked() {
                    self.scale.forward();
                }

                if ui.button("Rotate").clicked() {
                    self.rotate.animate_to(1.0);
                }

                if ui.button("Reset All").clicked() {
                    self.fade.reset();
                    self.scale.reset();
                    self.rotate.reset();
                }
            });

            ui.add_space(10.0);
            ui.label(format!("Opacity: {:.2}", opacity));
            ui.label(format!("Scale: {:.2}", scale));
            ui.label(format!("Rotation: {:.0}°", rotation));
        });
    }
}
```

---

## 🎓 Ключевые выводы

### Widget (Declarative UI)
```rust
// ✅ ПРАВИЛЬНО
impl Widget for Container {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        // self consumed - это ОК для виджетов
    }
}

// ✅ Использование
Container::builder()
    .width(300.0)
    .ui(ui);  // Создан и консумирован
```

### Controller (Imperative State)
```rust
// ✅ ПРАВИЛЬНО
impl Controller for AnimationController {
    fn update(&mut self, ctx: &Context) {
        // &mut self - это ОК для контроллеров
    }
}

// ✅ Использование
struct App {
    anim: AnimationController,  // Owned by App
}

anim.update(ctx);  // Borrow, не консумирует
anim.forward();    // Может вызвать многократно
```

### ❌ Не мешайте концепции!
```rust
// ❌ WRONG: Controller как Widget
impl Widget for AnimationController {
    fn ui(self, ...) { }  // Консумирует controller - плохо!
}

// ❌ WRONG: Widget как Controller
impl Controller for Container {
    fn update(&mut self, ...) { }  // Container должен быть immutable!
}
```

---

## 📚 Итого

**Widget** = **Immutable UI** + **move semantics** + **bon builder**
- Создаётся каждый фрейм
- Консумируется при рендеринге
- Declarative API

**Controller** = **Mutable State** + **borrow semantics** + **NO bon**
- Создаётся один раз
- Мутируется через `&mut self`
- Imperative API

**Золотое правило**:
- Если нужен `ui()` метод → Widget trait
- Если нужен `&mut self` → Controller trait
- **НИКОГДА не мешайте их!** ✨
