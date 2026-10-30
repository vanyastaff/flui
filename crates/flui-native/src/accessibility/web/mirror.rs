//! The tree the semantics host mirrors, and the DOM operations each update
//! turns into.
//!
//! Pure Rust: [`Mirror::apply`] keeps the nodes AccessKit sent and answers
//! with [`Op`]s, which the DOM side runs in order after the mirror's borrow
//! is released, so no DOM listener runs while the tree is being changed.

use std::collections::{HashMap, HashSet};

use accesskit::{Node, NodeId, TreeId, TreeUpdate};

use super::aria::{self, ElementSpec, ParentFrame};

/// One change the DOM must make, in the order given.
#[derive(Debug, PartialEq)]
pub(crate) enum Op {
    /// Create a detached element for `id`. A later [`Op::Children`] or
    /// [`Op::Root`] places it.
    Create { id: NodeId, spec: ElementSpec },
    /// Bring `id`'s element from `old` to `new`.
    Update {
        id: NodeId,
        old: ElementSpec,
        new: ElementSpec,
    },
    /// Put these elements, in this order, as `id`'s element's children.
    Children { id: NodeId, children: Vec<NodeId> },
    /// The host's one child is `id`'s element.
    Root(NodeId),
    /// Drop `id`'s element and its listeners.
    Remove(NodeId),
    /// The tree's focus moved to `id`.
    Focus(NodeId),
}

/// Why an update was not applied; the mirror is unchanged and the
/// composition root is asked for the whole tree again.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refused {
    #[error("the first update has no tree information")]
    NoTree,
    #[error("the update is for another tree than the root one")]
    Subtree,
    #[error("node {0:?} is named as a child but was never sent")]
    UnknownNode(NodeId),
}

struct Entry {
    node: Node,
    /// The spec its element was last brought to; `None` for a node that is
    /// not mirrored (a `TextRun`).
    spec: Option<ElementSpec>,
    parent: Option<NodeId>,
}

/// The mirrored tree.
#[derive(Default)]
pub(crate) struct Mirror {
    nodes: HashMap<NodeId, Entry>,
    root: Option<NodeId>,
    focus: Option<NodeId>,
}

