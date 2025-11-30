//! Advanced Type System Examples for FLUI Tree
//!
//! This file demonstrates cutting-edge Rust type system features used in FLUI Tree:
//! - GAT (Generic Associated Types)
//! - HRTB (Higher-Rank Trait Bounds)
//! - Const Generics
//! - Associated Constants
//! - Sealed Traits
//! - Typestate Pattern
//! - Never Type (!!)

use flui_foundation::ElementId;
use flui_tree::{
    arity::{Arity, BoundedChildren, Exact, SmartChildren, TypedChildren, Variable},
    prelude::*,
    visit_depth_first, visit_depth_first_typed, visit_stateful,
    visitor::{
        states, CollectVisitor, FindVisitor, StatefulVisitor, TreeVisitor, TypedVisitor,
        VisitorResult,
    },
    TreeNav, TreeNavExt, TreeRead, TreeReadExt,
};
use std::collections::HashMap;
use std::marker::PhantomData;

// ============================================================================
// EXAMPLE TREE IMPLEMENTATION WITH ADVANCED FEATURES
// ============================================================================

#[derive(Debug, Clone)]
struct ExampleNode {
    name: String,
    node_type: NodeType,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeType {
    Container,
    Widget,
    Text,
    Image,
}

impl ExampleNode {
    fn new(name: String, node_type: NodeType) -> Self {
        Self {
            name,
            node_type,
            parent: None,
            children: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }
}

struct ExampleTree {
    nodes: HashMap<ElementId, ExampleNode>,
    next_id: usize,
}

// Sealed trait implementations (required for safety)
impl flui_tree::traits::read::sealed::Sealed for ExampleTree {}
impl flui_tree::traits::nav::sealed::Sealed for ExampleTree {}

impl ExampleTree {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            next_id: 1,
        }
    }

    fn insert(&mut self, node: ExampleNode, parent: Option<ElementId>) -> ElementId {
        let id = ElementId::new(self.next_id);
        self.next_id += 1;

        let mut node = node;
        node.parent = parent;

        if let Some(parent_id) = parent {
            if let Some(parent_node) = self.nodes.get_mut(&parent_id) {
                parent_node.children.push(id);
            }
        }

        self.nodes.insert(id, node);
        id
    }
}

// TreeRead implementation with GAT and HRTB support
impl TreeRead for ExampleTree {
    type Node = ExampleNode;

    // GAT for flexible iterator types
    type NodeIter<'a>
        = impl Iterator<Item = ElementId> + 'a
    where
        Self: 'a;

    // Performance tuning constants
    const DEFAULT_CAPACITY: usize = 128;
    const INLINE_THRESHOLD: usize = 32;
    const CACHE_LINE_SIZE: usize = 64;

    fn get(&self, id: ElementId) -> Option<&ExampleNode> {
        self.nodes.get(&id)
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    // GAT-based iterator implementation
    fn node_ids(&self) -> Self::NodeIter<'_> {
        self.nodes.keys().copied()
    }
}

// TreeNav implementation with GAT iterators
impl TreeNav for ExampleTree {
    // GAT for flexible iterator types
    type ChildrenIter<'a>
        = impl Iterator<Item = ElementId> + 'a
    where
        Self: 'a;
    type AncestorsIter<'a> = flui_tree::AncestorIterator<'a, Self>;
    type DescendantsIter<'a> = flui_tree::DescendantsIterator<'a, Self>;
    type SiblingsIter<'a>
        = impl Iterator<Item = ElementId> + 'a
    where
        Self: 'a;

    // Performance constants for optimization
    const MAX_DEPTH: usize = 128;
    const AVG_CHILDREN: usize = 6;

    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.nodes.get(&id)?.parent
    }

    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        if let Some(node) = self.nodes.get(&id) {
            node.children.iter().copied()
        } else {
            [].iter().copied()
        }
    }

    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        flui_tree::AncestorIterator::new(self, start)
    }

    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        flui_tree::DescendantsIterator::new(self, root)
    }

    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        if let Some(parent_id) = self.parent(id) {
            self.children(parent_id)
                .filter(move |&child_id| child_id != id)
        } else {
            [].iter().copied()
        }
    }
}

// ============================================================================
// EXAMPLE 1: HRTB (Higher-Rank Trait Bounds) USAGE
// ============================================================================

