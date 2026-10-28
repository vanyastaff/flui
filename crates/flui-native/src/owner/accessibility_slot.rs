//! Where a UIKit window's view finds its accessibility bridge (ADR-0255
//! decision 12): the view is the owner layer's, the bridge the
//! accessibility feature's, so the two meet through this slot, which the
//! view and the window's reference share and the feature fills as the window
//! opens. Pure Rust, so its rules run on every host.

use std::cell::OnceCell;
use std::ffi::c_void;
use std::rc::Rc;

/// A point in a view's own coordinates, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// What a window's view asks of its accessibility bridge, on the main
/// thread: UIKit's container methods. `None` leaves the answer to `UIView`.
/// Objects cross as raw pointers, since the bridge's Objective-C binding
/// may not be the view's.
pub(crate) trait ViewAccessibility {
    /// `isAccessibilityElement`.
    fn is_accessibility_element(&self) -> Option<bool>;

    /// `accessibilityElements`, an autoreleased array.
    fn accessibility_elements(&self) -> Option<*mut c_void>;

    /// `accessibilityHitTest:` at `point`, an autoreleased element or null.
    fn hit_test(&self, point: ViewPoint) -> Option<*mut c_void>;

    /// The view moved into a window: an assistive technology already running
    /// gets the tree now.
    fn did_move_to_window(&self);
}

/// The bridge a window's view forwards to: shared by the view and the
/// window's reference, filled once by the accessibility position as the
/// window opens, and empty for a table that names none, in which case the
/// view answers as a plain `UIView`.
#[derive(Clone, Default)]
pub(crate) struct AccessibilitySlot(Rc<OnceCell<Rc<dyn ViewAccessibility>>>);

impl AccessibilitySlot {
    /// The bridge, from now on.
    ///
    /// # Panics
    ///
    /// When the slot is filled already: a window makes its accessibility
    /// part once, and a second bridge would be one UIKit never asks.
    pub(crate) fn fill(&self, bridge: Rc<dyn ViewAccessibility>) {
        assert!(
            self.0.set(bridge).is_ok(),
            "BUG: a window's accessibility slot is filled once, as the window opens"
        );
    }

    /// The bridge, once filled.
    pub(crate) fn get(&self) -> Option<&dyn ViewAccessibility> {
        self.0.get().map(|bridge| &**bridge)
    }
}

impl std::fmt::Debug for AccessibilitySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessibilitySlot")
            .field("filled", &self.0.get().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::rc::Rc;

    use super::{AccessibilitySlot, ViewAccessibility, ViewPoint};

    /// A bridge that answers "an element" and counts the views that moved
    /// into a window.
    #[derive(Default)]
    struct Answering(Cell<usize>);

    impl ViewAccessibility for Answering {
        fn is_accessibility_element(&self) -> Option<bool> {
            Some(true)
        }

        fn accessibility_elements(&self) -> Option<*mut c_void> {
            None
        }

        fn hit_test(&self, _point: ViewPoint) -> Option<*mut c_void> {
            None
        }

        fn did_move_to_window(&self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn a_view_reaches_the_bridge_its_window_filled() {
        let slot = AccessibilitySlot::default();
        let view = slot.clone();
        assert!(view.get().is_none(), "an empty slot leaves UIKit's answer");
        let bridge = Rc::new(Answering::default());
        slot.fill(Rc::clone(&bridge) as _);
        let reached = view.get().expect("the view's copy sees the fill");
        assert_eq!(reached.is_accessibility_element(), Some(true));
        assert!(reached.accessibility_elements().is_none());
        assert!(reached.hit_test(ViewPoint { x: 4.0, y: 8.0 }).is_none());
        reached.did_move_to_window();
        assert_eq!(bridge.0.get(), 1);
    }

    #[test]
    #[should_panic(expected = "filled once")]
    fn a_second_bridge_for_one_window_is_a_bug() {
        let slot = AccessibilitySlot::default();
        slot.fill(Rc::new(Answering::default()));
        slot.fill(Rc::new(Answering::default()));
    }
}
