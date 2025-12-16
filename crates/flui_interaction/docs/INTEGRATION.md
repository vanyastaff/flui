# Integration Guide: flui_interaction with Other Crates

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                  flui_app                           │
│  ┌──────────────────────────────────────────────┐  │
│  │  GestureBinding                              │  │
│  │  ├─ EventRouter (flui_interaction)           │  │
│  │  └─ Converts platform events                 │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────────────┐
│             flui_interaction                        │
│  ┌──────────────────────────────────────────────┐  │
│  │  EventRouter → Hit Testing → Dispatch        │  │
│  │  FocusManager → Keyboard focus               │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────────────┐
│              flui_gestures                          │
│  ┌──────────────────────────────────────────────┐  │
│  │  TapGestureRecognizer                        │  │
│  │  DragGestureRecognizer                       │  │
│  │  GestureDetector widget                      │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────────────┐
│              User Application                       │
│  Signal::update() → Widget rebuild                  │
└─────────────────────────────────────────────────────┘
```

## Integration 1: flui_app → flui_interaction

### GestureBinding (flui_app)

```rust
// crates/flui_app/src/binding/gesture.rs

use flui_interaction::{EventRouter, HitTestable};
use flui_types::events::{Event, PointerEvent, KeyEvent};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct GestureBinding {
    event_router: Arc<RwLock<EventRouter>>,
}

impl GestureBinding {
    pub fn new() -> Self {
        Self {
            event_router: Arc::new(RwLock::new(EventRouter::new())),
        }
    }

    /// Handle pointer event from platform (winit)
    pub fn handle_pointer_event(&self, event: PointerEvent, root: &mut dyn HitTestable) {
        let mut router = self.event_router.write();
        router.route_event(root, &Event::Pointer(event));
    }

    /// Handle keyboard event from platform
    pub fn handle_key_event(&self, event: KeyEvent, root: &mut dyn HitTestable) {
        let mut router = self.event_router.write();
        router.route_event(root, &Event::Key(event));
    }

    pub fn event_router(&self) -> Arc<RwLock<EventRouter>> {
        self.event_router.clone()
    }
}
```

### WgpuEmbedder (flui_app)

```rust
// crates/flui_app/src/embedder/wgpu.rs

use flui_interaction::input::{pointer_down, pointer_up, pointer_move};
use winit::event::{WindowEvent, MouseButton};

impl WgpuEmbedder {
    fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let event = pointer_move(
                    Offset::new(position.x, position.y),
                    PointerDeviceKind::Mouse,
                );
                
                // Get root layer from element tree
                let root_layer = self.get_root_layer();
                self.binding.gesture().handle_pointer_event(event, root_layer);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let position = self.last_cursor_position;
                let event = match state {
                    ElementState::Pressed => pointer_down(position, PointerDeviceKind::Mouse),
                    ElementState::Released => pointer_up(position, PointerDeviceKind::Mouse),
                };

                let root_layer = self.get_root_layer();
                self.binding.gesture().handle_pointer_event(event, root_layer);
            }

            WindowEvent::KeyboardInput { input, .. } => {
                let key_event = convert_winit_key_event(input);
                let root_layer = self.get_root_layer();
                self.binding.gesture().handle_key_event(key_event, root_layer);
            }

            _ => {}
        }
    }
}
```

## Integration 2: flui_rendering → flui_interaction

### Layer Implementation

Layers need to implement `HitTestable`:

```rust
// crates/flui_engine/src/layer/canvas_layer.rs (or similar)

use flui_interaction::{HitTestable, HitTestResult, HitTestEntry};

pub struct CanvasLayer {
    bounds: Rect,
    offset: Offset,
    event_handler: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
    children: Vec<Box<dyn Layer>>,
}

impl HitTestable for CanvasLayer {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Transform position to local coordinates
        let local_pos = position - self.offset;

        // Check bounds
        if !self.bounds.contains(local_pos) {
            return false;
        }

        // Hit test children (back to front)
        let mut hit = false;
        for child in self.children.iter().rev() {
            if child.hit_test(local_pos, result) {
                hit = true;
            }
        }

        // Add our own entry if we have a handler
        if let Some(handler) = &self.event_handler {
            let entry = HitTestEntry::with_handler(
                local_pos,
                self.bounds,
                handler.clone(),
            );
            result.add(entry);
            hit = true;
        }

        hit
    }
}
```

### RenderPointerListener

```rust
// crates/flui_rendering/src/objects/interaction/pointer_listener.rs

use flui_interaction::{HitTestable, HitTestResult, HitTestEntry};

pub struct RenderPointerListener {
    bounds: Rect,
    on_pointer_down: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
    on_pointer_up: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
    on_pointer_move: Option<Arc<dyn Fn(&PointerEvent) + Send + Sync>>,
}

