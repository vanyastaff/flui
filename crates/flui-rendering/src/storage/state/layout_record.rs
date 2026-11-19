//! Where a node keeps the constraints of its last layout and the geometry it
//! produced.
//!
//! Every slot of the render slab is as large as the larger of the two
//! protocols' nodes, so the record each protocol keeps inline is paid by
//! every node. A box record (a `Size` and a `BoxConstraints`) is kept
//! inline. A sliver record is three times that, and slivers are a handful
//! per scrollable, so it lives behind one allocation made on its first
//! write: a sliver that never lays out allocates nothing, and the box
//! nodes that fill the tree stop paying for sliver geometry.

use std::fmt::Debug;

use flui_foundation::geometry::Size;

use crate::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};

/// A protocol's store for the constraints of a node's last layout and the
/// geometry that layout produced. Each half is set and cleared on its own.
pub trait LayoutRecord<G, C>: Default + Clone + Debug {
    /// The geometry of the last layout, if one was recorded.
    fn geometry(&self) -> Option<&G>;
    /// The constraints of the last layout, if they were recorded.
    fn constraints(&self) -> Option<&C>;
    /// Records `geometry`, replacing any earlier one.
    fn set_geometry(&mut self, geometry: G);
    /// Records `constraints`, replacing any earlier ones.
    fn set_constraints(&mut self, constraints: C);
    /// Forgets the geometry.
    fn clear_geometry(&mut self);
    /// Forgets the constraints.
    fn clear_constraints(&mut self);
}

/// The box protocol's record, kept inline.
#[derive(Debug, Clone, Default)]
pub struct BoxLayoutRecord {
    size: Option<Size>,
    constraints: Option<BoxConstraints>,
}

impl LayoutRecord<Size, BoxConstraints> for BoxLayoutRecord {
    #[inline]
    fn geometry(&self) -> Option<&Size> {
        self.size.as_ref()
    }

    #[inline]
    fn constraints(&self) -> Option<&BoxConstraints> {
        self.constraints.as_ref()
    }

    #[inline]
    fn set_geometry(&mut self, geometry: Size) {
        self.size = Some(geometry);
    }

    #[inline]
    fn set_constraints(&mut self, constraints: BoxConstraints) {
        self.constraints = Some(constraints);
    }

    #[inline]
    fn clear_geometry(&mut self) {
        self.size = None;
    }

    #[inline]
    fn clear_constraints(&mut self) {
        self.constraints = None;
    }
}

/// The sliver protocol's record, out of line and allocated on its first
/// write. Clearing keeps the allocation: the next layout writes it again.
#[derive(Debug, Clone, Default)]
pub struct SliverLayoutRecord(Option<Box<SliverLayoutParts>>);

#[derive(Debug, Clone, Default)]
struct SliverLayoutParts {
    geometry: Option<SliverGeometry>,
    constraints: Option<SliverConstraints>,
}

impl SliverLayoutRecord {
    fn parts_mut(&mut self) -> &mut SliverLayoutParts {
        self.0.get_or_insert_with(Box::default)
    }
}

impl LayoutRecord<SliverGeometry, SliverConstraints> for SliverLayoutRecord {
    #[inline]
    fn geometry(&self) -> Option<&SliverGeometry> {
        self.0.as_ref()?.geometry.as_ref()
    }

    #[inline]
    fn constraints(&self) -> Option<&SliverConstraints> {
        self.0.as_ref()?.constraints.as_ref()
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.parts_mut().geometry = Some(geometry);
    }

    fn set_constraints(&mut self, constraints: SliverConstraints) {
        self.parts_mut().constraints = Some(constraints);
    }

    fn clear_geometry(&mut self) {
        if let Some(parts) = &mut self.0 {
            parts.geometry = None;
        }
    }

    fn clear_constraints(&mut self) {
        if let Some(parts) = &mut self.0 {
            parts.constraints = None;
        }
    }
}
