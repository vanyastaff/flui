# flui_interaction

Event routing, hit testing, focus management, and gesture recognition for FLUI.

## Core Concepts

### Event Flow

```
Platform (winit, etc.)
    ↓
PointerEvent / KeyboardEvent
    ↓
EventRouter
    ├─ HitTestResult (spatial dispatch)
    └─ FocusManager (keyboard routing)
        ↓
GestureRecognizers
    ├─ GestureArena (conflict resolution)
    └─ TapRecognizer, DragRecognizer, etc.
        ↓
User callbacks
```

### Hit Testing

Determines which UI elements are under a point. Follows Flutter's pattern with full transform support.

```rust
use flui_interaction::prelude::*;

impl HitTestable for MyWidget {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.bounds.contains(position) {
            return false;
        }
        
        // Push transform for children
        result.push_offset(self.offset);
        for child in &self.children {
            child.hit_test(position, result);
        }
        result.pop_transform();
        
        // Add self
        result.add(HitTestEntry::new(self.id, position, self.bounds));
        true
    }
    
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }
}
```

### Focus Management

Keyboard focus with scopes and traversal policies.

```rust
use flui_interaction::{FocusManager, FocusNodeId};

let node_id = FocusNodeId::new(42);

// Request focus
FocusManager::global().request_focus(node_id);

// Check focus
if FocusManager::global().has_focus(node_id) {
    // Handle keyboard input
}

// Tab traversal
FocusManager::global().next_focus();  // Tab
FocusManager::global().previous_focus();  // Shift+Tab
```

### Gesture Recognition

High-level gesture detection with arena-based conflict resolution.

```rust
use flui_interaction::prelude::*;

// Create recognizer
let tap = TapGestureRecognizer::new();
tap.on_tap(|| println!("Tapped!"));

// Handle events
tap.add_pointer(pointer_id, event);
```

---

## Event Types

Uses W3C-compliant event types from `ui-events`:

### PointerEvent

```rust
use flui_interaction::PointerEvent;

match event {
    PointerEvent::Down(data) => {
        let pos = data.position;
        let pointer_id = data.pointer_id;
        let pointer_type = data.pointer_type;  // Mouse, Touch, Pen
    }
    PointerEvent::Move(data) => { /* ... */ }
    PointerEvent::Up(data) => { /* ... */ }
    PointerEvent::Cancel(data) => { /* ... */ }
}
```

### KeyboardEvent

```rust
use flui_interaction::KeyboardEvent;

match event {
    KeyboardEvent::KeyDown(data) => {
        let key = &data.key;
        let code = &data.code;
        let modifiers = &data.modifiers;
    }
    KeyboardEvent::KeyUp(data) => { /* ... */ }
}
```

---

## Gesture Recognizers

### TapGestureRecognizer

Single tap detection.

```rust
let tap = TapGestureRecognizer::new();
tap.on_tap_down(|details| { /* pointer down */ });
tap.on_tap_up(|details| { /* pointer up, tap confirmed */ });
tap.on_tap(|| { /* complete tap */ });
tap.on_tap_cancel(|| { /* tap cancelled */ });
```

### DoubleTapGestureRecognizer

Two taps in quick succession.

```rust
let double_tap = DoubleTapGestureRecognizer::new();
double_tap.on_double_tap(|| println!("Double tapped!"));
double_tap.on_double_tap_down(|details| { /* first tap */ });
```

### LongPressGestureRecognizer

Press and hold.

```rust
let long_press = LongPressGestureRecognizer::new();
long_press.on_long_press_start(|details| { /* hold started */ });
long_press.on_long_press_move_update(|details| { /* moved while holding */ });
long_press.on_long_press_end(|details| { /* released */ });
```

### DragGestureRecognizer

Pan/drag gestures.

```rust
let drag = DragGestureRecognizer::new();
drag.on_drag_start(|details| { /* drag started */ });
drag.on_drag_update(|details| {
    let delta = details.delta;
    let velocity = details.velocity;
});
drag.on_drag_end(|details| { /* drag ended */ });
```

### ScaleGestureRecognizer

Pinch-to-zoom and rotation.

```rust
let scale = ScaleGestureRecognizer::new();
scale.on_scale_start(|details| { /* scale started */ });
scale.on_scale_update(|details| {
    let scale = details.scale;
    let rotation = details.rotation;
    let focal_point = details.focal_point;
});
scale.on_scale_end(|details| { /* scale ended */ });
```

### ForcePressGestureRecognizer

Pressure-sensitive input (3D Touch, Force Touch).

