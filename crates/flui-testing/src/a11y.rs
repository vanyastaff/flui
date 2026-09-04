//! Query the accessibility tree exactly as a screen reader receives it.
//!
//! A test asserts against the same [`accesskit::TreeUpdate`] a platform adapter
//! would hand to the OS, produced by the same translation
//! (`flui_semantics::tree_to_update`). There is deliberately no second,
//! test-only role vocabulary: if a query here finds a `Role::Button`, a screen
//! reader announces a button, and if it does not, the screen reader is silent
//! too. A parallel query path would agree with the adapter only by accident.
//!
//! Reach it through [`HeadlessBinding::a11y_tree`](crate::HeadlessBinding::a11y_tree),
//! after [`enable_semantics`](crate::HeadlessBinding::enable_semantics) — the
//! semantics phase is a no-op until something asks for it, exactly as in
//! production.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};

use accesskit::{Node, TreeUpdate};

// AccessKit's vocabulary is this module's vocabulary, so a consumer writes
// `use flui_testing::a11y::Role` and never has to add accesskit itself at a
// version that must match ours. Named items only — a glob would enrol every
// future accesskit item into this crate's contract without anyone deciding to.
pub use accesskit::{Action, NodeId, Rect as A11yRect, Role, Toggled};

/// The binding drives no tree, so there is no pipeline to assemble semantics
/// from.
///
/// Returned rather than panicked because this is a caller error, not a broken
/// internal invariant — the two are different claims, and `expect("BUG: …")`
/// would assert the wrong one. A test that knows its binding is tree-bound
/// unwraps at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("semantics requires a tree-bound binding; construct one with HeadlessBinding::with_tree")]
pub struct NotTreeBound;

// ============================================================================
// A11yTree
// ============================================================================

/// A snapshot of the accessibility tree, queryable by role and label.
///
/// # Traversal order
///
/// Queries return matches in **pre-order from the root** — the order a screen
/// reader walks the tree, and what "the first button" means to someone reading
/// the test. This is not the order the nodes arrive in: the semantics tree
/// stores nodes in a slab, so emission order follows slab indices and shifts as
/// freed slots are reused. Sorting by that would make `find_all(...)[0]`
/// silently pick a different node after an unrelated rebuild.
///
/// # Reachability
///
/// Only nodes reachable from the root are queryable, because only those are
/// announced. A node that was built but never linked into the tree is invisible
/// to assistive technology, and a finder that returned it would report a control
/// as present that no user can reach. [`unreachable_count`](Self::unreachable_count)
/// exposes how many such nodes exist, so "built but detached" stays diagnosable
/// instead of merely absent.
#[derive(Debug, Clone)]
pub struct A11yTree {
    update: TreeUpdate,
    /// Indices into `update.nodes`, pre-order from the root.
    order: Vec<usize>,
}

impl A11yTree {
    /// Builds the queryable snapshot from a raw update.
    #[must_use]
    pub fn new(update: TreeUpdate) -> Self {
        let index: HashMap<NodeId, usize> = update
            .nodes
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();

        let mut order = Vec::with_capacity(update.nodes.len());
        let mut seen: HashSet<NodeId> = HashSet::with_capacity(update.nodes.len());

        if let Some(tree) = update.tree.as_ref() {
            let mut stack = vec![tree.root];
            while let Some(id) = stack.pop() {
                let Some(&i) = index.get(&id) else { continue };
                // `seen` doubles as the cycle guard: a malformed update whose
                // children loop back to an ancestor terminates here rather than
                // spinning a test run forever.
                if !seen.insert(id) {
                    continue;
                }
                order.push(i);
                // Reversed, so popping yields children left-to-right.
                stack.extend(update.nodes[i].1.children().iter().rev().copied());
            }
        }

        Self { update, order }
    }

