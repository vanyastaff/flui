//! Primary Pointer Gesture Recognizer
//!
//! Extension of OneSequenceGestureRecognizer with a formal state machine
//! for gestures that have clear accept/reject transitions.
//!
//! # State Machine
//!
//! ```text
//! Ready ─────────────────► Possible (on pointer down)
//!   ▲                          │
//!   │                          ├──► Accepted (arena win)
//!   │                          │        │
//!   │                          │        └──► Ready (pointer up)
//!   │                          │
//!   │                          └──► Defunct (arena loss / slop exceeded)
//!   │                                   │
//!   └───────────────────────────────────┘ (all pointers released)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recognizers::{
//!     PrimaryPointerGestureRecognizer,
//!     GestureRecognizerState,
//! };
//!
//! struct TapRecognizer {
//!     state: PrimaryPointerState,
//! }
//!
//! impl TapRecognizer {
//!     fn handle_pointer_down(&mut self, position: Offset) {
//!         self.state.set_state(GestureRecognizerState::Possible);
//!         self.state.set_initial_position(position);
//!     }
//!
//!     fn handle_pointer_move(&mut self, position: Offset) {
//!         if self.state.exceeds_slop(position) {
//!             self.state.set_state(GestureRecognizerState::Defunct);
//!         }
//!     }
//! }
//! ```

use crate::ids::PointerId;
use crate::recognizers::one_sequence::{OneSequenceGestureRecognizer, OneSequenceState};
use crate::settings::GestureSettings;
use flui_types::geometry::Offset;
use std::time::{Duration, Instant};

/// State of a gesture recognizer.
///
/// Follows Flutter's state machine pattern for gesture recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GestureRecognizerState {
    /// No gesture in progress. Ready to start tracking.
    #[default]
    Ready,

    /// Tracking a potential gesture. Not yet accepted or rejected.
    ///
    /// In this state, the recognizer is watching pointer events to
    /// determine if they match the expected gesture pattern.
    Possible,

    /// Gesture has been accepted. Won the arena.
    ///
    /// The recognizer will receive all future events for this pointer
    /// until the gesture completes.
    Accepted,

    /// Gesture was rejected. Lost the arena or exceeded tolerances.
    ///
    /// The recognizer will no longer receive events for this pointer.
    /// Transitions back to Ready when all tracked pointers are released.
    Defunct,
}

impl GestureRecognizerState {
    /// Check if the recognizer is ready to start a new gesture.
    #[inline]
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Check if tracking a possible gesture.
    #[inline]
    pub fn is_possible(self) -> bool {
        matches!(self, Self::Possible)
    }

    /// Check if the gesture was accepted.
    #[inline]
    pub fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// Check if the gesture was rejected.
    #[inline]
    pub fn is_defunct(self) -> bool {
        matches!(self, Self::Defunct)
    }

    /// Check if the recognizer is active (Possible or Accepted).
    #[inline]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Possible | Self::Accepted)
    }

    /// Check if the recognizer can accept events.
    #[inline]
    pub fn can_accept_events(self) -> bool {
        matches!(self, Self::Possible | Self::Accepted)
    }
}

/// Trait for gesture recognizers with primary pointer state machine.
///
/// Extends OneSequenceGestureRecognizer with:
/// - Formal state machine (Ready → Possible → Accepted/Defunct)
/// - Initial position tracking
/// - Slop tolerance checking (pre and post acceptance)
/// - Optional deadline timer support
///
/// # State Transitions
///
/// | From      | To       | Trigger                    |
/// |-----------|----------|----------------------------|
/// | Ready     | Possible | Pointer down               |
/// | Possible  | Accepted | Arena win / gesture match  |
/// | Possible  | Defunct  | Arena loss / slop exceeded |
/// | Accepted  | Ready    | Pointer up / gesture end   |
/// | Defunct   | Ready    | All pointers released      |
pub trait PrimaryPointerGestureRecognizer: OneSequenceGestureRecognizer {
    /// Get the current state.
    fn state(&self) -> GestureRecognizerState;

    /// Set the state.
    fn set_state(&mut self, state: GestureRecognizerState);

    /// Get the initial position when tracking started.
    fn initial_position(&self) -> Option<Offset>;

    /// Set the initial position.
    fn set_initial_position(&mut self, position: Offset);

    /// Get the deadline (if set).
    fn deadline(&self) -> Option<Instant>;

