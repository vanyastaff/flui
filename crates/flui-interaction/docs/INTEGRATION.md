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

## Integration 2: flui_rendering/flui_objects → flui_interaction

Hit-test storage is data-only. A render object must not store executable
pointer callbacks in its `HitTestEntry`; it stores only the owner-local
`PointerTarget` identity. The executable handler is registered through
`RenderObjectContext` while the corresponding `RenderView` is created,
updated, or unmounted.

```rust
use flui_interaction::PointerTarget;
use flui_rendering::hit_testing::{HitTestBehavior, HitTestEntry};
use flui_view::RenderObjectContext;

pub struct RenderPointerListener {
    target: Option<PointerTarget>,
    behavior: HitTestBehavior,
}

impl RenderPointerListener {
    pub fn new(target: Option<PointerTarget>, behavior: HitTestBehavior) -> Self {
        Self { target, behavior }
    }

    pub fn set_target(&mut self, target: Option<PointerTarget>) {
        self.target = target;
    }

    pub fn hit_entry(&self, render_id: flui_foundation::RenderId) -> HitTestEntry {
        let mut entry = HitTestEntry::new(render_id);
        if let Some(target) = self.target {
            entry = entry.pointer_target(target);
        }
        entry
    }
}
```

The widget side owns callback composition and lane registration:

```rust
use flui_interaction::{PointerEvent, PointerTarget};
use flui_view::RenderObjectContext;

pub struct ListenerState {
    target: Option<PointerTarget>,
}

impl ListenerState {
    pub fn create_render_object(
        &mut self,
        ctx: &RenderObjectContext<'_>,
    ) -> RenderPointerListener {
        let target = ctx
            .register_pointer(|event: &PointerEvent| {
                // Dispatch to on_pointer_down / on_pointer_up / ...
            })
            .ok();
        self.target = target;
        RenderPointerListener::new(target, HitTestBehavior::DeferToChild)
    }

    pub fn update_render_object(
        &mut self,
        ctx: &RenderObjectContext<'_>,
        render: &mut RenderPointerListener,
    ) {
        match self.target {
            Some(target) => {
                let _ = ctx.replace_pointer(target, |event: &PointerEvent| {
                    // Dispatch to the updated callback set.
                });
                render.set_target(Some(target));
            }
            None => {
                self.target = ctx.register_pointer(|event: &PointerEvent| {}).ok();
                render.set_target(self.target);
            }
        }
    }

    pub fn unmount(&mut self, ctx: &RenderObjectContext<'_>) {
        if let Some(target) = self.target.take() {
            let _ = ctx.unregister_pointer(target);
        }
    }
}
```

Dispatch resolves each `PointerTarget` through the active owner lane and then
invokes every target in leaf-first order with its locally transformed
`PointerEvent`. Ordinary pointer delivery has no `EventPropagation::Stop`;
scroll/pointer-signal arbitration is the separate claiming path.

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

### Presentation-owned focus tree

```rust
use flui_interaction::{FocusManager, FocusNode};

let manager = FocusManager::new();
let node = FocusNode::new();
let attachment = manager
    .root_scope()
    .attach_node(&node)
    .expect("a fresh node must attach to its presentation root");

node.request_focus();
assert!(node.has_primary_focus());

// The attachment is the generation-checked lifecycle authority.
attachment.detach();
```

`FocusManager` is presentation-local: there is no global fallback. Production
embedders keep it in their `UiRealm`/binding owner and mount exactly one
`flui_widgets::FocusRoot` around the element-tree root. `FocusRoot` publishes
that manager's root scope and installs the standard Tab/Shift+Tab actions.

Inside that root, `flui_widgets::Focus` owns or hosts a `FocusNode`, attaches
it below the nearest focus provider during lifecycle initialization, reparents
it when dependencies change, and detaches it on disposal. External nodes have
two explicit policies: the regular `focus_node` path applies widget-provided
overrides, while `Focus::with_external_node` leaves node attributes entirely
caller-owned.

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
