//! Long press gesture recognizer
//!
//! Recognizes long press gestures (pointer held down for duration).
//!
//! A long press is defined as:
//! - Pointer down
//! - Pointer stays within touch_slop of initial position
//! - Pointer held for long_press_timeout (default 500ms)
//! - Optional move updates while pressed
//! - Pointer up
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/LongPressGestureRecognizer-class.html>

use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;
use tracing::instrument;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    settings::GestureSettings,
};

/// Callback for long press down events (initial contact)
pub type LongPressDownCallback = Rc<dyn Fn(LongPressDownDetails)>;

/// Callback for simple long press recognition (no details)
pub type LongPressSimpleCallback = Rc<dyn Fn()>;

/// Callback for long press start events
pub type LongPressStartCallback = Rc<dyn Fn(LongPressStartDetails)>;

/// Callback for long press move/up/cancel events
pub type LongPressCallback = Rc<dyn Fn(LongPressDetails)>;

/// Details about long press down (initial contact)
#[derive(Debug, Clone, PartialEq)]
pub struct LongPressDownDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Global position where pointer contacted screen
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about long press start
#[derive(Debug, Clone, PartialEq)]
pub struct LongPressStartDetails {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Global position where long press started
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Details about long press event
#[derive(Debug, Clone, PartialEq)]
pub struct LongPressDetails {
    /// Global position
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Recognizes long press gestures
///
/// A long press is a pointer down held for at least 500ms without moving
/// more than 18px from the initial position.
///
/// # Example
///
/// ```rust
/// use flui_interaction::arena::GestureArena;
/// use flui_interaction::recognizers::LongPressGestureRecognizer;
///
/// let arena = GestureArena::new();
/// let recognizer = LongPressGestureRecognizer::new(arena)
///     .with_on_long_press_start(|details| {
///         // fires after the long-press timer elapses without the
///         // pointer moving past the touch slop
///         let _pos = details.global_position;
///     })
///     .with_on_long_press_up(|details| {
///         let _pos = details.global_position;
///     });
/// // `add_pointer` is wired up by the gesture binding at runtime;
/// // see `flui_interaction::GestureBinding` for the integration.
#[derive(Clone)]
pub struct LongPressGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Rc<RefCell<LongPressCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<LongPressState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

// Field names keep Flutter's `onLongPressStart`-style callback names (parity).
#[allow(clippy::struct_field_names)]
#[derive(Default)]
struct LongPressCallbacks {
    on_long_press_down: Option<LongPressDownCallback>,
    on_long_press: Option<LongPressSimpleCallback>,
    on_long_press_start: Option<LongPressStartCallback>,
    on_long_press_move_update: Option<LongPressCallback>,
    on_long_press_up: Option<LongPressCallback>,
    on_long_press_end: Option<LongPressCallback>,
    on_long_press_cancel: Option<LongPressCallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LongPressPhase {
    /// Ready to start
    #[default]
    Ready,
    /// Pointer down, waiting for timer
    Possible,
    /// Timer elapsed, long press started
    Started,
    /// Cancelled (moved too far or rejected)
    Cancelled,
}

#[derive(Debug, Clone, Default)]
struct LongPressState {
    /// Current phase — `Ready` is the default start state.
    phase: LongPressPhase,
    /// Time when pointer went down
    down_time: Option<Instant>,
    /// Current position
    current_position: Option<Offset<Pixels>>,
    /// Pointer device kind
    device_kind: Option<PointerType>,
}

impl LongPressGestureRecognizer {
    /// Create a new long press recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(LongPressCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(LongPressState::default())),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a new long press recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Rc::new(RefCell::new(LongPressCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(LongPressState::default())),
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

    /// Get the long press duration from settings
    fn long_press_duration(&self) -> Duration {
        self.settings.lock().long_press_timeout()
    }

