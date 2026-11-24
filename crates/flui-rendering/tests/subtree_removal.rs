//! Removing a subtree takes each live node of it once, drops children before their parent, and
//! forgets the owner's records of exactly those nodes, whatever shape the child lists have.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::storage::RenderTree;
use flui_rendering::testing::raw_tree::{self, RawTree};

use crate::common::BoxedRenderObject;

/// The child lists a removal walks, beside the well-formed tree `root → [a → [a1], b]`.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Whole,
    /// `a1` also lists `root`.
    Cycle,
    /// `root` lists `b` twice.
    DuplicateChild,
    /// `a` still lists a child already removed.
    StaleChild,
}

const SHAPES: [Shape; 4] = [
    Shape::Whole,
    Shape::Cycle,
    Shape::DuplicateChild,
    Shape::StaleChild,
];

type DropLog = Rc<RefCell<Vec<&'static str>>>;

/// Records its label when dropped.
#[derive(Debug)]
struct Dropped {
    label: &'static str,
    log: DropLog,
}

impl Drop for Dropped {
    fn drop(&mut self) {
        self.log.borrow_mut().push(self.label);
    }
}

impl flui_foundation::inspect::Inspect for Dropped {
    fn kind(&self) -> &'static str {
        "Dropped"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for Dropped {
    type Arity = flui_foundation::Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<flui_foundation::geometry::Size> {
        Ok(ctx.constraints().biggest())
    }
}

fn object(label: &'static str, log: &DropLog) -> BoxedRenderObject {
    Box::new(Dropped {
        label,
        log: Rc::clone(log),
    })
}

/// Where [`build`] inserts: a bare tree, or an owner that queues each node it inserts.
trait Holds: RawTree {
    fn add(&mut self, parent: Option<RenderId>, object: BoxedRenderObject) -> RenderId;
}

impl Holds for RenderTree {
    fn add(&mut self, parent: Option<RenderId>, object: BoxedRenderObject) -> RenderId {
        match parent {
            Some(parent) => raw_tree::insert_box_child(self, parent, object).expect("live parent"),
            None => raw_tree::insert_box(self, object),
        }
    }
}

impl Holds for PipelineOwner {
    fn add(&mut self, parent: Option<RenderId>, object: BoxedRenderObject) -> RenderId {
        match parent {
            Some(parent) => self
                .insert_child_render_object(parent, object)
                .expect("live parent"),
            None => self.insert(object),
        }
    }
}

/// Builds `root → [a → [a1], b]` under `parent` (or as a root), then bends its lists as `shape`
/// says. Returns `root`.
fn build(host: &mut impl Holds, parent: Option<RenderId>, shape: Shape, log: &DropLog) -> RenderId {
    let root = host.add(parent, object("root", log));
    let a = host.add(Some(root), object("a", log));
    let a1 = host.add(Some(a), object("a1", log));
    let b = host.add(Some(root), object("b", log));
    match shape {
        Shape::Whole => {}
        Shape::Cycle => raw_tree::append_unlinked_child(host, a1, root),
        Shape::DuplicateChild => raw_tree::append_unlinked_child(host, root, b),
        Shape::StaleChild => {
            let gone =
                raw_tree::insert_box_child(host, a, object("gone", log)).expect("live parent");
            assert_eq!(raw_tree::remove_subtree(host, gone), 1);
            assert_eq!(*log.borrow(), ["gone"]);
            log.borrow_mut().clear();
            raw_tree::append_unlinked_child(host, a, gone);
        }
    }
    root
}

/// The ids in pre-order, children in their stored order.
fn walk_order(tree: &RenderTree, root: RenderId) -> Vec<RenderId> {
    let a = tree.children(root)[0];
    vec![root, a, tree.children(a)[0], tree.children(root)[1]]
}

/// Removing a bare tree's subtree (`raw_tree::remove_subtree`, `RenderTree::remove_recursive`)
/// lists each live node once in pre-order and drops them in reverse.
#[test]
fn a_subtree_leaves_the_tree_once_children_first() {
    for shape in SHAPES {
        let log = DropLog::default();
        let mut tree = RenderTree::new();
        let root = build(&mut tree, None, shape, &log);

        assert_eq!(
            tree.collect_subtree_ids(root),
            walk_order(&tree, root),
            "{shape:?}: each live id once, in pre-order",
        );
        assert_eq!(
            raw_tree::remove_subtree(&mut tree, root),
            4,
            "{shape:?}: every live node"
        );
        assert_eq!(
            *log.borrow(),
            ["b", "a1", "a", "root"],
            "{shape:?}: children drop before their parent",
        );
        assert!(tree.is_empty(), "{shape:?}: nothing left in the tree");
    }
}

/// `PipelineOwner::remove_render_object` drops the same nodes in the same order and forgets the
/// dirty records of exactly them: a sibling outside the subtree keeps its own.
#[test]
fn removing_a_subtree_forgets_the_records_of_exactly_its_nodes() {
    let reference = {
        let log = DropLog::default();
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let top = owner.insert(object("top", &log));
        owner
            .insert_child_render_object(top, object("kept", &log))
            .expect("live parent");
        owner.set_root_id(Some(top));
        owner.dirty_node_count()
    };

    for shape in SHAPES {
        let log = DropLog::default();
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let top = owner.insert(object("top", &log));
        let kept = owner
            .insert_child_render_object(top, object("kept", &log))
            .expect("live parent");
        owner.set_root_id(Some(top));
        let root = build(&mut owner, Some(top), shape, &log);
        assert!(
            owner.dirty_node_count() > reference,
            "{shape:?}: the subtree is queued"
        );

        assert_eq!(owner.remove_render_object(root), 4, "{shape:?}");
        assert_eq!(
            *log.borrow(),
            ["b", "a1", "a", "root"],
            "{shape:?}: children drop before their parent",
        );
        assert_eq!(
            owner.dirty_node_count(),
            reference,
            "{shape:?}: the subtree's records are gone and the sibling's remain",
        );
        assert!(owner.render_tree().contains(kept), "{shape:?}");
        assert_eq!(owner.render_tree().children(top), [kept], "{shape:?}");
    }
}

/// A parent of `count` children labelled `row`, under a cell; returns the cell, the parent and
/// its children in order.
fn parent_of_rows(count: usize, log: &DropLog) -> (PipelineCell, RenderId, Vec<RenderId>) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let top = owner.insert(object("top", log));
    owner.set_root_id(Some(top));
    let rows = (0..count)
        .map(|_| {
            owner
                .insert_child_render_object(top, object("row", log))
                .expect("live parent")
        })
        .collect::<Vec<RenderId>>();
    (PipelineCell::new(owner), top, rows)
}

