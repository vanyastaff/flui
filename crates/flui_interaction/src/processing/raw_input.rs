//! Raw input mode for direct pointer access
//!
//! This module provides a way to receive pointer events directly without
//! going through the gesture recognition system. Useful for:
//!
//! - Games that need direct control over input
//! - Custom gesture implementations
//! - Low-latency input handling
//! - Drawing applications
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::raw_input::{RawInputHandler, RawPointerEvent};
//!
//! let mut handler = RawInputHandler::new();
//!
//! // Set callback for raw events
//! handler.set_callback(|event| {
//!     match event {
//!         RawPointerEvent::Down { position, .. } => {
//!             start_drawing(position);
//!         }
//!         RawPointerEvent::Move { position, delta, .. } => {
//!             continue_drawing(position, delta);
//!         }
//!         RawPointerEvent::Up { position, .. } => {
//!             finish_drawing(position);
//!         }
//!         _ => {}
//!     }
//! });
//!
//! // Process events
//! handler.handle_event(&pointer_event);
//! ```

use crate::events::{PointerEvent, PointerType};
use flui_types::geometry::PixelDelta;
use flui_types::geometry::{px, Pixels};

use crate::ids::PointerId;
use flui_types::geometry::Offset;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

// ============================================================================
// RawPointerEvent
// ============================================================================

/// Raw pointer event with additional computed information.
///
/// Unlike the standard `PointerEvent`, this includes:
/// - Delta from previous position
/// - Pointer tracking state
/// - High-resolution timestamp
#[derive(Debug, Clone)]
pub enum RawPointerEvent {
    /// Pointer pressed down.
    Down {
        /// Pointer identifier.
        pointer: PointerId,
        /// Position in logical pixels.
        position: Offset<Pixels>,
        /// Device type.
        device_kind: PointerType,
        /// Event timestamp.
        timestamp: Instant,
    },

    /// Pointer moved.
    Move {
        /// Pointer identifier.
        pointer: PointerId,
        /// Current position.
        position: Offset<Pixels>,
        /// Delta from previous position.
        delta: Offset<PixelDelta>,
        /// Device type.
        device_kind: PointerType,
        /// Event timestamp.
        timestamp: Instant,
    },

    /// Pointer released.
    Up {
        /// Pointer identifier.
        pointer: PointerId,
        /// Final position.
        position: Offset<Pixels>,
        /// Delta from previous position.
        delta: Offset<PixelDelta>,
        /// Device type.
        device_kind: PointerType,
        /// Event timestamp.
        timestamp: Instant,
    },

    /// Pointer cancelled (e.g., palm rejection).
    Cancel {
        /// Pointer identifier.
        pointer: PointerId,
        /// Last known position.
        position: Offset<Pixels>,
        /// Device type.
        device_kind: PointerType,
        /// Event timestamp.
        timestamp: Instant,
    },

    /// Pointer hovering (no contact).
    Hover {
        /// Pointer identifier.
        pointer: PointerId,
        /// Current position.
        position: Offset<Pixels>,
        /// Delta from previous position.
        delta: Offset<PixelDelta>,
        /// Device type.
        device_kind: PointerType,
        /// Event timestamp.
        timestamp: Instant,
    },
}

impl RawPointerEvent {
    /// Get the pointer ID.
    pub fn pointer(&self) -> PointerId {
        match self {
            Self::Down { pointer, .. }
            | Self::Move { pointer, .. }
            | Self::Up { pointer, .. }
            | Self::Cancel { pointer, .. }
            | Self::Hover { pointer, .. } => *pointer,
        }
    }

    /// Get the position.
    pub fn position(&self) -> Offset<Pixels> {
        match self {
            Self::Down { position, .. }
            | Self::Move { position, .. }
            | Self::Up { position, .. }
            | Self::Cancel { position, .. }
            | Self::Hover { position, .. } => *position,
        }
    }

    /// Get the delta (zero for Down events).
    pub fn delta(&self) -> Offset<PixelDelta> {
        match self {
            Self::Down { .. } | Self::Cancel { .. } => {
                Offset::new(PixelDelta::ZERO, PixelDelta::ZERO)
            }
            Self::Move { delta, .. } | Self::Up { delta, .. } | Self::Hover { delta, .. } => *delta,
        }
    }

