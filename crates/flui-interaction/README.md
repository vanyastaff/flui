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

        // `with_paint_offset` takes the forward (paint-direction) offset and
        // pushes its inverse internally, popping automatically when the
        // closure returns -- the stack accumulates a GLOBAL-TO-LOCAL
        // mapping as the walk descends, so callers must never push the
        // forward offset directly (that's what the raw `push_offset`/
        // `pop_transform` pair does, and it does NOT invert for you).
        result.with_paint_offset(self.offset, |result| {
            for child in &self.children {
                child.hit_test(position, result);
            }
        });

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
use flui_interaction::{FocusManager, FocusNode};

// A presentation owns one manager. Widgets normally receive this owner
// through FocusRoot; there is no process-global focus manager.
let manager = FocusManager::new();
let node = FocusNode::new();
let _attachment = manager
    .root_scope()
    .attach_node(&node)
    .expect("a fresh node must attach to its presentation root");

// Request focus
node.request_focus();

// Check focus
if node.has_primary_focus() {
    // Handle keyboard input
}

// Tab traversal (defaults to root scope's reading-order policy)
manager.focus_next();      // Tab
manager.focus_previous();  // Shift+Tab
```

### Gesture Recognition

High-level gesture detection with arena-based conflict resolution.

```rust
use flui_interaction::prelude::*;
use flui_interaction::GestureRecognizer; // `add_pointer`

// Recognizers compete in one shared arena; `new` returns an `Arc<Self>`
// and each `with_on_*` builder consumes and returns it.
let arena = GestureArena::new();
let tap = TapGestureRecognizer::new(arena.clone())
    .with_on_tap(|details| println!("Tapped at {:?}", details.global_position));

// At runtime `GestureBinding` feeds it: `add_pointer` on pointer down,
// `handle_event` for the rest of the sequence.
tap.add_pointer(pointer_id, local_position, global_position);
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
let tap = TapGestureRecognizer::new(arena.clone())
    .with_on_tap_down(|details| { /* pointer down */ })
    .with_on_tap_up(|details| { /* pointer up, tap confirmed */ })
    .with_on_tap(|details| { /* complete tap */ })
    .with_on_tap_cancel(|details| { /* tap cancelled */ });
```

### DoubleTapGestureRecognizer

Two taps in quick succession.

```rust
let double_tap = DoubleTapGestureRecognizer::new(arena.clone())
    .with_on_double_tap(|details| println!("Double tapped!"))
    .with_on_double_tap_down(|details| { /* second pointer down */ });
```

### LongPressGestureRecognizer

Press and hold.

```rust
let long_press = LongPressGestureRecognizer::new(arena.clone())
    .with_on_long_press_start(|details| { /* hold started */ })
    .with_on_long_press_move_update(|details| { /* moved while holding */ })
    .with_on_long_press_end(|details| { /* released */ });
```

### DragGestureRecognizer

Pan/drag gestures.

```rust
let drag = DragGestureRecognizer::new(arena.clone(), DragAxis::Free)
    .with_on_start(|details| { /* drag started */ })
    .with_on_update(|details| {
        let delta = details.delta;
    })
    .with_on_end(|details| {
        let velocity = details.velocity;
    });
```

### ScaleGestureRecognizer

Pinch-to-zoom and rotation.

```rust
let scale = ScaleGestureRecognizer::new(arena.clone())
    .with_on_scale_start(|details| { /* scale started */ })
    .with_on_scale_update(|details| {
        let scale = details.scale;
        let rotation = details.rotation;
        let focal_point = details.focal_point;
    })
    .with_on_scale_end(|details| { /* scale ended */ });
```

### ForcePressGestureRecognizer

Pressure-sensitive input (3D Touch, Force Touch).

```rust
let force = ForcePressGestureRecognizer::new(arena.clone())
    .with_on_start(|details| { /* force threshold reached */ })
    .with_on_peak(|details| { /* max pressure */ })
    .with_on_update(|details| { /* pressure changed */ })
    .with_on_end(|details| { /* released */ });
```

---

## Gesture Arena

Resolves conflicts when multiple recognizers compete for the same pointer.

```rust
use flui_interaction::prelude::*;
use flui_interaction::GestureRecognizer; // `add_pointer`

// Recognizers built on clones of one arena share it.
let arena = GestureArena::new();
let tap = TapGestureRecognizer::new(arena.clone());
let drag = DragGestureRecognizer::new(arena.clone(), DragAxis::Free);

// Each recognizer joins the pointer's arena entry from `add_pointer`
// (`GestureArena::add` underneath).
tap.add_pointer(pointer_id, position, global_position);
drag.add_pointer(pointer_id, position, global_position);

