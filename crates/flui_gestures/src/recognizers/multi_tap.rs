//! Multi-tap gesture recognizer
//!
//! Recognizes multi-touch tap gestures (N fingers tapping simultaneously).
//!
//! A multi-tap requires:
//! - Specified number of pointers down within time window
//! - All pointers stay within slop tolerance
//! - All pointers released (tap completed)

use super::recognizer::{constants, GestureRecognizer, GestureRecognizerState};
use crate::arena::{GestureArenaMember, PointerId};
use flui_types::{events::PointerEvent, Offset};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Callback for multi-tap events
pub type MultiTapCallback = Arc<dyn Fn(MultiTapDetails) + Send + Sync>;

/// Details about a multi-tap gesture
#[derive(Debug, Clone)]
pub struct MultiTapDetails {
    /// Number of pointers/fingers involved
    pub pointer_count: usize,
    /// Positions of all pointers when tap completed
    pub positions: Vec<Offset>,
    /// Center point of all taps
    pub center: Offset,
    /// Pointer device kind
    pub kind: flui_types::events::PointerDeviceKind,
}

/// Recognizes multi-tap gestures (multiple simultaneous taps)
///
/// Can detect 2-finger tap, 3-finger tap, etc.
///
/// # Example
///
/// ```rust,ignore
/// use flui_gestures::prelude::*;
///
/// let arena = GestureArena::new();
///
/// // 2-finger tap recognizer
/// let recognizer = MultiTapGestureRecognizer::new(arena, 2)
///     .with_on_multi_tap(|details| {
///         println!("{}-finger tap at center {:?}",
///                  details.pointer_count, details.center);
///     });
///
/// // Add multiple pointers
/// recognizer.add_pointer(pointer1, position1);
/// recognizer.add_pointer(pointer2, position2);
/// recognizer.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct MultiTapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: GestureRecognizerState,

    /// Required number of simultaneous pointers
    required_pointer_count: usize,

    /// Callbacks
    callbacks: Arc<Mutex<MultiTapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<MultiTapState>>,

    /// Maximum time window for all pointers to go down (ms)
    max_time_window: Duration,
}

