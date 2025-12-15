//! Shifted container - single child with custom offset positioning.
//!
//! This module provides:
//! - [`ShiftedContainer`] - Generic trait for shifted containers with offset
//! - [`Shifted`] - Concrete shifted container implementation
//!
//! This is the Rust equivalent of Flutter's `RenderShiftedBox` pattern.
//! Use when parent needs to position child at a specific offset.

use flui_tree::arity::Optional;
use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::{Children, SingleChildContainer};
use crate::constraints::SliverGeometry;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};

// ============================================================================
// ShiftedContainer trait - Generic shifted container (child with offset)
// ============================================================================

/// Generic trait for shifted containers that store geometry and offset.
///
/// A shifted container positions its child at a computed offset.
/// This is the base for padding, alignment, etc.
///
/// # Type Parameters
///
/// - `T: ?Sized` - The child object type
/// - `G` - The geometry type
pub trait ShiftedContainer<T: ?Sized, G>: SingleChildContainer<T> {
    /// Returns a reference to the cached geometry.
    fn geometry(&self) -> &G;

    /// Sets the cached geometry.
    fn set_geometry(&mut self, geometry: G);

    /// Returns the child's offset within the parent.
    fn offset(&self) -> flui_types::Offset;

    /// Sets the child's offset within the parent.
    fn set_offset(&mut self, offset: flui_types::Offset);
}

/// Container that stores a single child with custom offset positioning.
///
/// This is the storage pattern for render objects that:
/// - Position child at a non-zero offset (padding, margins)
/// - Compute child position during layout
/// - Need to adjust hit testing by the offset
///
/// # Flutter Equivalent
///
/// This corresponds to `RenderShiftedBox` in Flutter, which:
/// - Stores `BoxParentData` on child for the offset
/// - Uses offset in `paint` and `hitTestChildren`
///
/// In FLUI, we store the offset directly in the container for simplicity.
///
/// # Generic Traits
///
/// `Shifted<P>` implements generic traits that work with any protocol:
///
/// - [`SingleChildContainer<P::Object>`] - child access methods
/// - [`ShiftedContainer<P::Object, P::Geometry>`] - geometry and offset storage
///
/// ```rust,ignore
/// // Works for both ShiftedBox and ShiftedSliver
/// fn use_shifted<T, G>(shifted: &T)
/// where
///     T: ShiftedContainer<dyn RenderBox, G>,
/// {
///     if let Some(child) = shifted.child() {
///         println!("Offset: {:?}", shifted.offset());
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderPadding {
///     shifted: ShiftedBox,
///     padding: EdgeInsets,
/// }
///
/// impl RenderBox for RenderPadding {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         let inner = constraints.deflate(&self.padding);
///
///         let child_size = self.shifted.child_mut()
///             .map(|c| c.perform_layout(inner))
///             .unwrap_or(Size::ZERO);
///
///         // Position child at padding offset
///         self.shifted.set_offset(Offset::new(
///             self.padding.left,
///             self.padding.top,
///         ));
///
///         let size = Size::new(
///             child_size.width + self.padding.horizontal(),
///             child_size.height + self.padding.vertical(),
///         );
///         self.shifted.set_geometry(size);
///         size
///     }
/// }
/// ```
pub struct Shifted<P: Protocol> {
    child: Children<P, Optional>,
    geometry: P::Geometry,
    offset: Offset,
}

impl<P: Protocol> Debug for Shifted<P>
where
    P::Object: Debug,
    P::Geometry: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shifted")
            .field("has_child", &self.child.has_child())
            .field("geometry", &self.geometry)
            .field("offset", &self.offset)
            .finish()
    }
}

impl<P: Protocol> Default for Shifted<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Shifted<P> {
    /// Creates a new empty shifted container.
    pub fn new() -> Self {
        Self {
            child: Children::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
        }
    }

    /// Creates a shifted container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        let mut container = Children::new();
        container.set(child);
        Self {
            child: container,
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
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
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }

    /// Returns the child's offset within the parent.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this is stored in `BoxParentData.offset`.
    /// We store it directly for simpler access.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the child's offset within the parent.
    ///
    /// This should be called during layout to position the child.
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

/// Box shifted container (geometry is `Size`, with offset).
///
/// Use for render objects that position child at a computed offset.
///
/// # Flutter Equivalent
///
/// `RenderShiftedBox` and subclasses like:
/// - `RenderPadding`
/// - `RenderPositionedBox` (via `RenderAligningShiftedBox`)
/// - `RenderFractionallySizedOverflowBox`
/// - `RenderConstrainedOverflowBox`
pub type ShiftedBox = Shifted<BoxProtocol>;

/// Sliver shifted container.
pub type ShiftedSliver = Shifted<SliverProtocol>;

// ============================================================================
// Generic trait implementations for Shifted<P>
// ============================================================================

impl<P: Protocol> SingleChildContainer<P::Object> for Shifted<P> {
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

impl<P: Protocol> ShiftedContainer<P::Object, P::Geometry> for Shifted<P> {
    #[inline]
    fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }

    #[inline]
    fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }

    #[inline]
    fn offset(&self) -> Offset {
        self.offset
    }