impl Mirror {
    /// Whether a tree has been mirrored yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// The node mirrored as `id`, if it is in the tree.
    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).map(|entry| &entry.node)
    }

    /// The node the tree's focus is on, if it is in the tree.
    pub(crate) fn focus(&self) -> Option<NodeId> {
        self.focus
    }

    /// Apply `update` and answer what the DOM must do.
    ///
    /// # Errors
    /// [`Refused`] when the update cannot stand on what is mirrored; nothing
    /// changes then.
    pub(crate) fn apply(&mut self, update: TreeUpdate) -> Result<Vec<Op>, Refused> {
        if update.tree_id != TreeId::ROOT {
            return Err(Refused::Subtree);
        }
        let root = match (&update.tree, self.root) {
            (Some(tree), _) => tree.root,
            (None, Some(root)) => root,
            (None, None) => return Err(Refused::NoTree),
        };
        self.check(&update, root)?;

        let mut ops = Vec::new();
        let sent: HashSet<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
        let mut structure_changed = self.root != Some(root);
        let mut placement_changed: HashSet<NodeId> = HashSet::new();
        let mut children_changed: Vec<NodeId> = Vec::new();
        for (id, node) in update.nodes {
            if let Some(entry) = self.nodes.get_mut(&id) {
                if entry.node.children() != node.children() {
                    children_changed.push(id);
                    structure_changed = true;
                }
                if entry.node.bounds() != node.bounds() {
                    placement_changed.insert(id);
                }
                entry.node = node;
                continue;
            }
            if !node.children().is_empty() {
                children_changed.push(id);
            }
            structure_changed = true;
            self.nodes.insert(
                id,
                Entry {
                    node,
                    spec: None,
                    parent: None,
                },
            );
        }

        // Parents from the children lists, and what left the tree.
        if structure_changed {
            let reachable = self.relink(root);
            let mut removed: Vec<NodeId> = self
                .nodes
                .keys()
                .copied()
                .filter(|id| !reachable.contains(id))
                .collect();
            removed.sort_unstable();
            for id in removed {
                if let Some(entry) = self.nodes.remove(&id)
                    && entry.spec.is_some()
                {
                    ops.push(Op::Remove(id));
                }
            }
        }

        // Specs: every node sent, and the children of a node whose bounds
        // moved or whose children changed, since they are placed in its box.
        let mut dirty: Vec<NodeId> = sent
            .iter()
            .copied()
            .filter(|id| self.nodes.contains_key(id))
            .collect();
        for parent in placement_changed.iter().chain(&children_changed) {
            if let Some(entry) = self.nodes.get(parent) {
                dirty.extend(entry.node.children().iter().copied());
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        for id in dirty {
            self.refresh(id, root, &mut ops);
        }

        children_changed.sort_unstable();
        for id in children_changed {
            let Some(entry) = self.nodes.get(&id) else {
                continue;
            };
            if entry.spec.is_none() {
                continue;
            }
            let children = entry
                .node
                .children()
                .iter()
                .copied()
                .filter(|child| {
                    self.nodes
                        .get(child)
                        .is_some_and(|entry| entry.spec.is_some())
                })
                .collect();
            ops.push(Op::Children { id, children });
        }
        if self.root != Some(root) {
            self.root = Some(root);
            ops.push(Op::Root(root));
        }
        let focus = Some(update.focus).filter(|focus| self.nodes.contains_key(focus));
        if focus != self.focus {
            self.focus = focus;
            if let Some(focus) = focus {
                ops.push(Op::Focus(focus));
            }
        }
        Ok(ops)
    }

    /// Every child named by the update, and the root, is either sent or
    /// already mirrored.
    fn check(&self, update: &TreeUpdate, root: NodeId) -> Result<(), Refused> {
        let sent: HashSet<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
        let known = |id: &NodeId| sent.contains(id) || self.nodes.contains_key(id);
        if !known(&root) {
            return Err(Refused::UnknownNode(root));
        }
        for (_, node) in &update.nodes {
            if let Some(child) = node.children().iter().find(|child| !known(child)) {
                return Err(Refused::UnknownNode(*child));
            }
        }
        Ok(())
    }

    /// Set each reachable node's parent and answer the reachable set.
    fn relink(&mut self, root: NodeId) -> HashSet<NodeId> {
        let mut reachable = HashSet::new();
        let mut stack = vec![(root, None)];
        while let Some((id, parent)) = stack.pop() {
            if !reachable.insert(id) {
                // A node named twice keeps its first parent.
                continue;
            }
            let Some(entry) = self.nodes.get_mut(&id) else {
                continue;
            };
            entry.parent = parent;
            stack.extend(entry.node.children().iter().map(|child| (*child, Some(id))));
        }
        reachable
    }

    /// Bring `id`'s spec up to date and say what changed.
    fn refresh(&mut self, id: NodeId, root: NodeId, ops: &mut Vec<Op>) {
        let Some(entry) = self.nodes.get(&id) else {
            return;
        };
        if !aria::is_mirrored(&entry.node) {
            return;
        }
        let frame = entry
            .parent
            .and_then(|parent| self.nodes.get(&parent))
            .map_or_else(ParentFrame::default, |parent| ParentFrame::of(&parent.node));
        let spec = aria::describe(&entry.node, frame, id == root);
        let entry = self
            .nodes
            .get_mut(&id)
            .expect("BUG: refreshed node is mirrored");
        match entry.spec.replace(spec.clone()) {
            None => ops.push(Op::Create { id, spec }),
            Some(old) if old != spec => ops.push(Op::Update { id, old, new: spec }),
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, Node, NodeId, Rect, Role, TreeId, TreeInfo, TreeUpdate};

    use super::{Mirror, Op, Refused};

    const ROOT: NodeId = NodeId(1);
    const BUTTON: NodeId = NodeId(2);
    const COUNT: NodeId = NodeId(3);

    fn root(children: &[NodeId]) -> Node {
        let mut node = Node::new(Role::Window);
        node.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
        node.set_children(children.to_vec());
        node
    }

    fn button() -> Node {
        let mut node = Node::new(Role::Button);
        node.set_label("Increment");
        node.add_action(Action::Click);
        node.set_bounds(Rect::new(10.0, 10.0, 110.0, 40.0));
        node
    }

    fn count(text: &str) -> Node {
        let mut node = Node::new(Role::Label);
        node.set_value(text);
        node.set_bounds(Rect::new(10.0, 50.0, 110.0, 70.0));
        node
    }

    fn full(nodes: Vec<(NodeId, Node)>, focus: NodeId) -> TreeUpdate {
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    fn partial(nodes: Vec<(NodeId, Node)>, focus: NodeId) -> TreeUpdate {
        TreeUpdate {
            nodes,
            tree: None,
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    fn kinds(ops: &[Op]) -> Vec<String> {
        ops.iter()
            .map(|op| match op {
                Op::Create { id, .. } => format!("create {}", id.0),
                Op::Update { id, .. } => format!("update {}", id.0),
                Op::Children { id, children } => format!(
                    "children {} {:?}",
                    id.0,
                    children.iter().map(|child| child.0).collect::<Vec<_>>()
                ),
                Op::Root(id) => format!("root {}", id.0),
                Op::Remove(id) => format!("remove {}", id.0),
                Op::Focus(id) => format!("focus {}", id.0),
            })
            .collect()
    }

    /// A sequence of updates against the DOM operations each one asks for.
    #[test]
    fn each_update_becomes_the_dom_operations_it_needs() {
        let mut mirror = Mirror::default();
        let steps: Vec<(&str, TreeUpdate, Vec<&str>)> = vec![
            (
                "the first tree creates every element, places them, then roots it",
                full(
                    vec![
                        (ROOT, root(&[BUTTON, COUNT])),
                        (BUTTON, button()),
                        (COUNT, count("Count 0")),
                    ],
                    ROOT,
                ),
                vec![
                    "create 1",
                    "create 2",
                    "create 3",
                    "children 1 [2, 3]",
                    "root 1",
                    "focus 1",
                ],
            ),
            (
                "a changed label updates only its element",
                partial(vec![(COUNT, count("Count 1"))], ROOT),
                vec!["update 3"],
            ),
            (
                "an identical node changes nothing",
                partial(vec![(COUNT, count("Count 1"))], ROOT),
                vec![],
            ),
            (
                "focus moving is its own operation",
                partial(vec![], BUTTON),
                vec!["focus 2"],
            ),
            (
                "a child left out of its parent's list is removed",
                partial(vec![(ROOT, root(&[BUTTON]))], BUTTON),
                vec!["remove 3", "children 1 [2]"],
            ),
            (
                "a child re-added comes back as a new element, after its siblings",
                partial(
                    vec![(ROOT, root(&[COUNT, BUTTON])), (COUNT, count("Count 1"))],
                    BUTTON,
                ),
                vec!["create 3", "children 1 [3, 2]"],
            ),
        ];
        for (name, update, expected) in steps {
            let ops = mirror.apply(update).expect(name);
            assert_eq!(kinds(&ops), expected, "{name}");
        }
        assert_eq!(mirror.focus(), Some(BUTTON));
        assert_eq!(mirror.node(COUNT).and_then(Node::value), Some("Count 1"));
    }

    #[test]
    fn a_parent_s_box_moving_replaces_its_children() {
        let mut mirror = Mirror::default();
        mirror
            .apply(full(
                vec![(ROOT, root(&[BUTTON])), (BUTTON, button())],
                ROOT,
            ))
            .expect("first tree");
        let mut moved = root(&[BUTTON]);
        moved.set_bounds(Rect::new(5.0, 5.0, 405.0, 305.0));
        let ops = mirror
            .apply(partial(vec![(ROOT, moved)], ROOT))
            .expect("moved root");
        // The root's own element keeps its box (the root is placed at the
        // host's origin by its bounds), and the button is re-placed in it.
        let button = ops.iter().find_map(|op| match op {
            Op::Update { id, new, .. } if *id == BUTTON => Some(new.placement),
            _ => None,
        });
        assert_eq!(
            button.map(|placement| (placement.left, placement.top)),
            Some((5.0, 5.0))
        );
    }

    #[test]
    fn an_update_that_cannot_stand_is_refused_and_changes_nothing() {
        let mut mirror = Mirror::default();
        assert_eq!(
            mirror.apply(partial(vec![(ROOT, root(&[]))], ROOT)),
            Err(Refused::NoTree)
        );
        assert_eq!(
            mirror.apply(full(vec![(ROOT, root(&[BUTTON]))], ROOT)),
            Err(Refused::UnknownNode(BUTTON))
        );
        assert!(mirror.is_empty(), "a refused update mirrors nothing");
        assert_eq!(mirror.node(ROOT), None, "nor keeps a node of it");
        mirror
            .apply(full(vec![(ROOT, root(&[]))], ROOT))
            .expect("a tree that stands alone");
        let mut subtree = partial(vec![], ROOT);
        subtree.tree_id = TreeId(accesskit::Uuid::from_u128(7));
        assert_eq!(mirror.apply(subtree), Err(Refused::Subtree));
    }

    #[test]
    fn text_runs_are_not_mirrored_as_elements() {
        let mut field = Node::new(Role::TextInput);
        field.set_value("hi");
        field.set_children(vec![NodeId(9)]);
        let run = Node::new(Role::TextRun);
        let mut mirror = Mirror::default();
        let ops = mirror
            .apply(full(
                vec![(ROOT, root(&[BUTTON])), (BUTTON, field), (NodeId(9), run)],
                ROOT,
            ))
            .expect("tree with a run");
        assert_eq!(
            kinds(&ops),
            vec![
                "create 1",
                "create 2",
                "children 1 [2]",
                "children 2 []",
                "root 1",
                "focus 1"
            ]
        );
    }
}
