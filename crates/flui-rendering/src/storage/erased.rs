//! Protocol-erased constraint / geometry enums for the pipeline → entry
//! dispatch seam.
//!
//! **D-block PR-A1b U18 (companion memo D4):** the pipeline operates on
//! `&mut RenderNode` (a protocol-erased enum over `RenderEntry<BoxProtocol>`
//! and `RenderEntry<SliverProtocol>`). `RenderEntry::layout` is generic
//! over `P: Protocol` and takes protocol-typed `ProtocolConstraints<P>` /
//! returns `ProtocolGeometry<P>`. Bridging the two ends requires an erased
//! enum + per-variant dispatch through `RenderNode::layout_erased`.
//!
//! `ErasedConstraints` and `ErasedGeometry` carry the protocol-typed value
//! inside an enum tag; `RenderNode::layout_erased` (added in U18 to
//! `crates/flui-rendering/src/storage/node.rs`) pattern-matches and forwards
//! to the appropriate `RenderEntry<P>::layout` call, returning
//! `Err(RenderError::ProtocolMismatch)` if the constraint variant does not
//! match the node's protocol.
//!
//! Conversions are provided via `From`/`TryFrom` so callers that already
//! hold a protocol-typed value can lift it into the erased enum cheaply
//! (`ErasedConstraints::from(c)` for `c: BoxConstraints`), and pipeline-side
//! recipients can downcast on the happy path. The `TryFrom` impls return
//! the local [`ErasedConstraintsMismatch`] / [`ErasedGeometryMismatch`]
//! error types — narrow value-level signals tied to a specific conversion
//! call site. `RenderNode::layout_erased` (which dispatches the erased
//! constraints to the protocol-typed entry) uses the broader
//! [`RenderError::ProtocolMismatch`](crate::error::RenderError::ProtocolMismatch)
//! variant for its own return type instead — the two error surfaces are
//! intentionally distinct because their callers have different recovery
//! paths (a value-conversion mismatch is typically a caller bug worth
//! `unwrap`-ing in tests, while a pipeline-dispatch mismatch flows through
//! `RenderResult` alongside other render errors).

use crate::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};
use flui_types::Size;

// ============================================================================
// ErasedConstraints
// ============================================================================

/// Protocol-erased constraints enum carrying either a `BoxConstraints` or a
/// `SliverConstraints` payload.
///
/// Pattern-matched by [`RenderNode::layout_leaf_erased`](super::RenderNode::layout_leaf_erased);
/// caller-side construction via `From<BoxConstraints>` / `From<SliverConstraints>`;
/// callee-side downcast via `TryFrom`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ErasedConstraints {
    /// Box-protocol constraints (`min_width`, `max_width`, `min_height`, `max_height`).
    Box(BoxConstraints),
    /// Sliver-protocol constraints (axis-direction-aware scroll layout).
    Sliver(SliverConstraints),
}

impl From<BoxConstraints> for ErasedConstraints {
    #[inline]
    fn from(c: BoxConstraints) -> Self {
        Self::Box(c)
    }
}

impl From<SliverConstraints> for ErasedConstraints {
    #[inline]
    fn from(c: SliverConstraints) -> Self {
        Self::Sliver(c)
    }
}

impl TryFrom<ErasedConstraints> for BoxConstraints {
    type Error = ErasedConstraintsMismatch;

    #[inline]
    fn try_from(value: ErasedConstraints) -> Result<Self, Self::Error> {
        match value {
            ErasedConstraints::Box(c) => Ok(c),
            ErasedConstraints::Sliver(_) => Err(ErasedConstraintsMismatch {
                expected: "Box",
                got: "Sliver",
            }),
        }
    }
}

impl TryFrom<ErasedConstraints> for SliverConstraints {
    type Error = ErasedConstraintsMismatch;

    #[inline]
    fn try_from(value: ErasedConstraints) -> Result<Self, Self::Error> {
        match value {
            ErasedConstraints::Sliver(c) => Ok(c),
            ErasedConstraints::Box(_) => Err(ErasedConstraintsMismatch {
                expected: "Sliver",
                got: "Box",
            }),
        }
    }
}

// ============================================================================
// ErasedGeometry
// ============================================================================

