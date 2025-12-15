//! Base container - state and geometry for render objects.
//!
//! This module provides:
//! - [`BaseContainer`] - Generic trait for base container delegation
//! - [`Base`] - Concrete base container implementation
//!
//! This is the foundation of the compositional render object hierarchy,
//! corresponding to `RenderObject` in Flutter's class hierarchy.

use ambassador::delegatable_trait;
use std::fmt::Debug;

use crate::lifecycle::RenderObjectState;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};

// ============================================================================
// BaseContainer trait
// ============================================================================

/// Generic trait for base containers that store state and geometry.
///
/// This trait enables delegation of lifecycle state and geometry storage
/// to inner containers in the compositional hierarchy.
///
/// # Type Parameters
///
/// - `G` - The geometry type (e.g., `Size` for Box, `SliverGeometry` for Sliver)
///
/// # Flutter Equivalence
///
/// This corresponds to the state management portions of `RenderObject`:
/// - Lifecycle state (attached, needs_layout, needs_paint, etc.)
/// - Cached geometry (size for RenderBox)
#[delegatable_trait]
pub trait BaseContainer<G> {
    /// Returns a reference to the render object state.
    fn state(&self) -> &RenderObjectState;

    /// Returns a mutable reference to the render object state.
    fn state_mut(&mut self) -> &mut RenderObjectState;

    /// Returns a reference to the cached geometry.
    fn geometry(&self) -> &G;

    /// Sets the cached geometry.
    fn set_geometry(&mut self, geometry: G);
}

// ============================================================================
// Base struct
// ============================================================================

/// Base container that stores lifecycle state and geometry.
///
/// This is the foundation of the compositional render object hierarchy.
/// All render objects contain a `Base<P>` (directly or through composition)
/// to manage their lifecycle state and cached geometry.
///
/// # Type Parameters
///
/// - `P` - The protocol (BoxProtocol or SliverProtocol)
///
/// # Flutter Equivalence
///
/// This corresponds to the state management portions of `RenderObject`:
/// - `_needsLayout`, `_needsPaint`, `_needsCompositingBitsUpdate`
/// - `_depth`, `_owner`, `_parent`
/// - `size` (for RenderBox) / `geometry` (for RenderSliver)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::containers::{Base, BoxProtocol};
///
/// // Create a base container for box protocol
/// let mut base: Base<BoxProtocol> = Base::new();
///
/// // Access state
/// assert!(base.state().needs_layout());
///
/// // Set geometry after layout
/// base.set_geometry(Size::new(100.0, 50.0));
/// ```
pub struct Base<P: Protocol> {
    /// Lifecycle state (attach/detach, dirty flags, depth, etc.)
    state: RenderObjectState,

    /// Cached geometry from last layout.
    /// - For BoxProtocol: `Size`
    /// - For SliverProtocol: `SliverGeometry`
    geometry: P::Geometry,
}

impl<P: Protocol> Debug for Base<P>
where
    P::Geometry: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Base")
            .field("state", &self.state)
            .field("geometry", &self.geometry)
            .finish()
    }
}

impl<P: Protocol> Default for Base<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Base<P> {
    /// Creates a new base container with default state and geometry.
    pub fn new() -> Self {
        Self {
            state: RenderObjectState::new(),
            geometry: P::default_geometry(),
        }
    }

    /// Creates a new base container with a specific node ID.
    pub fn with_node_id(node_id: usize) -> Self {
        Self {
            state: RenderObjectState::with_node_id(node_id),
            geometry: P::default_geometry(),
        }
    }

    /// Returns a reference to the render object state.
    #[inline]
    pub fn state(&self) -> &RenderObjectState {
        &self.state
    }

    /// Returns a mutable reference to the render object state.
    #[inline]
    pub fn state_mut(&mut self) -> &mut RenderObjectState {
        &mut self.state
    }

    /// Returns a reference to the cached geometry.
    #[inline]
    pub fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }

    /// Sets the cached geometry.
    #[inline]
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }
}

// ============================================================================
// BaseContainer trait implementation
// ============================================================================

impl<P: Protocol> BaseContainer<P::Geometry> for Base<P> {
    #[inline]
    fn state(&self) -> &RenderObjectState {
        &self.state
    }

    #[inline]
    fn state_mut(&mut self) -> &mut RenderObjectState {
        &mut self.state
    }

    #[inline]
    fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }

    #[inline]
    fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }
}

// ============================================================================
// Type aliases
// ============================================================================

/// Base container for box protocol (geometry is `Size`).
pub type BaseBox = Base<BoxProtocol>;

/// Base container for sliver protocol (geometry is `SliverGeometry`).
pub type BaseSliver = Base<SliverProtocol>;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

    #[test]
    fn test_base_box_new() {
        let base: BaseBox = Base::new();
        assert_eq!(base.geometry(), &Size::ZERO);
        // needs_layout is false initially until attached
        assert!(!base.state().is_attached());
    }

    #[test]
    fn test_base_box_set_geometry() {
        let mut base: BaseBox = Base::new();
        base.set_geometry(Size::new(100.0, 50.0));
        assert_eq!(base.geometry(), &Size::new(100.0, 50.0));
    }

    #[test]
    fn test_base_with_node_id() {
        let base: BaseBox = Base::with_node_id(42);
        assert_eq!(base.state().node_id(), 42);
    }

    #[test]
    fn test_base_container_trait() {
        let mut base: BaseBox = Base::new();

        // Test through trait
        fn use_base_container<G: Default + PartialEq + Debug>(
            container: &mut impl BaseContainer<G>,
            geometry: G,
        ) -> &G {
            container.set_geometry(geometry);
            container.geometry()
        }

        let geom = use_base_container(&mut base, Size::new(200.0, 100.0));
        assert_eq!(geom, &Size::new(200.0, 100.0));
        assert_eq!(base.geometry(), &Size::new(200.0, 100.0));
    }

    #[test]
    fn test_base_sliver_new() {
        let base: BaseSliver = Base::new();
        assert_eq!(base.geometry().scroll_extent, 0.0);
    }
}
