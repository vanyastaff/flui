//! Double tap gesture recognizer
//!
//! Recognizes double tap gestures (two taps in quick succession).
//!
//! A double tap is defined as:
//! - First tap completes (down + up within slop)
//! - Second tap starts within DOUBLE_TAP_TIMEOUT_MS (300ms)
//! - Second tap within DOUBLE_TAP_SLOP (100px) of first tap
//! - Second tap completes successfully
//!
//! Flutter reference: https://api.flutter.dev/flutter/gestures/DoubleTapGestureRecognizer-class.html

use super::recognizer::{GestureRecognizer, GestureRecognizerState};
use crate::arena::GestureArenaMember;
use crate::events::{PointerEvent, PointerEventExt, PointerType};
use crate::ids::PointerId;
use crate::settings::GestureSettings;
use flui_types::Offset;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Callback for double tap events
pub type DoubleTapCallback = Arc<dyn Fn(DoubleTapDetails) + Send + Sync>;

/// Details about a double tap gesture
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleTapDetails {
    /// Global position where double tap occurred
    pub global_position: Offset,
    /// Local position (relative to widget)
    pub local_position: Offset,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Recognizes double tap gestures
///
/// A double tap requires two taps within 300ms and 100px of each other.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = DoubleTapGestureRecognizer::new(arena)
///     .with_on_double_tap(|details| {
///         println!("Double tapped at {:?}", details.global_position);
///     });
///
/// // Handle pointer events
/// recognizer.add_pointer(pointer_id, position);
/// recognizer.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct DoubleTapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: GestureRecognizerState,

    /// Callbacks
    callbacks: Arc<Mutex<DoubleTapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<DoubleTapState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

#[derive(Default)]
struct DoubleTapCallbacks {
    on_double_tap: Option<DoubleTapCallback>,
    on_double_tap_cancel: Option<DoubleTapCallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoubleTapPhase {
    /// Ready to start
    Ready,
    /// First tap down
    FirstDown,
    /// Waiting for second tap
    WaitingForSecond,
    /// Second tap down
    SecondDown,
    /// Completed
    Completed,
    /// Cancelled
    Cancelled,
}

#[derive(Debug, Clone)]
struct DoubleTapState {
    /// Current phase
    phase: DoubleTapPhase,
    /// Position of first tap down
    first_tap_position: Option<Offset>,
    /// Time of first tap completion
    first_tap_time: Option<Instant>,
    /// Current position (for slop detection)
    current_position: Option<Offset>,
    /// Device kind
    device_kind: Option<PointerType>,
}

impl Default for DoubleTapState {
    fn default() -> Self {
        Self {
            phase: DoubleTapPhase::Ready,
            first_tap_position: None,
            first_tap_time: None,
            current_position: None,
            device_kind: None,
        }
    }
}

impl DoubleTapGestureRecognizer {
    /// Create a new double tap recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(DoubleTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(DoubleTapState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a new double tap recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(DoubleTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(DoubleTapState::default())),
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

    /// Get the double tap timeout from settings
    fn double_tap_timeout(&self) -> Duration {
        self.settings.lock().double_tap_timeout()
    }

    /// Get the double tap slop from settings
    fn double_tap_slop(&self) -> f32 {
        self.settings.lock().double_tap_slop()
    }

    /// Set the double tap callback
    pub fn with_on_double_tap(
        self: Arc<Self>,
        callback: impl Fn(DoubleTapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_double_tap = Some(Arc::new(callback));
        self
    }

    /// Set the double tap cancel callback
    pub fn with_on_double_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(DoubleTapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_double_tap_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down
    fn handle_down(&self, position: Offset, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            DoubleTapPhase::Ready => {
                // First tap down
                state.phase = DoubleTapPhase::FirstDown;
                state.first_tap_position = Some(position);
                state.current_position = Some(position);
                state.device_kind = Some(kind);
            }
            DoubleTapPhase::WaitingForSecond => {
                // Second tap down - check timing and distance
                if let Some(first_time) = state.first_tap_time {
                    let elapsed = Instant::now().duration_since(first_time);

                    if elapsed > self.double_tap_timeout() {
                        // Too slow - start over as first tap
                        state.phase = DoubleTapPhase::FirstDown;
                        state.first_tap_position = Some(position);
                        state.first_tap_time = None;
                        state.current_position = Some(position);
                        return;
                    }

                    // Check distance from first tap
                    if let Some(first_pos) = state.first_tap_position {
                        let distance = (position - first_pos).distance();
                        if distance > self.double_tap_slop() {
                            // Too far - start over as first tap
                            state.phase = DoubleTapPhase::FirstDown;
                            state.first_tap_position = Some(position);
                            state.first_tap_time = None;
                            state.current_position = Some(position);
                            return;
                        }
                    }

                    // Good! Second tap down
                    state.phase = DoubleTapPhase::SecondDown;
                    state.current_position = Some(position);
                }
            }
            _ => {}
        }
    }

    /// Handle pointer move
    fn handle_move(&self, position: Offset, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        state.current_position = Some(position);

        match state.phase {
            DoubleTapPhase::FirstDown | DoubleTapPhase::SecondDown => {
                // Check if moved too far (slop detection)
                if self.check_slop(position) {
                    // Moved too far, cancel
                    state.phase = DoubleTapPhase::Cancelled;
                    drop(state);

                    self.handle_cancel(position, kind);
                }
            }
            _ => {}
        }
    }

    /// Handle pointer up
    fn handle_up(&self, position: Offset, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            DoubleTapPhase::FirstDown => {
                // First tap completed
                state.phase = DoubleTapPhase::WaitingForSecond;
                state.first_tap_time = Some(Instant::now());
                state.first_tap_position = Some(position);
            }
            DoubleTapPhase::SecondDown => {
                // Second tap completed - double tap!
                state.phase = DoubleTapPhase::Completed;
                drop(state);

                // Call callback
                if let Some(callback) = self.callbacks.lock().on_double_tap.clone() {
                    let details = DoubleTapDetails {
                        global_position: position,
                        local_position: position,
                        kind,
                    };
                    callback(details);
                }

                // Reset and stop tracking
                self.gesture_state.lock().phase = DoubleTapPhase::Ready;
                self.gesture_state.lock().first_tap_position = None;
                self.gesture_state.lock().first_tap_time = None;
                self.state.stop_tracking();
            }
            _ => {}
        }
    }

    /// Handle cancel
    fn handle_cancel(&self, position: Offset, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        if state.phase != DoubleTapPhase::Ready && state.phase != DoubleTapPhase::Cancelled {
            state.phase = DoubleTapPhase::Cancelled;
            drop(state);

            // Call cancel callback
            if let Some(callback) = self.callbacks.lock().on_double_tap_cancel.clone() {
                let details = DoubleTapDetails {
                    global_position: position,
                    local_position: position,
                    kind,
                };
                callback(details);
            }

            self.state.reject();
        }
    }

    /// Check if pointer moved too far (beyond slop tolerance)
    fn check_slop(&self, current_position: Offset) -> bool {
        if let Some(initial_pos) = self.state.initial_position() {
            let delta = current_position - initial_pos;
            let distance = delta.distance();

            if self.settings.lock().exceeds_touch_slop(distance) {
                return true; // Moved too far
            }
        }
        false
    }

    /// Check if timeout for second tap has expired
    /// Should be called periodically
    pub fn check_timeout(&self) -> bool {
        let mut state = self.gesture_state.lock();

        if state.phase == DoubleTapPhase::WaitingForSecond {
            if let Some(first_time) = state.first_tap_time {
                let elapsed = Instant::now().duration_since(first_time);
                if elapsed > self.double_tap_timeout() {
                    // Timeout - reset to ready
                    state.phase = DoubleTapPhase::Ready;
                    state.first_tap_position = None;
                    state.first_tap_time = None;
                    return true;
                }
            }
        }

        false
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

impl GestureRecognizer for DoubleTapGestureRecognizer {
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
                self.handle_cancel(position, pointer_type);
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        self.callbacks.lock().on_double_tap = None;
        self.callbacks.lock().on_double_tap_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl GestureArenaMember for DoubleTapGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        if let Some(pos) = self.state.initial_position() {
            let kind = self
                .gesture_state
                .lock()
                .device_kind
                .unwrap_or(PointerType::Touch);
            self.handle_cancel(pos, kind);
        }
    }
}

impl std::fmt::Debug for DoubleTapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleTapGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("settings", &self.settings.lock())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;
    use crate::events::make_up_event;

    #[test]
    fn test_double_tap_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = DoubleTapGestureRecognizer::new(arena);

        assert_eq!(recognizer.primary_pointer(), None);
    }

    #[test]
    fn test_double_tap_timing() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer =
            DoubleTapGestureRecognizer::new(arena).with_on_double_tap(move |_details| {
                *tapped_clone.lock() = true;
            });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // First tap
        recognizer.add_pointer(pointer, position);
        let up_event = make_up_event(position, PointerType::Touch);
        recognizer.handle_event(&up_event);

        // Should be waiting for second tap
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, DoubleTapPhase::WaitingForSecond);
        drop(state);

        // Second tap (need to add pointer again for new sequence)
        recognizer.handle_down(position, PointerType::Touch);
        let up_event = make_up_event(position, PointerType::Touch);
        recognizer.handle_event(&up_event);

        // Should have called callback
        assert!(*tapped.lock());
    }

    #[test]
    fn test_double_tap_distance_check() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer =
            DoubleTapGestureRecognizer::new(arena).with_on_double_tap(move |_details| {
                *tapped_clone.lock() = true;
            });

        let pointer = PointerId::new(1);
        let first_pos = Offset::new(100.0, 100.0);

        // First tap
        recognizer.add_pointer(pointer, first_pos);
        let up_event = make_up_event(first_pos, PointerType::Touch);
        recognizer.handle_event(&up_event);

        // Second tap too far away (> 100px)
        let second_pos = Offset::new(250.0, 100.0); // 150px away
        recognizer.handle_down(second_pos, PointerType::Touch);

        // Should have reset to first tap, not double tap
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, DoubleTapPhase::FirstDown);
        drop(state);

        let up_event = make_up_event(second_pos, PointerType::Touch);
        recognizer.handle_event(&up_event);

        // Should NOT have called double tap callback
        assert!(!*tapped.lock());
    }

    #[test]
    fn test_double_tap_timeout() {
        let arena = GestureArena::new();
        let recognizer = DoubleTapGestureRecognizer::new(arena);

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // First tap
        recognizer.add_pointer(pointer, position);
        let up_event = make_up_event(position, PointerType::Touch);
        recognizer.handle_event(&up_event);

        // Wait longer than timeout
        std::thread::sleep(Duration::from_millis(350));

        // Check timeout
        let timed_out = recognizer.check_timeout();
        assert!(timed_out);

        // Should have reset to ready
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, DoubleTapPhase::Ready);
    }
}