/// Demonstrates HRTB predicates that work with any lifetime
fn hrtb_examples() {
    println!("=== HRTB (Higher-Rank Trait Bounds) Examples ===\n");

    let mut tree = create_example_tree();
    let root = tree.nodes.keys().next().copied().unwrap();

    // HRTB predicate that works with any lifetime
    let widget_nodes = tree.find_node_where(|node| {
        // This closure works with any lifetime thanks to HRTB
        node.node_type == NodeType::Widget
    });

    println!("Found widget node: {:?}", widget_nodes);

    // HRTB-compatible filtering across the entire tree
    let containers = tree.collect_nodes_where(|node| {
        node.node_type == NodeType::Container && !node.children.is_empty()
    });

    println!("Container nodes: {:?}", containers);

    // HRTB predicate for navigation
    let deep_nodes = tree.find_descendant_where(root, |node| {
        node.name.len() > 10 // Works with any string lifetime
    });

    println!("Deep node with long name: {:?}", deep_nodes);

    // Complex HRTB predicate combining multiple conditions
    let complex_filter = |node: &ExampleNode| {
        node.node_type != NodeType::Text
            && node.metadata.contains_key("important")
            && node.name.starts_with("my_")
    };

    let filtered = tree.filter_slice(&tree.node_ids().collect::<Vec<_>>(), complex_filter);

    println!("Complex filtered nodes: {} found\n", filtered.len());
}

// ============================================================================
// EXAMPLE 2: GAT (Generic Associated Types) USAGE
// ============================================================================

/// Custom typed visitor using GAT for flexible result collection
struct NodeTypeCollector {
    target_type: NodeType,
}

impl flui_tree::visitor::sealed::Sealed for NodeTypeCollector {}

impl TypedVisitor<ExampleTree> for NodeTypeCollector {
    // GAT allows flexible item types
    type Item<'a>
        = (ElementId, String)
    where
        ExampleTree: 'a;

    // GAT allows different collection types
    type Collection<'a>
        = Vec<(ElementId, String)>
    where
        ExampleTree: 'a;

    fn visit_typed<'a>(
        &'a mut self,
        tree: &'a ExampleTree,
        id: ElementId,
        _depth: usize,
    ) -> (VisitorResult, Option<Self::Item<'a>>) {
        if let Some(node) = tree.get(id) {
            if node.node_type == self.target_type {
                let item = (id, node.name.clone());
                (VisitorResult::Continue, Some(item))
            } else {
                (VisitorResult::Continue, None)
            }
        } else {
            (VisitorResult::Continue, None)
        }
    }

    fn create_collection<'a>(&self) -> Self::Collection<'a> {
        Vec::with_capacity(Self::EXPECTED_ITEMS)
    }

    const EXPECTED_ITEMS: usize = 16;
}

fn gat_examples() {
    println!("=== GAT (Generic Associated Types) Examples ===\n");

    let tree = create_example_tree();
    let root = tree.nodes.keys().next().copied().unwrap();

    // GAT-based typed visitor for collecting specific node types
    let mut widget_collector = NodeTypeCollector {
        target_type: NodeType::Widget,
    };

    let widgets = visit_depth_first_typed(&tree, root, &mut widget_collector);

    println!("Collected widgets using GAT:");
    for (id, name) in widgets {
        println!("  {:?}: {}", id, name);
    }

    // GAT allows different iterator types based on tree structure
    let children: Vec<_> = tree.children(root).collect();
    println!("\nGAT-based children iterator: {:?}", children);

    // Flexible GAT-based ancestor iteration
    if let Some(leaf) = tree.nodes.keys().last() {
        let ancestors: Vec<_> = tree.ancestors(*leaf).collect();
        println!("GAT-based ancestors: {:?}", ancestors);
    }

    println!();
}

// ============================================================================
// EXAMPLE 3: CONST GENERICS OPTIMIZATION
// ============================================================================

