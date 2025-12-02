//! Layout protocol definitions for the FLUI rendering system.
//!
//! This module defines the core protocol abstraction that enables different
//! layout algorithms to work within the unified rendering framework. Protocols
//! define the constraint and geometry types used for layout operations.
//!
//! # Design Philosophy
//!
//! - **Protocol abstraction**: Clean separation between different layout approaches
//! - **Type safety**: Each protocol has well-defined constraint and geometry types
//! - **Extensibility**: New protocols can be added without changing existing code
//! - **Zero-cost**: All protocol operations compile to direct type usage
//!
//! # Core Protocols
//!
//! ## Box Protocol
//!
//! The box protocol is used for standard 2D rectangular layouts. It uses
//! `BoxConstraints` to specify allowed sizes and returns `Size` as geometry.
//!
//! ## Sliver Protocol
//!
//! The sliver protocol is used for scrollable content with potentially infinite
//! dimensions. It uses `SliverConstraints` for viewport information and returns
//! `SliverGeometry` with scroll extent data.
//!
//! # Usage Examples
//!
//! ## Generic Protocol Functions
//!
//! ```rust,ignore
//! use flui_rendering::core::{Protocol, BoxProtocol, SliverProtocol};
//!
//! fn layout_with_protocol<P: Protocol>(
//!     constraints: P::Constraints,
//!     layout_fn: impl Fn(P::Constraints) -> P::Geometry,
//! ) -> P::Geometry {
//!     layout_fn(constraints)
//! }
//!
//! // Use with box protocol
//! let box_result = layout_with_protocol::<BoxProtocol>(
//!     BoxConstraints::tight(Size::new(100.0, 50.0)),
//!     |constraints| constraints.biggest(),
//! );
//!
//! // Use with sliver protocol
//! let sliver_result = layout_with_protocol::<SliverProtocol>(
//!     SliverConstraints::default(),
//!     |constraints| SliverGeometry::zero(),
//! );
//! ```
//!
//! ## Protocol-Aware Render Objects
//!
//! ```rust,ignore
//! use flui_rendering::core::{Protocol, RenderBox, LayoutContext};
//!
//! impl<P: Protocol> RenderBox<P> for GenericRenderObject
//! where
//!     P::Constraints: Clone,
//! {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Leaf, P>) -> RenderResult<P::Geometry> {
//!         // Protocol-agnostic layout logic
//!         Ok(P::Geometry::default())
//!     }
//! }
//! ```
//!
//! # Protocol Properties
//!
//! Each protocol defines:
//! - **Constraints**: Input parameters for layout (size limits, viewport info, etc.)
//! - **Geometry**: Output results from layout (computed sizes, scroll extents, etc.)
//! - **Semantic meaning**: What the constraints represent and how geometry is interpreted

use std::fmt::Debug;

use flui_types::{Size, SliverConstraints, SliverGeometry};

use super::geometry::BoxConstraints;

// ============================================================================
// CORE PROTOCOL TRAIT
// ============================================================================

