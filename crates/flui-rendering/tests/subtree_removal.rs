//! Removing a subtree takes each live node of it once, drops children before their parent, and
//! forgets the owner's records of exactly those nodes, whatever shape the child lists have.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::storage::RenderTree;

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
trait Holds {
    fn add(&mut self, parent: Option<RenderId>, object: BoxedRenderObject) -> RenderId;
    fn tree(&mut self) -> &mut RenderTree;
}

impl Holds for RenderTree {
    fn add(&mut self, parent: Option<RenderId>, object: BoxedRenderObject) -> RenderId {
        match parent {
            Some(parent) => self.insert_box_child(parent, object).expect("live parent"),
            None => self.insert_box(object),
        }
    }

    fn tree(&mut self) -> &mut RenderTree {
        self
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

    fn tree(&mut self) -> &mut RenderTree {
        self.render_tree_mut()
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
        Shape::Cycle => host.tree().append_unlinked_child(a1, root),
        Shape::DuplicateChild => host.tree().append_unlinked_child(root, b),
        Shape::StaleChild => {
            let gone = host
                .tree()
                .insert_box_child(a, object("gone", log))
                .expect("live parent");
            assert_eq!(host.tree().remove_recursive(gone), 1);
            assert_eq!(*log.borrow(), ["gone"]);
            log.borrow_mut().clear();
            host.tree().append_unlinked_child(a, gone);
        }
    }
    root
}

/// The ids in pre-order, children in their stored order.
fn walk_order(tree: &RenderTree, root: RenderId) -> Vec<RenderId> {
    let a = tree.children(root)[0];
    vec![root, a, tree.children(a)[0], tree.children(root)[1]]
}

/// `RenderTree::remove_recursive` lists each live node once in pre-order and drops them in reverse.
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
        assert_eq!(tree.remove_recursive(root), 4, "{shape:?}: every live node");
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
