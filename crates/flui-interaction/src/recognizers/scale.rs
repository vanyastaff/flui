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

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use web_time::Instant;

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
    routing::PointerDispatch,
    settings::GestureSettings,
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
    /// Focal point (center between pointers) in global coordinates
    pub focal_point: Offset<f64>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<f64>,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture update
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleUpdateDetails {
    /// Focal point (center between pointers) in global coordinates
    pub focal_point: Offset<f64>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<f64>,
    /// Scale factor (1.0 = no change, >1.0 = zoom in, <1.0 = zoom out)
    pub scale: f64,
    /// Horizontal scale factor
    pub horizontal_scale: f64,
    /// Vertical scale factor
    pub vertical_scale: f64,
    /// Rotation angle in radians (positive = clockwise)
    pub rotation: f64,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture end
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleEndDetails {
    /// Final focal point
    pub focal_point: Offset<f64>,
    /// Final scale factor
    pub scale: f64,
    /// Final rotation angle in radians
    pub rotation: f64,
    /// Velocity of scale change (scale units per second)
    pub velocity: f64,
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
/// recognizer.add_pointer(pointer1_id, position1, position1);
/// recognizer.add_pointer(pointer2_id, position2, position2);
/// recognizer.handle_event(PointerDispatch::at_root(&pointer_event));
/// ```
#[derive(Clone)]
pub struct ScaleGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<ScaleCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<ScaleState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

impl std::fmt::Debug for ScaleGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &*self.gesture_state.lock())
            .field("settings", &*self.settings.lock())
            .finish_non_exhaustive()
    }
}

// Field names keep Flutter's `onScaleStart`-style callback names (parity).
#[expect(clippy::struct_field_names)]
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
    pointers: HashMap<PointerId, Offset<f64>>,
    /// Initial span (distance between first two pointers)
    initial_span: Option<f64>,
    /// Initial focal point, captured with [`Self::initial_span`]
    ///
    /// Retained so the focal-point acceptance arm has a baseline to measure
    /// against; a two-finger pan changes this while leaving every span
    /// untouched.
    initial_focal_point: Option<Offset<f64>>,
    /// Initial horizontal span
    initial_horizontal_span: Option<f64>,
    /// Initial vertical span
    initial_vertical_span: Option<f64>,
    /// Initial rotation angle (radians)
    initial_rotation: Option<f64>,
    /// Previous span (for calculating delta)
    previous_span: Option<f64>,
    /// Current rotation angle
    current_rotation: f64,
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
            initial_focal_point: None,
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

impl ScaleState {
    /// Capture the baseline every acceptance arm measures against.
    ///
    /// The whole group moves together or not at all — a span baseline without
    /// its focal point, or vice versa, silently disables one arm — so the five
    /// places that (re)start a gesture all go through here rather than
    /// assigning the fields one by one.
    fn capture_baseline(&mut self) {
        let (span, h_span, v_span) = ScaleGestureRecognizer::calculate_spans(&self.pointers);
        self.initial_span = Some(span);
        self.initial_horizontal_span = Some(h_span);
        self.initial_vertical_span = Some(v_span);
        self.initial_focal_point = Some(ScaleGestureRecognizer::calculate_focal_point(
            &self.pointers,
        ));
        self.initial_rotation = Some(ScaleGestureRecognizer::calculate_rotation(&self.pointers));
        self.previous_span = Some(span);
    }

    /// Drop the baseline captured by [`Self::capture_baseline`].
    fn clear_baseline(&mut self) {
        self.initial_span = None;
        self.initial_horizontal_span = None;
        self.initial_vertical_span = None;
        self.initial_focal_point = None;
        self.initial_rotation = None;
        self.previous_span = None;
    }
}