/// Core trait defining a layout protocol.
///
/// A protocol defines the types and semantics for a particular approach to
/// layout computation. Different protocols enable different layout strategies
/// while maintaining type safety and performance.
///
/// # Type Parameters
///
/// ## Constraints
///
/// The constraints type represents the input parameters for layout operations.
/// This typically includes size limits, available space, or viewport information.
///
/// Requirements:
/// - `Debug` for debugging and development tools
/// - `Clone` for passing constraints to multiple children
/// - `Send + Sync` for thread safety
/// - `'static` for storage in trait objects
///
/// ## Geometry
///
/// The geometry type represents the output results from layout operations.
/// This typically includes computed sizes, positioning information, or scroll extents.
///
/// Requirements:
/// - `Debug` for debugging and development tools
/// - `Clone` for caching and manipulation
/// - `Send + Sync` for thread safety
/// - `'static` for storage in trait objects
///
/// # Protocol Semantics
///
/// Each protocol implementation defines:
/// - How constraints should be interpreted
/// - What geometry represents
/// - Valid constraint-to-geometry transformations
/// - Performance characteristics and optimization opportunities
///
/// # Thread Safety
///
/// All protocol types must be `Send + Sync` to enable:
/// - Parallel layout computation
/// - Cross-thread constraint passing
/// - Background geometry caching
///
/// # Example Implementation
///
/// ```rust,ignore
/// struct CustomProtocol;
///
/// impl Protocol for CustomProtocol {
///     type Constraints = CustomConstraints;
///     type Geometry = CustomGeometry;
/// }
///
/// #[derive(Debug, Clone)]
/// struct CustomConstraints {
///     available_space: f32,
///     priority: i32,
/// }
///
/// #[derive(Debug, Clone)]
/// struct CustomGeometry {
///     allocated_space: f32,
///     overflow: f32,
/// }
/// ```
pub trait Protocol: 'static {
    /// The constraint type for this protocol.
    ///
    /// Constraints define the input parameters for layout operations,
    /// such as available space, size limits, or viewport information.
    type Constraints: Debug + Clone + Send + Sync + 'static;

    /// The geometry type for this protocol.
    ///
    /// Geometry represents the output results from layout operations,
    /// such as computed sizes, positioning data, or scroll extents.
    type Geometry: Debug + Clone + Send + Sync + 'static;
}

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// Box layout protocol for 2D rectangular layouts.
///
/// The box protocol is the standard layout approach for most UI elements.
/// It uses rectangular constraints to specify allowed sizes and returns
/// computed sizes as geometry.
///
/// # Characteristics
///
/// - **2D layout**: Handles both width and height simultaneously
/// - **Bounded constraints**: Specifies minimum and maximum allowed sizes
/// - **Simple geometry**: Returns a single computed size
/// - **Performance**: Highly optimized for common layout scenarios
///
/// # Use Cases
///
/// - Standard UI elements (buttons, text, images)
/// - Container layouts (padding, margins, alignment)
/// - Flex layouts (rows, columns, wrapping)
/// - Grid layouts (fixed and flexible)
/// - Most traditional UI layout scenarios
///
/// # Constraint Semantics
///
/// Box constraints specify:
/// - Minimum width and height the element must occupy
/// - Maximum width and height the element can occupy
/// - Elements must return a size within these bounds
///
/// # Geometry Semantics
///
/// Box geometry represents:
/// - The computed width and height of the element
/// - Must satisfy the input constraints
/// - Used for positioning children and hit testing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = Size;

    // TODO: maybe add here types context ?
}

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

/// Sliver layout protocol for scrollable content with infinite dimensions.
///
/// The sliver protocol is designed for scrollable content where one dimension
/// can be infinite. It uses viewport-based constraints and returns scroll
/// extent information as geometry.
///
/// # Characteristics
///
/// - **Infinite dimension**: One axis can extend infinitely (scroll direction)
/// - **Viewport-based**: Constraints include scroll position and viewport size
/// - **Scroll geometry**: Returns paint extent, scroll extent, and layout extent
/// - **Lazy layout**: Supports efficient virtualization of large content
///
/// # Use Cases
///
/// - Scrollable lists (ListView, GridView)
/// - Infinite scrolling content
/// - Virtualized layouts for large datasets
/// - Custom scrollable widgets
/// - Nested scrolling scenarios
///
/// # Constraint Semantics
///
/// Sliver constraints specify:
/// - Scroll offset (how far the content has been scrolled)
/// - Remaining paint extent (visible viewport size)
/// - Cross-axis extent (width for vertical scrolling)
/// - Growth direction and scroll direction
///
/// # Geometry Semantics
///
/// Sliver geometry represents:
/// - Paint extent: How much space the sliver occupies in the viewport
/// - Layout extent: How much space the sliver takes up in the scroll view
/// - Scroll extent: How much the sliver contributes to total scrollable area
/// - Max paint extent: Maximum space the sliver could occupy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
}

// ============================================================================
// PROTOCOL UTILITIES
// ============================================================================

/// Trait for types that can identify their protocol at runtime.
///
/// This enables protocol-agnostic code to determine which protocol
/// a constraint or geometry belongs to.
pub trait ProtocolIdentifier {
    /// Returns the protocol identifier for this type.
    fn protocol_id() -> ProtocolId;
}

impl ProtocolIdentifier for BoxConstraints {
    fn protocol_id() -> ProtocolId {
        ProtocolId::Box
    }
}

impl ProtocolIdentifier for Size {
    fn protocol_id() -> ProtocolId {
        ProtocolId::Box
    }
}

impl ProtocolIdentifier for SliverConstraints {
    fn protocol_id() -> ProtocolId {
        ProtocolId::Sliver
    }
}

impl ProtocolIdentifier for SliverGeometry {
    fn protocol_id() -> ProtocolId {
        ProtocolId::Sliver
    }
}

