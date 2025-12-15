//! Shifted container - single child with custom offset positioning.
//!
//! This module provides:
//! - [`ShiftedContainer`] - Generic trait for shifted containers with offset
//! - [`Shifted`] - Concrete shifted container implementation
//!
//! This is the Rust equivalent of Flutter's `RenderShiftedBox` pattern.
//! Use when parent needs to position child at a specific offset.

use ambassador::{delegatable_trait, Delegate};

use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::base::{ambassador_impl_BaseContainer, BaseContainer};
use super::single_child::SingleChild;
use super::SingleChildContainer;
use crate::constraints::SliverGeometry;
use crate::lifecycle::RenderObjectState;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};

// ============================================================================
// ShiftedContainer trait - Generic shifted container (child with offset)
// ============================================================================

/// Generic trait for shifted containers that store offset.
///
/// A shifted container positions its child at a computed offset.
/// This is the base for padding, alignment, etc.
///
/// # Type Parameters
///
/// - `T` - The boxed child type (e.g., `Box<dyn RenderBox>`)
///
/// # Why `T: Sized`?
///
/// We use `T` as the boxed type (not `T: ?Sized`) to enable Ambassador delegation.
/// This means `T = Box<dyn RenderBox>` rather than `T = dyn RenderBox`.
#[delegatable_trait]
pub trait ShiftedContainer<T>: SingleChildContainer<T> {
    /// Returns the child's offset within the parent.
    fn offset(&self) -> Offset;

    /// Sets the child's offset within the parent.
    fn set_offset(&mut self, offset: Offset);
}

// ============================================================================
// Shifted struct - uses SingleChild<P> internally
// ============================================================================

/// Container that stores a single child with custom offset positioning.
///
/// This is the third level of the compositional hierarchy:
/// - `Base<P>` → state + geometry
/// - `SingleChild<P>` → base + child
/// - `Shifted<P>` → single_child + offset
///
/// # Flutter Equivalent
///
/// This corresponds to `RenderShiftedBox` in Flutter, which:
/// - Stores `BoxParentData` on child for the offset
/// - Uses offset in `paint` and `hitTestChildren`
///
/// In FLUI, we store the offset directly in the container for simplicity.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderPadding {
///     shifted: Shifted<BoxProtocol>,
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
#[derive(Delegate)]
#[delegate(BaseContainer<P::Geometry>, target = "inner")]
pub struct Shifted<P: Protocol> {
    /// Inner single child container (base + child).
    inner: SingleChild<P>,

    /// Child's offset within the parent.
    offset: Offset,
}

impl<P: Protocol> Debug for Shifted<P>
where
    P::Object: Debug,
    P::Geometry: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shifted")
            .field("inner", &self.inner)
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
            inner: SingleChild::new(),
            offset: Offset::ZERO,
        }
    }

    /// Creates a shifted container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        Self {
            inner: SingleChild::with_child(child),
            offset: Offset::ZERO,
        }
    }

    /// Returns a reference to the inner single child container.
    #[inline]
    pub fn inner(&self) -> &SingleChild<P> {
        &self.inner
    }

    /// Returns a mutable reference to the inner single child container.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut SingleChild<P> {
        &mut self.inner
    }

    /// Returns a reference to the child, if present.
    #[inline]
    pub fn child(&self) -> Option<&P::Object> {
        self.inner.child()
    }

    /// Returns a mutable reference to the child, if present.
    #[inline]
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.inner.child_mut()
    }

    /// Sets the child, replacing any existing child.
    #[inline]
    pub fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.inner.set_child(child)
    }

    /// Takes the child out of the container, leaving it empty.
    #[inline]
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.inner.take_child()
    }

    /// Returns `true` if the container has a child.
    #[inline]
    pub fn has_child(&self) -> bool {
        self.inner.has_child()
    }

    /// Returns the child's offset within the parent.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this is stored in `BoxParentData.offset`.
    /// We store it directly for simpler access.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the child's offset within the parent.
    ///
    /// This should be called during layout to position the child.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Returns a reference to the cached geometry.
    #[inline]
    pub fn geometry(&self) -> &P::Geometry {
        self.inner.base().geometry()
    }

    /// Sets the cached geometry.
    #[inline]
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.inner.base_mut().set_geometry(geometry);
    }
}

// ============================================================================
// SingleChildContainer trait implementation (manual, not delegated)
// ============================================================================