// When creating layer:
impl RenderPointerListener {
    pub fn create_layer(&self) -> Box<dyn Layer> {
        let callbacks = self.callbacks.clone();
        
        // Create handler that dispatches to appropriate callback
        let handler = Arc::new(move |event: &PointerEvent| {
            match event {
                PointerEvent::Down(_) => {
                    if let Some(cb) = &callbacks.on_pointer_down {
                        cb(event);
                    }
                }
                PointerEvent::Up(_) => {
                    if let Some(cb) = &callbacks.on_pointer_up {
                        cb(event);
                    }
                }
                PointerEvent::Move(_) => {
                    if let Some(cb) = &callbacks.on_pointer_move {
                        cb(event);
                    }
                }
                _ => {}
            }
        });

        Box::new(PointerListenerLayer {
            bounds: self.bounds,
            handler,
        })
    }
}
```

## Integration 3: flui_gestures → flui_interaction

### GestureDetector Widget

```rust
// crates/flui_gestures/src/detector.rs

use flui_interaction::input::pointer_down;
use flui_rendering::RenderPointerListener;

pub struct GestureDetector {
    on_tap: Option<Arc<dyn Fn() + Send + Sync>>,
    on_pan_update: Option<Arc<dyn Fn(DragUpdateDetails) + Send + Sync>>,
    child: Box<dyn Widget>,
}

impl View for GestureDetector {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create TapGestureRecognizer
        let recognizer = TapGestureRecognizer::new(GestureArena::new());
        if let Some(on_tap) = self.on_tap {
            recognizer.set_on_tap(on_tap);
        }

        // Create RenderPointerListener with recognizer handlers
        let render = RenderPointerListener::new()
            .on_pointer_down(Arc::new(move |event| {
                recognizer.handle_event(event);
            }))
            .on_pointer_up(Arc::new(move |event| {
                recognizer.handle_event(event);
            }));

        // Return tuple: (render, child)
        (render, self.child)
    }
}
```

## Integration 4: Focus Management

### FocusNode (flui_widgets)

```rust
// crates/flui_widgets/src/focus/focus_node.rs

use flui_interaction::{FocusManager, FocusNodeId};
use flui_types::events::{KeyEvent, KeyEventResult};

pub struct FocusNode {
    id: FocusNodeId,
    on_key: Option<Arc<dyn Fn(&KeyEvent) -> KeyEventResult + Send + Sync>>,
}

impl FocusNode {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: FocusNodeId::new(NEXT_ID.fetch_add(1, Ordering::SeqCst)),
            on_key: None,
        }
    }

    pub fn request_focus(&self) {
        FocusManager::global().request_focus(self.id);
    }

    pub fn has_focus(&self) -> bool {
        FocusManager::global().has_focus(self.id)
    }
}
```

### Focus Widget

```rust
// crates/flui_widgets/src/interaction/focus.rs

pub struct Focus {
    focus_node: FocusNode,
    autofocus: bool,
    child: Box<dyn Widget>,
}

impl View for Focus {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        if self.autofocus {
            self.focus_node.request_focus();
        }

        // Create RenderFocus that registers with EventRouter
        let render = RenderFocus::new(self.focus_node.id());

        (render, self.child)
    }
}
```

## Complete Event Flow Example

### 1. User clicks button

```
winit: WindowEvent::MouseInput { state: Pressed, button: Left, position: (100, 200) }
```

### 2. WgpuEmbedder converts to PointerEvent

```rust
let event = pointer_down(Offset::new(100.0, 200.0), PointerDeviceKind::Mouse);
```

### 3. GestureBinding routes event

```rust
self.binding.gesture().handle_pointer_event(event, root_layer);
```

### 4. EventRouter performs hit testing

```rust
let mut result = HitTestResult::new();
root_layer.hit_test(Offset::new(100.0, 200.0), &mut result);
// Result contains: [ButtonLayer, ContainerLayer, RootLayer]
```

### 5. HitTestResult dispatches to handlers

```rust
result.dispatch(&event);
// Calls handlers in order (front to back)
```

### 6. GestureRecognizer analyzes event

```rust
// In TapGestureRecognizer handler:
recognizer.handle_event(&event);
// State machine: Idle → Down
```

### 7. User releases (PointerUp)

```rust
// State machine: Down → Up
// Checks: movement < 18px? time < 300ms? → YES
recognizer.on_tap.call();
```

### 8. User callback fires

```rust
// In GestureDetector:
.on_tap(move || {
    count.update(|n| n + 1);  // Signal update
})
```

### 9. Framework rebuilds

```rust
// Signal tracks dependencies
// RebuildQueue schedules affected widgets
// Next frame: only button rebuilds
```

## Dependency Graph

```
flui_types (base types)
    ↓
flui_interaction (events, hit test, focus)
    ↓
flui_gestures (gesture recognizers)
    ↓
flui_widgets (UI components)
    ↓
flui_app (app framework)
```

**No circular dependencies!** ✅

## Summary

- ✅ **flui_interaction** provides core event infrastructure
- ✅ **flui_app** integrates with platform (winit, Win32, etc.)
- ✅ **flui_rendering** implements HitTestable on Layers
- ✅ **flui_gestures** uses handlers for gesture recognition
- ✅ **flui_widgets** uses FocusManager for keyboard focus

All crates work together through clean interfaces! 🚀