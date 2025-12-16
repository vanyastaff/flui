# Gesture Recognition Guide

Internal documentation for gesture recognition in `flui_interaction`.

## Gesture Recognizer Architecture

```
GestureArenaMember (trait)
    │
    ├── GestureRecognizer (trait) - base with add_pointer, handle_event
    │       │
    │       └── OneSequenceGestureRecognizer (trait) - single pointer tracking
    │               │
    │               └── PrimaryPointerGestureRecognizer (trait) - state machine + deadline
    │
    └── Concrete Recognizers
        ├── TapGestureRecognizer
        ├── LongPressGestureRecognizer
        ├── DoubleTapGestureRecognizer
        ├── DragGestureRecognizer
        ├── ScaleGestureRecognizer
        ├── MultiTapGestureRecognizer
        └── ForcePressGestureRecognizer
```

## Available Recognizers

### TapGestureRecognizer

Single tap detection with slop tolerance.

```rust
let recognizer = TapGestureRecognizer::new(arena)
    .with_on_tap_down(|details| { /* pointer contact */ })
    .with_on_tap_move(|details| { /* movement within slop */ })
    .with_on_tap_up(|details| { /* pointer released */ })
    .with_on_tap(|details| { /* successful tap */ })
    .with_on_tap_cancel(|details| { /* cancelled */ });
```

**State Machine:**
```
Ready → Down (on pointer_down)
Down → Ready (on pointer_up within slop → tap success)
Down → Cancelled (on move beyond TAP_SLOP → cancel)
```

**Details Struct:**
```rust
pub struct TapDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerType,
}
```

### DoubleTapGestureRecognizer

Two taps in quick succession within distance tolerance.

```rust
let recognizer = DoubleTapGestureRecognizer::new(arena)
    .with_on_double_tap(|details| { /* double tap recognized */ })
    .with_on_double_tap_cancel(|details| { /* cancelled */ });
```

**State Machine:**
```
Ready → FirstDown (first pointer_down)
FirstDown → WaitingForSecond (first pointer_up)
WaitingForSecond → Ready (timeout expired)
WaitingForSecond → FirstDown (second tap too far)
WaitingForSecond → SecondDown (second pointer_down within slop/timeout)
SecondDown → Completed (second pointer_up → success)
SecondDown → Cancelled (movement beyond slop)
```

**Timing/Distance Constants (from GestureSettings):**
- `double_tap_timeout()`: 300ms between taps
- `double_tap_slop()`: 100px max distance between taps

### LongPressGestureRecognizer

Pointer held for duration without movement.

```rust
let recognizer = LongPressGestureRecognizer::new(arena)
    .with_on_long_press_down(|details| { /* initial contact */ })
    .with_on_long_press(|| { /* timer elapsed - simple */ })
    .with_on_long_press_start(|details| { /* timer elapsed - with details */ })
    .with_on_long_press_move_update(|details| { /* movement after start */ })
    .with_on_long_press_up(|details| { /* released */ })
    .with_on_long_press_end(|details| { /* ended */ })
    .with_on_long_press_cancel(|details| { /* cancelled */ });
```

**State Machine:**
```
Ready → Possible (pointer_down)
Possible → Started (timer elapsed, within slop)
Possible → Cancelled (movement beyond slop)
Started → Ready (pointer_up → success)
```

**Timer Polling:**
```rust
// Call periodically in event loop
if recognizer.check_timer() {
    // Timer elapsed, long press started
}
```

**Duration Constant (from GestureSettings):**
- `long_press_timeout()`: 500ms

### DragGestureRecognizer

Pointer movement beyond slop threshold.

```rust
let recognizer = DragGestureRecognizer::new(arena, DragAxis::Vertical)
    .with_on_down(|details| { /* pointer contact before drag */ })
    .with_on_start(|details| { /* drag started */ })
    .with_on_update(|details| { /* position changed */ })
    .with_on_end(|details| { /* drag ended with velocity */ })
    .with_on_cancel(|| { /* cancelled */ });
```

**Axis Constraints:**
```rust
pub enum DragAxis {
    Vertical,   // up/down only
    Horizontal, // left/right only
    Free,       // any direction (pan)
}
```

**State Machine:**
```
Ready → Possible (pointer_down)
Possible → Started (movement beyond DRAG_SLOP)
Started → Ready (pointer_up → end with velocity)
Started → Cancelled (arena rejection)
```

