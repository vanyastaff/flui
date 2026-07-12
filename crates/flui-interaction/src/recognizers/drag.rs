//! Drag gesture recognizer
//!
//! Recognizes drag gestures (pointer down + move).
//!
//! Supports three types of drag:
//! - **Vertical**: Movement constrained to vertical axis
//! - **Horizontal**: Movement constrained to horizontal axis
//! - **Pan**: Free movement in any direction
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/DragGestureRecognizer-class.html>

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Instant};

use flui_types::{
    Offset,
    geometry::{PixelDelta, Pixels},
};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    settings::GestureSettings,
    traits::{DragAxis, PointerEventExtTrait},
};

/// Configures when the drag's initial position is reported.
///
/// Flutter parity: `gestures/recognizer.dart:48` `DragStartBehavior`.
///
/// - [`Down`](Self::Down): the initial position reported in
///   [`DragStartDetails`] is the pointer's position at the down event.
/// - [`Start`](Self::Start): the initial position is the pointer's position
///   when the recogniser crosses the slop threshold (i.e. the moment the
///   drag actually starts). This is the Flutter default and usually what
///   users expect for scrollable content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DragStartBehavior {
    /// Use the pointer's down position as the drag's initial position.
    Down,
    /// Use the position at slop-crossing as the drag's initial position.
    /// Flutter default — matches `DragGestureRecognizer.dragStartBehavior`.
    #[default]
    Start,
}

/// Details about drag down (pointer contact before drag starts)
#[derive(Debug, Clone, PartialEq)]
pub struct DragDownDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Global position where pointer contacted the screen
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about drag start
#[derive(Debug, Clone)]
pub struct DragStartDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Global position where drag started
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
    /// Pointer device kind
    pub kind: PointerType,
    /// When the drag started
    pub timestamp: Instant,
}

/// Details about drag update
#[derive(Debug, Clone, PartialEq)]
pub struct DragUpdateDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Current global position
    pub global_position: Offset<Pixels>,
    /// Current local position
    pub local_position: Offset<Pixels>,
    /// Delta since last update
    pub delta: Offset<PixelDelta>,
    /// Total delta since drag started
    pub primary_delta: f32,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about drag end
#[derive(Debug, Clone, PartialEq)]
pub struct DragEndDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Velocity at end of drag (pixels per second)
    pub velocity: Velocity,
    /// Final global position
    pub global_position: Offset<Pixels>,
    /// Final local position
    pub local_position: Offset<Pixels>,
    /// Primary velocity (axis-aligned)
    pub primary_velocity: f32,
}

// Re-export Velocity from the velocity module
pub use crate::processing::Velocity;

/// Callback fired when a pointer contacts the screen and might begin a drag.
pub type DragDownCallback = Arc<dyn Fn(DragDownDetails)>;
/// Callback fired when the drag is recognized (slop crossed and arena won).
pub type DragStartCallback = Arc<dyn Fn(DragStartDetails)>;
/// Callback fired for each pointer move while the drag is in progress.
pub type DragUpdateCallback = Arc<dyn Fn(DragUpdateDetails)>;
/// Callback fired when the pointer lifts and the drag completes.
pub type DragEndCallback = Arc<dyn Fn(DragEndDetails)>;
/// Callback fired when the gesture is cancelled (e.g. the arena rejects it).
pub type DragCancelCallback = Arc<dyn Fn()>;

/// Recognizes drag gestures
///
/// A drag is defined as:
/// - Pointer down
/// - Pointer moves beyond DRAG_SLOP (18px)
/// - Continuous movement until pointer up
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = DragGestureRecognizer::new(arena, DragAxis::Vertical)
///     .with_on_start(|details| {
///         println!("Drag started at {:?}", details.global_position);
///     })
///     .with_on_update(|details| {
///         println!("Dragged by {:?}", details.delta);
///     })
///     .with_on_end(|details| {
///         println!("Drag ended with velocity: {}", details.velocity.magnitude());
///     });
/// ```
#[derive(Clone)]
pub struct DragGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Drag axis constraint
    axis: DragAxis,

    /// When to fix the drag's initial position.
    ///
    /// - [`DragStartBehavior::Down`]: position is the down-event position.
    /// - [`DragStartBehavior::Start`]: position is the slop-crossing
    ///   position (Flutter default).
    start_behavior: DragStartBehavior,

    /// Callbacks
    callbacks: Rc<RefCell<DragCallbacks>>,

    /// Current drag state
    drag_state: Arc<Mutex<DragState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for DragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragGestureRecognizer")
            .field("state", &self.state)
            .field("axis", &self.axis)
            .field("start_behavior", &self.start_behavior)
            .field("drag_state", &*self.drag_state.lock())
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

