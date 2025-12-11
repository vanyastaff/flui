//! Leaf mixin — for render objects without children
//!
//! This module provides LeafBox<T> for render objects that have no children
//! and paint themselves directly (e.g., RenderColoredBox, RenderImage, RenderText).
//!
//! # Pattern
//!
//! ```rust,ignore
//! // 1. Define your data
//! #[derive(Clone, Debug)]
//! pub struct ColoredBoxData {
//!     pub color: Color,
//! }
//!
//! // 2. Type alias
//! pub type RenderColoredBox = LeafBox<ColoredBoxData>;
//!
//! // 3. MUST override perform_layout AND paint
//! impl RenderLeafBox for RenderColoredBox {
//!     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
//!         let size = constraints.biggest();
//!         self.set_size(size);
//!         size
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!         ctx.canvas().draw_rect(
//!             Rect::from_origin_size(offset, self.size()),
//!             &Paint::new().with_color(self.color), // self.color via Deref!
//!         );
//!     }
//! }
//!
//! // AUTO: hit_test() with bounds checking!
//! ```

use std::ops::{Deref, DerefMut};

use ambassador::Delegate;
use flui_types::{BoxConstraints, Offset, Size};

use crate::protocol::{Protocol, BoxProtocol};

// Re-export from proxy.rs
use super::proxy::{HasBoxGeometry, ProxyData};

// Import ambassador macros
use super::proxy::ambassador_impl_HasBoxGeometry;

// ============================================================================
// Part 1: Base Struct - LeafBase<P> (No delegatable traits needed)
// ============================================================================

/// Base for leaf render objects (internal use)
///
/// Contains only geometry (no children)
#[derive(Debug)]
pub struct LeafBase<P: Protocol> {
    pub(crate) geometry: P::Geometry,
}

impl<P: Protocol> Default for LeafBase<P>
where
    P::Geometry: Default,
{
    fn default() -> Self {
        Self {
            geometry: P::Geometry::default(),
        }
    }
}

// Box specialization - implement HasBoxGeometry
impl HasBoxGeometry for LeafBase<BoxProtocol> {
    fn size(&self) -> Size {
        self.geometry
    }

    fn set_size(&mut self, size: Size) {
        self.geometry = size;
    }
}

// ============================================================================
// Part 2: Generic LeafBox<T> with Ambassador + Deref
// ============================================================================

/// Generic leaf render object with automatic delegation
///
/// # Type Parameters
///
/// - `T`: Custom data type (must implement `ProxyData`)
///
/// # Automatic Features
///
/// - **HasBoxGeometry** via Ambassador delegation to `base`
/// - **Deref to T** for direct field access
/// - **hit_test()** with default bounds checking
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct ColoredBoxData {
///     pub color: Color,
/// }
///
/// pub type RenderColoredBox = LeafBox<ColoredBoxData>;
///
/// impl RenderLeafBox for RenderColoredBox {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         let size = constraints.biggest();
///         self.set_size(size);
///         size
///     }
///
///     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///         // Paint using self.color (via Deref) and self.size() (via Ambassador)
///         ctx.canvas().draw_rect(...);
///     }
/// }
/// ```
#[derive(Debug, Delegate)]
#[delegate(HasBoxGeometry, target = "base")]
pub struct LeafBox<T: ProxyData> {
    base: LeafBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> LeafBox<T> {
    /// Create new LeafBox with data
    pub fn new(data: T) -> Self {
        Self {
            base: LeafBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access
impl<T: ProxyData> Deref for LeafBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData> DerefMut for LeafBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 3: RenderLeafBox - Mixin Trait
// ============================================================================

/// Mixin trait for leaf Box render objects
///
/// Leaf render objects have no children and paint themselves.
///
/// **IMPORTANT:** You MUST override both `perform_layout` AND `paint`!
///
/// # Example
///
/// ```rust,ignore
/// impl RenderLeafBox for RenderColoredBox {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         let size = constraints.biggest();
///         self.set_size(size);
///         size
///     }
///
///     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///         ctx.canvas().draw_rect(
///             Rect::from_origin_size(offset, self.size()),
///             &Paint::new().with_color(self.color),
///         );
///     }
///
///     // hit_test() has default bounds checking
/// }
/// ```
pub trait RenderLeafBox: HasBoxGeometry {
    /// Perform layout (NO DEFAULT - must override!)
    ///
    /// Your implementation should:
    /// 1. Calculate size based on constraints
    /// 2. Call `self.set_size()` with calculated size
    /// 3. Return the size
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;

    /// Paint this render object (NO DEFAULT - must override!)
    ///
    /// Your implementation should paint the visual representation
    /// using the painting context.
    fn paint(&self, ctx: &mut dyn std::any::Any, offset: Offset);

    /// Hit test (default: check if position is within bounds)
    fn hit_test(&self, _result: &mut dyn std::any::Any, position: Offset) -> bool {
        let size = self.size();
        position.dx >= 0.0 && position.dx < size.width &&
        position.dy >= 0.0 && position.dy < size.height
    }

    /// Compute minimum intrinsic width (default: 0)
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Compute maximum intrinsic width (default: 0)
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Compute minimum intrinsic height (default: 0)
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Compute maximum intrinsic height (default: 0)
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
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

// Blanket impl: all LeafBox<T> get RenderLeafBox
// BUT: perform_layout and paint panic by default - MUST be overridden!
impl<T: ProxyData> RenderLeafBox for LeafBox<T> {
    fn perform_layout(&mut self, _constraints: &BoxConstraints) -> Size {
        panic!(
            "perform_layout must be overridden for LeafBox<{}>",
            std::any::type_name::<T>()
        )
    }

    fn paint(&self, _ctx: &mut dyn std::any::Any, _offset: Offset) {
        panic!(
            "paint must be overridden for LeafBox<{}>",
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
        value: u32,
    }

    #[test]
    fn test_leaf_box_creation() {
        let leaf = LeafBox::new(TestData { value: 42 });
        assert_eq!(leaf.value, 42); // Deref works!
    }

    #[test]
    fn test_leaf_box_deref() {
        let mut leaf = LeafBox::new(TestData { value: 1 });

        // Read via Deref
        assert_eq!(leaf.value, 1);

        // Write via DerefMut
        leaf.value = 100;
        assert_eq!(leaf.value, 100);
    }

    #[test]
    fn test_leaf_box_geometry() {
        let mut leaf = LeafBox::new(TestData::default());

        // HasBoxGeometry trait methods work via Ambassador
        let size = Size::new(100.0, 50.0);
        leaf.set_size(size);
        assert_eq!(leaf.size(), size);
    }

    #[test]
    fn test_leaf_box_hit_test() {
        let mut leaf = LeafBox::new(TestData::default());
        leaf.set_size(Size::new(100.0, 50.0));

        // Mock result (any type works since we're not using it)
        let mut result = ();

        // Inside bounds
        assert!(leaf.hit_test(&mut result, Offset::new(50.0, 25.0)));
        assert!(leaf.hit_test(&mut result, Offset::new(0.0, 0.0)));
        assert!(leaf.hit_test(&mut result, Offset::new(99.9, 49.9)));

        // Outside bounds
        assert!(!leaf.hit_test(&mut result, Offset::new(-1.0, 0.0)));
        assert!(!leaf.hit_test(&mut result, Offset::new(0.0, -1.0)));
        assert!(!leaf.hit_test(&mut result, Offset::new(100.0, 25.0)));
        assert!(!leaf.hit_test(&mut result, Offset::new(50.0, 50.0)));
    }

    #[test]
    #[should_panic(expected = "perform_layout must be overridden")]
    fn test_leaf_box_perform_layout_panics_by_default() {
        let mut leaf = LeafBox::new(TestData::default());
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Should panic because perform_layout is not overridden
        leaf.perform_layout(&constraints);
    }

    #[test]
    #[should_panic(expected = "paint must be overridden")]
    fn test_leaf_box_paint_panics_by_default() {
        let leaf = LeafBox::new(TestData::default());
        let mut ctx = ();

        // Should panic because paint is not overridden
        leaf.paint(&mut ctx, Offset::ZERO);
    }
}
