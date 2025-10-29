//! Benchmarks for Element enum performance
//!
//! Measures the performance improvements from migrating from Box<dyn DynElement>
//! to enum Element.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use flui_core::element::{
    ComponentElement, Element, ElementId, ElementTree, InheritedElement, StatefulElement,
};
use flui_core::widget::BoxedWidget;

// Mock widget for benchmarking
#[derive(Debug, Clone)]
struct BenchWidget {
    id: usize,
}

impl flui_core::DynWidget for BenchWidget {
    fn key(&self) -> Option<flui_core::KeyRef> {
        None
    }

    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Self>()
    }

    fn can_update(&self, other: &dyn flui_core::DynWidget) -> bool {
        other.type_id() == self.type_id()
    }

    fn debug_name(&self) -> &'static str {
        "BenchWidget"
    }
}

/// Benchmark: Element Tree Insert
///
/// Measures time to insert elements into the tree
fn bench_element_tree_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("element_tree_insert");

    for size in [100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut tree = ElementTree::new();
                for i in 0..size {
                    let widget: BoxedWidget = Box::new(BenchWidget { id: i });
                    let element = Element::Component(ComponentElement::new(widget));
                    black_box(tree.insert(element));
                }
            });
        });
    }

    group.finish();
}

/// Benchmark: Element Tree Access
///
/// Measures time to access elements by ID (this is the key improvement!)
fn bench_element_tree_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("element_tree_access");

    for size in [100, 1000, 10000] {
        // Setup: create tree with elements
        let mut tree = ElementTree::new();
        let ids: Vec<ElementId> = (0..size)
            .map(|i| {
                let widget: BoxedWidget = Box::new(BenchWidget { id: i });
                let element = Element::Component(ComponentElement::new(widget));
                tree.insert(element)
            })
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                for &id in &ids {
                    black_box(tree.get(id));
                }
            });
        });
    }

    group.finish();
}

/// Benchmark: Element Dispatch (Pattern Matching)
///
/// Measures time for enum variant dispatch via match statements
/// This is the PRIMARY benefit of enum vs vtable!
fn bench_element_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("element_dispatch");

    // Create different element variants
    let elements = vec![
        Element::Component(ComponentElement::new(Box::new(BenchWidget { id: 0 }))),
        Element::Stateful(StatefulElement::new(Box::new(BenchWidget { id: 1 }))),
        Element::Inherited(InheritedElement::new(Box::new(BenchWidget { id: 2 }))),
    ];

    group.bench_function("match_variant_check", |b| {
        b.iter(|| {
            for element in &elements {
                // Simulate common operations that dispatch based on variant
                black_box(match element {
                    Element::Component(_) => 1,
                    Element::Stateful(_) => 2,
                    Element::Inherited(_) => 3,
                    Element::Render(_) => 4,
                    Element::ParentData(_) => 5,
                });
            }
        });
    });

    group.bench_function("is_predicate_check", |b| {
        b.iter(|| {
            for element in &elements {
                black_box(element.is_component());
                black_box(element.is_stateful());
                black_box(element.is_inherited());
            }
        });
    });

    group.finish();
}

/// Benchmark: Element Method Calls
///
/// Measures time for calling common methods on elements
fn bench_element_methods(c: &mut Criterion) {
    let mut group = c.benchmark_group("element_methods");

    let mut tree = ElementTree::new();
    let ids: Vec<ElementId> = (0..1000)
        .map(|i| {
            let widget: BoxedWidget = Box::new(BenchWidget { id: i });
            let element = Element::Component(ComponentElement::new(widget));
            tree.insert(element)
        })
        .collect();

    group.bench_function("parent_access", |b| {
        b.iter(|| {
            for &id in &ids {
                if let Some(element) = tree.get(id) {
                    black_box(element.parent());
                }
            }
        });
    });

    group.bench_function("lifecycle_check", |b| {
        b.iter(|| {
            for &id in &ids {
                if let Some(element) = tree.get(id) {
                    black_box(element.lifecycle());
                }
            }
        });
    });

    group.bench_function("is_dirty_check", |b| {
        b.iter(|| {
            for &id in &ids {
                if let Some(element) = tree.get(id) {
                    black_box(element.is_dirty());
                }
            }
        });
    });

    group.finish();
}

/// Benchmark: Element Tree Traversal
///
/// Measures time to traverse element trees
fn bench_element_tree_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("element_tree_traversal");

    // Create a tree with 1000 elements
    let mut tree = ElementTree::new();
    let _ids: Vec<ElementId> = (0..1000)
        .map(|i| {
            let widget: BoxedWidget = Box::new(BenchWidget { id: i });
            let element = Element::Component(ComponentElement::new(widget));
            tree.insert(element)
        })
        .collect();

    group.bench_function("visit_all_elements", |b| {
        b.iter(|| {
            let mut count = 0;
            tree.visit_all_elements(|_id, _element| {
                count += 1;
            });
            black_box(count);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_element_tree_insert,
    bench_element_tree_access,
    bench_element_dispatch,
    bench_element_methods,
    bench_element_tree_traversal,
);

criterion_main!(benches);
