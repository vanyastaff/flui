//! View protocol system.
//!
//! Defines protocols for different view types with compile-time guarantees.

use std::marker::PhantomData;

use crate::foundation::Listenable;
use crate::render::arity::Arity;
use crate::view::ViewState;

// ============================================================================
// PROTOCOL TRAIT
// ============================================================================

/// View protocol - defines state type and behavior.
pub trait ViewProtocol: sealed::Sealed + 'static {
    /// Associated state type.
    type State: ViewState;

    /// Runtime mode identifier.
    const MODE: ViewMode;

    /// Whether state needs to be cloned on updates.
    const CLONE_STATE: bool;
}

/// Sealed trait to prevent external implementations.
mod sealed {
    pub trait Sealed {}

    impl Sealed for super::Stateless {}
    impl<S: super::ViewState> Sealed for super::Stateful<S> {}
    impl<L: super::Listenable + 'static> Sealed for super::Animated<L> {}
    impl<T: Send + 'static> Sealed for super::Provider<T> {}
    impl Sealed for super::Proxy {}
    impl<A: super::Arity> Sealed for super::RenderBox<A> {}
    impl<A: super::Arity> Sealed for super::RenderSliver<A> {}
}

// ============================================================================
// RUNTIME MODE
// ============================================================================

/// Runtime view mode identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

// ============================================================================
// CONCRETE PROTOCOLS
// ============================================================================

/// Stateless protocol - simple views without state.
pub struct Stateless;

impl ViewProtocol for Stateless {
    type State = ();
    const MODE: ViewMode = ViewMode::Stateless;
    const CLONE_STATE: bool = false;
}

/// Stateful protocol - views with persistent mutable state.
pub struct Stateful<S: ViewState>(PhantomData<S>);

impl<S: ViewState> ViewProtocol for Stateful<S> {
    type State = S;
    const MODE: ViewMode = ViewMode::Stateful;
    const CLONE_STATE: bool = true;
}

/// Animated protocol - views that subscribe to animations.
pub struct Animated<L: Listenable>(PhantomData<L>);

impl<L: Listenable + 'static> ViewProtocol for Animated<L> {
    type State = ();
    const MODE: ViewMode = ViewMode::Animated;
    const CLONE_STATE: bool = true;
}

/// Provider protocol - views that provide data to descendants.
pub struct Provider<T: Send + 'static>(PhantomData<T>);

impl<T: Send + 'static> ViewProtocol for Provider<T> {
    type State = ();
    const MODE: ViewMode = ViewMode::Provider;
    const CLONE_STATE: bool = true;
}

/// Proxy protocol - views that wrap single child.
pub struct Proxy;

impl ViewProtocol for Proxy {
    type State = ();
    const MODE: ViewMode = ViewMode::Proxy;
    const CLONE_STATE: bool = true;
}

/// Render Box protocol - views that create box render objects.
pub struct RenderBox<A: Arity>(PhantomData<A>);

impl<A: Arity> ViewProtocol for RenderBox<A> {
    type State = ();
    const MODE: ViewMode = ViewMode::RenderBox;
    const CLONE_STATE: bool = true;
}

/// Render Sliver protocol - views that create sliver render objects.
pub struct RenderSliver<A: Arity>(PhantomData<A>);

impl<A: Arity> ViewProtocol for RenderSliver<A> {
    type State = ();
    const MODE: ViewMode = ViewMode::RenderSliver;
    const CLONE_STATE: bool = true;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_modes() {
        assert_eq!(Stateless::MODE, ViewMode::Stateless);
        assert_eq!(Stateful::<()>::MODE, ViewMode::Stateful);
        assert_eq!(Proxy::MODE, ViewMode::Proxy);
    }

    #[test]
    fn test_clone_requirements() {
        assert!(!Stateless::CLONE_STATE);
        assert!(Stateful::<()>::CLONE_STATE);
        assert!(Animated::<()>::CLONE_STATE);
        assert!(Provider::<i32>::CLONE_STATE);
        assert!(Proxy::CLONE_STATE);
    }
}
