//! What a layout context reports of a child's size: nothing until this pass lays the child out,
//! then the size its layout returned.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::Variable;
use flui_foundation::geometry::Size;
use flui_rendering::{
    constraints::BoxConstraints, context::BoxLayoutContext, parent_data::BoxParentData,
    pipeline::PipelineOwner, traits::RenderBox,
};

use crate::common::BoxedRenderObject;

/// What [`Host`] saw of each child: before and after laying it out.
type Seen = Vec<(Option<Size>, Option<Size>)>;

#[derive(Debug)]
struct Host {
    seen: Rc<RefCell<Seen>>,
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
        let mut seen = Seen::new();
        // One past the last child: an index out of range reports no size either way.
        for index in 0..=ctx.child_count() {
            let before = ctx.child_geometry(index).copied();
            let side = 10.0 * (1.0 + f64::from(u8::try_from(index).expect("BUG: a test index")));
            let _ = ctx.layout_child(index, BoxConstraints::tight(Size::new(side, side)));
            seen.push((before, ctx.child_geometry(index).copied()));
        }
        *self.seen.borrow_mut() = seen;
        Ok(ctx.constraints().biggest())
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
        Ok(ctx.constraints().biggest())
    }
}

/// Each pass starts with no size for any child, the first pass and a relayout alike.
#[test]
fn a_child_has_a_size_once_this_pass_lays_it_out() {
    let seen = Rc::new(RefCell::new(Seen::new()));
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let host = owner.insert(Box::new(Host {
        seen: Rc::clone(&seen),
    }) as BoxedRenderObject);
    for _ in 0..2 {
        owner
            .render_tree_mut()
            .insert_box_child(host, Box::new(Leaf) as BoxedRenderObject)
            .expect("box child");
    }
    let mut owner = crate::common::laid_out_tight_300x100(owner, host);
    let expected = [
        (None, Some(Size::new(10.0, 10.0))),
        (None, Some(Size::new(20.0, 20.0))),
        (None, None),
    ];
    assert_eq!(*seen.borrow(), expected, "the first pass");

    owner.mark_needs_layout(host);
    owner.run_layout().expect("the relayout succeeds");
    assert_eq!(*seen.borrow(), expected, "a relayout");
}
