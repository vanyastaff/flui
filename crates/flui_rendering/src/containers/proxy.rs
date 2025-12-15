//! Proxy container - single child where size equals child's size.
//!
//! This module provides:
//! - [`SingleChildContainer`] - Generic trait for single-child containers
//! - [`ProxyContainer`] - Generic trait for proxy containers with geometry
//! - [`Proxy`] - Concrete proxy container implementation
//!
//! This is the Rust equivalent of Flutter's `RenderProxyBox` pattern.
//! Use when parent's geometry should match child's geometry (pass-through).

use ambassador::{delegatable_trait, Delegate};
use flui_tree::arity::Optional;
use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::Children;
use crate::constraints::SliverGeometry;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};

// ============================================================================
// SingleChildContainer trait - Generic single-child container
// ============================================================================

/// Generic trait for containers that hold a single optional child.
///
/// This trait is parameterized by the boxed child type `T`, enabling a single
/// implementation to work with any protocol (Box, Sliver, etc.).
///
/// # Type Parameter
///
/// - `T` - The boxed child type (e.g., `Box<dyn RenderBox>`, `Box<dyn RenderSliver>`)
///
/// # Why `T: Sized`?
///
/// We use `T` as the boxed type (not `T: ?Sized`) to enable Ambassador delegation.
/// This means `T = Box<dyn RenderBox>` rather than `T = dyn RenderBox`.
///
/// # Example
///
/// ```rust,ignore
/// // Works for Box protocol
/// impl SingleChildContainer<Box<dyn RenderBox>> for ProxyBox { ... }
///
/// // Works for Sliver protocol
/// impl SingleChildContainer<Box<dyn RenderSliver>> for SliverProxy { ... }
/// ```
#[delegatable_trait]
pub trait SingleChildContainer<T> {
    /// Returns a reference to the child, if present.
    fn child(&self) -> Option<&T>;

    /// Returns a mutable reference to the child, if present.
    fn child_mut(&mut self) -> Option<&mut T>;

    /// Sets the child, returning the previous child if any.
    fn set_child(&mut self, child: T) -> Option<T>;

    /// Takes the child out of the container.
    fn take_child(&mut self) -> Option<T>;

    /// Returns `true` if the container has a child.
    fn has_child(&self) -> bool {
        self.child().is_some()
    }
}

// ============================================================================
// ProxyContainer trait - Generic proxy container (size = child size)
// ============================================================================

/// Generic trait for proxy containers that store geometry.
///
/// A proxy container passes through child's geometry unchanged.
/// This is the base for effects like opacity, color filter, etc.
///
/// # Type Parameters
///
/// - `T` - The boxed child type (e.g., `Box<dyn RenderBox>`)
/// - `G` - The geometry type (e.g., `Size` for Box, `SliverGeometry` for Sliver)
///
/// # Example
///
/// ```rust,ignore
/// // Box proxy: child is Box<dyn RenderBox>, geometry is Size
/// impl ProxyContainer<Box<dyn RenderBox>, Size> for ProxyBox { ... }
///
/// // Sliver proxy: child is Box<dyn RenderSliver>, geometry is SliverGeometry
/// impl ProxyContainer<Box<dyn RenderSliver>, SliverGeometry> for SliverProxy { ... }
/// ```
#[delegatable_trait]
pub trait ProxyContainer<T, G>: SingleChildContainer<T> {
    /// Returns a reference to the cached geometry.
    fn geometry(&self) -> &G;

    /// Sets the cached geometry.
    fn set_geometry(&mut self, geometry: G);
}

// ============================================================================
// Children implementation of SingleChildContainer
// ============================================================================

// ============================================================================
// Proxy struct with Ambassador delegation
// ============================================================================