fn const_generics_examples() {
    println!("=== Const Generics Optimization Examples ===\n");

    let tree = create_example_tree();
    let root = tree.nodes.keys().next().copied().unwrap();

    // Const generic optimization for stack allocation
    let mut small_collector = CollectVisitor::<16>::new(); // 16 inline elements
    visit_depth_first::<_, _, 32>(&tree, root, &mut small_collector); // 32-deep stack

    println!(
        "Small collector (16 inline): {} nodes",
        small_collector.collected.len()
    );

    // Larger const generic for bigger trees
    let mut large_collector = CollectVisitor::<64>::new(); // 64 inline elements
    visit_depth_first::<_, _, 128>(&tree, root, &mut large_collector); // 128-deep stack

    println!(
        "Large collector (64 inline): {} nodes",
        large_collector.collected.len()
    );

    // Const generic arity validation
    let children_slice: Vec<_> = tree.children(root).collect();

    if children_slice.len() >= 2 {
        let slice = &children_slice[0..2];
        let exact_two = Exact::<2>::from_slice(slice);
        let (first, second) = exact_two.pair();
        println!("Const generic Exact<2>: {:?}, {:?}", first, second);
    }

    // Bounded children with const generic validation
    let all_children: Vec<_> = tree.children(root).collect();
    if let Some(bounded) = BoundedChildren::<_, 1, 10>::new(&all_children) {
        println!(
            "Bounded children (1-10): {} items within bounds",
            bounded.len()
        );
        println!(
            "Is at minimum: {}, Is at maximum: {}",
            bounded.is_at_min(),
            bounded.is_at_max()
        );
    }

    println!();
}

// ============================================================================
// EXAMPLE 4: TYPESTATE PATTERN WITH COMPILE-TIME SAFETY
// ============================================================================

fn typestate_examples() {
    println!("=== Typestate Pattern Examples ===\n");

    let tree = create_example_tree();
    let root = tree.nodes.keys().next().copied().unwrap();

    // Typestate pattern ensures correct visitor lifecycle
    let mut visit_count = 0;

    // Compile-time guarantee: Initial → Started → Finished
    let finished_visitor = visit_stateful(&tree, root, |_id, depth| {
        visit_count += 1;
        if depth > 5 {
            VisitorResult::SkipChildren
        } else {
            VisitorResult::Continue
        }
    });

    // Can only access final data after completion (compile-time enforced)
    let _final_data = finished_visitor.into_data();

    println!("Typestate visitor completed, visited {} nodes", visit_count);

    // Demonstrates compile-time state tracking
    let initial_visitor = StatefulVisitor::new(vec![String::new()]);
    let started_visitor = initial_visitor.start();
    // let finished = started_visitor.finish(); // Only available after start()

    println!("Typestate pattern ensures correct lifecycle transitions\n");
}

// ============================================================================
// EXAMPLE 5: ADVANCED ACCESSOR TYPES
// ============================================================================

fn advanced_accessor_examples() {
    println!("=== Advanced Accessor Types Examples ===\n");

    let tree = create_example_tree();
    let root = tree.nodes.keys().next().copied().unwrap();

    let children_vec: Vec<_> = tree.children(root).collect();

    // Smart accessor with adaptive allocation strategy
    let smart = SmartChildren::new(&children_vec);
    println!("Smart accessor strategy: {:?}", smart.strategy());

    // Batch processing with HRTB
    let processed = smart.batch_process(|&id| format!("processed_{}", id.get()));
    println!("Batch processed {} items", processed.len());

    // Type-aware accessor with optimization hints
    let typed = TypedChildren::new(&children_vec);
    println!("Typed accessor type info: {:?}", typed.type_info());

    // HRTB-compatible type-aware processing
    let type_results = typed
        .process_typed(|&id, type_info| (id, type_info, format!("{}_{:?}", id.get(), type_info)));

    println!("Type-aware processing: {} results", type_results.len());

    println!();
}

// ============================================================================
// EXAMPLE 6: PERFORMANCE OPTIMIZATION WITH ASSOCIATED CONSTANTS
// ============================================================================

fn performance_examples() {
    println!("=== Performance Optimization Examples ===\n");

    let tree = create_example_tree();

    // Access performance constants for optimization
    println!("TreeRead performance constants:");
    println!("  Default capacity: {}", ExampleTree::DEFAULT_CAPACITY);
    println!("  Inline threshold: {}", ExampleTree::INLINE_THRESHOLD);
    println!("  Cache line size: {}", ExampleTree::CACHE_LINE_SIZE);

    println!("\nTreeNav performance constants:");
    println!("  Max depth: {}", ExampleTree::MAX_DEPTH);
    println!("  Average children: {}", ExampleTree::AVG_CHILDREN);

    // Use constants for optimal allocation
    let mut optimized_vec = Vec::with_capacity(ExampleTree::DEFAULT_CAPACITY);
    for id in tree.node_ids() {
        if optimized_vec.len() < ExampleTree::INLINE_THRESHOLD {
            optimized_vec.push(id);
        }
    }

    println!(
        "Optimized allocation used constants for {} items",
        optimized_vec.len()
    );

    println!();
}

