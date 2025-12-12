//! Core traits with advanced type system features
//!
//! This module provides the foundational traits for the interaction system,
//! using Rust's advanced type features:
//!
//! - **GATs**: Generic Associated Types for flexible callbacks
//! - **Extension traits**: Add methods to foreign types
//! - **Marker traits**: Compile-time constraints

use crate::ids::PointerId;
use crate::routing::HitTestEntry;
use flui_types::events::PointerEvent;
use flui_types::geometry::Offset;

// ============================================================================
// HitTestTarget trait
// ============================================================================

/// Trait for types that can be hit test targets.
///
/// Any render object that can receive pointer events should implement this trait.
/// This follows Flutter's `HitTestTarget` interface exactly.
///
/// # Flutter Equivalence
/// ```dart
/// abstract interface class HitTestTarget {
///   void handleEvent(PointerEvent event, HitTestEntry<HitTestTarget> entry);
/// }
/// ```
pub trait HitTestTarget: Send + Sync {
    /// Handles a pointer event dispatched to this target.
    ///
    /// Called when a pointer event should be delivered to this target.
    /// The `entry` contains the hit test result including local position
    /// and transform information.
    ///
    /// # Arguments
    /// * `event` - The pointer event to handle
    /// * `entry` - The hit test entry containing position and transform info
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry);
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
// HitTestTarget implementations for wrapper types
// ============================================================================

impl<T: HitTestTarget + ?Sized> HitTestTarget for Box<T> {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        (**self).handle_event(event, entry)
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
