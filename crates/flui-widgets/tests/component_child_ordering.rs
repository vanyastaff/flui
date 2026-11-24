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
        Row::new((
            BoxBuilder {
                width: 50.0,
                height: 30.0,
            },
            SizedBox::new(70.0, 30.0),
        )),
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

/// An array of children of one type lays out like the tuple of the same
/// children: one child per element, in array order.
pub(crate) fn an_array_of_children_lays_out_in_array_order() {
    let laid = lay_out(
        Row::new([
            SizedBox::new(40.0, 30.0),
            SizedBox::new(60.0, 30.0),
            SizedBox::new(80.0, 30.0),
        ]),
        tight(300.0, 30.0),
    );

    let root = laid.root();
    for (index, (width, x)) in [(40.0, 0.0), (60.0, 40.0), (80.0, 100.0)]
        .into_iter()
        .enumerate()
    {
        let child = laid.child(root, index);
        assert_eq!(laid.size(child), size(width, 30.0), "child {index}");
        assert_eq!(laid.offset(child), offset(x, 0.0), "child {index}");
    }
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
        std::iter::successors(Some(id), |&node| owner.render_tree().parent(node))
            .collect::<Vec<RenderId>>()
    })
}

/// A row of [`a_column_dropping_most_of_its_keyed_rows_keeps_the_rest_in_slot_order`]: keyed by
/// its id, as wide as its id plus one so the reading names it.
#[derive(Clone, Debug, PartialEq, StatelessView)]
struct KeyedRow {
    #[view(key)]
    key: flui_foundation::ValueKey<u32>,
}

impl StatelessView for KeyedRow {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(f64::from(*self.key.value()) + 1.0, KEYED_ROW_HEIGHT)
    }
}

const KEYED_ROW_HEIGHT: f64 = 2.0;

/// The column of [`KeyedRow`]s with `ids`, in that order, beside an unkeyed box at the top.
fn keyed_column(ids: &[u32]) -> Column {
    let mut children = vec![SizedBox::new(500.0, KEYED_ROW_HEIGHT).boxed()];
    children.extend(ids.iter().map(|&id| {
        KeyedRow {
            key: flui_foundation::ValueKey::new(id),
        }
        .boxed()
    }));
    Column::new(children).cross_axis_alignment(flui_objects::CrossAxisAlignment::Start)
}

/// A column that drops many of its keyed rows in one update (a search filter) keeps each row
/// that stays, the same render object, at its new slot, and lays the rows out in that order;
/// rows inserted between the kept ones in the same update land at their slots too. A second
/// update that also drops the unkeyed box at the top keeps them the same way.
pub(crate) fn a_column_dropping_most_of_its_keyed_rows_keeps_the_rest_in_slot_order() {
    const ROWS: u32 = 400;
    let all: Vec<u32> = (0..ROWS).collect();
    let mut laid = lay_out(keyed_column(&all), tight(1_000.0, 2_000.0));
    let root = laid.root();
    let before = laid.children(root);
    assert_eq!(before.len(), 1 + all.len());
    let render_of = |id: u32| before[1 + id as usize];

    // The even rows stay, every third of them gains a new row after it.
    let mut ids = Vec::new();
    let mut kept = Vec::new();
    for id in (0..ROWS).filter(|id| id % 2 == 0) {
        ids.push(id);
        kept.push(id);
        if id % 3 == 0 {
            ids.push(ROWS + id);
        }
    }
    laid.pump_widget(keyed_column(&ids));
    assert_eq!(laid.root(), root);
    let after = laid.children(root);
    assert_eq!(after.len(), 1 + ids.len(), "one render child per row");
    for (slot, &id) in ids.iter().enumerate() {
        let child = after[1 + slot];
        if id < ROWS {
            assert_eq!(child, render_of(id), "row {id} keeps its render object");
        }
        assert_eq!(
            laid.size(child),
            size(f64::from(id) + 1.0, KEYED_ROW_HEIGHT),
            "row {id} at slot {slot}"
        );
        assert_eq!(
            laid.offset(child),
            offset(0.0, KEYED_ROW_HEIGHT * (slot + 1) as f64),
            "row {id} at slot {slot}"
        );
    }
    assert!(
        before[1..]
            .iter()
            .enumerate()
            .filter(|(id, _)| id % 2 == 1)
            .all(|(_, render)| !after.contains(render)),
        "no odd row is listed"
    );

    // The same filter dropping the unkeyed top box as well.
    laid.pump_widget(
        Column::new(
            kept.iter()
                .map(|&id| {
                    KeyedRow {
                        key: flui_foundation::ValueKey::new(id),
                    }
                    .boxed()
                })
                .collect::<Vec<BoxedView>>(),
        )
        .cross_axis_alignment(flui_objects::CrossAxisAlignment::Start),
    );
    let last = laid.children(root);
    assert_eq!(last.len(), kept.len());
    for (slot, &id) in kept.iter().enumerate() {
        assert_eq!(
            last[slot],
            render_of(id),
            "row {id} keeps its render object"
        );
        assert_eq!(
            laid.offset(last[slot]),
            offset(0.0, KEYED_ROW_HEIGHT * slot as f64)
        );
    }
}
