# Stateful Widgets - Advanced Pattern

## Overview

Stateful widgets are a special category that sits between regular widgets and controllers. They use egui's `WidgetWithState` trait to persist state in egui's Memory across frames while still using move semantics (`self`).

## Widget Categories Comparison

| Aspect | Stateless Widget | Stateful Widget | Controller |
|--------|-----------------|-----------------|------------|
| **Semantics** | `self` (move) | `self` (move) | `&mut self` (borrow) |
| **Recreated** | Every frame | Every frame | Never |
| **State** | None | In egui Memory | In app struct |
| **Storage** | N/A | `ctx.memory()` | Your code |
| **Trait** | `Widget` + `WidgetExt` | `WidgetWithState` | `Controller` |
| **Example** | Container, Text | Area, Window | AnimationController |
| **Use when** | Pure rendering | Persistent UI state | Complex logic |

## How Stateful Widgets Work

### The Pattern

```rust
// 1. Define the state struct (persisted between frames)
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct MyWidgetState {
    pub position: Pos2,
    pub size: Vec2,
    pub is_collapsed: bool,
}

impl Default for MyWidgetState {
    fn default() -> Self {
        Self {
            position: Pos2::ZERO,
            size: Vec2::ZERO,
            is_collapsed: false,
        }
    }
}

impl MyWidgetState {
    /// Load state from egui memory
    pub fn load(ctx: &Context, id: Id) -> Option<Self> {
        ctx.data(|data| data.get_persisted(id))
    }

    /// Save state to egui memory
    pub fn store(self, ctx: &Context, id: Id) {
        ctx.data_mut(|data| data.insert_persisted(id, self));
    }
}

// 2. Define the widget struct (recreated each frame)
#[derive(Clone, Debug)]
pub struct MyWidget {
    pub id: Id,
    pub default_pos: Option<Pos2>,
    pub movable: bool,
}

// 3. Implement WidgetWithState
impl egui::WidgetWithState for MyWidget {
    type State = MyWidgetState;
}

// 4. Implement the widget with state loading/saving
impl MyWidget {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            default_pos: None,
            movable: true,
        }
    }

    pub fn show<R>(
        self,
        ctx: &Context,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        // Load state from memory (or create default)
        let mut state = MyWidgetState::load(ctx, self.id)
            .unwrap_or_default();

        // Use default position on first show
        if state.position == Pos2::ZERO {
            state.position = self.default_pos
                .unwrap_or_else(|| Pos2::new(100.0, 100.0));
        }

        // Create UI using state
        let response = self.render(ctx, &mut state, add_contents);

        // Save state back to memory
        state.store(ctx, self.id);

        response
    }

    fn render<R>(
        self,
        ctx: &Context,
        state: &mut MyWidgetState,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        // Your rendering logic here using state
        // ...
        unimplemented!()
    }
}
```

## Real Example: Collapsible Panel

```rust
use egui::{Context, Id, Ui, Response, InnerResponse};

/// State for a collapsible panel
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct CollapsiblePanelState {
    /// Is the panel currently open?
    pub open: bool,

    /// Cached content height when open
    #[cfg_attr(feature = "serde", serde(skip))]
    pub content_height: f32,
}

impl CollapsiblePanelState {
    pub fn load(ctx: &Context, id: Id) -> Self {
        ctx.data(|data| data.get_persisted(id))
            .unwrap_or_default()
    }

    pub fn store(&self, ctx: &Context, id: Id) {
        ctx.data_mut(|data| data.insert_persisted(id, self.clone()));
    }
}

/// A collapsible panel widget
pub struct CollapsiblePanel {
    id: Id,
    title: String,
    default_open: bool,
}

impl egui::WidgetWithState for CollapsiblePanel {
    type State = CollapsiblePanelState;
}

impl CollapsiblePanel {
    pub fn new(id: impl Into<Id>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            default_open: true,
        }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let Self { id, title, default_open } = self;

        // Load state (or create with default)
        let mut state = CollapsiblePanelState::load(ui.ctx(), id);

        // First time showing? Use default
        if state.content_height == 0.0 {
            state.open = default_open;
        }

        // Render header (clickable)
        let header_response = ui.horizontal(|ui| {
            let icon = if state.open { "▼" } else { "▶" };
            if ui.button(icon).clicked() {
                state.open = !state.open;
            }
            ui.heading(&title);
        }).response;

        // Render content if open
        let inner = if state.open {
            let content_response = ui.vertical(|ui| {
                ui.indent(id, |ui| {
                    add_contents(ui)
                })
            });

            // Cache content height
            state.content_height = content_response.response.rect.height();

            Some(content_response.inner.inner)
        } else {
            None
        };

        // Save state
        state.store(ui.ctx(), id);

        InnerResponse {
            inner: inner.unwrap_or_else(|| add_contents(&mut ui.child_ui(
                ui.max_rect(),
                egui::Layout::top_down(egui::Align::LEFT),
            ))),
            response: header_response,
        }
    }
}

// Usage:
CollapsiblePanel::new("my_panel", "Settings")
    .default_open(false)
    .show(ui, |ui| {
        ui.label("Panel content here");
        ui.slider(&mut value, 0.0..=1.0);
    });
```