/// Container that stores a single child where parent size equals child size.
///
/// This is the storage pattern for render objects that:
/// - Apply visual effects (opacity, color filters)
/// - Apply transformations (scale, rotation)
/// - Simply wrap a child without changing size
///
/// # Flutter Equivalent
///
/// This corresponds to `RenderProxyBox` in Flutter, which uses:
/// - `RenderObjectWithChildMixin<RenderBox>` for child storage
/// - Size passthrough from child to parent
///
/// # Ambassador Delegation
///
/// `Proxy<P>` uses `#[derive(Delegate)]` to automatically delegate
/// `SingleChildContainer` methods to the inner `Children` storage.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderOpacity {
///     proxy: ProxyBox,  // child + Size storage
///     opacity: f32,
/// }
///
/// impl RenderBox for RenderOpacity {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         let size = if let Some(child) = self.proxy.child_mut() {
///             child.perform_layout(constraints)
///         } else {
///             constraints.smallest()
///         };
///         self.proxy.set_geometry(size);
///         size
///     }
///
///     fn size(&self) -> Size {
///         *self.proxy.geometry()
///     }
/// }
/// ```
#[derive(Delegate)]
#[delegate(SingleChildContainer<Box<P::Object>>, target = "child")]
pub struct Proxy<P: Protocol> {
    child: Children<P, Optional>,
    geometry: P::Geometry,
}

impl<P: Protocol> Debug for Proxy<P>
where
    P::Object: Debug,
    P::Geometry: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy")
            .field("has_child", &self.child.has_child())
            .field("geometry", &self.geometry)
            .finish()
    }
}

impl<P: Protocol> Default for Proxy<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Proxy<P> {
    /// Creates a new empty proxy container with default geometry.
    pub fn new() -> Self {
        Self {
            child: Children::new(),
            geometry: P::default_geometry(),
        }
    }

    /// Creates a proxy container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        Self {
            child: Children::with_child(child),
            geometry: P::default_geometry(),
        }
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
// ProxyContainer implementation for Proxy<P>
// ============================================================================

impl<P: Protocol> ProxyContainer<Box<P::Object>, P::Geometry> for Proxy<P> {
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

/// Box proxy container (geometry is `Size`).
///
/// Use for render objects that pass through child's size unchanged.
///
/// # Flutter Equivalent
///
/// `RenderProxyBox` and subclasses like:
/// - `RenderOpacity`
/// - `RenderColorFilter`
/// - `RenderAnimatedOpacity`
/// - `RenderBackdropFilter`
pub type ProxyBox = Proxy<BoxProtocol>;

/// Sliver proxy container (geometry is `SliverGeometry`).
///
/// Use for sliver render objects that pass through child's geometry.
pub type SliverProxy = Proxy<SliverProtocol>;

// ============================================================================
// Convenience methods for ProxyBox
// ============================================================================

impl ProxyBox {
    /// Returns the cached size.
    pub fn size(&self) -> Size {
        self.geometry
    }

    /// Returns a reference to the child as `&dyn RenderBox`.
    ///
    /// This is a convenience method that dereferences the Box.
    pub fn child_ref(&self) -> Option<&dyn RenderBox> {
        self.child().map(|boxed| &**boxed)
    }

    /// Returns a mutable reference to the child as `&mut dyn RenderBox`.
    ///
    /// This is a convenience method that dereferences the Box.
    pub fn child_mut_ref(&mut self) -> Option<&mut dyn RenderBox> {
        self.child_mut().map(|boxed| &mut **boxed)
    }
}

impl SliverProxy {
    /// Returns the cached sliver geometry.
    pub fn sliver_geometry(&self) -> &SliverGeometry {
        &self.geometry
    }
}

// ============================================================================
// Paint and Hit Testing Helpers for ProxyBox
// ============================================================================

impl ProxyBox {
    /// Paints the child at the same offset (pass-through).
    ///
    /// For proxy boxes, the child is always painted at the same position
    /// since the proxy's size equals the child's size.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBox.paint`.
    pub fn paint_child<F>(&self, offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child_ref() {
            paint_fn(child, offset);
        }
    }

