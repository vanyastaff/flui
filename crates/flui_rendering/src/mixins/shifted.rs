//! Shifted mixin — applies offset transform to single child
//!
//! This module provides ShiftedBox<T> for render objects that position their child
//! with an offset (e.g., RenderPadding, RenderPositioned).
//!
//! # Pattern
//!
//! ```rust,ignore
//! // 1. Define your data
//! #[derive(Default, Clone, Debug)]
//! pub struct PaddingData {
//!     pub padding: EdgeInsets,
//! }
//!
//! // 2. Type alias
//! pub type RenderPadding = ShiftedBox<PaddingData>;
//!
//! // 3. MUST override perform_layout
//! impl RenderShiftedBox for RenderPadding {
//!     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
//!         let inner = constraints.deflate(&self.padding); // self.padding via Deref!
//!         if let Some(child) = self.child_mut().get_mut() {
//!             let child_size = child.layout(&inner);
//!             self.set_child_offset(Offset::new(self.padding.left, self.padding.top));
//!             let size = constraints.constrain(child_size + self.padding.size());
//!             self.set_size(size);
//!             size
//!         } else {
//!             let size = constraints.constrain(self.padding.size());
//!             self.set_size(size);
//!             size
//!         }
//!     }
//! }
//!
//! // AUTO: paint(), hit_test() apply child_offset automatically!
//! ```

use std::ops::{Deref, DerefMut};

use ambassador::{delegatable_trait, Delegate};
use flui_interaction::HitTestResult;
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::children::Child;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::PaintingContext;

// Re-export from proxy.rs
use super::proxy::{HasBoxGeometry, HasChild, HasSliverGeometry, ProxyBase, ProxyData};

// Import ambassador macros from proxy module
use super::proxy::{
    ambassador_impl_HasBoxGeometry, ambassador_impl_HasChild, ambassador_impl_HasSliverGeometry,
};

// ============================================================================
// Part 1: Delegatable Trait - HasOffset
// ============================================================================

/// Trait for accessing child offset (delegatable)
#[delegatable_trait]
pub trait HasOffset {
    fn child_offset(&self) -> Offset;
    fn set_child_offset(&mut self, offset: Offset);
}

// ============================================================================
// Part 2: Base Struct - ShiftedBase<P>
// ============================================================================

/// Base for shifted render objects (internal use)
///
/// Contains ProxyBase + offset field
#[derive(Debug)]
pub struct ShiftedBase<P: Protocol> {
    pub(crate) proxy: ProxyBase<P>,
    pub(crate) offset: Offset,
}

impl<P: Protocol> Default for ShiftedBase<P>
where
    P::Geometry: Default,
{
    fn default() -> Self {
        Self {
            proxy: ProxyBase::default(),
            offset: Offset::ZERO,
        }
    }
}

// Implement delegatable traits for ShiftedBase by forwarding to proxy
impl<P: Protocol> HasChild<P> for ShiftedBase<P> {
    fn child(&self) -> &Child<P> {
        self.proxy.child()
    }

    fn child_mut(&mut self) -> &mut Child<P> {
        self.proxy.child_mut()
    }
}

// Box specialization - delegate geometry to proxy
impl HasBoxGeometry for ShiftedBase<BoxProtocol> {
    fn size(&self) -> Size {
        self.proxy.size()
    }

    fn set_size(&mut self, size: Size) {
        self.proxy.set_size(size);
    }
}

// Sliver specialization - delegate geometry to proxy
impl HasSliverGeometry for ShiftedBase<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry {
        self.proxy.geometry()
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.proxy.set_geometry(geometry);
    }
}

// Implement HasOffset for ShiftedBase
impl<P: Protocol> HasOffset for ShiftedBase<P> {
    fn child_offset(&self) -> Offset {
        self.offset
    }

