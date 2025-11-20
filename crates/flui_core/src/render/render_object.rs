//! Type-erased render object trait.
//!
//! This module provides `RenderObject`, the main interface for render objects
//! stored in `RenderElement`. It enables type erasure over Protocol and Arity
//! while maintaining type safety at the boundary.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<A> → BoxRenderWrapper → Box<dyn RenderObject>
//! SliverRender<A> → SliverRenderWrapper → Box<dyn RenderObject>
//! ```
//!
//! # Type Erasure
//!
//! The `Constraints` and `Geometry` enums provide runtime discrimination
//! between Box and Sliver protocols. The wrappers in `wrappers.rs` convert
//! between typed and type-erased representations.

use crate::element::{ElementId, ElementTree};
use crate::render::protocol::{BoxConstraints, SliverConstraints, SliverGeometry};
use flui_types::{Offset, Size};
use std::fmt::Debug;

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
#[derive(Debug, Clone)]
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

// ============================================================================
// RenderObject Trait
// ============================================================================

/// Type-erased render object trait.
///
/// Stored in `RenderElement`. Provides type erasure over Protocol and Arity
/// so that all render objects can be stored uniformly.
///
/// # Implementation
///
/// Don't implement this trait directly. Instead, implement `RenderBox<A>` or
/// `SliverRender<A>`, which will be automatically wrapped via `BoxRenderWrapper`
/// or `SliverRenderWrapper`.
///
/// # Architecture
///
/// ```text
/// RenderBox<A> / SliverRender<A> → Wrapper → Box<dyn RenderObject>
/// ```
pub trait RenderObject: Send + Sync + Debug {
    /// Computes layout and returns geometry.
    ///
    /// Called during layout phase. The wrapper handles converting between
    /// type-erased and typed constraints/geometry.
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &Constraints,
    ) -> Geometry;

    /// Paints to a canvas.
    ///
    /// Called during paint phase. Returns a canvas with all drawing operations.
    fn paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> flui_painting::Canvas;

    /// Performs hit testing.
    ///
    /// Called during pointer event routing to determine which element was hit.
    fn hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
        geometry: &Geometry,
    ) -> bool;

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Downcasts to mutable concrete type for mutation.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dyn_constraints_box() {
        let constraints = BoxConstraints::default();
        let dyn_constraints = Constraints::from(constraints);

        assert!(dyn_constraints.try_as_box().is_some());
        assert!(dyn_constraints.try_as_sliver().is_none());
    }

    #[test]
    fn test_dyn_constraints_sliver() {
        let constraints = SliverConstraints::default();
        let dyn_constraints = Constraints::from(constraints);

        assert!(dyn_constraints.try_as_sliver().is_some());
        assert!(dyn_constraints.try_as_box().is_none());
    }

    #[test]
    fn test_dyn_geometry_box() {
        let size = Size::new(100.0, 200.0);
        let dyn_geometry = Geometry::from(size);

        assert_eq!(dyn_geometry.try_as_box(), Some(size));
        assert!(dyn_geometry.try_as_sliver().is_none());
    }

    #[test]
    fn test_dyn_geometry_sliver() {
        let geometry = SliverGeometry::default();
        let dyn_geometry = Geometry::from(geometry);

        assert!(dyn_geometry.try_as_sliver().is_some());
        assert!(dyn_geometry.try_as_box().is_none());
    }
}