// Field names keep Flutter's `onDragStart`-style callback names (parity).
#[allow(clippy::struct_field_names)]
#[derive(Default)]
struct DragCallbacks {
    on_down: Option<DragDownCallback>,
    on_start: Option<DragStartCallback>,
    on_update: Option<DragUpdateCallback>,
    on_end: Option<DragEndCallback>,
    on_cancel: Option<DragCancelCallback>,
}

#[derive(Debug, Clone)]
struct DragState {
    /// Current state
    state: DragPhase,
    /// When drag started
    start_time: Option<Instant>,
    /// Position reported in [`DragStartDetails`] — depends on
    /// `start_behavior` (down position or slop-crossing position).
    start_position: Option<Offset<Pixels>>,
    /// Last update position
    last_position: Option<Offset<Pixels>>,
    /// Last update time (for velocity calculation)
    last_time: Option<Instant>,
    /// Total delta since start
    total_delta: Offset<PixelDelta>,
    /// Velocity tracker
    velocity_tracker: VelocityTracker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragPhase {
    Ready,
    Possible, // Pointer down but haven't moved beyond slop yet
    Started,  // Drag in progress
    Cancelled,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            state: DragPhase::Ready,
            start_time: None,
            start_position: None,
            last_position: None,
            last_time: None,
            total_delta: Offset::new(PixelDelta::ZERO, PixelDelta::ZERO),
            velocity_tracker: VelocityTracker::new(),
        }
    }
}