    /// Get the timestamp.
    pub fn timestamp(&self) -> Instant {
        match self {
            Self::Down { timestamp, .. }
            | Self::Move { timestamp, .. }
            | Self::Up { timestamp, .. }
            | Self::Cancel { timestamp, .. }
            | Self::Hover { timestamp, .. } => *timestamp,
        }
    }

    /// Returns true if this is a Down event.
    pub fn is_down(&self) -> bool {
        matches!(self, Self::Down { .. })
    }

    /// Returns true if this is a Move event.
    pub fn is_move(&self) -> bool {
        matches!(self, Self::Move { .. })
    }

    /// Returns true if this is an Up event.
    pub fn is_up(&self) -> bool {
        matches!(self, Self::Up { .. })
    }

    /// Returns true if this is a Cancel event.
    pub fn is_cancel(&self) -> bool {
        matches!(self, Self::Cancel { .. })
    }
}

// ============================================================================
// RawInputCallback
// ============================================================================

/// Callback type for raw input events.
pub type RawInputCallback = Arc<dyn Fn(RawPointerEvent) + Send + Sync>;

// ============================================================================
// PointerTrackingState
// ============================================================================

/// Tracking state for a single pointer.
#[derive(Debug, Clone)]
struct PointerTrackingState {
    /// Last known position.
    last_position: Offset<Pixels>,
    /// Is pointer currently down?
    is_down: bool,
    /// Device kind (stored for potential future use).
    #[allow(dead_code)]
    device_kind: PointerType,
}

// ============================================================================
// RawInputHandler
// ============================================================================

/// Handler for raw input events.
///
/// Converts standard `PointerEvent` to `RawPointerEvent` with additional
/// computed information (deltas, timestamps) and invokes callbacks.
///
/// # Thread Safety
///
/// `RawInputHandler` is thread-safe and can be shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// let handler = RawInputHandler::new();
///
/// handler.set_callback(|event| {
///     println!("Raw event: {:?}", event);
/// });
///
/// // In your event loop:
/// handler.handle_event(&pointer_event);
/// ```
#[derive(Clone)]
pub struct RawInputHandler {
    /// Tracking state for each pointer.
    tracking: Arc<Mutex<HashMap<PointerId, PointerTrackingState>>>,
    /// Callback for raw events.
    callback: Arc<Mutex<Option<RawInputCallback>>>,
    /// Whether raw mode is enabled.
    enabled: Arc<Mutex<bool>>,
}

impl Default for RawInputHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RawInputHandler {
    /// Create a new raw input handler.
    pub fn new() -> Self {
        Self {
            tracking: Arc::new(Mutex::new(HashMap::new())),
            callback: Arc::new(Mutex::new(None)),
            enabled: Arc::new(Mutex::new(true)),
        }
    }

    /// Set the callback for raw input events.
    pub fn set_callback(&self, callback: impl Fn(RawPointerEvent) + Send + Sync + 'static) {
        *self.callback.lock() = Some(Arc::new(callback));
    }

    /// Clear the callback.
    pub fn clear_callback(&self) {
        *self.callback.lock() = None;
    }

    /// Enable or disable raw input handling.
    pub fn set_enabled(&self, enabled: bool) {
        *self.enabled.lock() = enabled;
    }

    /// Check if raw input is enabled.
    pub fn is_enabled(&self) -> bool {
        *self.enabled.lock()
    }

    /// Handle a pointer event, converting to raw event and invoking callback.
    ///
    /// Returns the generated `RawPointerEvent` if one was created.
    pub fn handle_event(&self, event: &PointerEvent) -> Option<RawPointerEvent> {
        // Single lock acquisition for enabled check (avoids separate is_enabled() call)
        if !*self.enabled.lock() {
            return None;
        }

        let raw_event = self.convert_event(event);

        if let Some(ref raw) = raw_event {
            if let Some(callback) = self.callback.lock().clone() {
                callback(raw.clone());
            }
        }

        raw_event
    }

    /// Extract pointer ID from event (use 0 for primary pointer).
    #[inline]
    fn get_pointer_id(event: &PointerEvent) -> PointerId {
        crate::events::extract_pointer_id(event)
    }

