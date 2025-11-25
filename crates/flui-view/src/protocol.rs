//! View protocol system.
//!
//! Defines protocols for different view types with compile-time guarantees.

// ============================================================================
// VIEW MODE
// ============================================================================

/// Runtime view mode identifier.
///
/// Used to identify the type of view at runtime without downcasting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ViewMode {
    /// Stateless view - consumed once, no lifecycle.
    Stateless,

    /// Stateful view - persistent state with full lifecycle.
    Stateful,

    /// Animated view - subscribes to animation changes.
    Animated,

    /// Provider view - provides data to descendants.
    Provider,

    /// Proxy view - wraps single child without layout changes.
    Proxy,

    /// Render view (Box protocol) - creates box render objects.
    RenderBox,

    /// Render view (Sliver protocol) - creates sliver render objects.
    RenderSliver,
}

impl ViewMode {
    /// Returns `true` if this mode has persistent state.
    #[inline]
    pub const fn has_state(self) -> bool {
        matches!(self, Self::Stateful | Self::Animated)
    }

    /// Returns `true` if this mode creates render objects.
    #[inline]
    pub const fn is_render(self) -> bool {
        matches!(self, Self::RenderBox | Self::RenderSliver)
    }

    /// Returns `true` if this mode supports children.
    #[inline]
    pub const fn supports_children(self) -> bool {
        !matches!(self, Self::Stateless)
    }
}

// ============================================================================
// VIEW PROTOCOL TRAIT
// ============================================================================

/// View protocol - defines behavior category at compile time.
///
/// This is a sealed trait that categorizes views into protocols.
/// Each protocol has specific lifecycle and behavior characteristics.
///
/// # Protocols
///
/// - `Stateless` - Simple views without state
/// - `Stateful<S>` - Views with persistent state `S`
/// - `Proxy` - Views that wrap single child
/// - `RenderBox<A>` - Views creating box render objects with arity `A`
/// - `RenderSliver<A>` - Views creating sliver render objects with arity `A`
pub trait ViewProtocol: sealed::Sealed + 'static {
    /// Runtime mode identifier.
    const MODE: ViewMode;
}

/// Sealed trait module to prevent external implementations.
mod sealed {
    use crate::state::ViewState;

    pub trait Sealed {}

    impl Sealed for super::Stateless {}
    impl<S: ViewState> Sealed for super::Stateful<S> {}
    impl Sealed for super::Proxy {}
}

// ============================================================================
// CONCRETE PROTOCOLS
// ============================================================================

/// Stateless protocol - simple views without state.
///
/// Views with this protocol are consumed during build and have no lifecycle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stateless;

impl ViewProtocol for Stateless {
    const MODE: ViewMode = ViewMode::Stateless;
}

/// Stateful protocol - views with persistent mutable state.
///
/// Views with this protocol maintain state across rebuilds.
#[derive(Debug)]
pub struct Stateful<S: crate::state::ViewState>(std::marker::PhantomData<S>);

impl<S: crate::state::ViewState> ViewProtocol for Stateful<S> {
    const MODE: ViewMode = ViewMode::Stateful;
}

/// Proxy protocol - views that wrap single child.
///
/// Views with this protocol pass through to a single child without layout changes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Proxy;

impl ViewProtocol for Proxy {
    const MODE: ViewMode = ViewMode::Proxy;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_mode_has_state() {
        assert!(!ViewMode::Stateless.has_state());
        assert!(ViewMode::Stateful.has_state());
        assert!(ViewMode::Animated.has_state());
        assert!(!ViewMode::Proxy.has_state());
        assert!(!ViewMode::RenderBox.has_state());
    }

    #[test]
    fn test_view_mode_is_render() {
        assert!(!ViewMode::Stateless.is_render());
        assert!(!ViewMode::Stateful.is_render());
        assert!(ViewMode::RenderBox.is_render());
        assert!(ViewMode::RenderSliver.is_render());
    }

    #[test]
    fn test_protocol_mode() {
        assert_eq!(Stateless::MODE, ViewMode::Stateless);
        assert_eq!(Stateful::<()>::MODE, ViewMode::Stateful);
        assert_eq!(Proxy::MODE, ViewMode::Proxy);
    }
}
