//! Pointer event conveniences and drag constraints.

use flui_foundation::geometry::Offset;

use crate::{
    events::{PointerEvent, PointerEventExt as EventsPointerEventExt},
    ids::PointerId,
};

// ============================================================================
// PointerEventExtTrait extension trait (additional methods)
// ============================================================================

/// Extension trait for `PointerEvent` with convenience methods for gesture
/// recognition.
///
/// Adds commonly needed methods without modifying the original type.
pub trait PointerEventExtTrait {
    /// Returns the position of this pointer event.
    fn position(&self) -> Offset<f64>;

    /// Returns the pointer/device ID.
    fn pointer_id(&self) -> PointerId;

    /// Returns `true` if this is a "down" event (pointer contact started).
    fn is_down(&self) -> bool;

    /// Returns `true` if this is an "up" event (pointer contact ended).
    fn is_up(&self) -> bool;

    /// Returns `true` if this is a movement event (hover or move).
    fn is_move(&self) -> bool;

    /// Returns `true` if this event should start gesture tracking.
    fn starts_gesture(&self) -> bool;

    /// Returns `true` if this event should end gesture tracking.
    fn ends_gesture(&self) -> bool;
}

impl PointerEventExtTrait for PointerEvent {
    fn position(&self) -> Offset<f64> {
        // Use the PointerEventExt trait from events module
        EventsPointerEventExt::position(self)
    }

    fn pointer_id(&self) -> PointerId {
        // PointerId is `ui_events::pointer::PointerId(NonZeroU64)`.
        // Delegate to the canonical extractor — zero-cost field load,
        // no per-event hasher allocation.
        crate::events::extract_pointer_id(self)
    }

    fn is_down(&self) -> bool {
        matches!(self, PointerEvent::Down(_))
    }

    fn is_up(&self) -> bool {
        matches!(self, PointerEvent::Up(_))
    }

    fn is_move(&self) -> bool {
        matches!(self, PointerEvent::Move(_))
    }

    fn starts_gesture(&self) -> bool {
        self.is_down()
    }

    fn ends_gesture(&self) -> bool {
        matches!(self, PointerEvent::Up(_) | PointerEvent::Cancel(_))
    }
}

/// Drag axis constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DragAxis {
    /// Vertical drag only (up/down).
    Vertical,
    /// Horizontal drag only (left/right).
    Horizontal,
    /// Free drag (any direction).
    #[default]
    Free,
}
