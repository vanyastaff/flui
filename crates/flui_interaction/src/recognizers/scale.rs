//! Scale gesture recognizer
//!
//! Recognizes scale gestures (pinch to zoom with 2+ pointers).
//!
//! A scale gesture requires:
//! - Two or more pointers down
//! - Distance between pointers changes
//! - Calculates scale factor and focal point (center)

use super::recognizer::{GestureRecognizer, GestureRecognizerState};
use crate::arena::{GestureArenaMember, PointerId};
use flui_types::{events::PointerEvent, Offset};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Callback for scale start events
pub type ScaleStartCallback = Arc<dyn Fn(ScaleStartDetails) + Send + Sync>;

/// Callback for scale update events
pub type ScaleUpdateCallback = Arc<dyn Fn(ScaleUpdateDetails) + Send + Sync>;

/// Callback for scale end events
pub type ScaleEndCallback = Arc<dyn Fn(ScaleEndDetails) + Send + Sync>;

/// Callback for scale cancel events
pub type ScaleCancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Details about scale gesture start
#[derive(Debug, Clone)]
pub struct ScaleStartDetails {
    /// Focal point (center between pointers) in global coordinates
    pub focal_point: Offset,
    /// Focal point in local coordinates
    pub local_focal_point: Offset,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture update
#[derive(Debug, Clone)]
pub struct ScaleUpdateDetails {
    /// Focal point (center between pointers) in global coordinates
    pub focal_point: Offset,
    /// Focal point in local coordinates
    pub local_focal_point: Offset,
    /// Scale factor (1.0 = no change, >1.0 = zoom in, <1.0 = zoom out)
    pub scale: f32,
    /// Horizontal scale factor
    pub horizontal_scale: f32,
    /// Vertical scale factor
    pub vertical_scale: f32,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture end
#[derive(Debug, Clone)]
pub struct ScaleEndDetails {
    /// Final focal point
    pub focal_point: Offset,
    /// Final scale factor
    pub scale: f32,
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
    state: GestureRecognizerState,

    /// Callbacks
    callbacks: Arc<Mutex<ScaleCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<ScaleState>>,

    /// Minimum scale change to start gesture (factor)
    min_scale_delta: f32,
}

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
    /// Cancelled
    Cancelled,
}

#[derive(Debug, Clone)]
struct ScaleState {
    /// Current phase
    phase: ScalePhase,
    /// Active pointers and their positions
    pointers: HashMap<PointerId, Offset>,
    /// Initial span (distance between first two pointers)
    initial_span: Option<f32>,
    /// Initial horizontal span
    initial_horizontal_span: Option<f32>,
    /// Initial vertical span
    initial_vertical_span: Option<f32>,
    /// Previous span (for calculating delta)
    previous_span: Option<f32>,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            phase: ScalePhase::Ready,
            pointers: HashMap::new(),
            initial_span: None,
            initial_horizontal_span: None,
            initial_vertical_span: None,
            previous_span: None,
        }
    }
}