    /// The number of queryable (root-reachable) nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether no node is reachable from the root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Nodes present in the update but not reachable from the root.
    ///
    /// Non-zero means the semantics build produced a node that no assistive
    /// technology will ever announce — see [the type docs](Self#reachability).
    #[must_use]
    pub fn unreachable_count(&self) -> usize {
        self.update.nodes.len() - self.order.len()
    }

    /// Every queryable node, in pre-order.
    pub fn nodes(&self) -> impl Iterator<Item = A11yNode<'_>> + '_ {
        self.order.iter().map(|&i| {
            let (id, node) = &self.update.nodes[i];
            A11yNode { id: *id, node }
        })
    }

    /// The focused node, if the update's focus target is reachable.
    #[must_use]
    pub fn focus(&self) -> Option<A11yNode<'_>> {
        self.nodes().find(|n| n.id == self.update.focus)
    }

    /// Every node with `role`, in pre-order.
    #[must_use]
    pub fn find_all(&self, role: Role) -> Vec<A11yNode<'_>> {
        self.nodes().filter(|n| n.role() == role).collect()
    }

    /// The single node with `role`.
    ///
    /// # Errors
    ///
    /// [`A11yQueryError::NotFound`] if nothing matches, or
    /// [`A11yQueryError::Ambiguous`] if more than one node does. Ambiguity is an
    /// error rather than "take the first" because a test that says *the* button
    /// is asserting there is one; silently picking one would keep passing after
    /// a second button appears.
    pub fn find(&self, role: Role) -> Result<A11yNode<'_>, A11yQueryError> {
        self.exactly_one(A11yQuery::Role(role), self.find_all(role))
    }

    /// Every node whose label equals `label`, in pre-order.
    #[must_use]
    pub fn find_all_by_label(&self, label: &str) -> Vec<A11yNode<'_>> {
        self.nodes().filter(|n| n.label() == Some(label)).collect()
    }

    /// The single node whose label equals `label`.
    ///
    /// # Errors
    ///
    /// As [`find`](Self::find).
    pub fn find_by_label(&self, label: &str) -> Result<A11yNode<'_>, A11yQueryError> {
        self.exactly_one(
            A11yQuery::Label(label.to_string()),
            self.find_all_by_label(label),
        )
    }

    fn exactly_one<'a>(
        &'a self,
        query: A11yQuery,
        mut matches: Vec<A11yNode<'a>>,
    ) -> Result<A11yNode<'a>, A11yQueryError> {
        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => Err(A11yQueryError::NotFound {
                query,
                tree: self.describe(),
            }),
            _ => Err(A11yQueryError::Ambiguous {
                query,
                matches: matches.iter().map(A11yNode::describe).collect(),
            }),
        }
    }

    /// A one-line-per-node rendering of the tree, for failure messages.
    ///
    /// Carried inside [`A11yQueryError::NotFound`] so a failed `find` shows what
    /// the tree *did* contain. A finder that reports only "not found" makes the
    /// reader re-run the test with ad-hoc printing to learn anything.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.order.is_empty() {
            return "<no reachable semantics nodes>".to_string();
        }
        self.nodes()
            .map(|n| format!("  {}", n.describe()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The underlying update, for assertions this API does not cover.
    #[must_use]
    pub fn raw(&self) -> &TreeUpdate {
        &self.update
    }
}

// ============================================================================
// A11yNode
// ============================================================================

/// One node in an [`A11yTree`].
#[derive(Debug, Clone, Copy)]
pub struct A11yNode<'a> {
    id: NodeId,
    node: &'a Node,
}

impl<'a> A11yNode<'a> {
    /// The AccessKit node id.
    #[must_use]
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The resolved role.
    #[must_use]
    pub fn role(&self) -> Role {
        self.node.role()
    }