// Arena resolves winner based on:
// 1. First to accept wins
// 2. Last remaining after others reject
// 3. Timeout forces resolution
```

### Disambiguation

- **Tap vs Drag**: Drag wins if movement > slop threshold
- **Tap vs Long Press**: Long press wins after timeout
- **Tap vs Double Tap**: Waits for possible second tap

At runtime, `GestureBinding` owns a `BindingDriven` arena shared with the
mounted widget tree. It dispatches the full hit path before closing the arena
on pointer down, sweeps on pointer up, and does not sweep on pointer cancel;
cancelled recognizers reject themselves so an interrupted gesture cannot be
turned into a forced first-member win.

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
use flui_interaction::{PointerDeviceKind, VelocityTracker};

let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
tracker.add_position(timestamp, position);
// ... more positions ...

let velocity = tracker.get_velocity();
// logical pixels per second
```

### PointerEventResampler

Synchronizes pointer events with frame timing.

```rust
use flui_interaction::PointerEventResampler;

let resampler = PointerEventResampler::new(pointer_id);
resampler.add_event(event);

// At frame time: emit events interpolated for this sampling window
resampler.sample(sample_time, next_sample_time, |event| dispatch(event));
```

### InputPredictor

Predicts future pointer positions to reduce latency.

```rust
use flui_interaction::InputPredictor;

let mut predictor = InputPredictor::new();
predictor.add_sample(timestamp, position);

let predicted = predictor.predict(time_ahead); // a `Duration`
```

---

## Testing Utilities

### Synthetic events

`flui_interaction::testing::input` (behind the `testing` feature) builds
individual pointer, keyboard, and modifier events for a test that drives a
recogniser or a binding directly.

```rust
use flui_interaction::testing::input::KeyEventBuilder;
```

### Scripted gestures

Gesture scripting and replay live in **`flui-testing`**, not here. A gesture is
a shape in time, and replaying one means replaying its timing — which needs a
virtual clock this crate has no driver for. See `flui_testing::replay`:
`PointerScript` authors explicit virtual-time offsets and
`HeadlessBinding::replay` advances the binding's `ManualClock` to each one.

This crate's part of that contract is that every recogniser samples the
**arena's** clock (`RecognizerState::now()`) rather than `Instant::now()`, so a
replayed gesture's own sample spacing decides the velocity it carries.

---

## Type-Safe IDs

Newtype pattern prevents mixing ID types. `PointerId` is re-exported from
the `ui-events` crate (W3C-compliant `NonZeroU64`); `FocusNodeId` and
`HandlerId` are crate-local `NonZeroU64` newtypes.

```rust
use flui_interaction::{PointerId, FocusNodeId, HandlerId};

// Primary pointer convention.
let pointer = PointerId::PRIMARY;
let focus = FocusNodeId::new(42);

// fn process(id: PointerId) { ... }
// process(focus);  // Compile error - wrong type!
```

---

## Configuration

### GestureSettings

```rust
use flui_interaction::GestureSettings;

use std::time::Duration;

let settings = GestureSettings::default()
    .with_touch_slop(18.0)                                 // Movement before drag starts
    .with_pan_slop(36.0)                                   // Movement for pan gesture
    .with_double_tap_timeout(Duration::from_millis(300))   // Between double-tap contacts
    .with_long_press_timeout(Duration::from_millis(500))   // To trigger long press
    .with_min_fling_velocity(50.0)
    .with_max_fling_velocity(8000.0);
```

---

## Ownership and threading

FLUI separates executable UI ownership from data-plane routing:

- recognizer callbacks, focus/key callbacks, and mouse-region callbacks are
  owner-local and may capture `Rc` UI state;
- pointer events, route identities, and other data-plane tokens keep the
  thread-safety required by the embedding/runtime boundary;
- executable callbacks should stay in the owner runtime, not in render storage
  or a generic cross-thread executor.

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

---

## Beyond Flutter

Input-processing capabilities Flutter does not ship, each implemented from
the canonical published source:

| Capability | Source | API |
|---|---|---|
| Impulse fling velocity (Android's default strategy since 8.1) | AOSP `VelocityTracker.cpp` | `processing::ImpulseVelocityTracker` |
| Speed-adaptive pointer smoothing | 1€ filter, Casiez et al., CHI 2012 | `processing::OneEuroFilter`, `OneEuroFilter2D` |
| Platform-faithful gesture presets with runtime dispatch | AOSP `ViewConfiguration` / `UIGestureRecognizer` | `GestureSettings::for_platform`, `native`, `android_defaults`, `ios_defaults` |
| Evidence-capped pointer prediction (25 ms) | Chromium `input_predictor.h` | `processing::InputPredictor` |

See `examples/pointer_filtering.rs`.