## When to Use Stateful Widgets

### ✅ Use Stateful Widgets When:

1. **UI State Persistence**
   - Window position/size
   - Scroll position
   - Collapse/expand state
   - Tab selection

2. **Per-Widget State**
   - Each instance needs its own state
   - State identified by `Id`
   - State should survive app restarts (with serde)

3. **egui-Managed State**
   - You want egui to handle serialization
   - You want automatic state cleanup
   - You want to leverage egui's Id system

### ❌ Don't Use Stateful Widgets When:

1. **No State Needed**
   - Pure rendering based on props → Use regular Widget

2. **Complex Business Logic**
   - Multi-step workflows → Use Controller
   - Animations → Use AnimationController
   - Form validation → Use FormController

3. **App-Level State**
   - Shared across widgets → Store in app struct
   - Needs custom serialization → Use app state + Controller

## Stateful Widget vs Controller

```rust
// ❌ DON'T: Use controller for simple UI state
struct PanelController {
    is_open: bool,  // This is just UI state!
}

impl Controller for PanelController {
    fn update(&mut self, ctx: &Context) {
        // Nothing to update...
    }
}

// ✅ DO: Use stateful widget
#[derive(Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
struct PanelState {
    is_open: bool,
}

struct Panel { id: Id }

impl WidgetWithState for Panel {
    type State = PanelState;
}
```

```rust
// ✅ DO: Use controller for complex logic
struct AnimationController {
    value: f64,
    target: f64,
    curve: Box<dyn Curve>,
    start_time: Option<f64>,
}

impl Controller for AnimationController {
    fn update(&mut self, ctx: &Context) {
        // Complex animation logic
        self.tick();
        if self.is_animating() {
            ctx.request_repaint();
        }
    }
}
```

## Best Practices

### 1. Always Provide Default State

```rust
impl Default for MyWidgetState {
    fn default() -> Self {
        Self {
            // Sensible defaults
            position: Pos2::ZERO,
            open: true,
        }
    }
}
```

### 2. Skip Non-Serializable Fields

```rust
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct AreaState {
    pub position: Pos2,  // ✅ Serializable

    #[cfg_attr(feature = "serde", serde(skip))]
    pub size: Option<Vec2>,  // ❌ Temporary, don't persist

    #[cfg_attr(feature = "serde", serde(skip))]
    pub last_visible_at: Option<f64>,  // ❌ Time-dependent
}
```

### 3. Use Proper Id Namespacing

```rust
// ❌ BAD: Might conflict
let id = Id::new("panel");

// ✅ GOOD: Scoped to your widget type
let id = Id::new("my_app::CollapsiblePanel").with("settings");

// ✅ GOOD: Use Id::new with unique string
CollapsiblePanel::new(Id::new("settings_panel"), "Settings")
```

### 4. Provide Builder Methods

```rust
impl CollapsiblePanel {
    pub fn new(id: impl Into<Id>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            default_open: true,
        }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn show<R>(/* ... */) -> InnerResponse<R> {
        // ...
    }
}
```

## egui Examples

Study these egui stateful widgets:

1. **Area** (`egui::Area`)
   - State: position, size, pivot
   - Persists window position
   - See: `egui/containers/area.rs`

2. **Window** (`egui::Window`)
   - Built on Area
   - State: position, size, collapsed
   - See: `egui/containers/window.rs`

3. **ScrollArea** (`egui::ScrollArea`)
   - State: scroll offset
   - Persists scroll position
   - See: `egui/containers/scroll_area.rs`

4. **CollapsingHeader** (`egui::CollapsingHeader`)
   - State: open/closed
   - Persists collapse state
   - See: `egui/widgets/collapsing_header.rs`

## Summary

**Stateful Widgets** are for UI state that should persist but doesn't need complex logic:
- ✅ Window position/size
- ✅ Scroll offset
- ✅ Collapse state
- ✅ Tab selection

**Controllers** are for complex business logic:
- ✅ Animations
- ✅ Form validation
- ✅ Multi-step workflows
- ✅ Complex state machines

**Regular Widgets** are for pure rendering:
- ✅ Container
- ✅ Text
- ✅ Row/Column
- ✅ Decorative elements

Choose the right tool for the job! 🎯