    /// Set the long press down callback (called on initial contact)
    ///
    /// This is triggered immediately when a pointer contacts the screen,
    /// before the long press timer has elapsed.
    pub fn with_on_long_press_down(
        self: Arc<Self>,
        callback: impl Fn(LongPressDownDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press_down = Some(Rc::new(callback));
        self
    }

    /// Set the simple long press callback (called when gesture is recognized)
    ///
    /// This is a simple callback with no details, called when the long press
    /// duration threshold is reached. For detailed information, use
    /// `with_on_long_press_start` instead.
    pub fn with_on_long_press(self: Arc<Self>, callback: impl Fn() + 'static) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press = Some(Rc::new(callback));
        self
    }

    /// Set the long press start callback (called when timer elapses)
    pub fn with_on_long_press_start(
        self: Arc<Self>,
        callback: impl Fn(LongPressStartDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press_start = Some(Rc::new(callback));
        self
    }

    /// Set the long press move callback (called during long press if pointer
    /// moves)
    pub fn with_on_long_press_move_update(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press_move_update = Some(Rc::new(callback));
        self
    }

    /// Set the long press up callback (called when pointer released after long
    /// press)
    pub fn with_on_long_press_up(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press_up = Some(Rc::new(callback));
        self
    }

    /// Set the long press end callback (called after up, with details)
    ///
    /// Similar to `on_long_press_up` but called after the up event is
    /// processed. This follows Flutter's pattern of having both
    /// onLongPressUp and onLongPressEnd.
    pub fn with_on_long_press_end(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press_end = Some(Rc::new(callback));
        self
    }

    /// Set the long press cancel callback
    pub fn with_on_long_press_cancel(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + 'static,
    ) -> Arc<Self> {
        self.callbacks.borrow_mut().on_long_press_cancel = Some(Rc::new(callback));
        self
    }

    /// Handle pointer down event
    fn handle_down(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.gesture_state.lock();
        state.phase = LongPressPhase::Possible;
        state.down_time = Some(self.state.now());
        state.current_position = Some(position);
        state.device_kind = Some(kind);
        drop(state); // Release lock before callback

        // Call on_long_press_down callback (initial contact)
        if let Some(callback) = self.callbacks.borrow().on_long_press_down.clone() {
            let details = LongPressDownDetails {
                global_position: position,
                local_position: position,
                kind,
            };
            callback(details);
        }
    }

    /// Handle pointer move event
    fn handle_move(&self, position: Offset<Pixels>, kind: PointerType) {
        // Cache settings to avoid multiple locks
        let settings = self.settings.lock().clone();
        let mut state = self.gesture_state.lock();

        match state.phase {
            LongPressPhase::Possible => {
                // Check if moved too far (slop detection)
                if let Some(initial_pos) = self.state.initial_position() {
                    let delta = position - initial_pos;
                    if settings.exceeds_touch_slop(delta.distance()) {
                        // Moved too far, cancel
                        drop(state); // Release lock before calling handle_cancel
                        self.handle_cancel(position, kind);
                        return;
                    }
                }
                drop(state); // Release lock before firing callbacks

                // Delegate timer-elapsed resolution to the shared helper —
                // identical logic powers `check_timer` and the deadline
                // hook below.
                self.try_fire_timer(position);
            }
            LongPressPhase::Started => {
                // Long press already started, update position
                state.current_position = Some(position);
                drop(state); // Release lock before calling callback

                // Call on_long_press_move_update callback
                if let Some(callback) = self.callbacks.borrow().on_long_press_move_update.clone() {
                    let details = LongPressDetails {
                        global_position: position,
                        local_position: position,
                        kind,
                    };
                    callback(details);
                }
            }
            _ => {}
        }
    }

    /// Handle pointer up event
    fn handle_up(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        match state.phase {
            LongPressPhase::Possible => {
                // Pointer up before timer elapsed - just cancel silently.
                state.phase = LongPressPhase::Ready;
                // Release the lock first: stop_tracking() sweeps the arena,
                // which can synchronously reject THIS recognizer, and
                // reject_gesture -> handle_cancel re-locks gesture_state
                // (parking_lot is non-reentrant -> deadlock).
                drop(state);
                self.state.stop_tracking();
            }
            LongPressPhase::Started => {
                // Long press completed successfully
                state.phase = LongPressPhase::Ready;
                drop(state); // Release lock before calling callback

                let details = LongPressDetails {
                    global_position: position,
                    local_position: position,
                    kind,
                };

                // Call on_long_press_up callback
                if let Some(callback) = self.callbacks.borrow().on_long_press_up.clone() {
                    callback(details.clone());
                }

                // Call on_long_press_end callback
                if let Some(callback) = self.callbacks.borrow().on_long_press_end.clone() {
                    callback(details);
                }

                self.state.stop_tracking();
            }
            _ => {}
        }
    }

    /// Handle cancel event
    fn handle_cancel(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.gesture_state.lock();

        if state.phase == LongPressPhase::Started || state.phase == LongPressPhase::Possible {
            state.phase = LongPressPhase::Cancelled;
            drop(state); // Release lock before calling callback

            // Call on_long_press_cancel callback
            if let Some(callback) = self.callbacks.borrow().on_long_press_cancel.clone() {
                let details = LongPressDetails {
                    global_position: position,
                    local_position: position,
                    kind,
                };
                callback(details);
            }

            self.state.reject();
        }
    }

    /// Check if the long-press deadline has elapsed and, if so, fire
    /// the start callbacks + advance state to `Started`. Returns
    /// `true` when the deadline fired.
    ///
    /// This is the single timer-elapsed resolution path — called from
    /// [`Self::check_timer`] (the event-loop tick driver), from
    /// [`Self::handle_move`] (move events carry their own deadline
    /// resolution), and from `did_exceed_deadline` (the parent
    /// `PrimaryPointerGestureRecognizer` deadline hook). Extracting
    /// it once keeps the three call sites in lock-step with Flutter
    /// `long_press.dart::_checkLongPressStart` semantics.
    #[instrument(
        name = "long_press.try_fire_timer",
        level = "trace",
        skip(self),
        fields(pointer = ?self.state.primary_pointer())
    )]
    fn try_fire_timer(&self, position: Offset<Pixels>) -> bool {
        // Snapshot under the lock, then drop it before invoking
        // user callbacks (callbacks may re-enter recognizer API).
        let snapshot = {
            let mut state = self.gesture_state.lock();
            if state.phase != LongPressPhase::Possible {
                return false;
            }
            let Some(down_time) = state.down_time else {
                return false;
            };
            if self.state.now().duration_since(down_time) < self.long_press_duration() {
                return false;
            }
            state.phase = LongPressPhase::Started;
            state.current_position = Some(position);
            (state.device_kind, position)
        };
        let (kind, fired_pos) = snapshot;

        if let Some(callback) = self.callbacks.borrow().on_long_press.clone() {
            callback();
        }
        if let Some(callback) = self.callbacks.borrow().on_long_press_start.clone() {
            let details = LongPressStartDetails {
                global_position: fired_pos,
                local_position: fired_pos,
                kind: kind.unwrap_or(PointerType::Touch),
            };
            callback(details);
        }
        true
    }