impl DragGestureRecognizer {
    /// Create a new drag recognizer with gesture arena and axis constraint
    pub fn new(arena: crate::arena::GestureArena, axis: DragAxis) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            start_behavior: DragStartBehavior::default(),
            callbacks: Rc::new(RefCell::new(DragCallbacks::default())),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a new drag recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        axis: DragAxis,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            axis,
            start_behavior: DragStartBehavior::default(),
            callbacks: Rc::new(RefCell::new(DragCallbacks::default())),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            settings: Arc::new(Mutex::new(settings)),
        })
    }

    /// Configure when the drag's initial position is reported.
    ///
    /// See [`DragStartBehavior`] for the semantics. Default is
    /// [`DragStartBehavior::Start`].
    pub fn with_drag_start_behavior(self: Arc<Self>, behavior: DragStartBehavior) -> Arc<Self> {
        // Re-construct with the new behavior — fields are all `Copy`/Arc so
        // this is a cheap move, and it keeps the constructor pattern uniform.
        Arc::new(Self {
            start_behavior: behavior,
            ..(*self).clone()
        })
    }

    /// Get the current gesture settings
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Update gesture settings
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Drag axis this recogniser is bound to.
    pub fn axis(&self) -> DragAxis {
        self.axis
    }

    /// Currently-configured [`DragStartBehavior`].
    pub fn drag_start_behavior(&self) -> DragStartBehavior {
        self.start_behavior
    }

    /// Minimum drag distance for the current axis. Per-axis slop:
    /// - [`DragAxis::Vertical`][]: [`GestureSettings::pan_slop_vertical`]
    /// - [`DragAxis::Horizontal`][]: [`GestureSettings::pan_slop_horizontal`]
    /// - [`DragAxis::Free`][]: [`GestureSettings::pan_slop`]
    fn min_drag_distance(&self) -> f32 {
        let s = self.settings.lock();
        match self.axis {
            DragAxis::Vertical => s.pan_slop_vertical(),
            DragAxis::Horizontal => s.pan_slop_horizontal(),
            DragAxis::Free => s.pan_slop(),
        }
    }

    /// Get the minimum fling velocity from settings
    fn min_fling_velocity(&self) -> f32 {
        self.settings.lock().min_fling_velocity()
    }

    /// Set the drag down callback (called on pointer contact before drag
    /// starts)
    ///
    /// This is called when a pointer contacts the screen with a primary button
    /// and might begin to move. Unlike `on_start`, this is called before any
    /// movement threshold is met.
    pub fn with_on_down(
        self: Arc<Self>,
        callback: impl Fn(DragDownDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_down = Some(Arc::new(callback));
        self
    }

    /// Set the drag start callback
    pub fn with_on_start(
        self: Arc<Self>,
        callback: impl Fn(DragStartDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_start = Some(Arc::new(callback));
        self
    }

    /// Set the drag update callback
    pub fn with_on_update(
        self: Arc<Self>,
        callback: impl Fn(DragUpdateDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_update = Some(Arc::new(callback));
        self
    }

    /// Set the drag end callback
    pub fn with_on_end(self: Arc<Self>, callback: impl Fn(DragEndDetails) + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_end = Some(Arc::new(callback));
        self
    }

    /// Set the drag cancel callback
    pub fn with_on_cancel(self: Arc<Self>, callback: impl Fn() + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down - start tracking
    fn handle_down(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.drag_state.lock();
        state.state = DragPhase::Possible;
        state.start_time = Some(Instant::now());
        state.last_position = Some(position);
        state.last_time = Some(Instant::now());
        state.total_delta = Offset::new(PixelDelta::ZERO, PixelDelta::ZERO);
        state.velocity_tracker.reset();
        state
            .velocity_tracker
            .add_position(Instant::now(), position);
        drop(state); // Release lock before callback

        // Call on_down callback (pointer contact before drag starts)
        if let Some(callback) = self.callbacks.borrow().on_down.clone() {
            let details = DragDownDetails {
                global_position: position,
                local_position: position,
                kind,
            };
            callback(details);
        }
    }

    /// Handle pointer move - check slop and start/update drag
    fn handle_move(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.drag_state.lock();

        match state.state {
            DragPhase::Possible => {
                // Check if moved beyond slop
                if let Some(initial_pos) = self.state.initial_position() {
                    let delta = position - initial_pos;
                    let distance = self.calculate_primary_delta(delta);

                    if distance.abs() > self.min_drag_distance() {
                        // Start drag!
                        //
                        // The drag's initial position depends on
                        // `start_behavior`:
                        // - `Down`: the down-event position (`initial_pos`).
                        // - `Start`: the slop-crossing position
                        //   (`position`) — matches Flutter's default and
                        //   prevents the in-flight motion from being
                        //   counted as a "drag" before the user actually
                        //   committed to it.
                        let start_position = match self.start_behavior {
                            DragStartBehavior::Down => initial_pos,
                            DragStartBehavior::Start => position,
                        };
                        state.state = DragPhase::Started;
                        state.start_position = Some(start_position);
                        state.last_position = Some(position);
                        state.last_time = Some(Instant::now());
                        state
                            .velocity_tracker
                            .add_position(Instant::now(), position);
                        drop(state); // Release lock before calling callback

                        if let Some(callback) = self.callbacks.borrow().on_start.clone() {
                            let details = DragStartDetails {
                                global_position: start_position,
                                local_position: start_position,
                                kind,
                                timestamp: Instant::now(),
                            };
                            callback(details);
                        }
                    }
                }
            }
            DragPhase::Started => {
                // Update drag
                if let Some(last_pos) = state.last_position {
                    let delta = (position - last_pos).to_delta();
                    state.total_delta += delta;
                    state.last_position = Some(position);
                    state.last_time = Some(Instant::now());
                    state
                        .velocity_tracker
                        .add_position(Instant::now(), position);

                    let primary_delta = self.calculate_primary_delta(state.total_delta.to_pixels());

                    drop(state); // Release lock before calling callback

                    if let Some(callback) = self.callbacks.borrow().on_update.clone() {
                        let details = DragUpdateDetails {
                            global_position: position,
                            local_position: position,
                            delta,
                            primary_delta,
                            kind,
                        };
                        callback(details);
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle pointer up - end drag
    fn handle_up(&self, position: Offset<Pixels>, _kind: PointerType) {
        let mut state = self.drag_state.lock();

        if state.state == DragPhase::Started {
            // Calculate final velocity
            let velocity = state.velocity_tracker.get_velocity();
            let primary_velocity = self.calculate_primary_velocity(velocity.pixels_per_second);

            state.state = DragPhase::Ready;
            drop(state); // Release lock before calling callback

            if let Some(callback) = self.callbacks.borrow().on_end.clone() {
                let details = DragEndDetails {
                    velocity,
                    global_position: position,
                    local_position: position,
                    primary_velocity,
                };
                callback(details);
            }

            self.state.stop_tracking();
        } else {
            // Didn't start dragging - just cancel
            state.state = DragPhase::Ready;
        }
    }

    /// Handle cancel
    fn handle_cancel(&self) {
        let mut state = self.drag_state.lock();

        if state.state != DragPhase::Ready {
            state.state = DragPhase::Cancelled;
            drop(state);

            if let Some(callback) = self.callbacks.borrow().on_cancel.clone() {
                callback();
            }

            self.state.reject();
        }
    }

    /// Calculate primary delta based on axis
    fn calculate_primary_delta(&self, delta: Offset<Pixels>) -> f32 {
        match self.axis {
            DragAxis::Vertical => delta.dy.0,
            DragAxis::Horizontal => delta.dx.0,
            DragAxis::Free => delta.distance().0,
        }
    }

    /// Calculate primary velocity based on axis
    fn calculate_primary_velocity(&self, velocity: Offset<Pixels>) -> f32 {
        match self.axis {
            DragAxis::Vertical => velocity.dy.0,
            DragAxis::Horizontal => velocity.dx.0,
            DragAxis::Free => velocity.distance().0,
        }
    }

    /// Check if velocity is sufficient for a fling gesture
    pub fn is_fling(&self, velocity: &Velocity) -> bool {
        use flui_types::geometry::px;
        let speed = velocity.pixels_per_second.distance();
        speed >= px(self.min_fling_velocity())
    }

    /// Extract position and pointer type from a PointerEvent
    fn extract_event_data(event: &PointerEvent) -> (Offset<Pixels>, PointerType) {
        let position = event.position();
        let pointer_type = match event {
            PointerEvent::Down(e) | PointerEvent::Up(e) => e.pointer.pointer_type,
            PointerEvent::Move(e) => e.pointer.pointer_type,
            PointerEvent::Cancel(info) | PointerEvent::Enter(info) | PointerEvent::Leave(info) => {
                info.pointer_type
            }
            PointerEvent::Scroll(e) => e.pointer.pointer_type,
            PointerEvent::Gesture(e) => e.pointer.pointer_type,
        };
        (position, pointer_type)
    }
}

impl GestureRecognizer for DragGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // Start tracking this pointer
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);

        // Handle pointer down
        self.handle_down(position, PointerType::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Only process if we're tracking a pointer
        let Some(primary) = self.state.primary_pointer() else {
            return;
        };
        // Filter to the primary pointer we are tracking.
        if event.pointer_id() != primary {
            return;
        }

        let (position, pointer_type) = Self::extract_event_data(event);

        match event {
            PointerEvent::Move(_) => {
                self.handle_move(position, pointer_type);
            }
            PointerEvent::Up(_) => {
                self.handle_up(position, pointer_type);
            }
            PointerEvent::Cancel(_) => {
                self.handle_cancel();
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear tracked pointer (Flutter parity:
        // gestures/recognizer.dart:485-493 disposing GestureRecognizer
        // clears arena state for tracked pointers).
        self.state.reject();
        let mut callbacks = self.callbacks.borrow_mut();
        callbacks.on_down = None;
        callbacks.on_start = None;
        callbacks.on_update = None;
        callbacks.on_end = None;
        callbacks.on_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// Flutter parity: `monodrag.dart:81 sealed class DragGestureRecognizer
// extends OneSequenceGestureRecognizer`. Drag is OneSequence (NOT
// PrimaryPointer) — it tracks a single sequence but doesn't have the
// pre-acceptance deadline semantics of PrimaryPointer recognizers.

impl crate::recognizers::OneSequenceGestureRecognizer for DragGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, _pointer: PointerId, disposition: crate::arena::GestureDisposition) {
        match disposition {
            crate::arena::GestureDisposition::Accepted => {
                // No-op — Drag callbacks fire from event handlers, not arena
                // resolution. accept_gesture below mirrors.
            }
            crate::arena::GestureDisposition::Rejected => {
                self.state.reject();
            }
        }
    }

    fn stop_tracking_pointer(&self, _pointer: PointerId) {
        self.state.stop_tracking();
    }
}

impl GestureArenaMember for DragGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
        // Drag can continue
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the drag
        self.handle_cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{arena::GestureArena, events::make_move_event};

    #[test]
    fn test_drag_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = DragGestureRecognizer::new(arena, DragAxis::Vertical);

        assert_eq!(recognizer.primary_pointer(), None);
    }

    #[test]
    fn test_drag_recognizer_vertical() {
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let updated = Arc::new(Mutex::new(false));

        let started_clone = started.clone();
        let updated_clone = updated.clone();

        let recognizer = DragGestureRecognizer::new(arena, DragAxis::Vertical)
            .with_on_start(move |_details| {
                *started_clone.lock() = true;
            })
            .with_on_update(move |_details| {
                *updated_clone.lock() = true;
            });

        let pointer = PointerId::PRIMARY;
        let start_pos = Offset::new(Pixels(100.0), Pixels(100.0));

        // Start tracking
        recognizer.add_pointer(pointer, start_pos);

        // Move vertically beyond slop
        let moved_pos = Offset::new(Pixels(100.0), Pixels(130.0)); // 30px down
        let move_event = make_move_event(moved_pos, PointerType::Touch);
        recognizer.handle_event(&move_event);

        // Should have started
        assert!(*started.lock());

        // Move more
        let moved_pos2 = Offset::new(Pixels(100.0), Pixels(150.0));
        let move_event2 = make_move_event(moved_pos2, PointerType::Touch);
        recognizer.handle_event(&move_event2);

        // Should have updated
        assert!(*updated.lock());
    }

    #[test]
    fn test_velocity_tracker() {
        let mut tracker = VelocityTracker::new();

        // Flutter's `MIN_SAMPLE_SIZE` is 3 — the least-squares fit needs at
        // least three contiguous samples. We feed four for a clean linear
        // motion: 0→33→66→100 px over 0→33→66→100 ms → slope ~1000 px/s.
        let start_time = Instant::now();
        let dt = std::time::Duration::from_millis(33);
        for i in 0..=3 {
            let t = start_time + dt * i;
            let pos = Offset::new(Pixels(i as f32 * 33.0), Pixels(0.0));
            tracker.add_position(t, pos);
        }

        let velocity = tracker.get_velocity();

        // Should be approximately 1000 px/s horizontally
        assert!(velocity.pixels_per_second.dx > Pixels(900.0));
        assert!(velocity.pixels_per_second.dx < Pixels(1100.0));
    }

    // ========================================================================
    // H/V/Pan split tests
    //
    // Verifies Flutter parity for:
    // - per-axis slop (Vertical/Horizontal pick their own slop, Free uses
    //   the generic `pan_slop`),
    // - `DragStartBehavior::Down` vs `Start` (start_position differs).
    // ========================================================================

    #[test]
    fn drag_start_behavior_down_uses_down_position() {
        let arena = GestureArena::new();
        let start_reported = Arc::new(Mutex::new(None::<Offset<Pixels>>));

        let start_clone = start_reported.clone();
        let recognizer = DragGestureRecognizer::new(arena, DragAxis::Free)
            .with_drag_start_behavior(DragStartBehavior::Down)
            .with_on_start(move |d| {
                *start_clone.lock() = Some(d.global_position);
            });

        let pointer = PointerId::PRIMARY;
        let down_pos = Offset::new(Pixels(50.0), Pixels(50.0));
        recognizer.add_pointer(pointer, down_pos);

        // Cross slop with one big move (50→80 → 30px travel).
        let move_event =
            make_move_event(Offset::new(Pixels(80.0), Pixels(80.0)), PointerType::Touch);
        recognizer.handle_event(&move_event);

        // With `Down` behavior, the reported start position is the down
        // position, NOT the slop-crossing position.
        assert_eq!(*start_reported.lock(), Some(down_pos));
    }

    #[test]
    fn drag_start_behavior_start_uses_slop_crossing_position() {
        let arena = GestureArena::new();
        let start_reported = Arc::new(Mutex::new(None::<Offset<Pixels>>));

        let start_clone = start_reported.clone();
        let recognizer = DragGestureRecognizer::new(arena, DragAxis::Free)
            // Default is already Start; this makes the test explicit.
            .with_drag_start_behavior(DragStartBehavior::Start)
            .with_on_start(move |d| {
                *start_clone.lock() = Some(d.global_position);
            });

        let pointer = PointerId::PRIMARY;
        let down_pos = Offset::new(Pixels(50.0), Pixels(50.0));
        recognizer.add_pointer(pointer, down_pos);

        let crossing_pos = Offset::new(Pixels(80.0), Pixels(80.0));
        let move_event = make_move_event(crossing_pos, PointerType::Touch);
        recognizer.handle_event(&move_event);

        // With `Start` behavior, the reported start position is the
        // slop-crossing position itself.
        assert_eq!(*start_reported.lock(), Some(crossing_pos));
    }

    #[test]
    fn per_axis_vertical_uses_vertical_slop() {
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let started_clone = started.clone();

        // Tune vertical slop to 25px (greater than the default 18).
        let settings = GestureSettings::touch_defaults().with_pan_slop_vertical(25.0);
        let recognizer = DragGestureRecognizer::with_settings(arena, DragAxis::Vertical, settings)
            .with_on_start(move |_| {
                *started_clone.lock() = true;
            });

        let pointer = PointerId::PRIMARY;
        recognizer.add_pointer(pointer, Offset::new(Pixels(0.0), Pixels(0.0)));

        // 20px vertical move — under 25px vertical slop, no start yet.
        let move_event =
            make_move_event(Offset::new(Pixels(0.0), Pixels(20.0)), PointerType::Touch);
        recognizer.handle_event(&move_event);
        assert!(!*started.lock());

        // 30px vertical move — crosses 25px slop, drag starts.
        let move_event2 =
            make_move_event(Offset::new(Pixels(0.0), Pixels(30.0)), PointerType::Touch);
        recognizer.handle_event(&move_event2);
        assert!(*started.lock());
    }

    #[test]
    fn per_axis_horizontal_uses_horizontal_slop() {
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let started_clone = started.clone();

        // Tune horizontal slop to 10px. Vertical moves along the 50px
        // diagonal axis should NOT cross the horizontal slop (we measure
        // the horizontal projection of the delta).
        let settings = GestureSettings::touch_defaults().with_pan_slop_horizontal(10.0);
        let recognizer =
            DragGestureRecognizer::with_settings(arena, DragAxis::Horizontal, settings)
                .with_on_start(move |_| {
                    *started_clone.lock() = true;
                });

        let pointer = PointerId::PRIMARY;
        recognizer.add_pointer(pointer, Offset::new(Pixels(0.0), Pixels(0.0)));

        // Move 50px down, 5px right — horizontal projection (5px) is under
        // the 10px horizontal slop, no start.
        let move_event =
            make_move_event(Offset::new(Pixels(5.0), Pixels(50.0)), PointerType::Touch);
        recognizer.handle_event(&move_event);
        assert!(!*started.lock());

        // Move 15px right — crosses 10px slop on horizontal axis.
        let move_event2 =
            make_move_event(Offset::new(Pixels(15.0), Pixels(50.0)), PointerType::Touch);
        recognizer.handle_event(&move_event2);
        assert!(*started.lock());
    }

    #[test]
    fn drag_axis_getter_reflects_constructor() {
        let arena = GestureArena::new();
        assert_eq!(
            DragGestureRecognizer::new(arena.clone(), DragAxis::Vertical).axis(),
            DragAxis::Vertical,
        );
        assert_eq!(
            DragGestureRecognizer::new(arena.clone(), DragAxis::Horizontal).axis(),
            DragAxis::Horizontal,
        );
        assert_eq!(
            DragGestureRecognizer::new(arena, DragAxis::Free).axis(),
            DragAxis::Free,
        );
    }
}
