//! Shifted container - single child with custom offset positioning.
//!
//! This is the Rust equivalent of Flutter's `RenderShiftedBox` pattern.
//! Use when parent needs to position child at a specific offset.

use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use flui_types::{Offset, Size, SliverGeometry};
use std::fmt::Debug;

use super::Single;

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
    child: Single<P>,
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
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
        }
    }

    /// Creates a shifted container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        Self {
            child: Single::with_child(child),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
