//! Unified geometry types for render nodes.
//!
//! This module provides protocol-agnostic geometry types that can represent
//! either Box (2D) or Sliver (scrolling) geometry.
//!
//! # Flutter Equivalence
//!
//! In Flutter, `RenderObject` stores constraints and geometry as abstract types,
//! with concrete implementations in `RenderBox` and `RenderSliver`.
//!
//! In FLUI, we use enums to allow a single `RenderNode` to hold either type.

use flui_types::constraints::{BoxConstraints, Constraints};
use flui_types::geometry::Size;
use flui_types::{SliverConstraints, SliverGeometry};

// ============================================================================
// CONSTRAINTS ENUM
// ============================================================================

/// Protocol-agnostic constraints for render nodes.
///
/// This enum allows `RenderNode` to store constraints without knowing
/// the specific protocol (Box vs Sliver) at compile time.
///
/// # Flutter Equivalence
///
/// ```dart
/// abstract class RenderObject {
///   Constraints? _constraints;
/// }
/// ```
///
/// Flutter uses the abstract `Constraints` type. In Rust, we use an enum
/// to achieve the same flexibility with type safety.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderConstraints {
    /// Box constraints for 2D layout (width × height).
    ///
    /// Used by `RenderBox` implementations like containers, text, images.
    Box(BoxConstraints),

    /// Sliver constraints for scrolling layout.
    ///
    /// Used by `RenderSliver` implementations in scroll views.
    Sliver(SliverConstraints),
}

impl RenderConstraints {
    /// Returns true if these are tight constraints.
    ///
    /// For Box: both width and height are exactly specified.
    /// For Sliver: always false (slivers don't have tight constraints).
    #[inline]
    pub fn is_tight(&self) -> bool {
        match self {
            Self::Box(c) => c.is_tight(),
            Self::Sliver(_) => false,
        }
    }

    /// Returns true if this is a Box constraint.
    #[inline]
    pub fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Returns true if this is a Sliver constraint.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }

    /// Attempts to get Box constraints.
    ///
    /// Returns `None` if this is a Sliver constraint.
    #[inline]
    pub fn as_box(&self) -> Option<&BoxConstraints> {
        match self {
            Self::Box(c) => Some(c),
            Self::Sliver(_) => None,
        }
    }

    /// Attempts to get Sliver constraints.
    ///
    /// Returns `None` if this is a Box constraint.
    #[inline]
    pub fn as_sliver(&self) -> Option<&SliverConstraints> {
        match self {
            Self::Sliver(c) => Some(c),
            Self::Box(_) => None,
        }
    }
}

impl From<BoxConstraints> for RenderConstraints {
    fn from(constraints: BoxConstraints) -> Self {
        Self::Box(constraints)
    }
}

impl From<SliverConstraints> for RenderConstraints {
    fn from(constraints: SliverConstraints) -> Self {
        Self::Sliver(constraints)
    }
}

impl Default for RenderConstraints {
    fn default() -> Self {
        Self::Box(BoxConstraints::UNCONSTRAINED)
    }
}

impl Constraints for RenderConstraints {
    fn is_tight(&self) -> bool {
        match self {
            Self::Box(c) => c.is_tight(),
            Self::Sliver(_) => false,
        }
    }

    fn is_normalized(&self) -> bool {
        match self {
            Self::Box(c) => c.is_normalized(),
            Self::Sliver(c) => c.is_normalized(),
        }
    }
}

// ============================================================================
// GEOMETRY ENUM
// ============================================================================

/// Protocol-agnostic geometry result from layout.
///
/// This enum allows `RenderNode` to store layout results without knowing
/// the specific protocol (Box vs Sliver) at compile time.
///
/// # Flutter Equivalence
///
/// In Flutter, `RenderBox` stores `Size` and `RenderSliver` stores `SliverGeometry`.
/// This enum unifies both cases.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderGeometry {
    /// Box geometry - a simple 2D size.
    ///
    /// Used by `RenderBox` implementations.
    Box(Size),

    /// Sliver geometry - scroll extent, paint extent, etc.
    ///
    /// Used by `RenderSliver` implementations.
    Sliver(SliverGeometry),
}

