//! Type erasure for render objects
//!
//! This module provides type-erased wrappers that hide protocol and arity information,
//! allowing render objects to be stored in trait objects while maintaining type safety
//! at the boundary.
//!
//! # Architecture
//!
//! ```text
//! Render<A> → BoxRenderObjectWrapper<A, R> → Box<dyn DynRenderObject>
//! ```
//!
//! The key insight is that protocol and arity are stored in RenderElement,
//! not duplicated in the wrapper or trait object.

use crate::element::{ElementId, ElementTree};
use crate::render::protocol::{BoxConstraints, SliverConstraints, SliverGeometry};
use flui_types::{Offset, Size};
use std::fmt::Debug;

// ============================================================================
// Type-Erased Constraint Types
// ============================================================================

/// Type-erased constraints (Box or Sliver)
///
/// This enum allows storing constraints without knowing the protocol at compile time.
#[derive(Debug, Clone)]
pub enum DynConstraints {
    /// Box protocol constraints (width/height bounds)
    Box(BoxConstraints),
    /// Sliver protocol constraints (scroll state)
    Sliver(SliverConstraints),
}

impl DynConstraints {
    /// Extract BoxConstraints, panics if not Box variant
    pub fn as_box(&self) -> &BoxConstraints {
        match self {
            Self::Box(c) => c,
            Self::Sliver(_) => panic!("Expected BoxConstraints, got SliverConstraints"),
        }
    }

    /// Extract SliverConstraints, panics if not Sliver variant
    pub fn as_sliver(&self) -> &SliverConstraints {
        match self {
            Self::Sliver(c) => c,
            Self::Box(_) => panic!("Expected SliverConstraints, got BoxConstraints"),
        }
    }

    /// Try to extract BoxConstraints
    pub fn try_as_box(&self) -> Option<&BoxConstraints> {
        match self {
            Self::Box(c) => Some(c),
            _ => None,
        }
    }

    /// Try to extract SliverConstraints
    pub fn try_as_sliver(&self) -> Option<&SliverConstraints> {
        match self {
            Self::Sliver(c) => Some(c),
            _ => None,
        }
    }
}

impl From<BoxConstraints> for DynConstraints {
    fn from(c: BoxConstraints) -> Self {
        Self::Box(c)
    }
}

impl From<SliverConstraints> for DynConstraints {
    fn from(c: SliverConstraints) -> Self {
        Self::Sliver(c)
    }
}

// ============================================================================
// Type-Erased Geometry Types
// ============================================================================

/// Type-erased geometry (Box or Sliver)
///
/// This enum allows storing layout results without knowing the protocol at compile time.
#[derive(Debug, Clone)]
pub enum DynGeometry {
    /// Box protocol geometry (size)
    Box(Size),
    /// Sliver protocol geometry (scroll/paint extents)
    Sliver(SliverGeometry),
}

impl DynGeometry {
    /// Extract Size, panics if not Box variant
    pub fn as_box(&self) -> Size {
        match self {
            Self::Box(s) => *s,
            Self::Sliver(_) => panic!("Expected Size, got SliverGeometry"),
        }
    }

    /// Extract SliverGeometry, panics if not Sliver variant
    pub fn as_sliver(&self) -> &SliverGeometry {
        match self {
            Self::Sliver(g) => g,
            Self::Box(_) => panic!("Expected SliverGeometry, got Size"),
        }
    }

    /// Try to extract Size
    pub fn try_as_box(&self) -> Option<Size> {
        match self {
            Self::Box(s) => Some(*s),
            _ => None,
        }
    }

    /// Try to extract SliverGeometry
    pub fn try_as_sliver(&self) -> Option<&SliverGeometry> {
        match self {
            Self::Sliver(g) => Some(g),
            _ => None,
        }
    }
}

impl From<Size> for DynGeometry {
    fn from(s: Size) -> Self {
        Self::Box(s)
    }
}

impl From<SliverGeometry> for DynGeometry {
    fn from(g: SliverGeometry) -> Self {
        Self::Sliver(g)
    }
}

// ============================================================================
// Type-Erased Hit Test Result
// ============================================================================

/// Type-erased hit test result
///
/// Currently both Box and Sliver use bool, but this allows future extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynHitTestResult {
    /// Box protocol hit test result
    Box(bool),
    /// Sliver protocol hit test result
    Sliver(bool),
}