    /// Check if long press timer has elapsed.
    ///
    /// This should be called periodically by the event loop. Returns
    /// `true` when the deadline fired this tick.
    pub fn check_timer(&self) -> bool {
        let position = self
            .gesture_state
            .lock()
            .current_position
            .unwrap_or_else(|| Offset::new(Pixels(0.0), Pixels(0.0)));
        self.try_fire_timer(position)
    }
}

impl GestureRecognizer for LongPressGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "long_press.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // Start tracking this pointer
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);

        // Handle pointer down
        self.handle_down(position, PointerType::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "long_press.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Only process if we're tracking a pointer
        if self.state.primary_pointer().is_none() {
            return;
        }

        match event {
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                self.handle_move(position, data.pointer.pointer_type);
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                self.handle_up(position, data.pointer.pointer_type);
            }
            PointerEvent::Cancel(info) => {
                // Cancel doesn't have position, use last known position
                if let Some(pos) = self.state.initial_position() {
                    self.handle_cancel(pos, info.pointer_type);
                }
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
        let mut callbacks = self.callbacks.borrow_mut();
        callbacks.on_long_press_down = None;
        callbacks.on_long_press = None;
        callbacks.on_long_press_start = None;
        callbacks.on_long_press_move_update = None;
        callbacks.on_long_press_up = None;
        callbacks.on_long_press_end = None;
        callbacks.on_long_press_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// Flutter parity: `long_press.dart:262 LongPressGestureRecognizer extends
// PrimaryPointerGestureRecognizer`.

impl crate::recognizers::OneSequenceGestureRecognizer for LongPressGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, _pointer: PointerId, disposition: crate::arena::GestureDisposition) {
        match disposition {
            crate::arena::GestureDisposition::Accepted => {
                // No-op — long-press callbacks fire from timer/up handlers,
                // not from arena resolution. accept_gesture below mirrors.
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

impl crate::recognizers::PrimaryPointerGestureRecognizer for LongPressGestureRecognizer {
    fn initial_position(&self) -> Option<Offset<Pixels>> {
        self.state.initial_position()
    }

    fn deadline(&self) -> Option<std::time::Duration> {
        // LongPress has a pre-acceptance deadline from settings.
        Some(self.settings.lock().long_press_timeout())
    }

    fn did_exceed_deadline(&self) {
        // The long-press deadline expiring IS acceptance: fire the start
        // callbacks AND win the arena so competing recognizers (e.g. a tap on
        // the same region) are rejected. Flutter parity:
        // `long_press.dart::didExceedDeadline` -> `resolve(accepted)`.
        let position = self
            .gesture_state
            .lock()
            .current_position
            .or_else(|| self.initial_position())
            .unwrap_or_else(|| Offset::new(Pixels(0.0), Pixels(0.0)));
        self.try_fire_timer(position);
        self.state.accept_tracked();
    }

    fn handle_primary_pointer(&self, event: &PointerEvent) {
        <Self as GestureRecognizer>::handle_event(self, event);
    }
}

impl GestureArenaMember for LongPressGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
        // Callbacks will be called when timer elapses or pointer moves/up
    }

    fn poll_deadline(&self) {
        // Frame-driven deadline check: fires `on_long_press_start` once the
        // hold deadline elapses even if the finger is held still (no further
        // pointer event arrives to drive it). `check_timer` is idempotent, so
        // polling every frame fires at most once.
        //
        // When the deadline fires, ALSO win the arena — mirroring
        // `did_exceed_deadline`. Without this, a held long-press in a
        // multi-recognizer detector fires its callback but never rejects the
        // competing tap, so the tap would still fire on release. `check_timer`
        // returns having already dropped the gesture_state lock, so resolving
        // here is re-entrancy-safe (the arena defers member notifications out
        // of its own lock).
        if self.check_timer() {
            self.state.accept_tracked();
        }
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

impl std::fmt::Debug for LongPressGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LongPressGestureRecognizer")
            .field("state", &self.state)
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
    fn test_long_press_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = LongPressGestureRecognizer::new(arena);

        assert_eq!(recognizer.primary_pointer(), None);
    }

    #[test]
    fn deadline_rejects_competing_arena_member() {
        use crate::recognizers::PrimaryPointerGestureRecognizer;

        struct Competitor {
            rejected: Arc<Mutex<bool>>,
        }
        impl crate::sealed::arena_member::Sealed for Competitor {}
        impl crate::arena::GestureArenaMember for Competitor {
            fn accept_gesture(&self, _pointer: PointerId) {}
            fn reject_gesture(&self, _pointer: PointerId) {
                *self.rejected.lock() = true;
            }
        }

        let arena = GestureArena::new();
        let recognizer = LongPressGestureRecognizer::new(arena.clone());
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        recognizer.add_pointer(pointer, Offset::new(Pixels(10.0), Pixels(10.0)));

        // A competing recognizer (e.g. a tap) contends for the same pointer.
        let rejected = Arc::new(Mutex::new(false));
        arena.add(
            pointer,
            Arc::new(Competitor {
                rejected: rejected.clone(),
            }),
        );

        // The deadline expiring must win the arena and reject the competitor
        // (Flutter parity: `didExceedDeadline` -> `resolve(accepted)`).
        recognizer.did_exceed_deadline();

        assert!(
            *rejected.lock(),
            "competing member should be rejected when the long-press deadline fires"
        );
    }

    #[test]
    fn up_before_deadline_with_competitor_does_not_deadlock() {
        // Regression: handle_up's Possible branch used to hold the
        // gesture_state lock across stop_tracking(). stop_tracking sweeps the
        // arena; when the sweep resolves in favor of an earlier member, THIS
        // recognizer is rejected synchronously and handle_cancel re-locks
        // gesture_state -> guaranteed self-deadlock on any lift-before-
        // deadline interaction with a competitor.
        struct Competitor;
        impl crate::sealed::arena_member::Sealed for Competitor {}
        impl crate::arena::GestureArenaMember for Competitor {
            fn accept_gesture(&self, _pointer: PointerId) {}
            fn reject_gesture(&self, _pointer: PointerId) {}
        }

        let arena = GestureArena::new();
        let pointer = PointerId::new(3).expect("nonzero pointer id");
        // Competitor joins FIRST so the sweep accepts it and rejects the
        // long press.
        arena.add(pointer, Arc::new(Competitor));

        let recognizer = LongPressGestureRecognizer::new(arena.clone());
        let position = Offset::new(Pixels(10.0), Pixels(10.0));
        recognizer.add_pointer(pointer, position);

        // Lift before the deadline: must complete without deadlocking.
        recognizer.handle_up(position, PointerType::Touch);
        assert_eq!(recognizer.primary_pointer(), None);
    }

    #[test]
    fn test_long_press_timer() {
        let arena = GestureArena::new();
        let pressed = Arc::new(Mutex::new(false));
        let pressed_clone = pressed.clone();

        let recognizer =
            LongPressGestureRecognizer::new(arena).with_on_long_press_start(move |_details| {
                *pressed_clone.lock() = true;
            });

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(Pixels(100.0), Pixels(100.0));

        // Start long press
        recognizer.add_pointer(pointer, position);

        // Check immediately - should not be pressed yet
        assert!(!*pressed.lock());

        // Wait for timer (500ms + margin)
        std::thread::sleep(Duration::from_millis(550));

        // Check timer
        recognizer.check_timer();

        // Should have called callback
        assert!(*pressed.lock());
    }

    #[test]
    fn test_long_press_cancelled_by_movement() {
        let arena = GestureArena::new();
        let pressed = Arc::new(Mutex::new(false));
        let cancelled = Arc::new(Mutex::new(false));

        let pressed_clone = pressed.clone();
        let cancelled_clone = cancelled.clone();

        let recognizer = LongPressGestureRecognizer::new(arena)
            .with_on_long_press_start(move |_details| {
                *pressed_clone.lock() = true;
            })
            .with_on_long_press_cancel(move |_details| {
                *cancelled_clone.lock() = true;
            });

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let start_pos = Offset::new(Pixels(100.0), Pixels(100.0));

        // Start long press
        recognizer.add_pointer(pointer, start_pos);

        // Move too far (beyond TAP_SLOP = 18px)
        let moved_pos = Offset::new(Pixels(100.0), Pixels(130.0)); // 30px away
        recognizer.handle_event(&crate::events::make_move_event(
            moved_pos,
            PointerType::Touch,
        ));

        // Should have cancelled
        assert!(*cancelled.lock());
        assert!(!*pressed.lock());
    }

    #[test]
    fn test_long_press_with_move_update() {
        let arena = GestureArena::new();
        let moved = Arc::new(Mutex::new(false));
        let moved_clone = moved.clone();

        let recognizer = LongPressGestureRecognizer::new(arena)
            .with_on_long_press_start(|_| {})
            .with_on_long_press_move_update(move |_details| {
                *moved_clone.lock() = true;
            });

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(Pixels(100.0), Pixels(100.0));

        // Start long press
        recognizer.add_pointer(pointer, position);

        // Wait for timer
        std::thread::sleep(Duration::from_millis(550));
        recognizer.check_timer();

        // Move slightly (within slop)
        let moved_pos = Offset::new(Pixels(105.0), Pixels(105.0));
        recognizer.handle_event(&crate::events::make_move_event(
            moved_pos,
            PointerType::Touch,
        ));

        // Should have called move callback
        assert!(*moved.lock());
    }

    // ========================================================================
    // deadline-hook + shared-timer-helper coverage.
    // ========================================================================

    /// `PrimaryPointerGestureRecognizer::did_exceed_deadline` must
    /// fire `on_long_press_start` AND resolve the arena. Pre-fix the
    /// hook only resolved silently, leaving deadline-driven acceptance
    /// a no-op for callers.
    #[test]
    fn did_exceed_deadline_fires_start_callbacks() {
        use crate::recognizers::PrimaryPointerGestureRecognizer;
        use std::time::Duration;

        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let s_clone = started.clone();

        let recognizer = LongPressGestureRecognizer::with_settings(
            arena,
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(100)),
        )
        .with_on_long_press_start(move |_| *s_clone.lock() = true);

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(Pixels(100.0), Pixels(100.0));
        recognizer.add_pointer(pointer, position);

        // Wait past the deadline.
        std::thread::sleep(Duration::from_millis(150));
        recognizer.did_exceed_deadline();

        assert!(*started.lock());
    }

    /// `try_fire_timer` is the single source of truth for
    /// deadline-elapsed resolution — `check_timer`, the move-driven
    /// path, and `did_exceed_deadline` all funnel through it. Verify
    /// it returns `false` before the deadline and `true` after, and
    /// does not refire once `Started` is reached.
    #[test]
    fn try_fire_timer_is_idempotent() {
        use std::time::Duration;

        let arena = GestureArena::new();
        let started_count = Arc::new(Mutex::new(0u32));
        let c_clone = started_count.clone();

        let recognizer = LongPressGestureRecognizer::with_settings(
            arena,
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(80)),
        )
        .with_on_long_press(move || *c_clone.lock() += 1);

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(Pixels(50.0), Pixels(50.0));
        recognizer.add_pointer(pointer, position);

        // Before deadline — no fire.
        assert!(!recognizer.check_timer());
        assert_eq!(*started_count.lock(), 0);

        // Wait past the deadline.
        std::thread::sleep(Duration::from_millis(120));

        // First tick — fires.
        assert!(recognizer.check_timer());
        assert_eq!(*started_count.lock(), 1);

        // Second tick — must not refire (phase is now `Started`).
        assert!(!recognizer.check_timer());
        assert_eq!(*started_count.lock(), 1);
    }

    #[test]
    fn poll_deadline_wins_the_arena_when_it_fires() {
        // The frame-driven deadline poll must not only fire the long-press
        // callback but WIN the arena, so a competing member (e.g. a tap on the
        // same region) is rejected. Mirrors `did_exceed_deadline`. Without the
        // `accept_tracked` in `poll_deadline`, a frame-polled long-press leaves
        // its competitor live, so a held press would let the tap also fire on
        // release. Driven entirely off a `ManualClock` (no sleep).
        struct Competitor {
            rejected: Arc<Mutex<bool>>,
        }
        impl crate::sealed::arena_member::Sealed for Competitor {}
        impl crate::arena::GestureArenaMember for Competitor {
            fn accept_gesture(&self, _: PointerId) {}
            fn reject_gesture(&self, _: PointerId) {
                *self.rejected.lock() = true;
            }
        }

        let clock = flui_foundation::ManualClock::new();
        let arena = GestureArena::with_clock(Arc::new(clock.clone()));
        let recognizer = LongPressGestureRecognizer::with_settings(
            arena.clone(),
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(100)),
        );
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        recognizer.add_pointer(pointer, Offset::new(Pixels(10.0), Pixels(10.0)));

        // A competing recognizer (e.g. a tap) joins the same arena entry.
        let rejected = Arc::new(Mutex::new(false));
        arena.add(
            pointer,
            Arc::new(Competitor {
                rejected: rejected.clone(),
            }),
        );
        arena.close(pointer);

        // Before the deadline: the frame poll fires nothing and rejects no one.
        arena.poll_deadlines();
        assert!(
            !*rejected.lock(),
            "no resolution before the hold deadline elapses"
        );

        // Past the deadline: the frame poll fires the long-press AND wins the
        // arena, rejecting the competitor.
        clock.advance(Duration::from_millis(150));
        arena.poll_deadlines();
        assert!(
            *rejected.lock(),
            "poll_deadline must win the arena and reject the competing member",
        );
    }