```rust
let force = ForcePressGestureRecognizer::new();
force.on_force_press_start(|details| { /* force threshold reached */ });
force.on_force_press_peak(|details| { /* max pressure */ });
force.on_force_press_update(|details| { /* pressure changed */ });
force.on_force_press_end(|details| { /* released */ });
```

---

## Gesture Arena

Resolves conflicts when multiple recognizers compete for the same pointer.

```rust
use flui_interaction::{GestureArena, GestureDisposition};

let arena = GestureArena::new();

// Recognizers join arena
arena.add(pointer_id, tap_recognizer);
arena.add(pointer_id, drag_recognizer);

// Arena resolves winner based on:
// 1. First to accept wins
// 2. Last remaining after others reject
// 3. Timeout forces resolution
```

### Disambiguation

- **Tap vs Drag**: Drag wins if movement > slop threshold
- **Tap vs Long Press**: Long press wins after timeout
- **Tap vs Double Tap**: Waits for possible second tap

---

## Hit Test Behaviors

| Behavior | Hit Self | Block Events Below |
|----------|----------|-------------------|
| `Opaque` | Always | Yes |
| `Translucent` | Always | No |
| `DeferToChild` | Only if child hit | Only if child hit |

---

## Input Processing

### VelocityTracker

Estimates pointer velocity for fling gestures.

```rust
use flui_interaction::VelocityTracker;

let mut tracker = VelocityTracker::new();
tracker.add_position(timestamp, position);
// ... more positions ...

let velocity = tracker.velocity();
// velocity.x, velocity.y in logical pixels per second
```

### PointerEventResampler

Synchronizes pointer events with frame timing.

```rust
use flui_interaction::PointerEventResampler;

let mut resampler = PointerEventResampler::new();
resampler.add_event(event);

// At frame time
let resampled = resampler.sample(frame_timestamp);
```

### InputPredictor

Predicts future pointer positions to reduce latency.

```rust
use flui_interaction::InputPredictor;

let mut predictor = InputPredictor::new();
predictor.add_sample(timestamp, position);

let predicted = predictor.predict(future_timestamp);
```

---

## Testing Utilities

### GestureRecorder

Record gestures for replay.

```rust
use flui_interaction::testing::{GestureRecorder, GesturePlayer};

// Record
let mut recorder = GestureRecorder::new();
recorder.start();
// ... user performs gesture ...
recorder.stop();
let recording = recorder.recording();

// Replay
let player = GesturePlayer::new(recording);
player.play(&mut hit_testable);
```

### GestureBuilder

Programmatically create gesture sequences.

```rust
use flui_interaction::testing::GestureBuilder;

let tap = GestureBuilder::tap(Offset::new(100.0, 100.0));
let drag = GestureBuilder::drag(
    Offset::new(0.0, 0.0),
    Offset::new(100.0, 0.0),
    Duration::from_millis(200),
);
```

---

## Type-Safe IDs

Newtype pattern prevents mixing ID types:

```rust
use flui_interaction::{PointerId, FocusNodeId, HandlerId};

let pointer = PointerId::new(0);
let focus = FocusNodeId::new(42);

// fn process(id: PointerId) { ... }
// process(focus);  // Compile error - wrong type!
```

---

## Configuration

### GestureSettings

```rust
use flui_interaction::GestureSettings;

let settings = GestureSettings {
    touch_slop: 18.0,           // Movement before drag starts
    pan_slop: 36.0,             // Movement for pan gesture
    double_tap_timeout: 300,     // ms between double tap
    long_press_timeout: 500,     // ms to trigger long press
    min_fling_velocity: 50.0,    // Minimum fling velocity
    max_fling_velocity: 8000.0,  // Maximum fling velocity
    ..Default::default()
};
```

---

## Thread Safety

All types are `Send + Sync`:

- `FocusManager` uses `RwLock` for global state
- `GestureArena` uses `Mutex` for entry management
- Recognizers are individually thread-safe

---

## Module Summary

| Module | Description |
|--------|-------------|
| `routing` | Hit testing, event dispatch, focus management |
| `recognizers` | Tap, drag, scale, long press, etc. |
| `arena` | Gesture conflict resolution |
| `processing` | Velocity tracking, resampling, prediction |
| `testing` | Recording, playback, builders |
| `mouse_tracker` | Mouse enter/exit/hover detection |
| `ids` | Type-safe identifiers |

---

## See Also

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — Internal design
- [docs/HIT_TESTING.md](docs/HIT_TESTING.md) — Hit testing guide
- [docs/GESTURES.md](docs/GESTURES.md) — Gesture recognition details
- [docs/INTEGRATION.md](docs/INTEGRATION.md) — Integration with other crates