    /// Convert a PointerEvent to RawPointerEvent.
    fn convert_event(&self, event: &PointerEvent) -> Option<RawPointerEvent> {
        let timestamp = Instant::now();
        let pointer = Self::get_pointer_id(event);

        match event {
            PointerEvent::Down(data) => {
                let pos = data.state.position;
                let position = Offset::new(px(pos.x as f32), px(pos.y as f32));
                let device_kind = data.pointer.pointer_type;

                // Start tracking
                self.tracking.lock().insert(
                    pointer,
                    PointerTrackingState {
                        last_position: position,
                        is_down: true,
                        device_kind,
                    },
                );

                Some(RawPointerEvent::Down {
                    pointer,
                    position,
                    device_kind,
                    timestamp,
                })
            }

            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(px(pos.x as f32), px(pos.y as f32));
                let device_kind = data.pointer.pointer_type;

                let delta = {
                    let mut tracking = self.tracking.lock();
                    if let Some(state) = tracking.get_mut(&pointer) {
                        let delta = position - state.last_position;
                        state.last_position = position;
                        delta
                    } else {
                        // Not tracking this pointer, start now
                        tracking.insert(
                            pointer,
                            PointerTrackingState {
                                last_position: position,
                                is_down: false,
                                device_kind,
                            },
                        );
                        Offset::ZERO
                    }
                };

                Some(RawPointerEvent::Move {
                    pointer,
                    position,
                    delta: delta.to_delta(),
                    device_kind,
                    timestamp,
                })
            }

            PointerEvent::Up(data) => {
                let pos = data.state.position;
                let position = Offset::new(px(pos.x as f32), px(pos.y as f32));
                let device_kind = data.pointer.pointer_type;

                let delta = {
                    let mut tracking = self.tracking.lock();
                    if let Some(state) = tracking.remove(&pointer) {
                        position - state.last_position
                    } else {
                        Offset::ZERO
                    }
                };

                Some(RawPointerEvent::Up {
                    pointer,
                    position,
                    delta: delta.to_delta(),
                    device_kind,
                    timestamp,
                })
            }

            PointerEvent::Cancel(info) => {
                let device_kind = info.pointer_type;

                // Single lock: get last position and remove in one acquisition
                let position = {
                    let mut tracking = self.tracking.lock();
                    tracking
                        .remove(&pointer)
                        .map(|s| s.last_position)
                        .unwrap_or(Offset::ZERO)
                };

                Some(RawPointerEvent::Cancel {
                    pointer,
                    position,
                    device_kind,
                    timestamp,
                })
            }

            // Events we don't convert to raw (Enter, Leave, Scroll, Gesture)
            _ => None,
        }
    }

    /// Get the number of pointers currently being tracked.
    pub fn tracked_pointer_count(&self) -> usize {
        self.tracking.lock().len()
    }

    /// Get the number of pointers currently down.
    pub fn active_pointer_count(&self) -> usize {
        self.tracking.lock().values().filter(|s| s.is_down).count()
    }

    /// Check if a specific pointer is currently down.
    pub fn is_pointer_down(&self, pointer: PointerId) -> bool {
        self.tracking
            .lock()
            .get(&pointer)
            .is_some_and(|s| s.is_down)
    }

    /// Get the last known position of a pointer.
    pub fn pointer_position(&self, pointer: PointerId) -> Option<Offset<Pixels>> {
        self.tracking.lock().get(&pointer).map(|s| s.last_position)
    }

    /// Clear all tracking state.
    pub fn reset(&self) {
        self.tracking.lock().clear();
    }
}

impl std::fmt::Debug for RawInputHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawInputHandler")
            .field("tracked_pointers", &self.tracked_pointer_count())
            .field("active_pointers", &self.active_pointer_count())
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

// ============================================================================
// InputMode enum
// ============================================================================

/// Input processing mode.
///
/// Determines how pointer events are processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Standard gesture recognition (default).
    ///
    /// Events go through hit testing and gesture arena.
    #[default]
    Gesture,

    /// Raw input mode.
    ///
    /// Events are delivered directly without gesture processing.
    /// Use this for games or custom gesture implementations.
    Raw,

    /// Both modes simultaneously.
    ///
    /// Events are delivered to both gesture system and raw handlers.
    Both,
}

impl InputMode {
    /// Returns true if gesture recognition is active.
    pub fn has_gestures(self) -> bool {
        matches!(self, Self::Gesture | Self::Both)
    }