    /// Paints the child with a custom offset.
    pub fn paint_child_at<F>(&self, base_offset: Offset, child_offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child_ref() {
            paint_fn(child, base_offset + child_offset);
        }
    }

    /// Hit tests the child at zero offset (pass-through).
    ///
    /// For proxy boxes, the child is always at offset (0, 0) since the
    /// proxy's size equals the child's size.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBox.hitTestChildren`.
    pub fn hit_test_child(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child_ref() {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Hit tests the child with a custom offset.
    pub fn hit_test_child_at(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
        child_offset: Offset,
    ) -> bool {
        if let Some(child) = self.child_ref() {
            result.add_with_paint_offset(Some(child_offset), position, |result, transformed| {
                child.hit_test(result, transformed)
            })
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::RenderSliver;

    #[test]
    fn test_proxy_box_default() {
        let proxy: ProxyBox = Proxy::new();
        assert!(!proxy.has_child());
        assert_eq!(proxy.size(), Size::ZERO);
    }

    #[test]
    fn test_proxy_box_set_geometry() {
        let mut proxy: ProxyBox = Proxy::new();
        proxy.set_geometry(Size::new(100.0, 200.0));
        assert_eq!(proxy.size(), Size::new(100.0, 200.0));
    }

    // ========================================================================
    // Generic trait tests - verify traits work with any Protocol
    // ========================================================================

    /// Helper function that works with any SingleChildContainer
    fn use_single_child<T, C: SingleChildContainer<T>>(container: &C) -> bool {
        container.has_child()
    }

    /// Helper function that works with any ProxyContainer
    fn get_geometry<T, G: Clone, C: ProxyContainer<T, G>>(container: &C) -> G {
        container.geometry().clone()
    }

    #[test]
    fn test_single_child_container_box_protocol() {
        let proxy: Proxy<BoxProtocol> = Proxy::new();
        // Verify generic function works with BoxProtocol
        assert!(!use_single_child::<Box<dyn RenderBox>, _>(&proxy));
    }

    #[test]
    fn test_single_child_container_sliver_protocol() {
        let proxy: Proxy<SliverProtocol> = Proxy::new();
        // Verify generic function works with SliverProtocol
        assert!(!use_single_child::<Box<dyn RenderSliver>, _>(&proxy));
    }

    #[test]
    fn test_proxy_container_box_protocol() {
        let mut proxy: Proxy<BoxProtocol> = Proxy::new();
        proxy.set_geometry(Size::new(100.0, 50.0));

        // Verify ProxyContainer trait works with BoxProtocol
        let size: Size = get_geometry::<Box<dyn RenderBox>, Size, _>(&proxy);
        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_proxy_container_sliver_protocol() {
        let mut proxy: Proxy<SliverProtocol> = Proxy::new();
        let geom = SliverGeometry::default();
        proxy.set_geometry(geom.clone());

        // Verify ProxyContainer trait works with SliverProtocol
        let retrieved: SliverGeometry =
            get_geometry::<Box<dyn RenderSliver>, SliverGeometry, _>(&proxy);
        assert_eq!(retrieved.scroll_extent, geom.scroll_extent);
    }

    // ========================================================================
    // Type alias tests
    // ========================================================================

    #[test]
    fn test_proxy_box_alias() {
        let _: ProxyBox = Proxy::new();
    }

    #[test]
    fn test_sliver_proxy_alias() {
        let _: SliverProxy = Proxy::new();
    }

    // ========================================================================
    // Ambassador delegation test
    // ========================================================================

    #[test]
    fn test_delegation_works() {
        let mut proxy: ProxyBox = Proxy::new();

        // These methods are delegated to Children via Ambassador
        assert!(!proxy.has_child());
        assert!(proxy.child().is_none());
        assert!(proxy.take_child().is_none());
    }
}