**Details Structs:**
```rust
pub struct DragDownDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerType,
}

pub struct DragStartDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerType,
    pub timestamp: Instant,
}

pub struct DragUpdateDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub delta: Offset,           // since last update
    pub primary_delta: f32,      // axis-aligned delta
    pub kind: PointerType,
}

pub struct DragEndDetails {
    pub velocity: Velocity,
    pub global_position: Offset,
    pub local_position: Offset,
    pub primary_velocity: f32,   // axis-aligned velocity
}
```

**Velocity Tracking:**
The recognizer uses `VelocityTracker` to estimate fling velocity:
```rust
if recognizer.is_fling(&details.velocity) {
    // velocity exceeds MIN_FLING_VELOCITY
}
```

### ScaleGestureRecognizer

Pinch-to-zoom with 2+ pointers.

```rust
let recognizer = ScaleGestureRecognizer::new(arena)
    .with_on_scale_start(|details| { /* 2+ pointers, scale changing */ })
    .with_on_scale_update(|details| { /* scale/rotation updated */ })
    .with_on_scale_end(|details| { /* gesture ended */ })
    .with_on_scale_cancel(|| { /* cancelled */ });
```

**State Machine:**
```
Ready → Possible (2 pointers down)
Possible → Started (scale delta > min_scale_delta)
Started → Ready (< 2 pointers → end)
```

**Details Structs:**
```rust
pub struct ScaleStartDetails {
    pub focal_point: Offset,
    pub local_focal_point: Offset,
    pub pointer_count: usize,
}

pub struct ScaleUpdateDetails {
    pub focal_point: Offset,
    pub local_focal_point: Offset,
    pub scale: f32,             // 1.0 = no change
    pub horizontal_scale: f32,
    pub vertical_scale: f32,
    pub rotation: f32,          // radians, positive = clockwise
    pub pointer_count: usize,
}

pub struct ScaleEndDetails {
    pub focal_point: Offset,
    pub scale: f32,
    pub rotation: f32,
    pub velocity: f32,          // scale velocity
}
```

**Calculations:**
- **Focal point**: Center of all active pointers
- **Span**: Average distance between pointer pairs
- **Scale**: current_span / initial_span
- **Rotation**: Angle change from initial pointer configuration

### ForcePressGestureRecognizer

Pressure-sensitive touch (3D Touch, Force Touch).

```rust
let recognizer = ForcePressGestureRecognizer::new(arena)
    .with_start_pressure(0.4)   // 40% threshold
    .with_peak_pressure(0.85)   // 85% peak
    .with_on_start(|details| { /* pressure exceeded start */ })
    .with_on_update(|details| { /* pressure changed */ })
    .with_on_peak(|details| { /* pressure exceeded peak */ })
    .with_on_end(|details| { /* pressure dropped or released */ });
```

**State Machine:**
```
Ready → Possible (pointer_down with pressure > 0)
Ready → Ended (pointer_down with pressure = 0, no support)
Possible → Started (pressure >= start_threshold)
Started → Peaked (pressure >= peak_threshold)
Started/Peaked → Ended (pressure < start_threshold or pointer_up)
```

**Pressure Constants:**
- `FORCE_PRESS_START_PRESSURE`: 0.4 (40%)
- `FORCE_PRESS_PEAK_PRESSURE`: 0.85 (85%)

**Details Struct:**
```rust
pub struct ForcePressDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub pressure: f32,
    pub max_pressure: f32,      // always 1.0 for normalized
}

impl ForcePressDetails {
    pub fn normalized_pressure(&self) -> f32;
}
```

### MultiTapGestureRecognizer

Configurable N-finger tap detection.

```rust
let recognizer = MultiTapGestureRecognizer::new(arena, 3) // 3-finger tap
    .with_on_multi_tap(|details| { /* N fingers tapped */ });
```

## Slop Constants

Touch slop values from `GestureSettings`:

| Constant | Default | Description |
|----------|---------|-------------|
| `touch_slop()` | 18.0 | Max movement for tap |
| `double_tap_slop()` | 100.0 | Max distance between double-tap locations |
| `pan_slop()` | 18.0 | Min movement to start drag |
| `scale_slop()` | 18.0 | Min pointer distance change for scale |

## GestureSettings

Device-specific gesture thresholds:

```rust
pub struct GestureSettings {
    touch_slop: f32,            // 18.0
    double_tap_slop: f32,       // 100.0
    double_tap_timeout: Duration, // 300ms
    long_press_timeout: Duration, // 500ms
    pan_slop: f32,              // 18.0
    scale_slop: f32,            // 18.0
    min_fling_velocity: f32,    // 50.0 px/s
}

// Apply custom settings
let recognizer = TapGestureRecognizer::with_settings(arena, settings);
recognizer.set_settings(new_settings);
```

