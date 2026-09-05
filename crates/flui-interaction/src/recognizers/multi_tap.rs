//! Multi-tap gesture recognizer
//!
//! Recognizes multi-touch tap gestures (N fingers tapping simultaneously).
//!
//! A multi-tap requires:
//! - Specified number of pointers down within time window
//! - All pointers stay within slop tolerance
//! - All pointers released (tap completed)
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/MultiTapGestureRecognizer-class.html>

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use web_time::{Duration, Instant};

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
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
    pub positions: Vec<Offset<Pixels>>,
    /// Center point of all taps
    pub center: Offset<Pixels>,
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
/// recognizer.add_pointer(pointer1, position1);
/// recognizer.add_pointer(pointer2, position2);
/// recognizer.handle_event(&pointer_event);
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
    /// Completed successfully
    Completed,
    /// Cancelled
    Cancelled,
}

#[derive(Debug, Clone)]
struct PointerInfo {
    /// Initial position
    initial_position: Offset<Pixels>,
    /// Current position
    current_position: Offset<Pixels>,
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
    fn handle_pointer_down(&self, pointer: PointerId, position: Offset<Pixels>, kind: PointerType) {
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
            _ => {}
        }
    }

    /// Handle pointer move
    fn handle_pointer_move(&self, pointer: PointerId, position: Offset<Pixels>, kind: PointerType) {
        // Cache settings to avoid nested locks
        let settings = self.settings.lock().clone();
        let mut state = self.gesture_state.lock();

        if let Some(info) = state.pointers.get_mut(&pointer) {
            info.current_position = position;

            // Check slop
            let delta = position - info.initial_position;
            let distance = delta.distance();

            // Kind-aware, matching `isWithinGlobalTolerance(event,
            // computeHitSlop(event.kind, gestureSettings))` at
            // `multitap.dart:419`. Reading the touch tier unconditionally let a
            // pointer from a precise device wander the full finger tolerance
            // before the tap was cancelled.
            if distance.get() > settings.hit_slop(kind) {
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
                // Multi-tap completed!
                state.phase = MultiTapPhase::Completed;

                let positions: Vec<Offset<Pixels>> = state
                    .pointers
                    .values()
                    .map(|info| info.initial_position)
                    .collect();

                let center = Self::calculate_center(&positions);
                let count = positions.len();

                drop(state);

                // Call callback
                if let Some(callback) = self.callbacks.borrow().on_multi_tap.clone() {
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

            let positions: Vec<Offset<Pixels>> = state
                .pointers
                .values()
                .map(|info| info.initial_position)
                .collect();

            let center = if positions.is_empty() {
                Offset::new(Pixels::ZERO, Pixels::ZERO)
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
    fn calculate_center(positions: &[Offset<Pixels>]) -> Offset<Pixels> {
        if positions.is_empty() {
            return Offset::new(Pixels::ZERO, Pixels::ZERO);
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for pos in positions {
            sum_x += pos.dx.0;
            sum_y += pos.dy.0;
        }

        let count = positions.len() as f32;
        Offset::new(Pixels(sum_x / count), Pixels(sum_y / count))
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
    fn add_pointer(self: &Arc<Self>, pointer: PointerId, position: Offset<Pixels>) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // For the first pointer, track with arena
        if self.gesture_state.lock().pointers.is_empty() {
            self.state.start_tracking(pointer, position, self);
        }

        self.handle_pointer_down(pointer, position, PointerType::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        match event {
            PointerEvent::Move(data) => {
                // In a real implementation, we'd need to know which pointer this is
                // For now, we'll track via primary pointer
                if let Some(pointer) = self.state.primary_pointer() {
                    let pos = data.current.position;
                    let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                    self.handle_pointer_move(pointer, position, data.pointer.pointer_type);
                }
            }
            PointerEvent::Up(data) => {
                if let Some(pointer) = self.state.primary_pointer() {
                    self.handle_pointer_up(pointer, data.pointer.pointer_type);
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
        // Reject arena entries + clear tracked pointer (Flutter parity:
        // gestures/recognizer.dart:485-493 disposing GestureRecognizer
        // clears arena state for tracked pointers).
        self.state.reject();
        self.callbacks.borrow_mut().on_multi_tap = None;
        self.callbacks.borrow_mut().on_multi_tap_cancel = None;
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
    fn test_multi_tap_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = MultiTapGestureRecognizer::new(arena, 2);

        assert_eq!(recognizer.primary_pointer(), None);
        assert_eq!(recognizer.required_pointer_count, 2);
    }

    #[test]
    fn panicking_cancel_callback_cannot_strand_multi_tap_tracking() {
        let arena = GestureArena::new();
        let recognizer = MultiTapGestureRecognizer::new(arena.clone(), 2)
            .with_on_multi_tap_cancel(|_| panic!("multi tap cancel panic"));
        recognizer.add_pointer(PointerId::PRIMARY, Offset::new(Pixels(1.0), Pixels(2.0)));
        arena.close(PointerId::PRIMARY);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recognizer.handle_event(&crate::events::make_cancel_event(PointerType::Touch));
        }));

        assert!(unwind.is_err());
        assert_eq!(recognizer.primary_pointer(), None);
        assert!(arena.is_empty());
    }

    /// A timed-out multi-tap really cancels.
    ///
    /// The timeout path had the same shape as the slop path: it set
    /// `Cancelled` itself, so `handle_cancel`'s `phase != Cancelled` guard
    /// refused to do the cleanup. The recognizer went quiet holding its
    /// pointers and its arena entry — and because the phase *looked* right, a
    /// phase-based oracle would have called that a pass.
    #[test]
    fn a_timed_out_multi_tap_fires_its_cancel_and_releases_everything() {
        use flui_foundation::ManualClock;

        let clock = ManualClock::new();
        let arena = GestureArena::with_clock(Arc::new(clock.clone()));

        let cancelled = Arc::new(Mutex::new(false));
        let flag = cancelled.clone();
        // Three required, only one arrives: the gesture stays `Collecting`,
        // which is the phase `check_timeout` acts on.
        let recognizer = MultiTapGestureRecognizer::new(arena, 3)
            .with_on_multi_tap_cancel(move |_| *flag.lock() = true);

        recognizer.add_pointer(PointerId::PRIMARY, Offset::new(Pixels(0.0), Pixels(0.0)));
        assert_eq!(
            recognizer.gesture_state.lock().phase,
            MultiTapPhase::Collecting,
            "premise: one of three pointers down leaves the gesture collecting"
        );

        assert!(
            !recognizer.check_timeout(),
            "the window has not elapsed yet"
        );

        clock.advance(Duration::from_millis(200));
        assert!(recognizer.check_timeout(), "200ms is past the 100ms window");

        assert!(
            *cancelled.lock(),
            "a timed-out multi-tap must tell its listener"
        );
        assert!(
            recognizer.gesture_state.lock().pointers.is_empty(),
            "and release the pointer it was holding"
        );
        assert!(
            recognizer.primary_pointer().is_none(),
            "and withdraw from the arena instead of blocking competitors"
        );
    }

    /// A mouse cancels a multi-tap at a drift a finger still tolerates.
    ///
    /// `multitap.dart:419` gates this on
    /// `computeHitSlop(event.kind, gestureSettings)`; FLUI read the touch tier
    /// for every device. The drift sits strictly between the two tiers, which
    /// is the only region where the outcome can disagree — under both, or over
    /// both, the gesture survives or cancels regardless of which was read.
    ///
    /// The oracle is the **cancel callback**, not the phase. A stranded
    /// recognizer — one that set `Cancelled` itself and so made
    /// `handle_cancel`'s `phase != Cancelled` guard reject its own cleanup —
    /// reaches the same phase while keeping its pointers, holding its arena
    /// entry, and never telling anyone. Asserting on the phase alone passes on
    /// exactly that broken state, which is what the first draft did.
    #[test]
    fn mouse_drift_cancels_a_multi_tap_a_finger_survives() {
        use crate::settings::DEFAULT_MOUSE_SLOP;

        let touch_slop = GestureSettings::touch_defaults().touch_slop();
        let drift = f32::midpoint(DEFAULT_MOUSE_SLOP, touch_slop);
        assert!(drift > DEFAULT_MOUSE_SLOP && drift < touch_slop);

        // (cancel callback fired, pointers still tracked, still in the arena)
        let outcome_after_drift = |kind: PointerType| {
            let arena = GestureArena::new();
            let cancelled = Arc::new(Mutex::new(false));
            let flag = cancelled.clone();
            let recognizer = MultiTapGestureRecognizer::new(arena.clone(), 2)
                .with_on_multi_tap_cancel(move |_| *flag.lock() = true);

            let origin = Offset::new(Pixels(100.0), Pixels(100.0));
            recognizer.add_pointer(PointerId::PRIMARY, origin);
            recognizer.add_pointer(
                PointerId::new(3).expect("nonzero pointer id"),
                Offset::new(Pixels(200.0), Pixels(100.0)),
            );

            recognizer.handle_event(&crate::events::make_move_event(
                Offset::new(Pixels(100.0 + drift), Pixels(100.0)),
                kind,
            ));

            let tracked = recognizer.gesture_state.lock().pointers.len();
            // `reject` withdraws only this member and deliberately leaves the
            // shared entry standing for any competitor, so the recognizer's own
            // primary pointer — not arena emptiness — is what says it bowed out.
            (
                *cancelled.lock(),
                tracked,
                recognizer.primary_pointer().is_some(),
            )
        };

        assert_eq!(
            outcome_after_drift(PointerType::Mouse),
            (true, 0, false),
            "a mouse drifting {drift} px past the {DEFAULT_MOUSE_SLOP} px mouse \
             slop must fire the cancel callback, release its pointers, and \
             leave the arena"
        );
        assert_eq!(
            outcome_after_drift(PointerType::Touch),
            (false, 2, true),
            "a finger drifting {drift} px is well inside the {touch_slop} px \
             touch slop: no cancel, both pointers still tracked, arena held"
        );
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

        let pointer1 = PointerId::new(2).expect("nonzero pointer id");
        let pointer2 = PointerId::new(3).expect("nonzero pointer id");

        // Add two pointers
        recognizer.add_pointer(pointer1, Offset::new(Pixels(100.0), Pixels(100.0)));
        recognizer.add_pointer(pointer2, Offset::new(Pixels(200.0), Pixels(100.0)));

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
        recognizer.add_pointer(
            PointerId::new(2).expect("nonzero pointer id"),
            Offset::new(Pixels(100.0), Pixels(100.0)),
        );
        recognizer.add_pointer(
            PointerId::new(3).expect("nonzero pointer id"),
            Offset::new(Pixels(200.0), Pixels(100.0)),
        );
        recognizer.add_pointer(
            PointerId::new(4).expect("nonzero pointer id"),
            Offset::new(Pixels(150.0), Pixels(200.0)),
        );

        // Verify waiting for up phase
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, MultiTapPhase::WaitingForUp);
        assert_eq!(state.pointers.len(), 3);
        drop(state);

        // Release all pointers
        recognizer.handle_pointer_up(
            PointerId::new(2).expect("nonzero pointer id"),
            PointerType::Touch,
        );
        recognizer.handle_pointer_up(
            PointerId::new(3).expect("nonzero pointer id"),
            PointerType::Touch,
        );
        recognizer.handle_pointer_up(
            PointerId::new(4).expect("nonzero pointer id"),
            PointerType::Touch,
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
        recognizer.add_pointer(
            PointerId::new(2).expect("nonzero pointer id"),
            Offset::new(Pixels(0.0), Pixels(0.0)),
        );
        recognizer.add_pointer(
            PointerId::new(3).expect("nonzero pointer id"),
            Offset::new(Pixels(100.0), Pixels(0.0)),
        );

        // Release both
        recognizer.handle_pointer_up(
            PointerId::new(2).expect("nonzero pointer id"),
            PointerType::Touch,
        );
        recognizer.handle_pointer_up(
            PointerId::new(3).expect("nonzero pointer id"),
            PointerType::Touch,
        );

        // Center should be at (50, 0)
        let center = *center_pos.lock();
        assert!((center.dx - Pixels(50.0)).abs() < Pixels(0.01));
        assert!(center.dy.abs() < Pixels(0.01));
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
        recognizer.add_pointer(
            PointerId::new(2).expect("nonzero pointer id"),
            Offset::new(Pixels(100.0), Pixels(100.0)),
        );
        recognizer.add_pointer(
            PointerId::new(3).expect("nonzero pointer id"),
            Offset::new(Pixels(200.0), Pixels(100.0)),
        );
        recognizer.add_pointer(
            PointerId::new(4).expect("nonzero pointer id"),
            Offset::new(Pixels(150.0), Pixels(200.0)),
        );

        // Should have cancelled
        assert!(*cancelled.lock());
    }
}
