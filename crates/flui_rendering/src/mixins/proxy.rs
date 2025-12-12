//! Proxy mixin — delegates all to single child
//!
//! This module provides the foundational ProxyBox<T> pattern using:
//! - **Ambassador** for automatic trait delegation
//! - **Deref** for direct field access
//!
//! # Pattern
//!
//! ```rust,ignore
//! // 1. Define your data
//! #[derive(Default, Clone, Debug)]
//! pub struct OpacityData {
//!     pub alpha: f32,
//! }
//!
//! // 2. Type alias
//! pub type RenderOpacity = ProxyBox<OpacityData>;
//!
//! // 3. Override only what differs
//! impl RenderProxyBoxMixin for RenderOpacity {
//!     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!         // self.alpha works via Deref!
//!         ctx.push_opacity(self.alpha, offset, |ctx| {
//!             // self.child() works via Ambassador!
//!             if let Some(c) = self.child().get() {
//!                 c.paint(ctx, Offset::ZERO);
//!             }
//!         });
//!     }
//! }
//! ```

use std::ops::{Deref, DerefMut};

use ambassador::{delegatable_trait, Delegate};
use flui_interaction::HitTestResult;
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::children::Child;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::PaintingContext;

// ============================================================================
// Part 1: Delegatable Traits
// ============================================================================

/// Trait for accessing single child (delegatable)
#[delegatable_trait]
pub trait HasChild<P: Protocol> {
    fn child(&self) -> &Child<P>;
    fn child_mut(&mut self) -> &mut Child<P>;

    /// Check if child exists
    fn has_child(&self) -> bool {
        self.child().is_some()
    }
}

/// Trait for accessing box geometry (delegatable)
#[delegatable_trait]
pub trait HasBoxGeometry {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);
}

/// Trait for accessing sliver geometry (delegatable)
#[delegatable_trait]
pub trait HasSliverGeometry {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);
}

// ============================================================================
// Part 2: Base Struct
// ============================================================================

/// Base for proxy render objects (internal use)
#[derive(Debug)]
pub struct ProxyBase<P: Protocol> {
    pub(crate) child: Child<P>,
    pub(crate) geometry: P::Geometry,
}

impl<P: Protocol> Default for ProxyBase<P>
where
    P::Geometry: Default,
{
    fn default() -> Self {
        Self {
            child: Child::default(),
            geometry: P::Geometry::default(),
        }
    }
}

// Implement delegatable traits for ProxyBase
impl<P: Protocol> HasChild<P> for ProxyBase<P> {
    fn child(&self) -> &Child<P> {
        &self.child
    }

    fn child_mut(&mut self) -> &mut Child<P> {
        &mut self.child
    }
}

// Box specialization
impl HasBoxGeometry for ProxyBase<BoxProtocol> {
    fn size(&self) -> Size {
        self.geometry
    }

    fn set_size(&mut self, size: Size) {
        self.geometry = size;
    }
}

// Sliver specialization
impl HasSliverGeometry for ProxyBase<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
    }
}

// ============================================================================
// Part 3: ProxyData Marker Trait
// ============================================================================

/// Marker trait for data types that can be used with ProxyBox<T>
///
/// Automatically implemented for all types that are Debug + 'static.
pub trait ProxyData: std::fmt::Debug + 'static {}

// Blanket impl
impl<T> ProxyData for T where T: std::fmt::Debug + 'static {}

// ============================================================================
// Part 4: Generic ProxyBox<T> with Ambassador + Deref
// ============================================================================