## Gesture Arena Integration

All recognizers implement `GestureArenaMember`:

```rust
impl GestureArenaMember for TapGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        // Won arena - gesture accepted
    }
    
    fn reject_gesture(&self, pointer: PointerId) {
        // Lost arena - cancel gesture
    }
}
```

### Entry Handle Pattern

```rust
// Preferred: Use entry handle for resolution
let entry = arena.add(pointer, recognizer.clone());

// Later, when recognizer decides:
entry.resolve(GestureDisposition::Accepted);
// or
entry.resolve(GestureDisposition::Rejected);
```

### Lifecycle

1. **Pointer down** → `add_pointer(pointer, position)`
2. **Arena adds** → Recognizer stored in arena entry
3. **Events** → `handle_event(&event)` called for each
4. **Resolution** → Arena calls `accept_gesture` or `reject_gesture`
5. **Cleanup** → `dispose()` or arena sweep

## Velocity Tracking

```rust
let mut tracker = VelocityTracker::new();

// Add samples during drag
tracker.add_position(Instant::now(), position);

// Get velocity at end
let velocity = tracker.velocity();
println!("Speed: {} px/s", velocity.pixels_per_second.distance());
```

## Custom Recognizers

Implement `CustomGestureRecognizer` (blanket impl provides `GestureArenaMember`):

```rust
use flui_interaction::sealed::CustomGestureRecognizer;

struct TripleTapRecognizer {
    state: GestureRecognizerState,
    tap_count: AtomicU32,
}

impl CustomGestureRecognizer for TripleTapRecognizer {
    fn on_arena_accept(&self, pointer: PointerId) {
        // Handle winning arena
    }
    
    fn on_arena_reject(&self, pointer: PointerId) {
        // Handle losing arena
    }
}

impl GestureRecognizer for TripleTapRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset) {
        let arc = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &arc);
    }
    
    fn handle_event(&self, event: &PointerEvent) {
        // Process events, increment tap_count
    }
    
    fn dispose(&self) {
        self.state.mark_disposed();
    }
    
    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}
```

## Common Patterns

### Combining Recognizers

```rust
// Multiple recognizers on same widget
let arena = GestureArena::new();

let tap = TapGestureRecognizer::new(arena.clone());
let long_press = LongPressGestureRecognizer::new(arena.clone());
let drag = DragGestureRecognizer::new(arena.clone(), DragAxis::Free);

// Arena resolves conflicts - only one wins per pointer
```

### Fling Detection

```rust
DragGestureRecognizer::new(arena, DragAxis::Free)
    .with_on_end(|details| {
        if details.velocity.pixels_per_second.distance() >= 50.0 {
            // Fling gesture - apply momentum
            start_fling_animation(details.velocity);
        }
    });
```

### Gesture Disambiguation

```rust
// For overlapping gestures (tap vs double-tap)
DoubleTapGestureRecognizer::new(arena.clone())
    .with_on_double_tap(|_| { /* zoom in */ });

// Single tap delayed until double-tap times out
TapGestureRecognizer::new(arena.clone())
    .with_on_tap(|_| { /* select item */ });

// Arena waits for double-tap timeout before awarding to tap
```

## Thread Safety

All recognizers are `Send + Sync`:
- State protected by `parking_lot::Mutex`
- Callbacks stored in `Arc<dyn Fn + Send + Sync>`
- Arena uses `DashMap` for lock-free concurrent access

## Testing

```rust
#[test]
fn test_tap_recognition() {
    let arena = GestureArena::new();
    let tapped = Arc::new(Mutex::new(false));
    
    let recognizer = TapGestureRecognizer::new(arena)
        .with_on_tap({
            let tapped = tapped.clone();
            move |_| *tapped.lock() = true
        });
    
    // Simulate tap
    recognizer.add_pointer(PointerId::new(1), Offset::new(100.0, 100.0));
    recognizer.handle_event(&make_up_event(Offset::new(100.0, 100.0), PointerType::Touch));
    
    assert!(*tapped.lock());
}
```

## See Also

- [ARCHITECTURE.md](ARCHITECTURE.md) - Core architecture
- [HIT_TESTING.md](HIT_TESTING.md) - Hit testing system
- [PERFORMANCE.md](PERFORMANCE.md) - Performance guide