    #[inline]
    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

impl ShiftedBox {
    /// Returns the cached size.
    pub fn size(&self) -> Size {
        self.geometry
    }
}

impl ShiftedSliver {
    /// Returns the cached sliver geometry.
    pub fn sliver_geometry(&self) -> &SliverGeometry {
        &self.geometry
    }
}

// ============================================================================
// Paint and Hit Testing Helpers for ShiftedBox
// ============================================================================

impl ShiftedBox {
    /// Paints the child at the stored offset.
    ///
    /// This is the default paint implementation for shifted box containers.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.paint`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderPadding {
    ///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
    ///         self.shifted.paint_child(offset, |child, child_offset| {
    ///             child.paint(context, child_offset);
    ///         });
    ///     }
    /// }
    /// ```
    pub fn paint_child<F>(&self, base_offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child() {
            let child_offset = base_offset + self.offset;
            paint_fn(child, child_offset);
        }
    }

    /// Paints the child with a custom offset (ignoring stored offset).
    pub fn paint_child_at<F>(&self, base_offset: Offset, child_offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child() {
            paint_fn(child, base_offset + child_offset);
        }
    }

    /// Hit tests the child at the stored offset.
    ///
    /// This is the default hit test implementation for shifted box containers.
    /// It transforms the position by the stored offset and tests the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.hitTestChildren`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderPadding {
    ///     fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
    ///         self.shifted.hit_test_child(result, position)
    ///     }
    /// }
    /// ```
    pub fn hit_test_child(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            result.add_with_paint_offset(Some(self.offset), position, |result, transformed| {
                child.hit_test(result, transformed)
            })
        } else {
            false
        }
    }

    /// Hit tests the child with a custom offset (ignoring stored offset).
    ///
    /// Use this when you need to apply a different offset than what's stored,
    /// such as for animated positions.
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
    use crate::traits::RenderSliver;

    #[test]
    fn test_shifted_box_default() {
        let shifted: ShiftedBox = Shifted::new();
        assert!(!shifted.has_child());
        assert_eq!(shifted.size(), Size::ZERO);
        assert_eq!(shifted.offset(), Offset::ZERO);
    }

    #[test]
    fn test_shifted_box_set_offset() {
        let mut shifted: ShiftedBox = Shifted::new();
        shifted.set_offset(Offset::new(10.0, 20.0));
        assert_eq!(shifted.offset(), Offset::new(10.0, 20.0));
    }

    // ========================================================================
    // Generic trait tests - verify traits work with any Protocol
    // ========================================================================

    /// Helper function that works with any SingleChildContainer
    fn use_single_child<T: ?Sized, C: SingleChildContainer<T>>(container: &C) -> bool {
        container.has_child()
    }

    /// Helper function that works with any ShiftedContainer - set and get offset
    fn set_and_get_offset<T: ?Sized, G, C: ShiftedContainer<T, G>>(
        container: &mut C,
        offset: Offset,
    ) -> Offset {
        container.set_offset(offset);
        container.offset()
    }

    #[test]
    fn test_single_child_container_box_protocol() {
        let shifted: Shifted<BoxProtocol> = Shifted::new();
        // Verify generic function works with BoxProtocol
        assert!(!use_single_child::<dyn RenderBox, _>(&shifted));
    }

    #[test]
    fn test_single_child_container_sliver_protocol() {
        let shifted: Shifted<SliverProtocol> = Shifted::new();
        // Verify generic function works with SliverProtocol
        assert!(!use_single_child::<dyn RenderSliver, _>(&shifted));
    }

    #[test]
    fn test_shifted_container_box_protocol() {
        let mut shifted: Shifted<BoxProtocol> = Shifted::new();

        // Verify ShiftedContainer trait works with BoxProtocol
        let offset =
            set_and_get_offset::<dyn RenderBox, Size, _>(&mut shifted, Offset::new(15.0, 25.0));
        assert_eq!(offset, Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_shifted_container_sliver_protocol() {
        let mut shifted: Shifted<SliverProtocol> = Shifted::new();

        // Verify ShiftedContainer trait works with SliverProtocol
        let offset = set_and_get_offset::<dyn RenderSliver, SliverGeometry, _>(
            &mut shifted,
            Offset::new(30.0, 40.0),
        );
        assert_eq!(offset, Offset::new(30.0, 40.0));
    }

    #[test]
    fn test_shifted_container_geometry_box() {
        let mut shifted: Shifted<BoxProtocol> = Shifted::new();
        shifted.set_geometry(Size::new(200.0, 100.0));

        // Access geometry through ShiftedContainer trait
        assert_eq!(*shifted.geometry(), Size::new(200.0, 100.0));
    }

    #[test]
    fn test_shifted_container_geometry_sliver() {
        let mut shifted: Shifted<SliverProtocol> = Shifted::new();
        let geom = SliverGeometry::default();
        shifted.set_geometry(geom.clone());

        // Access geometry through ShiftedContainer trait
        assert_eq!(shifted.geometry().scroll_extent, geom.scroll_extent);
    }

    // ========================================================================
    // Type alias tests
    // ========================================================================

    #[test]
    fn test_shifted_box_alias() {
        let _: ShiftedBox = Shifted::new();
    }

    #[test]
    fn test_shifted_sliver_alias() {
        let _: ShiftedSliver = Shifted::new();
    }
}