/// Generic proxy render object with automatic delegation
///
/// # Type Parameters
///
/// - `T`: Custom data type (must implement `ProxyData`)
///
/// # Automatic Features
///
/// - **HasChild** via Ambassador delegation to `base`
/// - **HasBoxGeometry** via Ambassador delegation to `base`
/// - **Deref to T** for direct field access (`self.alpha` instead of `self.data.alpha`)
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Default, Clone, Debug)]
/// pub struct OpacityData {
///     pub alpha: f32,
/// }
///
/// pub type RenderOpacity = ProxyBox<OpacityData>;
///
/// impl RenderOpacity {
///     pub fn new(alpha: f32) -> Self {
///         ProxyBox::new(OpacityData { alpha })
///     }
/// }
/// ```
#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
pub struct ProxyBox<T: ProxyData> {
    base: ProxyBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> ProxyBox<T> {
    /// Create new ProxyBox with data
    pub fn new(data: T) -> Self {
        Self {
            base: ProxyBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access (self.alpha instead of self.data.alpha)
impl<T: ProxyData> Deref for ProxyBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData> DerefMut for ProxyBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 5: RenderProxyBoxMixin - Default Behavior
// ============================================================================

/// Mixin trait for proxy Box render objects with default implementations
///
/// All methods delegate to child by default. Override to customize behavior.
///
/// # Example
///
/// ```rust,ignore
/// impl RenderProxyBoxMixin for RenderOpacity {
///     // Override only paint
///     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///         ctx.push_opacity(self.alpha, offset, |ctx| {
///             if let Some(c) = self.child().get() {
///                 // Custom paint logic
///             }
///         });
///     }
///
///     // All other methods use defaults (delegate to child)
/// }
/// ```
pub trait RenderProxyBoxMixin: HasChild<BoxProtocol> + HasBoxGeometry {
    /// Perform layout (default: delegate to child)
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(_id) = self.child_mut().get() {
            // TODO: call child.layout(constraints) via RenderTree
            // For now, return constraints.smallest()
            let size = constraints.smallest();
            self.set_size(size);
            size
        } else {
            let size = constraints.smallest();
            self.set_size(size);
            size
        }
    }

    /// Paint this render object (default: do nothing if no child)
    fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // TODO: if let Some(id) = self.child().get() {
        //     ctx.paint_child(id, offset);
        // }
    }

    /// Hit test (default: delegate to child)
    fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        // TODO: if let Some(id) = self.child().get() {
        //     return render_tree.hit_test(id, result, position);
        // }
        false
    }

    /// Compute minimum intrinsic width (default: delegate to child)
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0 // TODO: delegate to child
    }

    /// Compute maximum intrinsic width (default: delegate to child)
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0 // TODO: delegate to child
    }

    /// Compute minimum intrinsic height (default: delegate to child)
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0 // TODO: delegate to child
    }

    /// Compute maximum intrinsic height (default: delegate to child)
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0 // TODO: delegate to child
    }

    /// Whether this render object always needs compositing
    fn always_needs_compositing(&self) -> bool {
        false
    }

    /// Whether this render object is a repaint boundary
    fn is_repaint_boundary(&self) -> bool {
        false
    }
}

// Blanket impl: all ProxyBox<T> automatically get RenderProxyBoxMixin
impl<T: ProxyData> RenderProxyBoxMixin for ProxyBox<T> {}

// ============================================================================
// Part 6: ProxySliver<T> with Ambassador + Deref
// ============================================================================

/// Generic proxy sliver render object with automatic delegation
///
/// # Type Parameters
///
/// - `T`: Custom data type (must implement `ProxyData`)
///
/// # Automatic Features
///
/// - **HasChild** via Ambassador delegation to `base`
/// - **HasSliverGeometry** via Ambassador delegation to `base`
/// - **Deref to T** for direct field access
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Default, Clone, Debug)]
/// pub struct SliverOpacityData {
///     pub alpha: f32,
/// }
///
/// pub type RenderSliverOpacity = ProxySliver<SliverOpacityData>;
/// ```
#[derive(Debug, Delegate)]
#[delegate(HasChild<SliverProtocol>, target = "base")]
#[delegate(HasSliverGeometry, target = "base")]
pub struct ProxySliver<T: ProxyData> {
    base: ProxyBase<SliverProtocol>,
    pub data: T,
}