    /// The accessible label.
    #[must_use]
    pub fn label(&self) -> Option<&'a str> {
        self.node.label()
    }

    /// The node's value, for controls that carry one.
    #[must_use]
    pub fn value(&self) -> Option<&'a str> {
        self.node.value()
    }

    /// Whether the node is announced as disabled.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.node.is_disabled()
    }

    /// Checkbox/switch/radio state, `None` when the node has no toggle concept.
    #[must_use]
    pub fn toggled(&self) -> Option<Toggled> {
        self.node.toggled()
    }

    /// Whether the node advertises `action`.
    #[must_use]
    pub fn supports_action(&self, action: Action) -> bool {
        self.node.supports_action(action)
    }

    /// The node's bounds, in the coordinate space the platform adapter reads.
    ///
    /// `None` when the assembler published no rect for this node. A screen
    /// reader uses this to place its focus ring and to decide what a touch
    /// lands on, so a rect that outgrows what is actually on screen is a
    /// user-visible defect, not a cosmetic one.
    #[must_use]
    pub fn bounds(&self) -> Option<A11yRect> {
        self.node.bounds()
    }

    /// The node's one-based position within its set, if it has one.
    ///
    /// This is what a screen reader reads out as the "12" in "item 12 of 100"
    /// — paired with the container's [`size_of_set`](Self::size_of_set). It is
    /// one-based here because AccessKit's `position_in_set` is; the framework
    /// side keeps the reference's zero-based index and converts once, at the
    /// translation boundary.
    #[must_use]
    pub fn position_in_set(&self) -> Option<usize> {
        self.node.position_in_set()
    }

    /// The total number of items in this node's set, if it declares one — the
    /// "100" in "item 12 of 100".
    #[must_use]
    pub fn size_of_set(&self) -> Option<usize> {
        self.node.size_of_set()
    }

    /// The node's children, in tree order.
    #[must_use]
    pub fn child_ids(&self) -> &'a [NodeId] {
        self.node.children()
    }

    /// The underlying node, for assertions this API does not cover.
    #[must_use]
    pub fn raw(&self) -> &'a Node {
        self.node
    }

    /// A compact one-line rendering, used in query-failure messages.
    #[must_use]
    pub fn describe(&self) -> String {
        // Writing to a String is infallible, so the `fmt::Result`s are dropped.
        let mut out = format!("{:?}", self.role());
        if let Some(label) = self.label() {
            let _ = write!(out, " label={label:?}");
        }
        if let Some(value) = self.value() {
            let _ = write!(out, " value={value:?}");
        }
        if let Some(toggled) = self.toggled() {
            let _ = write!(out, " toggled={toggled:?}");
        }
        if self.is_disabled() {
            out.push_str(" disabled");
        }
        out
    }
}

// ============================================================================
// A11yQueryError
// ============================================================================

/// What a unique-node query searched for.
///
/// Kept as a typed value rather than a pre-rendered string so a test can match
/// on the query a failure names without parsing a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A11yQuery {
    /// Match on [`A11yNode::role`].
    Role(Role),
    /// Match on an exact [`A11yNode::label`].
    Label(String),
}

impl fmt::Display for A11yQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Role(role) => write!(f, "role == {role:?}"),
            Self::Label(label) => write!(f, "label == {label:?}"),
        }
    }
}

