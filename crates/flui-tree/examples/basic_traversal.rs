//! Basic tree traversal examples.
//!
//! This example demonstrates the core iteration capabilities of flui-tree.
//!
//! Run with: cargo run --example basic_traversal

use flui_foundation::ElementId;
use flui_tree::{
    iter::{Ancestors, BreadthFirstIter, DepthFirstIter, DescendantsWithDepth},
    prelude::*,
    traits::sealed::{TreeNavSealed, TreeReadSealed},
    visitor::{for_each, visit_depth_first, CollectVisitor, MaxDepthVisitor},
};

// Note: TreeVisitor and VisitorResult are not imported since TreeVisitor is sealed.
// Use for_each() for custom visiting logic instead.

// ============================================================================
// EXAMPLE TREE IMPLEMENTATION
// ============================================================================

/// A simple tree node for demonstration.
#[derive(Debug)]
struct DemoNode {
    name: String,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
}

/// A simple tree implementation demonstrating the traits.
struct DemoTree {
    nodes: Vec<Option<DemoNode>>,
}

// Implement sealed traits for external usage
impl TreeReadSealed for DemoTree {}
impl TreeNavSealed for DemoTree {}

impl DemoTree {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    fn insert(&mut self, name: &str, parent: Option<ElementId>) -> ElementId {
        let id = ElementId::new(self.nodes.len() + 1);
        self.nodes.push(Some(DemoNode {
            name: name.to_string(),
            parent,
            children: Vec::new(),
        }));

        // Add to parent's children
        if let Some(parent_id) = parent {
            if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() - 1) {
                parent_node.children.push(id);
            }
        }

        id
    }

    fn name(&self, id: ElementId) -> Option<&str> {
        self.get(id).map(|n| n.name.as_str())
    }
}

impl TreeRead for DemoTree {
    type Id = ElementId;
    type Node = DemoNode;
    type NodeIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

    fn get(&self, id: ElementId) -> Option<&DemoNode> {
        self.nodes.get(id.get() - 1)?.as_ref()
    }

    fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    fn node_ids(&self) -> Self::NodeIter<'_> {
        Box::new((0..self.nodes.len()).filter_map(|i| {
            if self.nodes[i].is_some() {
                Some(ElementId::new(i + 1))
            } else {
                None
            }
        }))
    }
}

