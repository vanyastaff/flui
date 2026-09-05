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

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    processing::VelocityTracker,
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
    pub focal_point: Offset<Pixels>,
    /// Focal point in local coordinates
    pub local_focal_point: Offset<Pixels>,
    /// Number of pointers involved
    pub pointer_count: usize,
}

/// Details about scale gesture update
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleUpdateDetails {
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
    pointers: HashMap<PointerId, Offset<Pixels>>,
    /// Initial span (distance between first two pointers)
    initial_span: Option<f32>,
    /// Initial focal point, captured with [`Self::initial_span`]
    ///
    /// Retained so the focal-point acceptance arm has a baseline to measure
    /// against; a two-finger pan changes this while leaving every span
    /// untouched.
    initial_focal_point: Option<Offset<Pixels>>,
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
    fn handle_pointer_down(&self, pointer: PointerId, position: Offset<Pixels>) {
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
    fn should_accept(&self, state: &ScaleState, current_span: f32, kind: PointerType) -> bool {
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
            if focal_delta.distance().0 > settings.pan_slop_for(kind) {
                return true;
            }
        }

        false
    }

    /// Handle pointer move - update scale
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset<Pixels>, kind: PointerType) {
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
    fn calculate_spans(pointers: &HashMap<PointerId, Offset<Pixels>>) -> (f32, f32, f32) {
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
            total_deviation += delta.distance().0;
            total_h_deviation += delta.dx.abs().0;
            total_v_deviation += delta.dy.abs().0;
        }

        let count = pointers.len() as f32;
        (
            total_deviation / count,
            total_h_deviation / count,
            total_v_deviation / count,
        )
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
        let recognizer =
            ScaleGestureRecognizer::new(arena.clone()).with_on_scale_update(move |details| {
                updates2.fetch_add(1, Ordering::SeqCst);
                *last_scale2.lock() = details.scale;
            });

        let finger1 = PointerId::new(2).expect("nonzero pointer id");
        let finger2 = PointerId::new(3).expect("nonzero pointer id");
        recognizer.add_pointer(finger1, Offset::new(Pixels(0.0), Pixels(0.0)));
        recognizer.add_pointer(finger2, Offset::new(Pixels(100.0), Pixels(0.0)));
        // The start now waits on arena acceptance, so the arena must be closed
        // for it to land -- dispatch closes it after the pointer-down burst.
        arena.close(finger1);

        // Move ONLY the second finger outward through the public event path.
        let move2 = make_move_event_for_id(
            finger2,
            Offset::new(Pixels(200.0), Pixels(0.0)),
            PointerType::Touch,
        );
        recognizer.handle_event(&move2); // crosses a tier -> arena accepts -> Started
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

        // Mean deviation from the focal point, which for two pointers is HALF
        // their separation -- the reference's definition
        // (`scale.dart`'s "Span is the average deviation from focal point"),
        // not the pairwise distance. The distinction is invisible to every
        // ratio consumer and load-bearing for anything comparing a span
        // against an absolute threshold like `kScaleSlop`.
        assert!((span - 50.0).abs() < 0.01);
        assert!((h_span - 50.0).abs() < 0.01);
        assert!(v_span.abs() < 0.01);
    }

    /// Fingers moving *together* start the gesture.
    ///
    /// A two-finger pan holds the span — and therefore the ratio — constant;
    /// only the focal point moves. With the ratio as the sole acceptance
    /// criterion this gesture could not start at all, which is the path a
    /// two-finger pan rides (`scale.dart:747`'s
    /// `focalPointDelta > computePanSlop`).
    ///
    /// The pan is walked in **small alternating steps** on purpose. Moving one
    /// pointer the whole way and then the other leaves the span 40 px short
    /// mid-walk, which trips the span and ratio arms and makes the test pass
    /// with the focal arm deleted — the first draft did exactly that. Stepping
    /// 2 px at a time keeps the intermediate span within 2 px of its baseline
    /// (2% ratio), under both other tiers throughout, so the focal arm is the
    /// only one that can fire.
    #[test]
    fn two_fingers_moving_together_start_the_gesture() {
        use crate::settings::{DEFAULT_SCALE_SLOP, DEFAULT_SPAN_SLOP};

        const STEP: f32 = 2.0;
        const PAIRS: usize = 10;
        const SEPARATION: f32 = 100.0;
        // Span is the mean deviation from the focal point: half the
        // separation for two pointers.
        const BASELINE_SPAN: f32 = SEPARATION / 2.0;

        // At each half-step only one pointer has moved, leaving the pair
        // STEP closer together and the span STEP/2 short of its baseline.
        // That must stay inside both other tiers or one of them can accept
        // instead of the focal arm. Checked at compile time so that raising a
        // tier constant breaks the build rather than quietly making this
        // vacuous.
        const {
            assert!(
                STEP / 2.0 < DEFAULT_SPAN_SLOP,
                "half-step must stay under the span slop"
            );
            assert!(
                (STEP / 2.0) / BASELINE_SPAN < DEFAULT_SCALE_SLOP,
                "half-step must stay under the ratio slop"
            );
        }

        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena.clone());
        let (p1, p2) = (
            PointerId::new(2).expect("nonzero pointer id"),
            PointerId::new(3).expect("nonzero pointer id"),
        );
        recognizer.add_pointer(p1, Offset::new(Pixels(0.0), Pixels(0.0)));
        recognizer.add_pointer(p2, Offset::new(Pixels(SEPARATION), Pixels(0.0)));
        arena.close(p1);

        let mut max_span_drift: f32 = 0.0;
        for i in 1..=PAIRS {
            let shift = STEP * i as f32;
            for (p, base) in [(p1, 0.0), (p2, SEPARATION)] {
                recognizer.handle_pointer_move(
                    p,
                    Offset::new(Pixels(base + shift), Pixels(0.0)),
                    PointerType::Touch,
                );
                let state = recognizer.gesture_state.lock();
                let span = ScaleGestureRecognizer::calculate_spans(&state.pointers).0;
                max_span_drift = max_span_drift.max((span - BASELINE_SPAN).abs());
            }
        }

        // The premise, measured rather than assumed: no intermediate state
        // ever came close to the span or ratio tiers.
        assert!(
            max_span_drift < DEFAULT_SPAN_SLOP
                && max_span_drift / BASELINE_SPAN < DEFAULT_SCALE_SLOP,
            "the span drifted {max_span_drift} px during the pan, so a span or \
             ratio arm could be what accepted"
        );
        assert_eq!(
            recognizer.gesture_state.lock().phase,
            ScalePhase::Started,
            "a {} px two-finger pan must start the gesture through the \
             focal-point arm",
            STEP * PAIRS as f32
        );
    }

    /// The arena resolves before `on_start`, not after.
    ///
    /// The reference resolves at the crossing (`scale.dart`'s
    /// `resolve(GestureDisposition.accepted)`) and dispatches `onStart` only
    /// once the arena has settled on this recognizer. FLUI advanced the phase
    /// and fired `on_start` inline, consulting the arena not at all — so an
    /// observer could act on a scale start that the arena had not granted, and
    /// competitors were never told they had lost.
    /// `DragGestureRecognizer` already follows the correct rule: "crossing
    /// slop is a request to win, not permission to invoke callbacks".
    ///
    /// The oracle is ORDER, recorded from both sides: a competitor's
    /// `reject_gesture` must land before `on_start`. Asserting only that
    /// `on_start` eventually fires cannot fail — it fires either way, which is
    /// exactly why the inline start went unnoticed.
    #[test]
    fn the_arena_resolves_before_on_start() {
        #[derive(Debug)]
        struct Competitor(Arc<Mutex<Vec<&'static str>>>);
        impl crate::sealed::arena_member::Sealed for Competitor {}
        impl GestureArenaMember for Competitor {
            fn accept_gesture(&self, _pointer: PointerId) {
                self.0.lock().push("competitor accepted");
            }
            fn reject_gesture(&self, _pointer: PointerId) {
                self.0.lock().push("competitor rejected");
            }
        }

        let log = Arc::new(Mutex::new(Vec::new()));
        let arena = GestureArena::new();
        let sink = Arc::clone(&log);
        let recognizer = ScaleGestureRecognizer::new(arena.clone())
            .with_on_scale_start(move |_| sink.lock().push("scale started"));

        let (p1, p2) = (
            PointerId::new(2).expect("nonzero pointer id"),
            PointerId::new(3).expect("nonzero pointer id"),
        );
        recognizer.add_pointer(p1, Offset::new(Pixels(0.0), Pixels(0.0)));
        recognizer.add_pointer(p2, Offset::new(Pixels(1000.0), Pixels(0.0)));
        arena.add(p1, Arc::new(Competitor(Arc::clone(&log))));
        arena.close(p1);

        assert!(
            log.lock().is_empty(),
            "nothing is decided by closing alone -- the arena is contested"
        );

        // A symmetric spread well past every tier.
        for (p, to) in [(p1, -100.0), (p2, 1100.0)] {
            recognizer.handle_pointer_move(
                p,
                Offset::new(Pixels(to), Pixels(0.0)),
                PointerType::Touch,
            );
        }

        assert_eq!(
            log.lock().as_slice(),
            &["competitor rejected", "scale started"],
            "the crossing must resolve the arena FIRST -- turning the \
             competitor down -- and only then dispatch the start"
        );
    }

    /// A wide pinch starts on absolute movement, below the ratio tier.
    ///
    /// Fingers far apart travel a long way before they travel 5%. The ratio
    /// arm alone rejects this; `spanDelta > computeScaleSlop` is what catches
    /// it (`scale.dart:746`).
    ///
    /// The pinch is **symmetric** — both pointers move outward by the same
    /// amount — so the focal point does not move at all and the focal arm
    /// cannot be what accepts. Moving one pointer instead shifts the focal
    /// point by exactly half the growth, which is the same quantity the span
    /// changes by, so the two arms are inseparable that way.
    #[test]
    fn a_wide_pinch_starts_below_the_ratio_tier() {
        use crate::settings::{DEFAULT_PAN_SLOP, DEFAULT_SCALE_SLOP, DEFAULT_SPAN_SLOP};

        // Pointers 1000 px apart: span (mean deviation from the focal point)
        // is 500. Each moves 20 px outward, so the span grows by 20 -- past
        // the 18 px tier -- while the ratio changes by 20/500 = 4%, under the
        // 5% tier. The gap between the tiers is the only region where the two
        // arms disagree.
        const SEPARATION: f32 = 1000.0;
        const GROWTH: f32 = 20.0;
        let baseline_span = SEPARATION / 2.0;

        const {
            assert!(GROWTH > DEFAULT_SPAN_SLOP, "must cross the span tier");
            // Only one pointer has moved at the halfway step, which shifts the
            // focal point by half the growth. That must stay under the pan
            // tier or the focal arm accepts mid-walk.
            assert!(
                GROWTH / 2.0 < DEFAULT_PAN_SLOP,
                "the intermediate focal shift must stay inside the pan tier"
            );
        }
        assert!(
            GROWTH / baseline_span < DEFAULT_SCALE_SLOP,
            "and must stay under the ratio tier, or the ratio arm could be \
             what accepts"
        );

        let arena = GestureArena::new();
        let recognizer = ScaleGestureRecognizer::new(arena.clone());
        let (p1, p2) = (
            PointerId::new(2).expect("nonzero pointer id"),
            PointerId::new(3).expect("nonzero pointer id"),
        );
        recognizer.add_pointer(p1, Offset::new(Pixels(0.0), Pixels(0.0)));
        recognizer.add_pointer(p2, Offset::new(Pixels(SEPARATION), Pixels(0.0)));
        // The arena must be closed for an acceptance to land, exactly as
        // dispatch closes it after the pointer-down burst.
        arena.close(p1);

        for (p, to) in [(p1, -GROWTH), (p2, SEPARATION + GROWTH)] {
            recognizer.handle_pointer_move(
                p,
                Offset::new(Pixels(to), Pixels(0.0)),
                PointerType::Touch,
            );
        }

        let state = recognizer.gesture_state.lock();
        let focal = ScaleGestureRecognizer::calculate_focal_point(&state.pointers);
        assert!(
            (focal.dx.0 - SEPARATION / 2.0).abs() < 0.01,
            "premise: a symmetric pinch leaves the focal point where it was, \
             so only the span arm can have accepted"
        );
        assert_eq!(
            state.phase,
            ScalePhase::Started,
            "a {GROWTH} px span growth past the {DEFAULT_SPAN_SLOP} px span \
             slop must start the gesture even at a {}% ratio change",
            100.0 * GROWTH / baseline_span
        );
    }

    /// The span tier is per-kind: a mouse accepts where a finger does not.
    ///
    /// Symmetric again, and deliberately SMALL: the growth is picked so that
    /// even the halfway step's focal shift stays inside the *mouse* pan slop,
    /// which is 2 px. A larger probe would let the mouse half accept through
    /// the focal arm instead and say nothing about the span tier.
    #[test]
    fn the_span_tier_is_kind_aware() {
        use crate::settings::{DEFAULT_MOUSE_PAN_SLOP, DEFAULT_MOUSE_SPAN_SLOP, DEFAULT_SPAN_SLOP};

        const SEPARATION: f32 = 1000.0;
        const GROWTH: f32 = 2.5;

        const {
            assert!(
                GROWTH > DEFAULT_MOUSE_SPAN_SLOP && GROWTH < DEFAULT_SPAN_SLOP,
                "the growth must sit strictly between the two span tiers"
            );
            assert!(
                GROWTH / 2.0 < DEFAULT_MOUSE_PAN_SLOP,
                "and the halfway focal shift must stay inside even the mouse                  pan slop, or the focal arm accepts instead"
            );
        }

        let phase_after = |kind: PointerType| {
            let arena = GestureArena::new();
            let recognizer = ScaleGestureRecognizer::new(arena.clone());
            let (p1, p2) = (
                PointerId::new(2).expect("nonzero pointer id"),
                PointerId::new(3).expect("nonzero pointer id"),
            );
            recognizer.add_pointer(p1, Offset::new(Pixels(0.0), Pixels(0.0)));
            recognizer.add_pointer(p2, Offset::new(Pixels(SEPARATION), Pixels(0.0)));
            arena.close(p1);
            for (p, to) in [(p1, -GROWTH), (p2, SEPARATION + GROWTH)] {
                recognizer.handle_pointer_move(p, Offset::new(Pixels(to), Pixels(0.0)), kind);
            }
            recognizer.gesture_state.lock().phase
        };

        assert_eq!(
            phase_after(PointerType::Mouse),
            ScalePhase::Started,
            "a {GROWTH} px span growth is past the {DEFAULT_MOUSE_SPAN_SLOP} px \
             mouse span slop"
        );
        assert_eq!(
            phase_after(PointerType::Touch),
            ScalePhase::Possible,
            "the same growth is well inside the {DEFAULT_SPAN_SLOP} px touch \
             span slop"
        );
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
        // Half the 100 px separation -- see `test_span_calculation`.
        assert!((state.initial_span.expect("captured") - 50.0).abs() < 0.01);

        // Manually test scale calculation by updating pointer and checking span
        drop(state);
        recognizer.handle_pointer_move(
            pointer2,
            Offset::new(Pixels(200.0), Pixels(0.0)),
            PointerType::Touch,
        );

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