/// Runtime identifier for layout protocols.
///
/// This enum allows runtime identification of which protocol
/// a constraint or geometry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolId {
    /// Box protocol identifier.
    Box,
    /// Sliver protocol identifier.
    Sliver,
}

impl ProtocolId {
    /// Returns the human-readable name of the protocol.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Sliver => "Sliver",
        }
    }

    /// Returns whether this protocol supports 2D layout.
    pub const fn supports_2d_layout(self) -> bool {
        match self {
            Self::Box => true,
            Self::Sliver => false, // Slivers have one infinite dimension
        }
    }

    /// Returns whether this protocol supports infinite dimensions.
    pub const fn supports_infinite_dimensions(self) -> bool {
        match self {
            Self::Box => false,
            Self::Sliver => true,
        }
    }

    /// Returns whether this protocol supports scrolling.
    pub const fn supports_scrolling(self) -> bool {
        match self {
            Self::Box => false,
            Self::Sliver => true,
        }
    }
}

// ============================================================================
// LAYOUT PROTOCOL ENUM (Runtime protocol identifier)
// ============================================================================

/// Runtime identifier for layout protocols.
///
/// This enum is used for runtime protocol identification when type erasure
/// is needed (e.g., in RenderObjectWrapper, RenderState).
///
/// For compile-time protocol usage, use the `Protocol` trait with type
/// parameters (e.g., `BoxProtocol`, `SliverProtocol`).
pub type LayoutProtocol = ProtocolId;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_types() {
        // Test that protocol types implement required traits
        fn assert_protocol<P: Protocol>()
        where
            P::Constraints: Debug + Clone + Send + Sync + 'static,
            P::Geometry: Debug + Clone + Send + Sync + 'static,
        {
            // This function just needs to compile to verify trait bounds
        }

        assert_protocol::<BoxProtocol>();
        assert_protocol::<SliverProtocol>();
    }

    #[test]
    fn test_protocol_identifiers() {
        assert_eq!(BoxConstraints::protocol_id(), ProtocolId::Box);
        assert_eq!(Size::protocol_id(), ProtocolId::Box);
        assert_eq!(SliverConstraints::protocol_id(), ProtocolId::Sliver);
        assert_eq!(SliverGeometry::protocol_id(), ProtocolId::Sliver);
    }

    #[test]
    fn test_protocol_id_properties() {
        assert_eq!(ProtocolId::Box.name(), "Box");
        assert_eq!(ProtocolId::Sliver.name(), "Sliver");

        assert!(ProtocolId::Box.supports_2d_layout());
        assert!(!ProtocolId::Sliver.supports_2d_layout());

        assert!(!ProtocolId::Box.supports_infinite_dimensions());
        assert!(ProtocolId::Sliver.supports_infinite_dimensions());

        assert!(!ProtocolId::Box.supports_scrolling());
        assert!(ProtocolId::Sliver.supports_scrolling());
    }

    #[test]
    fn test_protocol_zero_cost() {
        // Test that protocols compile to zero-cost abstractions
        fn use_box_protocol() -> Size {
            let constraints = <BoxProtocol as Protocol>::Constraints::tight(Size::new(100.0, 50.0));
            constraints.biggest()
        }

        fn use_sliver_protocol() -> SliverGeometry {
            let _constraints = <SliverProtocol as Protocol>::Constraints::default();
            SliverGeometry::zero()
        }

        let size = use_box_protocol();
        assert_eq!(size, Size::new(100.0, 50.0));

        let geometry = use_sliver_protocol();
        assert_eq!(geometry.paint_extent, 0.0);
    }

    #[test]
    fn test_protocol_equality() {
        let box1 = BoxProtocol;
        let box2 = BoxProtocol;
        assert_eq!(box1, box2);

        let sliver1 = SliverProtocol;
        let sliver2 = SliverProtocol;
        assert_eq!(sliver1, sliver2);
    }

    #[test]
    fn test_generic_protocol_usage() {
        fn layout_with_protocol<P: Protocol>(constraints: P::Constraints) -> P::Constraints {
            // Just return constraints to test generic usage
            constraints
        }

        let box_constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let result = layout_with_protocol::<BoxProtocol>(box_constraints);
        assert_eq!(result, box_constraints);

        let sliver_constraints = SliverConstraints::default();
        let result = layout_with_protocol::<SliverProtocol>(sliver_constraints);
        assert_eq!(result, sliver_constraints);
    }
}
