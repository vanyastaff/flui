//! A child's parent data belongs to its parent: what the parent's layout writes into it is what
//! the child keeps, and a layout that fails or panics leaves the child with its parent data.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::geometry::{Offset, Size};
use flui_foundation::{RenderId, Variable};
use flui_objects::RenderSliverFixedExtentList;
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints},
    context::BoxLayoutContext,
    parent_data::{BoxParentData, SliverMultiBoxAdaptorParentData},
    pipeline::{PipelineOwner, phase::Layout},
    traits::RenderBox,
};

use crate::common::{BoxedRenderObject, BoxedSliverObject, vertical_constraints};

/// What [`Host`] does after laying out its child.
#[derive(Clone, Copy, Debug)]
enum Then {
    /// Moves the child to `WRITTEN` through its parent data and returns its size.
    Writes,
    /// Returns an error.
    Fails,
    /// Panics.
    Panics,
}

const SEEDED: Offset = Offset::new(2.0, 3.0);
const WRITTEN: Offset = Offset::new(5.0, 7.0);

#[derive(Debug)]
struct Host {
    then: Rc<Cell<Then>>,
}

impl flui_foundation::inspect::Inspect for Host {
    fn kind(&self) -> &'static str {
        "Host"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for Host {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = ctx.constraints().loosen();
        ctx.layout_child(0, constraints)?;
        match self.then.get() {
            Then::Writes => {
                if let Some(data) = ctx.child_parent_data_mut(0) {
                    data.offset = WRITTEN;
                }
                Ok(ctx.constraints().biggest())
            }
            Then::Fails => Err(flui_rendering::error::RenderError::contract_violation(
                "Host",
                "layout fails while armed",
            )),
            Then::Panics => panic!("a layout that panics"),
        }
    }
}

#[derive(Debug)]
struct Leaf;

impl flui_foundation::inspect::Inspect for Leaf {
    fn kind(&self) -> &'static str {
        "Leaf"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for Leaf {
    type Arity = flui_foundation::Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, flui_foundation::Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(ctx.constraints().constrain(Size::new(10.0, 10.0)))
    }
}

fn parent_data<T: flui_rendering::parent_data::ParentData + Clone>(
    owner: &PipelineOwner<Layout>,
    id: RenderId,
) -> Option<T> {
    owner
        .render_tree()
        .get(id)
        .expect("the node exists")
        .parent_data()
        .and_then(|data| data.downcast_ref::<T>())
        .cloned()
}

/// A box parent's write persists, and an error or a panic in its layout leaves the child's
/// parent data where it was, ready for the next pass.
#[test]
fn a_child_keeps_the_parent_data_its_parent_left_it() {
    for (then, expected) in [
        (Then::Writes, WRITTEN),
        (Then::Fails, SEEDED),
        (Then::Panics, SEEDED),
    ] {
        let switch = Rc::new(Cell::new(then));
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let host = owner.insert(Box::new(Host {
            then: Rc::clone(&switch),
        }) as BoxedRenderObject);
        let child = owner
            .render_tree_mut()
            .insert_box_child(host, Box::new(Leaf) as BoxedRenderObject)
            .expect("box child");
        owner
            .render_tree_mut()
            .get_mut(child)
            .expect("just inserted")
            .set_parent_data(Box::new(BoxParentData::new(SEEDED)));
        owner.set_root_id(Some(host));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
        let mut owner = owner.into_layout();

        match catch_unwind(AssertUnwindSafe(|| owner.run_layout())) {
            Ok(_) => {}
            Err(payload) => flui_foundation::panic::discard_payload(payload),
        }

        assert_eq!(
            parent_data::<BoxParentData>(&owner, child).map(|data| data.offset),
            Some(expected),
            "{then:?}: the child's parent data after its parent's layout",
        );

        // The next pass reads what the failed one left, and its write lands.
        switch.set(Then::Writes);
        owner.mark_needs_layout(host);
        owner.run_layout().expect("the next pass succeeds");
        assert_eq!(
            parent_data::<BoxParentData>(&owner, child).map(|data| data.offset),
            Some(WRITTEN),
            "{then:?}: the pass after it",
        );
    }
}

#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
}

impl flui_foundation::inspect::Inspect for SliverHost {
    fn kind(&self) -> &'static str {
        "SliverHost"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for SliverHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let _ = ctx.layout_sliver_child(0, self.constraints)?;
        Ok(ctx.constraints().biggest())
    }
}

/// A lazy list writes each child's `layout_offset` into its parent data during layout; the child
/// keeps it, beside the index the element tree stamped.
#[test]
fn a_sliver_list_child_keeps_the_layout_offset_its_list_wrote() {
    let item_extent = 30.0;
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let host = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0),
    }) as BoxedRenderObject);
    let list = owner
        .render_tree_mut()
        .insert_sliver_child(
            host,
            Box::new(RenderSliverFixedExtentList::new(item_extent, 3)) as BoxedSliverObject,
        )
        .expect("fixed extent sliver");
    let children: Vec<RenderId> = (0..3)
        .map(|index| {
            let child = owner
                .render_tree_mut()
                .insert_box_child(list, Box::new(Leaf) as BoxedRenderObject)
                .expect("box child");
            owner
                .render_tree_mut()
                .get_mut(child)
                .expect("just inserted")
                .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(index)));
            child
        })
        .collect();
    let owner = crate::common::laid_out_tight_300x100(owner, host);

    for (index, &child) in children.iter().enumerate() {
        let data = parent_data::<SliverMultiBoxAdaptorParentData>(&owner, child)
            .expect("the child keeps its parent data");
        assert_eq!(data.index, index, "the stamped index");
        #[expect(clippy::cast_precision_loss, reason = "a test index below 3")]
        let offset = index as f64 * item_extent;
        assert_eq!(data.layout_offset, offset, "child {index}'s layout offset");
    }
}
