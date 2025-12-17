//! Pointer state tracking and event coalescing
//!
//! Manages cursor/touch position tracking and coalesces high-frequency
//! pointer move events for better performance.

use flui_interaction::events::{
    make_pointer_event, Event, PointerEventData, PointerEventKind, PointerType,
};
use flui_types::Offset;

/// Pointer state tracker
///
/// Tracks cursor/touch position and coalesces pointer move events.
/// High-frequency mouse events are combined into a single event per frame.
///
/// # Event Coalescing
///
/// Desktop platforms can generate 100+ mouse move events per second.
/// This tracker stores only the latest position, processing one
/// coalesced event per frame.
#[derive(Debug, Default)]
pub struct PointerState {
    /// Last known cursor/touch position
    last_position: Offset,

    /// Pending pointer move event (for coalescing)
    pending_move: Option<PointerEventData>,

    /// Current pointer device kind
    device_kind: Option<PointerType>,

    /// Whether pointer is currently down (for drag tracking)
    is_down: bool,
}

impl PointerState {
    /// Create a new pointer state tracker
    pub fn new() -> Self {
        Self {
            last_position: Offset::ZERO,
            pending_move: None,
            device_kind: None,
            is_down: false,
        }
    }

    /// Get the last known position
    pub fn last_position(&self) -> Offset {
        self.last_position
    }

    /// Get the current device kind
    pub fn device_kind(&self) -> Option<PointerType> {
        self.device_kind
    }

    /// Check if pointer is currently down
    pub fn is_down(&self) -> bool {
        self.is_down
    }

    /// Update position and store coalesced event
    ///
    /// This replaces any pending move event with the new position.
    pub fn update_position(&mut self, position: Offset, device: PointerType) {
        self.last_position = position;
        self.device_kind = Some(device);

        // Store coalesced event (replaces previous)
        let data = PointerEventData::new(position, device);
        self.pending_move = Some(data);
    }

    /// Take the pending move event (consuming it)
    ///
    /// Returns the coalesced move event and clears the pending state.
    /// Called at the start of each frame to process input.
    pub fn take_pending_move(&mut self) -> Option<Event> {
        self.pending_move.take().map(|data| {
            // Convert PointerEventData to PointerEvent::Move using helper
            Event::Pointer(make_pointer_event(PointerEventKind::Move, data))
        })
    }

    /// Check if there's a pending move event
    pub fn has_pending_move(&self) -> bool {
        self.pending_move.is_some()
    }

    /// Mark pointer as down
    pub fn set_down(&mut self, down: bool) {
        self.is_down = down;
    }

    /// Create a pointer event data with current state
    pub fn create_event_data(&self, device: PointerType) -> PointerEventData {
        PointerEventData::new(self.last_position, device)
    }

    /// Clear all state
    pub fn clear(&mut self) {
        self.last_position = Offset::ZERO;
        self.pending_move = None;
        self.device_kind = None;
        self.is_down = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_state_new() {
        let state = PointerState::new();
        assert_eq!(state.last_position(), Offset::ZERO);
        assert!(!state.is_down());
        assert!(!state.has_pending_move());
    }

    #[test]
    fn test_update_position() {
        let mut state = PointerState::new();

        state.update_position(Offset::new(100.0, 200.0), PointerType::Mouse);

        assert_eq!(state.last_position(), Offset::new(100.0, 200.0));
        assert_eq!(state.device_kind(), Some(PointerType::Mouse));
        assert!(state.has_pending_move());
    }

    #[test]
    fn test_event_coalescing() {
        let mut state = PointerState::new();

        // Multiple updates
        state.update_position(Offset::new(10.0, 20.0), PointerType::Mouse);
        state.update_position(Offset::new(15.0, 25.0), PointerType::Mouse);
        state.update_position(Offset::new(20.0, 30.0), PointerType::Mouse);

        // Only last position should be in pending event
        assert_eq!(state.last_position(), Offset::new(20.0, 30.0));

        // Take should consume the pending event
        let event = state.take_pending_move();
        assert!(event.is_some());
        assert!(!state.has_pending_move());

        // Second take should return None
        assert!(state.take_pending_move().is_none());
    }

    #[test]
    fn test_pointer_down_state() {
        let mut state = PointerState::new();

        assert!(!state.is_down());

        state.set_down(true);
        assert!(state.is_down());

        state.set_down(false);
        assert!(!state.is_down());
    }
}
