//! Layout protocol definitions for the FLUI rendering system.
//!
//! This module defines the core protocol abstraction that enables different
//! layout algorithms to work within the unified rendering framework. Protocols
//! define the constraint and geometry types used for layout operations.
//!
//! # Design Philosophy
//!
//! - **Compile-time type safety**: Protocol types determined at compile time via generics
//! - **Zero-cost abstractions**: All protocol operations compile to direct type usage
//! - **Sealed trait**: Only Box and Sliver protocols allowed (prevents invalid extensions)
//! - **Extensibility**: New protocols can be added by framework maintainers
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

use std::fmt::Debug;

use flui_types::{BoxConstraints, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// SEALED TRAIT (Prevents external protocol implementations)
// ============================================================================

mod sealed {
    /// Sealed trait to prevent external protocol implementations.
    ///
    /// Only `BoxProtocol` and `SliverProtocol` can implement `Protocol`.
    /// This ensures type safety and prevents invalid protocol combinations.
    pub trait Sealed {}

    impl Sealed for super::BoxProtocol {}
    impl Sealed for super::SliverProtocol {}
}

// ============================================================================
// CORE PROTOCOL TRAIT
// ============================================================================

/// Core trait defining a layout protocol.
///
/// A protocol defines the types and semantics for a particular approach to
/// layout computation. Different protocols enable different layout strategies
/// while maintaining compile-time type safety and zero-cost abstractions.
///
/// # Sealed Trait
///
/// This trait is sealed and can only be implemented by framework-defined protocols
/// (`BoxProtocol` and `SliverProtocol`). This prevents invalid external implementations
/// that could break type safety invariants.
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
pub trait Protocol: sealed::Sealed + 'static + Copy + Debug + Send + Sync {
    /// The constraint type for this protocol.
    ///
    /// Constraints define the input parameters for layout operations,
    /// such as available space, size limits, or viewport information.
    ///
    /// # Examples
    ///
    /// - `BoxProtocol::Constraints` = `BoxConstraints` (min/max width/height)
    /// - `SliverProtocol::Constraints` = `SliverConstraints` (scroll offset, viewport)
    type Constraints: Debug + Clone + Send + Sync + 'static;

    /// The geometry type for this protocol.
    ///
    /// Geometry represents the output results from layout operations,
    /// such as computed sizes, positioning data, or scroll extents.
    ///
    /// # Examples
    ///
    /// - `BoxProtocol::Geometry` = `Size` (width, height)
    /// - `SliverProtocol::Geometry` = `SliverGeometry` (paint/scroll/layout extents)
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
/// # Type Mappings
///
/// - `Constraints` = `BoxConstraints` (min/max width/height)
/// - `Geometry` = `Size` (computed width/height)
///
/// # Characteristics
///
/// - **2D layout**: Handles both width and height simultaneously
/// - **Bounded constraints**: Specifies minimum and maximum allowed sizes
/// - **Simple geometry**: Returns a single computed size
/// - **Performance**: Highly optimized for common layout scenarios
/// - **Zero-cost**: Direct `Size` type, no enum overhead
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = Size;
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
/// # Type Mappings
///
/// - `Constraints` = `SliverConstraints` (scroll offset, viewport, cross-axis)
/// - `Geometry` = `SliverGeometry` (paint/scroll/layout extents)
///
/// # Characteristics
///
/// - **Infinite dimension**: One axis can extend infinitely (scroll direction)
/// - **Viewport-based**: Constraints include scroll position and viewport size
/// - **Scroll geometry**: Returns paint extent, scroll extent, and layout extent
/// - **Lazy layout**: Supports efficient virtualization of large content
/// - **Zero-cost**: Direct `SliverGeometry` type, no enum overhead
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
/// a constraint or geometry belongs to at runtime.
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

// ============================================================================
// PROTOCOL ID (Runtime identification)
// ============================================================================

/// Runtime identifier for layout protocols.
///
/// This enum allows runtime identification of which protocol a constraint or
/// geometry belongs to. With typed `RenderState<P>`, this is rarely needed,
/// but useful for debugging and ViewMode-based dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolId {
    /// Box protocol identifier.
    Box,
    /// Sliver protocol identifier.
    Sliver,
}

