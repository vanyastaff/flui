//! Proxy container - single child where size equals child's size.
//!
//! This is the Rust equivalent of Flutter's `RenderProxyBox` pattern.
//! Use when parent's geometry should match child's geometry (pass-through).

use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use flui_types::{Size, SliverGeometry};
use std::fmt::Debug;

use super::Single;

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
pub struct Proxy<P: Protocol> {
    child: Single<P>,
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
            child: Single::new(),
            geometry: P::default_geometry(),
        }
    }

    /// Creates a proxy container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        Self {
            child: Single::with_child(child),
            geometry: P::default_geometry(),
        }
    }

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&P::Object> {
        self.child.child()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.child_mut()
    }

    /// Sets the child, replacing any existing child.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this is handled by the `child` setter in
    /// `RenderObjectWithChildMixin`, which calls `dropChild` for the
    /// old child and `adoptChild` for the new child.
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child.set_child(child);
    }

    /// Takes the child out of the container, leaving it empty.
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take_child()
    }

    /// Returns `true` if the container has a child.
    pub fn has_child(&self) -> bool {
        self.child.has_child()
    }

    /// Returns a reference to the geometry.
    pub fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }

    /// Sets the geometry.
    ///
    /// This should typically be called after layout to cache the
    /// computed size/geometry.
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }
}

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

impl ProxyBox {
    /// Returns the cached size.
    pub fn size(&self) -> Size {
        self.geometry
    }
}

impl SliverProxy {
    /// Returns the cached sliver geometry.
    pub fn sliver_geometry(&self) -> &SliverGeometry {
        &self.geometry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