impl<P: Protocol> SingleChildContainer<Box<P::Object>> for Shifted<P> {
    #[inline]
    fn child(&self) -> Option<&Box<P::Object>> {
        self.inner.child_box()
    }

    #[inline]
    fn child_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.inner.child_box_mut()
    }

    #[inline]
    fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.inner.set_child(child)
    }

    #[inline]
    fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.inner.take_child()
    }
}

// ============================================================================
// ShiftedContainer trait implementation
// ============================================================================

impl<P: Protocol> ShiftedContainer<Box<P::Object>> for Shifted<P> {
    #[inline]
    fn offset(&self) -> Offset {
        self.offset
    }

    #[inline]
    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

// ============================================================================
// Type aliases
// ============================================================================

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
// Convenience methods for ShiftedBox
// ============================================================================

impl ShiftedBox {
    /// Returns the cached size.
    #[inline]
    pub fn size(&self) -> Size {
        *self.inner.base().geometry()
    }

    /// Sets the cached size.
    #[inline]
    pub fn set_size(&mut self, size: Size) {
        self.inner.base_mut().set_geometry(size);
    }

    /// Paints the child at the stored offset.
    ///
    /// This is the default paint implementation for shifted box containers.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.paint`.
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

impl ShiftedSliver {
    /// Returns the cached sliver geometry.
    #[inline]
    pub fn sliver_geometry(&self) -> &SliverGeometry {
        self.inner.base().geometry()
    }

    /// Sets the cached sliver geometry.
    #[inline]
    pub fn set_sliver_geometry(&mut self, geometry: SliverGeometry) {
        self.inner.base_mut().set_geometry(geometry);
    }
}

// ============================================================================
// Tests
// ============================================================================

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

    #[test]
    fn test_shifted_box_set_size() {
        let mut shifted: ShiftedBox = Shifted::new();
        shifted.set_size(Size::new(100.0, 50.0));
        assert_eq!(shifted.size(), Size::new(100.0, 50.0));
    }

    #[test]
    fn test_shifted_uses_single_child() {
        let shifted: ShiftedBox = Shifted::new();
        // Access inner SingleChild
        assert!(!shifted.inner().has_child());
        // Access base through inner (not attached initially)
        assert!(!shifted.inner().base().state().is_attached());
    }

    // ========================================================================
    // Generic trait tests - verify traits work with any Protocol
    // ========================================================================

    /// Helper function that works with any SingleChildContainer
    fn use_single_child<T, C: SingleChildContainer<T>>(container: &C) -> bool {
        container.has_child()
    }

    /// Helper function that works with any ShiftedContainer
    fn set_and_get_offset<T, C: ShiftedContainer<T>>(container: &mut C, offset: Offset) -> Offset {
        container.set_offset(offset);
        container.offset()
    }

    #[test]
    fn test_single_child_container_box_protocol() {
        let shifted: Shifted<BoxProtocol> = Shifted::new();
        // Verify generic function works with BoxProtocol
        assert!(!use_single_child::<Box<dyn RenderBox>, _>(&shifted));
    }

    #[test]
    fn test_single_child_container_sliver_protocol() {
        let shifted: Shifted<SliverProtocol> = Shifted::new();
        // Verify generic function works with SliverProtocol
        assert!(!use_single_child::<Box<dyn RenderSliver>, _>(&shifted));
    }

    #[test]
    fn test_shifted_container_box_protocol() {
        let mut shifted: Shifted<BoxProtocol> = Shifted::new();

        // Verify ShiftedContainer trait works with BoxProtocol
        let offset =
            set_and_get_offset::<Box<dyn RenderBox>, _>(&mut shifted, Offset::new(15.0, 25.0));
        assert_eq!(offset, Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_shifted_container_sliver_protocol() {
        let mut shifted: Shifted<SliverProtocol> = Shifted::new();

        // Verify ShiftedContainer trait works with SliverProtocol
        let offset =
            set_and_get_offset::<Box<dyn RenderSliver>, _>(&mut shifted, Offset::new(30.0, 40.0));
        assert_eq!(offset, Offset::new(30.0, 40.0));
    }

    #[test]
    fn test_base_container_delegation() {
        let mut shifted: ShiftedBox = Shifted::new();

        // Access through BaseContainer trait (delegated to inner)
        shifted.set_geometry(Size::new(200.0, 100.0));
        assert_eq!(shifted.geometry(), &Size::new(200.0, 100.0));

        // State should be accessible (not attached initially)
        assert!(!shifted.state().is_attached());
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