    /// Set a deadline for the gesture.
    ///
    /// If the deadline expires and the gesture is still in Possible state,
    /// the recognizer should resolve (accept or reject).
    fn set_deadline(&mut self, deadline: Option<Instant>);

    /// Check if the deadline has passed.
    fn is_deadline_exceeded(&self) -> bool {
        self.deadline().is_some_and(|d| Instant::now() >= d)
    }

    /// Set deadline from duration.
    fn set_deadline_from_duration(&mut self, duration: Duration) {
        self.set_deadline(Some(Instant::now() + duration));
    }

    /// Clear the deadline.
    fn clear_deadline(&mut self) {
        self.set_deadline(None);
    }

    /// Check if movement exceeds pre-acceptance slop.
    ///
    /// Pre-acceptance slop is the maximum movement allowed while
    /// still in the Possible state.
    fn exceeds_pre_acceptance_slop(&self, position: Offset) -> bool {
        if let Some(initial) = self.initial_position() {
            let delta = position - initial;
            let distance = (delta.dx * delta.dx + delta.dy * delta.dy).sqrt();
            self.settings().exceeds_touch_slop(distance)
        } else {
            false
        }
    }

    /// Check if movement exceeds post-acceptance slop.
    ///
    /// Post-acceptance slop is typically larger or infinite, allowing
    /// more movement after the gesture is accepted.
    fn exceeds_post_acceptance_slop(&self, _position: Offset) -> bool {
        // By default, no post-acceptance slop limit
        false
    }

    /// Handle state transition to Possible.
    fn did_exceed_deadline(&mut self) {
        // Default: reject if deadline exceeded while possible
        if self.state().is_possible() {
            self.set_state(GestureRecognizerState::Defunct);
        }
    }

    /// Called when gesture should accept.
    fn accept(&mut self) {
        self.set_state(GestureRecognizerState::Accepted);
    }

    /// Called when gesture should reject.
    fn reject(&mut self) {
        self.set_state(GestureRecognizerState::Defunct);
    }

    /// Reset to ready state.
    fn reset(&mut self) {
        self.set_state(GestureRecognizerState::Ready);
        self.set_initial_position(Offset::ZERO);
        self.clear_deadline();
        self.stop_tracking_all();
    }
}

/// Helper struct for managing primary pointer state.
///
/// Combines OneSequenceState with state machine management.
#[derive(Debug, Clone)]
pub struct PrimaryPointerState {
    /// Base tracking state.
    pub base: OneSequenceState,
    /// Current state machine state.
    state: GestureRecognizerState,
    /// Initial position when tracking started.
    initial_position: Option<Offset>,
    /// Deadline for gesture resolution.
    deadline: Option<Instant>,
}

impl Default for PrimaryPointerState {
    fn default() -> Self {
        Self::new()
    }
}

impl PrimaryPointerState {
    /// Create new state.
    pub fn new() -> Self {
        Self {
            base: OneSequenceState::new(),
            state: GestureRecognizerState::Ready,
            initial_position: None,
            deadline: None,
        }
    }