impl RenderGeometry {
    /// Returns true if this is Box geometry.
    #[inline]
    pub fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Returns true if this is Sliver geometry.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }

    /// Attempts to get Box geometry (Size).
    ///
    /// Returns `None` if this is Sliver geometry.
    #[inline]
    pub fn as_box(&self) -> Option<Size> {
        match self {
            Self::Box(size) => Some(*size),
            Self::Sliver(_) => None,
        }
    }

    /// Attempts to get the size.
    ///
    /// For Box: returns the size directly.
    /// For Sliver: returns a size based on paint extent (main axis only).
    #[inline]
    pub fn size(&self) -> Size {
        match self {
            Self::Box(size) => *size,
            // Slivers don't have cross-axis extent in geometry,
            // return paint_extent as height (main axis)
            Self::Sliver(geom) => Size::new(0.0, geom.paint_extent),
        }
    }

    /// Attempts to get Sliver geometry.
    ///
    /// Returns `None` if this is Box geometry.
    #[inline]
    pub fn as_sliver(&self) -> Option<&SliverGeometry> {
        match self {
            Self::Sliver(g) => Some(g),
            Self::Box(_) => None,
        }
    }
}

impl From<Size> for RenderGeometry {
    fn from(size: Size) -> Self {
        Self::Box(size)
    }
}

impl From<SliverGeometry> for RenderGeometry {
    fn from(geometry: SliverGeometry) -> Self {
        Self::Sliver(geometry)
    }
}

impl Default for RenderGeometry {
    fn default() -> Self {
        Self::Box(Size::ZERO)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_constraints() {
        let constraints = RenderConstraints::Box(BoxConstraints::tight(Size::new(100.0, 50.0)));

        assert!(constraints.is_box());
        assert!(!constraints.is_sliver());
        assert!(constraints.is_tight());
        assert!(constraints.as_box().is_some());
        assert!(constraints.as_sliver().is_none());
    }

    #[test]
    fn test_sliver_constraints() {
        let constraints = RenderConstraints::Sliver(SliverConstraints::default());

        assert!(constraints.is_sliver());
        assert!(!constraints.is_box());
        assert!(!constraints.is_tight()); // Slivers are never tight
        assert!(constraints.as_sliver().is_some());
        assert!(constraints.as_box().is_none());
    }

    #[test]
    fn test_box_geometry() {
        let geometry = RenderGeometry::Box(Size::new(100.0, 50.0));

        assert!(geometry.is_box());
        assert!(!geometry.is_sliver());
        assert_eq!(geometry.size(), Size::new(100.0, 50.0));
        assert_eq!(geometry.as_box(), Some(Size::new(100.0, 50.0)));
        assert!(geometry.as_sliver().is_none());
    }

    #[test]
    fn test_sliver_geometry() {
        let sliver_geom = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 100.0,
            layout_extent: Some(100.0),
            max_paint_extent: Some(500.0),
            ..Default::default()
        };
        let geometry = RenderGeometry::Sliver(sliver_geom);

        assert!(geometry.is_sliver());
        assert!(!geometry.is_box());
        assert!(geometry.as_sliver().is_some());
        assert!(geometry.as_box().is_none());
        // Size is derived from sliver geometry (0.0, paint_extent)
        assert_eq!(geometry.size(), Size::new(0.0, 100.0));
    }

    #[test]
    fn test_from_conversions() {
        let box_constraints: RenderConstraints =
            BoxConstraints::tight(Size::new(10.0, 20.0)).into();
        assert!(box_constraints.is_box());

        let box_geometry: RenderGeometry = Size::new(10.0, 20.0).into();
        assert!(box_geometry.is_box());

        let sliver_constraints: RenderConstraints = SliverConstraints::default().into();
        assert!(sliver_constraints.is_sliver());

        let sliver_geometry: RenderGeometry = SliverGeometry::default().into();
        assert!(sliver_geometry.is_sliver());
    }
}
