# flui_interaction

Event routing and interaction handling for FLUI.

## Overview

This crate provides the event handling infrastructure for FLUI, separate from rendering (`flui_engine`):

- **EventRouter**: Routes pointer/keyboard events via hit testing
- **HitTest**: Determines which UI elements are under cursor/touch
- **FocusManager**: Manages keyboard focus (global singleton)

## Architecture

```
Platform (winit, Win32, etc.)
    ↓
PointerEvent/KeyEvent (flui_types)
    ↓
EventRouter (this crate)
    ├─ Hit Testing (spatial)
    └─ Focus Management (keyboard)
        ↓
Handlers (closures in Layers)
    ↓
GestureRecognizers (flui_gestures)
    ↓
User code (Signal::update, etc.)
```

## Why Separate from flui_engine?

**flui_engine** is for RENDERING (GPU, shaders, paint)
**flui_interaction** is for EVENTS (input, hit test, focus)

Benefits:
- ✅ Test event logic without GPU
- ✅ Use rendering without event handling (headless)
- ✅ Clear separation of concerns (SOLID)
- ✅ Smaller compile times

## Usage

### Basic Event Routing

```rust
use flui_interaction::{EventRouter, HitTestable};
use flui_types::events::{Event, PointerEvent};

let mut router = EventRouter::new();

// Route pointer event
let event = PointerEvent::Down { position: Offset::new(50.0, 50.0), ... };
router.route_event(&mut root_layer, &Event::Pointer(event));
```

### Hit Testing

Implement `HitTestable` on your layer:

```rust
use flui_interaction::{HitTestable, HitTestResult, HitTestEntry};

impl HitTestable for MyLayer {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Check if position is within bounds
        if !self.bounds.contains(position) {
            return false;
        }

        // Add entry with handler
        let entry = HitTestEntry::with_handler(
            position - self.offset,
            self.bounds,
            self.event_handler.clone(),  // Arc<dyn Fn(&PointerEvent)>
        );
        result.add(entry);

        true
    }
}
```

### Keyboard Focus

```rust
use flui_interaction::{FocusManager, FocusNodeId};

let my_text_field = FocusNodeId::new(42);

// Request focus
FocusManager::global().request_focus(my_text_field);

// Check focus
if FocusManager::global().has_focus(my_text_field) {
    println!("We have focus!");
}

// Clear focus
FocusManager::global().unfocus();
```

### Event Helpers

```rust
use flui_interaction::input::{pointer_down, KeyEventBuilder, ModifiersBuilder};

// Create pointer events
let event = pointer_down(Offset::new(100.0, 200.0), PointerDeviceKind::Mouse);

// Create key events
let key_event = KeyEventBuilder::new(PhysicalKey::Enter)
    .ctrl(true)
    .logical_key("Enter")
    .build();

// Create modifiers
let modifiers = ModifiersBuilder::new()
    .ctrl(true)
    .shift(true)
    .build();
```

## Integration with flui_app

In `flui_app`, `GestureBinding` uses `EventRouter`:

```rust
// flui_app/src/binding/gesture.rs

use flui_interaction::EventRouter;

pub struct GestureBinding {
    event_router: Arc<RwLock<EventRouter>>,
}

impl GestureBinding {
    pub fn handle_pointer_event(&self, event: PointerEvent, root: &mut dyn HitTestable) {
        let mut router = self.event_router.write();
        router.route_event(root, &Event::Pointer(event));
    }
}
```

## Event Flow

### Pointer Events (Mouse, Touch)

1. Platform generates event (winit::WindowEvent::CursorMoved)
2. `flui_app` converts to `PointerEvent`
3. `GestureBinding` calls `EventRouter::route_event()`
4. `EventRouter` performs hit testing
5. `HitTestResult` dispatches to handlers
6. Handlers call `TapGestureRecognizer`, etc.
7. Recognizers fire user callbacks
8. User code updates `Signal`
9. Framework rebuilds affected widgets

### Keyboard Events

1. Platform generates event (winit::WindowEvent::KeyboardInput)
2. `flui_app` converts to `KeyEvent`
3. `GestureBinding` calls `EventRouter::route_event()`
4. `EventRouter` gets focused element from `FocusManager`
5. Event dispatched to focused element's handler
6. Handler processes key (e.g., insert character in TextField)

## Testing

```rust
use flui_interaction::{EventRouter, HitTestable, HitTestResult};

#[test]
fn test_hit_testing() {
    let mut router = EventRouter::new();
    let mut layer = MockLayer { bounds: Rect::from_xywh(0.0, 0.0, 100.0, 100.0) };

    let event = pointer_down(Offset::new(50.0, 50.0), PointerDeviceKind::Mouse);
    router.route_event(&mut layer, &Event::Pointer(event));

    // Verify event was routed
}
```

## Performance

- **Hit testing**: O(tree depth), typically 5-20 layers
- **Focus lookup**: O(1), single RwLock read
- **Event dispatch**: O(hit count), typically 1-3 elements

All hot paths use lock-free or minimal locking for 60+ FPS.

## Future Work

- [ ] Focus traversal (Tab navigation)
- [ ] Focus scopes (modal dialogs)
- [ ] Event bubbling control (stopPropagation)
- [ ] Event capture phase
- [ ] Touch gesture disambiguation
- [ ] Accessibility integration

## License

MIT OR Apache-2.0