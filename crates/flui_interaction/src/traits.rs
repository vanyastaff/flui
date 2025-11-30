//! Core traits with advanced type system features
//!
//! This module provides the foundational traits for the interaction system,
//! using Rust's advanced type features:
//!
//! - **Sealed traits**: External crates can use but not implement
//! - **GATs**: Generic Associated Types for flexible callbacks
//! - **Extension traits**: Add methods to foreign types
//! - **Marker traits**: Compile-time constraints

use crate::ids::PointerId;
use flui_types::events::PointerEvent;
use flui_types::geometry::Offset;

// ============================================================================
// HitTestTarget trait with sealed pattern
// ============================================================================

/// Marker trait for types that can be hit test targets.
///
/// This is a sealed trait - only types in this crate can implement it.
/// External code can work with `dyn HitTestTarget` but cannot add new targets.
pub trait HitTestTarget: sealed::HitTestTargetSealed + Send + Sync {
    /// Handles a pointer event dispatched to this target.
    ///
    /// Returns `true` if the event was handled and should not propagate further.
    fn handle_event(&self, event: &PointerEvent) -> bool;

    /// Returns the element ID of this target, if it has one.
    fn element_id(&self) -> Option<flui_foundation::ElementId> {
        None
    }
}

// ============================================================================
// GestureCallback trait with GAT
// ============================================================================

/// A callback that can be invoked with gesture details.
///
/// Uses GAT to allow different detail types per callback kind.
///
/// # Example
///
/// ```rust,ignore
/// struct TapCallback<F>(F);
///
/// impl<F: Fn(TapDetails)> GestureCallback for TapCallback<F> {
///     type Details<'a> = TapDetails;
///
///     fn invoke(&self, details: Self::Details<'_>) {
///         (self.0)(details);
///     }
/// }
/// ```
pub trait GestureCallback: Send + Sync {
    /// The type of details passed to this callback.
    ///
    /// Using GAT allows callbacks to borrow data from the gesture recognizer
    /// without requiring clones.
    type Details<'a>
    where
        Self: 'a;

    /// Invokes the callback with the given details.
    fn invoke(&self, details: Self::Details<'_>);
}

/// A boxed gesture callback for dynamic dispatch.
pub type BoxedCallback<D> = Box<dyn Fn(D) + Send + Sync>;

// ============================================================================
// PointerEventExt extension trait
// ============================================================================

/// Extension trait for `PointerEvent` with convenience methods.
///
/// Adds commonly needed methods without modifying the original type.
pub trait PointerEventExt {
    /// Returns the position of this pointer event.
    fn position(&self) -> Offset;

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

impl PointerEventExt for PointerEvent {
    fn position(&self) -> Offset {
        // Delegate to the inherent method
        PointerEvent::position(self)
    }

    fn pointer_id(&self) -> PointerId {
        PointerId::new(self.device())
    }

    fn is_down(&self) -> bool {
        matches!(self, PointerEvent::Down(_))
    }

    fn is_up(&self) -> bool {
        matches!(self, PointerEvent::Up(_))
    }

    fn is_move(&self) -> bool {
        matches!(self, PointerEvent::Move(_) | PointerEvent::Hover(_))
    }

    fn starts_gesture(&self) -> bool {
        self.is_down()
    }

    fn ends_gesture(&self) -> bool {
        matches!(self, PointerEvent::Up(_) | PointerEvent::Cancel(_))
    }
}

// ============================================================================
// GestureRecognizerExt extension trait
// ============================================================================

/// Extension trait for gesture recognizers with utility methods.
pub trait GestureRecognizerExt {
    /// Checks if the gesture has exceeded the slop threshold.
    ///
    /// # Arguments
    ///
    /// * `initial` - Initial pointer position
    /// * `current` - Current pointer position
    /// * `slop` - Maximum allowed movement (typically 18px)
    fn exceeds_slop(initial: Offset, current: Offset, slop: f32) -> bool {
        let delta = current - initial;
        delta.distance() > slop
    }

    /// Calculates the primary delta for a given drag axis.
    fn primary_delta(delta: Offset, axis: DragAxis) -> f32 {
        match axis {
            DragAxis::Vertical => delta.dy,
            DragAxis::Horizontal => delta.dx,
            DragAxis::Free => delta.distance(),
        }
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

// ============================================================================
// Disposable trait
// ============================================================================

/// Trait for resources that can be disposed/cleaned up.
///
/// Similar to `Drop` but for explicit cleanup before destruction.
pub trait Disposable {
    /// Disposes of this resource, releasing any held callbacks or state.
    ///
    /// After calling this method, the object should be considered unusable.
    /// Subsequent method calls may panic or return default values.
    fn dispose(&mut self);

    /// Returns `true` if this resource has been disposed.
    fn is_disposed(&self) -> bool;
}

// ============================================================================
// Sealed trait implementations
// ============================================================================

mod sealed {
    /// Sealed trait for `HitTestTarget`.
    pub trait HitTestTargetSealed {}

    // Implement for standard library types if needed
    // impl HitTestTargetSealed for MyTarget {}
}

// Allow implementing HitTestTarget for Box<dyn HitTestTarget>
impl<T: HitTestTarget + ?Sized> sealed::HitTestTargetSealed for Box<T> {}
impl<T: HitTestTarget + ?Sized> HitTestTarget for Box<T> {
    fn handle_event(&self, event: &PointerEvent) -> bool {
        (**self).handle_event(event)
    }

    fn element_id(&self) -> Option<flui_foundation::ElementId> {
        (**self).element_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::events::{PointerDeviceKind, PointerEventData};

    #[test]
    fn test_pointer_event_ext() {
        let pos = Offset::new(100.0, 200.0);
        let data = PointerEventData::new(pos, PointerDeviceKind::Mouse);

        let down = PointerEvent::Down(data.clone());
        assert!(down.is_down());
        assert!(!down.is_up());
        assert!(down.starts_gesture());
        assert_eq!(down.position(), pos);

        let up = PointerEvent::Up(data.clone());
        assert!(up.is_up());
        assert!(!up.is_down());
        assert!(up.ends_gesture());

        let mv = PointerEvent::Move(data);
        assert!(mv.is_move());
        assert!(!mv.is_down());
        assert!(!mv.is_up());
    }

    #[test]
    fn test_exceeds_slop() {
        struct Helper;
        impl GestureRecognizerExt for Helper {}

        let initial = Offset::new(100.0, 100.0);

        // Within slop (18px)
        let within = Offset::new(110.0, 105.0); // ~11px
        assert!(!Helper::exceeds_slop(initial, within, 18.0));

        // Beyond slop
        let beyond = Offset::new(100.0, 125.0); // 25px
        assert!(Helper::exceeds_slop(initial, beyond, 18.0));
    }

    #[test]
    fn test_primary_delta() {
        struct Helper;
        impl GestureRecognizerExt for Helper {}

        let delta = Offset::new(10.0, 20.0);

        assert_eq!(Helper::primary_delta(delta, DragAxis::Horizontal), 10.0);
        assert_eq!(Helper::primary_delta(delta, DragAxis::Vertical), 20.0);

        // Free axis returns distance
        let dist = Helper::primary_delta(delta, DragAxis::Free);
        assert!((dist - delta.distance()).abs() < 0.001);
    }

    #[test]
    fn test_drag_axis_default() {
        assert_eq!(DragAxis::default(), DragAxis::Free);
    }
}
