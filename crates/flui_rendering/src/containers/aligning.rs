//! Aligning container - single child with alignment and size factors.
//!
//! This is the Rust equivalent of Flutter's `RenderAligningShiftedBox` pattern.

use crate::constraints::SliverGeometry;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};
use flui_types::{Alignment, Offset, Size};
use std::fmt::Debug;

use super::Single;

/// Container for single child with alignment and optional size factors.
///
/// This is the storage pattern for render objects that:
/// - Align child within available space
/// - Optionally scale size based on child size (width/height factors)
///
/// # Flutter Equivalent
///
/// This corresponds to `RenderAligningShiftedBox` in Flutter.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderAlign {
///     aligning: AligningBox,
/// }
///
/// impl RenderBox for RenderAlign {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         // ... layout child and compute alignment offset
///         self.aligning.align_child(my_size, child_size);
///         my_size
///     }
/// }
/// ```
pub struct Aligning<P: Protocol> {
    child: Single<P>,
    geometry: P::Geometry,
    offset: Offset,
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
}

impl<P: Protocol> Debug for Aligning<P>
where
    P::Object: Debug,
    P::Geometry: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Aligning")
            .field("has_child", &self.child.has_child())
            .field("geometry", &self.geometry)
            .field("offset", &self.offset)
            .field("alignment", &self.alignment)
            .field("width_factor", &self.width_factor)
            .field("height_factor", &self.height_factor)
            .finish()
    }
}

impl<P: Protocol> Default for Aligning<P> {
    fn default() -> Self {
        Self::new(Alignment::CENTER)
    }
}

impl<P: Protocol> Aligning<P> {
    /// Creates a new aligning container with the given alignment.
    pub fn new(alignment: Alignment) -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
            alignment,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Creates an aligning container with the given child and alignment.
    pub fn with_child(child: Box<P::Object>, alignment: Alignment) -> Self {
        Self {
            child: Single::with_child(child),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
            alignment,
            width_factor: None,
            height_factor: None,
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
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }

    /// Returns the child's offset within the parent.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the child's offset within the parent.
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Returns the alignment.
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Sets the alignment.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Returns the width factor.
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    /// Sets the width factor.
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        self.width_factor = factor;
    }

    /// Returns the height factor.
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }

    /// Sets the height factor.
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        self.height_factor = factor;
    }
}

/// Box aligning container.
pub type AligningBox = Aligning<BoxProtocol>;

/// Sliver aligning container.
pub type AligningSliver = Aligning<SliverProtocol>;

impl AligningBox {
    /// Returns the cached size.
    pub fn size(&self) -> Size {
        self.geometry
    }

    /// Computes and sets the child offset based on alignment.
    ///
    /// # Flutter Equivalent
    ///
    /// This is `RenderAligningShiftedBox.alignChild()`.
    pub fn align_child(&mut self, parent_size: Size, child_size: Size) {
        self.offset = self.alignment.along_offset(Offset::new(
            parent_size.width - child_size.width,
            parent_size.height - child_size.height,
        ));
    }
}

impl AligningSliver {
    /// Returns the cached sliver geometry.
    pub fn sliver_geometry(&self) -> &SliverGeometry {
        &self.geometry
    }
}

// ============================================================================
// Paint and Hit Testing Helpers for AligningBox
// ============================================================================

impl AligningBox {
    /// Paints the child at the computed alignment offset.
    ///
    /// Uses the stored offset that was computed by `align_child()`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.paint`,
    /// which is inherited from `RenderShiftedBox`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderAlign {
    ///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
    ///         self.aligning.paint_child(offset, |child, child_offset| {
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

    /// Paints the child with a custom offset (ignoring computed alignment offset).
    pub fn paint_child_at<F>(&self, base_offset: Offset, child_offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        if let Some(child) = self.child() {
            paint_fn(child, base_offset + child_offset);
        }
    }

    /// Hit tests the child at the computed alignment offset.
    ///
    /// Uses the stored offset that was computed by `align_child()`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderAligningShiftedBox.hitTestChildren`,
    /// which is inherited from `RenderShiftedBox`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderAlign {
    ///     fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
    ///         self.aligning.hit_test_child(result, position)
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

    /// Hit tests the child with a custom offset (ignoring computed alignment offset).
    ///
    /// Use this when you need to apply a different offset than what's computed,
    /// such as for animated alignment transitions.
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
    fn test_aligning_box_default() {
        let aligning: AligningBox = Aligning::new(Alignment::CENTER);
        assert!(!aligning.has_child());
        assert_eq!(aligning.alignment(), Alignment::CENTER);
    }

    #[test]
    fn test_aligning_box_align_child() {
        let mut aligning: AligningBox = Aligning::new(Alignment::CENTER);
        aligning.align_child(Size::new(100.0, 100.0), Size::new(50.0, 50.0));
        assert_eq!(aligning.offset(), Offset::new(25.0, 25.0));
    }

    #[test]
    fn test_aligning_box_top_left() {
        let mut aligning: AligningBox = Aligning::new(Alignment::TOP_LEFT);
        aligning.align_child(Size::new(100.0, 100.0), Size::new(50.0, 50.0));
        assert_eq!(aligning.offset(), Offset::new(0.0, 0.0));
    }

    #[test]
    fn test_aligning_box_bottom_right() {
        let mut aligning: AligningBox = Aligning::new(Alignment::BOTTOM_RIGHT);
        aligning.align_child(Size::new(100.0, 100.0), Size::new(50.0, 50.0));
        assert_eq!(aligning.offset(), Offset::new(50.0, 50.0));
    }
}
