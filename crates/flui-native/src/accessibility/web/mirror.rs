//! The tree the semantics host mirrors, and the DOM operations each update
//! turns into.
//!
//! Pure Rust: [`Mirror::apply`] keeps the nodes AccessKit sent and answers
//! with [`Op`]s, which the DOM side runs in order after the mirror's borrow
//! is released, so no DOM listener runs while the tree is being changed.

use std::collections::{HashMap, HashSet};

use accesskit::{Action, Node, NodeId, Role, TextPosition, TextSelection, TreeId, TreeUpdate};

use super::aria::{self, ElementIds, ElementSpec, ParentFrame};
use super::selection::{self, Run};

/// One change the DOM must make, in the order given.
#[derive(Debug, PartialEq)]
pub(crate) enum Op {
    /// Create a detached element for `id`. A later [`Op::Children`] or
    /// [`Op::Root`] places it.
    Create { id: NodeId, spec: ElementSpec },
    /// Put a new element for `id`, of `spec`, in the place of its old one,
    /// with the old one's children.
    Replace { id: NodeId, spec: ElementSpec },
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
    /// Take `id`'s element out of its parent's, keeping it: an [`Op::Children`]
    /// of its new parent places it again.
    Detach(NodeId),
    /// Drop `id`'s element and its listeners.
    Remove(NodeId),
    /// The tree's focus moved to `id`.
    Focus(NodeId),
    /// The page's selection is to show this one, published by static text
    /// that publishes runs, or none: the text it was in is gone or holds no
    /// selection now. Also asked when such text is written again, which
    /// drops the page's selection in it.
    Select(Option<TextSelection>),
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
pub(crate) struct Mirror {
    nodes: HashMap<NodeId, Entry>,
    root: Option<NodeId>,
    focus: Option<NodeId>,
    /// The node whose published selection the page shows, and that
    /// selection.
    selection: Option<(NodeId, TextSelection)>,
    ids: ElementIds,
}

impl Mirror {
    /// An empty mirror whose elements take their `id`s from `ids`.
    pub(crate) fn new(ids: ElementIds) -> Self {
        Self {
            nodes: HashMap::new(),
            root: None,
            focus: None,
            selection: None,
            ids,
        }
    }