impl ProtocolId {
    /// Returns the human-readable name of the protocol.
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Sliver => "Sliver",
        }
    }

    /// Returns whether this protocol supports 2D layout.
    #[inline]
    pub const fn supports_2d_layout(self) -> bool {
        matches!(self, Self::Box)
    }

    /// Returns whether this protocol supports infinite dimensions.
    #[inline]
    pub const fn supports_infinite_dimensions(self) -> bool {
        matches!(self, Self::Sliver)
    }

    /// Returns whether this protocol supports scrolling.
    #[inline]
    pub const fn supports_scrolling(self) -> bool {
        matches!(self, Self::Sliver)
    }
}

/// Alias for protocol runtime identification.
pub type LayoutProtocol = ProtocolId;

// ============================================================================
// PROTOCOL CAST TRAIT (Safe Protocol Conversion)
// ============================================================================

/// Trait for safe protocol state access without unsafe pointer casts.
///
/// This trait provides compile-time checked access to protocol-specific state.
/// Instead of using unsafe TypeId checks and pointer casts, we use this sealed
/// trait to enable type-safe downcasting.
///
/// # Safety
///
/// This trait is sealed and only implemented for `BoxProtocol` and `SliverProtocol`.
/// The implementations guarantee that the cast is valid at compile time.
///
/// # Example
///
/// ```rust,ignore
/// fn get_box_state<P: Protocol>(state: &RenderState<P>) -> Option<&BoxRenderState> {
///     P::as_box_state(state)
/// }
/// ```
pub trait ProtocolCast: Protocol {
    /// Returns true if this protocol is BoxProtocol.
    fn is_box() -> bool;

    /// Returns true if this protocol is SliverProtocol.
    fn is_sliver() -> bool;

    /// Returns the protocol ID for this protocol.
    fn id() -> ProtocolId;
}

impl ProtocolCast for BoxProtocol {
    #[inline]
    fn is_box() -> bool {
        true
    }

    #[inline]
    fn is_sliver() -> bool {
        false
    }

    #[inline]
    fn id() -> ProtocolId {
        ProtocolId::Box
    }
}

impl ProtocolCast for SliverProtocol {
    #[inline]
    fn is_box() -> bool {
        false
    }

    #[inline]
    fn is_sliver() -> bool {
        true
    }

    #[inline]
    fn id() -> ProtocolId {
        ProtocolId::Sliver
    }
}

// ============================================================================
// COMPILE-TIME PROTOCOL MATCHING
// ============================================================================

/// Marker trait for protocols that are BoxProtocol.
///
/// This enables compile-time checked access to box state without unsafe.
pub trait IsBoxProtocol: Protocol {}
impl IsBoxProtocol for BoxProtocol {}

/// Marker trait for protocols that are SliverProtocol.
///
/// This enables compile-time checked access to sliver state without unsafe.
pub trait IsSliverProtocol: Protocol {}
impl IsSliverProtocol for SliverProtocol {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_types() {
        fn assert_protocol<P: Protocol>()
        where
            P::Constraints: Debug + Clone + Send + Sync + 'static,
            P::Geometry: Debug + Clone + Send + Sync + 'static,
        {
        }

        assert_protocol::<BoxProtocol>();
        assert_protocol::<SliverProtocol>();
    }

    #[test]
    fn test_protocol_copy() {
        let box1 = BoxProtocol;
        let box2 = box1;
        let _box3 = box1;
        assert_eq!(box1, box2);

        let sliver1 = SliverProtocol;
        let sliver2 = sliver1;
        let _sliver3 = sliver1;
        assert_eq!(sliver1, sliver2);
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
    fn test_protocol_size() {
        use std::mem::size_of;

        assert_eq!(size_of::<BoxProtocol>(), 0);
        assert_eq!(size_of::<SliverProtocol>(), 0);
    }
}
