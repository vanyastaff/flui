//! Basic tree traversal examples.
//!
//! This example demonstrates the core iteration capabilities of flui-tree.
//!
//! Run with: cargo run --example basic_traversal

use flui_foundation::{ElementId, Slot};
use flui_tree::{
    iter::{BreadthFirstIter, DepthFirstIter, DepthFirstOrder},
    prelude::*,
    visitor::{visit_breadth_first, visit_depth_first, CollectVisitor, MaxDepthVisitor},
};

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
    type Node = DemoNode;

    fn get(&self, id: ElementId) -> Option<&DemoNode> {
        self.nodes.get(id.get() as usize - 1)?.as_ref()
    }

    fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }
}

impl TreeNav for DemoTree {
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent
    }

    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get(id).map(|n| n.children.as_slice()).unwrap_or(&[])
    }

    fn slot(&self, _id: ElementId) -> Option<Slot> {
        None
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
    let profile = tree.insert("Profile", Some(app));
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
        .map(|id| tree.name(id).unwrap())
        .collect();
    println!("   {:?}", descendants);
    println!();

    println!("📍 Descendants with depth:");
    for (id, depth) in tree.descendants_with_depth(app) {
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
    println!("   Root of Post1: {}", tree.name(tree.root(post1)).unwrap());
    println!("   Depth of Post1: {}", tree.depth(post1));
    println!("   Is App a root? {}", tree.is_root(app));
    println!("   Is Post1 a leaf? {}", tree.is_leaf(post1));
    println!(
        "   Is Post1 descendant of Feed? {}",
        tree.is_descendant(post1, feed)
    );
    println!("   Subtree size of Feed: {}", tree.subtree_size(feed));
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

    // Custom visitor
    struct NamePrinter<'a> {
        tree: &'a DemoTree,
    }

    impl TreeVisitor for NamePrinter<'_> {
        fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
            let indent = "  ".repeat(depth);
            println!("   {}→ {}", indent, self.tree.name(id).unwrap());
            VisitorResult::Continue
        }
    }

    println!("   Visiting with custom printer:");
    let mut printer = NamePrinter { tree: &tree };
    visit_depth_first(&tree, app, &mut printer);

    println!();
    println!("✅ All traversals complete!");
}
