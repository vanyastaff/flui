//! A child's parent data belongs to its parent: what the parent's layout writes into it is what
//! the child keeps, on success, on an error and on a panic alike, and only the child's own parent
//! takes it.

use std::cell::{Cell, RefCell};
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

/// How [`Host`]'s layout ends, after it has written its children's parent data.
#[derive(Clone, Copy, Debug)]
enum Then {
    Returns,
    Fails,
    Panics,
}

const SEEDED: Offset = Offset::new(2.0, 3.0);

/// What a [`Host`] writes into slot `slot` of its children, on its `pass`-th layout.
fn written(pass: u8, slot: usize) -> Offset {
    #[expect(clippy::cast_precision_loss, reason = "a test slot below 3")]
    Offset::new(f64::from(pass) * 100.0 + slot as f64, 7.0)
}

/// Lays out each child (a refused one is skipped), records the parent data it found in each slot,
/// writes [`written`] into each slot, then ends as `then` says.
#[derive(Debug, Default)]
struct Host {
    pass: Rc<Cell<u8>>,
    then: Rc<Cell<Option<Then>>>,
    read: Rc<RefCell<Vec<Option<Offset>>>>,
}

impl Host {
    fn shares(&self) -> Self {
        Self {
            pass: Rc::clone(&self.pass),
            then: Rc::clone(&self.then),
            read: Rc::clone(&self.read),
        }
    }
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
        let mut read = Vec::with_capacity(ctx.child_count());
        for slot in 0..ctx.child_count() {
            // A torn list names a child this host does not own; laying it out is refused.
            let _ = ctx.layout_child(slot, constraints);
            read.push(ctx.child_parent_data(slot).map(|data| data.offset));
            if let Some(data) = ctx.child_parent_data_mut(slot) {
                data.offset = written(self.pass.get(), slot);
            }
        }
        *self.read.borrow_mut() = read;
        match self.then.get().unwrap_or(Then::Returns) {
            Then::Returns => Ok(ctx.constraints().biggest()),
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

fn offset_of(owner: &PipelineOwner<Layout>, id: RenderId) -> Option<Offset> {
    parent_data::<BoxParentData>(owner, id).map(|data| data.offset)
}

/// `host` with one [`Leaf`] child seeded at [`SEEDED`], laid out under a tight 100×100 root.
fn host_over_a_leaf(host: Host) -> (PipelineOwner, RenderId, RenderId) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let host = owner.insert(Box::new(host) as BoxedRenderObject);
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
    (owner, host, child)
}

/// What a parent wrote stays with the child however its layout ends, and the next pass reads it.
#[test]
fn a_child_keeps_the_parent_data_its_parent_left_it() {
    for then in [Then::Returns, Then::Fails, Then::Panics] {
        let probe = Host::default();
        probe.then.set(Some(then));
        let (owner, host, child) = host_over_a_leaf(probe.shares());
        let mut owner = owner.into_layout();

        match catch_unwind(AssertUnwindSafe(|| owner.run_layout())) {
            Ok(_) => {}
            Err(payload) => flui_foundation::panic::discard_payload(payload),
        }
        assert_eq!(
            offset_of(&owner, child),
            Some(written(0, 0)),
            "{then:?}: the child keeps what its parent wrote",
        );

        probe.pass.set(1);
        probe.then.set(Some(Then::Returns));
        owner.mark_needs_layout(host);
        owner.run_layout().expect("the next pass succeeds");
        assert_eq!(
            *probe.read.borrow(),
            [Some(written(0, 0))],
            "{then:?}: the next pass reads it"
        );
        assert_eq!(offset_of(&owner, child), Some(written(1, 0)), "{then:?}");
    }
}

/// A child listed twice is lent to its first slot; what the second slot creates never replaces
/// the original.
#[test]
fn a_child_listed_twice_keeps_what_its_first_slot_wrote() {
    let probe = Host::default();
    let (mut owner, host, child) = host_over_a_leaf(probe.shares());
    owner.render_tree_mut().append_unlinked_child(host, child);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");

    assert_eq!(*probe.read.borrow(), [Some(SEEDED), None]);
    assert_eq!(offset_of(&owner, child), Some(written(0, 0)));
}

/// A torn list that names a child another node owns reads a copy: the owner lays the child out
/// with its parent data, and the copy goes nowhere.
#[test]
fn a_torn_list_reads_a_copy_of_a_child_it_does_not_own() {
    let outer = Host::default();
    let inner = Host::default();
    let (mut owner, inner_id, child) = host_over_a_leaf(inner.shares());
    let outer_id = owner.insert(Box::new(outer.shares()) as BoxedRenderObject);
    owner.render_tree_mut().adopt_child(outer_id, inner_id);
    owner
        .render_tree_mut()
        .append_unlinked_child(outer_id, child);
    owner.set_root_id(Some(outer_id));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");

    assert_eq!(
        *inner.read.borrow(),
        [Some(SEEDED)],
        "the owner reads the child's parent data"
    );
    assert_eq!(
        offset_of(&owner, child),
        Some(written(0, 0)),
        "the owner's write lands, not the torn list's"
    );
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