    /// Returns true if raw input is active.
    pub fn has_raw(self) -> bool {
        matches!(self, Self::Raw | Self::Both)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{make_down_event, make_move_event, make_up_event};

    #[test]
    fn test_raw_handler_creation() {
        let handler = RawInputHandler::new();
        assert!(handler.is_enabled());
        assert_eq!(handler.tracked_pointer_count(), 0);
    }

    #[test]
    fn test_raw_handler_down_event() {
        let handler = RawInputHandler::new();

        let event = make_down_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        let raw = handler.handle_event(&event).unwrap();

        assert!(raw.is_down());
        assert_eq!(raw.position(), Offset::new(Pixels(100.0), Pixels(100.0)));
        assert_eq!(handler.active_pointer_count(), 1);
    }

    #[test]
    fn test_raw_handler_move_delta() {
        let handler = RawInputHandler::new();

        // Down at (100, 100)
        let down = make_down_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        handler.handle_event(&down);

        // Move to (120, 110) - delta should be (20, 10)
        let mv = make_move_event(
            Offset::new(Pixels(120.0), Pixels(110.0)),
            PointerType::Touch,
        );
        let raw = handler.handle_event(&mv).unwrap();

        assert!(raw.is_move());
        assert_eq!(raw.position(), Offset::new(Pixels(120.0), Pixels(110.0)));
        assert_eq!(raw.delta(), Offset::new(PixelDelta(20.0), PixelDelta(10.0)));
    }

    #[test]
    fn test_raw_handler_up_clears_tracking() {
        let handler = RawInputHandler::new();

        let down = make_down_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        handler.handle_event(&down);
        assert_eq!(handler.active_pointer_count(), 1);

        let up = make_up_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        handler.handle_event(&up);
        assert_eq!(handler.tracked_pointer_count(), 0);
    }

    #[test]
    fn test_raw_handler_callback() {
        let handler = RawInputHandler::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        handler.set_callback(move |_event| {
            *called_clone.lock() = true;
        });

        let down = make_down_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        handler.handle_event(&down);

        assert!(*called.lock());
    }

    #[test]
    fn test_raw_handler_disabled() {
        let handler = RawInputHandler::new();
        handler.set_enabled(false);

        let down = make_down_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        let result = handler.handle_event(&down);

        assert!(result.is_none());
        assert_eq!(handler.tracked_pointer_count(), 0);
    }

    #[test]
    fn test_input_mode() {
        assert!(InputMode::Gesture.has_gestures());
        assert!(!InputMode::Gesture.has_raw());

        assert!(!InputMode::Raw.has_gestures());
        assert!(InputMode::Raw.has_raw());

        assert!(InputMode::Both.has_gestures());
        assert!(InputMode::Both.has_raw());
    }

    #[test]
    fn test_raw_event_helpers() {
        let handler = RawInputHandler::new();

        let down = make_down_event(Offset::new(Pixels(50.0), Pixels(50.0)), PointerType::Touch);
        let raw_down = handler.handle_event(&down).unwrap();
        assert!(raw_down.is_down());
        assert!(!raw_down.is_move());
        assert!(!raw_down.is_up());
        assert_eq!(
            raw_down.delta(),
            Offset::new(PixelDelta::ZERO, PixelDelta::ZERO)
        ); // Down has no delta

        let mv = make_move_event(Offset::new(Pixels(60.0), Pixels(60.0)), PointerType::Touch);
        let raw_mv = handler.handle_event(&mv).unwrap();
        assert!(raw_mv.is_move());

        let up = make_up_event(Offset::new(Pixels(70.0), Pixels(70.0)), PointerType::Touch);
        let raw_up = handler.handle_event(&up).unwrap();
        assert!(raw_up.is_up());
    }

    #[test]
    fn test_pointer_position_query() {
        let handler = RawInputHandler::new();

        assert!(handler.pointer_position(PointerId::new(0)).is_none());

        let down = make_down_event(
            Offset::new(Pixels(100.0), Pixels(200.0)),
            PointerType::Touch,
        );
        handler.handle_event(&down);

        let pos = handler.pointer_position(PointerId::new(0)).unwrap();
        assert_eq!(pos, Offset::new(Pixels(100.0), Pixels(200.0)));
    }

    #[test]
    fn test_reset() {
        let handler = RawInputHandler::new();

        let down = make_down_event(
            Offset::new(Pixels(100.0), Pixels(100.0)),
            PointerType::Touch,
        );
        handler.handle_event(&down);
        assert_eq!(handler.tracked_pointer_count(), 1);

        handler.reset();
        assert_eq!(handler.tracked_pointer_count(), 0);
    }
}
