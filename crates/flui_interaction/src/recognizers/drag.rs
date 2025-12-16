//! Drag gesture recognizer
//!
//! Recognizes drag gestures (pointer down + move).
//!
//! Supports three types of drag:
//! - **Vertical**: Movement constrained to vertical axis
//! - **Horizontal**: Movement constrained to horizontal axis
//! - **Pan**: Free movement in any direction
//!
//! Flutter reference: https://api.flutter.dev/flutter/gestures/DragGestureRecognizer-class.html

use super::recognizer::{GestureRecognizer, GestureRecognizerState};
use crate::arena::GestureArenaMember;
use crate::events::{PointerEvent, PointerEventExt, PointerType};
use crate::ids::PointerId;
use crate::processing::VelocityTracker;
use crate::settings::GestureSettings;
use flui_types::Offset;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;

/// Drag axis constraint
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAxis {
    /// Vertical drag only (up/down)
    Vertical,
    /// Horizontal drag only (left/right)
    Horizontal,
    /// Free drag (any direction)
    Free,
}

/// Details about drag down (pointer contact before drag starts)
#[derive(Debug, Clone, PartialEq)]
pub struct DragDownDetails {
    /// Global position where pointer contacted the screen
    pub global_position: Offset,
    /// Local position (relative to widget)
    pub local_position: Offset,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about drag start
#[derive(Debug, Clone)]
pub struct DragStartDetails {
    /// Global position where drag started
    pub global_position: Offset,
    /// Local position (relative to widget)
    pub local_position: Offset,
    /// Pointer device kind
    pub kind: PointerType,
    /// When the drag started
    pub timestamp: Instant,
}

/// Details about drag update
#[derive(Debug, Clone, PartialEq)]
pub struct DragUpdateDetails {
    /// Current global position
    pub global_position: Offset,
    /// Current local position
    pub local_position: Offset,
    /// Delta since last update
    pub delta: Offset,
    /// Total delta since drag started
    pub primary_delta: f32,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about drag end
#[derive(Debug, Clone, PartialEq)]
pub struct DragEndDetails {
    /// Velocity at end of drag (pixels per second)
    pub velocity: Velocity,
    /// Final global position
    pub global_position: Offset,
    /// Final local position
    pub local_position: Offset,
    /// Primary velocity (axis-aligned)
    pub primary_velocity: f32,
}

// Re-export Velocity from the velocity module
pub use crate::processing::Velocity;

/// Callback types for drag events
pub type DragDownCallback = Arc<dyn Fn(DragDownDetails) + Send + Sync>;
pub type DragStartCallback = Arc<dyn Fn(DragStartDetails) + Send + Sync>;
pub type DragUpdateCallback = Arc<dyn Fn(DragUpdateDetails) + Send + Sync>;
pub type DragEndCallback = Arc<dyn Fn(DragEndDetails) + Send + Sync>;
pub type DragCancelCallback = Arc<dyn Fn() + Send + Sync>;

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
    state: GestureRecognizerState,

    /// Drag axis constraint
    axis: DragAxis,

    /// Callbacks
    callbacks: Arc<Mutex<DragCallbacks>>,

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
            .field("drag_state", &*self.drag_state.lock())
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

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
    /// Last update position
    last_position: Option<Offset>,
    /// Last update time (for velocity calculation)
    last_time: Option<Instant>,
    /// Total delta since start
    total_delta: Offset,
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
            last_position: None,
            last_time: None,
            total_delta: Offset::ZERO,
            velocity_tracker: VelocityTracker::new(),
        }
    }
}

impl DragGestureRecognizer {
    /// Create a new drag recognizer with gesture arena and axis constraint
    pub fn new(arena: crate::arena::GestureArena, axis: DragAxis) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            axis,
            callbacks: Arc::new(Mutex::new(DragCallbacks::default())),
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
            state: GestureRecognizerState::new(arena),
            axis,
            callbacks: Arc::new(Mutex::new(DragCallbacks::default())),
            drag_state: Arc::new(Mutex::new(DragState::default())),
            settings: Arc::new(Mutex::new(settings)),
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

    /// Get the minimum drag distance from settings
    fn min_drag_distance(&self) -> f32 {
        self.settings.lock().pan_slop()
    }

    /// Get the minimum fling velocity from settings
    fn min_fling_velocity(&self) -> f32 {
        self.settings.lock().min_fling_velocity()
    }

