//! Proxy container - single child where size equals child's size.
//!
//! This is the Rust equivalent of Flutter's `RenderProxyBox` pattern.
//! Use when parent's geometry should match child's geometry (pass-through).

use flui_tree::arity::Optional;
use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::{Children, ProxyContainer, SingleChildContainer};
use crate::constraints::SliverGeometry;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};

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
/// # Generic Traits
///
/// `Proxy<P>` implements generic traits that work with any protocol:
///
/// - [`SingleChildContainer<P::Object>`] - child access methods
/// - [`ProxyContainer<P::Object, P::Geometry>`] - geometry storage
///
/// This enables a single implementation for both Box and Sliver protocols:
///
/// ```rust,ignore
/// // Works for ProxyBox (BoxProtocol)
/// fn use_proxy<T: ProxyContainer<dyn RenderBox, Size>>(proxy: &T) {
///     if let Some(child) = proxy.child() {
///         println!("Size: {:?}", proxy.geometry());
///     }
/// }
///
/// // Works for SliverProxy (SliverProtocol) too!
/// fn use_sliver<T: ProxyContainer<dyn RenderSliver, SliverGeometry>>(proxy: &T) {
///     if let Some(child) = proxy.child() {
///         println!("Geometry: {:?}", proxy.geometry());
///     }
/// }
/// ```
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
        let mut container = Children::new();
        container.set(child);
        Self {
            child: container,
            geometry: P::default_geometry(),
        }
    }

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&P::Object> {
        self.child.get()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.get_mut()
    }

    /// Sets the child, replacing any existing child.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this is handled by the `child` setter in
    /// `RenderObjectWithChildMixin`, which calls `dropChild` for the
    /// old child and `adoptChild` for the new child.
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child.set(child);
    }

    /// Takes the child out of the container, leaving it empty.
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
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

// ============================================================================
// Generic trait implementations for Proxy<P>
// ============================================================================

impl<P: Protocol> SingleChildContainer<P::Object> for Proxy<P> {
    #[inline]
    fn child(&self) -> Option<&P::Object> {
        self.child.get()
    }

    #[inline]
    fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.get_mut()
    }

    #[inline]
    fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.child.set(child)
    }

    #[inline]
    fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
    }

    #[inline]
    fn has_child(&self) -> bool {
        self.child.has_child()
    }
}

impl<P: Protocol> ProxyContainer<P::Object, P::Geometry> for Proxy<P> {
    #[inline]
    fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }

    #[inline]
    fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }
}

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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderOpacity {
    ///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
    ///         context.push_opacity(self.opacity);
    ///         self.proxy.paint_child(offset, |child, child_offset| {
    ///             child.paint(context, child_offset);
    ///         });
    ///     }
    /// }
    /// ```
    pub fn paint_child<F>(&self, offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child() {
            paint_fn(child, offset);
        }
    }

    /// Paints the child with a custom offset.
    ///
    /// Use this when the proxy needs to apply an offset for some reason,
    /// such as for scroll effects or animations.
    pub fn paint_child_at<F>(&self, base_offset: Offset, child_offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child() {
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderOpacity {
    ///     fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
    ///         self.proxy.hit_test_child(result, position)
    ///     }
    /// }
    /// ```
    pub fn hit_test_child(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Hit tests the child with a custom offset.
    ///
    /// Use this when the proxy needs to apply an offset for some reason,
    /// such as for scroll effects or animations.
    pub fn hit_test_child_at(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
        child_offset: Offset,
    ) -> bool {
        if let Some(child) = self.child() {
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
