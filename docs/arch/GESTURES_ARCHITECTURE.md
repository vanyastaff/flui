# FLUI Gestures Architecture

**Version:** 0.1.0
**Date:** 2025-11-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document defines the architecture for FLUI's gesture recognition system, based on Flutter's proven gesture framework. The system follows the **persistent object pattern** with clear separation of concerns:

- **Persistent objects** (`GestureRecognizer`, `PointerRouter`, `GestureArena`) in `flui_gestures` crate - Arc-based, extend GestureArenaMember
- **Gesture widgets** (`GestureDetector`, `RawGestureDetector`, `InkWell`) in `flui_widgets` - manage recognizer lifecycle
- **Hit testing** (`HitTestable`, `HitTestTarget`) in `flui_core/foundation` - route pointer events to targets

**Key Design Principles:**
1. **Gesture Arena**: Conflict resolution when multiple recognizers compete for same pointer
2. **Persistent Recognizers**: GestureRecognizer objects survive widget rebuilds, must be disposed
3. **Pointer Routing**: Efficient event dispatch to registered recognizers
4. **Hit Testing**: Determine which render objects receive pointer events
5. **Type-Safe Callbacks**: Generic recognizer types with specific callback signatures

**Total Work Estimate:** ~2,500 LOC in gestures crate + ~800 LOC in widgets

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Gesture Types](#core-gesture-types)
3. [Gesture Arena (Conflict Resolution)](#gesture-arena-conflict-resolution)
4. [Pointer Router](#pointer-router)
5. [Hit Testing](#hit-testing)
6. [Gesture Recognizers](#gesture-recognizers)
7. [Gesture Widgets](#gesture-widgets)
8. [Implementation Plan](#implementation-plan)
9. [Usage Examples](#usage-examples)
10. [Testing Strategy](#testing-strategy)

---

## Architecture Overview

### Three-Layer Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                       flui_widgets                          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  GestureDetector, RawGestureDetector                 │   │
│  │  InkWell, Dismissible, Draggable                     │   │
│  │  LongPressDraggable, DragTarget                      │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓ uses
┌─────────────────────────────────────────────────────────────┐
│                     flui_gestures                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  GestureRecognizer hierarchy:                        │   │
│  │    - TapGestureRecognizer                            │   │
│  │    - DoubleTapGestureRecognizer                      │   │
│  │    - LongPressGestureRecognizer                      │   │
│  │    - DragGestureRecognizer (Vertical, Horizontal)    │   │
│  │    - PanGestureRecognizer                            │   │
│  │    - ScaleGestureRecognizer                          │   │
│  │  PointerRouter, GestureArena                         │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↓ uses
┌─────────────────────────────────────────────────────────────┐
│                  flui_core/foundation                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  HitTestable, HitTestTarget, HitTestResult           │   │
│  │  PointerEvent types (already in flui_types)          │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Event Flow Pipeline

```text
Raw Pointer Event (PointerDownEvent, PointerMoveEvent, etc.)
         ↓
   Hit Testing (determine which RenderObjects are hit)
         ↓
   PointerRouter (dispatch to registered recognizers)
         ↓
   GestureRecognizers (detect specific gestures)
         ↓
   GestureArena (resolve conflicts if multiple recognizers)
         ↓
   Winner Recognizer (calls user callbacks)
```

### Persistent Object Pattern

Following the same pattern as AnimationController and ScrollController:

```rust
// GestureRecognizers are PERSISTENT (like AnimationController)
let tap_recognizer = TapGestureRecognizer::new();
tap_recognizer.on_tap(Arc::new(|| println!("Tapped!")));

// They survive widget rebuilds
let drag_recognizer = VerticalDragGestureRecognizer::new();
drag_recognizer.on_start(Arc::new(|details| { /* ... */ }));

// Widgets manage their lifecycle
GestureDetector::builder()
    .on_tap(|| println!("Tapped!"))  // Creates TapGestureRecognizer internally
    .child(Container::new())
    .build()

// CRITICAL: Must dispose when done
tap_recognizer.dispose();
```

---

## Core Gesture Types

### 1. GestureArenaMember Trait (Base for Arena)

```rust
// In flui_gestures/src/arena.rs

/// Member of the gesture arena
///
/// All gesture recognizers must implement this to participate in conflict resolution.
pub trait GestureArenaMember: Send + Sync + fmt::Debug {
    /// Called when this member wins the arena
    fn accept_gesture(&self, pointer: PointerId);

    /// Called when this member loses the arena
    fn reject_gesture(&self, pointer: PointerId);
}

/// Unique identifier for a pointer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(pub u64);

impl PointerId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}
```

### 2. GestureRecognizer Trait (Base for All Recognizers)

```rust
// In flui_gestures/src/recognizer.rs

/// Base trait for all gesture recognizers
///
/// GestureRecognizer is a PERSISTENT OBJECT that survives widget rebuilds.
/// It must be disposed when no longer needed.
pub trait GestureRecognizer: GestureArenaMember + Send + Sync + fmt::Debug {
    /// Add pointer to this recognizer's tracking
    fn add_pointer(&self, event: &PointerDownEvent);

    /// Handle pointer event
    fn handle_event(&self, event: &PointerEvent);

    /// CRITICAL: Dispose when done to prevent leaks
    fn dispose(&self);

    /// Get the kind of device this recognizer handles
    fn kind(&self) -> Option<PointerDeviceKind> {
        None  // None means all devices
    }
}

/// Device kind for pointer events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerDeviceKind {
    Touch,
    Mouse,
    Stylus,
    InvertedStylus,
    Trackpad,
}
```

### 3. OneSequenceGestureRecognizer (Base for Most Recognizers)

```rust
// In flui_gestures/src/one_sequence_recognizer.rs

/// Base class for recognizers that track a single gesture sequence
///
/// Most recognizers (Tap, Drag, Scale) extend this. It ensures that only
/// one gesture is tracked at a time, even if multiple pointers are involved.
pub struct OneSequenceGestureRecognizer {
    inner: Arc<Mutex<OneSequenceInner>>,
}

struct OneSequenceInner {
    /// Current pointer being tracked
    primary_pointer: Option<PointerId>,

    /// Initial position of primary pointer
    initial_position: Option<Offset>,

    /// State of gesture recognition
    state: GestureRecognizerState,

    /// Pointer router for event routing
    router: Arc<PointerRouter>,

    /// Arena entry for conflict resolution
    arena_entry: Option<GestureArenaEntry>,

    /// Is disposed?
    disposed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureRecognizerState {
    /// Waiting for gesture to start
    Ready,

    /// Gesture might be starting (collecting evidence)
    Possible,

    /// Gesture accepted (won arena)
    Accepted,

    /// Gesture rejected (lost arena)
    Defunct,
}

impl OneSequenceGestureRecognizer {
    pub fn new(router: Arc<PointerRouter>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(OneSequenceInner {
                primary_pointer: None,
                initial_position: None,
                state: GestureRecognizerState::Ready,
                router,
                arena_entry: None,
                disposed: false,
            })),
        }
    }

    /// Start tracking a pointer
    pub fn start_tracking_pointer(&self, pointer: PointerId, event: &PointerDownEvent) {
        let mut inner = self.inner.lock();

        if inner.primary_pointer.is_some() {
            // Already tracking a pointer, ignore
            return;
        }

        inner.primary_pointer = Some(pointer);
        inner.initial_position = Some(event.position);
        inner.state = GestureRecognizerState::Possible;

        // Add to gesture arena
        let entry = inner.router.arena().add(pointer, Arc::new(self.clone()));
        inner.arena_entry = Some(entry);
    }

    /// Stop tracking the current pointer
    pub fn stop_tracking_pointer(&self, pointer: PointerId) {
        let mut inner = self.inner.lock();

        if inner.primary_pointer != Some(pointer) {
            return;
        }

        inner.primary_pointer = None;
        inner.initial_position = None;
        inner.state = GestureRecognizerState::Ready;
    }

    /// Resolve gesture (called when recognizer decides outcome)
    pub fn resolve(&self, disposition: GestureDisposition) {
        let mut inner = self.inner.lock();

        if let Some(entry) = &inner.arena_entry {
            match disposition {
                GestureDisposition::Accepted => entry.resolve(GestureDisposition::Accepted),
                GestureDisposition::Rejected => entry.resolve(GestureDisposition::Rejected),
            }
        }

        inner.arena_entry = None;
    }

    pub fn primary_pointer(&self) -> Option<PointerId> {
        self.inner.lock().primary_pointer
    }

    pub fn initial_position(&self) -> Option<Offset> {
        self.inner.lock().initial_position
    }

    pub fn state(&self) -> GestureRecognizerState {
        self.inner.lock().state
    }
}

impl GestureArenaMember for OneSequenceGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        let mut inner = self.inner.lock();

        if inner.primary_pointer != Some(pointer) {
            return;
        }

        inner.state = GestureRecognizerState::Accepted;
    }

    fn reject_gesture(&self, pointer: PointerId) {
        let mut inner = self.inner.lock();

        if inner.primary_pointer != Some(pointer) {
            return;
        }

        inner.state = GestureRecognizerState::Defunct;
        self.stop_tracking_pointer(pointer);
    }
}

impl GestureRecognizer for OneSequenceGestureRecognizer {
    fn add_pointer(&self, event: &PointerDownEvent) {
        self.start_tracking_pointer(PointerId::new(event.pointer), event);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // Override in subclasses
    }

    fn dispose(&self) {
        let mut inner = self.inner.lock();
        inner.disposed = true;

        if let Some(pointer) = inner.primary_pointer {
            self.stop_tracking_pointer(pointer);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureDisposition {
    Accepted,
    Rejected,
}
```

---

## Gesture Arena (Conflict Resolution)

### GestureArena - Conflict Resolution System

```rust
// In flui_gestures/src/arena.rs

/// Manages gesture conflict resolution
///
/// When multiple gesture recognizers compete for the same pointer,
/// the arena decides which one wins.
#[derive(Clone)]
pub struct GestureArenaManager {
    arenas: Arc<Mutex<HashMap<PointerId, GestureArena>>>,
}

struct GestureArena {
    members: Vec<Arc<dyn GestureArenaMember>>,
    state: ArenaState,
    eager_winner: Option<Arc<dyn GestureArenaMember>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArenaState {
    Open,
    Closed,
    Resolved,
}

impl GestureArenaManager {
    pub fn new() -> Self {
        Self {
            arenas: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a member to the arena for a specific pointer
    pub fn add(
        &self,
        pointer: PointerId,
        member: Arc<dyn GestureArenaMember>,
    ) -> GestureArenaEntry {
        let mut arenas = self.arenas.lock();

        let arena = arenas.entry(pointer).or_insert_with(|| GestureArena {
            members: Vec::new(),
            state: ArenaState::Open,
            eager_winner: None,
        });

        arena.members.push(member.clone());

        GestureArenaEntry {
            pointer,
            member,
            manager: self.clone(),
        }
    }

    /// Close the arena (no more members can be added)
    pub fn close(&self, pointer: PointerId) {
        let mut arenas = self.arenas.lock();

        if let Some(arena) = arenas.get_mut(&pointer) {
            arena.state = ArenaState::Closed;
            self.try_resolve_arena(pointer, arena);
        }
    }

    /// Sweep the arena (force resolution on pointer up)
    pub fn sweep(&self, pointer: PointerId) {
        let mut arenas = self.arenas.lock();

        if let Some(mut arena) = arenas.remove(&pointer) {
            if arena.state != ArenaState::Resolved {
                self.resolve_in_favor_of(&mut arena, arena.members.first().cloned());
            }
        }
    }

    /// Hold the arena (keep it open even after initial close)
    pub fn hold(&self, pointer: PointerId) {
        // Used by LongPress to delay resolution
        let mut arenas = self.arenas.lock();

        if let Some(arena) = arenas.get_mut(&pointer) {
            arena.state = ArenaState::Open;
        }
    }

    /// Release the arena (allow resolution)
    pub fn release(&self, pointer: PointerId) {
        self.close(pointer);
    }

    fn try_resolve_arena(&self, pointer: PointerId, arena: &mut GestureArena) {
        if arena.state != ArenaState::Closed {
            return;
        }

        // Check for eager winner
        if let Some(winner) = &arena.eager_winner {
            self.resolve_in_favor_of(arena, Some(winner.clone()));
            return;
        }

        // If only one member, it wins
        if arena.members.len() == 1 {
            self.resolve_in_favor_of(arena, arena.members.first().cloned());
        }
    }

    fn resolve_in_favor_of(&self, arena: &mut GestureArena, winner: Option<Arc<dyn GestureArenaMember>>) {
        arena.state = ArenaState::Resolved;

        for member in &arena.members {
            if let Some(ref win) = winner {
                if Arc::ptr_eq(member, win) {
                    member.accept_gesture(PointerId::new(0));  // FIXME: Pass actual pointer
                } else {
                    member.reject_gesture(PointerId::new(0));
                }
            } else {
                member.reject_gesture(PointerId::new(0));
            }
        }
    }
}

/// Handle to an arena entry
pub struct GestureArenaEntry {
    pointer: PointerId,
    member: Arc<dyn GestureArenaMember>,
    manager: GestureArenaManager,
}

impl GestureArenaEntry {
    /// Resolve this entry (accept or reject)
    pub fn resolve(&self, disposition: GestureDisposition) {
        let mut arenas = self.manager.arenas.lock();

        if let Some(arena) = arenas.get_mut(&self.pointer) {
            match disposition {
                GestureDisposition::Accepted => {
                    arena.eager_winner = Some(self.member.clone());
                }
                GestureDisposition::Rejected => {
                    arena.members.retain(|m| !Arc::ptr_eq(m, &self.member));
                }
            }
        }
    }
}
```

**Key Arena Concepts:**

1. **Open State**: Arena is accepting new members
2. **Closed State**: No more members can join, attempting resolution
3. **Resolved State**: Winner declared, callbacks fired
4. **Eager Winner**: Member that claimed victory before arena closed
5. **Sweep**: Force resolution on pointer up event
6. **Hold/Release**: LongPress uses this to delay resolution

---

## Pointer Router

### PointerRouter - Event Dispatch System

```rust
// In flui_gestures/src/pointer_router.rs

/// Routes pointer events to registered gesture recognizers
#[derive(Clone)]
pub struct PointerRouter {
    routes: Arc<Mutex<HashMap<PointerId, Vec<PointerRoute>>>>,
    arena: Arc<GestureArenaManager>,
}

struct PointerRoute {
    callback: PointerRouteCallback,
}

pub type PointerRouteCallback = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

impl PointerRouter {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(Mutex::new(HashMap::new())),
            arena: Arc::new(GestureArenaManager::new()),
        }
    }

    /// Add a route for a specific pointer
    pub fn add_route(&self, pointer: PointerId, callback: PointerRouteCallback) {
        let mut routes = self.routes.lock();
        routes.entry(pointer).or_default().push(PointerRoute { callback });
    }

    /// Remove a route for a specific pointer
    pub fn remove_route(&self, pointer: PointerId, callback: &PointerRouteCallback) {
        let mut routes = self.routes.lock();

        if let Some(pointer_routes) = routes.get_mut(&pointer) {
            pointer_routes.retain(|route| !Arc::ptr_eq(&route.callback, callback));

            if pointer_routes.is_empty() {
                routes.remove(&pointer);
            }
        }
    }

    /// Dispatch event to all registered routes
    pub fn route(&self, event: &PointerEvent) {
        let pointer = PointerId::new(event.pointer());
        let routes = self.routes.lock();

        if let Some(pointer_routes) = routes.get(&pointer) {
            for route in pointer_routes {
                (route.callback)(event);
            }
        }
    }

    /// Get the gesture arena manager
    pub fn arena(&self) -> &GestureArenaManager {
        &self.arena
    }
}
```

---

## Hit Testing

### Hit Testing Infrastructure

```rust
// In flui_core/src/foundation/hit_test.rs

/// Determines which objects are located at a given position
pub trait HitTestable {
    /// Perform hit test at the given position
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool;
}

/// Target that can receive hit test results
pub trait HitTestTarget: Send + Sync {
    /// Handle an event that was dispatched to this target
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry);
}

/// Result of a hit test
#[derive(Default)]
pub struct HitTestResult {
    path: Vec<HitTestEntry>,
}

impl HitTestResult {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a hit test entry
    pub fn add(&mut self, entry: HitTestEntry) {
        self.path.push(entry);
    }

    /// Add a hit test entry with transformation
    pub fn add_with_transform(
        &mut self,
        transform: Matrix4,
        position: Offset,
        hit_test: impl FnOnce(&mut HitTestResult, Offset) -> bool,
    ) -> bool {
        // Transform position and recurse
        let transformed = transform.transform_point(position);
        hit_test(self, transformed)
    }

    /// Get the hit test path
    pub fn path(&self) -> &[HitTestEntry] {
        &self.path
    }
}

/// Entry in the hit test path
pub struct HitTestEntry {
    target: Arc<dyn HitTestTarget>,
    transform: Option<Matrix4>,
}

impl HitTestEntry {
    pub fn new(target: Arc<dyn HitTestTarget>) -> Self {
        Self {
            target,
            transform: None,
        }
    }

    pub fn with_transform(target: Arc<dyn HitTestTarget>, transform: Matrix4) -> Self {
        Self {
            target,
            transform: Some(transform),
        }
    }

    pub fn target(&self) -> &Arc<dyn HitTestTarget> {
        &self.target
    }

    pub fn transform(&self) -> Option<&Matrix4> {
        self.transform.as_ref()
    }
}
```

**Integration with RenderObjects:**

```rust
// In flui_rendering/src/objects/render_box.rs

impl HitTestable for RenderBox {
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if !self.size().contains(position) {
            return false;
        }

        // Add self to hit test path
        result.add(HitTestEntry::new(Arc::new(self.clone())));

        // Test children
        self.hit_test_children(result, position)
    }
}
```

---

## Gesture Recognizers

### 1. TapGestureRecognizer

```rust
// In flui_gestures/src/recognizers/tap.rs

/// Recognizes tap gestures
#[derive(Clone)]
pub struct TapGestureRecognizer {
    base: OneSequenceGestureRecognizer,
    callbacks: Arc<Mutex<TapCallbacks>>,
}

struct TapCallbacks {
    on_tap_down: Option<TapDownCallback>,
    on_tap_up: Option<TapUpCallback>,
    on_tap: Option<TapCallback>,
    on_tap_cancel: Option<TapCancelCallback>,
}

pub type TapDownCallback = Arc<dyn Fn(TapDownDetails) + Send + Sync>;
pub type TapUpCallback = Arc<dyn Fn(TapUpDetails) + Send + Sync>;
pub type TapCallback = Arc<dyn Fn() + Send + Sync>;
pub type TapCancelCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone)]
pub struct TapDownDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerDeviceKind,
}

#[derive(Debug, Clone)]
pub struct TapUpDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerDeviceKind,
}

impl TapGestureRecognizer {
    pub fn new(router: Arc<PointerRouter>) -> Self {
        Self {
            base: OneSequenceGestureRecognizer::new(router),
            callbacks: Arc::new(Mutex::new(TapCallbacks {
                on_tap_down: None,
                on_tap_up: None,
                on_tap: None,
                on_tap_cancel: None,
            })),
        }
    }

    pub fn on_tap_down(&self, callback: TapDownCallback) {
        self.callbacks.lock().on_tap_down = Some(callback);
    }

    pub fn on_tap_up(&self, callback: TapUpCallback) {
        self.callbacks.lock().on_tap_up = Some(callback);
    }

    pub fn on_tap(&self, callback: TapCallback) {
        self.callbacks.lock().on_tap = Some(callback);
    }

    pub fn on_tap_cancel(&self, callback: TapCancelCallback) {
        self.callbacks.lock().on_tap_cancel = Some(callback);
    }

    fn handle_tap_down(&self, event: &PointerDownEvent) {
        let callbacks = self.callbacks.lock();

        if let Some(ref cb) = callbacks.on_tap_down {
            cb(TapDownDetails {
                global_position: event.position,
                local_position: event.local_position,
                kind: event.kind,
            });
        }
    }

    fn handle_tap_up(&self, event: &PointerUpEvent) {
        let callbacks = self.callbacks.lock();

        if let Some(ref cb) = callbacks.on_tap_up {
            cb(TapUpDetails {
                global_position: event.position,
                local_position: event.local_position,
                kind: event.kind,
            });
        }

        if let Some(ref cb) = callbacks.on_tap {
            cb();
        }

        // Accept gesture
        self.base.resolve(GestureDisposition::Accepted);
    }

    fn handle_tap_cancel(&self) {
        let callbacks = self.callbacks.lock();

        if let Some(ref cb) = callbacks.on_tap_cancel {
            cb();
        }

        // Reject gesture
        self.base.resolve(GestureDisposition::Rejected);
    }
}

impl GestureRecognizer for TapGestureRecognizer {
    fn add_pointer(&self, event: &PointerDownEvent) {
        self.base.add_pointer(event);
        self.handle_tap_down(event);
    }

    fn handle_event(&self, event: &PointerEvent) {
        match event {
            PointerEvent::Up(up_event) => {
                if self.base.primary_pointer() == Some(PointerId::new(up_event.pointer)) {
                    self.handle_tap_up(up_event);
                }
            }
            PointerEvent::Cancel(_) => {
                self.handle_tap_cancel();
            }
            PointerEvent::Move(move_event) => {
                // Check if moved too far
                if let Some(initial) = self.base.initial_position() {
                    let delta = move_event.position - initial;
                    if delta.distance() > TAP_SLOP {
                        self.handle_tap_cancel();
                    }
                }
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.base.dispose();
        self.callbacks.lock().on_tap_down = None;
        self.callbacks.lock().on_tap_up = None;
        self.callbacks.lock().on_tap = None;
        self.callbacks.lock().on_tap_cancel = None;
    }
}

impl GestureArenaMember for TapGestureRecognizer {
    fn accept_gesture(&self, pointer: PointerId) {
        self.base.accept_gesture(pointer);
    }

    fn reject_gesture(&self, pointer: PointerId) {
        self.base.reject_gesture(pointer);
        self.handle_tap_cancel();
    }
}

/// Maximum distance pointer can move before tap is cancelled
const TAP_SLOP: f64 = 18.0;
```

### 2. DragGestureRecognizer (Vertical/Horizontal/Pan)

```rust
// In flui_gestures/src/recognizers/drag.rs

/// Recognizes drag gestures (vertical, horizontal, or pan)
#[derive(Clone)]
pub struct DragGestureRecognizer {
    base: OneSequenceGestureRecognizer,
    callbacks: Arc<Mutex<DragCallbacks>>,
    axis: DragAxis,
    min_fling_distance: f64,
    min_fling_velocity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAxis {
    Vertical,
    Horizontal,
    Free,  // Pan
}

struct DragCallbacks {
    on_start: Option<DragStartCallback>,
    on_update: Option<DragUpdateCallback>,
    on_end: Option<DragEndCallback>,
    on_cancel: Option<DragCancelCallback>,
}

pub type DragStartCallback = Arc<dyn Fn(DragStartDetails) + Send + Sync>;
pub type DragUpdateCallback = Arc<dyn Fn(DragUpdateDetails) + Send + Sync>;
pub type DragEndCallback = Arc<dyn Fn(DragEndDetails) + Send + Sync>;
pub type DragCancelCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone)]
pub struct DragStartDetails {
    pub global_position: Offset,
    pub local_position: Offset,
}

#[derive(Debug, Clone)]
pub struct DragUpdateDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub delta: Offset,
    pub primary_delta: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct DragEndDetails {
    pub velocity: Velocity,
    pub primary_velocity: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Velocity {
    pub pixels_per_second: Offset,
}

impl DragGestureRecognizer {
    pub fn vertical(router: Arc<PointerRouter>) -> Self {
        Self::new(router, DragAxis::Vertical)
    }

    pub fn horizontal(router: Arc<PointerRouter>) -> Self {
        Self::new(router, DragAxis::Horizontal)
    }

    pub fn pan(router: Arc<PointerRouter>) -> Self {
        Self::new(router, DragAxis::Free)
    }

    fn new(router: Arc<PointerRouter>, axis: DragAxis) -> Self {
        Self {
            base: OneSequenceGestureRecognizer::new(router),
            callbacks: Arc::new(Mutex::new(DragCallbacks {
                on_start: None,
                on_update: None,
                on_end: None,
                on_cancel: None,
            })),
            axis,
            min_fling_distance: 50.0,
            min_fling_velocity: 50.0,
        }
    }

    pub fn on_start(&self, callback: DragStartCallback) {
        self.callbacks.lock().on_start = Some(callback);
    }

    pub fn on_update(&self, callback: DragUpdateCallback) {
        self.callbacks.lock().on_update = Some(callback);
    }

    pub fn on_end(&self, callback: DragEndCallback) {
        self.callbacks.lock().on_end = Some(callback);
    }

    pub fn on_cancel(&self, callback: DragCancelCallback) {
        self.callbacks.lock().on_cancel = Some(callback);
    }

    fn is_fling(&self, velocity: &Velocity) -> bool {
        let speed = velocity.pixels_per_second.distance();
        speed > self.min_fling_velocity
    }

    fn resolve_axis_delta(&self, delta: Offset) -> Option<f64> {
        match self.axis {
            DragAxis::Vertical => Some(delta.dy),
            DragAxis::Horizontal => Some(delta.dx),
            DragAxis::Free => None,
        }
    }
}

// Implementation similar to TapGestureRecognizer...
```

### 3. ScaleGestureRecognizer (Pinch/Zoom)

```rust
// In flui_gestures/src/recognizers/scale.rs

/// Recognizes scale gestures (pinch to zoom)
#[derive(Clone)]
pub struct ScaleGestureRecognizer {
    base: OneSequenceGestureRecognizer,
    callbacks: Arc<Mutex<ScaleCallbacks>>,
    pointers: Arc<Mutex<HashMap<PointerId, Offset>>>,
}

struct ScaleCallbacks {
    on_start: Option<ScaleStartCallback>,
    on_update: Option<ScaleUpdateCallback>,
    on_end: Option<ScaleEndCallback>,
}

pub type ScaleStartCallback = Arc<dyn Fn(ScaleStartDetails) + Send + Sync>;
pub type ScaleUpdateCallback = Arc<dyn Fn(ScaleUpdateDetails) + Send + Sync>;
pub type ScaleEndCallback = Arc<dyn Fn(ScaleEndDetails) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ScaleStartDetails {
    pub focal_point: Offset,
    pub local_focal_point: Offset,
    pub pointer_count: usize,
}

#[derive(Debug, Clone)]
pub struct ScaleUpdateDetails {
    pub focal_point: Offset,
    pub local_focal_point: Offset,
    pub scale: f64,
    pub horizontal_scale: f64,
    pub vertical_scale: f64,
    pub rotation: f64,
    pub pointer_count: usize,
}

#[derive(Debug, Clone)]
pub struct ScaleEndDetails {
    pub velocity: Velocity,
    pub pointer_count: usize,
}

// Implementation that tracks multiple pointers and calculates:
// - Focal point (average of all pointer positions)
// - Scale (change in distance between pointers)
// - Rotation (change in angle between pointers)
```

### 4. LongPressGestureRecognizer

```rust
// In flui_gestures/src/recognizers/long_press.rs

/// Recognizes long press gestures
#[derive(Clone)]
pub struct LongPressGestureRecognizer {
    base: OneSequenceGestureRecognizer,
    callbacks: Arc<Mutex<LongPressCallbacks>>,
    duration: Duration,
    timer: Arc<Mutex<Option<JoinHandle<()>>>>,
}

struct LongPressCallbacks {
    on_long_press_down: Option<LongPressDownCallback>,
    on_long_press_start: Option<LongPressStartCallback>,
    on_long_press_move_update: Option<LongPressMoveUpdateCallback>,
    on_long_press_up: Option<LongPressUpCallback>,
    on_long_press_end: Option<LongPressEndCallback>,
    on_long_press: Option<LongPressCallback>,
}

pub type LongPressDownCallback = Arc<dyn Fn(LongPressDownDetails) + Send + Sync>;
pub type LongPressStartCallback = Arc<dyn Fn(LongPressStartDetails) + Send + Sync>;
pub type LongPressMoveUpdateCallback = Arc<dyn Fn(LongPressMoveUpdateDetails) + Send + Sync>;
pub type LongPressUpCallback = Arc<dyn Fn() + Send + Sync>;
pub type LongPressEndCallback = Arc<dyn Fn(LongPressEndDetails) + Send + Sync>;
pub type LongPressCallback = Arc<dyn Fn() + Send + Sync>;

// Long press holds the arena to prevent other gestures from winning
// until the timer fires
```

---

## Gesture Widgets

### 1. GestureDetector (Main Gesture Widget)

```rust
// In flui_widgets/src/gestures/gesture_detector.rs

/// High-level widget for detecting gestures
///
/// GestureDetector creates and manages GestureRecognizer objects internally.
/// It automatically disposes them when the widget is removed.
#[derive(Debug)]
pub struct GestureDetector {
    // Tap gestures
    on_tap: Option<TapCallback>,
    on_tap_down: Option<TapDownCallback>,
    on_tap_up: Option<TapUpCallback>,
    on_tap_cancel: Option<TapCancelCallback>,

    // Double tap
    on_double_tap: Option<DoubleTapCallback>,

    // Long press
    on_long_press: Option<LongPressCallback>,
    on_long_press_start: Option<LongPressStartCallback>,
    on_long_press_up: Option<LongPressUpCallback>,

    // Drag gestures
    on_vertical_drag_start: Option<DragStartCallback>,
    on_vertical_drag_update: Option<DragUpdateCallback>,
    on_vertical_drag_end: Option<DragEndCallback>,

    on_horizontal_drag_start: Option<DragStartCallback>,
    on_horizontal_drag_update: Option<DragUpdateCallback>,
    on_horizontal_drag_end: Option<DragEndCallback>,

    on_pan_start: Option<DragStartCallback>,
    on_pan_update: Option<DragUpdateCallback>,
    on_pan_end: Option<DragEndCallback>,

    // Scale gestures
    on_scale_start: Option<ScaleStartCallback>,
    on_scale_update: Option<ScaleUpdateCallback>,
    on_scale_end: Option<ScaleEndCallback>,

    // Behavior
    behavior: HitTestBehavior,
    exclude_from_semantics: bool,

    // Child
    child: Option<AnyElement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTestBehavior {
    /// Only hit test if the widget itself is not transparent
    Deferring,

    /// Always participate in hit testing (even if transparent)
    Opaque,

    /// Only participate if a child is hit
    Translucent,
}

impl GestureDetector {
    pub fn builder() -> GestureDetectorBuilder {
        GestureDetectorBuilder::default()
    }
}

pub struct GestureDetectorBuilder {
    detector: GestureDetector,
}

impl GestureDetectorBuilder {
    pub fn on_tap(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.detector.on_tap = Some(Arc::new(callback));
        self
    }

    pub fn on_tap_down(mut self, callback: impl Fn(TapDownDetails) + Send + Sync + 'static) -> Self {
        self.detector.on_tap_down = Some(Arc::new(callback));
        self
    }

    pub fn on_long_press(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.detector.on_long_press = Some(Arc::new(callback));
        self
    }

    pub fn on_vertical_drag_update(
        mut self,
        callback: impl Fn(DragUpdateDetails) + Send + Sync + 'static,
    ) -> Self {
        self.detector.on_vertical_drag_update = Some(Arc::new(callback));
        self
    }

    pub fn on_scale_update(
        mut self,
        callback: impl Fn(ScaleUpdateDetails) + Send + Sync + 'static,
    ) -> Self {
        self.detector.on_scale_update = Some(Arc::new(callback));
        self
    }

    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.detector.behavior = behavior;
        self
    }

    pub fn child(mut self, child: AnyElement) -> Self {
        self.detector.child = Some(child);
        self
    }

    pub fn build(self) -> GestureDetector {
        self.detector
    }
}

impl View for GestureDetector {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Get router from context
        let router = ctx.pointer_router();

        // Create recognizers based on callbacks
        let mut recognizers: Vec<Arc<dyn GestureRecognizer>> = Vec::new();

        // Tap recognizer
        if self.on_tap.is_some()
            || self.on_tap_down.is_some()
            || self.on_tap_up.is_some()
            || self.on_tap_cancel.is_some()
        {
            let tap = Arc::new(TapGestureRecognizer::new(router.clone()));

            if let Some(cb) = self.on_tap {
                tap.on_tap(cb);
            }
            if let Some(cb) = self.on_tap_down {
                tap.on_tap_down(cb);
            }
            if let Some(cb) = self.on_tap_up {
                tap.on_tap_up(cb);
            }
            if let Some(cb) = self.on_tap_cancel {
                tap.on_tap_cancel(cb);
            }

            recognizers.push(tap);
        }

        // Drag recognizers (similar pattern)
        // ...

        // Create RawGestureDetector with recognizers
        RawGestureDetector::new(recognizers, self.child)
    }
}
```

### 2. RawGestureDetector (Low-Level)

```rust
// In flui_widgets/src/gestures/raw_gesture_detector.rs

/// Low-level gesture detector that manages recognizers manually
///
/// Use this when you need fine-grained control over recognizer lifecycle.
#[derive(Debug)]
pub struct RawGestureDetector {
    recognizers: Vec<Arc<dyn GestureRecognizer>>,
    behavior: HitTestBehavior,
    child: Option<AnyElement>,
}

impl RawGestureDetector {
    pub fn new(recognizers: Vec<Arc<dyn GestureRecognizer>>, child: Option<AnyElement>) -> Self {
        Self {
            recognizers,
            behavior: HitTestBehavior::Deferring,
            child,
        }
    }

    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }
}

impl View for RawGestureDetector {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create RenderPointerListener that:
        // 1. Implements HitTestTarget
        // 2. Forwards pointer events to recognizers
        // 3. Manages recognizer lifecycle

        (
            RenderPointerListener::new(self.recognizers, self.behavior),
            self.child,
        )
    }
}

// In flui_rendering/src/objects/render_pointer_listener.rs

pub struct RenderPointerListener {
    recognizers: Vec<Arc<dyn GestureRecognizer>>,
    behavior: HitTestBehavior,
}

impl RenderPointerListener {
    pub fn new(
        recognizers: Vec<Arc<dyn GestureRecognizer>>,
        behavior: HitTestBehavior,
    ) -> Self {
        Self {
            recognizers,
            behavior,
        }
    }
}

impl SingleRender for RenderPointerListener {
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        tree.paint_child(child_id, offset)
    }
}

impl HitTestTarget for RenderPointerListener {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        match event {
            PointerEvent::Down(down) => {
                for recognizer in &self.recognizers {
                    recognizer.add_pointer(down);
                }
            }
            _ => {
                for recognizer in &self.recognizers {
                    recognizer.handle_event(event);
                }
            }
        }
    }
}

impl Drop for RenderPointerListener {
    fn drop(&mut self) {
        // Dispose all recognizers
        for recognizer in &self.recognizers {
            recognizer.dispose();
        }
    }
}
```

---

## Implementation Plan

### Phase 1: Core Infrastructure (~800 LOC)

**Location:** `crates/flui_gestures/src/`

1. **arena.rs** (~250 LOC)
   - `GestureArenaMember` trait
   - `GestureArenaManager` struct
   - `GestureArena` internal struct
   - `GestureArenaEntry` handle

2. **pointer_router.rs** (~150 LOC)
   - `PointerRouter` struct
   - Route registration/removal
   - Event dispatch

3. **recognizer.rs** (~200 LOC)
   - `GestureRecognizer` trait
   - `OneSequenceGestureRecognizer` base class
   - `GestureRecognizerState` enum

4. **types.rs** (~200 LOC)
   - Gesture detail types (TapDownDetails, DragUpdateDetails, etc.)
   - Velocity calculation
   - Common constants (TAP_SLOP, etc.)

**Total Phase 1:** ~800 LOC

### Phase 2: Hit Testing (~200 LOC)

**Location:** `crates/flui_core/src/foundation/`

5. **hit_test.rs** (~200 LOC)
   - `HitTestable` trait
   - `HitTestTarget` trait
   - `HitTestResult` struct
   - `HitTestEntry` struct

**Total Phase 2:** ~200 LOC

### Phase 3: Gesture Recognizers (~1,000 LOC)

**Location:** `crates/flui_gestures/src/recognizers/`

6. **tap.rs** (~200 LOC)
   - `TapGestureRecognizer`
   - `DoubleTapGestureRecognizer`

7. **long_press.rs** (~150 LOC)
   - `LongPressGestureRecognizer`

8. **drag.rs** (~300 LOC)
   - `DragGestureRecognizer` (base)
   - `VerticalDragGestureRecognizer`
   - `HorizontalDragGestureRecognizer`
   - `PanGestureRecognizer`

9. **scale.rs** (~200 LOC)
   - `ScaleGestureRecognizer`

10. **multi_tap.rs** (~150 LOC)
    - `MultiTapGestureRecognizer` (for multi-touch)

**Total Phase 3:** ~1,000 LOC

### Phase 4: Gesture Widgets (~800 LOC)

**Location:** `crates/flui_widgets/src/gestures/`

11. **gesture_detector.rs** (~400 LOC)
    - `GestureDetector` widget
    - `GestureDetectorBuilder`

12. **raw_gesture_detector.rs** (~150 LOC)
    - `RawGestureDetector` widget

13. **render_pointer_listener.rs** (~150 LOC)
    - `RenderPointerListener` render object
    - HitTestTarget implementation

14. **ink_well.rs** (~100 LOC)
    - `InkWell` widget (Material ripple effect)

**Total Phase 4:** ~800 LOC

### Phase 5: Testing & Documentation (~500 LOC)

15. **tests/** (~400 LOC)
    - Arena conflict resolution tests
    - Recognizer accuracy tests
    - Multi-gesture tests
    - Hit testing tests

16. **examples/** (~100 LOC)
    - Basic tap example
    - Drag example
    - Scale example
    - Complex gestures example

**Total Phase 5:** ~500 LOC

---

## Usage Examples

### Example 1: Basic Tap Detection

```rust
use flui_gestures::*;
use flui_widgets::*;

#[derive(Debug)]
struct TapDemo;

impl View for TapDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        GestureDetector::builder()
            .on_tap(|| {
                println!("Tapped!");
            })
            .on_tap_down(|details| {
                println!("Tap down at: {:?}", details.local_position);
            })
            .child(Box::new(
                Container::new()
                    .width(200.0)
                    .height(200.0)
                    .color(Color::BLUE),
            ))
            .build()
    }
}
```

### Example 2: Vertical Drag

```rust
use flui_gestures::*;
use flui_widgets::*;

#[derive(Debug)]
struct DragDemo;

impl View for DragDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let offset = use_signal(ctx, Offset::zero());

        GestureDetector::builder()
            .on_vertical_drag_update({
                let offset = offset.clone();
                move |details| {
                    offset.update(|o| {
                        o.dy += details.delta.dy;
                    });
                }
            })
            .child(Box::new(
                Transform::translate(
                    offset.get(),
                    Some(Box::new(
                        Container::new()
                            .width(100.0)
                            .height(100.0)
                            .color(Color::RED),
                    )),
                ),
            ))
            .build()
    }
}
```

### Example 3: Pinch to Zoom

```rust
use flui_gestures::*;
use flui_widgets::*;

#[derive(Debug)]
struct ScaleDemo;

impl View for ScaleDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let scale = use_signal(ctx, 1.0);

        GestureDetector::builder()
            .on_scale_update({
                let scale = scale.clone();
                move |details| {
                    scale.set(details.scale);
                }
            })
            .child(Box::new(
                Transform::scale(
                    scale.get(),
                    Some(Box::new(
                        Container::new()
                            .width(200.0)
                            .height(200.0)
                            .color(Color::GREEN),
                    )),
                ),
            ))
            .build()
    }
}
```

### Example 4: Long Press

```rust
use flui_gestures::*;
use flui_widgets::*;

#[derive(Debug)]
struct LongPressDemo;

impl View for LongPressDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let is_pressed = use_signal(ctx, false);

        GestureDetector::builder()
            .on_long_press({
                let is_pressed = is_pressed.clone();
                move || {
                    is_pressed.set(true);
                }
            })
            .child(Box::new(
                Container::new()
                    .width(200.0)
                    .height(200.0)
                    .color(if is_pressed.get() {
                        Color::RED
                    } else {
                        Color::BLUE
                    }),
            ))
            .build()
    }
}
```

### Example 5: Custom Recognizer with RawGestureDetector

```rust
use flui_gestures::*;
use flui_widgets::*;

#[derive(Debug)]
struct CustomGestureDemo {
    router: Arc<PointerRouter>,
}

impl View for CustomGestureDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create custom recognizers
        let tap = Arc::new(TapGestureRecognizer::new(self.router.clone()));
        tap.on_tap(Arc::new(|| println!("Custom tap!")));

        let long_press = Arc::new(LongPressGestureRecognizer::new(
            self.router.clone(),
            Duration::from_secs(2),
        ));
        long_press.on_long_press(Arc::new(|| println!("Custom long press!")));

        // Use RawGestureDetector for manual control
        RawGestureDetector::new(
            vec![tap as Arc<dyn GestureRecognizer>, long_press],
            Some(Box::new(
                Container::new()
                    .width(200.0)
                    .height(200.0)
                    .color(Color::PURPLE),
            )),
        )
    }
}
```

---

## Testing Strategy

### Unit Tests

1. **Gesture Arena:**
   - Test single member (auto-win)
   - Test multiple members (first accept wins)
   - Test eager winner
   - Test hold/release
   - Test sweep

2. **Recognizers:**
   - Test tap detection (down → up)
   - Test tap cancel (moved too far)
   - Test double tap timing
   - Test long press timer
   - Test drag threshold
   - Test scale calculation

3. **Pointer Router:**
   - Test route registration
   - Test event dispatch
   - Test route removal

4. **Hit Testing:**
   - Test simple hit
   - Test miss
   - Test transformed hit
   - Test nested hit testing

### Integration Tests

1. **Gesture Conflicts:**
   - Tap vs Long Press
   - Vertical Drag vs Horizontal Drag
   - Pan vs Scale
   - Multiple simultaneous gestures

2. **Widget Lifecycle:**
   - Create recognizers
   - Dispose on widget removal
   - Rebuild with same recognizers
   - Memory leak check

3. **Performance:**
   - Benchmark arena resolution
   - Test 100+ simultaneous pointers
   - Measure hit test overhead

---

## Crate Dependencies

```toml
# crates/flui_gestures/Cargo.toml

[package]
name = "flui_gestures"
version = "0.1.0"
edition = "2021"

[dependencies]
flui_core = { path = "../flui_core" }
flui_types = { path = "../flui_types" }
parking_lot = "0.12"
thiserror = "1.0"
tokio = { version = "1.43", features = ["time", "sync"] }

[dev-dependencies]
tokio = { version = "1.43", features = ["full", "test-util"] }
```

```toml
# crates/flui_widgets/Cargo.toml (add gestures dependency)

[dependencies]
flui_gestures = { path = "../flui_gestures" }
# ... existing dependencies ...
```

---

## Open Questions

1. **Pointer Capture:**
   - Should we support explicit pointer capture?
   - How do we handle pointer capture across windows?

2. **Platform Integration:**
   - How do we integrate with native gesture recognizers (iOS, Android)?
   - Should we use platform-specific recognizers when available?

3. **Accessibility:**
   - How do we expose gestures to screen readers?
   - Should we provide alternative input methods (keyboard)?

4. **Advanced Gestures:**
   - Should we support custom gesture recognizers?
   - Should we support gesture sequences (tap then drag)?
   - Should we support gesture paths (swipe patterns)?

---

## Version History

| Version | Date       | Author | Changes                        |
|---------|------------|--------|--------------------------------|
| 0.1.0   | 2025-11-10 | Claude | Initial gestures architecture  |

---

## References

- [Flutter GestureDetector API](https://api.flutter.dev/flutter/widgets/GestureDetector-class.html)
- [Flutter GestureRecognizer API](https://api.flutter.dev/flutter/gestures/GestureRecognizer-class.html)
- [Flutter Gesture Arena](https://api.flutter.dev/flutter/gestures/GestureArenaManager-class.html)
- [Flutter Deep Dive: Gestures](https://medium.com/flutter-community/flutter-deep-dive-gestures-c16203b3434f)
- [Handling Gestures in Flutter](https://blog.logrocket.com/handling-gestures-flutter-gesturedetector/)

---

## Conclusion

This architecture provides a **complete, Flutter-accurate gesture recognition system** for FLUI:

✅ **Gesture Arena** for conflict resolution when multiple recognizers compete
✅ **Persistent recognizers** (TapGestureRecognizer, DragGestureRecognizer, etc.) in `flui_gestures`
✅ **Pointer router** for efficient event dispatch
✅ **Hit testing** infrastructure in `flui_core/foundation`
✅ **Gesture widgets** (GestureDetector, RawGestureDetector) in `flui_widgets`
✅ **Type-safe callbacks** with specific detail types
✅ **Memory safe** with proper disposal

**Key Architectural Patterns:**

1. **Persistent Objects**: GestureRecognizers are long-lived, surviving widget rebuilds
2. **Arena-Based Conflict Resolution**: Automatic handling of gesture conflicts
3. **Composable**: Multiple recognizers can coexist, arena decides winner
4. **Efficient**: Pointer router minimizes event dispatch overhead
5. **Flexible**: Both high-level (GestureDetector) and low-level (RawGestureDetector) APIs

**Estimated Total Work:** ~3,300 LOC (800 core + 200 hit test + 1,000 recognizers + 800 widgets + 500 tests/examples)

This provides a solid foundation for FLUI's gesture system! 👆