/// Why a unique-node query did not resolve.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum A11yQueryError {
    /// Nothing matched.
    #[error("no semantics node matched {query}; reachable nodes were:\n{tree}")]
    NotFound {
        /// The predicate that was searched for.
        query: A11yQuery,
        /// The tree as it actually was, from [`A11yTree::describe`].
        tree: String,
    },
    /// More than one node matched.
    #[error("{} semantics nodes matched {query}, expected exactly one:\n  {}", matches.len(), matches.join("\n  "))]
    Ambiguous {
        /// The predicate that was searched for.
        query: A11yQuery,
        /// One [`A11yNode::describe`] line per match.
        matches: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Tree, TreeId};

    fn node(role: Role, label: &str, children: &[NodeId]) -> Node {
        let mut n = Node::new(role);
        n.set_label(label.to_string());
        n.set_children(children.to_vec());
        n
    }

    /// Root -> [a, b], where `b` is emitted *before* `a` and the root last, so
    /// slab order and tree order disagree on every element.
    fn scrambled_update() -> TreeUpdate {
        let (root, a, b) = (NodeId(0), NodeId(1), NodeId(2));
        TreeUpdate {
            nodes: vec![
                (b, node(Role::Button, "b", &[])),
                (a, node(Role::Button, "a", &[])),
                (root, node(Role::GenericContainer, "root", &[a, b])),
            ],
            tree: Some(Tree::new(root)),
            tree_id: TreeId::ROOT,
            focus: a,
        }
    }

    #[test]
    fn queries_return_tree_order_not_emission_order() {
        let tree = A11yTree::new(scrambled_update());

        let labels: Vec<_> = tree
            .nodes()
            .map(|n| n.label().unwrap().to_string())
            .collect();
        assert_eq!(
            labels,
            ["root", "a", "b"],
            "pre-order from the root, not the order nodes were emitted in"
        );

        let buttons = tree.find_all(Role::Button);
        assert_eq!(
            buttons
                .iter()
                .map(|n| n.label().unwrap())
                .collect::<Vec<_>>(),
            ["a", "b"],
            "`find_all(...)[0]` must be the first button a screen reader reaches"
        );
    }

    #[test]
    fn a_node_unreachable_from_the_root_is_not_queryable_but_is_counted() {
        let mut update = scrambled_update();
        let orphan = NodeId(9);
        update
            .nodes
            .push((orphan, node(Role::Button, "orphan", &[])));

        let tree = A11yTree::new(update);

        assert_eq!(
            tree.len(),
            3,
            "the orphan is not announced, so not queryable"
        );
        assert_eq!(tree.unreachable_count(), 1, "but it stays diagnosable");
        assert!(
            tree.find_all_by_label("orphan").is_empty(),
            "a detached node must not be reported as a present control"
        );
    }

    #[test]
    fn a_cycle_in_the_update_terminates() {
        let (root, a) = (NodeId(0), NodeId(1));
        let update = TreeUpdate {
            nodes: vec![
                (root, node(Role::GenericContainer, "root", &[a])),
                // `a` points back at the root.
                (a, node(Role::Button, "a", &[root])),
            ],
            tree: Some(Tree::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        };

        let tree = A11yTree::new(update);
        assert_eq!(tree.len(), 2, "each node is visited exactly once");
    }

    #[test]
    fn find_reports_ambiguity_rather_than_taking_the_first() {
        let tree = A11yTree::new(scrambled_update());

        let err = tree.find(Role::Button).unwrap_err();
        let A11yQueryError::Ambiguous { matches, .. } = &err else {
            panic!("expected Ambiguous, got {err:?}");
        };
        assert_eq!(matches.len(), 2);
        // Both candidates are named, so the reader can pick a narrower query.
        let text = err.to_string();
        assert!(text.contains("\"a\""), "{text}");
        assert!(text.contains("\"b\""), "{text}");
    }

    #[test]
    fn a_failed_find_shows_the_tree_it_searched() {
        let tree = A11yTree::new(scrambled_update());

        let err = tree.find(Role::Slider).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("Slider"), "names the query: {text}");
        assert!(
            text.contains("label: \"a\"") || text.contains("label=\"a\""),
            "and dumps what was actually there: {text}"
        );
    }

    #[test]
    fn find_by_label_resolves_a_unique_match() {
        let tree = A11yTree::new(scrambled_update());

        let found = tree
            .find_by_label("a")
            .expect("exactly one node labelled a");
        assert_eq!(found.role(), Role::Button);
        assert_eq!(found.id(), NodeId(1));
    }

    #[test]
    fn focus_resolves_through_the_reachable_set() {
        let tree = A11yTree::new(scrambled_update());
        assert_eq!(tree.focus().expect("focus is reachable").label(), Some("a"));
    }
}