    /// Whether a tree has been mirrored yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// The node mirrored as `id`, if it is in the tree.
    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).map(|entry| &entry.node)
    }

    /// The spec `id`'s element was last brought to, if it is mirrored.
    pub(crate) fn spec(&self, id: NodeId) -> Option<&ElementSpec> {
        self.nodes.get(&id).and_then(|entry| entry.spec.as_ref())
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
        let mut selected: Vec<(NodeId, Option<TextSelection>)> = Vec::new();
        for (id, node) in update.nodes {
            let previous = self
                .nodes
                .get(&id)
                .and_then(|entry| entry.node.text_selection());
            if node.text_selection() != previous && !aria::takes_text(&node) {
                selected.push((id, node.text_selection().copied()));
            }
            if let Some(entry) = self.nodes.get_mut(&id) {
                if entry.node.children() != node.children() {
                    children_changed.push(id);
                    structure_changed = true;
                }
                // Its children are placed in its box, scrolled as it is.
                if ParentFrame::of(&entry.node) != ParentFrame::of(&node) {
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

        // Parents from the children lists, and what left the tree. An element
        // whose parent changes is taken out first: the parents are arranged
        // in identity order, and one placed under its old descendant before
        // that descendant moves out would be a cycle the DOM refuses.
        if structure_changed {
            let before: HashMap<NodeId, Option<NodeId>> = self
                .nodes
                .iter()
                .filter(|(_, entry)| entry.spec.is_some())
                .map(|(&id, entry)| (id, entry.parent))
                .collect();
            let reachable = self.relink(root);
            let mut moved: Vec<NodeId> = before
                .into_iter()
                .filter(|&(id, parent)| {
                    reachable.contains(&id)
                        && self
                            .nodes
                            .get(&id)
                            .is_some_and(|entry| entry.parent != parent)
                })
                .map(|(id, _)| id)
                .collect();
            moved.sort_unstable();
            ops.extend(moved.into_iter().map(Op::Detach));
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
        // moved, whose scroll changed or whose children changed, since they
        // are placed in its box.
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
        // A paragraph's text is its runs'.
        dirty.extend(
            sent.iter()
                .filter_map(|id| self.nodes.get(id))
                .filter(|entry| entry.node.role() == Role::TextRun)
                .filter_map(|entry| entry.parent),
        );
        dirty.sort_unstable();
        dirty.dedup();
        let mut rewritten = false;
        for id in dirty {
            rewritten |= self.refresh(id, root, &mut ops);
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
        // An element moved or replaced takes the page's selection in it
        // along: shown again like text written again.
        let moved = ops.iter().any(|op| {
            matches!(
                op,
                Op::Replace { .. } | Op::Children { .. } | Op::Root(_) | Op::Detach(_)
            )
        });
        self.select(selected, rewritten || moved, &mut ops);
        Ok(ops)
    }

    /// Follow the selections `selected` names, as (node, published
    /// selection): the last one published is the page's. The one shown is
    /// dropped when its node left the tree or published none, and shown
    /// again when text it may lie in was `rewritten` or moved.
    fn select(
        &mut self,
        selected: Vec<(NodeId, Option<TextSelection>)>,
        rewritten: bool,
        ops: &mut Vec<Op>,
    ) {
        let before = self.selection.map(|(_, selection)| selection);
        for (id, selection) in selected {
            match selection {
                Some(selection) => self.selection = Some((id, selection)),
                None if self.selection.is_some_and(|(owner, _)| owner == id) => {
                    self.selection = None;
                }
                None => {}
            }
        }
        if self
            .selection
            .is_some_and(|(owner, _)| !self.nodes.contains_key(&owner))
        {
            self.selection = None;
        }
        let after = self.selection.map(|(_, selection)| selection);
        if after != before || (rewritten && after.is_some()) {
            ops.push(Op::Select(after));
        }
    }

    /// The selection the page shows, as its node published it.
    pub(crate) fn selection(&self) -> Option<TextSelection> {
        self.selection.map(|(_, selection)| selection)
    }

    /// The runs of `paragraph`, in order, with their ids: its `TextRun`
    /// children.
    fn runs(&self, paragraph: NodeId) -> Vec<(NodeId, Run<'_>)> {
        let Some(entry) = self.nodes.get(&paragraph) else {
            return Vec::new();
        };
        entry
            .node
            .children()
            .iter()
            .filter_map(|&child| {
                let node = &self.nodes.get(&child)?.node;
                (node.role() == Role::TextRun).then(|| {
                    (
                        child,
                        Run {
                            text: node.value().unwrap_or_default(),
                            lengths: node.character_lengths(),
                        },
                    )
                })
            })
            .collect()
    }

    /// The text of static text that publishes runs: the runs' text joined,
    /// so that the page's offsets in its element are the runs'. `None` for
    /// any other node.
    fn run_text(&self, id: NodeId) -> Option<String> {
        let node = &self.nodes.get(&id)?.node;
        if node.role() != Role::Label {
            return None;
        }
        let runs = self.runs(id);
        (!runs.is_empty()).then(|| runs.iter().map(|(_, run)| run.text).collect())
    }

    /// The position in `paragraph`'s runs of UTF-16 `offset` into its
    /// element's text; see [`selection::position_at`].
    pub(crate) fn text_position(&self, paragraph: NodeId, offset: u32) -> Option<TextPosition> {
        self.run_text(paragraph)?;
        let runs = self.runs(paragraph);
        let texts: Vec<Run<'_>> = runs.iter().map(|&(_, run)| run).collect();
        let (run, character_index) = selection::position_at(&texts, offset)?;
        Some(TextPosition {
            node: runs[run].0,
            character_index,
        })
    }

    /// Where `position` lies in the page: its run's paragraph and the
    /// UTF-16 offset into the paragraph element's text.
    pub(crate) fn dom_position(&self, position: TextPosition) -> Option<(NodeId, u32)> {
        let paragraph = self.nodes.get(&position.node)?.parent?;
        self.run_text(paragraph)?;
        let runs = self.runs(paragraph);
        let run = runs.iter().position(|&(id, _)| id == position.node)?;
        let texts: Vec<Run<'_>> = runs.iter().map(|&(_, run)| run).collect();
        let offset = selection::offset_of(&texts, run, position.character_index)?;
        Some((paragraph, offset))
    }

    /// The node a selection from `anchor`'s text to `focus`'s is asked of:
    /// the nearest node over both that takes `SetTextSelection`, the
    /// paragraph itself when both ends are in it.
    pub(crate) fn selection_target(&self, anchor: NodeId, focus: NodeId) -> Option<NodeId> {
        let ancestors = |mut id: NodeId| {
            let mut chain = vec![id];
            while let Some(parent) = self.nodes.get(&id).and_then(|entry| entry.parent) {
                chain.push(parent);
                id = parent;
            }
            chain
        };
        let over_focus: HashSet<NodeId> = ancestors(focus).into_iter().collect();
        ancestors(anchor).into_iter().find(|id| {
            over_focus.contains(id)
                && self
                    .nodes
                    .get(id)
                    .is_some_and(|entry| entry.node.supports_action(Action::SetTextSelection))
        })
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

    /// Bring `id`'s spec up to date and say what changed. Answers whether
    /// the text of static text that publishes runs was written, which drops
    /// the page's selection in it.
    fn refresh(&mut self, id: NodeId, root: NodeId, ops: &mut Vec<Op>) -> bool {
        let Some(entry) = self.nodes.get(&id) else {
            return false;
        };
        if !aria::is_mirrored(&entry.node) {
            return false;
        }
        let frame = entry
            .parent
            .and_then(|parent| self.nodes.get(&parent))
            .map_or_else(ParentFrame::default, |parent| ParentFrame::of(&parent.node));
        let mut spec = aria::describe(id, &entry.node, frame, id == root, &self.ids);
        let run_text = self.run_text(id);
        let runs = run_text.is_some();
        if runs {
            spec.text = run_text;
        }
        let entry = self
            .nodes
            .get_mut(&id)
            .expect("BUG: refreshed node is mirrored");
        match entry.spec.replace(spec.clone()) {
            None => {
                ops.push(Op::Create { id, spec });
                runs
            }
            // A node that is mirrored as another element now (a link given a
            // URL, a slider made read-only) gets a new one in its place.
            Some(old) if old.tag != spec.tag => {
                ops.push(Op::Replace { id, spec });
                runs
            }
            Some(old) if old != spec => {
                let written = runs && old.text != spec.text;
                ops.push(Op::Update { id, old, new: spec });
                written
            }
            Some(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{
        Action, Node, NodeId, Rect, Role, TextPosition, TextSelection, TreeId, TreeInfo, TreeUpdate,
    };

    use super::{ElementIds, Mirror, Op, Refused};

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
                Op::Replace { id, .. } => format!("replace {}", id.0),
                Op::Children { id, children } => format!(
                    "children {} {:?}",
                    id.0,
                    children.iter().map(|child| child.0).collect::<Vec<_>>()
                ),
                Op::Detach(id) => format!("detach {}", id.0),
                Op::Root(id) => format!("root {}", id.0),
                Op::Remove(id) => format!("remove {}", id.0),
                Op::Focus(id) => format!("focus {}", id.0),
                Op::Select(selection) => format!("select {selection:?}"),
            })
            .collect()
    }

    /// A sequence of updates against the DOM operations each one asks for.
    #[test]
    fn each_update_becomes_the_dom_operations_it_needs() {
        let mut mirror = Mirror::new(ElementIds::new("t"));
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
        let mut mirror = Mirror::new(ElementIds::new("t"));
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

    /// A scroller scrolling re-places its children, as far into its content
    /// as it is scrolled, so each stays where it is seen.
    #[test]
    fn a_scroller_scrolling_replaces_its_children() {
        let scroller = |position: f64| {
            let mut node = Node::new(Role::ScrollView);
            node.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
            node.add_action(Action::SetScrollOffset);
            node.set_scroll_y(position);
            node.set_scroll_y_min(0.0);
            node.set_scroll_y_max(500.0);
            node.set_children(vec![BUTTON]);
            node
        };
        let list = NodeId(4);
        let mut mirror = Mirror::new(ElementIds::new("t"));
        mirror
            .apply(full(
                vec![
                    (ROOT, root(&[list])),
                    (list, scroller(0.0)),
                    (BUTTON, button()),
                ],
                ROOT,
            ))
            .expect("first tree");
        let ops = mirror
            .apply(partial(vec![(list, scroller(120.0))], ROOT))
            .expect("scrolled");
        let placed = |ops: &[Op], id| {
            ops.iter().find_map(|op| match op {
                Op::Update {
                    id: updated, new, ..
                } if *updated == id => Some(new.clone()),
                _ => None,
            })
        };
        assert_eq!(
            placed(&ops, list)
                .and_then(|spec| spec.scroll)
                .map(|scroll| (scroll.top, scroll.height)),
            Some((120.0, 800.0)),
            "the scroller is scrolled inside content as tall as its extent"
        );
        assert_eq!(
            placed(&ops, BUTTON).map(|spec| spec.placement.top),
            Some(130.0),
            "the button is re-placed into the scrolled content"
        );
    }

    /// A node mirrored as another element now gets a new element in the
    /// place of its old one.
    #[test]
    fn a_link_given_a_url_is_replaced_by_an_anchor() {
        let link = |url: Option<&str>| {
            let mut node = Node::new(Role::Link);
            node.set_label("Docs");
            node.add_action(Action::Click);
            if let Some(url) = url {
                node.set_url(url);
            }
            node
        };
        let mut mirror = Mirror::new(ElementIds::new("t"));
        mirror
            .apply(full(
                vec![(ROOT, root(&[BUTTON])), (BUTTON, link(None))],
                ROOT,
            ))
            .expect("first tree");
        let ops = mirror
            .apply(partial(
                vec![(BUTTON, link(Some("https://example.com/docs")))],
                ROOT,
            ))
            .expect("given a URL");
        assert_eq!(kinds(&ops), ["replace 2"]);
        assert_eq!(mirror.spec(BUTTON).map(|spec| spec.tag.name()), Some("a"));
    }

    /// A node moving under its old descendant leaves its parent's element
    /// before any parent is arranged, so no arrangement makes a cycle.
    #[test]
    fn a_reparented_node_is_detached_before_parents_are_arranged() {
        let group = |children: &[NodeId]| {
            let mut node = Node::new(Role::Group);
            node.set_children(children.to_vec());
            node
        };
        let (wrapper, outer, inner) = (NodeId(4), NodeId(3), NodeId(2));
        let mut mirror = Mirror::new(ElementIds::new("t"));
        mirror
            .apply(full(
                vec![
                    (ROOT, root(&[wrapper])),
                    (wrapper, group(&[outer])),
                    (outer, group(&[inner])),
                    (inner, group(&[])),
                ],
                ROOT,
            ))
            .expect("first tree");
        let ops = mirror
            .apply(partial(
                vec![
                    (wrapper, group(&[inner])),
                    (inner, group(&[outer])),
                    (outer, group(&[])),
                ],
                ROOT,
            ))
            .expect("the two swapped");
        let structure: Vec<&Op> = ops
            .iter()
            .filter(|op| matches!(op, Op::Detach(_) | Op::Children { .. }))
            .collect();
        assert_eq!(
            structure,
            [
                &Op::Detach(inner),
                &Op::Detach(outer),
                &Op::Children {
                    id: inner,
                    children: vec![outer]
                },
                &Op::Children {
                    id: outer,
                    children: vec![]
                },
                &Op::Children {
                    id: wrapper,
                    children: vec![inner]
                },
            ]
        );
    }

    #[test]
    fn an_update_that_cannot_stand_is_refused_and_changes_nothing() {
        let mut mirror = Mirror::new(ElementIds::new("t"));
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
        let mut mirror = Mirror::new(ElementIds::new("t"));
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

    const AREA: NodeId = NodeId(20);
    const FIRST: NodeId = NodeId(21);
    const SECOND: NodeId = NodeId(22);
    const FIRST_RUN: NodeId = NodeId(31);
    const SECOND_RUN: NodeId = NodeId(32);
    const THIRD_RUN: NodeId = NodeId(33);

    fn run(text: &str) -> Node {
        let mut node = Node::new(Role::TextRun);
        node.set_value(text);
        let lengths: Vec<u8> = text
            .chars()
            .map(|character| u8::try_from(character.len_utf8()).expect("one character"))
            .collect();
        node.set_character_lengths(lengths);
        node
    }

    fn paragraph(value: &str, runs: &[NodeId]) -> Node {
        let mut node = Node::new(Role::Label);
        node.set_value(value);
        node.set_children(runs.to_vec());
        node.add_action(Action::SetTextSelection);
        node
    }

    fn area(selection: Option<TextSelection>) -> Node {
        let mut node = Node::new(Role::Document);
        node.set_children(vec![FIRST, SECOND]);
        node.add_action(Action::SetTextSelection);
        if let Some(selection) = selection {
            node.set_text_selection(selection);
        }
        node
    }

    fn at(node: NodeId, character_index: usize) -> TextPosition {
        TextPosition {
            node,
            character_index,
        }
    }

    fn document(selection: Option<TextSelection>) -> TreeUpdate {
        full(
            vec![
                (ROOT, root(&[AREA])),
                (AREA, area(selection)),
                (
                    FIRST,
                    paragraph("Greeting\nHi 😀 there", &[FIRST_RUN, SECOND_RUN]),
                ),
                (SECOND, paragraph("Bye", &[THIRD_RUN])),
                (FIRST_RUN, run("Hi 😀")),
                (SECOND_RUN, run(" there")),
                (THIRD_RUN, run("Bye")),
            ],
            ROOT,
        )
    }

    /// Static text that publishes runs shows the runs' text, and the page's
    /// offsets in it are positions in the runs: an emoji is one character of
    /// two offsets, and one run's end is the next one's start.
    #[test]
    fn a_paragraph_s_text_is_its_runs_and_its_offsets_their_positions() {
        let mut mirror = Mirror::new(ElementIds::new("t"));
        mirror.apply(document(None)).expect("a document");
        assert_eq!(
            mirror.spec(FIRST).and_then(|spec| spec.text.as_deref()),
            Some("Hi 😀 there"),
            "the runs' text, not the label joined to it"
        );
        let rows = [
            (3, at(FIRST_RUN, 3)),
            (4, at(FIRST_RUN, 3)),
            (5, at(SECOND_RUN, 0)),
            (11, at(SECOND_RUN, 6)),
        ];
        for (offset, position) in rows {
            assert_eq!(
                mirror.text_position(FIRST, offset),
                Some(position),
                "offset {offset}"
            );
        }
        assert_eq!(mirror.text_position(FIRST, 12), None, "past the end");
        assert_eq!(mirror.text_position(AREA, 0), None, "the area has no runs");
        assert_eq!(mirror.dom_position(at(FIRST_RUN, 4)), Some((FIRST, 5)));
        assert_eq!(mirror.dom_position(at(SECOND_RUN, 1)), Some((FIRST, 6)));
        assert_eq!(mirror.dom_position(at(THIRD_RUN, 3)), Some((SECOND, 3)));
        assert_eq!(mirror.dom_position(at(THIRD_RUN, 4)), None);
        assert_eq!(mirror.selection_target(FIRST, FIRST), Some(FIRST));
        assert_eq!(mirror.selection_target(FIRST, SECOND), Some(AREA));
    }

    /// The page shows the selection static text last published, again when
    /// the text it lies in is written or moved, and none once it is cleared or its
    /// node goes.
    #[test]
    fn the_page_shows_the_published_selection() {
        let across = TextSelection {
            anchor: at(FIRST_RUN, 1),
            focus: at(THIRD_RUN, 2),
        };
        let select = |selection: Option<TextSelection>| format!("select {selection:?}");
        let mut mirror = Mirror::new(ElementIds::new("t"));
        let ops = mirror.apply(document(Some(across))).expect("a document");
        assert_eq!(kinds(&ops).last(), Some(&select(Some(across))));
        let ops = mirror
            .apply(partial(vec![(AREA, area(Some(across)))], ROOT))
            .expect("the same selection");
        assert_eq!(kinds(&ops), Vec::<String>::new(), "published already");
        let ops = mirror
            .apply(partial(vec![(THIRD_RUN, run("Bye!"))], ROOT))
            .expect("a run's text changes");
        assert_eq!(
            kinds(&ops),
            vec!["update 22".to_owned(), select(Some(across))],
            "the rewritten text drops the page's selection, so it is shown again"
        );
        let mut reordered = area(Some(across));
        reordered.set_children(vec![SECOND, FIRST]);
        let ops = mirror
            .apply(partial(vec![(AREA, reordered)], ROOT))
            .expect("the paragraphs swap");
        assert_eq!(
            kinds(&ops).last(),
            Some(&select(Some(across))),
            "moved elements drop the page's selection in them, so it is shown again"
        );
        let ops = mirror
            .apply(partial(vec![(AREA, area(None))], ROOT))
            .expect("the selection is cleared, the paragraphs back in order");
        assert_eq!(
            kinds(&ops),
            vec!["children 20 [21, 22]".to_owned(), select(None)]
        );
        assert_eq!(mirror.selection(), None);

        mirror
            .apply(partial(vec![(AREA, area(Some(across)))], ROOT))
            .expect("selected again");
        let ops = mirror
            .apply(partial(vec![(ROOT, root(&[]))], ROOT))
            .expect("the area goes");
        assert_eq!(kinds(&ops).last(), Some(&select(None)));
    }
}
