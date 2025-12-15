//! SingleChild container - base + single child storage.
//!
//! This module provides:
//! - [`SingleChild`] - Container with base state and single optional child
//!
//! This corresponds to `RenderObjectWithChildMixin<RenderBox>` in Flutter,
//! providing the foundation for single-child render objects.

use ambassador::Delegate;
use std::fmt::Debug;

use flui_tree::arity::{ChildrenStorage, Optional};

use super::base::{ambassador_impl_BaseContainer, Base, BaseContainer};
use super::children::Children;
use super::SingleChildContainer;
use crate::lifecycle::RenderObjectState;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};

// ============================================================================
// SingleChild struct
// ============================================================================

/// Container that stores base state and a single optional child.
///
/// This is the second level of the compositional hierarchy:
/// - `Base<P>` → state + geometry
/// - `SingleChild<P>` → base + child
///
/// # Type Parameters
///
/// - `P` - The protocol (BoxProtocol or SliverProtocol)
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderObjectWithChildMixin<RenderBox>` which adds:
/// - `child` getter/setter
/// - Child lifecycle management (attach/detach)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::containers::{SingleChild, BoxProtocol};
///
/// pub struct RenderOpacity {
///     inner: SingleChild<BoxProtocol>,
///     opacity: f32,
/// }
///
/// impl RenderOpacity {
///     fn child(&self) -> Option<&dyn RenderBox> {
///         self.inner.child().map(|b| &**b as &dyn RenderBox)
///     }
/// }
/// ```
#[derive(Delegate)]
#[delegate(BaseContainer<P::Geometry>, target = "base")]
pub struct SingleChild<P: Protocol> {
    /// Base state and geometry.
    base: Base<P>,

    /// Single optional child.
    child: Children<P, Optional>,
}

impl<P: Protocol> Debug for SingleChild<P>
where
    P::Object: Debug,
    P::Geometry: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleChild")
            .field("base", &self.base)
            .field("has_child", &self.child.has_child())
            .finish()
    }
}

impl<P: Protocol> Default for SingleChild<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> SingleChild<P> {
    /// Creates a new empty single child container.
    pub fn new() -> Self {
        Self {
            base: Base::new(),
            child: Children::new(),
        }
    }

    /// Creates a single child container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        let mut container = Children::new();
        container.set(child);
        Self {
            base: Base::new(),
            child: container,
        }
    }

    /// Returns a reference to the base container.
    #[inline]
    pub fn base(&self) -> &Base<P> {
        &self.base
    }

    /// Returns a mutable reference to the base container.
    #[inline]
    pub fn base_mut(&mut self) -> &mut Base<P> {
        &mut self.base
    }

    /// Returns a reference to the child, if present.
    #[inline]
    pub fn child(&self) -> Option<&P::Object> {
        self.child.get()
    }

    /// Returns a mutable reference to the child, if present.
    #[inline]
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.get_mut()
    }

    /// Returns a reference to the boxed child, if present.
    #[inline]
    pub fn child_box(&self) -> Option<&Box<P::Object>> {
        self.child.single_child()
    }

    /// Returns a mutable reference to the boxed child, if present.
    #[inline]
    pub fn child_box_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.child.single_child_mut()
    }

    /// Sets the child, replacing any existing child.
    #[inline]
    pub fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.child.set(child)
    }

    /// Takes the child out of the container, leaving it empty.
    #[inline]
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
    }

    /// Returns `true` if the container has a child.
    #[inline]
    pub fn has_child(&self) -> bool {
        self.child.has_child()
    }
}

// ============================================================================
// SingleChildContainer trait implementation
// ============================================================================

impl<P: Protocol> SingleChildContainer<Box<P::Object>> for SingleChild<P> {
    #[inline]
    fn child(&self) -> Option<&Box<P::Object>> {
        self.child.single_child()
    }

    #[inline]
    fn child_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.child.single_child_mut()
    }

    #[inline]
    fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.child.set(child)
    }

    #[inline]
    fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
    }
}

// ============================================================================
// Type aliases
// ============================================================================

/// Single child container for box protocol.
pub type SingleChildBox = SingleChild<BoxProtocol>;

/// Single child container for sliver protocol.
pub type SingleChildSliver = SingleChild<SliverProtocol>;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

    #[test]
    fn test_single_child_box_new() {
        let container: SingleChildBox = SingleChild::new();
        assert!(!container.has_child());
        assert_eq!(container.base().geometry(), &Size::ZERO);
    }

    #[test]
    fn test_single_child_base_delegation() {
        let mut container: SingleChildBox = SingleChild::new();

        // Access through BaseContainer trait (delegated)
        container.set_geometry(Size::new(100.0, 50.0));
        assert_eq!(container.geometry(), &Size::new(100.0, 50.0));

        // State should be accessible (not attached initially)
        assert!(!container.state().is_attached());
    }

    #[test]
    fn test_single_child_container_trait() {
        let container: SingleChildBox = SingleChild::new();

        // Test through SingleChildContainer trait
        fn use_single_child<T>(container: &impl SingleChildContainer<T>) -> bool {
            container.has_child()
        }

        assert!(!use_single_child(&container));
    }

    #[test]
    fn test_single_child_sliver() {
        let container: SingleChildSliver = SingleChild::new();
        assert!(!container.has_child());
        assert_eq!(container.base().geometry().scroll_extent, 0.0);
    }
}
