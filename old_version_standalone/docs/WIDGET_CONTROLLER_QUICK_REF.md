# Widget vs Controller - Quick Reference

## At a Glance

| Aspect | Widget | Stateful Widget | Controller |
|--------|--------|-----------------|------------|
| **Semantics** | `self` (move) | `self` (move) | `&mut self` (borrow) |
| **Lifetime** | One frame | One frame | Multiple frames |
| **State** | None | egui Memory | App struct |
| **Purpose** | Declarative UI | UI + Persistent State | Business Logic |
| **Builder** | ✅ bon | ✅ bon | ❌ Traditional |
| **Trait** | `WidgetExt` | `WidgetWithState` | `Controller` |
| **Forms** | Struct, Closure | Struct only | Struct only |
| **Example** | Container, Text | Area, Window | AnimationController |

---

## Widget Forms

Widgets can take multiple forms:

1. **Struct Widgets** - Full-featured widgets with bon builders (e.g., Container)
2. **Stateful Widgets** - Widgets with persistent state in egui Memory (e.g., Area, Window)
3. **Closure Widgets** - Lightweight widgets via `FnOnce(&mut Ui) -> Response`
4. **Function Widgets** - Functions returning `impl Widget` (closures)

---

## Decision Tree

```
Need to create something?
│
├─ Manages state across frames?
│  │
│  ├─ Complex business logic? → Controller
│  │  └─ Use &mut self, live in app struct
│  │
│  └─ Simple UI state (position, collapse)? → Stateful Widget
│     └─ Use WidgetWithState, store in egui Memory
│
└─ Just renders UI?
   │
   ├─ Complex configuration? → Struct Widget
   │  └─ Use bon builder, key field, validation
   │
   └─ Simple, ad-hoc? → Closure Widget
      └─ Use inline closure or function → impl Widget
```

---

## Widget Template

```rust
use bon::Builder;

#[derive(Builder)]
#[builder(
    on(Color, into),
    on(EdgeInsets, into),
    finish_fn(vis = "", name = build_internal)
)]
pub struct MyWidget {
    /// ALWAYS first field - for state persistence
    #[builder(into)]
    pub key: Option<egui::Id>,

    pub width: Option<f32>,

    #[builder(into)]
    pub color: Option<Color>,

    #[builder(default = EdgeInsets::ZERO, into)]
    pub padding: EdgeInsets,

    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn FnOnce(&mut egui::Ui) -> Response>>,
}

impl MyWidget {
    pub fn new() -> Self { /* ... */ }
    pub fn validate(&self) -> Result<(), String> { /* ... */ }
}

// bon smart setters
impl<S: State> MyWidgetBuilder<S> {
    pub fn child<F>(self, child: F) -> MyWidgetBuilder<SetChild<S>>
    where
        S::Child: IsUnset,
        F: FnOnce(&mut egui::Ui) -> Response + 'static,
    {
        self.child_internal(Box::new(child))
    }
}

// bon finishing functions
impl<S: IsComplete> MyWidgetBuilder<S> {
    pub fn ui(self, ui: &mut egui::Ui) -> Response {
        egui::Widget::ui(self.build_internal(), ui)
    }

    pub fn build(self, ui: &mut egui::Ui) -> Result<Response, String> {
        let widget = self.build_internal();
        widget.validate()?;
        Ok(egui::Widget::ui(widget, ui))
    }
}

// egui::Widget with key handling
impl egui::Widget for MyWidget {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        if let Some(key) = self.key {
            ui.push_id(key, |ui| self.render(ui)).inner
        } else {
            self.render(ui)
        }
    }
}

impl MyWidget {
    fn render(self, ui: &mut egui::Ui) -> Response {
        // Actual rendering logic
    }
}

// WidgetExt
impl WidgetExt for MyWidget {
    fn id(&self) -> Option<egui::Id> { self.key }
    fn validate(&self) -> Result<(), String> { MyWidget::validate(self) }
    fn debug_name(&self) -> &'static str { "MyWidget" }
    fn size_hint(&self, ui: &egui::Ui) -> Option<Vec2> { /* ... */ }
}
```

---

## Controller Template

```rust
pub struct MyController {
    pub value: f32,
    pub target: f32,
    pub is_active: bool,
}

impl MyController {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            target: 0.0,
            is_active: false,
        }
    }

    pub fn start(&mut self) {
        self.is_active = true;
        self.target = 1.0;
    }

    pub fn stop(&mut self) {
        self.is_active = false;
    }
}

impl Controller for MyController {
    fn update(&mut self, ctx: &egui::Context) {
        if !self.is_active {
            return;
        }

        // Update state
        self.value += (self.target - self.value) * 0.1;

        // Request repaint if still active
        if self.is_active() {
            ctx.request_repaint();
        }
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.is_active = false;
    }

    fn debug_name(&self) -> &'static str {
        "MyController"
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}
```

