//! Scale gesture recognizer
//!
//! Recognizes scale gestures (pinch to zoom with 2+ pointers).
//!
//! A scale gesture requires:
//! - Two or more pointers down
//! - Distance between pointers changes
//! - Calculates scale factor, rotation angle, and focal point (center)
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/ScaleGestureRecognizer-class.html>

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Instant};

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember, events::PointerEvent, ids::PointerId, processing::VelocityTracker,
};

/// Callback for scale start events
pub type ScaleStartCallback = Rc<dyn Fn(ScaleStartDetails)>;

/// Callback for scale update events
pub type ScaleUpdateCallback = Rc<dyn Fn(ScaleUpdateDetails)>;

/// Callback for scale end events
pub type ScaleEndCallback = Rc<dyn Fn(ScaleEndDetails)>;

/// Callback for scale cancel events
pub type ScaleCancelCallback = Rc<dyn Fn()>;

/// Details about scale gesture start
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleStartDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Focal point (center between pointers) in global coordinates
    pub focal_point: Offset<Pixels>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<Pixels>,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture update
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleUpdateDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Focal point (center between pointers) in global coordinates
    pub focal_point: Offset<Pixels>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<Pixels>,
    /// Scale factor (1.0 = no change, >1.0 = zoom in, <1.0 = zoom out)
    pub scale: f32,
    /// Horizontal scale factor
    pub horizontal_scale: f32,
    /// Vertical scale factor
    pub vertical_scale: f32,
    /// Rotation angle in radians (positive = clockwise)
    pub rotation: f32,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture end
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleEndDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Final focal point
    pub focal_point: Offset<Pixels>,
    /// Final scale factor
    pub scale: f32,
    /// Final rotation angle in radians
    pub rotation: f32,
    /// Velocity of scale change (scale units per second)
    pub velocity: f32,
}

/// Recognizes scale (pinch/zoom) gestures
///
/// Requires at least 2 pointers. Tracks distance between pointers
/// and calculates scale factor and focal point.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = ScaleGestureRecognizer::new(arena)
///     .with_on_scale_start(|details| {
///         println!("Scale started at {:?} with {} pointers",
///                  details.focal_point, details.pointer_count);
///     })
///     .with_on_scale_update(|details| {
///         println!("Scale: {:.2}x", details.scale);
///     });
///
/// // Multi-touch events will be tracked
/// recognizer.add_pointer(pointer1_id, position1);
/// recognizer.add_pointer(pointer2_id, position2);
/// recognizer.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct ScaleGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<ScaleCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<ScaleState>>,

    /// Minimum scale change to start gesture (factor)
    min_scale_delta: f32,
}

impl std::fmt::Debug for ScaleGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &*self.gesture_state.lock())
            .field("min_scale_delta", &self.min_scale_delta)
            .finish_non_exhaustive()
    }
}

// Field names keep Flutter's `onScaleStart`-style callback names (parity).
#[allow(clippy::struct_field_names)]
#[derive(Default)]
struct ScaleCallbacks {
    on_start: Option<ScaleStartCallback>,
    on_update: Option<ScaleUpdateCallback>,
    on_end: Option<ScaleEndCallback>,
    on_cancel: Option<ScaleCancelCallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalePhase {
    /// Ready to start
    Ready,
    /// Waiting for second pointer or sufficient movement
    Possible,
    /// Scale gesture started
    Started,
}

#[derive(Debug, Clone)]
struct ScaleState {
    /// Current phase
    phase: ScalePhase,
    /// Active pointers and their positions
    pointers: HashMap<PointerId, Offset<Pixels>>,
    /// Initial span (distance between first two pointers)
    initial_span: Option<f32>,
    /// Initial horizontal span
    initial_horizontal_span: Option<f32>,
    /// Initial vertical span
    initial_vertical_span: Option<f32>,
    /// Initial rotation angle (radians)
    initial_rotation: Option<f32>,
    /// Previous span (for calculating delta)
    previous_span: Option<f32>,
    /// Current rotation angle
    current_rotation: f32,
    /// Velocity tracker for scale changes
    scale_velocity_tracker: VelocityTracker,
    /// Last update time for velocity calculation
    last_update_time: Option<Instant>,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            phase: ScalePhase::Ready,
            pointers: HashMap::new(),
            initial_span: None,
            initial_horizontal_span: None,
            initial_vertical_span: None,
            initial_rotation: None,
            previous_span: None,
            current_rotation: 0.0,
            scale_velocity_tracker: VelocityTracker::new(),
            last_update_time: None,
        }
    }
}

