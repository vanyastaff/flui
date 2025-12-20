//! Core Protocol trait with GATs and type-level features.
//!
//! The Protocol trait is the foundation of FLUI's type-safe render object system,
//! using Generic Associated Types (GATs), sealed traits, and compile-time validation.

use std::fmt::Debug;
use std::hash::Hash;

use ambassador::delegatable_trait;

// ============================================================================
// SEALED TRAIT
// ============================================================================

/// Private module for sealed trait pattern.
///
/// Prevents external crates from implementing Protocol directly.
pub(crate) mod sealed {
    /// Sealed marker trait preventing external Protocol implementations.
    pub trait Sealed {}
}

// Make Sealed publicly accessible through module path
pub use sealed::Sealed;

// ============================================================================
// PROTOCOL MARKER TRAITS
// ============================================================================

/// Protocols supporting bidirectional layout (both main-axis directions).
pub trait BidirectionalProtocol: Protocol {}

/// Protocols supporting intrinsic dimension queries before layout.
pub trait IntrinsicProtocol: Protocol {
    /// Compute minimum intrinsic main-axis extent for given cross-axis extent.
    fn compute_min_intrinsic_main_axis(&self, cross_axis: f32) -> f32;

    /// Compute maximum intrinsic main-axis extent for given cross-axis extent.
    fn compute_max_intrinsic_main_axis(&self, cross_axis: f32) -> f32;
}

/// Protocols supporting baseline alignment for text and inline content.
pub trait BaselineProtocol: Protocol {
    /// Distance from top edge to baseline, or None if no baseline.
    fn get_distance_to_baseline(&self) -> Option<f32>;
}

// ============================================================================
// CORE PROTOCOL TRAIT
// ============================================================================

/// Type system for a complete layout protocol family.
///
/// Defines all types needed for a layout protocol: constraints from parent,
/// geometry from child, metadata, and protocol-specific contexts.
///
/// # Type Parameters
///
/// - `Object`: Trait object type for render objects (`dyn RenderBox`, etc)
/// - `Constraints`: Layout input (must be hashable for caching)
/// - `ParentData`: Child metadata stored by parent
/// - `Geometry`: Layout output returned to parent
///
/// # Generic Associated Types (GATs)
///
/// - `LayoutContext<'ctx>`: Layout operations with lifetime
/// - `PaintContext<'ctx>`: Painting operations with lifetime
/// - `HitTestContext<'ctx>`: Hit testing with lifetime
///
/// # Example
///
/// ```ignore
/// #[derive(Debug, Clone, Copy, Default)]
/// pub struct MyProtocol;
///
/// impl sealed::Sealed for MyProtocol {}
///
/// impl Protocol for MyProtocol {
///     type Object = dyn MyRenderObject;
///     type Constraints = MyConstraints;
///     type ParentData = MyParentData;
///     type Geometry = MyGeometry;
///
///     type LayoutContext<'ctx> = MyLayoutContext<'ctx> where Self: 'ctx;
///     type PaintContext<'ctx> = MyPaintContext<'ctx> where Self: 'ctx;
///     type HitTestContext<'ctx> = MyHitTestContext<'ctx> where Self: 'ctx;
///
///     fn name() -> &'static str { "my_protocol" }
/// }
/// ```
pub trait Protocol: Send + Sync + Debug + Clone + Copy + Sealed + 'static {
    /// Render object trait for this protocol.
    type Object: ?Sized + Send + Sync;

    /// Layout constraints (must be hashable for cache keys).
    type Constraints: Clone + Debug + Send + Sync + Hash + Eq + 'static;

    /// Per-child metadata stored by parent render object.
    type ParentData: Default + Debug + Send + Sync + 'static;

    /// Layout result geometry returned to parent.
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;

    /// Context for layout operations with protocol-specific types.
    type LayoutContext<'ctx>: LayoutContext<'ctx, Self>
    where
        Self: 'ctx;

    /// Context for painting operations with canvas access.
    type PaintContext<'ctx>: PaintContext<'ctx, Self>
    where
        Self: 'ctx;

    /// Context for hit testing with position queries.
    type HitTestContext<'ctx>: HitTestContext<'ctx, Self>
    where
        Self: 'ctx;

    /// Protocol name for debugging and diagnostics.
    fn name() -> &'static str;

    /// Default geometry for uninitialized state.
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }

    /// Validate constraints before layout (returns true if valid).
    fn validate_constraints(_constraints: &Self::Constraints) -> bool {
        true // Default: accept all
    }

    /// Normalize constraints for consistent cache keys (handles float precision).
    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints // Default: no normalization
    }
}

