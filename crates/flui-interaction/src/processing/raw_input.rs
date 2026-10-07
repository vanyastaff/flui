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

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use web_time::Instant;

use flui_foundation::geometry::Offset;

use crate::{
    events::{PointerEvent, PointerType},
    ids::PointerId,
};

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
        position: Offset<f64>,
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
        position: Offset<f64>,
        /// Delta from previous position.
        delta: Offset<f64>,
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
        position: Offset<f64>,
        /// Delta from previous position.
        delta: Offset<f64>,
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
        position: Offset<f64>,
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
        position: Offset<f64>,
        /// Delta from previous position.
        delta: Offset<f64>,
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
    pub fn position(&self) -> Offset<f64> {
        match self {
            Self::Down { position, .. }
            | Self::Move { position, .. }
            | Self::Up { position, .. }
            | Self::Cancel { position, .. }
            | Self::Hover { position, .. } => *position,
        }
    }

    /// Get the delta (zero for Down events).
    pub fn delta(&self) -> Offset<f64> {
        match self {
            Self::Down { .. } | Self::Cancel { .. } => Offset::new(0.0, 0.0),
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
pub type RawInputCallback = Rc<dyn Fn(RawPointerEvent)>;

// ============================================================================
// PointerTrackingState
// ============================================================================

/// Tracking state for a single pointer.
#[derive(Debug, Clone)]
struct PointerTrackingState {
    /// Last known position.
    last_position: Offset<f64>,
    /// Is pointer currently down?
    is_down: bool,
    /// Device kind (stored for potential future use).
    #[expect(dead_code)]
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
/// # Thread affinity
///
/// `RawInputHandler` is owner-local under ADR-0027. It stores executable raw
/// input callbacks as `Rc` and should be driven by the owning UI runtime.
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
    tracking: Rc<RefCell<HashMap<PointerId, PointerTrackingState>>>,
    /// Callback for raw events.
    callback: Rc<RefCell<Option<RawInputCallback>>>,
    /// Whether raw mode is enabled.
    enabled: Rc<Cell<bool>>,
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
            tracking: Rc::new(RefCell::new(HashMap::new())),
            callback: Rc::new(RefCell::new(None)),
            enabled: Rc::new(Cell::new(true)),
        }
    }

    /// Set the callback for raw input events.
    pub fn set_callback(&self, callback: impl Fn(RawPointerEvent) + 'static) {
        let _prev = self.callback.borrow_mut().replace(Rc::new(callback));
    }

    /// Clear the callback.
    pub fn clear_callback(&self) {
        let _prev = self.callback.borrow_mut().take();
    }

    /// Enable or disable raw input handling.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
    }

    /// Check if raw input is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Handle a pointer event, converting to raw event and invoking callback.
    ///
    /// Returns the generated `RawPointerEvent` if one was created. The
    /// callback runs with no borrow held, so it may call
    /// [`Self::set_callback`] or [`Self::clear_callback`]; the change applies
    /// from the next event.
    pub fn handle_event(&self, event: &PointerEvent) -> Option<RawPointerEvent> {
        if !self.enabled.get() {
            return None;
        }

        let raw_event = self.convert_event(event);

        // Clone the callback out and release the borrow before calling it:
        // the callback may replace or clear itself through this handler.
        let callback = self.callback.borrow().clone();
        if let (Some(raw), Some(callback)) = (&raw_event, callback) {
            callback(raw.clone());
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
                let position = Offset::new(pos.x, pos.y);
                let device_kind = data.pointer.pointer_type;

                // Start tracking
                self.tracking.borrow_mut().insert(
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
                let position = Offset::new(pos.x, pos.y);
                let device_kind = data.pointer.pointer_type;

                let delta = {
                    let mut tracking = self.tracking.borrow_mut();
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
                let position = Offset::new(pos.x, pos.y);
                let device_kind = data.pointer.pointer_type;

                let delta = {
                    let mut tracking = self.tracking.borrow_mut();
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
                    let mut tracking = self.tracking.borrow_mut();
                    tracking
                        .remove(&pointer)
                        .map_or(Offset::ZERO, |s| s.last_position)
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
        self.tracking.borrow().len()
    }

    /// Get the number of pointers currently down.
    pub fn active_pointer_count(&self) -> usize {
        self.tracking
            .borrow()
            .values()
            .filter(|s| s.is_down)
            .count()
    }

    /// Check if a specific pointer is currently down.
    pub fn is_pointer_down(&self, pointer: PointerId) -> bool {
        self.tracking
            .borrow()
            .get(&pointer)
            .is_some_and(|s| s.is_down)
    }

    /// Get the last known position of a pointer.
    pub fn pointer_position(&self, pointer: PointerId) -> Option<Offset<f64>> {
        self.tracking
            .borrow()
            .get(&pointer)
            .map(|s| s.last_position)
    }

    /// Clear all tracking state.
    pub fn reset(&self) {
        self.tracking.borrow_mut().clear();
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
