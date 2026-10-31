//! Regression: a multi-child render parent (`Row`) keeps its render children in
//! element-slot order even when a child is a COMPONENT element (a
//! `StatelessView`) whose render descendant is built in a *later* `build_scope`
//! iteration than a render sibling that already attached.
//!
//! This generalizes the `Expanded`/`Positioned` parent-data coverage: the
//! slot-correct ordering fix (`reorder_render_children_after_build`) is keyed on
//! "component child of a multi-child render", not on parent-data specifically.
//! Without the fix the directly-rendered `SizedBox` (slot 1) would attach first
//! and land at render-index 0, inverting the layout.

use crate::common::{LaidOut, lay_out, offset, size, tight};
use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_interaction::gesture::PanClaim;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::prelude::{BuildContext, IntoView, StatelessView};
use flui_widgets::row;
use flui_widgets::{Column, ListView, PanClaimRegion, Row, SizedBox, Stack, Wrap};

/// A minimal `StatelessView` (component element — no render object of its own)
/// that builds to a fixed-size box one level down.
#[derive(Clone, Debug, StatelessView)]
struct BoxBuilder {
    width: f64,
    height: f64,
}

impl StatelessView for BoxBuilder {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(self.width, self.height)
    }
}

pub(crate) fn component_child_keeps_slot_order_before_a_render_sibling() {
    // Row 300 wide: a component child FIRST (builds its box in a later
    // iteration), a directly-rendered box SECOND. Slot order must win.
    let laid = lay_out(
        Row::new(row![
            BoxBuilder {
                width: 50.0,
                height: 30.0,
            },
            SizedBox::new(70.0, 30.0),
        ]),
        tight(300.0, 30.0),
    );

    let root = laid.root();

    let first = laid.child(root, 0);
    let second = laid.child(root, 1);

    // child(root, 0) must be the COMPONENT's box (50 wide) at the left edge —
    // not the directly-rendered SizedBox that attached first.
    assert_eq!(laid.size(first), size(50.0, 30.0));
    assert_eq!(laid.offset(first), offset(0.0, 0.0));

    // child(root, 1) is the directly-rendered SizedBox, following at x = 50.
    assert_eq!(laid.size(second), size(70.0, 30.0));
    assert_eq!(laid.offset(second), offset(50.0, 0.0));
}

/// The first child of each container in
/// [`a_lazy_list_item_keeps_its_containers_children_in_slot_order`]: a
/// component whose 44 × 44 box attaches after its render sibling's.
#[derive(Clone, Copy, Debug)]
enum DeferredFirst {
    /// A `StatelessView` one level above the box.
    Stateless,
    /// A `StatefulView` that reads an inherited scope in `init_state`.
    PanClaimRegion,
}

impl DeferredFirst {
    fn view(self) -> BoxedView {
        match self {
            Self::Stateless => BoxBuilder {
                width: 44.0,
                height: 44.0,
            }
            .boxed(),
            Self::PanClaimRegion => {
                PanClaimRegion::new(PanClaim::None, SizedBox::new(44.0, 44.0)).boxed()
            }
        }
    }
}

/// An order-sensitive container holding `[deferred, SizedBox(30, 50)]`.
#[derive(Clone, Copy, Debug)]
enum Container {
    Row,
    Column,
    Stack,
    Wrap,
}

impl Container {
    fn view(self, first: DeferredFirst) -> BoxedView {
        let children = vec![first.view(), SizedBox::new(30.0, 50.0).boxed()];
        match self {
            Self::Row => Row::new(children).boxed(),
            Self::Column => Column::new(children).boxed(),
            Self::Stack => Stack::new(children).boxed(),
            Self::Wrap => Wrap::new(children).boxed(),
        }
    }
}

/// A list item builds in the sliver's layout-time build, a later build pass
/// than the one that mounted the list, and its subtree is reached only
/// through the sliver's child manager. Its containers still keep their
/// render children in element-slot order: the deferred first child stays
/// first instead of being laid out after its render sibling.
pub(crate) fn a_lazy_list_item_keeps_its_containers_children_in_slot_order() {
    for container in [
        Container::Row,
        Container::Column,
        Container::Stack,
        Container::Wrap,
    ] {
        for first in [DeferredFirst::Stateless, DeferredFirst::PanClaimRegion] {
            let laid = lay_out(
                ListView::builder(1, 120.0, move |index| {
                    (index == 0).then(|| container.view(first))
                }),
                tight(400.0, 120.0),
            );
            let case = format!("{container:?} with a {first:?} first child");
            let first_box = render_node_sized(&laid, size(44.0, 44.0), &case);
            let second_box = render_node_sized(&laid, size(30.0, 50.0), &case);
            let first_path = render_ancestry(&laid, first_box);
            let second_path = render_ancestry(&laid, second_box);
            let container_id = *first_path
                .iter()
                .find(|id| second_path.contains(id))
                .expect("both boxes sit under the container");
            let order = laid.children(container_id);
            assert_eq!(order.len(), 2, "{case}: the container holds both children");
            assert!(
                first_path.contains(&order[0]) && second_path.contains(&order[1]),
                "{case}: the container's render children follow slot order"
            );

            let first_at = laid.absolute_offset(first_box);
            let second_at = laid.absolute_offset(second_box);
            match container {
                Container::Row | Container::Wrap => {
                    assert_eq!(first_at.dx, 0.0, "{case}: the first box leads");
                    assert_eq!(second_at.dx, 44.0, "{case}: the second box follows");
                }
                Container::Column => {
                    assert_eq!(first_at.dy, 0.0, "{case}: the first box leads");
                    assert_eq!(second_at.dy, 44.0, "{case}: the second box follows");
                }
                Container::Stack => {}
            }
        }
    }
}

/// The one render node laid out at `wanted`.
fn render_node_sized(laid: &LaidOut, wanted: Size, case: &str) -> RenderId {
    let ids: Vec<RenderId> = laid
        .pipeline_owner()
        .with(|owner| owner.render_tree().iter().map(|(id, _)| id).collect());
    let mut found = ids
        .into_iter()
        .filter(|&id| laid.try_size(id) == Some(wanted) && laid.children(id).is_empty());
    let id = found
        .next()
        .unwrap_or_else(|| panic!("{case}: a leaf box sized {wanted:?}"));
    assert!(
        found.next().is_none(),
        "{case}: one leaf box sized {wanted:?}"
    );
    id
}

/// `id` and its render ancestors, nearest first.
fn render_ancestry(laid: &LaidOut, id: RenderId) -> Vec<RenderId> {
    laid.pipeline_owner().with(|owner| {
        std::iter::successors(Some(id), |&node| owner.render_tree().parent(node)).collect()
    })
}