    /// Create with settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        Self {
            base: OneSequenceState::with_settings(settings),
            state: GestureRecognizerState::Ready,
            initial_position: None,
            deadline: None,
        }
    }

    /// Get current state.
    pub fn state(&self) -> GestureRecognizerState {
        self.state
    }

    /// Set state.
    pub fn set_state(&mut self, state: GestureRecognizerState) {
        self.state = state;
    }

    /// Get initial position.
    pub fn initial_position(&self) -> Option<Offset> {
        self.initial_position
    }

    /// Set initial position.
    pub fn set_initial_position(&mut self, position: Offset) {
        self.initial_position = Some(position);
    }

    /// Get deadline.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Set deadline.
    pub fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// Set deadline from duration.
    pub fn set_deadline_from_duration(&mut self, duration: Duration) {
        self.deadline = Some(Instant::now() + duration);
    }

    /// Check if deadline exceeded.
    pub fn is_deadline_exceeded(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    /// Check if exceeds slop.
    pub fn exceeds_slop(&self, position: Offset) -> bool {
        if let Some(initial) = self.initial_position {
            let delta = position - initial;
            let distance = (delta.dx * delta.dx + delta.dy * delta.dy).sqrt();
            self.base.settings().exceeds_touch_slop(distance)
        } else {
            false
        }
    }

    /// Get distance from initial position.
    pub fn distance_from_initial(&self, position: Offset) -> f32 {
        if let Some(initial) = self.initial_position {
            let delta = position - initial;
            (delta.dx * delta.dx + delta.dy * delta.dy).sqrt()
        } else {
            0.0
        }
    }

    /// Start tracking with initial position.
    pub fn start_tracking_at(&mut self, pointer: PointerId, position: Offset) {
        self.base.start_tracking(pointer);
        self.initial_position = Some(position);
        self.state = GestureRecognizerState::Possible;
    }

    /// Accept the gesture.
    pub fn accept(&mut self) {
        self.state = GestureRecognizerState::Accepted;
    }

    /// Reject the gesture.
    pub fn reject(&mut self) {
        self.state = GestureRecognizerState::Defunct;
    }

    /// Reset to ready state.
    pub fn reset(&mut self) {
        self.base.reset();
        self.state = GestureRecognizerState::Ready;
        self.initial_position = None;
        self.deadline = None;
    }

    /// Get settings.
    pub fn settings(&self) -> &GestureSettings {
        self.base.settings()
    }

    /// Set settings.
    pub fn set_settings(&mut self, settings: GestureSettings) {
        self.base.set_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_is_ready() {
        assert!(GestureRecognizerState::Ready.is_ready());
        assert!(!GestureRecognizerState::Possible.is_ready());
        assert!(!GestureRecognizerState::Accepted.is_ready());
        assert!(!GestureRecognizerState::Defunct.is_ready());
    }

    #[test]
    fn test_state_is_active() {
        assert!(!GestureRecognizerState::Ready.is_active());
        assert!(GestureRecognizerState::Possible.is_active());
        assert!(GestureRecognizerState::Accepted.is_active());
        assert!(!GestureRecognizerState::Defunct.is_active());
    }

    #[test]
    fn test_state_can_accept_events() {
        assert!(!GestureRecognizerState::Ready.can_accept_events());
        assert!(GestureRecognizerState::Possible.can_accept_events());
        assert!(GestureRecognizerState::Accepted.can_accept_events());
        assert!(!GestureRecognizerState::Defunct.can_accept_events());
    }

    #[test]
    fn test_primary_pointer_state_new() {
        let state = PrimaryPointerState::new();
        assert!(state.state().is_ready());
        assert!(state.initial_position().is_none());
        assert!(state.deadline().is_none());
    }

    #[test]
    fn test_primary_pointer_state_transitions() {
        let mut state = PrimaryPointerState::new();
        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Start tracking
        state.start_tracking_at(pointer, position);
        assert!(state.state().is_possible());
        assert_eq!(state.initial_position(), Some(position));

        // Accept
        state.accept();
        assert!(state.state().is_accepted());

        // Reset
        state.reset();
        assert!(state.state().is_ready());
        assert!(state.initial_position().is_none());
    }

    #[test]
    fn test_primary_pointer_state_reject() {
        let mut state = PrimaryPointerState::new();
        let pointer = PointerId::new(1);

        state.start_tracking_at(pointer, Offset::ZERO);
        state.reject();
        assert!(state.state().is_defunct());
    }

    #[test]
    fn test_exceeds_slop() {
        let mut state = PrimaryPointerState::new();
        let pointer = PointerId::new(1);
        let initial = Offset::new(100.0, 100.0);

        state.start_tracking_at(pointer, initial);

        // Within slop (default 18.0)
        assert!(!state.exceeds_slop(Offset::new(110.0, 100.0)));

        // Beyond slop
        assert!(state.exceeds_slop(Offset::new(130.0, 100.0)));
    }

    #[test]
    fn test_distance_from_initial() {
        let mut state = PrimaryPointerState::new();
        let pointer = PointerId::new(1);

        state.start_tracking_at(pointer, Offset::new(0.0, 0.0));

        let distance = state.distance_from_initial(Offset::new(3.0, 4.0));
        assert!((distance - 5.0).abs() < 0.001); // 3-4-5 triangle
    }

    #[test]
    fn test_deadline() {
        let mut state = PrimaryPointerState::new();

        assert!(!state.is_deadline_exceeded());

        // Set deadline in the past
        state.set_deadline(Some(Instant::now() - Duration::from_secs(1)));
        assert!(state.is_deadline_exceeded());

        // Set deadline in the future
        state.set_deadline(Some(Instant::now() + Duration::from_secs(10)));
        assert!(!state.is_deadline_exceeded());
    }

    #[test]
    fn test_deadline_from_duration() {
        let mut state = PrimaryPointerState::new();

        state.set_deadline_from_duration(Duration::from_secs(10));
        assert!(!state.is_deadline_exceeded());
        assert!(state.deadline().is_some());
    }
}
