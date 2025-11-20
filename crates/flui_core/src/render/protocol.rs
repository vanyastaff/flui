//! Protocol system for unified rendering architecture.
//!
//! The protocol system abstracts over different layout systems:
//! - **Box Protocol**: Standard 2D layout (constraints → size)
//! - **Sliver Protocol**: Scrollable content (sliver constraints → sliver geometry)
//!
//! This enables a single `RenderBox<A>` or `SliverRender<A>` trait to work
//! with both protocols transparently while maintaining type safety.
//!
//! # Architecture
//!
//! ```text
//! Protocol (trait)
//! ├── BoxProtocol    → BoxConstraints → Size
//! └── SliverProtocol → SliverConstraints → SliverGeometry
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! fn layout<P: Protocol>(&mut self, constraints: P::Constraints) -> P::Geometry {
//!     // Generic over protocol
//! }
//! ```

use std::fmt;

// Re-export from flui_types
pub use flui_types::constraints::BoxConstraints;
pub use flui_types::{Size, SliverConstraints, SliverGeometry};

/// Runtime identifier for layout protocols.
///
/// Used for type-erased operations where the protocol type is not known
/// at compile time, such as in `RenderElement` storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutProtocol {
    /// Standard 2D box layout (BoxConstraints → Size).
    Box,
    /// Scrollable/sliver layout (SliverConstraints → SliverGeometry).
    Sliver,
}

impl fmt::Display for LayoutProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Box => write!(f, "Box"),
            Self::Sliver => write!(f, "Sliver"),
        }
    }
}

/// Protocol trait defining constraints and geometry types.
///
/// This trait provides the type-level association between constraint inputs
/// and geometry outputs for a layout protocol. It is sealed to ensure only
/// `BoxProtocol` and `SliverProtocol` can implement it.
///
/// # Sealed Trait
///
/// Users cannot implement this trait directly. Use one of the provided
/// protocol types:
/// - [`BoxProtocol`]: For standard 2D layouts
/// - [`SliverProtocol`]: For scrollable content
pub trait Protocol: sealed::Sealed + Send + Sync + 'static {
    /// Input constraint type for layout computation.
    type Constraints: Clone + fmt::Debug + Default + Send + Sync + 'static;

    /// Output geometry type from layout computation.
    type Geometry: Clone + fmt::Debug + Default + Send + Sync + 'static;

    /// Runtime protocol identifier for type-erased operations.
    const ID: LayoutProtocol;

    /// Human-readable protocol name for debugging.
    const NAME: &'static str;
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::BoxProtocol {}
    impl Sealed for super::SliverProtocol {}
}

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// Box protocol for standard 2D layout.
///
/// The most common protocol, used for regular UI elements that lay out
/// within rectangular bounds. Takes `BoxConstraints` (min/max width/height)
/// and produces a `Size`.
///
/// # Constraint Flow
///
/// ```text
/// Parent passes BoxConstraints → Child computes Size
/// ```
///
/// # Usage
///
/// Used by most render objects: containers, text, images, buttons, etc.
#[derive(Debug, Clone, Copy)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = Size;

    const ID: LayoutProtocol = LayoutProtocol::Box;
    const NAME: &'static str = "Box";
}

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

/// Sliver protocol for scrollable/viewport-aware layout.
///
/// Used for elements within scrollable containers that need viewport awareness.
/// Takes `SliverConstraints` (scroll offset, remaining extent) and produces
/// `SliverGeometry` (scroll/paint/layout extents).
///
/// # Constraint Flow
///
/// ```text
/// Viewport passes SliverConstraints → Sliver computes SliverGeometry
/// ```
///
/// # Usage
///
/// Used by scrollable content: lists, grids, sliver app bars, etc.
#[derive(Debug, Clone, Copy)]
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;

    const ID: LayoutProtocol = LayoutProtocol::Sliver;
    const NAME: &'static str = "Sliver";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_protocol_display() {
        assert_eq!(format!("{}", LayoutProtocol::Box), "Box");
        assert_eq!(format!("{}", LayoutProtocol::Sliver), "Sliver");
    }

    #[test]
    fn test_box_protocol_constants() {
        assert_eq!(BoxProtocol::ID, LayoutProtocol::Box);
        assert_eq!(BoxProtocol::NAME, "Box");
    }

    #[test]
    fn test_sliver_protocol_constants() {
        assert_eq!(SliverProtocol::ID, LayoutProtocol::Sliver);
        assert_eq!(SliverProtocol::NAME, "Sliver");
    }

    #[test]
    fn test_box_constraints_tight() {
        let size = Size::new(100.0, 50.0);
        let constraints = BoxConstraints::tight(size);

        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_box_constraints_loose() {
        let constraints = BoxConstraints::loose(Size::new(f32::INFINITY, f32::INFINITY));

        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, f32::INFINITY);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, f32::INFINITY);
    }

    #[test]
    fn test_sliver_constraints_default() {
        let constraints = SliverConstraints::default();

        assert_eq!(constraints.scroll_offset, 0.0);
        assert_eq!(constraints.remaining_paint_extent, 0.0);
        assert_eq!(constraints.remaining_cache_extent, 0.0);
    }
}