impl ScaleGestureRecognizer {
    /// Create a new scale recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(ScaleCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(ScaleState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a scale recognizer with custom settings.
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        let recognizer = Self::new(arena);
        *recognizer.settings.lock() = settings;
        recognizer
    }

    /// Replace the gesture settings.
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
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
    fn handle_pointer_down(&self, pointer: PointerId, position: Offset<f64>) {
        let mut state = self.gesture_state.lock();

        // Add pointer to tracking
        state.pointers.insert(pointer, position);

        if state.pointers.len() == 2 {
            // We have two pointers now - can start tracking
            state.phase = ScalePhase::Possible;

            // Calculate initial spans and rotation
            state.capture_baseline();
            state.current_rotation = 0.0;
        } else if state.pointers.len() > 2 {
            // Additional pointers - recalculate initial span if not started
            if state.phase == ScalePhase::Possible {
                state.capture_baseline();
                state.current_rotation = 0.0;
            }
        }
    }

    /// Whether the gesture has moved enough to claim the arena.
    ///
    /// Mirrors `_advanceStateMachine`'s three-way test
    /// (`gestures/scale.dart`, tag `3.44.0`): a scale is accepted on absolute
    /// span change, **or** focal-point movement, **or** the scale ratio —
    /// any one of them, not all three.
    ///
    /// The arms are not redundant, and dropping any of them loses a whole
    /// class of gesture rather than a little precision:
    ///
    /// - **Ratio alone** cannot see a pinch that starts wide. Fingers 1000 px
    ///   apart moving 40 px further apart are 4% — under the 5% tier — while
    ///   having moved twice any slop.
    /// - **Span arms alone** cannot see a two-finger *pan*. Fingers moving
    ///   together hold both the span and the ratio exactly constant; only the
    ///   focal point moves. This is the arm a two-finger pan rides.
    ///
    /// The span and focal tiers are per-kind (`computeScaleSlop` /
    /// `computePanSlop`); the ratio tier is dimensionless and so has no kind.
    fn should_accept(&self, state: &ScaleState, current_span: f64, kind: PointerType) -> bool {
        let settings = self.settings.lock();

        if let Some(initial_span) = state.initial_span {
            if (current_span - initial_span).abs() > settings.span_slop_for(kind) {
                return true;
            }
            // A zero initial span means the pointers started coincident; the
            // ratio is undefined there, so leave that arm to the two distance
            // tiers rather than dividing by zero.
            if initial_span != 0.0 && settings.exceeds_scale_slop(current_span / initial_span) {
                return true;
            }
        }

        if let Some(initial_focal) = state.initial_focal_point {
            let focal_delta = Self::calculate_focal_point(&state.pointers) - initial_focal;
            if focal_delta.distance() > settings.pan_slop_for(kind) {
                return true;
            }
        }

        false
    }

    /// Handle pointer move - update scale
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset<f64>, kind: PointerType) {
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
                let crossed = self.should_accept(&state, current_span, kind);
                drop(state);

                // Crossing a tier is a request to win, not permission to
                // invoke callbacks -- the same rule `DragGestureRecognizer`
                // follows. A competitor can still take the arena, and an
                // observer that saw `on_start` before that was decided would
                // have acted on a gesture that then gets cancelled.
                // `accept_gesture` is the sole start transition; the reference
                // resolves here too (`scale.dart:749`'s
                // `resolve(GestureDisposition.accepted)`).
                if crossed {
                    self.state.accept_tracked();
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
                    // (we track how scale changes over time).
                    // Read the arena's clock, not the OS clock: production binds it to
                    // `SystemClock` (identical there), but a headless frame driver binds a
                    // `ManualClock`, so a replayed gesture's own sample spacing decides the
                    // velocity instead of however the test process happened to be scheduled.
                    let now = self.state.now();
                    state
                        .scale_velocity_tracker
                        .add_position(now, Offset::new(scale, 0.0));
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
                    .dx;

                state.phase = ScalePhase::Ready;
                state.clear_baseline();
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
                state.clear_baseline();
                state.current_rotation = 0.0;
                state.scale_velocity_tracker.reset();
                state.last_update_time = None;
            }
        } else if state.pointers.len() >= 2 && state.phase == ScalePhase::Possible {
            // Still have 2+ pointers, recalculate initial span
            state.capture_baseline();
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
    fn calculate_spans(pointers: &HashMap<PointerId, Offset<f64>>) -> (f64, f64, f64) {
        if pointers.len() < 2 {
            return (0.0, 0.0, 0.0);
        }

        // Mean deviation from the FOCAL POINT, not mean pairwise distance --
        // `_ScaleGestureRecognizer._update` (`gestures/scale.dart`, tag
        // `3.44.0`): "Span is the average deviation from focal point."
        //
        // For two pointers the two definitions differ by exactly a factor of
        // two, which every RATIO consumer here is blind to (`current /
        // initial` cancels it) -- which is why the pairwise form went
        // unnoticed while the ratio was the only acceptance criterion. It
        // stops being invisible the moment a span is compared against an
        // ABSOLUTE threshold: `kScaleSlop` is calibrated against the
        // reference's definition, so a pairwise span crosses it at half the
        // real movement.
        let focal = Self::calculate_focal_point(pointers);

        let mut total_deviation = 0.0;
        let mut total_h_deviation = 0.0;
        let mut total_v_deviation = 0.0;

        for position in pointers.values() {
            let delta = focal - *position;
            total_deviation += delta.distance();
            total_h_deviation += delta.dx.abs();
            total_v_deviation += delta.dy.abs();
        }

        let count = pointers.len() as f64;
        (
            total_deviation / count,
            total_h_deviation / count,
            total_v_deviation / count,
        )
    }

    /// Calculate focal point (center of all pointers)
    fn calculate_focal_point(pointers: &HashMap<PointerId, Offset<f64>>) -> Offset<f64> {
        if pointers.is_empty() {
            return Offset::ZERO;
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for pos in pointers.values() {
            sum_x += pos.dx;
            sum_y += pos.dy;
        }

        let count = pointers.len() as f64;
        Offset::new(sum_x / count, sum_y / count)
    }

    /// Calculate rotation angle between pointers (in radians)
    ///
    /// For 2 pointers, returns the angle of the line between them.
    /// For more pointers, returns the average angle from the focal point to
    /// each pointer.
    fn calculate_rotation(pointers: &HashMap<PointerId, Offset<f64>>) -> f64 {
        if pointers.len() < 2 {
            return 0.0;
        }

        let positions: Vec<&Offset<f64>> = pointers.values().collect();

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
                if delta.distance() > 0.001 {
                    // Avoid division by zero
                    total_angle += delta.dy.atan2(delta.dx);
                    count += 1;
                }
            }

            if count > 0 {
                total_angle / count as f64
            } else {
                0.0
            }
        }
    }
}