#[derive(Default)]
struct MultiTapCallbacks {
    on_multi_tap: Option<MultiTapCallback>,
    on_multi_tap_cancel: Option<MultiTapCallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiTapPhase {
    /// Ready to start
    Ready,
    /// Collecting pointers (waiting for N pointers)
    Collecting,
    /// All pointers down, waiting for all up
    WaitingForUp,
    /// Completed successfully
    Completed,
    /// Cancelled
    Cancelled,
}

#[derive(Debug, Clone)]
struct PointerInfo {
    /// Initial position
    initial_position: Offset,
    /// Current position
    current_position: Offset,
    /// Time when pointer went down
    down_time: Instant,
    /// Whether pointer is still down
    is_down: bool,
}

#[derive(Debug, Clone)]
struct MultiTapState {
    /// Current phase
    phase: MultiTapPhase,
    /// Tracked pointers
    pointers: HashMap<PointerId, PointerInfo>,
    /// Time when first pointer went down
    first_down_time: Option<Instant>,
    /// Device kind
    device_kind: Option<flui_types::events::PointerDeviceKind>,
}

impl Default for MultiTapState {
    fn default() -> Self {
        Self {
            phase: MultiTapPhase::Ready,
            pointers: HashMap::new(),
            first_down_time: None,
            device_kind: None,
        }
    }
}

impl MultiTapGestureRecognizer {
    /// Create a new multi-tap recognizer
    ///
    /// # Arguments
    /// * `arena` - Gesture arena for conflict resolution
    /// * `required_pointer_count` - Number of simultaneous pointers required (2, 3, 4, etc.)
    pub fn new(arena: crate::arena::GestureArena, required_pointer_count: usize) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            required_pointer_count,
            callbacks: Arc::new(Mutex::new(MultiTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(MultiTapState::default())),
            max_time_window: Duration::from_millis(100), // 100ms to get all pointers down
        })
    }

    /// Set the multi-tap callback
    pub fn with_on_multi_tap(
        self: Arc<Self>,
        callback: impl Fn(MultiTapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_multi_tap = Some(Arc::new(callback));
        self
    }

    /// Set the multi-tap cancel callback
    pub fn with_on_multi_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(MultiTapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_multi_tap_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down
    fn handle_pointer_down(
        &self,
        pointer: PointerId,
        position: Offset,
        kind: flui_types::events::PointerDeviceKind,
    ) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            MultiTapPhase::Ready | MultiTapPhase::Collecting => {
                // Add pointer
                let now = Instant::now();

                // Check time window if not first pointer
                if let Some(first_time) = state.first_down_time {
                    let elapsed = now.duration_since(first_time);
                    if elapsed > self.max_time_window {
                        // Too slow - reset and start over
                        state.pointers.clear();
                        state.first_down_time = Some(now);
                    }
                } else {
                    // First pointer
                    state.first_down_time = Some(now);
                }

                state.pointers.insert(
                    pointer,
                    PointerInfo {
                        initial_position: position,
                        current_position: position,
                        down_time: now,
                        is_down: true,
                    },
                );

                state.device_kind = Some(kind);

                if state.pointers.len() < self.required_pointer_count {
                    state.phase = MultiTapPhase::Collecting;
                } else if state.pointers.len() == self.required_pointer_count {
                    // Got all required pointers!
                    state.phase = MultiTapPhase::WaitingForUp;
                } else {
                    // Too many pointers - cancel (don't set phase here, let handle_cancel do it)
                    drop(state);
                    self.handle_cancel();
                }
            }
            MultiTapPhase::WaitingForUp => {
                // Already have enough pointers, another one means too many - cancel
                drop(state);
                self.handle_cancel();
            }
            _ => {}
        }
    }

    /// Handle pointer move
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset) {
        let mut state = self.gesture_state.lock();

        if let Some(info) = state.pointers.get_mut(&pointer) {
            info.current_position = position;

            // Check slop
            let delta = position - info.initial_position;
            let distance = delta.distance();

            if distance > constants::TAP_SLOP as f32 {
                // Moved too far - cancel
                state.phase = MultiTapPhase::Cancelled;
                drop(state);
                self.handle_cancel();
            }
        }
    }

    /// Handle pointer up
    fn handle_pointer_up(&self, pointer: PointerId, kind: flui_types::events::PointerDeviceKind) {
        let mut state = self.gesture_state.lock();

        if let Some(info) = state.pointers.get_mut(&pointer) {
            info.is_down = false;
        }

        if state.phase == MultiTapPhase::WaitingForUp {
            // Check if all pointers are up
            let all_up = state.pointers.values().all(|info| !info.is_down);

            if all_up {
                // Multi-tap completed!
                state.phase = MultiTapPhase::Completed;

                let positions: Vec<Offset> = state
                    .pointers
                    .values()
                    .map(|info| info.initial_position)
                    .collect();

                let center = self.calculate_center(&positions);
                let count = positions.len();

                drop(state);

                // Call callback
                if let Some(callback) = self.callbacks.lock().on_multi_tap.clone() {
                    let details = MultiTapDetails {
                        pointer_count: count,
                        positions,
                        center,
                        kind,
                    };
                    callback(details);
                }

                // Reset
                self.gesture_state.lock().phase = MultiTapPhase::Ready;
                self.gesture_state.lock().pointers.clear();
                self.gesture_state.lock().first_down_time = None;
                self.state.stop_tracking();
            }
        }
    }

    /// Handle cancel
    fn handle_cancel(&self) {
        let mut state = self.gesture_state.lock();

        if state.phase != MultiTapPhase::Ready && state.phase != MultiTapPhase::Cancelled {
            state.phase = MultiTapPhase::Cancelled;

            let positions: Vec<Offset> = state
                .pointers
                .values()
                .map(|info| info.initial_position)
                .collect();

            let center = if !positions.is_empty() {
                self.calculate_center(&positions)
            } else {
                Offset::ZERO
            };

            let count = positions.len();
            let kind = state
                .device_kind
                .unwrap_or(flui_types::events::PointerDeviceKind::Touch);

            drop(state);

            // Call cancel callback
            if let Some(callback) = self.callbacks.lock().on_multi_tap_cancel.clone() {
                let details = MultiTapDetails {
                    pointer_count: count,
                    positions,
                    center,
                    kind,
                };
                callback(details);
            }

            self.gesture_state.lock().pointers.clear();
            self.gesture_state.lock().first_down_time = None;
            self.state.reject();
        }
    }

    /// Calculate center point of all positions
    fn calculate_center(&self, positions: &[Offset]) -> Offset {
        if positions.is_empty() {
            return Offset::ZERO;
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for pos in positions {
            sum_x += pos.dx;
            sum_y += pos.dy;
        }

        let count = positions.len() as f32;
        Offset::new(sum_x / count, sum_y / count)
    }

    /// Check if time window has expired
    pub fn check_timeout(&self) -> bool {
        let mut state = self.gesture_state.lock();

        if state.phase == MultiTapPhase::Collecting {
            if let Some(first_time) = state.first_down_time {
                let elapsed = Instant::now().duration_since(first_time);
                if elapsed > self.max_time_window {
                    // Timeout - cancel
                    state.phase = MultiTapPhase::Cancelled;
                    drop(state);
                    self.handle_cancel();
                    return true;
                }
            }
        }

        false
    }
}

