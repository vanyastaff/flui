//! Layout constraints system for FLUI rendering pipeline.
//!
//! This module provides constraint types that follow Flutter's proven constraint model
//! while leveraging Rust's type system and performance characteristics.
//!
//! # Core Concepts
//!
//! ## Constraint Types
//!
//! - [`BoxConstraints`] - Rectangular constraints for box-based layout (min/max width/height)
//! - [`SliverConstraints`] - Viewport-aware constraints for scrollable content
//! - [`SliverGeometry`] - Layout results describing space occupied by slivers
//!
//! ## Direction Types
//!
//! - [`GrowthDirection`] - Content growth direction in scrollable areas
//!
//! ## Scroll State
//!
//! - [`FixedScrollMetrics`] - Scroll position and bounds tracking
//! - [`FixedExtentMetrics`] - Scroll metrics with uniform item sizing
//!
//! # Performance Features
//!
//! All constraint types are optimized for performance-critical layout operations:
//!
//! - **Hash + Eq** - Enable efficient constraint-based caching
//! - **Normalization** - Consistent floating-point comparison for cache keys
//! - **Copy semantics** - Zero-cost constraint passing
//! - **Const constructors** - Compile-time initialization where possible
//!
//! ## Layout Caching
//!
//! Constraints can be used as cache keys to avoid redundant layout calculations:
//!
//! ```ignore
//! use std::collections::HashMap;
//! use flui_rendering::constraints::BoxConstraints;
//!
//! let mut cache = HashMap::new();
//!
//! // Normalize constraints for stable cache keys
//! let key = constraints.normalize();
//! cache.insert(key, computed_size);
//! ```
//!
//! ## Batch Operations
//!
//! BoxConstraints supports SIMD-accelerated batch operations when the `simd` feature is enabled:
//!
//! ```ignore
//! #[cfg(feature = "simd")]
//! {
//!     let sizes = vec![/* many sizes */];
//!     let constrained = constraints.batch_constrain(&sizes);
//! }
//! ```
//!
//! # Builder Pattern
//!
//! All constraint types provide fluent builder APIs:
//!
//! ```ignore
//! let constraints = BoxConstraints::UNCONSTRAINED
//!     .with_min_width(10.0)
//!     .with_max_width(100.0)
//!     .with_tight_height(50.0);
//! ```
//!
//! # Flutter Equivalence
//!
//! Types map directly to Flutter's constraint system:
//!
//! - `BoxConstraints` ↔ Flutter `BoxConstraints`
//! - `SliverConstraints` ↔ Flutter `SliverConstraints`
//! - `SliverGeometry` ↔ Flutter `SliverGeometry`
//! - `GrowthDirection` ↔ Flutter `GrowthDirection`

mod box_constraints;
mod direction;
mod scroll_metrics;
mod sliver_constraints;
mod sliver_geometry;

pub use box_constraints::BoxConstraints;
pub use direction::GrowthDirection;
pub use scroll_metrics::{FixedExtentMetrics, FixedScrollMetrics, ScrollMetrics};
pub use sliver_constraints::SliverConstraints;
pub use sliver_geometry::SliverGeometry;

use std::fmt;

/// Abstract constraint trait following Flutter's protocol.
///
/// Defines the contract for all constraint types in FLUI, matching Flutter's
/// abstract `Constraints` class.
pub trait Constraints: Clone + PartialEq + fmt::Debug + Send + Sync + 'static {
    /// Returns whether exactly one size satisfies these constraints.
    ///
    /// Tight constraints force a specific size and leave no flexibility
    /// for the child to choose its own dimensions.
    fn is_tight(&self) -> bool;

    /// Returns whether constraints are in canonical/valid form.
    ///
    /// Normalized constraints have non-negative values and proper ordering
    /// (min <= max). Invalid constraints violate layout invariants.
    fn is_normalized(&self) -> bool;

    /// Validates constraint invariants in debug builds.
    ///
    /// # Parameters
    ///
    /// - `is_applied_constraint`: Whether these constraints are being passed
    ///   to a child during layout (enables stricter validation)
    #[cfg(debug_assertions)]
    fn debug_assert_is_valid(&self, is_applied_constraint: bool) -> bool {
        debug_assert!(
            self.is_normalized(),
            "Constraints must be normalized: {:?}",
            self
        );

        if is_applied_constraint {
            // Additional validation for applied constraints
        }

        true
    }

    /// No-op in release builds for zero overhead.
    #[cfg(not(debug_assertions))]
    #[inline]
    fn debug_assert_is_valid(&self, _is_applied_constraint: bool) -> bool {
        true
    }
}

/// Convenience prelude for common imports.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::constraints::prelude::*;
///
/// let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
/// let normalized = constraints.normalize();
/// ```
pub mod prelude {
    pub use super::{
        BoxConstraints, Constraints, FixedExtentMetrics, FixedScrollMetrics, GrowthDirection,
        ScrollMetrics, SliverConstraints, SliverGeometry,
    };
}
