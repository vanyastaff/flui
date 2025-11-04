//! View trait - Core abstraction for reactive UI
//!
//! The View trait is the primary abstraction for building UI in Flui.
//! It follows Xilem's approach: immutable view trees that efficiently
//! diff and update a mutable element tree.

use super::sealed::Sealed;
use super::build_context::BuildContext;
use crate::element::Element;
use std::any::Any;

/// View trait - immutable description of UI
///
/// Views are lightweight, immutable descriptions of what the UI should look like.
/// They are created cheaply on every frame and compared (diffed) to determine
/// what changed in the element tree.
///
/// # Design Philosophy
///
/// - **Immutable**: Views are created fresh each frame
/// - **Cheap**: Views should be cheap to create and clone
/// - **Pure**: Views don't contain mutable state
/// - **Composable**: Views can contain other views
///
/// # Type Parameters
///
/// - `State`: Persistent state that survives across rebuilds
/// - `Element`: The element type this view creates
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::view::{View, BuildContext};
///
/// #[derive(Clone)]
/// struct Counter {
///     count: i32,
/// }
///
/// impl View for Counter {
///     type State = ();
///     type Element = ComponentElement;
///
///     fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
///         let element = ComponentElement::new(/* ... */);
///         (element, ())
///     }
///
///     fn rebuild(
///         self,
///         prev: &Self,
///         state: &mut Self::State,
///         element: &mut Self::Element,
///     ) -> ChangeFlags {
///         if self.count != prev.count {
///             element.mark_dirty();
///             ChangeFlags::NEEDS_BUILD
///         } else {
///             ChangeFlags::empty()
///         }
///     }
/// }
/// ```
pub trait View: Sealed + Clone + 'static {
    /// Persistent state that survives across rebuilds
    ///
    /// Use `()` if no state is needed.
    type State: 'static;

    /// The element type this view creates
    ///
    /// Typically `ComponentElement`, `RenderElement`, etc.
    type Element: ViewElement;

    /// Build initial element from this view
    ///
    /// Called when this view is first mounted.
    /// Returns both the element and initial state.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Build context for creating child elements
    ///
    /// # Returns
    ///
    /// Tuple of (element, initial_state)
    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State);

    /// Rebuild existing element with new view
    ///
    /// Called when the view tree changes but can be updated in-place.
    /// Should compare `self` with `prev` and update `element` if needed.
    ///
    /// # Parameters
    ///
    /// - `prev`: Previous view (for comparison)
    /// - `state`: Mutable state from previous build
    /// - `element`: Element to update
    ///
    /// # Returns
    ///
    /// ChangeFlags indicating what changed
    ///
    /// # Default Implementation
    ///
    /// By default, always rebuilds. Override for better performance.
    fn rebuild(
        self,
        _prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Default: always mark as needing rebuild
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }

    /// Teardown when view is removed
    ///
    /// Override to perform cleanup when this view is unmounted.
    fn teardown(
        &self,
        _state: &mut Self::State,
        _element: &mut Self::Element,
    ) {
        // Default: no teardown needed
    }
}

/// ViewElement trait - bridge between View and Element
///
/// This trait allows views to work with different element types.
pub trait ViewElement: 'static {
    /// Convert this typed element into the Element enum
    fn into_element(self: Box<Self>) -> Element;

    /// Mark this element as needing rebuild
    fn mark_dirty(&mut self);

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as Any mut for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Change flags indicating what changed during rebuild
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeFlags(u8);

impl ChangeFlags {
    /// No changes
    pub const NONE: Self = Self(0);

    /// View needs rebuild (children changed)
    pub const NEEDS_BUILD: Self = Self(1 << 0);

    /// Layout needs recalculation
    pub const NEEDS_LAYOUT: Self = Self(1 << 1);

    /// Paint needs refresh
    pub const NEEDS_PAINT: Self = Self(1 << 2);

    /// All changes
    pub const ALL: Self = Self(0xFF);

    /// Create empty flags
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Check if any flag is set
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check if specific flag is set
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Union of flags
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for ChangeFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_flags() {
        assert!(ChangeFlags::NONE.is_empty());
        assert!(!ChangeFlags::NEEDS_BUILD.is_empty());

        let flags = ChangeFlags::NEEDS_BUILD | ChangeFlags::NEEDS_LAYOUT;
        assert!(flags.contains(ChangeFlags::NEEDS_BUILD));
        assert!(flags.contains(ChangeFlags::NEEDS_LAYOUT));
        assert!(!flags.contains(ChangeFlags::NEEDS_PAINT));
    }
}
