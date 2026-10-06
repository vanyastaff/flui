//! Multi-tap gesture recognizer
//!
//! Recognizes multi-touch tap gestures (N fingers tapping simultaneously).
//!
//! A multi-tap requires:
//! - Specified number of pointers down within time window
//! - All pointers stay within slop tolerance
//! - All pointers released (tap completed)

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use web_time::{Duration, Instant};

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType, extract_pointer_id},
    ids::PointerId,
    routing::PointerDispatch,
    settings::GestureSettings,
};

/// Callback for multi-tap events
pub type MultiTapCallback = Rc<dyn Fn(MultiTapDetails)>;

/// Details about a multi-tap gesture
#[derive(Debug, Clone, PartialEq)]
pub struct MultiTapDetails {
    /// Number of pointers/fingers involved
    pub pointer_count: usize,
    /// Positions of all pointers when tap completed
    pub positions: Vec<Offset<f64>>,
    /// Center point of all taps
    pub center: Offset<f64>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Recognizes multi-tap gestures (multiple simultaneous taps)
///
/// Can detect 2-finger tap, 3-finger tap, etc.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
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
/// recognizer.add_pointer(pointer1, position1, position1);
/// recognizer.add_pointer(pointer2, position2, position2);
/// recognizer.handle_event(PointerDispatch::at_root(&pointer_event));
/// ```
#[derive(Clone)]
pub struct MultiTapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Required number of simultaneous pointers
    required_pointer_count: usize,

    /// Callbacks
    callbacks: Rc<RefCell<MultiTapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<MultiTapState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,

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
    /// Cancelled
    Cancelled,
}

