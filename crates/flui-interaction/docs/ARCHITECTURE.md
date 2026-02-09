# Architecture

Internal design of `flui_interaction`.

## Module Structure

```
src/
├── lib.rs              # Public API, re-exports, prelude
│
├── ids.rs              # Type-safe identifiers (PointerId, FocusNodeId, etc.)
├── sealed.rs           # Sealed trait infrastructure
├── traits.rs           # Core traits (Disposable, GestureCallback, etc.)
├── typestate.rs        # Typestate pattern implementations
│
├── routing/            # Event routing
│   ├── mod.rs
│   ├── event_router.rs # Main event dispatcher
│   ├── hit_test.rs     # HitTestResult, HitTestEntry, HitTestable
│   ├── focus.rs        # FocusManager, FocusNode
│   ├── focus_scope.rs  # Focus scopes for modal dialogs
│   └── pointer_router.rs # Pointer event routing
│
├── recognizers/        # Gesture recognizers
│   ├── mod.rs
│   ├── recognizer.rs   # GestureRecognizer trait
│   ├── tap.rs          # TapGestureRecognizer
│   ├── double_tap.rs   # DoubleTapGestureRecognizer
│   ├── long_press.rs   # LongPressGestureRecognizer
│   ├── drag.rs         # DragGestureRecognizer
│   ├── scale.rs        # ScaleGestureRecognizer
│   ├── force_press.rs  # ForcePressGestureRecognizer
│   ├── multi_tap.rs    # MultiTapGestureRecognizer
│   ├── one_sequence.rs # OneSequenceGestureRecognizer base
│   └── primary_pointer.rs # PrimaryPointerGestureRecognizer base
│
├── arena.rs            # GestureArena for conflict resolution
├── team.rs             # GestureArenaTeam for cooperative recognizers
├── timer.rs            # Gesture timer service
│
├── processing/         # Input processing
│   ├── mod.rs
│   ├── velocity.rs     # VelocityTracker
│   ├── resampler.rs    # PointerEventResampler
│   ├── prediction.rs   # InputPredictor
│   └── raw_input.rs    # RawInputHandler
│
├── testing/            # Testing utilities
│   ├── mod.rs
│   ├── recorder.rs     # GestureRecorder
│   ├── player.rs       # GesturePlayer
│   └── builder.rs      # GestureBuilder, ModifiersBuilder
│
├── events.rs           # W3C event type re-exports
├── binding.rs          # GestureBinding for app integration
├── mouse_tracker.rs    # Mouse enter/exit/hover tracking
├── signal_resolver.rs  # Pointer signal conflict resolution
└── settings.rs         # GestureSettings, defaults
```

## Core Abstractions

### HitTestable Trait

Sealed trait for hit testing:

```rust
pub trait HitTestable: sealed::Sealed {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool;
    fn hit_test_behavior(&self) -> HitTestBehavior;
}
```

Sealed because:
- API evolution without breaking changes
- Controlled implementation for correctness
- Extension via `CustomHitTestable` helper trait

### GestureRecognizer Trait

Base trait for all recognizers:

```rust
pub trait GestureRecognizer: GestureArenaMember + Send + Sync {
    fn add_pointer(&self, pointer: PointerId, event: &PointerEvent);
    fn handle_event(&self, event: &PointerEvent);
    fn reject_gesture(&self, pointer: PointerId);
    fn dispose(&self);
    
    fn is_pointer_allowed(&self, event: &PointerEvent) -> bool;
    fn gesture_settings(&self) -> &GestureSettings;
}
```

### GestureArenaMember Trait

For arena participation:

```rust
pub trait GestureArenaMember: sealed::Sealed {
    fn accept_gesture(&self, pointer: PointerId);
    fn reject_gesture(&self, pointer: PointerId);
}
```

## Hit Testing

### Transform Stack

Manages coordinate transformations:

```rust
pub struct HitTestResult {
    entries: Vec<HitTestEntry>,
    transform_stack: Vec<Matrix4>,
    current_transform: Matrix4,
}

impl HitTestResult {
    pub fn push_offset(&mut self, offset: Offset);
    pub fn push_transform(&mut self, transform: Matrix4);
    pub fn pop_transform(&mut self);
    
    pub fn add(&mut self, entry: HitTestEntry);
}
```

### Entry Storage

Entries stored front-to-back (leaf first):

```rust
pub fn add(&mut self, entry: HitTestEntry) {
    let mut entry = entry;
    entry.transform = self.current_transform;
    self.entries.insert(0, entry);  // Leaf first
}
```

Dispatch order: leaf → root (most specific handler first).

### Event Dispatch

```rust
pub fn dispatch(&self, event: &PointerEvent) -> EventPropagation {
    for entry in &self.entries {
        // Transform event to local coordinates
        let local_event = entry.transform_event(event);
        
        // Call handler
        if let Some(handler) = &entry.handler {
            match handler(&local_event) {
                EventPropagation::Stop => return EventPropagation::Stop,
                EventPropagation::Continue => continue,
            }
        }
    }
    EventPropagation::Continue
}
```

## Focus Management

### FocusManager

Global singleton for keyboard focus:

```rust
pub struct FocusManager {
    focused: RwLock<Option<FocusNodeId>>,
    nodes: RwLock<HashMap<FocusNodeId, FocusNode>>,
    scopes: RwLock<Vec<FocusScopeNode>>,
    traversal_policy: RwLock<Box<dyn FocusTraversalPolicy>>,
}

impl FocusManager {
    pub fn global() -> &'static FocusManager;
    
    pub fn request_focus(&self, node: FocusNodeId);
    pub fn unfocus(&self);
    pub fn has_focus(&self, node: FocusNodeId) -> bool;
    
    pub fn next_focus(&self);      // Tab
    pub fn previous_focus(&self);  // Shift+Tab
}
```

### Focus Scopes

Isolate focus within regions:

```rust
pub struct FocusScopeNode {
    id: FocusScopeId,
    first_focus: Option<FocusNodeId>,
    trap_focus: bool,  // Modal behavior
}
```

### Traversal Policies

```rust
pub trait FocusTraversalPolicy: Send + Sync {
    fn find_first_focus(&self, scope: &FocusScopeNode) -> Option<FocusNodeId>;
    fn find_next_focus(&self, current: FocusNodeId, direction: TraversalDirection) 
        -> Option<FocusNodeId>;
}

// Built-in policies
pub struct OrderedTraversalPolicy;   // Explicit order
pub struct ReadingOrderPolicy;       // Top-left to bottom-right
pub struct DirectionalFocusPolicy;   // Arrow key navigation
```

## Gesture Arena

### Conflict Resolution

When multiple recognizers compete:

```rust
pub struct GestureArena {
    entries: Mutex<HashMap<PointerId, Vec<GestureArenaEntry>>>,
    disambiguation_timeout: Duration,
}

impl GestureArena {
    pub fn add(&self, pointer: PointerId, member: Arc<dyn GestureArenaMember>);
    
    pub fn accept(&self, pointer: PointerId, member: &dyn GestureArenaMember);
    pub fn reject(&self, pointer: PointerId, member: &dyn GestureArenaMember);
    
    pub fn sweep(&self, pointer: PointerId);  // Force resolution
}
```

### Resolution Rules

1. First to `accept()` wins
2. Last remaining after others `reject()` wins
3. After timeout, first entry wins (default: 100ms)

### GestureArenaTeam

Cooperative recognizers that share victory:

```rust
pub struct GestureArenaTeam {
    members: Vec<Arc<dyn GestureArenaMember>>,
    captain: Option<Arc<dyn GestureArenaMember>>,
}

// All members win together
team.accept(pointer_id);
```

## Recognizer State Machines

### TapGestureRecognizer

```
Idle → Down → (movement < slop && time < timeout) → Up → Tap!
        ↓
    (movement > slop) → Rejected
        ↓
    (time > timeout) → Rejected
```

### DragGestureRecognizer

```
Idle → Possible → (movement > slop) → Accepted → Dragging → End
         ↓
    (pointer up) → Rejected
```

### LongPressGestureRecognizer

```
Idle → Possible → (time > timeout) → Accepted → LongPressing → End
         ↓                              ↓
    (pointer up) → Rejected      (movement > slop) → End
```

### ScaleGestureRecognizer

```
Idle → OnePointer → TwoPointers → Scaling → End
           ↓             ↓
       (pointer up)  (pointer up) → OnePointer or End
```

## Input Processing

### VelocityTracker

Estimates velocity from position samples:

```rust
pub struct VelocityTracker {
    samples: VecDeque<Sample>,  // Ring buffer
    strategy: VelocityEstimationStrategy,
}

impl VelocityTracker {
    pub fn add_position(&mut self, time: Duration, position: Offset);
    pub fn velocity(&self) -> Velocity;
}
```

Strategies:
- **LSQ2**: Least squares quadratic fit (default)
- **LSQ3**: Least squares cubic fit
- **Impulse**: For discrete input

### PointerEventResampler

Synchronizes events with vsync:

```rust
pub struct PointerEventResampler {
    events: VecDeque<TimestampedEvent>,
    last_sample: Option<(Duration, Offset)>,
}

impl PointerEventResampler {
    pub fn add_event(&mut self, event: PointerEvent);
    pub fn sample(&mut self, frame_time: Duration) -> Option<PointerEvent>;
}
```

### InputPredictor

Reduces perceived latency:

```rust
pub struct InputPredictor {
    history: VecDeque<(Duration, Offset)>,
    config: PredictionConfig,
}

impl InputPredictor {
    pub fn predict(&self, target_time: Duration) -> Option<PredictedPosition>;
}
```

## Thread Safety

| Component | Synchronization |
|-----------|-----------------|
| FocusManager | `RwLock` (read-heavy) |
| GestureArena | `Mutex` (write-heavy) |
| HitTestResult | Not shared (per-event) |
| Recognizers | Internal `Mutex` |
| VelocityTracker | Not thread-safe (owned per pointer) |

All types exposed in public API are `Send + Sync`.

## Type Safety

### Sealed Traits

Prevent external implementation:

```rust
mod sealed {
    pub trait Sealed {}
}

pub trait HitTestable: sealed::Sealed { /* ... */ }

// External crates use helper trait
pub trait CustomHitTestable {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool;
}

impl<T: CustomHitTestable> sealed::Sealed for T {}
impl<T: CustomHitTestable> HitTestable for T { /* delegate */ }
```

### Newtype IDs

Compile-time type safety:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(NonZeroU64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusNodeId(NonZeroU64);

// Cannot mix: fn process(id: PointerId) cannot accept FocusNodeId
```

Benefits:
- `Option<PointerId>` same size as `PointerId` (niche optimization)
- Type-level documentation of intent
- Compile errors instead of runtime bugs
