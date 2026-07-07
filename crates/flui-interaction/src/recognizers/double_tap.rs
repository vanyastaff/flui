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
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/DoubleTapGestureRecognizer-class.html>

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerEventExt, PointerType},
    ids::PointerId,
    settings::GestureSettings,
};

/// Callback for double tap events
pub type DoubleTapCallback = Arc<dyn Fn(DoubleTapDetails) + Send + Sync>;

/// Details about a double tap gesture
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleTapDetails {
    /// Global position where double tap occurred
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
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
    state: RecognizerBase,

    /// Callbacks
    callbacks: Arc<Mutex<DoubleTapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<DoubleTapState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,

    /// The first contact's pointer id, captured on the first up so its held
    /// arena entry can be resolved + released after the second tap (or the
    /// give-up). `None` outside the inter-tap window.
    first_pointer: Arc<Mutex<Option<PointerId>>>,

    /// The first contact's arena member, captured alongside `first_pointer`, so
    /// completion can resolve the first entry in favour of the double-tap
    /// (rejecting the first tap). Held as the exact `Arc` identity the arena
    /// matches on via `Arc::ptr_eq`.
    first_member: Arc<Mutex<Option<Arc<dyn GestureArenaMember>>>>,
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
    first_tap_position: Option<Offset<Pixels>>,
    /// Time of first tap completion
    first_tap_time: Option<Instant>,
    /// Current position (for slop detection)
    current_position: Option<Offset<Pixels>>,
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
            state: RecognizerBase::new(arena),
            callbacks: Arc::new(Mutex::new(DoubleTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(DoubleTapState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
            first_pointer: Arc::new(Mutex::new(None)),
            first_member: Arc::new(Mutex::new(None)),
        })
    }

    /// Create a new double tap recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Arc::new(Mutex::new(DoubleTapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(DoubleTapState::default())),
            settings: Arc::new(Mutex::new(settings)),
            first_pointer: Arc::new(Mutex::new(None)),
            first_member: Arc::new(Mutex::new(None)),
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
    fn handle_down(&self, position: Offset<Pixels>, kind: PointerType) {
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
                // Second tap down: validate timing and distance.
                //
                // When routed through `add_pointer`, expired-window and
                // out-of-slop contacts are intercepted before `start_tracking`
                // and never reach this point. These guards are a safety fallback
                // for direct `handle_down` callers (unit tests).
                let settings = self.settings.lock().clone();

                if let Some(first_time) = state.first_tap_time {
                    let elapsed = self.state.now().duration_since(first_time);

                    if elapsed > settings.double_tap_timeout() {
                        // Window expired. `add_pointer` handles this via
                        // `check_timeout()` before registration. Reaching here
                        // means a direct call: stay in WaitingForSecond and let
                        // the next `check_timeout()` poll clean up the hold.
                        // Must not release the hold here — we don't know whether
                        // `start_tracking` was already called for this contact.
                        return;
                    }

                    // Out-of-slop contact: Flutter ignores it, keeps the first
                    // entry held, and stays in WaitingForSecond (Flutter parity:
                    // `addAllowedPointer` filters out-of-slop contacts before
                    // they compete). Do NOT reset to FirstDown — that would
                    // orphan the held first entry on the next up.
                    if let Some(first_pos) = state.first_tap_position {
                        let distance = (position - first_pos).distance();
                        if distance.get() > settings.double_tap_slop() {
                            return;
                        }
                    }

                    // Valid second tap down.
                    state.phase = DoubleTapPhase::SecondDown;
                    state.current_position = Some(position);
                }
            }
            _ => {}
        }
    }

    /// Handle pointer move
    fn handle_move(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        state.current_position = Some(position);

        // Slop detection: only act on FirstDown / SecondDown when the finger
        // has moved beyond the tap-slop tolerance.
        if matches!(
            state.phase,
            DoubleTapPhase::FirstDown | DoubleTapPhase::SecondDown
        ) && self.check_slop(position)
        {
            // Moved too far, cancel
            state.phase = DoubleTapPhase::Cancelled;
            drop(state);

            self.handle_cancel(position, kind);
        }
    }

    /// Handle pointer up
    fn handle_up(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            DoubleTapPhase::FirstDown => {
                // First tap completed — enter the inter-tap window.
                state.phase = DoubleTapPhase::WaitingForSecond;
                state.first_tap_time = Some(self.state.now());
                state.first_tap_position = Some(position);
                drop(state); // Release before touching the arena.

                // Capture the first contact and HOLD its arena entry across the
                // window (Flutter `_registerFirstTap` -> `gestureArena.hold`).
                // The binding's first-up sweep then sees the entry held and
                // defers, so a competing front-member tap cannot win yet. Do
                // NOT stop tracking — the recognizer stays live for the second
                // contact.
                let first_pointer = self.state.primary_pointer();
                *self.first_pointer.lock() = first_pointer;
                *self.first_member.lock() = self.state.tracked_member();
                if let Some(pointer) = first_pointer {
                    self.state.arena().hold(pointer);
                }
            }
            DoubleTapPhase::SecondDown => {
                // Second tap completed — a double tap.
                state.phase = DoubleTapPhase::Completed;
                drop(state);

                // Resolve BOTH contended entries in favour of the double-tap so
                // neither single tap fires (Flutter `_registerSecondTap`'s two
                // `entry.resolve(accepted)` calls): the held first entry wins for
                // its captured member (rejecting tap1), and the current entry
                // wins for the second contact's tracked member (rejecting tap2).
                // Resolving in *favour of* the double-tap — not `resolve(p,
                // None)` — is required: a no-winner resolve would reject the
                // double-tap's own member and spuriously fire `on_double_tap_cancel`.
                let first_pointer = *self.first_pointer.lock();
                let first_member = self.first_member.lock().take();
                if let (Some(pointer), Some(member)) = (first_pointer, first_member) {
                    self.state.arena().resolve(pointer, Some(member));
                }
                self.state.accept_tracked();

                // Fire the double-tap callback once.
                if let Some(callback) = self.callbacks.lock().on_double_tap.clone() {
                    callback(DoubleTapDetails {
                        global_position: position,
                        local_position: position,
                        kind,
                    });
                }

                // Release the first entry's hold (drains any deferred sweep) and
                // reset (Flutter `_reset` -> `gestureArena.release`).
                if let Some(pointer) = first_pointer {
                    self.state.arena().release(pointer);
                }
                *self.first_pointer.lock() = None;
                {
                    let mut state = self.gesture_state.lock();
                    state.phase = DoubleTapPhase::Ready;
                    state.first_tap_position = None;
                    state.first_tap_time = None;
                }
                self.state.stop_tracking();
            }
            _ => {}
        }
    }

    /// Handle cancel
    fn handle_cancel(&self, position: Offset<Pixels>, kind: PointerType) {
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
    fn check_slop(&self, current_position: Offset<Pixels>) -> bool {
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

        if state.phase == DoubleTapPhase::WaitingForSecond
            && let Some(first_time) = state.first_tap_time
        {
            let elapsed = self.state.now().duration_since(first_time);
            if elapsed > self.double_tap_timeout() {
                // Window expired with no second contact. Flutter parity:
                // `DoubleTapGestureRecognizer._reset` fires `onDoubleTapCancel`,
                // withdraws the double-tap from the held first entry, and
                // releases the hold.
                let position = state.first_tap_position.take().unwrap_or(Offset::ZERO);
                let kind = state.device_kind.unwrap_or(PointerType::Touch);
                state.phase = DoubleTapPhase::Ready;
                state.first_tap_time = None;
                drop(state); // Release lock before callback + arena release

                if let Some(callback) = self.callbacks.lock().on_double_tap_cancel.clone() {
                    callback(DoubleTapDetails {
                        global_position: position,
                        local_position: position,
                        kind,
                    });
                }
                // Withdraw the double-tap from the held first entry. The entry
                // was closed as `[tap1, double_tap]`; removing the double-tap
                // leaves the lone tap, which the closed-arena single-member rule
                // resolves the winner — so the held tap finally fires. Then
                // release the hold to drain any deferred sweep (idempotent — the
                // entry is already resolved or removed).
                let first_pointer = self.first_pointer.lock().take();
                *self.first_member.lock() = None;
                self.state.reject();
                if let Some(pointer) = first_pointer {
                    self.state.arena().release(pointer);
                }
                return true;
            }
        }

        false
    }

    /// Extract position and pointer type from a PointerEvent
    fn extract_event_data(event: &PointerEvent) -> (Offset<Pixels>, PointerType) {
        let position = event.position();
        let pointer_type = match event {
            PointerEvent::Down(e) | PointerEvent::Up(e) => e.pointer.pointer_type,
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
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }

        // Pre-registration checks for contacts arriving while we hold the first
        // entry. Mirror Flutter `addAllowedPointer`: expired-window and
        // out-of-slop contacts are handled *before* the new pointer is registered
        // in the arena so that `reject`/`release` on the first entry runs while
        // `primary_pointer` is still the first pointer — not yet updated by
        // `start_tracking` for the new contact.
        {
            let state = self.gesture_state.lock();
            if state.phase == DoubleTapPhase::WaitingForSecond {
                let settings = self.settings.lock().clone();

                let window_expired = state.first_tap_time.is_some_and(|first_time| {
                    self.state.now().duration_since(first_time) > settings.double_tap_timeout()
                });

                let out_of_slop = state.first_tap_position.is_some_and(|first_pos| {
                    (position - first_pos).distance().get() > settings.double_tap_slop()
                });

                // Release the lock before any re-entrant path (check_timeout re-acquires it).
                drop(state);

                if window_expired {
                    // The inter-tap window has expired. Release the held first
                    // entry (so the lone tap can fire) and fire
                    // `on_double_tap_cancel`. check_timeout() handles all of
                    // this, identical to the periodic poll path, and resets
                    // phase to Ready. Afterwards, start_tracking + handle_down
                    // treat this contact as the new first tap.
                    self.check_timeout();
                    // Fall through: phase is now Ready.
                } else if out_of_slop {
                    // Flutter parity: out-of-slop contacts are ignored — keep
                    // the first entry held and stay in WaitingForSecond.
                    return;
                }
                // In-window + in-slop: fall through to register normally.
            } else {
                drop(state);
            }
        }

        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);
        self.handle_down(position, PointerType::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
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
        // Reject arena entries + clear tracked pointer (Flutter parity:
        // gestures/recognizer.dart:485-493 disposing GestureRecognizer
        // clears arena state for tracked pointers). reject_gesture (fired by
        // state.reject) also drains the inter-tap hold via first_pointer, but
        // that relies on the entry still being active.
        self.state.reject();
        // Belt-and-suspenders: clear first_pointer/first_member even when
        // reject_gesture was a no-op (arena already settled or no active entry).
        // Avoids retaining one Arc<dyn GestureArenaMember> after unmount
        // mid-inter-tap-window.
        let first_ptr = self.first_pointer.lock().take();
        *self.first_member.lock() = None;
        if let Some(ptr) = first_ptr {
            self.state.arena().release(ptr);
        }
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

    fn poll_deadline(&self) {
        // Frame-driven give-up check: after a completed first tap, the
        // inter-tap window must eventually expire if no second tap arrives.
        // Without this poll, `poll_deadlines()` never drives the timeout, so a
        // detector combining `on_tap` + `on_double_tap` could leave the lone
        // single-tap forever holding the arena. `check_timeout` is idempotent
        // (it only fires once the window has elapsed in the `WaitingForSecond`
        // phase) and drops the gesture_state lock before releasing the arena.
        self.check_timeout();
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
        // If a competitor won while we held the first entry across the inter-tap
        // window, drain the hold so the entry is not left held.
        let first_pointer = self.first_pointer.lock().take();
        *self.first_member.lock() = None;
        if let Some(pointer) = first_pointer {
            self.state.arena().release(pointer);
        }
    }
}

impl std::fmt::Debug for DoubleTapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleTapGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("settings", &self.settings.lock())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;
    use crate::{arena::GestureArena, events::make_up_event};

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

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(px(100.0), px(100.0));

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
    fn timeout_fires_double_tap_cancel() {
        // Flutter parity: when the inter-tap window expires after a completed
        // first tap, `onDoubleTapCancel` must fire (previously the state was
        // silently reset and the callback never ran).
        let arena = GestureArena::new();
        let cancelled = Arc::new(Mutex::new(false));
        let cancelled_clone = cancelled.clone();

        // Zero inter-tap window so check_timeout() expires immediately.
        let settings = GestureSettings::new(
            18.0,
            36.0,
            0.05,
            100.0,
            Duration::ZERO,
            Duration::from_millis(500),
            50.0,
            8000.0,
        );
        let recognizer = DoubleTapGestureRecognizer::with_settings(arena, settings)
            .with_on_double_tap_cancel(move |_details| {
                *cancelled_clone.lock() = true;
            });

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(px(100.0), px(100.0));

        // Complete the first tap.
        recognizer.add_pointer(pointer, position);
        let up_event = make_up_event(position, PointerType::Touch);
        recognizer.handle_event(&up_event);
        assert_eq!(
            recognizer.gesture_state.lock().phase,
            DoubleTapPhase::WaitingForSecond
        );

        // Zero timeout: the window expires as soon as measurable time passes
        // (the comparison is strict, so let the clock tick once).
        std::thread::sleep(Duration::from_millis(5));
        assert!(recognizer.check_timeout(), "timeout must be reported");
        assert!(
            *cancelled.lock(),
            "on_double_tap_cancel must fire when the inter-tap window expires"
        );
        assert_eq!(recognizer.gesture_state.lock().phase, DoubleTapPhase::Ready);
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

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let first_pos = Offset::new(px(100.0), px(100.0));

        // First tap
        recognizer.add_pointer(pointer, first_pos);
        let up_event = make_up_event(first_pos, PointerType::Touch);
        recognizer.handle_event(&up_event);

        // Second tap too far away (> 100px)
        let second_pos = Offset::new(px(250.0), px(100.0)); // 150px away
        recognizer.handle_down(second_pos, PointerType::Touch);

        // Flutter parity: an out-of-slop contact is ignored. The recognizer
        // stays in WaitingForSecond with the first entry still held — it does
        // NOT reset to FirstDown, which would orphan the held entry.
        let state = recognizer.gesture_state.lock();
        assert_eq!(state.phase, DoubleTapPhase::WaitingForSecond);
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

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(px(100.0), px(100.0));

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

    #[test]
    fn poll_deadline_drives_the_give_up_timeout() {
        // After a completed first tap the recognizer sits in `WaitingForSecond`,
        // holding its arena entry. The binding's `poll_deadlines()` must drive
        // the inter-tap give-up off the frame tick — without the `poll_deadline`
        // override on the `GestureArenaMember` impl, `poll_deadlines` never calls
        // `check_timeout`, so a lone tap combined with a double-tap recognizer
        // could hold the arena forever. Driven off a `ManualClock` (no sleep).
        let clock = flui_foundation::ManualClock::new();
        let arena = GestureArena::with_clock(Arc::new(clock.clone()));
        let cancelled = Arc::new(Mutex::new(false));
        let cancelled_clone = cancelled.clone();
        let recognizer = DoubleTapGestureRecognizer::new(arena.clone())
            .with_on_double_tap_cancel(move |_| *cancelled_clone.lock() = true);

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(px(10.0), px(10.0));

        // Complete the first tap → `WaitingForSecond`, member still in the arena.
        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&make_up_event(position, PointerType::Touch));

        // Poll before the 300ms window expires: nothing fires.
        arena.poll_deadlines();
        assert!(
            !*cancelled.lock(),
            "no give-up before the inter-tap window expires"
        );

        // Advance past the window; the frame poll must drive `check_timeout`.
        clock.advance(Duration::from_millis(350));
        arena.poll_deadlines();
        assert!(
            *cancelled.lock(),
            "poll_deadlines must drive the double-tap give-up timeout",
        );
    }

    #[test]
    fn double_tap_timeout_is_deterministic_via_virtual_clock() {
        // Same as `test_double_tap_timeout` but with NO wall-clock sleep: a
        // `ManualClock` drives the arena's `now()`, so the inter-tap window
        // expires on virtual time the moment the driver advances past it.
        let clock = flui_foundation::ManualClock::new();
        let arena = GestureArena::with_clock(Arc::new(clock.clone()));
        let recognizer = DoubleTapGestureRecognizer::new(arena);

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(px(100.0), px(100.0));

        // First tap completes → records `first_tap_time` from the virtual clock.
        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&make_up_event(position, PointerType::Touch));

        // Advance virtual time past the 300ms double-tap window — no sleep.
        clock.advance(Duration::from_millis(350));

        assert!(
            recognizer.check_timeout(),
            "the inter-tap window expired on the virtual clock",
        );
        assert_eq!(recognizer.gesture_state.lock().phase, DoubleTapPhase::Ready,);
    }

    #[test]
    fn no_arena_leak_on_mistimed_second_tap() {
        // RED-WITHOUT-FIX: before this fix, `add_pointer` in WaitingForSecond
        // did not release the held first-pointer entry when the second contact
        // arrived too late. The held entry was orphaned: `has_pending_sweep`
        // never drained, member Arcs were retained, and the first tap was
        // silently swallowed.
        //
        // This test asserts that after a too-slow second contact:
        //   (a) the first-pointer entry's stale hold is gone — no pending-sweep
        //       leak (the OLD held entry was drained by check_timeout + release;
        //       a fresh entry for the second contact exists, but without a hold),
        //   (b) the first tap has fired (the tap recognizer's callback ran), and
        //   (c) the double-tap recognizer correctly restarted as a fresh first tap.
        //
        // Driven with a binding_driven arena + ManualClock (deterministic, no sleep).
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{
            arena::run_pointer_lifecycle,
            events::{PointerType, make_down_event, make_up_event},
            recognizers::TapGestureRecognizer,
        };

        let clock = flui_foundation::ManualClock::new();
        let arena = GestureArena::binding_driven(Arc::new(clock.clone()));

        let tap_fired = Arc::new(AtomicBool::new(false));
        let tap_fired_clone = Arc::clone(&tap_fired);

        let position = Offset::new(px(10.0), px(10.0));
        let first_pointer = PointerId::PRIMARY;

        // Two recognizers competing on the same pointer, as under a GestureDetector.
        let tap = TapGestureRecognizer::new(arena.clone())
            .with_on_tap(move |_| tap_fired_clone.store(true, Ordering::SeqCst));
        let double_tap = DoubleTapGestureRecognizer::new(arena.clone());

        // --- First tap: route (add recognizers), then binding closes the arena ---
        let first_down = make_down_event(position, PointerType::Touch);
        tap.add_pointer(first_pointer, position);
        double_tap.add_pointer(first_pointer, position);
        run_pointer_lifecycle(&arena, &first_down); // close → 2 members, contested

        // First up: recognizers handle it first, then binding sweeps.
        // double_tap.handle_event holds the entry; the binding's sweep defers.
        let first_up = make_up_event(position, PointerType::Touch);
        tap.handle_event(&first_up);
        double_tap.handle_event(&first_up);
        run_pointer_lifecycle(&arena, &first_up); // sweep → held → has_pending_sweep

        // The held entry is still in the arena; the tap has not fired yet.
        assert!(
            arena.contains(first_pointer),
            "held entry must not be swept while double-tap waits for the second contact"
        );
        assert!(
            !tap_fired.load(Ordering::SeqCst),
            "tap must not fire while double-tap holds the arena"
        );

        // Advance past the inter-tap window so the second down arrives too late.
        clock.advance(Duration::from_millis(350));

        // --- Second contact (too-slow) ---
        // add_pointer detects the expired window, calls check_timeout internally,
        // releases the hold, and restarts as a fresh first tap.
        let second_down = make_down_event(position, PointerType::Touch);
        run_pointer_lifecycle(&arena, &second_down);
        double_tap.add_pointer(first_pointer, position);
        // After add_pointer:
        //   - check_timeout fired: double-tap withdrew from the first entry,
        //     lone tap won (single-member-wins rule), tap fired.
        //   - release(first_pointer) drained has_pending_sweep, removed old entry.
        //   - start_tracking added a fresh entry for the second contact (no hold).

        // (a) No stale pending-sweep on the fresh entry — the OLD hold was drained.
        assert!(
            !arena.has_pending_sweep(first_pointer),
            "no stale pending-sweep on the fresh entry — old hold was properly drained"
        );
        // (b) The first tap fired: check_timeout resolved the first-tap entry in
        //     favour of the lone tap, which had its pending_up set by handle_event.
        assert!(
            tap_fired.load(Ordering::SeqCst),
            "the first tap must fire when double-tap gives up the held entry"
        );
        // (c) The double-tap restarted: handle_down(Ready) → FirstDown.
        assert_eq!(
            double_tap.gesture_state.lock().phase,
            DoubleTapPhase::FirstDown,
            "double-tap must restart as a fresh first tap for the second contact"
        );
    }
}