---

## Usage Examples

### Widget Usage

```rust
// ✅ bon builder (recommended)
Container::builder()
    .key("my_container")       // Optional ID for state
    .width(300.0)
    .color(Color::BLUE)
    .padding(EdgeInsets::all(20.0))
    .child(|ui| {
        ui.label("Hello!")
    })
    .ui(ui);

// ✅ Struct literal (Flutter-like)
Container {
    key: Some("my_container".into()),
    width: Some(300.0),
    color: Some(Color::BLUE),
    padding: EdgeInsets::all(20.0),
    ..Default::default()
}.ui(ui);

// ✅ Closure widget (lightweight, no struct needed)
ui.add(|ui: &mut egui::Ui| {
    ui.horizontal(|ui| {
        ui.label("Quick widget:");
        ui.button("Click me")
    })
    .response
});

// ✅ Function returning impl Widget
fn custom_slider(value: &mut f32) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            ui.label("Value:");
            ui.add(egui::Slider::new(value, 0.0..=1.0));
        })
        .response
    }
}

// Use it
ui.add(custom_slider(&mut my_value));
```

### Controller Usage

```rust
// In app state
struct MyApp {
    animation: AnimationController,
}

// In update function
impl MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update controller
        self.animation.update(ctx);

        // Use controller state in UI
        egui::CentralPanel::default().show(ctx, |ui| {
            let opacity = self.animation.value as f32;

            Container::builder()
                .color(Color::from_rgba(255, 0, 0, (opacity * 255.0) as u8))
                .ui(ui);
        });
    }
}
```

---

## Key Rules

### ✅ Widgets - DO:
- Include `key: Option<egui::Id>` as **first field**
- Use `#[derive(Builder)]` from bon
- Use `#[builder(into)]` for convertible types
- Implement `egui::Widget` with key handling
- Implement `WidgetExt` trait
- Make fields public
- Use `self` (move semantics)

### ❌ Widgets - DON'T:
- Use `&mut self` methods
- Store mutable state
- Implement `Controller` trait
- Forget key field handling

### ✅ Controllers - DO:
- Use `&mut self` for all mutations
- Implement `Controller` trait
- Call `ctx.request_repaint()` when active
- Provide start/stop/reset methods
- Store in app state (lives across frames)

### ❌ Controllers - DON'T:
- Use bon builders
- Implement `egui::Widget`
- Use move semantics (`self`)
- Forget to stop repainting

---

## Decision Tree

```
Need to create UI component?
│
├─ Manages state across frames? → Controller
│  └─ Use &mut self, implement Controller trait
│
└─ Renders UI elements? → Widget
   └─ Use self (move), implement egui::Widget + WidgetExt
```

---

## Common Mistakes

❌ **Using bon builder for Controllers**
```rust
// WRONG - Controllers don't use bon
#[derive(Builder)]  // ❌
pub struct MyController { ... }
```

✅ **Use traditional builder or no builder**
```rust
// RIGHT
pub struct MyController { ... }

impl MyController {
    pub fn new() -> Self { ... }
}
```

---

❌ **Forgetting key field in Widget**
```rust
// WRONG - Missing key field
pub struct MyWidget {
    pub width: f32,  // ❌ key should be first
}
```

✅ **Always include key as first field**
```rust
// RIGHT
pub struct MyWidget {
    #[builder(into)]
    pub key: Option<egui::Id>,  // ✅ Always first
    pub width: f32,
}
```

---

❌ **Storing mutable state in Widget**
```rust
// WRONG - Widgets are recreated each frame
pub struct MyWidget {
    pub counter: &mut i32,  // ❌ Will be lost next frame
}
```

✅ **Use Controller for persistent state**
```rust
// RIGHT - Controller persists across frames
pub struct CounterController {
    pub counter: i32,  // ✅ Lives in app state
}
```

---

## See Also

- [WRITING_WIDGETS_AND_CONTROLLERS.md](./WRITING_WIDGETS_AND_CONTROLLERS.md) - Full guide
- [STATEFUL_WIDGETS.md](./STATEFUL_WIDGETS.md) - **NEW:** Stateful widgets with WidgetWithState
- [WIDGET_VS_CONTROLLER.md](./WIDGET_VS_CONTROLLER.md) - Detailed comparison
- [Container](../src/widgets/primitives/container.rs) - Widget example
- [AnimationController](../src/controllers/animation.rs) - Controller example