/// A removal batch removes each subtree at once and leaves its parent's list alone until it
/// ends: no removal scans or shifts the list. Its end compacts the list in one pass, keeping the
/// other children in their order, and marks the parent: the odd half of 3000 rows, one of them,
/// removed last, with a child of its own removed before it, as a filtering list unmounts them.
/// That child's removal writes its own parent's list at once: the batch is for `top`.
#[test]
fn a_removal_batch_compacts_the_parent_once_and_keeps_the_order_of_the_rest() {
    const ROWS: usize = 3_000;
    let log = DropLog::default();
    let (cell, top, rows) = parent_of_rows(ROWS, &log);
    let nested = cell.with_mut(|owner| {
        owner
            .insert_child_render_object(rows[1], object("nested", &log))
            .expect("live parent")
    });
    let kept = rows.iter().copied().step_by(2).collect::<Vec<RenderId>>();
    let dropped = rows
        .iter()
        .copied()
        .skip(1)
        .step_by(2)
        .collect::<Vec<RenderId>>();
    cell.with_mut(|owner| owner.layout_pass().run_layout().expect("layout"));
    let clean = cell.with(PipelineOwner::dirty_node_count);

    cell.with_batched_removals(top, || {
        for &id in dropped.iter().filter(|&&id| id != rows[1]) {
            cell.with_mut(|owner| assert_eq!(owner.remove_render_object(id), 1));
        }
        cell.with(|owner| {
            let tree = owner.render_tree();
            assert!(
                dropped.iter().skip(1).all(|&id| !tree.contains(id)),
                "removed at once"
            );
            assert_eq!(
                tree.children(top),
                rows,
                "the list is left to the batch's end"
            );
            assert_eq!(
                owner.dirty_node_count(),
                clean,
                "and so is the parent's mark"
            );
        });
        // A row's own child goes first, as its element unmounts before the row.
        cell.with_mut(|owner| assert_eq!(owner.remove_render_object(nested), 1));
        cell.with(|owner| {
            assert!(
                owner.render_tree().children(rows[1]).is_empty(),
                "a list under another parent is written at once"
            );
        });
        cell.with_mut(|owner| assert_eq!(owner.remove_render_object(rows[1]), 1));
        cell.with(|owner| {
            assert!(!owner.render_tree().contains(rows[1]));
            assert_eq!(owner.render_tree().children(top), rows);
        });
        assert_eq!(
            log.borrow().len(),
            dropped.len() + 1,
            "each node dropped at its removal"
        );
    });

    cell.with(|owner| {
        let tree = owner.render_tree();
        assert_eq!(
            tree.children(top),
            kept,
            "the rows that stay, in their order"
        );
        assert_eq!(tree.len(), 1 + kept.len());
        assert!(
            owner.dirty_node_count() > clean,
            "the parent that lost children is marked"
        );
    });
}

