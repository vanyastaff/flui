//! Tap gesture recognizer
//!
//! Recognizes tap gestures (pointer down + up within slop tolerance).
//!
//! A tap is defined as:
//! - Pointer down
//! - Pointer stays within TAP_SLOP (18px) of initial position
//! - Pointer up within timeout

use super::recognizer::{constants, GestureRecognizer, GestureRecognizerState};
use crate::arena::{GestureArenaMember, PointerId};
use flui_types::{events::PointerEvent, Offset};
use parking_lot::Mutex;
use std::sync::Arc;

/// Callback for tap events
pub type TapCallback = Arc<dyn Fn(TapDetails) + Send + Sync>;

/// Details about a tap gesture
#[derive(Debug, Clone)]
pub struct TapDetails {
    /// Global position where tap occurred
    pub global_position: Offset,
    /// Local position (relative to widget)
    pub local_position: Offset,
    /// Pointer device kind
    pub kind: flui_types::events::PointerDeviceKind,
}

/// Recognizes tap gestures
///
/// A tap is a quick press-and-release within a small movement tolerance.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
///
/// let arena = GestureArena::new();
/// let recognizer = TapGestureRecognizer::new(arena)
///     .with_on_tap(|details| {
///         println!("Tapped at {:?}", details.global_position);
///     });
///
/// // Add to arena and handle events
/// recognizer.add_pointer(pointer_id, position);
/// recognizer.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct TapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: GestureRecognizerState,

    /// Callbacks
    callbacks: Arc<Mutex<TapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<TapState>>,
}

#[derive(Default)]
struct TapCallbacks {
    on_tap_down: Option<TapCallback>,
    on_tap_up: Option<TapCallback>,
    on_tap: Option<TapCallback>,
    on_tap_cancel: Option<TapCallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TapState {
    Ready,
    Down,
    Cancelled,
}

impl TapGestureRecognizer {
    /// Create a new tap recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: GestureRecognizerState::new(arena),
            callbacks: Arc::new(Mutex::new(TapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(TapState::Ready)),
        })
    }

    /// Set the tap down callback
    pub fn with_on_tap_down(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the tap up callback
    pub fn with_on_tap_up(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_up = Some(Arc::new(callback));
        self
    }

    /// Set the tap callback (called on successful tap)
    pub fn with_on_tap(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap = Some(Arc::new(callback));
        self
    }

    /// Set the tap cancel callback
    pub fn with_on_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle tap down event
    fn handle_tap_down(&self, position: Offset, kind: flui_types::events::PointerDeviceKind) {
        *self.gesture_state.lock() = TapState::Down;

        // Call on_tap_down callback
        if let Some(callback) = self.callbacks.lock().on_tap_down.clone() {
            let details = TapDetails {
                global_position: position,
                local_position: position,
                kind,
            };
            callback(details);
        }
    }

    /// Handle tap up event
    fn handle_tap_up(&self, position: Offset, kind: flui_types::events::PointerDeviceKind) {
        let current_state = *self.gesture_state.lock();

        if current_state == TapState::Down {
            // Successful tap!
            *self.gesture_state.lock() = TapState::Ready;

            let details = TapDetails {
                global_position: position,
                local_position: position,
                kind,
            };

            // Call on_tap_up callback
            if let Some(callback) = self.callbacks.lock().on_tap_up.clone() {
                callback(details.clone());
            }

            // Call on_tap callback
            if let Some(callback) = self.callbacks.lock().on_tap.clone() {
                callback(details);
            }

            // We won! Accept in arena
            // Note: Arena resolution happens via GestureArenaMember trait
            self.state.stop_tracking();
        }
    }

    /// Handle tap cancel event
    fn handle_tap_cancel(&self, position: Offset, kind: flui_types::events::PointerDeviceKind) {
        let current_state = *self.gesture_state.lock();

        if current_state == TapState::Down {
            *self.gesture_state.lock() = TapState::Cancelled;

            // Call on_tap_cancel callback
            if let Some(callback) = self.callbacks.lock().on_tap_cancel.clone() {
                let details = TapDetails {
                    global_position: position,
                    local_position: position,
                    kind,
                };
                callback(details);
            }

            // Reject in arena
            self.state.reject();
        }
    }

    /// Check if pointer moved too far (beyond slop tolerance)
    fn check_slop(&self, current_position: Offset) -> bool {
        if let Some(initial_pos) = self.state.initial_position() {
            let delta = current_position - initial_pos;
            let distance = delta.distance();

            if distance > constants::TAP_SLOP as f32 {
                return true; // Moved too far
            }
        }
        false
    }
}