impl DynHitTestResult {
    /// Check if hit test succeeded (either protocol)
    pub fn is_hit(&self) -> bool {
        match self {
            Self::Box(hit) | Self::Sliver(hit) => *hit,
        }
    }

    /// Extract Box hit result
    pub fn as_box(&self) -> bool {
        match self {
            Self::Box(hit) => *hit,
            Self::Sliver(_) => panic!("Expected Box hit result, got Sliver"),
        }
    }

    /// Extract Sliver hit result
    pub fn as_sliver(&self) -> bool {
        match self {
            Self::Sliver(hit) => *hit,
            Self::Box(_) => panic!("Expected Sliver hit result, got Box"),
        }
    }
}

impl From<bool> for DynHitTestResult {
    fn from(hit: bool) -> Self {
        // Default to Box protocol
        Self::Box(hit)
    }
}

// ============================================================================
// Type-Erased Render Object Trait
// ============================================================================

/// Type-erased render object trait
///
/// This trait allows calling layout/paint/hit_test without knowing the
/// protocol or arity at compile time. The wrapper implementations provide
/// the bridge between typed and type-erased APIs.
///
/// # Key Design
///
/// - Protocol and arity are NOT stored in this trait
/// - They are stored in RenderElement as the single source of truth
/// - Wrappers validate arity in debug builds only (zero cost in release)
pub trait DynRenderObject: Send + Sync + Debug {
    /// Perform layout with type-erased constraints
    ///
    /// # Arguments
    ///
    /// * `tree` - Element tree for accessing children
    /// * `children` - Child element IDs (slice, not typed accessor)
    /// * `constraints` - Type-erased constraints (Box or Sliver)
    ///
    /// # Returns
    ///
    /// Type-erased geometry (Size or SliverGeometry)
    ///
    /// # Panics
    ///
    /// - In debug builds: if children count doesn't match arity
    /// - Always: if constraints protocol doesn't match render object protocol
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry;

    /// Perform painting with type-erased context
    ///
    /// # Arguments
    ///
    /// * `tree` - Element tree for accessing children
    /// * `children` - Child element IDs
    /// * `offset` - Paint offset in parent coordinates
    ///
    /// # Returns
    ///
    /// Canvas with painted content
    fn dyn_paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> flui_painting::Canvas;

    /// Perform hit testing with type-erased context
    ///
    /// # Arguments
    ///
    /// * `tree` - Element tree for accessing children
    /// * `children` - Child element IDs
    /// * `position` - Hit test position in local coordinates
    ///
    /// # Returns
    ///
    /// Type-erased hit test result (bool for both protocols currently)
    fn dyn_hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
    ) -> DynHitTestResult;

    /// Get debug name for this render object
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcast to concrete type (for debugging/introspection)
    fn as_any(&self) -> &dyn std::any::Any;

    /// Downcast to mutable concrete type
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dyn_constraints_box() {
        let constraints = BoxConstraints::default();
        let dyn_constraints = DynConstraints::from(constraints);

        assert!(dyn_constraints.try_as_box().is_some());
        assert!(dyn_constraints.try_as_sliver().is_none());
        assert_eq!(dyn_constraints.as_box(), &constraints);
    }

    #[test]
    fn test_dyn_constraints_sliver() {
        let constraints = SliverConstraints::default();
        let dyn_constraints = DynConstraints::from(constraints);

        assert!(dyn_constraints.try_as_sliver().is_some());
        assert!(dyn_constraints.try_as_box().is_none());
    }

    #[test]
    fn test_dyn_geometry_box() {
        let size = Size::new(100.0, 200.0);
        let dyn_geometry = DynGeometry::from(size);

        assert_eq!(dyn_geometry.try_as_box(), Some(size));
        assert!(dyn_geometry.try_as_sliver().is_none());
        assert_eq!(dyn_geometry.as_box(), size);
    }

    #[test]
    fn test_dyn_geometry_sliver() {
        let geometry = SliverGeometry::default();
        let dyn_geometry = DynGeometry::from(geometry);

        assert!(dyn_geometry.try_as_sliver().is_some());
        assert!(dyn_geometry.try_as_box().is_none());
    }

    #[test]
    fn test_dyn_hit_test_result() {
        let result = DynHitTestResult::Box(true);
        assert!(result.is_hit());
        assert!(result.as_box());

        let result = DynHitTestResult::Sliver(false);
        assert!(!result.is_hit());
        assert!(!result.as_sliver());
    }
}