impl GestureRecognizer for MultiTapGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset) {
        // For the first pointer, track with arena
        if self.gesture_state.lock().pointers.is_empty() {
            let recognizer = Arc::new(self.clone());
            self.state.start_tracking(pointer, position, &recognizer);
        }

        self.handle_pointer_down(
            pointer,
            position,
            flui_types::events::PointerDeviceKind::Touch,
        );
    }

    fn handle_event(&self, event: &PointerEvent) {
        match event {
            PointerEvent::Move(data) => {
                // In a real implementation, we'd need to know which pointer this is
                // For now, we'll track via primary pointer
                if let Some(pointer) = self.state.primary_pointer() {
                    self.handle_pointer_move(pointer, data.position);
                }
            }
            PointerEvent::Up(data) => {
                if let Some(pointer) = self.state.primary_pointer() {
                    self.handle_pointer_up(pointer, data.device_kind);
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
        self.callbacks.lock().on_multi_tap = None;
        self.callbacks.lock().on_multi_tap_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl GestureArenaMember for MultiTapGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        self.handle_cancel();
    }
}

impl std::fmt::Debug for MultiTapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiTapGestureRecognizer")
            .field("state", &self.state)
            .field("required_pointer_count", &self.required_pointer_count)
            .field("gesture_state", &self.gesture_state.lock())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

    #[test]
    fn test_multi_tap_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = MultiTapGestureRecognizer::new(arena, 2);

        assert_eq!(recognizer.primary_pointer(), None);
        assert_eq!(recognizer.required_pointer_count, 2);
    }

    #[test]
    fn test_two_finger_tap() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tap_count = Arc::new(Mutex::new(0usize));

        let tapped_clone = tapped.clone();
        let count_clone = tap_count.clone();

        let recognizer =
            MultiTapGestureRecognizer::new(arena, 2).with_on_multi_tap(move |details| {
                *tapped_clone.lock() = true;
                *count_clone.lock() = details.pointer_count;
            });

        let pointer1 = PointerId::new(1);
        let pointer2 = PointerId::new(2);

        // Add two pointers
        recognizer.add_pointer(pointer1, Offset::new(100.0, 100.0));
        recognizer.add_pointer(pointer2, Offset::new(200.0, 100.0));

        // Verify collecting phase
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, MultiTapPhase::WaitingForUp);
        assert_eq!(state.pointers.len(), 2);
        drop(state);

        // Release both pointers
        recognizer.handle_pointer_up(pointer1, flui_types::events::PointerDeviceKind::Touch);
        recognizer.handle_pointer_up(pointer2, flui_types::events::PointerDeviceKind::Touch);

        // Should have called callback
        assert!(*tapped.lock());
        assert_eq!(*tap_count.lock(), 2);
    }

    #[test]
    fn test_three_finger_tap() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tap_count = Arc::new(Mutex::new(0usize));

        let tapped_clone = tapped.clone();
        let count_clone = tap_count.clone();

        let recognizer =
            MultiTapGestureRecognizer::new(arena, 3).with_on_multi_tap(move |details| {
                *tapped_clone.lock() = true;
                *count_clone.lock() = details.pointer_count;
            });

        // Add three pointers
        recognizer.add_pointer(PointerId::new(1), Offset::new(100.0, 100.0));
        recognizer.add_pointer(PointerId::new(2), Offset::new(200.0, 100.0));
        recognizer.add_pointer(PointerId::new(3), Offset::new(150.0, 200.0));

        // Verify waiting for up phase
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, MultiTapPhase::WaitingForUp);
        assert_eq!(state.pointers.len(), 3);
        drop(state);

        // Release all pointers
        recognizer.handle_pointer_up(
            PointerId::new(1),
            flui_types::events::PointerDeviceKind::Touch,
        );
        recognizer.handle_pointer_up(
            PointerId::new(2),
            flui_types::events::PointerDeviceKind::Touch,
        );
        recognizer.handle_pointer_up(
            PointerId::new(3),
            flui_types::events::PointerDeviceKind::Touch,
        );

        // Should have called callback
        assert!(*tapped.lock());
        assert_eq!(*tap_count.lock(), 3);
    }

    #[test]
    fn test_center_calculation() {
        let arena = GestureArena::new();
        let center_pos = Arc::new(Mutex::new(Offset::ZERO));
        let center_clone = center_pos.clone();

        let recognizer =
            MultiTapGestureRecognizer::new(arena, 2).with_on_multi_tap(move |details| {
                *center_clone.lock() = details.center;
            });

        // Add two pointers at (0, 0) and (100, 0)
        recognizer.add_pointer(PointerId::new(1), Offset::new(0.0, 0.0));
        recognizer.add_pointer(PointerId::new(2), Offset::new(100.0, 0.0));

        // Release both
        recognizer.handle_pointer_up(
            PointerId::new(1),
            flui_types::events::PointerDeviceKind::Touch,
        );
        recognizer.handle_pointer_up(
            PointerId::new(2),
            flui_types::events::PointerDeviceKind::Touch,
        );

        // Center should be at (50, 0)
        let center = *center_pos.lock();
        assert!((center.dx - 50.0).abs() < 0.01);
        assert!(center.dy.abs() < 0.01);
    }

    #[test]
    fn test_too_many_pointers() {
        let arena = GestureArena::new();
        let cancelled = Arc::new(Mutex::new(false));
        let cancelled_clone = cancelled.clone();

        let recognizer =
            MultiTapGestureRecognizer::new(arena, 2).with_on_multi_tap_cancel(move |_details| {
                *cancelled_clone.lock() = true;
            });

        // Add three pointers (one too many)
        recognizer.add_pointer(PointerId::new(1), Offset::new(100.0, 100.0));
        recognizer.add_pointer(PointerId::new(2), Offset::new(200.0, 100.0));
        recognizer.add_pointer(PointerId::new(3), Offset::new(150.0, 200.0));

        // Should have cancelled
        assert!(*cancelled.lock());
    }
}