impl ScaleGestureRecognizer {
    /// Create a new scale recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(ScaleCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ScaleState::default())),
            min_scale_delta: 0.05, // 5% change minimum
        })
    }

    /// Set the scale start callback
    pub fn with_on_scale_start(
        self: Arc<Self>,
        callback: impl Fn(ScaleStartDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_start = Some(Arc::new(callback));
        self
    }

    /// Set the scale update callback
    pub fn with_on_scale_update(
        self: Arc<Self>,
        callback: impl Fn(ScaleUpdateDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_update = Some(Arc::new(callback));
        self
    }

    /// Set the scale end callback
    pub fn with_on_scale_end(
        self: Arc<Self>,
        callback: impl Fn(ScaleEndDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_end = Some(Arc::new(callback));
        self
    }

    /// Set the scale cancel callback
    pub fn with_on_scale_cancel(
        self: Arc<Self>,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down - add to tracking
    fn handle_pointer_down(&self, pointer: PointerId, position: Offset) {
        let mut state = self.gesture_state.lock();

        // Add pointer to tracking
        state.pointers.insert(pointer, position);

        if state.pointers.len() == 2 {
            // We have two pointers now - can start tracking
            state.phase = ScalePhase::Possible;

            // Calculate initial spans
            let spans = self.calculate_spans(&state.pointers);
            state.initial_span = Some(spans.0);
            state.initial_horizontal_span = Some(spans.1);
            state.initial_vertical_span = Some(spans.2);
            state.previous_span = Some(spans.0);
        } else if state.pointers.len() > 2 {
            // Additional pointers - recalculate initial span if not started
            if state.phase == ScalePhase::Possible {
                let spans = self.calculate_spans(&state.pointers);
                state.initial_span = Some(spans.0);
                state.initial_horizontal_span = Some(spans.1);
                state.initial_vertical_span = Some(spans.2);
                state.previous_span = Some(spans.0);
            }
        }
    }

    /// Handle pointer move - update scale
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset) {
        let mut state = self.gesture_state.lock();

        // Update pointer position
        if let Some(pos) = state.pointers.get_mut(&pointer) {
            *pos = position;
        }

        if state.pointers.len() < 2 {
            return; // Need at least 2 pointers
        }

        let spans = self.calculate_spans(&state.pointers);
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

                        let focal_point = self.calculate_focal_point(&state.pointers);
                        drop(state); // Release lock before callback

                        // Call on_start callback
                        if let Some(callback) = self.callbacks.lock().on_start.clone() {
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
                // Update scale
                if let (Some(initial_span), Some(initial_h_span), Some(initial_v_span)) = (
                    state.initial_span,
                    state.initial_horizontal_span,
                    state.initial_vertical_span,
                ) {
                    let scale = current_span / initial_span;
                    let h_scale = current_h_span / initial_h_span;
                    let v_scale = current_v_span / initial_v_span;

                    state.previous_span = Some(current_span);

                    let focal_point = self.calculate_focal_point(&state.pointers);
                    let pointer_count = state.pointers.len();
                    drop(state); // Release lock before callback

                    // Call on_update callback
                    if let Some(callback) = self.callbacks.lock().on_update.clone() {
                        let details = ScaleUpdateDetails {
                            focal_point,
                            local_focal_point: focal_point,
                            scale,
                            horizontal_scale: h_scale,
                            vertical_scale: v_scale,
                            pointer_count,
                        };
                        callback(details);
                    }
                }
            }
            _ => {}
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
                let focal_point = if !state.pointers.is_empty() {
                    self.calculate_focal_point(&state.pointers)
                } else {
                    Offset::ZERO
                };

                let scale = if let (Some(initial_span), Some(prev_span)) =
                    (state.initial_span, state.previous_span)
                {
                    prev_span / initial_span
                } else {
                    1.0
                };

                state.phase = ScalePhase::Ready;
                state.initial_span = None;
                state.initial_horizontal_span = None;
                state.initial_vertical_span = None;
                state.previous_span = None;
                drop(state); // Release lock before callback

                // Call on_end callback
                if let Some(callback) = self.callbacks.lock().on_end.clone() {
                    let details = ScaleEndDetails {
                        focal_point,
                        scale,
                        velocity: 0.0, // TODO: Calculate velocity
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
                state.previous_span = None;
            }
        } else if state.pointers.len() >= 2 && state.phase == ScalePhase::Possible {
            // Still have 2+ pointers, recalculate initial span
            let spans = self.calculate_spans(&state.pointers);
            state.initial_span = Some(spans.0);
            state.initial_horizontal_span = Some(spans.1);
            state.initial_vertical_span = Some(spans.2);
            state.previous_span = Some(spans.0);
        }
    }

    /// Handle cancel
    fn handle_cancel(&self) {
        let mut state = self.gesture_state.lock();

        if state.phase == ScalePhase::Started || state.phase == ScalePhase::Possible {
            state.phase = ScalePhase::Cancelled;
            state.pointers.clear();
            state.initial_span = None;
            state.initial_horizontal_span = None;
            state.initial_vertical_span = None;
            state.previous_span = None;
            drop(state);

            // Call on_cancel callback
            if let Some(callback) = self.callbacks.lock().on_cancel.clone() {
                callback();
            }

            self.state.reject();
        }
    }

    /// Calculate span (distance) between pointers
    /// Returns (total_span, horizontal_span, vertical_span)
    fn calculate_spans(&self, pointers: &HashMap<PointerId, Offset>) -> (f32, f32, f32) {
        if pointers.len() < 2 {
            return (0.0, 0.0, 0.0);
        }

        let positions: Vec<&Offset> = pointers.values().collect();

        // Calculate average span between all pairs
        let mut total_distance = 0.0;
        let mut total_h_distance = 0.0;
        let mut total_v_distance = 0.0;
        let mut count = 0;

        for i in 0..positions.len() {
            for j in (i + 1)..positions.len() {
                let delta = *positions[j] - *positions[i];
                total_distance += delta.distance();
                total_h_distance += delta.dx.abs();
                total_v_distance += delta.dy.abs();
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
    fn calculate_focal_point(&self, pointers: &HashMap<PointerId, Offset>) -> Offset {
        if pointers.is_empty() {
            return Offset::ZERO;
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for pos in pointers.values() {
            sum_x += pos.dx;
            sum_y += pos.dy;
        }

        let count = pointers.len() as f32;
        Offset::new(sum_x / count, sum_y / count)
    }
}

impl GestureRecognizer for ScaleGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset) {
        // For the first pointer, track with arena
        if self.gesture_state.lock().pointers.is_empty() {
            let recognizer = Arc::new(self.clone());
            self.state.start_tracking(pointer, position, &recognizer);
        }

        self.handle_pointer_down(pointer, position);
    }

    fn handle_event(&self, event: &PointerEvent) {
        match event {
            PointerEvent::Move(data) => {
                // We need to figure out which pointer this is
                // For now, assume it's the primary pointer
                // In a real implementation, we'd track pointer IDs
                if let Some(pointer) = self.state.primary_pointer() {
                    self.handle_pointer_move(pointer, data.position);
                }
            }
            PointerEvent::Up(_data) => {
                if let Some(pointer) = self.state.primary_pointer() {
                    self.handle_pointer_up(pointer);
                }
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

impl GestureArenaMember for ScaleGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        self.handle_cancel();
    }
}

impl std::fmt::Debug for ScaleGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("min_scale_delta", &self.min_scale_delta)
            .finish()
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
    fn test_focal_point_calculation() {
        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena);

        let mut pointers = HashMap::new();
        pointers.insert(PointerId::new(1), Offset::new(0.0, 0.0));
        pointers.insert(PointerId::new(2), Offset::new(100.0, 100.0));

        let focal_point = recognizer.calculate_focal_point(&pointers);

        // Center should be at (50, 50)
        assert!((focal_point.dx - 50.0).abs() < 0.01);
        assert!((focal_point.dy - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_span_calculation() {
        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena);

        let mut pointers = HashMap::new();
        pointers.insert(PointerId::new(1), Offset::new(0.0, 0.0));
        pointers.insert(PointerId::new(2), Offset::new(100.0, 0.0));

        let (span, h_span, v_span) = recognizer.calculate_spans(&pointers);

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

        let pointer1 = PointerId::new(1);
        let pointer2 = PointerId::new(2);

        // Add two pointers 100px apart
        recognizer.add_pointer(pointer1, Offset::new(0.0, 0.0));
        recognizer.add_pointer(pointer2, Offset::new(100.0, 0.0));

        // Verify we have 2 pointers and initial span is set
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.pointers.len(), 2);
        assert!(state.initial_span.is_some());
        assert!((state.initial_span.unwrap() - 100.0).abs() < 0.01);

        // Manually test scale calculation by updating pointer and checking span
        drop(state);
        recognizer.handle_pointer_move(pointer2, Offset::new(200.0, 0.0));

        let state = recognizer.gesture_state.lock();
        let current_span = recognizer.calculate_spans(&state.pointers).0;
        assert!((current_span - 200.0).abs() < 0.01);

        // Calculate scale manually
        let scale = current_span / state.initial_span.unwrap();
        assert!(
            (scale - 2.0).abs() < 0.01,
            "Scale was {}, expected 2.0",
            scale
        );
    }
}
