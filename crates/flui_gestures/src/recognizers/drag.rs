//! Drag gesture recognizer
//!
//! Recognizes drag gestures (pointer down + move).
//!
//! Supports three types of drag:
//! - **Vertical**: Movement constrained to vertical axis
//! - **Horizontal**: Movement constrained to horizontal axis
//! - **Pan**: Free movement in any direction

use super::recognizer::{constants, GestureRecognizer, GestureRecognizerState};
use crate::arena::{GestureArenaMember, PointerId};
use flui_types::{events::PointerEvent, Offset};
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

/// Details about drag start
#[derive(Debug, Clone)]
pub struct DragStartDetails {
    /// Global position where drag started
    pub global_position: Offset,
    /// Local position (relative to widget)
    pub local_position: Offset,
    /// Pointer device kind
    pub kind: flui_types::events::PointerDeviceKind,
    /// When the drag started
    pub timestamp: Instant,
}

/// Details about drag update
#[derive(Debug, Clone)]
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
    pub kind: flui_types::events::PointerDeviceKind,
}

/// Details about drag end
#[derive(Debug, Clone)]
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

/// Velocity information
#[derive(Debug, Clone, Copy)]
pub struct Velocity {
    /// Velocity in pixels per second
    pub pixels_per_second: Offset,
}

impl Velocity {
    /// Create zero velocity
    pub fn zero() -> Self {
        Self {
            pixels_per_second: Offset::ZERO,
        }
    }

    /// Get the magnitude of velocity
    pub fn magnitude(&self) -> f32 {
        self.pixels_per_second.distance()
    }
}

/// Callback types for drag events
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
/// use flui_gestures::prelude::*;
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

    /// Minimum distance to start drag
    min_drag_distance: f32,

    /// Minimum velocity to trigger fling (pixels per second)
    min_fling_velocity: f32,
}

#[derive(Default)]
struct DragCallbacks {
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

/// Simple velocity tracker
#[derive(Debug, Clone)]
struct VelocityTracker {
    /// Recent position samples
    samples: Vec<(Instant, Offset)>,
    /// Maximum number of samples to keep
    max_samples: usize,
}

impl VelocityTracker {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            max_samples: 20,
        }
    }

    fn add_sample(&mut self, time: Instant, position: Offset) {
        self.samples.push((time, position));
        if self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
    }

    fn calculate_velocity(&self) -> Velocity {
        if self.samples.len() < 2 {
            return Velocity::zero();
        }

        // Use last N samples (or all if less than N)
        let n = self.samples.len().min(5);
        let start_idx = self.samples.len() - n;

        let (start_time, start_pos) = self.samples[start_idx];
        let (end_time, end_pos) = self.samples[self.samples.len() - 1];

        let dt = end_time.duration_since(start_time).as_secs_f32();
        if dt < 0.001 {
            return Velocity::zero();
        }

        let delta = end_pos - start_pos;
        let velocity = Offset::new(delta.dx / dt, delta.dy / dt);

        Velocity {
            pixels_per_second: velocity,
        }
    }

    fn reset(&mut self) {
        self.samples.clear();
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
            min_drag_distance: constants::DRAG_SLOP as f32,
            min_fling_velocity: constants::MIN_FLING_VELOCITY as f32,
        })
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
    pub fn with_on_cancel(self: Arc<Self>, callback: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        self.callbacks.lock().on_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down - start tracking
    fn handle_down(&self, position: Offset, _kind: flui_types::events::PointerDeviceKind) {
        let mut state = self.drag_state.lock();
        state.state = DragPhase::Possible;
        state.start_time = Some(Instant::now());
        state.last_position = Some(position);
        state.last_time = Some(Instant::now());
        state.total_delta = Offset::ZERO;
        state.velocity_tracker.reset();
        state.velocity_tracker.add_sample(Instant::now(), position);
    }

    /// Handle pointer move - check slop and start/update drag
    fn handle_move(&self, position: Offset, kind: flui_types::events::PointerDeviceKind) {
        let mut state = self.drag_state.lock();

        match state.state {
            DragPhase::Possible => {
                // Check if moved beyond slop
                if let Some(initial_pos) = self.state.initial_position() {
                    let delta = position - initial_pos;
                    let distance = self.calculate_primary_delta(delta);

                    if distance.abs() > self.min_drag_distance {
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
                    state.velocity_tracker.add_sample(Instant::now(), position);

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
    fn handle_up(&self, position: Offset, _kind: flui_types::events::PointerDeviceKind) {
        let mut state = self.drag_state.lock();

        if state.state == DragPhase::Started {
            // Calculate final velocity
            let velocity = state.velocity_tracker.calculate_velocity();
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
}

impl GestureRecognizer for DragGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset) {
        // Start tracking this pointer
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);

        // Handle pointer down
        self.handle_down(position, flui_types::events::PointerDeviceKind::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // Only process if we're tracking a pointer
        if self.state.primary_pointer().is_none() {
            return;
        }

        match event {
            PointerEvent::Move(data) => {
                self.handle_move(data.position, data.device_kind);
            }
            PointerEvent::Up(data) => {
                self.handle_up(data.position, data.device_kind);
            }
            PointerEvent::Cancel(_) => {
                self.handle_cancel();
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        self.callbacks.lock().on_start = None;
        self.callbacks.lock().on_update = None;
        self.callbacks.lock().on_end = None;
        self.callbacks.lock().on_cancel = None;
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

impl std::fmt::Debug for DragGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragGestureRecognizer")
            .field("axis", &self.axis)
            .field("state", &self.drag_state.lock().state)
            .field("has_on_start", &self.callbacks.lock().on_start.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

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
        recognizer.handle_event(&PointerEvent::Move(
            flui_types::events::PointerEventData::new(
                moved_pos,
                flui_types::events::PointerDeviceKind::Touch,
            ),
        ));

        // Should have started
        assert!(*started.lock());

        // Move more
        let moved_pos2 = Offset::new(100.0, 150.0);
        recognizer.handle_event(&PointerEvent::Move(
            flui_types::events::PointerEventData::new(
                moved_pos2,
                flui_types::events::PointerDeviceKind::Touch,
            ),
        ));

        // Should have updated
        assert!(*updated.lock());
    }

    #[test]
    fn test_velocity_tracker() {
        let mut tracker = VelocityTracker::new();

        let start_time = Instant::now();
        let start_pos = Offset::new(0.0, 0.0);

        tracker.add_sample(start_time, start_pos);

        // Simulate movement over 100ms
        let dt = std::time::Duration::from_millis(100);
        let end_pos = Offset::new(100.0, 0.0); // Moved 100px in 100ms = 1000 px/s

        tracker.add_sample(start_time + dt, end_pos);

        let velocity = tracker.calculate_velocity();

        // Should be approximately 1000 px/s horizontally
        assert!(velocity.pixels_per_second.dx > 900.0);
        assert!(velocity.pixels_per_second.dx < 1100.0);
    }
}