impl<T: ProxyData> ProxySliver<T> {
    /// Create new ProxySliver with data
    pub fn new(data: T) -> Self {
        Self {
            base: ProxyBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access
impl<T: ProxyData> Deref for ProxySliver<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData> DerefMut for ProxySliver<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 7: RenderProxySliverMixin - Default Behavior
// ============================================================================

/// Mixin trait for proxy Sliver render objects with default implementations
///
/// All methods delegate to child by default. Override to customize behavior.
pub trait RenderProxySliverMixin: HasChild<SliverProtocol> + HasSliverGeometry {
    /// Perform layout (default: delegate to child)
    fn perform_layout(&mut self, _constraints: &SliverConstraints) -> SliverGeometry {
        if let Some(_id) = self.child_mut().get() {
            // TODO: call child.layout(constraints) via RenderTree
            // For now, return empty geometry
            let geometry = SliverGeometry::default();
            self.set_geometry(geometry);
            geometry
        } else {
            let geometry = SliverGeometry::default();
            self.set_geometry(geometry);
            geometry
        }
    }

    /// Paint this render object (default: do nothing if no child)
    fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // TODO: if let Some(id) = self.child().get() {
        //     ctx.paint_child(id, offset);
        // }
    }

    /// Hit test (default: delegate to child)
    fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        // TODO: if let Some(id) = self.child().get() {
        //     return render_tree.hit_test(id, result, position);
        // }
        false
    }

    /// Whether this render object always needs compositing
    fn always_needs_compositing(&self) -> bool {
        false
    }

    /// Whether this render object is a repaint boundary
    fn is_repaint_boundary(&self) -> bool {
        false
    }
}

// Blanket impl: all ProxySliver<T> automatically get RenderProxySliverMixin
impl<T: ProxyData> RenderProxySliverMixin for ProxySliver<T> {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone, Debug)]
    struct TestData {
        value: f32,
    }

    #[test]
    fn test_proxy_box_creation() {
        let proxy = ProxyBox::new(TestData { value: 42.0 });
        assert_eq!(proxy.value, 42.0); // Deref works!
    }

    #[test]
    fn test_proxy_box_deref() {
        let mut proxy = ProxyBox::new(TestData { value: 1.0 });

        // Read via Deref
        assert_eq!(proxy.value, 1.0);

        // Write via DerefMut
        proxy.value = 2.0;
        assert_eq!(proxy.value, 2.0);
    }

    #[test]
    fn test_proxy_box_child_access() {
        let proxy = ProxyBox::new(TestData { value: 1.0 });

        // HasChild trait methods work via Ambassador
        assert!(!proxy.has_child());
        assert!(proxy.child().is_none());
    }

    #[test]
    fn test_proxy_box_geometry() {
        let mut proxy = ProxyBox::new(TestData { value: 1.0 });

        // HasBoxGeometry trait methods work via Ambassador
        let size = Size::new(100.0, 50.0);
        proxy.set_size(size);
        assert_eq!(proxy.size(), size);
    }

    // ========== ProxySliver tests ==========

    #[test]
    fn test_proxy_sliver_creation() {
        let proxy = ProxySliver::new(TestData { value: 42.0 });
        assert_eq!(proxy.value, 42.0); // Deref works!
    }

    #[test]
    fn test_proxy_sliver_deref() {
        let mut proxy = ProxySliver::new(TestData { value: 1.0 });

        // Read via Deref
        assert_eq!(proxy.value, 1.0);

        // Write via DerefMut
        proxy.value = 2.0;
        assert_eq!(proxy.value, 2.0);
    }

    #[test]
    fn test_proxy_sliver_child_access() {
        let proxy = ProxySliver::new(TestData { value: 1.0 });

        // HasChild trait methods work via Ambassador
        assert!(!proxy.has_child());
        assert!(proxy.child().is_none());
    }

    #[test]
    fn test_proxy_sliver_geometry() {
        let mut proxy = ProxySliver::new(TestData { value: 1.0 });

        // HasSliverGeometry trait methods work via Ambassador
        let geometry = SliverGeometry::default();
        proxy.set_geometry(geometry.clone());
        assert_eq!(proxy.geometry(), &geometry);
    }
}