/// Protocol-erased geometry enum carrying either a box `Size` or a
/// `SliverGeometry` payload.
///
/// Returned from [`RenderNode::layout_leaf_erased`](super::RenderNode::layout_leaf_erased);
/// caller-side construction via `From<Size>` / `From<SliverGeometry>`;
/// callee-side downcast via `TryFrom`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ErasedGeometry {
    /// Box-protocol geometry (a 2D size).
    Box(Size),
    /// Sliver-protocol geometry (scroll-extent / paint-extent / etc.).
    Sliver(SliverGeometry),
}

impl From<Size> for ErasedGeometry {
    #[inline]
    fn from(g: Size) -> Self {
        Self::Box(g)
    }
}

impl From<SliverGeometry> for ErasedGeometry {
    #[inline]
    fn from(g: SliverGeometry) -> Self {
        Self::Sliver(g)
    }
}

impl TryFrom<ErasedGeometry> for Size {
    type Error = ErasedGeometryMismatch;

    #[inline]
    fn try_from(value: ErasedGeometry) -> Result<Self, Self::Error> {
        match value {
            ErasedGeometry::Box(g) => Ok(g),
            ErasedGeometry::Sliver(_) => Err(ErasedGeometryMismatch {
                expected: "Box",
                got: "Sliver",
            }),
        }
    }
}

impl TryFrom<ErasedGeometry> for SliverGeometry {
    type Error = ErasedGeometryMismatch;

    #[inline]
    fn try_from(value: ErasedGeometry) -> Result<Self, Self::Error> {
        match value {
            ErasedGeometry::Sliver(g) => Ok(g),
            ErasedGeometry::Box(_) => Err(ErasedGeometryMismatch {
                expected: "Sliver",
                got: "Box",
            }),
        }
    }
}

// ============================================================================
// Mismatch errors
// ============================================================================

/// Returned by `TryFrom<ErasedConstraints>` when the enum variant does not
/// match the requested protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErasedConstraintsMismatch {
    /// Protocol name the caller asked for.
    pub expected: &'static str,
    /// Protocol name actually carried by the erased value.
    pub got: &'static str,
}

impl core::fmt::Display for ErasedConstraintsMismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "ErasedConstraints variant mismatch: expected {}, got {}",
            self.expected, self.got
        )
    }
}

impl std::error::Error for ErasedConstraintsMismatch {}

/// Returned by `TryFrom<ErasedGeometry>` when the enum variant does not
/// match the requested protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErasedGeometryMismatch {
    /// Protocol name the caller asked for.
    pub expected: &'static str,
    /// Protocol name actually carried by the erased value.
    pub got: &'static str,
}

impl core::fmt::Display for ErasedGeometryMismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "ErasedGeometry variant mismatch: expected {}, got {}",
            self.expected, self.got
        )
    }
}

impl std::error::Error for ErasedGeometryMismatch {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erased_constraints_box_roundtrip() {
        let c = BoxConstraints::tight(Size::new(
            flui_types::geometry::px(50.0),
            flui_types::geometry::px(30.0),
        ));
        let erased: ErasedConstraints = c.into();
        let back: BoxConstraints = erased.try_into().expect("box round-trip");
        assert_eq!(back, c);
    }

    #[test]
    fn erased_constraints_box_to_sliver_is_mismatch() {
        let c = BoxConstraints::loose(Size::new(
            flui_types::geometry::px(100.0),
            flui_types::geometry::px(100.0),
        ));
        let erased: ErasedConstraints = c.into();
        let err = SliverConstraints::try_from(erased).expect_err("box→sliver mismatch");
        assert_eq!(err.expected, "Sliver");
        assert_eq!(err.got, "Box");
    }

    #[test]
    fn erased_geometry_size_roundtrip() {
        let g = Size::new(
            flui_types::geometry::px(75.0),
            flui_types::geometry::px(25.0),
        );
        let erased: ErasedGeometry = g.into();
        let back: Size = erased.try_into().expect("size round-trip");
        assert_eq!(back, g);
    }

    #[test]
    fn erased_geometry_size_to_sliver_is_mismatch() {
        let g = Size::new(
            flui_types::geometry::px(10.0),
            flui_types::geometry::px(10.0),
        );
        let erased: ErasedGeometry = g.into();
        let err = SliverGeometry::try_from(erased).expect_err("size→sliver mismatch");
        assert_eq!(err.expected, "Sliver");
        assert_eq!(err.got, "Box");
    }
}