impl ScaleGestureRecognizer {
    /// Create a new scale recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(ScaleCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ScaleState::default())),
            min_scale_delta: 0.05, // 5% change minimum
        })
    }

    /// Set the scale start callback
    pub fn with_on_scale_start(
        self: Arc<Self>,
        callback: impl Fn(ScaleStartDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_start = Some(Rc::new(callback));
        self
    }

    /// Set the scale update callback
    pub fn with_on_scale_update(
        self: Arc<Self>,
        callback: impl Fn(ScaleUpdateDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_update = Some(Rc::new(callback));
        self
    }

    /// Set the scale end callback
    pub fn with_on_scale_end(
        self: Arc<Self>,
        callback: impl Fn(ScaleEndDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_end = Some(Rc::new(callback));
        self
    }

    /// Set the scale cancel callback
    pub fn with_on_scale_cancel(self: Arc<Self>, callback: impl Fn() + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_cancel = Some(Rc::new(callback));
        self
    }

    /// Handle pointer down - add to tracking
    fn handle_pointer_down(&self, pointer: PointerId, position: Offset<Pixels>) {
        let mut state = self.gesture_state.lock();

        // Add pointer to tracking
        state.pointers.insert(pointer, position);

        if state.pointers.len() == 2 {
            // We have two pointers now - can start tracking
            state.phase = ScalePhase::Possible;

            // Calculate initial spans and rotation
            let spans = Self::calculate_spans(&state.pointers);
            state.initial_span = Some(spans.0);
            state.initial_horizontal_span = Some(spans.1);
            state.initial_vertical_span = Some(spans.2);
            state.initial_rotation = Some(Self::calculate_rotation(&state.pointers));
            state.previous_span = Some(spans.0);
            state.current_rotation = 0.0;
        } else if state.pointers.len() > 2 {
            // Additional pointers - recalculate initial span if not started
            if state.phase == ScalePhase::Possible {
                let spans = Self::calculate_spans(&state.pointers);
                state.initial_span = Some(spans.0);
                state.initial_horizontal_span = Some(spans.1);
                state.initial_vertical_span = Some(spans.2);
                state.initial_rotation = Some(Self::calculate_rotation(&state.pointers));
                state.previous_span = Some(spans.0);
                state.current_rotation = 0.0;
            }
        }
    }

    /// Handle pointer move - update scale
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset<Pixels>) {
        let mut state = self.gesture_state.lock();

        // Update pointer position
        if let Some(pos) = state.pointers.get_mut(&pointer) {
            *pos = position;
        }

        if state.pointers.len() < 2 {
            return; // Need at least 2 pointers
        }

        let spans = Self::calculate_spans(&state.pointers);
        let current_span = spans.0;
        let current_h_span = spans.1;
        let current_v_span = spans.2;

        match state.phase {
            ScalePhase::Possible => {
                // Check if scale changed enough to start
                if let (Some(initial_span), Some(_prev_span)) =
                    (state.initial_span, state.previous_span)
                {
                    let scale = current_span / initial_span;
                    let scale_delta = (scale - 1.0).abs();

                    if scale_delta >= self.min_scale_delta {
                        // Scale changed enough - start gesture
                        state.phase = ScalePhase::Started;
                        state.previous_span = Some(current_span);

                        let focal_point = Self::calculate_focal_point(&state.pointers);
                        drop(state); // Release lock before callback

                        // Call on_start callback
                        if let Some(callback) = self.callbacks.borrow().on_start.clone() {
                            let details = ScaleStartDetails {
                                focal_point,
                                local_focal_point: focal_point,
                                pointer_count: self.gesture_state.lock().pointers.len(),
                            };
                            callback(details);
                        }
                    }
                }
            }
            ScalePhase::Started => {
                // Update scale and rotation
                if let (
                    Some(initial_span),
                    Some(initial_h_span),
                    Some(initial_v_span),
                    Some(initial_rotation),
                ) = (
                    state.initial_span,
                    state.initial_horizontal_span,
                    state.initial_vertical_span,
                    state.initial_rotation,
                ) {
                    let scale = current_span / initial_span;
                    let h_scale = current_h_span / initial_h_span;
                    let v_scale = current_v_span / initial_v_span;

                    // Calculate rotation delta from initial angle
                    let current_rotation_raw = Self::calculate_rotation(&state.pointers);
                    let rotation = current_rotation_raw - initial_rotation;

                    // Track scale velocity: use scale as a position-like value
                    // (we track how scale changes over time)
                    let now = Instant::now();
                    state
                        .scale_velocity_tracker
                        .add_position(now, Offset::new(Pixels(scale), Pixels(0.0)));
                    state.last_update_time = Some(now);

                    state.previous_span = Some(current_span);
                    state.current_rotation = rotation;

                    let focal_point = Self::calculate_focal_point(&state.pointers);
                    let pointer_count = state.pointers.len();
                    drop(state); // Release lock before callback

                    // Call on_update callback
                    if let Some(callback) = self.callbacks.borrow().on_update.clone() {
                        let details = ScaleUpdateDetails {
                            focal_point,
                            local_focal_point: focal_point,
                            scale,
                            horizontal_scale: h_scale,
                            vertical_scale: v_scale,
                            rotation,
                            pointer_count,
                        };
                        callback(details);
                    }
                }
            }
            ScalePhase::Ready => {}
        }
    }

    /// Handle pointer up - remove from tracking
    fn handle_pointer_up(&self, pointer: PointerId) {
        let mut state = self.gesture_state.lock();

        state.pointers.remove(&pointer);

        if state.pointers.len() < 2 {
            // Not enough pointers anymore
            if state.phase == ScalePhase::Started {
                // End the gesture
                let focal_point = if state.pointers.is_empty() {
                    Offset::ZERO
                } else {
                    Self::calculate_focal_point(&state.pointers)
                };

                let scale = if let (Some(initial_span), Some(prev_span)) =
                    (state.initial_span, state.previous_span)
                {
                    prev_span / initial_span
                } else {
                    1.0
                };

                let rotation = state.current_rotation;

                // Calculate scale velocity from tracker
                // The velocity is in scale units per second (e.g., 0.5 means scaling at 50% per
                // second)
                let velocity = state
                    .scale_velocity_tracker
                    .get_velocity()
                    .pixels_per_second
                    .dx
                    .0;

                state.phase = ScalePhase::Ready;
                state.initial_span = None;
                state.initial_horizontal_span = None;
                state.initial_vertical_span = None;
                state.initial_rotation = None;
                state.previous_span = None;
                state.current_rotation = 0.0;
                state.scale_velocity_tracker.reset();
                state.last_update_time = None;
                drop(state); // Release lock before callback

                // Call on_end callback
                if let Some(callback) = self.callbacks.borrow().on_end.clone() {
                    let details = ScaleEndDetails {
                        focal_point,
                        scale,
                        rotation,
                        velocity,
                    };
                    callback(details);
                }

                self.state.stop_tracking();
            } else {
                // Reset to ready
                state.phase = ScalePhase::Ready;
                state.initial_span = None;
                state.initial_horizontal_span = None;
                state.initial_vertical_span = None;
                state.initial_rotation = None;
                state.previous_span = None;
                state.current_rotation = 0.0;
                state.scale_velocity_tracker.reset();
                state.last_update_time = None;
            }
        } else if state.pointers.len() >= 2 && state.phase == ScalePhase::Possible {
            // Still have 2+ pointers, recalculate initial span
            let spans = Self::calculate_spans(&state.pointers);
            state.initial_span = Some(spans.0);
            state.initial_horizontal_span = Some(spans.1);
            state.initial_vertical_span = Some(spans.2);
            state.initial_rotation = Some(Self::calculate_rotation(&state.pointers));
            state.previous_span = Some(spans.0);
        }
    }

    /// Handle cancel
    fn handle_cancel(&self) {
        let mut state = self.gesture_state.lock();

        if state.phase == ScalePhase::Started || state.phase == ScalePhase::Possible {
            let callback = self.callbacks.borrow().on_cancel.clone();
            *state = ScaleState::default();
            drop(state);

            self.state.reject();
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    /// Calculate span (distance) between pointers
    /// Returns (total_span, horizontal_span, vertical_span)
    fn calculate_spans(pointers: &HashMap<PointerId, Offset<Pixels>>) -> (f32, f32, f32) {
        if pointers.len() < 2 {
            return (0.0, 0.0, 0.0);
        }

        let positions: Vec<&Offset<Pixels>> = pointers.values().collect();

        // Calculate average span between all pairs
        let mut total_distance = 0.0;
        let mut total_h_distance = 0.0;
        let mut total_v_distance = 0.0;
        let mut count = 0;

        for i in 0..positions.len() {
            for j in (i + 1)..positions.len() {
                let delta = *positions[j] - *positions[i];
                total_distance += delta.distance().0;
                total_h_distance += delta.dx.abs().0;
                total_v_distance += delta.dy.abs().0;
                count += 1;
            }
        }

        if count > 0 {
            (
                total_distance / count as f32,
                total_h_distance / count as f32,
                total_v_distance / count as f32,
            )
        } else {
            (0.0, 0.0, 0.0)
        }
    }

    /// Calculate focal point (center of all pointers)
    fn calculate_focal_point(pointers: &HashMap<PointerId, Offset<Pixels>>) -> Offset<Pixels> {
        if pointers.is_empty() {
            return Offset::ZERO;
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for pos in pointers.values() {
            sum_x += pos.dx.0;
            sum_y += pos.dy.0;
        }

        let count = pointers.len() as f32;
        Offset::new(Pixels(sum_x / count), Pixels(sum_y / count))
    }

    /// Calculate rotation angle between pointers (in radians)
    ///
    /// For 2 pointers, returns the angle of the line between them.
    /// For more pointers, returns the average angle from the focal point to
    /// each pointer.
    fn calculate_rotation(pointers: &HashMap<PointerId, Offset<Pixels>>) -> f32 {
        if pointers.len() < 2 {
            return 0.0;
        }

        let positions: Vec<&Offset<Pixels>> = pointers.values().collect();

        if positions.len() == 2 {
            // For exactly 2 pointers, calculate angle of line between them
            let delta = *positions[1] - *positions[0];
            delta.dy.atan2(delta.dx)
        } else {
            // For more pointers, calculate average angle from focal point
            let focal = Self::calculate_focal_point(pointers);
            let mut total_angle = 0.0;
            let mut count = 0;

            for pos in positions {
                let delta = *pos - focal;
                if delta.distance() > Pixels(0.001) {
                    // Avoid division by zero
                    total_angle += delta.dy.0.atan2(delta.dx.0);
                    count += 1;
                }
            }

            if count > 0 {
                total_angle / count as f32
            } else {
                0.0
            }
        }
    }
}

impl GestureRecognizer for ScaleGestureRecognizer {
    fn add_pointer(self: &Arc<Self>, pointer: PointerId, position: Offset<Pixels>) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // For the first pointer, track with arena
        if self.gesture_state.lock().pointers.is_empty() {
            self.state.start_tracking(pointer, position, self);
        }

        self.handle_pointer_down(pointer, position);
    }

    fn handle_event(&self, event: &PointerEvent) {
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Route by the event's own pointer id (Flutter parity:
        // `ScaleGestureRecognizer.handleEvent` keys `_pointerLocations` by
        // `event.pointer`). Attributing a secondary finger's events to the
        // primary pointer corrupts span and focal point and leaves two-finger
        // pinch inert.
        match event {
            PointerEvent::Move(data) => {
                let pointer = crate::events::extract_pointer_id(event);
                let pos = data.current.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                self.handle_pointer_move(pointer, position);
            }
            PointerEvent::Up(_) => {
                let pointer = crate::events::extract_pointer_id(event);
                self.handle_pointer_up(pointer);
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
        self.callbacks.borrow_mut().on_start = None;
        self.callbacks.borrow_mut().on_update = None;
        self.callbacks.borrow_mut().on_end = None;
        self.callbacks.borrow_mut().on_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// Flutter parity: `scale.dart:345 ScaleGestureRecognizer extends
// OneSequenceGestureRecognizer`. Scale tracks multiple pointers (2+
// for pinch) but resolves as a single sequence in the arena.

impl crate::recognizers::OneSequenceGestureRecognizer for ScaleGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        // Scale's RecognizerBase only tracks the primary pointer; richer
        // multi-pointer tracking lives on ScaleGestureRecognizer's own
        // internal state. Return what RecognizerBase knows for the canonical
        // single-pointer arena protocol.
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, _pointer: PointerId, disposition: crate::arena::GestureDisposition) {
        match disposition {
            crate::arena::GestureDisposition::Accepted => {
                // No-op — Scale callbacks fire from event handlers.
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

impl GestureArenaMember for ScaleGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        self.handle_cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

    #[test]
    fn test_scale_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena);

        assert_eq!(recognizer.primary_pointer(), None);
    }

    #[test]
    fn panicking_cancel_callback_cannot_strand_scale_tracking() {
        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena.clone())
            .with_on_scale_cancel(|| panic!("scale cancel panic"));
        recognizer.add_pointer(PointerId::PRIMARY, Offset::new(Pixels(1.0), Pixels(2.0)));
        recognizer.add_pointer(
            PointerId::new(2).expect("nonzero pointer id"),
            Offset::new(Pixels(3.0), Pixels(4.0)),
        );
        arena.close(PointerId::PRIMARY);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recognizer.handle_event(&crate::events::make_cancel_event(
                crate::events::PointerType::Touch,
            ));
        }));

        assert!(unwind.is_err());
        assert_eq!(recognizer.primary_pointer(), None);
        assert!(arena.is_empty());
    }

    #[test]
    fn two_finger_pinch_routes_events_per_pointer() {
        use crate::events::{PointerType, make_move_event_for_id, make_up_event_for_id};
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Regression: handle_event used to attribute every Move/Up to the
        // primary pointer, so the second finger's movement never updated its
        // own slot and pinch produced no scale updates.
        let arena = GestureArena::new();
        let updates = Arc::new(AtomicUsize::new(0));
        let last_scale = Arc::new(Mutex::new(1.0_f32));
        let updates2 = Arc::clone(&updates);
        let last_scale2 = Arc::clone(&last_scale);
        let recognizer = ScaleGestureRecognizer::new(arena).with_on_scale_update(move |details| {
            updates2.fetch_add(1, Ordering::SeqCst);
            *last_scale2.lock() = details.scale;
        });

        let finger1 = PointerId::new(2).expect("nonzero pointer id");
        let finger2 = PointerId::new(3).expect("nonzero pointer id");
        recognizer.add_pointer(finger1, Offset::new(Pixels(0.0), Pixels(0.0)));
        recognizer.add_pointer(finger2, Offset::new(Pixels(100.0), Pixels(0.0)));

        // Move ONLY the second finger outward through the public event path.
        let move2 = make_move_event_for_id(
            finger2,
            Offset::new(Pixels(200.0), Pixels(0.0)),
            PointerType::Touch,
        );
        recognizer.handle_event(&move2); // crosses the 5% slop -> Started
        let move2b = make_move_event_for_id(
            finger2,
            Offset::new(Pixels(220.0), Pixels(0.0)),
            PointerType::Touch,
        );
        recognizer.handle_event(&move2b);

        assert!(
            updates.load(Ordering::SeqCst) >= 1,
            "second finger's movement must drive scale updates"
        );
        assert!(
            (*last_scale.lock() - 2.2).abs() < 0.05,
            "span 220/100 must be reported, got {}",
            *last_scale.lock()
        );

        // Lifting the SECOND finger must remove its own slot.
        let up2 = make_up_event_for_id(
            finger2,
            Offset::new(Pixels(220.0), Pixels(0.0)),
            PointerType::Touch,
        );
        recognizer.handle_event(&up2);
        assert_eq!(recognizer.gesture_state.lock().pointers.len(), 1);
        assert!(
            recognizer
                .gesture_state
                .lock()
                .pointers
                .contains_key(&finger1),
            "the remaining slot must belong to the first finger"
        );
    }

    #[test]
    fn test_focal_point_calculation() {
        let mut pointers = HashMap::new();
        pointers.insert(
            PointerId::new(2).expect("nonzero pointer id"),
            Offset::new(Pixels(0.0), Pixels(0.0)),
        );
        pointers.insert(
            PointerId::new(3).expect("nonzero pointer id"),
            Offset::new(Pixels(100.0), Pixels(100.0)),
        );

        let focal_point = ScaleGestureRecognizer::calculate_focal_point(&pointers);

        // Center should be at (50, 50)
        assert!((focal_point.dx - Pixels(50.0)).abs() < Pixels(0.01));
        assert!((focal_point.dy - Pixels(50.0)).abs() < Pixels(0.01));
    }

    #[test]
    fn test_span_calculation() {
        let mut pointers = HashMap::new();
        pointers.insert(
            PointerId::new(2).expect("nonzero pointer id"),
            Offset::new(Pixels(0.0), Pixels(0.0)),
        );
        pointers.insert(
            PointerId::new(3).expect("nonzero pointer id"),
            Offset::new(Pixels(100.0), Pixels(0.0)),
        );

        let (span, h_span, v_span) = ScaleGestureRecognizer::calculate_spans(&pointers);

        // Distance should be 100
        assert!((span - 100.0).abs() < 0.01);
        assert!((h_span - 100.0).abs() < 0.01);
        assert!(v_span.abs() < 0.01);
    }

    #[test]
    fn test_scale_calculation() {
        // Test that scale calculation works correctly
        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena);

        let pointer1 = PointerId::new(2).expect("nonzero pointer id");
        let pointer2 = PointerId::new(3).expect("nonzero pointer id");

        // Add two pointers 100px apart
        recognizer.add_pointer(pointer1, Offset::new(Pixels(0.0), Pixels(0.0)));
        recognizer.add_pointer(pointer2, Offset::new(Pixels(100.0), Pixels(0.0)));

        // Verify we have 2 pointers and initial span is set
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.pointers.len(), 2);
        assert!(state.initial_span.is_some());
        assert!((state.initial_span.unwrap() - 100.0).abs() < 0.01);

        // Manually test scale calculation by updating pointer and checking span
        drop(state);
        recognizer.handle_pointer_move(pointer2, Offset::new(Pixels(200.0), Pixels(0.0)));

        let state = recognizer.gesture_state.lock();
        let current_span = ScaleGestureRecognizer::calculate_spans(&state.pointers).0;
        assert!((current_span - 200.0).abs() < 0.01);

        // Calculate scale manually
        let scale = current_span / state.initial_span.unwrap();
        assert!(
            (scale - 2.0).abs() < 0.01,
            "Scale was {scale}, expected 2.0"
        );
    }
}