// ============================================================================
// CONTEXT TRAITS
// ============================================================================

/// Layout context providing access to constraints and child layout operations.
pub trait LayoutContext<'ctx, P: Protocol>: Send + Sync {
    /// Get layout constraints.
    fn constraints(&self) -> &P::Constraints;

    /// Check if layout is complete.
    fn is_complete(&self) -> bool;

    /// Mark layout complete with final geometry.
    fn complete_layout(&mut self, geometry: P::Geometry);

    /// Get number of children.
    fn child_count(&self) -> usize;
}

/// Paint context providing canvas access and transform/clip stack management.
pub trait PaintContext<'ctx, P: Protocol>: Send + Sync {
    /// Get mutable canvas for drawing operations.
    fn canvas(&mut self) -> &mut dyn Canvas;

    /// Push transform onto stack.
    fn push_transform(&mut self, transform: flui_types::Matrix4);

    /// Pop transform from stack.
    fn pop_transform(&mut self);

    /// Push clip rectangle.
    fn push_clip_rect(&mut self, rect: flui_types::Rect);

    /// Pop clip rectangle.
    fn pop_clip(&mut self);
}

/// Hit test context for pointer event handling.
pub trait HitTestContext<'ctx, P: Protocol>: Send + Sync {
    /// Get hit test position in local coordinates.
    fn position(&self) -> flui_types::Offset;

    /// Add hit test result entry.
    fn add_hit(&mut self, target: impl HitTestTarget + 'static);

    /// Check if position is inside bounds.
    fn is_hit(&self, bounds: flui_types::Rect) -> bool;
}

// ============================================================================
// SUPPORTING TRAITS
// ============================================================================

/// Canvas abstraction for drawing operations.
///
/// Implemented by backend-specific canvas types.
pub trait Canvas: Send + Sync {}

/// Hit test result target.
///
/// Implemented by render objects that can handle pointer events.
pub trait HitTestTarget: Send + Sync {}

// ============================================================================
// DELEGATABLE OPERATIONS
// ============================================================================

/// Protocol operations that can be delegated via Ambassador.
#[delegatable_trait]
pub trait DelegateProtocolOps<P: Protocol> {
    /// Get current constraints if available.
    fn get_constraints(&self) -> Option<&P::Constraints>;

    /// Get computed geometry if available.
    fn get_geometry(&self) -> Option<&P::Geometry>;

    /// Get parent data reference.
    fn get_parent_data(&self) -> &P::ParentData;

    /// Get mutable parent data reference.
    fn get_parent_data_mut(&mut self) -> &mut P::ParentData;
}

// ============================================================================
// PROTOCOL REGISTRY & IDS
// ============================================================================

/// Registry for compile-time protocol validation and runtime IDs.
pub trait ProtocolRegistry {
    /// Check if protocol is registered (default: true).
    fn is_registered<P: Protocol>() -> bool {
        true
    }

    /// Get unique protocol ID for runtime dispatch.
    fn protocol_id<P: Protocol>() -> ProtocolId;
}

/// Unique runtime protocol identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolId(u32);

impl ProtocolId {
    /// Create protocol ID from raw value.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get raw ID value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

// ============================================================================
// PROTOCOL COMPATIBILITY
// ============================================================================

/// Trait for checking protocol compatibility (for adapters).
pub trait ProtocolCompatible<Other: Protocol>: Protocol {
    /// Returns true if protocols can be adapted together.
    fn is_compatible() -> bool {
        false // Default: not compatible
    }
}

// ============================================================================
// CONSTRAINED PROTOCOL
// ============================================================================

/// Protocol with compile-time constraint validation and bounds checking.
pub trait ConstrainedProtocol: Protocol {
    /// Minimum valid constraint values.
    fn min_constraints() -> Self::Constraints;

    /// Maximum valid constraint values.
    fn max_constraints() -> Self::Constraints;

    /// Check if constraints are within valid bounds.
    fn check_bounds(constraints: &Self::Constraints) -> bool;
}

// ============================================================================
// ASYNC PROTOCOL (Optional)
// ============================================================================

#[cfg(feature = "async")]
/// Protocol with async layout support for operations requiring async I/O.
pub trait AsyncProtocol: Protocol {
    /// Async-capable layout context.
    type AsyncLayoutContext<'ctx>: AsyncLayoutContext<'ctx, Self>
    where
        Self: 'ctx;

    /// Perform async layout operation.
    async fn layout_async<'ctx>(
        &mut self,
        ctx: Self::AsyncLayoutContext<'ctx>,
    ) -> Result<Self::Geometry, LayoutError>;
}

