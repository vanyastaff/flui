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
//! Flutter reference: https://api.flutter.dev/flutter/gestures/LongPressGestureRecognizer-class.html

use super::recognizer::{GestureRecognizer, GestureRecognizerState};
use flui_types::geometry::Pixels;

use crate::arena::GestureArenaMember;
use crate::events::{PointerEvent, PointerType};
use crate::ids::PointerId;
use crate::settings::GestureSettings;
use flui_types::Offset;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Callback for long press down events (initial contact)
pub type LongPressDownCallback = Arc<dyn Fn(LongPressDownDetails) + Send + Sync>;

/// Callback for simple long press recognition (no details)
pub type LongPressSimpleCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback for long press start events
pub type LongPressStartCallback = Arc<dyn Fn(LongPressStartDetails) + Send + Sync>;

/// Callback for long press move/up/cancel events
pub type LongPressCallback = Arc<dyn Fn(LongPressDetails) + Send + Sync>;

/// Details about long press down (initial contact)
#[derive(Debug, Clone, PartialEq)]
pub struct LongPressDownDetails {
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
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = LongPressGestureRecognizer::new(arena)
///     .with_on_long_press_start(|details| {
///         println!("Long press started at {:?}", details.global_position);
///     })
///     .with_on_long_press_up(|details| {
///         println!("Long press ended at {:?}", details.global_position);
///     });
///
/// // Add to arena and handle events
/// recognizer.add_pointer(pointer_id, position);
/// recognizer.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct LongPressGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: GestureRecognizerState,

    /// Callbacks
    callbacks: Arc<Mutex<LongPressCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<LongPressState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LongPressPhase {
    /// Ready to start
    Ready,
    /// Pointer down, waiting for timer
    Possible,
    /// Timer elapsed, long press started
    Started,
    /// Cancelled (moved too far or rejected)
    Cancelled,
}

#[derive(Debug, Clone)]
struct LongPressState {
    /// Current phase
    phase: LongPressPhase,
    /// Time when pointer went down
    down_time: Option<Instant>,
    /// Current position
    current_position: Option<Offset<Pixels>>,
    /// Pointer device kind
    device_kind: Option<PointerType>,
}

impl Default for LongPressState {
    fn default() -> Self {
        Self {
            phase: LongPressPhase::Ready,
            down_time: None,
            current_position: None,
            device_kind: None,
        }
    }
}

impl LongPressGestureRecognizer {
    /// Create a new long press recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(LongPressCallbacks::default())),
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
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(LongPressCallbacks::default())),
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
        callback: impl Fn(LongPressDownDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press_down = Some(Arc::new(callback));
        self
    }

    /// Set the simple long press callback (called when gesture is recognized)
    ///
    /// This is a simple callback with no details, called when the long press
    /// duration threshold is reached. For detailed information, use
    /// `with_on_long_press_start` instead.
    pub fn with_on_long_press(
        self: Arc<Self>,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press = Some(Arc::new(callback));
        self
    }