// ============================================================================
// EXAMPLE 7: NEVER TYPE SAFETY
// ============================================================================

fn never_type_examples() {
    println!("=== Never Type Safety Examples ===\n");

    // Demonstrate impossible operations with leaf nodes
    let leaf_children: &[ElementId] = &[];
    let leaf_accessor = flui_tree::arity::Leaf::from_slice(leaf_children);

    // This would be a compile-time error if uncommented:
    // let first = leaf_accessor.first_impossible(); // Returns !

    println!("Leaf accessor guarantees no children exist");
    println!("Impossible operations return never type (!) for safety");

    // Never type prevents impossible states at compile time
    println!("Never type eliminates undefined behavior\n");
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn create_example_tree() -> ExampleTree {
    let mut tree = ExampleTree::new();

    // Create a sample tree structure
    let root = tree.insert(
        ExampleNode::new("root_container".to_string(), NodeType::Container)
            .with_metadata("important", "true"),
        None,
    );

    let header = tree.insert(
        ExampleNode::new("header_widget".to_string(), NodeType::Widget)
            .with_metadata("position", "top"),
        Some(root),
    );

    let content = tree.insert(
        ExampleNode::new("content_container".to_string(), NodeType::Container)
            .with_metadata("layout", "flex"),
        Some(root),
    );

    let title = tree.insert(
        ExampleNode::new("title_text".to_string(), NodeType::Text),
        Some(header),
    );

    let body = tree.insert(
        ExampleNode::new("body_text_with_long_name".to_string(), NodeType::Text),
        Some(content),
    );

    let image = tree.insert(
        ExampleNode::new("my_hero_image".to_string(), NodeType::Image)
            .with_metadata("important", "true"),
        Some(content),
    );

    let footer = tree.insert(
        ExampleNode::new("footer_widget".to_string(), NodeType::Widget),
        Some(root),
    );

    tree
}

// ============================================================================
// MAIN FUNCTION
// ============================================================================

fn main() {
    println!("FLUI Tree - Advanced Type System Features Demo");
    println!("==============================================\n");

    hrtb_examples();
    gat_examples();
    const_generics_examples();
    typestate_examples();
    advanced_accessor_examples();
    performance_examples();
    never_type_examples();

    println!("All advanced type system features demonstrated!");
    println!("\nKey benefits:");
    println!("✓ Compile-time safety with GAT and HRTB");
    println!("✓ Zero-cost abstractions with const generics");
    println!("✓ Performance optimization via associated constants");
    println!("✓ Type safety with sealed traits and never type");
    println!("✓ Ergonomic APIs with extension traits");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hrtb_predicates() {
        let tree = create_example_tree();

        // Test HRTB predicate
        let widgets = tree.collect_nodes_where(|node| node.node_type == NodeType::Widget);

        assert!(!widgets.is_empty());
    }

    #[test]
    fn test_gat_iterators() {
        let tree = create_example_tree();
        let root = tree.nodes.keys().next().copied().unwrap();

        // Test GAT-based iteration
        let children_count = tree.children(root).count();
        assert!(children_count > 0);
    }

    #[test]
    fn test_const_generic_visitors() {
        let tree = create_example_tree();
        let root = tree.nodes.keys().next().copied().unwrap();

        let mut collector = CollectVisitor::<8>::new();
        visit_depth_first::<_, _, 16>(&tree, root, &mut collector);

        assert!(!collector.collected.is_empty());
    }

    #[test]
    fn test_typestate_pattern() {
        let tree = create_example_tree();
        let root = tree.nodes.keys().next().copied().unwrap();

        let finished = visit_stateful(&tree, root, |_, _| VisitorResult::Continue);
        let _data = finished.into_data(); // Only available after finish

        // If this compiles, typestate is working correctly
    }
}
