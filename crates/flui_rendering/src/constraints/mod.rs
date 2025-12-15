//! Layout constraints for the rendering layer.
//!
//! This module provides constraint types used in the rendering pipeline.
//! Constraints flow down from parent to child during layout, describing
//! the space available for the child.
//!
//! # Constraint Types
//!
//! - [`BoxConstraints`]: 2D rectangular constraints (min/max width/height)
//! - [`SliverConstraints`]: Scrollable viewport constraints
//! - [`SliverGeometry`]: Layout output for slivers
//!
//! # Direction Types
//!
//! - [`GrowthDirection`]: Direction in which content grows in scrollable areas
//!
//! # Scroll Metrics
//!
//! - [`FixedScrollMetrics`]: Scroll metrics with fixed boundaries
//! - [`FixedExtentMetrics`]: Scroll metrics with fixed item extent
//!
//! # Flutter Equivalence
//!
//! - `BoxConstraints` → Flutter's `BoxConstraints`
//! - `SliverConstraints` → Flutter's `SliverConstraints`
//! - `SliverGeometry` → Flutter's `SliverGeometry`
//! - `GrowthDirection` → Flutter's `GrowthDirection`

mod box_constraints;
mod direction;
mod scroll_metrics;
mod sliver_constraints;
mod sliver_geometry;

pub use box_constraints::BoxConstraints;
pub use direction::GrowthDirection;
pub use scroll_metrics::{FixedExtentMetrics, FixedScrollMetrics};
pub use sliver_constraints::SliverConstraints;
pub use sliver_geometry::SliverGeometry;

use std::fmt;

/// Abstract constraints trait following Flutter's protocol.
///
/// This trait defines the contract for all constraint types in FLUI,
/// matching Flutter's abstract `Constraints` class.
///
/// # Flutter Equivalence
///
/// ```dart
/// abstract class Constraints {
///   const Constraints();
///   bool get isTight;
///   bool get isNormalized;
///   bool debugAssertIsValid({bool isAppliedConstraint = false});
/// }
/// ```
///
/// # Required Properties
///
/// ## `is_tight()`
/// Returns whether exactly one size satisfies these constraints.
/// For box constraints, this means `min == max` for both dimensions.
///
/// ## `is_normalized()`
/// Returns whether constraints are in canonical form:
/// - All min values are non-negative
/// - All min values <= max values
/// - No NaN or invalid values
pub trait Constraints: Clone + PartialEq + fmt::Debug + Send + Sync + 'static {
    /// Whether exactly one size satisfies these constraints.
    ///
    /// For `BoxConstraints`, returns true if:
    /// - `min_width == max_width` AND
    /// - `min_height == max_height`
    ///
    /// For `SliverConstraints`, always returns false (slivers don't have tight constraints).
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool get isTight;
    /// ```
    fn is_tight(&self) -> bool;

    /// Whether constraints are in canonical form.
    ///
    /// Returns true if:
    /// - All values are non-negative (>= 0.0)
    /// - All min values <= max values
    /// - No NaN values
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool get isNormalized;
    /// ```
    fn is_normalized(&self) -> bool;

    /// Validates constraints (debug mode only).
    ///
    /// # Parameters
    ///
    /// - `is_applied_constraint`: Whether these constraints are about to be
    ///   applied during a layout call. If true, performs additional validation
    ///   (e.g., ensuring max values are finite).
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool debugAssertIsValid({
    ///   bool isAppliedConstraint = false,
    ///   InformationCollector? informationCollector,
    /// });
    /// ```
    #[cfg(debug_assertions)]
    fn debug_assert_is_valid(&self, is_applied_constraint: bool) -> bool {
        debug_assert!(
            self.is_normalized(),
            "Constraints must be normalized: {:?}",
            self
        );

        if is_applied_constraint {
            // Additional validation for constraints being applied
            // Override in implementation if needed
        }

        true
    }

    /// Validates constraints (no-op in release mode).
    #[cfg(not(debug_assertions))]
    #[inline(always)]
    fn debug_assert_is_valid(&self, _is_applied_constraint: bool) -> bool {
        true
    }
}
