//! Unified protocol types for multi-protocol rendering.
//!
//! This module provides unified enums that support both Box and Sliver protocols,
//! allowing RenderTree to work with both protocols through a single API.

use super::protocol::ProtocolId;
use super::{BoxConstraints, SliverConstraints};
use flui_types::{Size, SliverGeometry};

// ============================================================================
// UNIFIED CONSTRAINTS
// ============================================================================

/// Unified constraints that support both Box and Sliver protocols.
///
/// This enum allows RenderTree to work with both layout protocols through
/// a single `layout_element()` method, with protocol dispatch handled internally.
///
/// # Example
///
/// ```rust,ignore
/// // Box layout
/// let constraints = Constraints::Box(BoxConstraints::tight(Size::new(100.0, 100.0)));
/// let geometry = render_tree.layout_element(id, constraints);
///
/// // Sliver layout
/// let constraints = Constraints::Sliver(SliverConstraints::new(0.0, 1000.0));
/// let geometry = render_tree.layout_element(id, constraints);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Constraints {
    /// Box protocol constraints (min/max width/height).
    Box(BoxConstraints),
    /// Sliver protocol constraints (scroll offset, viewport).
    Sliver(SliverConstraints),
}

impl Constraints {
    /// Returns the protocol ID for these constraints.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::{Constraints, ProtocolId, BoxConstraints};
    ///
    /// let constraints = Constraints::Box(BoxConstraints::tight_for(100.0, 100.0));
    /// assert_eq!(constraints.protocol(), ProtocolId::Box);
    /// ```
    #[inline]
    pub fn protocol(&self) -> ProtocolId {
        match self {
            Self::Box(_) => ProtocolId::Box,
            Self::Sliver(_) => ProtocolId::Sliver,
        }
    }

    /// Returns true if these are Box constraints.
    #[inline]
    pub fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Returns true if these are Sliver constraints.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }

    /// Unwraps Box constraints, panicking if this is not a Box protocol.
    #[inline]
    pub fn as_box(&self) -> BoxConstraints {
        match self {
            Self::Box(c) => *c,
            Self::Sliver(_) => panic!("Expected Box constraints, got Sliver"),
        }
    }

    /// Unwraps Sliver constraints, panicking if this is not a Sliver protocol.
    #[inline]
    pub fn as_sliver(&self) -> SliverConstraints {
        match self {
            Self::Sliver(c) => *c,
            Self::Box(_) => panic!("Expected Sliver constraints, got Box"),
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
// UNIFIED GEOMETRY
// ============================================================================

/// Unified geometry that supports both Box and Sliver protocols.
///
/// This enum represents the result of layout operations for both protocols,
/// allowing uniform handling in RenderTree.
///
/// # Example
///
/// ```rust,ignore
/// match geometry {
///     Geometry::Box(size) => {
///         println!("Box layout: {}x{}", size.width, size.height);
///     }
///     Geometry::Sliver(geom) => {
///         println!("Sliver layout: paint_extent={}", geom.paint_extent);
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// Box protocol geometry (width, height).
    Box(Size),
    /// Sliver protocol geometry (paint/scroll/layout extents).
    Sliver(SliverGeometry),
}

impl Geometry {
    /// Returns the protocol ID for this geometry.
    #[inline]
    pub fn protocol(&self) -> ProtocolId {
        match self {
            Self::Box(_) => ProtocolId::Box,
            Self::Sliver(_) => ProtocolId::Sliver,
        }
    }

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

    /// Unwraps Box geometry, panicking if this is not a Box protocol.
    #[inline]
    pub fn as_box(&self) -> Size {
        match self {
            Self::Box(s) => *s,
            Self::Sliver(_) => panic!("Expected Box geometry, got Sliver"),
        }
    }

    /// Unwraps Sliver geometry, panicking if this is not a Sliver protocol.
    #[inline]
    pub fn as_sliver(&self) -> SliverGeometry {
        match self {
            Self::Sliver(g) => *g,
            Self::Box(_) => panic!("Expected Sliver geometry, got Box"),
        }
    }

    /// Returns a default geometry for the given protocol.
    #[inline]
    pub fn zero(protocol: ProtocolId) -> Self {
        match protocol {
            ProtocolId::Box => Self::Box(Size::ZERO),
            ProtocolId::Sliver => Self::Sliver(SliverGeometry::default()),
        }
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self::Box(Size::ZERO)
    }
}

impl From<Size> for Geometry {
    fn from(size: Size) -> Self {
        Self::Box(size)
    }
}

impl From<SliverGeometry> for Geometry {
    fn from(geom: SliverGeometry) -> Self {
        Self::Sliver(geom)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints_protocol() {
        let box_c = Constraints::Box(BoxConstraints::tight_for(100.0, 100.0));
        assert_eq!(box_c.protocol(), ProtocolId::Box);
        assert!(box_c.is_box());
        assert!(!box_c.is_sliver());

        let sliver_c = Constraints::Sliver(SliverConstraints::new(0.0, 1000.0));
        assert_eq!(sliver_c.protocol(), ProtocolId::Sliver);
        assert!(sliver_c.is_sliver());
        assert!(!sliver_c.is_box());
    }

    #[test]
    fn test_geometry_protocol() {
        let box_g = Geometry::Box(Size::new(100.0, 100.0));
        assert_eq!(box_g.protocol(), ProtocolId::Box);
        assert!(box_g.is_box());
        assert!(!box_g.is_sliver());

        let sliver_g = Geometry::Sliver(SliverGeometry::ZERO);
        assert_eq!(sliver_g.protocol(), ProtocolId::Sliver);
        assert!(sliver_g.is_sliver());
        assert!(!sliver_g.is_box());
    }

    #[test]
    fn test_geometry_zero() {
        let box_g = Geometry::zero(ProtocolId::Box);
        assert!(matches!(box_g, Geometry::Box(_)));

        let sliver_g = Geometry::zero(ProtocolId::Sliver);
        assert!(matches!(sliver_g, Geometry::Sliver(_)));
    }

    #[test]
    fn test_from_conversions() {
        let box_c: Constraints = BoxConstraints::tight_for(100.0, 100.0).into();
        assert!(box_c.is_box());

        let sliver_c: Constraints = SliverConstraints::new(0.0, 1000.0).into();
        assert!(sliver_c.is_sliver());

        let box_g: Geometry = Size::new(100.0, 100.0).into();
        assert!(box_g.is_box());

        let sliver_g: Geometry = SliverGeometry::ZERO.into();
        assert!(sliver_g.is_sliver());
    }
}