    /// Set the drag down callback (called on pointer contact before drag starts)
    ///
    /// This is called when a pointer contacts the screen with a primary button
    /// and might begin to move. Unlike `on_start`, this is called before any
    /// movement threshold is met.
    pub fn with_on_down(
        self: Arc<Self>,
        callback: impl Fn(DragDownDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_down = Some(Arc::new(callback));
        self
    }

    /// Set the drag start callback
    pub fn with_on_start(
        self: Arc<Self>,
        callback: impl Fn(DragStartDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_start = Some(Arc::new(callback));
        self
    }

    /// Set the drag update callback
    pub fn with_on_update(
        self: Arc<Self>,
        callback: impl Fn(DragUpdateDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_update = Some(Arc::new(callback));
        self
    }

    /// Set the drag end callback
    pub fn with_on_end(
        self: Arc<Self>,
        callback: impl Fn(DragEndDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_end = Some(Arc::new(callback));
        self
    }

    /// Set the drag cancel callback
    pub fn with_on_cancel(
        self: Arc<Self>,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down - start tracking
    fn handle_down(&self, position: Offset, kind: PointerType) {
        let mut state = self.drag_state.lock();
        state.state = DragPhase::Possible;
        state.start_time = Some(Instant::now());
        state.last_position = Some(position);
        state.last_time = Some(Instant::now());
        state.total_delta = Offset::ZERO;
        state.velocity_tracker.reset();
        state
            .velocity_tracker
            .add_position(Instant::now(), position);
        drop(state); // Release lock before callback

        // Call on_down callback (pointer contact before drag starts)
        if let Some(callback) = self.callbacks.lock().on_down.clone() {
            let details = DragDownDetails {
                global_position: position,
                local_position: position,
                kind,
            };
            callback(details);
        }
    }

    /// Handle pointer move - check slop and start/update drag
    fn handle_move(&self, position: Offset, kind: PointerType) {
        let mut state = self.drag_state.lock();

        match state.state {
            DragPhase::Possible => {
                // Check if moved beyond slop
                if let Some(initial_pos) = self.state.initial_position() {
                    let delta = position - initial_pos;
                    let distance = self.calculate_primary_delta(delta);

                    if distance.abs() > self.min_drag_distance() {
                        // Start drag!
                        state.state = DragPhase::Started;
                        drop(state); // Release lock before calling callback

                        if let Some(callback) = self.callbacks.lock().on_start.clone() {
                            let details = DragStartDetails {
                                global_position: position,
                                local_position: position,
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
                    let delta = position - last_pos;
                    state.total_delta = state.total_delta + delta;
                    state.last_position = Some(position);
                    state.last_time = Some(Instant::now());
                    state
                        .velocity_tracker
                        .add_position(Instant::now(), position);

                    let primary_delta = self.calculate_primary_delta(state.total_delta);

                    drop(state); // Release lock before calling callback

                    if let Some(callback) = self.callbacks.lock().on_update.clone() {
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
    fn handle_up(&self, position: Offset, _kind: PointerType) {
        let mut state = self.drag_state.lock();

        if state.state == DragPhase::Started {
            // Calculate final velocity
            let velocity = state.velocity_tracker.velocity();
            let primary_velocity = self.calculate_primary_velocity(velocity.pixels_per_second);

            state.state = DragPhase::Ready;
            drop(state); // Release lock before calling callback

            if let Some(callback) = self.callbacks.lock().on_end.clone() {
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

            if let Some(callback) = self.callbacks.lock().on_cancel.clone() {
                callback();
            }

            self.state.reject();
        }
    }

    /// Calculate primary delta based on axis
    fn calculate_primary_delta(&self, delta: Offset) -> f32 {
        match self.axis {
            DragAxis::Vertical => delta.dy,
            DragAxis::Horizontal => delta.dx,
            DragAxis::Free => delta.distance(),
        }
    }

    /// Calculate primary velocity based on axis
    fn calculate_primary_velocity(&self, velocity: Offset) -> f32 {
        match self.axis {
            DragAxis::Vertical => velocity.dy,
            DragAxis::Horizontal => velocity.dx,
            DragAxis::Free => velocity.distance(),
        }
    }

    /// Check if velocity is sufficient for a fling gesture
    pub fn is_fling(&self, velocity: &Velocity) -> bool {
        let speed = velocity.pixels_per_second.distance();
        speed >= self.min_fling_velocity()
    }

    /// Extract position and pointer type from a PointerEvent
    fn extract_event_data(event: &PointerEvent) -> (Offset, PointerType) {
        let position = event.position();
        let pointer_type = match event {
            PointerEvent::Down(e) => e.pointer.pointer_type,
            PointerEvent::Up(e) => e.pointer.pointer_type,
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
    fn add_pointer(&self, pointer: PointerId, position: Offset) {
        // Start tracking this pointer
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);

        // Handle pointer down
        self.handle_down(position, PointerType::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // Only process if we're tracking a pointer
        if self.state.primary_pointer().is_none() {
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
        let mut callbacks = self.callbacks.lock();
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
    use crate::arena::GestureArena;
    use crate::events::make_move_event;

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

        let pointer = PointerId::new(1);
        let start_pos = Offset::new(100.0, 100.0);

        // Start tracking
        recognizer.add_pointer(pointer, start_pos);

        // Move vertically beyond slop
        let moved_pos = Offset::new(100.0, 130.0); // 30px down
        let move_event = make_move_event(moved_pos, PointerType::Touch);
        recognizer.handle_event(&move_event);

        // Should have started
        assert!(*started.lock());

        // Move more
        let moved_pos2 = Offset::new(100.0, 150.0);
        let move_event2 = make_move_event(moved_pos2, PointerType::Touch);
        recognizer.handle_event(&move_event2);

        // Should have updated
        assert!(*updated.lock());
    }

    #[test]
    fn test_velocity_tracker() {
        let mut tracker = VelocityTracker::new();

        let start_time = Instant::now();
        let start_pos = Offset::new(0.0, 0.0);

        tracker.add_position(start_time, start_pos);

        // Simulate movement over 100ms
        let dt = std::time::Duration::from_millis(100);
        let end_pos = Offset::new(100.0, 0.0); // Moved 100px in 100ms = 1000 px/s

        tracker.add_position(start_time + dt, end_pos);

        let velocity = tracker.velocity();

        // Should be approximately 1000 px/s horizontally
        assert!(velocity.pixels_per_second.dx > 900.0);
        assert!(velocity.pixels_per_second.dx < 1100.0);
    }
}