#[derive(Debug, Clone)]
struct PointerInfo {
    /// Initial position
    initial_position: Offset<f64>,
    /// Current position
    current_position: Offset<f64>,
    /// Time when pointer went down
    #[expect(dead_code)]
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
    device_kind: Option<PointerType>,
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
    /// * `required_pointer_count` - Number of simultaneous pointers required
    ///   (2, 3, 4, etc.)
    ///
    /// # Panics
    ///
    /// Panics if `required_pointer_count` is less than 2.
    pub fn new(arena: crate::arena::GestureArena, required_pointer_count: usize) -> Arc<Self> {
        assert!(
            required_pointer_count >= 2,
            "MultiTapGestureRecognizer requires at least 2 pointers, got {required_pointer_count}"
        );
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            required_pointer_count,
            callbacks: Rc::new(RefCell::new(MultiTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(MultiTapState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
            max_time_window: Duration::from_millis(100), // 100ms to get all pointers down
        })
    }

    /// Create a new multi-tap recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        required_pointer_count: usize,
        settings: GestureSettings,
    ) -> Arc<Self> {
        assert!(
            required_pointer_count >= 2,
            "MultiTapGestureRecognizer requires at least 2 pointers, got {required_pointer_count}"
        );
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            required_pointer_count,
            callbacks: Rc::new(RefCell::new(MultiTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(MultiTapState::default())),
            settings: Arc::new(Mutex::new(settings)),
            max_time_window: Duration::from_millis(100),
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

    /// Set the multi-tap callback
    pub fn with_on_multi_tap(
        self: Arc<Self>,
        callback: impl Fn(MultiTapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_multi_tap = Some(Rc::new(callback));
        self
    }

    /// Set the multi-tap cancel callback
    pub fn with_on_multi_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(MultiTapDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_multi_tap_cancel = Some(Rc::new(callback));
        self
    }

    /// Handle pointer down
    fn handle_pointer_down(&self, pointer: PointerId, position: Offset<f64>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            MultiTapPhase::Ready | MultiTapPhase::Collecting => {
                // Add pointer
                let now = self.state.now();

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

                match state.pointers.len().cmp(&self.required_pointer_count) {
                    std::cmp::Ordering::Less => state.phase = MultiTapPhase::Collecting,
                    std::cmp::Ordering::Equal => {
                        // Got all required pointers!
                        state.phase = MultiTapPhase::WaitingForUp;
                    }
                    std::cmp::Ordering::Greater => {
                        // Too many pointers - cancel (don't set phase here, let
                        // handle_cancel do it)
                        drop(state);
                        self.handle_cancel();
                    }
                }
            }
            MultiTapPhase::WaitingForUp => {
                // Already have enough pointers, another one means too many - cancel
                drop(state);
                self.handle_cancel();
            }
            MultiTapPhase::Cancelled => {}
        }
    }

    /// Handle pointer move
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset<f64>, kind: PointerType) {
        // Cache settings to avoid nested locks
        let settings = self.settings.lock().clone();
        let mut state = self.gesture_state.lock();

        if let Some(info) = state.pointers.get_mut(&pointer) {
            info.current_position = position;

            // Check slop
            let delta = position - info.initial_position;
            let distance = delta.distance();

            // Kind-aware: reading the touch tier unconditionally let a
            // pointer from a precise device wander the full finger tolerance
            // before the tap was cancelled.
            if distance > settings.hit_slop(kind) {
                // Moved too far - cancel. Leave the phase alone: `handle_cancel`
                // guards on `phase != Cancelled` and does the transition
                // itself, so setting it here would make that guard reject its
                // own work and strand the recognizer holding its pointers and
                // its arena entry, with no cancel callback.
                drop(state);
                self.handle_cancel();
            }
        }
    }

    /// Handle pointer up
    fn handle_pointer_up(&self, pointer: PointerId, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        if let Some(info) = state.pointers.get_mut(&pointer) {
            info.is_down = false;
        }

        if state.phase == MultiTapPhase::WaitingForUp {
            // Check if all pointers are up
            let all_up = state.pointers.values().all(|info| !info.is_down);

            if all_up {
                // Multi-tap completed! The recognizer resets and stops
                // tracking before user code runs, so a callback that panics or
                // disposes leaves it ready for the next gesture.
                let positions: Vec<Offset<f64>> = state
                    .pointers
                    .values()
                    .map(|info| info.initial_position)
                    .collect();

                let center = Self::calculate_center(&positions);
                let count = positions.len();

                *state = MultiTapState::default();
                drop(state);
                self.state.stop_tracking();

                let callback = self.callbacks.borrow().on_multi_tap.clone();
                if let Some(callback) = callback {
                    callback(MultiTapDetails {
                        pointer_count: count,
                        positions,
                        center,
                        kind,
                    });
                }
            }
        }
    }

    /// Handle cancel
    fn handle_cancel(&self) {
        let mut state = self.gesture_state.lock();

        if state.phase != MultiTapPhase::Ready && state.phase != MultiTapPhase::Cancelled {
            state.phase = MultiTapPhase::Cancelled;

            let positions: Vec<Offset<f64>> = state
                .pointers
                .values()
                .map(|info| info.initial_position)
                .collect();

            let center = if positions.is_empty() {
                Offset::new(0.0, 0.0)
            } else {
                Self::calculate_center(&positions)
            };

            let count = positions.len();
            let kind = state.device_kind.unwrap_or(PointerType::Touch);
            let callback = self.callbacks.borrow().on_multi_tap_cancel.clone();

            *state = MultiTapState::default();
            drop(state);

            self.state.reject();
            if let Some(callback) = callback {
                callback(MultiTapDetails {
                    pointer_count: count,
                    positions,
                    center,
                    kind,
                });
            }
        }
    }

    /// Calculate center point of all positions
    fn calculate_center(positions: &[Offset<f64>]) -> Offset<f64> {
        if positions.is_empty() {
            return Offset::new(0.0, 0.0);
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for pos in positions {
            sum_x += pos.dx;
            sum_y += pos.dy;
        }

        let count = positions.len() as f64;
        Offset::new(sum_x / count, sum_y / count)
    }

    /// Check if time window has expired
    pub fn check_timeout(&self) -> bool {
        let state = self.gesture_state.lock();

        if state.phase == MultiTapPhase::Collecting
            && let Some(first_time) = state.first_down_time
        {
            let elapsed = self.state.now().duration_since(first_time);
            if elapsed > self.max_time_window {
                // Timeout - cancel. As on the slop path, the phase transition
                // belongs to `handle_cancel`: setting `Cancelled` here trips
                // its own `phase != Cancelled` guard and cancels nothing.
                drop(state);
                self.handle_cancel();
                return true;
            }
        }

        false
    }
}

impl GestureRecognizer for MultiTapGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        // Multi-tap's per-pointer callbacks carry no position, so none of them
        // reports the global one. The base records it anyway — the stored
        // contact is one value in two spaces, and half of it is a trap.
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

        self.handle_pointer_down(pointer, position, PointerType::Touch);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        let pointer = extract_pointer_id(event);
        if !self.gesture_state.lock().pointers.contains_key(&pointer) {
            return;
        }
        match event {
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(pos.x, pos.y);
                self.handle_pointer_move(pointer, position, data.pointer.pointer_type);
            }
            PointerEvent::Up(data) => {
                self.handle_pointer_up(pointer, data.pointer.pointer_type);
            }
            PointerEvent::Cancel(_) => {
                self.handle_cancel();
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear tracked pointer, so a disposed
        // recognizer never lingers in the arena for a tracked pointer.
        self.state.reject();
        // Captures are dropped outside the cell, so a capture whose destructor
        // reaches this recognizer finds it unborrowed.
        let callbacks = std::mem::take(&mut *self.callbacks.borrow_mut());
        drop(callbacks);
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
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

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

        let pointer1 = PointerId::new(2).expect("nonzero pointer id");
        let pointer2 = PointerId::new(3).expect("nonzero pointer id");

        // Add two pointers
        recognizer.add_pointer(
            pointer1,
            Offset::new(100.0, 100.0),
            Offset::new(100.0, 100.0),
        );
        recognizer.add_pointer(
            pointer2,
            Offset::new(200.0, 100.0),
            Offset::new(200.0, 100.0),
        );

        // Verify collecting phase
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, MultiTapPhase::WaitingForUp);
        assert_eq!(state.pointers.len(), 2);
        drop(state);

        // Release both pointers
        recognizer.handle_pointer_up(pointer1, PointerType::Touch);
        recognizer.handle_pointer_up(pointer2, PointerType::Touch);

        // Should have called callback
        assert!(*tapped.lock());
        assert_eq!(*tap_count.lock(), 2);
    }
}