    /// Set the long press start callback (called when timer elapses)
    pub fn with_on_long_press_start(
        self: Arc<Self>,
        callback: impl Fn(LongPressStartDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press_start = Some(Arc::new(callback));
        self
    }

    /// Set the long press move callback (called during long press if pointer moves)
    pub fn with_on_long_press_move_update(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press_move_update = Some(Arc::new(callback));
        self
    }

    /// Set the long press up callback (called when pointer released after long press)
    pub fn with_on_long_press_up(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press_up = Some(Arc::new(callback));
        self
    }

    /// Set the long press end callback (called after up, with details)
    ///
    /// Similar to `on_long_press_up` but called after the up event is processed.
    /// This follows Flutter's pattern of having both onLongPressUp and onLongPressEnd.
    pub fn with_on_long_press_end(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press_end = Some(Arc::new(callback));
        self
    }

    /// Set the long press cancel callback
    pub fn with_on_long_press_cancel(
        self: Arc<Self>,
        callback: impl Fn(LongPressDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_long_press_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle pointer down event
    fn handle_down(&self, position: Offset<Pixels>, kind: PointerType) {
        let mut state = self.gesture_state.lock();
        state.phase = LongPressPhase::Possible;
        state.down_time = Some(Instant::now());
        state.current_position = Some(position);
        state.device_kind = Some(kind);
        drop(state); // Release lock before callback

        // Call on_long_press_down callback (initial contact)
        if let Some(callback) = self.callbacks.lock().on_long_press_down.clone() {
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

                // Check if timer elapsed
                if let Some(down_time) = state.down_time {
                    let elapsed = Instant::now().duration_since(down_time);
                    if elapsed >= settings.long_press_timeout() {
                        // Timer elapsed! Start long press
                        state.phase = LongPressPhase::Started;
                        state.current_position = Some(position);
                        drop(state); // Release lock before calling callback

                        // Call on_long_press (simple callback)
                        if let Some(callback) = self.callbacks.lock().on_long_press.clone() {
                            callback();
                        }

                        // Call on_long_press_start callback
                        if let Some(callback) = self.callbacks.lock().on_long_press_start.clone() {
                            let details = LongPressStartDetails {
                                global_position: position,
                                local_position: position,
                                kind,
                            };
                            callback(details);
                        }
                    }
                }
            }
            LongPressPhase::Started => {
                // Long press already started, update position
                state.current_position = Some(position);
                drop(state); // Release lock before calling callback

                // Call on_long_press_move_update callback
                if let Some(callback) = self.callbacks.lock().on_long_press_move_update.clone() {
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
                // Pointer up before timer elapsed - just cancel silently
                state.phase = LongPressPhase::Ready;
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
                if let Some(callback) = self.callbacks.lock().on_long_press_up.clone() {
                    callback(details.clone());
                }

                // Call on_long_press_end callback
                if let Some(callback) = self.callbacks.lock().on_long_press_end.clone() {
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
            if let Some(callback) = self.callbacks.lock().on_long_press_cancel.clone() {
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

    /// Check if long press timer has elapsed
    /// This should be called periodically by the event loop
    pub fn check_timer(&self) -> bool {
        let mut state = self.gesture_state.lock();

        if state.phase == LongPressPhase::Possible {
            if let Some(down_time) = state.down_time {
                let elapsed = Instant::now().duration_since(down_time);
                if elapsed >= self.long_press_duration() {
                    // Timer elapsed! Start long press
                    state.phase = LongPressPhase::Started;

                    if let (Some(position), Some(kind)) =
                        (state.current_position, state.device_kind)
                    {
                        drop(state); // Release lock before calling callback

                        // Call on_long_press (simple callback)
                        if let Some(callback) = self.callbacks.lock().on_long_press.clone() {
                            callback();
                        }

                        // Call on_long_press_start callback
                        if let Some(callback) = self.callbacks.lock().on_long_press_start.clone() {
                            let details = LongPressStartDetails {
                                global_position: position,
                                local_position: position,
                                kind,
                            };
                            callback(details);
                        }

                        return true;
                    }
                }
            }
        }

        false
    }
}

impl GestureRecognizer for LongPressGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
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
        let mut callbacks = self.callbacks.lock();
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

impl GestureArenaMember for LongPressGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
        // Callbacks will be called when timer elapses or pointer moves/up
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
            .finish()
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
    fn test_long_press_timer() {
        let arena = GestureArena::new();
        let pressed = Arc::new(Mutex::new(false));
        let pressed_clone = pressed.clone();

        let recognizer =
            LongPressGestureRecognizer::new(arena).with_on_long_press_start(move |_details| {
                *pressed_clone.lock() = true;
            });

        let pointer = PointerId::new(1);
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

        let pointer = PointerId::new(1);
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

        let pointer = PointerId::new(1);
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
}