impl GestureRecognizer for TapGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset) {
        // Start tracking this pointer
        // Create Arc from self for arena tracking
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);

        // Handle tap down
        self.handle_tap_down(position, flui_types::events::PointerDeviceKind::Touch);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // Only process if we're tracking a pointer
        if self.state.primary_pointer().is_none() {
            return;
        }

        match event {
            PointerEvent::Move(data) => {
                // Check if moved too far (slop detection)
                if self.check_slop(data.position) {
                    self.handle_tap_cancel(data.position, data.device_kind);
                }
            }
            PointerEvent::Up(data) => {
                self.handle_tap_up(data.position, data.device_kind);
            }
            PointerEvent::Cancel(data) => {
                self.handle_tap_cancel(data.position, data.device_kind);
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        self.callbacks.lock().on_tap_down = None;
        self.callbacks.lock().on_tap_up = None;
        self.callbacks.lock().on_tap = None;
        self.callbacks.lock().on_tap_cancel = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl GestureArenaMember for TapGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // We won the arena - gesture is accepted
        // Callbacks already called in handle_tap_up
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // We lost the arena - cancel the gesture
        if let Some(pos) = self.state.initial_position() {
            self.handle_tap_cancel(pos, flui_types::events::PointerDeviceKind::Touch);
        }
    }
}

impl std::fmt::Debug for TapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &self.gesture_state.lock())
            .field("has_on_tap", &self.callbacks.lock().on_tap.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;

    #[test]
    fn test_tap_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = TapGestureRecognizer::new(arena);

        assert_eq!(recognizer.primary_pointer(), None);
    }

    #[test]
    fn test_tap_recognizer_with_callback() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena).with_on_tap(move |_details| {
            *tapped_clone.lock() = true;
        });

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 100.0);

        // Simulate tap: down -> up
        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&PointerEvent::Up(
            flui_types::events::PointerEventData::new(
                position,
                flui_types::events::PointerDeviceKind::Touch,
            ),
        ));

        // Should have called callback
        assert!(*tapped.lock());
    }

    #[test]
    fn test_tap_recognizer_slop_detection() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let cancelled = Arc::new(Mutex::new(false));

        let tapped_clone = tapped.clone();
        let cancelled_clone = cancelled.clone();

        let recognizer = TapGestureRecognizer::new(arena)
            .with_on_tap(move |_details| {
                *tapped_clone.lock() = true;
            })
            .with_on_tap_cancel(move |_details| {
                *cancelled_clone.lock() = true;
            });

        let pointer = PointerId::new(1);
        let start_pos = Offset::new(100.0, 100.0);

        // Start tap
        recognizer.add_pointer(pointer, start_pos);

        // Move too far (beyond TAP_SLOP = 18px)
        let moved_pos = Offset::new(100.0, 130.0); // 30px away
        recognizer.handle_event(&PointerEvent::Move(
            flui_types::events::PointerEventData::new(
                moved_pos,
                flui_types::events::PointerDeviceKind::Touch,
            ),
        ));

        // Should have cancelled
        assert!(*cancelled.lock());
        assert!(!*tapped.lock());
    }

    #[test]
    fn test_tap_within_slop() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena).with_on_tap(move |_details| {
            *tapped_clone.lock() = true;
        });

        let pointer = PointerId::new(1);
        let start_pos = Offset::new(100.0, 100.0);

        // Start tap
        recognizer.add_pointer(pointer, start_pos);

        // Move slightly (within TAP_SLOP = 18px)
        let moved_pos = Offset::new(105.0, 105.0); // ~7px away
        recognizer.handle_event(&PointerEvent::Move(
            flui_types::events::PointerEventData::new(
                moved_pos,
                flui_types::events::PointerDeviceKind::Touch,
            ),
        ));

        // Tap up
        recognizer.handle_event(&PointerEvent::Up(
            flui_types::events::PointerEventData::new(
                moved_pos,
                flui_types::events::PointerDeviceKind::Touch,
            ),
        ));

        // Should have succeeded (within slop)
        assert!(*tapped.lock());
    }
}