impl TreeNav for DemoTree {
    type ChildrenIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;
    type AncestorsIter<'a> = Ancestors<'a, Self>;
    type DescendantsIter<'a> = DescendantsWithDepth<'a, Self>;
    type SiblingsIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent
    }

    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        if let Some(node) = self.get(id) {
            Box::new(node.children.iter().copied())
        } else {
            Box::new(std::iter::empty())
        }
    }

    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        Ancestors::new(self, start)
    }

    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        DescendantsWithDepth::new(self, root)
    }

    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        if let Some(parent_id) = self.parent(id) {
            Box::new(
                self.children(parent_id)
                    .filter(move |&child_id| child_id != id),
            )
        } else {
            Box::new(std::iter::empty())
        }
    }
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    println!("🌳 FLUI Tree - Basic Traversal Examples\n");

    // Build a sample tree:
    //
    //           App
    //          / | \
    //       Home Feed Profile
    //        |     |
    //      Hero  Posts
    //             / \
    //          Post1 Post2

    let mut tree = DemoTree::new();

    let app = tree.insert("App", None);
    let home = tree.insert("Home", Some(app));
    let feed = tree.insert("Feed", Some(app));
    let _profile = tree.insert("Profile", Some(app));
    let hero = tree.insert("Hero", Some(home));
    let posts = tree.insert("Posts", Some(feed));
    let post1 = tree.insert("Post1", Some(posts));
    let post2 = tree.insert("Post2", Some(posts));

    println!("Tree structure:");
    println!("           App");
    println!("          / | \\");
    println!("       Home Feed Profile");
    println!("        |     |");
    println!("      Hero  Posts");
    println!("             / \\");
    println!("          Post1 Post2");
    println!();

    // ========================================================================
    // ANCESTOR ITERATION
    // ========================================================================

    println!("📍 Ancestors of Post1:");
    let ancestors: Vec<_> = tree
        .ancestors(post1)
        .map(|id| tree.name(id).unwrap())
        .collect();
    println!("   Path to root: {:?}", ancestors);
    println!();

    // ========================================================================
    // DESCENDANT ITERATION
    // ========================================================================

    println!("📍 Descendants of App (pre-order DFS):");
    let descendants: Vec<_> = tree
        .descendants(app)
        .map(|(id, _depth)| tree.name(id).unwrap())
        .collect();
    println!("   {:?}", descendants);
    println!();

    println!("📍 Descendants with depth:");
    for (id, depth) in tree.descendants(app) {
        let indent = "  ".repeat(depth);
        println!("   {}{}", indent, tree.name(id).unwrap());
    }
    println!();

    // ========================================================================
    // DEPTH-FIRST TRAVERSAL (Pre vs Post order)
    // ========================================================================

    println!("📍 Pre-order DFS (parent before children):");
    let pre_order: Vec<_> = DepthFirstIter::pre_order(&tree, app)
        .map(|id| tree.name(id).unwrap())
        .collect();
    println!("   {:?}", pre_order);

    println!("📍 Post-order DFS (children before parent):");
    let post_order: Vec<_> = DepthFirstIter::post_order(&tree, app)
        .map(|id| tree.name(id).unwrap())
        .collect();
    println!("   {:?}", post_order);
    println!();

    // ========================================================================
    // BREADTH-FIRST TRAVERSAL
    // ========================================================================

    println!("📍 Breadth-first (level order):");
    let bfs: Vec<_> = BreadthFirstIter::new(&tree, app)
        .map(|id| tree.name(id).unwrap())
        .collect();
    println!("   {:?}", bfs);
    println!();

    // ========================================================================
    // TREE NAVIGATION UTILITIES
    // ========================================================================

    println!("📍 Navigation utilities:");
    // Find root by walking ancestors
    let root_of_post1 = tree.ancestors(post1).last().unwrap_or(post1);
    println!("   Root of Post1: {}", tree.name(root_of_post1).unwrap());
    println!("   Depth of Post1: {}", tree.depth(post1));
    println!("   Is App a root? {}", tree.is_root(app));
    println!("   Is Post1 a leaf? {}", tree.is_leaf(post1));
    // Check if post1 is a descendant of feed
    let is_descendant = tree.ancestors(post1).any(|id| id == feed);
    println!("   Is Post1 descendant of Feed? {}", is_descendant);
    // Count subtree size
    let subtree_size = tree.descendants(feed).count();
    println!("   Subtree size of Feed: {}", subtree_size);
    println!();

    // ========================================================================
    // LOWEST COMMON ANCESTOR
    // ========================================================================

    println!("📍 Lowest Common Ancestor:");
    if let Some(lca) = tree.lowest_common_ancestor(post1, hero) {
        println!("   LCA of Post1 and Hero: {}", tree.name(lca).unwrap());
    }
    if let Some(lca) = tree.lowest_common_ancestor(post1, post2) {
        println!("   LCA of Post1 and Post2: {}", tree.name(lca).unwrap());
    }
    println!();

    // ========================================================================
    // VISITOR PATTERN
    // ========================================================================

    println!("📍 Visitor pattern:");

    // Collect all nodes
    let mut collector = CollectVisitor::new();
    visit_depth_first(&tree, app, &mut collector);
    println!("   Collected {} nodes", collector.collected.len());

    // Find max depth
    let mut depth_finder = MaxDepthVisitor::new();
    visit_depth_first(&tree, app, &mut depth_finder);
    println!("   Maximum depth: {}", depth_finder.max_depth);

    // Custom visitor using for_each
    println!("   Visiting with custom printer:");
    for_each(&tree, app, |id, depth| {
        let indent = "  ".repeat(depth);
        println!("   {}→ {}", indent, tree.name(id).unwrap());
    });

    println!();
    println!("✅ All traversals complete!");
}