impl GestureRecognizer for ScaleGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        // Scale reports a focal point derived from every tracked contact, in
        // the recogniser's own space — a global focal point needs all the
        // contacts' globals, not this one, and is not attempted here. The base
        // still records this contact in both spaces, because the stored pair
        // is one value and a half-written one is a trap for the next reader.
        global_position: Offset<f64>,
    ) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // For the first pointer, track with arena
        if self.gesture_state.lock().pointers.is_empty() {
            self.state
                .start_tracking(pointer, position, global_position, self);
        }

        self.handle_pointer_down(pointer, position);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
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
                let position = Offset::new(pos.x, pos.y);
                self.handle_pointer_move(pointer, position, data.pointer.pointer_type);
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
        // The sole start transition. Reached once the arena has actually
        // settled on this recognizer, which is the earliest moment
        // `on_start` can be dispatched without the risk of a later
        // cancellation retracting it.
        let mut state = self.gesture_state.lock();
        if state.phase != ScalePhase::Possible {
            return;
        }
        state.phase = ScalePhase::Started;
        state.previous_span = Some(Self::calculate_spans(&state.pointers).0);

        let focal_point = Self::calculate_focal_point(&state.pointers);
        let pointer_count = state.pointers.len();
        drop(state);

        if let Some(callback) = self.callbacks.borrow().on_start.clone() {
            callback(ScaleStartDetails {
                focal_point,
                local_focal_point: focal_point,
                pointer_count,
            });
        }
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
    fn test_scale_calculation() {
        // Test that scale calculation works correctly
        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena);

        let pointer1 = PointerId::new(2).expect("nonzero pointer id");
        let pointer2 = PointerId::new(3).expect("nonzero pointer id");

        // Add two pointers 100px apart
        recognizer.add_pointer(pointer1, Offset::new(0.0, 0.0), Offset::new(0.0, 0.0));
        recognizer.add_pointer(pointer2, Offset::new(100.0, 0.0), Offset::new(100.0, 0.0));

        // Verify we have 2 pointers and initial span is set
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.pointers.len(), 2);
        assert!(state.initial_span.is_some());
        // Half the 100 px separation -- see `test_span_calculation`.
        assert!((state.initial_span.expect("captured") - 50.0).abs() < 0.01);

        // Manually test scale calculation by updating pointer and checking span
        drop(state);
        recognizer.handle_pointer_move(pointer2, Offset::new(200.0, 0.0), PointerType::Touch);

        let state = recognizer.gesture_state.lock();
        let current_span = ScaleGestureRecognizer::calculate_spans(&state.pointers).0;
        // Half the 200 px separation -- mean deviation from the focal point.
        assert!((current_span - 100.0).abs() < 0.01);

        // Calculate scale manually
        let scale = current_span / state.initial_span.unwrap();
        assert!(
            (scale - 2.0).abs() < 0.01,
            "Scale was {scale}, expected 2.0"
        );
    }
}