/// A batch whose scope unwinds stays open, its parent's list still naming the removed child, and
/// the next layout pass closes it first: the frame never walks a list that names a removed node,
/// and a removal after it writes the list at once again.
#[test]
fn a_removal_batch_left_open_by_an_unwind_closes_at_the_next_layout_pass() {
    let log = DropLog::default();
    let (cell, top, rows) = parent_of_rows(3, &log);

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cell.with_batched_removals(top, || {
            cell.with_mut(|owner| owner.remove_render_object(rows[1]));
            panic!("a hook of the batch panics");
        })
    }));
    assert!(unwound.is_err());
    cell.with(|owner| {
        assert_eq!(owner.render_tree().children(top), rows, "not yet compacted");
        assert!(!owner.render_tree().contains(rows[1]));
    });

    cell.with_mut(|owner| drop(owner.layout_pass()));
    cell.with(|owner| assert_eq!(owner.render_tree().children(top), [rows[0], rows[2]]));
    assert_eq!(*log.borrow(), ["row"]);

    assert_eq!(
        cell.with_mut(|owner| owner.remove_render_object(rows[2])),
        1
    );
    cell.with(|owner| assert_eq!(owner.render_tree().children(top), [rows[0]]));
}

/// A row of `top` with a child, each with a grandchild: `row → cell → leaf`. Returns the row,
/// the cell and the leaf.
fn row_of_three(cell: &PipelineCell, top: RenderId, log: &DropLog) -> [RenderId; 3] {
    cell.with_mut(|owner| {
        let row = owner
            .insert_child_render_object(top, object("row", log))
            .expect("live parent");
        let inner = owner
            .insert_child_render_object(row, object("cell", log))
            .expect("live parent");
        let leaf = owner
            .insert_child_render_object(inner, object("leaf", log))
            .expect("live parent");
        [row, inner, leaf]
    })
}

