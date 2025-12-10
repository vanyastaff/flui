//! Constraint types for layout
//!
//! This module provides constraint types for box layout, slivers (scrollable areas),
//! scroll metrics, and direction types for scroll and layout.

use std::fmt;

pub mod box_constraints;
pub mod direction;
pub mod scroll_metrics;
pub mod sliver;

pub use box_constraints::BoxConstraints;
pub use direction::{AxisDirection, GrowthDirection, ScrollDirection};
pub use scroll_metrics::{FixedExtentMetrics, FixedScrollMetrics};
pub use sliver::{SliverConstraints, SliverGeometry};

// ============================================================================
// CONSTRAINTS TRAIT (Flutter Protocol)
// ============================================================================

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
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::{BoxConstraints, Constraints};
///
/// let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
/// assert!(tight.is_tight());
/// assert!(tight.is_normalized());
///
/// let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
/// assert!(!loose.is_tight());
/// assert!(loose.is_normalized());
/// ```
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
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    ///
    /// // Validate before applying
    /// constraints.debug_assert_is_valid(true);
    ///
    /// // General validation
    /// constraints.debug_assert_is_valid(false);
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
