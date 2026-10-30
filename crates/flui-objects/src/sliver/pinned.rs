//! Pinned children of the lazy multi-box slivers.
//!
//! A pinned child is a resident the element tree keeps through a pin
//! (`KeepAliveHandle::pin` in `flui-view`) while it lies outside the cache
//! band. The sliver lays it out after the band, at its estimated place, so the
//! semantics tree keeps it and focus can measure it. It is laid out and placed
//! like any other child, which is what the semantics walk asks for, so paint
//! and hit testing skip it explicitly by slot: nothing else would, since a
//! viewport clip is not there under `Clip::None`. Its extent goes into none of
//! the sliver's geometry, anchor or measured extents.

use std::collections::BTreeSet;
use std::ops::Range;

use flui_foundation::Variable;
use flui_foundation::geometry::Offset;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::context::SliverLayoutContext;
use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;

/// Where one pinned child goes: the constraints it is laid out under, its
/// main-axis layout offset and its paint offset.
pub(super) struct PinnedPlace {
    pub(super) constraints: BoxConstraints,
    pub(super) layout_offset: f64,
    pub(super) paint_offset: Offset,
}

/// The pinned indices a lazy sliver was told about, and the dense slots it
/// laid out for them in its last pass.
#[derive(Clone, Debug, Default)]
pub(super) struct PinnedChildren {
    indices: BTreeSet<usize>,
    slots: Vec<usize>,
}

impl PinnedChildren {
    /// Replace the pinned indices. A change relays out the sliver and
    /// republishes its semantics, since a pinned child joins the tree.
    pub(super) fn set(&mut self, indices: &BTreeSet<usize>) -> RenderUpdateImpact {
        if self.indices == *indices {
            return RenderUpdateImpact::NONE;
        }
        self.indices.clone_from(indices);
        RenderUpdateImpact::LAYOUT | RenderUpdateImpact::SEMANTICS
    }

    /// Lay out and place every pinned index outside `band` that has an
    /// attached child, at the place `place` gives for its logical index. An
    /// index `place` has no place for (past the end of the data) is skipped.
    pub(super) fn lay_out(
        &mut self,
        band: Range<usize>,
        ctx: &mut SliverLayoutContext<'_, Variable, SliverMultiBoxAdaptorParentData>,
        mut place: impl FnMut(usize) -> Option<PinnedPlace>,
    ) -> flui_rendering::RenderResult<()> {
        self.slots.clear();
        if self.indices.is_empty() {
            return Ok(());
        }
        for slot in 0..ctx.child_count() {
            let Some(index) = ctx.child_parent_data(slot).map(|data| data.index) else {
                continue;
            };
            if band.contains(&index) || !self.indices.contains(&index) {
                continue;
            }
            let Some(at) = place(index) else {
                continue;
            };
            ctx.layout_box_child(slot, at.constraints)?;
            ctx.position_child(slot, at.paint_offset);
            if let Some(data) = ctx.child_parent_data_mut(slot) {
                data.layout_offset = at.layout_offset;
            }
            self.slots.push(slot);
        }
        Ok(())
    }

    /// Forget the slots of the last pass: a pass that lays out no pinned
    /// child (an empty or exhausted sliver) leaves nothing to skip.
    pub(super) fn clear_slots(&mut self) {
        self.slots.clear();
    }

    /// Whether the last pass laid out `slot` as a pinned child, which paint
    /// and hit testing skip.
    pub(super) fn is_pinned_slot(&self, slot: usize) -> bool {
        self.slots.contains(&slot)
    }
}
