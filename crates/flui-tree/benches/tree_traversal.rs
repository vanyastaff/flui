//! Benchmarks for tree traversal operations.
//!
//! Run with: cargo bench -p flui-tree

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use flui_foundation::{ElementId, Slot};
use flui_tree::{
    iter::{BreadthFirstIter, DepthFirstIter, DepthFirstOrder},
    prelude::*,
    visitor::{visit_breadth_first, visit_depth_first, CollectVisitor, CountVisitor},
};

// ============================================================================
// TEST TREE IMPLEMENTATION
// ============================================================================

struct BenchNode {
    parent: Option<ElementId>,
    children: Vec<ElementId>,
}

struct BenchTree {
    nodes: Vec<Option<BenchNode>>,
}

impl BenchTree {
    fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    fn insert(&mut self, parent: Option<ElementId>) -> ElementId {
        let id = ElementId::new(self.nodes.len() as u64 + 1);
        self.nodes.push(Some(BenchNode {
            parent,
            children: Vec::new(),
        }));

        if let Some(parent_id) = parent {
            if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                parent_node.children.push(id);
            }
        }

        id
    }

    /// Creates a balanced tree with given branching factor and depth.
    fn create_balanced(branching: usize, depth: usize) -> (Self, ElementId) {
        let mut tree = Self::new();
        let root = tree.insert(None);

        fn build_subtree(
            tree: &mut BenchTree,
            parent: ElementId,
            branching: usize,
            remaining_depth: usize,
        ) {
            if remaining_depth == 0 {
                return;
            }

            for _ in 0..branching {
                let child = tree.insert(Some(parent));
                build_subtree(tree, child, branching, remaining_depth - 1);
            }
        }

        build_subtree(&mut tree, root, branching, depth);
        (tree, root)
    }

    /// Creates a linear (deep) tree.
    fn create_linear(depth: usize) -> (Self, ElementId) {
        let mut tree = Self::new();
        let root = tree.insert(None);
        let mut current = root;

        for _ in 0..depth {
            current = tree.insert(Some(current));
        }

        (tree, root)
    }

    /// Creates a wide (shallow) tree.
    fn create_wide(width: usize) -> (Self, ElementId) {
        let mut tree = Self::new();
        let root = tree.insert(None);

        for _ in 0..width {
            tree.insert(Some(root));
        }

        (tree, root)
    }
}

impl TreeRead for BenchTree {
    type Node = BenchNode;

    fn get(&self, id: ElementId) -> Option<&BenchNode> {
        self.nodes.get(id.get() as usize - 1)?.as_ref()
    }

    fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }
}