/// In a teardown each member whose parent is a member stays in place when its element lets it
/// go, deepest first, and the topmost member's removal takes the whole subtree out once,
/// children dropped first: no parent list under the row is written and no member below it is
/// marked, as Flutter detaches only the topmost render object.
#[test]
fn a_teardown_takes_each_member_out_with_its_topmost_member() {
    let log = DropLog::default();
    let (cell, top, _) = parent_of_rows(0, &log);
    let [row, inner, leaf] = row_of_three(&cell, top, &log);
    cell.with_mut(|owner| owner.layout_pass().run_layout().expect("layout"));
    let clean = cell.with(PipelineOwner::dirty_node_count);

    cell.with_teardown(&mut vec![row, inner, leaf], || {
        for id in [leaf, inner] {
            assert_eq!(cell.with_mut(|owner| owner.remove_render_object(id)), 0);
        }
        cell.with(|owner| {
            let tree = owner.render_tree();
            assert_eq!(tree.children(inner), [leaf], "the leaf stays in place");
            assert_eq!(tree.children(row), [inner], "and so does its parent");
            assert_eq!(owner.dirty_node_count(), clean, "nothing is marked");
        });
        assert!(log.borrow().is_empty(), "nothing is dropped yet");
        assert_eq!(cell.with_mut(|owner| owner.remove_render_object(row)), 3);
        assert_eq!(*log.borrow(), ["leaf", "cell", "row"]);
    });
    cell.with(|owner| {
        let tree = owner.render_tree();
        assert!(![row, inner, leaf].iter().any(|&id| tree.contains(id)));
        assert_eq!(tree.children(top), []);
    });
}

/// A member whose topmost member stays (its element kept it) is removed when the teardown
/// ends, and a node that is not a member, or a member under a parent that is not, leaves at
/// once.
#[test]
fn a_teardown_removes_at_its_end_the_members_still_in_the_tree() {
    let log = DropLog::default();
    let (cell, top, _) = parent_of_rows(0, &log);
    let [row, inner, leaf] = row_of_three(&cell, top, &log);
    let [other, other_inner, _] = row_of_three(&cell, top, &log);

    cell.with_teardown(&mut vec![inner, leaf, other], || {
        assert_eq!(cell.with_mut(|owner| owner.remove_render_object(leaf)), 0);
        assert_eq!(
            cell.with_mut(|owner| owner.remove_render_object(other_inner)),
            2,
            "not a member: removed at once"
        );
        assert_eq!(
            cell.with_mut(|owner| owner.remove_render_object(other)),
            1,
            "a member under a parent that is not one: removed at once"
        );
    });
    cell.with(|owner| {
        let tree = owner.render_tree();
        assert!(
            !tree.contains(inner) && !tree.contains(leaf),
            "swept at the end"
        );
        assert!(tree.contains(row), "not a member");
        assert_eq!(tree.children(row), []);
    });
}

/// A teardown whose scope unwinds closes without removing anything, so the next removal of a
/// former member leaves at once; one opened inside another runs without one.
#[test]
fn a_teardown_that_unwinds_closes_and_one_inside_another_does_not_open() {
    let log = DropLog::default();
    let (cell, top, _) = parent_of_rows(0, &log);
    let [row, inner, leaf] = row_of_three(&cell, top, &log);

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cell.with_teardown(&mut vec![row, inner, leaf], || {
            assert_eq!(cell.with_mut(|owner| owner.remove_render_object(leaf)), 0);
            panic!("an unmount hook of the teardown panics");
        })
    }));
    assert!(unwound.is_err());
    cell.with(|owner| assert!(owner.render_tree().contains(leaf), "nothing removed"));
    assert_eq!(cell.with_mut(|owner| owner.remove_render_object(leaf)), 1);

    cell.with_teardown(&mut vec![row, inner], || {
        cell.with_teardown(&mut vec![inner], || {
            assert_eq!(
                cell.with_mut(|owner| owner.remove_render_object(inner)),
                0,
                "the outer teardown is the open one"
            );
        });
    });
    cell.with(|owner| assert!(!owner.render_tree().contains(inner)));
}