    fn set_child_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

// ============================================================================
// Part 3: Generic ShiftedBox<T> with Ambassador + Deref
// ============================================================================

/// Generic shifted render object with automatic delegation
///
/// # Type Parameters
///
/// - `T`: Custom data type (must implement `ProxyData`)
///
/// # Automatic Features
///
/// - **HasChild** via Ambassador delegation to `base`
/// - **HasBoxGeometry** via Ambassador delegation to `base`
/// - **HasOffset** via Ambassador delegation to `base`
/// - **Deref to T** for direct field access
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Default, Clone, Debug)]
/// pub struct PaddingData {
///     pub padding: EdgeInsets,
/// }
///
/// pub type RenderPadding = ShiftedBox<PaddingData>;
///
/// impl RenderShiftedBox for RenderPadding {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         // Layout logic using self.padding (via Deref)
///         // ...
///     }
/// }
/// ```
#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
#[delegate(HasOffset, target = "base")]
pub struct ShiftedBox<T: ProxyData> {
    base: ShiftedBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> ShiftedBox<T> {
    /// Create new ShiftedBox with data
    pub fn new(data: T) -> Self {
        Self {
            base: ShiftedBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access (self.padding instead of self.data.padding)
impl<T: ProxyData> Deref for ShiftedBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData> DerefMut for ShiftedBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 4: RenderShiftedBox - Mixin Trait with Defaults
// ============================================================================

/// Mixin trait for shifted Box render objects
///
/// Applies offset transform in paint/hit_test automatically.
///
/// **IMPORTANT:** `perform_layout` has NO default - you MUST override it!
///
/// # Example
///
/// ```rust,ignore
/// impl RenderShiftedBox for RenderPadding {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         // Your layout logic here
///         let inner = constraints.deflate(&self.padding);
///         // ... layout child, set offset, return size
///     }
///
///     // paint() and hit_test() auto-apply child_offset!
/// }
/// ```
pub trait RenderShiftedBox: HasChild<BoxProtocol> + HasBoxGeometry + HasOffset {
    /// Perform layout (NO DEFAULT - must override!)
    ///
    /// Your implementation should:
    /// 1. Layout child with modified constraints
    /// 2. Call `self.set_child_offset()` with calculated offset
    /// 3. Call `self.set_size()` with final size
    /// 4. Return the final size
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;

    /// Paint this render object (default: paint child with offset)
    fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // TODO: if let Some(id) = self.child().get() {
        //     ctx.paint_child(id, offset + self.child_offset());
        // }
    }

    /// Hit test (default: test child with offset adjustment)
    fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        // TODO: if let Some(id) = self.child().get() {
        //     return render_tree.hit_test(id, result, position - self.child_offset());
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

// Blanket impl: all ShiftedBox<T> get RenderShiftedBox
// BUT: perform_layout panics by default - MUST be overridden!
impl<T: ProxyData> RenderShiftedBox for ShiftedBox<T> {
    fn perform_layout(&mut self, _constraints: &BoxConstraints) -> Size {
        panic!(
            "perform_layout must be overridden for ShiftedBox<{}>",
            std::any::type_name::<T>()
        )
    }
}

// ============================================================================
// Part 5: ShiftedSliver<T> with Ambassador + Deref
// ============================================================================

/// Generic shifted sliver render object with automatic delegation
#[derive(Debug, Delegate)]
#[delegate(HasChild<SliverProtocol>, target = "base")]
#[delegate(HasSliverGeometry, target = "base")]
#[delegate(HasOffset, target = "base")]
pub struct ShiftedSliver<T: ProxyData> {
    base: ShiftedBase<SliverProtocol>,
    pub data: T,
}

impl<T: ProxyData> ShiftedSliver<T> {
    /// Create new ShiftedSliver with data
    pub fn new(data: T) -> Self {
        Self {
            base: ShiftedBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access
impl<T: ProxyData> Deref for ShiftedSliver<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData> DerefMut for ShiftedSliver<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 6: RenderShiftedSliver - Mixin Trait
// ============================================================================

/// Mixin trait for shifted Sliver render objects
///
/// Applies offset transform in paint/hit_test automatically.
///
/// **IMPORTANT:** `perform_layout` has NO default - you MUST override it!
pub trait RenderShiftedSliver: HasChild<SliverProtocol> + HasSliverGeometry + HasOffset {
    /// Perform layout (NO DEFAULT - must override!)
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;

    /// Paint this render object (default: paint child with offset)
    fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // TODO: if let Some(id) = self.child().get() {
        //     ctx.paint_child(id, offset + self.child_offset());
        // }
    }

    /// Hit test (default: test child with offset adjustment)
    fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        // TODO: if let Some(id) = self.child().get() {
        //     return render_tree.hit_test(id, result, position - self.child_offset());
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

// Blanket impl: all ShiftedSliver<T> get RenderShiftedSliver
// BUT: perform_layout panics by default - MUST be overridden!
impl<T: ProxyData> RenderShiftedSliver for ShiftedSliver<T> {
    fn perform_layout(&mut self, _constraints: &SliverConstraints) -> SliverGeometry {
        panic!(
            "perform_layout must be overridden for ShiftedSliver<{}>",
            std::any::type_name::<T>()
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone, Debug)]
    struct TestData {
        padding: f32,
    }

    #[test]
    fn test_shifted_box_creation() {
        let shifted = ShiftedBox::new(TestData { padding: 10.0 });
        assert_eq!(shifted.padding, 10.0); // Deref works!
    }

    #[test]
    fn test_shifted_box_deref() {
        let mut shifted = ShiftedBox::new(TestData { padding: 5.0 });

        // Read via Deref
        assert_eq!(shifted.padding, 5.0);

        // Write via DerefMut
        shifted.padding = 10.0;
        assert_eq!(shifted.padding, 10.0);
    }

    #[test]
    fn test_shifted_box_child_access() {
        let shifted = ShiftedBox::new(TestData { padding: 10.0 });

        // HasChild trait methods work via Ambassador
        assert!(!shifted.has_child());
        assert!(shifted.child().is_none());
    }

    #[test]
    fn test_shifted_box_geometry() {
        let mut shifted = ShiftedBox::new(TestData { padding: 10.0 });

        // HasBoxGeometry trait methods work via Ambassador
        let size = Size::new(100.0, 50.0);
        shifted.set_size(size);
        assert_eq!(shifted.size(), size);
    }

    #[test]
    fn test_shifted_box_offset() {
        let mut shifted = ShiftedBox::new(TestData { padding: 10.0 });

        // HasOffset trait methods work via Ambassador
        let offset = Offset::new(5.0, 10.0);
        shifted.set_child_offset(offset);
        assert_eq!(shifted.child_offset(), offset);
    }

    #[test]
    #[should_panic(expected = "perform_layout must be overridden")]
    fn test_shifted_box_perform_layout_panics_by_default() {
        let mut shifted = ShiftedBox::new(TestData { padding: 10.0 });
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Should panic because perform_layout is not overridden
        shifted.perform_layout(&constraints);
    }

    // ========== ShiftedSliver tests ==========

    #[test]
    fn test_shifted_sliver_creation() {
        let shifted = ShiftedSliver::new(TestData { padding: 10.0 });
        assert_eq!(shifted.padding, 10.0); // Deref works!
    }

    #[test]
    fn test_shifted_sliver_deref() {
        let mut shifted = ShiftedSliver::new(TestData { padding: 5.0 });

        // Read via Deref
        assert_eq!(shifted.padding, 5.0);

        // Write via DerefMut
        shifted.padding = 10.0;
        assert_eq!(shifted.padding, 10.0);
    }

    #[test]
    fn test_shifted_sliver_child_access() {
        let shifted = ShiftedSliver::new(TestData { padding: 10.0 });

        // HasChild trait methods work via Ambassador
        assert!(!shifted.has_child());
        assert!(shifted.child().is_none());
    }

    #[test]
    fn test_shifted_sliver_geometry() {
        let mut shifted = ShiftedSliver::new(TestData { padding: 10.0 });

        // HasSliverGeometry trait methods work via Ambassador
        let geometry = SliverGeometry::default();
        shifted.set_geometry(geometry.clone());
        assert_eq!(shifted.geometry(), &geometry);
    }

    #[test]
    fn test_shifted_sliver_offset() {
        let mut shifted = ShiftedSliver::new(TestData { padding: 10.0 });

        // HasOffset trait methods work via Ambassador
        let offset = Offset::new(5.0, 10.0);
        shifted.set_child_offset(offset);
        assert_eq!(shifted.child_offset(), offset);
    }

    #[test]
    #[should_panic(expected = "perform_layout must be overridden")]
    fn test_shifted_sliver_perform_layout_panics_by_default() {
        let mut shifted = ShiftedSliver::new(TestData { padding: 10.0 });
        let constraints = SliverConstraints::default();

        // Should panic because perform_layout is not overridden
        shifted.perform_layout(&constraints);
    }
}