impl TreeNav for BenchTree {
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
// BENCHMARKS
// ============================================================================

fn bench_ancestors(c: &mut Criterion) {
    let mut group = c.benchmark_group("ancestors");

    for depth in [10, 50, 100, 500, 1000] {
        let (tree, _root) = BenchTree::create_linear(depth);
        // Get the deepest node
        let deepest = ElementId::new(depth as u64 + 1);

        group.throughput(Throughput::Elements(depth as u64));

        group.bench_with_input(
            BenchmarkId::new("iterator", depth),
            &(&tree, deepest),
            |b, (tree, id)| b.iter(|| black_box(tree.ancestors(*id).count())),
        );
    }

    group.finish();
}

fn bench_descendants(c: &mut Criterion) {
    let mut group = c.benchmark_group("descendants");

    for size in [100, 1000, 10000] {
        // Binary tree with log2(size) depth
        let depth = (size as f64).log2() as usize;
        let (tree, root) = BenchTree::create_balanced(2, depth);
        let actual_size = tree.len();

        group.throughput(Throughput::Elements(actual_size as u64));

        group.bench_with_input(
            BenchmarkId::new("iterator", actual_size),
            &(&tree, root),
            |b, (tree, root)| b.iter(|| black_box(tree.descendants(*root).count())),
        );

        group.bench_with_input(
            BenchmarkId::new("visitor", actual_size),
            &(&tree, root),
            |b, (tree, root)| {
                b.iter(|| {
                    let mut visitor = CountVisitor::new();
                    visit_depth_first(*tree, *root, &mut visitor);
                    black_box(visitor.count)
                })
            },
        );
    }

    group.finish();
}

fn bench_dfs_vs_bfs(c: &mut Criterion) {
    let mut group = c.benchmark_group("dfs_vs_bfs");

    // Use a balanced tree for fair comparison
    let (tree, root) = BenchTree::create_balanced(4, 6); // 4^6 = 4096 nodes
    let size = tree.len();

    group.throughput(Throughput::Elements(size as u64));

    group.bench_function("dfs_pre_order", |b| {
        b.iter(|| black_box(DepthFirstIter::pre_order(&tree, root).count()))
    });

    group.bench_function("dfs_post_order", |b| {
        b.iter(|| black_box(DepthFirstIter::post_order(&tree, root).count()))
    });

    group.bench_function("bfs", |b| {
        b.iter(|| black_box(BreadthFirstIter::new(&tree, root).count()))
    });

    group.bench_function("descendants_iterator", |b| {
        b.iter(|| black_box(tree.descendants(root).count()))
    });

    group.finish();
}

fn bench_tree_shapes(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_shapes");

    // Deep tree (1000 nodes in a line)
    let (deep_tree, deep_root) = BenchTree::create_linear(999);

    // Wide tree (1000 children of root)
    let (wide_tree, wide_root) = BenchTree::create_wide(999);

    // Balanced tree (~1000 nodes)
    let (balanced_tree, balanced_root) = BenchTree::create_balanced(3, 6); // 3^6 = 729 nodes

    group.throughput(Throughput::Elements(1000));

    group.bench_function("deep_tree_descendants", |b| {
        b.iter(|| black_box(deep_tree.descendants(deep_root).count()))
    });

    group.bench_function("wide_tree_descendants", |b| {
        b.iter(|| black_box(wide_tree.descendants(wide_root).count()))
    });

    group.bench_function("balanced_tree_descendants", |b| {
        b.iter(|| black_box(balanced_tree.descendants(balanced_root).count()))
    });

    // Ancestors from deepest node
    let deep_leaf = ElementId::new(1000);

    group.bench_function("deep_tree_ancestors", |b| {
        b.iter(|| black_box(deep_tree.ancestors(deep_leaf).count()))
    });

    group.finish();
}

fn bench_navigation(c: &mut Criterion) {
    let mut group = c.benchmark_group("navigation");

    let (tree, root) = BenchTree::create_balanced(4, 5); // ~1000 nodes
    let size = tree.len();

    // Get a mid-depth node for testing
    let mid_node = ElementId::new(50);

    group.throughput(Throughput::Elements(1));

    group.bench_function("parent", |b| b.iter(|| black_box(tree.parent(mid_node))));

    group.bench_function("children", |b| {
        b.iter(|| black_box(tree.children(root).len()))
    });

    group.bench_function("is_descendant", |b| {
        let leaf = ElementId::new(size as u64);
        b.iter(|| black_box(tree.is_descendant(leaf, root)))
    });

    group.bench_function("depth", |b| {
        let leaf = ElementId::new(size as u64);
        b.iter(|| black_box(tree.depth(leaf)))
    });

    group.bench_function("lowest_common_ancestor", |b| {
        let node_a = ElementId::new(100);
        let node_b = ElementId::new(200);
        b.iter(|| black_box(tree.lowest_common_ancestor(node_a, node_b)))
    });

    group.finish();
}

fn bench_collect(c: &mut Criterion) {
    let mut group = c.benchmark_group("collect");

    for size in [100, 1000, 10000] {
        let depth = (size as f64).log2() as usize;
        let (tree, root) = BenchTree::create_balanced(2, depth);
        let actual_size = tree.len();

        group.throughput(Throughput::Elements(actual_size as u64));

        group.bench_with_input(
            BenchmarkId::new("iterator_collect", actual_size),
            &(&tree, root),
            |b, (tree, root)| {
                b.iter(|| {
                    let v: Vec<_> = tree.descendants(*root).collect();
                    black_box(v.len())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("visitor_collect", actual_size),
            &(&tree, root),
            |b, (tree, root)| {
                b.iter(|| {
                    let mut visitor = CollectVisitor::with_capacity(actual_size);
                    visit_depth_first(*tree, *root, &mut visitor);
                    black_box(visitor.collected.len())
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_ancestors,
    bench_descendants,
    bench_dfs_vs_bfs,
    bench_tree_shapes,
    bench_navigation,
    bench_collect,
);

criterion_main!(benches);
