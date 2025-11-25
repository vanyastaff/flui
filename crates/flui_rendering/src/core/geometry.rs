//! Type-erased geometry and constraints for render objects.
//!
//! These enums provide runtime discrimination between Box and Sliver protocols,
//! enabling type erasure in the render system while maintaining type safety.

use crate::core::protocol::{BoxConstraints, SliverConstraints};
use flui_types::{Size, SliverGeometry};

// ============================================================================
// Type-Erased Constraints
// ============================================================================

/// Type-erased constraints for layout computation.
///
/// Wraps either `BoxConstraints` or `SliverConstraints` for use in
/// type-erased `RenderObject::layout()` calls.
#[derive(Debug, Clone)]
pub enum Constraints {
    /// Standard 2D box constraints.
    Box(BoxConstraints),
    /// Scrollable sliver constraints.
    Sliver(SliverConstraints),
}

impl Constraints {
    /// Returns box constraints, panicking if this is sliver constraints.
    pub fn as_box(&self) -> &BoxConstraints {
        match self {
            Self::Box(c) => c,
            Self::Sliver(_) => panic!("Expected BoxConstraints, got SliverConstraints"),
        }
    }

    /// Returns sliver constraints, panicking if this is box constraints.
    pub fn as_sliver(&self) -> &SliverConstraints {
        match self {
            Self::Sliver(c) => c,
            Self::Box(_) => panic!("Expected SliverConstraints, got BoxConstraints"),
        }
    }

    /// Returns box constraints if this is a box, `None` otherwise.
    pub fn try_as_box(&self) -> Option<&BoxConstraints> {
        match self {
            Self::Box(c) => Some(c),
            _ => None,
        }
    }

    /// Returns sliver constraints if this is a sliver, `None` otherwise.
    pub fn try_as_sliver(&self) -> Option<&SliverConstraints> {
        match self {
            Self::Sliver(c) => Some(c),
            _ => None,
        }
    }
}

impl From<BoxConstraints> for Constraints {
    fn from(c: BoxConstraints) -> Self {
        Self::Box(c)
    }
}

impl From<SliverConstraints> for Constraints {
    fn from(c: SliverConstraints) -> Self {
        Self::Sliver(c)
    }
}

// ============================================================================
// Type-Erased Geometry
// ============================================================================

/// Type-erased geometry from layout computation.
///
/// Wraps either `Size` (box) or `SliverGeometry` (sliver) for use in
/// type-erased `RenderObject::layout()` return values.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// Size for box protocol elements.
    Box(Size),
    /// Sliver geometry for scrollable elements.
    Sliver(SliverGeometry),
}

impl Geometry {
    /// Returns size, panicking if this is sliver geometry.
    pub fn as_box(&self) -> Size {
        match self {
            Self::Box(s) => *s,
            Self::Sliver(_) => panic!("Expected Size, got SliverGeometry"),
        }
    }

    /// Returns sliver geometry, panicking if this is box geometry.
    pub fn as_sliver(&self) -> &SliverGeometry {
        match self {
            Self::Sliver(g) => g,
            Self::Box(_) => panic!("Expected SliverGeometry, got Size"),
        }
    }

    /// Returns size if this is box geometry, `None` otherwise.
    pub fn try_as_box(&self) -> Option<Size> {
        match self {
            Self::Box(s) => Some(*s),
            _ => None,
        }
    }

    /// Returns sliver geometry if this is sliver, `None` otherwise.
    pub fn try_as_sliver(&self) -> Option<&SliverGeometry> {
        match self {
            Self::Sliver(g) => Some(g),
            _ => None,
        }
    }
}

impl From<Size> for Geometry {
    fn from(s: Size) -> Self {
        Self::Box(s)
    }
}

impl From<SliverGeometry> for Geometry {
    fn from(g: SliverGeometry) -> Self {
        Self::Sliver(g)
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self::Box(Size::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints_box() {
        let constraints = BoxConstraints::default();
        let dyn_constraints = Constraints::from(constraints);

        assert!(dyn_constraints.try_as_box().is_some());
        assert!(dyn_constraints.try_as_sliver().is_none());
    }

    #[test]
    fn test_constraints_sliver() {
        let constraints = SliverConstraints::default();
        let dyn_constraints = Constraints::from(constraints);

        assert!(dyn_constraints.try_as_sliver().is_some());
        assert!(dyn_constraints.try_as_box().is_none());
    }

    #[test]
    fn test_geometry_box() {
        let size = Size::new(100.0, 200.0);
        let dyn_geometry = Geometry::from(size);

        assert_eq!(dyn_geometry.try_as_box(), Some(size));
        assert!(dyn_geometry.try_as_sliver().is_none());
    }

    #[test]
    fn test_geometry_sliver() {
        let geometry = SliverGeometry::default();
        let dyn_geometry = Geometry::from(geometry);

        assert!(dyn_geometry.try_as_sliver().is_some());
        assert!(dyn_geometry.try_as_box().is_none());
    }
}