    #[test]
    fn held_pointer_fires_long_press_via_arena_poll() {
        use std::time::Duration;

        // A finger held perfectly still past the deadline must fire
        // `on_long_press_start` when the binding polls deadlines once per frame,
        // with NO intervening pointer event to drive it. This exercises the real
        // wiring `GestureArena::poll_deadlines` -> `poll_deadline` ->
        // `check_timer` -> fire, not the recognizer's deadline hook directly.
        let arena = GestureArena::new();
        let started = Arc::new(Mutex::new(false));
        let s_clone = started.clone();

        let recognizer = LongPressGestureRecognizer::with_settings(
            arena.clone(),
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(60)),
        )
        .with_on_long_press_start(move |_| *s_clone.lock() = true);

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        recognizer.add_pointer(pointer, Offset::new(Pixels(100.0), Pixels(100.0)));

        // No moves. Polling before the deadline elapses fires nothing.
        arena.poll_deadlines();
        assert!(
            !*started.lock(),
            "must not fire before the deadline elapses"
        );

        // Hold still past the deadline; only the per-frame poll can drive it.
        std::thread::sleep(Duration::from_millis(90));
        arena.poll_deadlines();
        assert!(
            *started.lock(),
            "a held-still pointer past the long-press deadline must fire on the deadline poll"
        );
    }
}