#[cfg(feature = "async")]
/// Async layout context extending base LayoutContext with future support.
pub trait AsyncLayoutContext<'ctx, P: Protocol>: LayoutContext<'ctx, P> {
    /// Await future during layout (e.g., image loading).
    async fn await_future<F: std::future::Future>(&mut self, future: F) -> F::Output;
}

// ============================================================================
// ERROR TYPES
// ============================================================================

/// Layout operation errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LayoutError {
    /// Invalid constraint values.
    #[error("Invalid constraints: {0}")]
    InvalidConstraints(String),

    /// Layout algorithm exceeded maximum iterations.
    #[error("Layout exceeded maximum iterations: {0}")]
    MaxIterationsExceeded(usize),

    /// Child layout failed.
    #[error("Child layout failed: {0}")]
    ChildLayoutFailed(String),

    /// Protocol type mismatch.
    #[error("Protocol mismatch: expected {expected}, got {actual}")]
    ProtocolMismatch {
        /// Expected protocol name.
        expected: String,
        /// Actual protocol name.
        actual: String,
    },
}

// ============================================================================
// DEFAULT CONTEXT IMPLEMENTATIONS
// ============================================================================

/// Default layout context (stub implementation).
pub struct DefaultLayoutContext<'ctx, P: Protocol> {
    _phantom: std::marker::PhantomData<&'ctx P>,
}

/// Default paint context (stub implementation).
pub struct DefaultPaintContext<'ctx, P: Protocol> {
    _phantom: std::marker::PhantomData<&'ctx P>,
}

/// Default hit test context (stub implementation).
pub struct DefaultHitTestContext<'ctx, P: Protocol> {
    _phantom: std::marker::PhantomData<&'ctx P>,
}

impl<'ctx, P: Protocol> LayoutContext<'ctx, P> for DefaultLayoutContext<'ctx, P> {
    fn constraints(&self) -> &P::Constraints {
        unimplemented!("Override with custom implementation")
    }

    fn is_complete(&self) -> bool {
        false
    }

    fn complete_layout(&mut self, _geometry: P::Geometry) {
        unimplemented!("Override with custom implementation")
    }

    fn child_count(&self) -> usize {
        0
    }
}

impl<'ctx, P: Protocol> PaintContext<'ctx, P> for DefaultPaintContext<'ctx, P> {
    fn canvas(&mut self) -> &mut dyn Canvas {
        unimplemented!("Override with custom implementation")
    }

    fn push_transform(&mut self, _transform: flui_types::Matrix4) {}
    fn pop_transform(&mut self) {}
    fn push_clip_rect(&mut self, _rect: flui_types::Rect) {}
    fn pop_clip(&mut self) {}
}

impl<'ctx, P: Protocol> HitTestContext<'ctx, P> for DefaultHitTestContext<'ctx, P> {
    fn position(&self) -> flui_types::Offset {
        flui_types::Offset::ZERO
    }

    fn add_hit(&mut self, _target: impl HitTestTarget + 'static) {}

    fn is_hit(&self, _bounds: flui_types::Rect) -> bool {
        false
    }
}

// ============================================================================
// PROTOCOL DEFINITION MACRO
// ============================================================================

/// Define a protocol with minimal boilerplate.
///
/// # Example
///
/// ```ignore
/// define_protocol! {
///     /// My custom protocol for grid layout.
///     pub struct GridProtocol {
///         object: dyn RenderGrid,
///         constraints: GridConstraints,
///         parent_data: GridParentData,
///         geometry: GridGeometry,
///         name: "grid",
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_protocol {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            object: $object:ty,
            constraints: $constraints:ty,
            parent_data: $parent_data:ty,
            geometry: $geometry:ty,
            name: $protocol_name:expr,
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default)]
        $vis struct $name;

        impl $crate::protocol::Sealed for $name {}

        impl $crate::protocol::Protocol for $name {
            type Object = $object;
            type Constraints = $constraints;
            type ParentData = $parent_data;
            type Geometry = $geometry;

            type LayoutContext<'ctx> = $crate::protocol::DefaultLayoutContext<'ctx, Self>
            where
                Self: 'ctx;

            type PaintContext<'ctx> = $crate::protocol::DefaultPaintContext<'ctx, Self>
            where
                Self: 'ctx;

            type HitTestContext<'ctx> = $crate::protocol::DefaultHitTestContext<'ctx, Self>
            where
                Self: 'ctx;

            fn name() -> &'static str {
                $protocol_name
            }
        }
    };
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_id() {
        let id1 = ProtocolId::new(1);
        let id2 = ProtocolId::new(1);
        let id3 = ProtocolId::new(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.get(), 1);
    }
}
