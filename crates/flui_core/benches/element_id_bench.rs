//! Benchmark: ElementId with NonZeroUsize
//!
//! Compares performance of:
//! 1. Old: type alias + sentinel value (usize::MAX)
//! 2. New: NonZeroUsize with Option (niche optimization)
//!
//! Run with: cargo bench --bench element_id_bench

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use flui_core::ElementId;

// ============================================================================
// Old Implementation (for comparison)
// ============================================================================

type OldElementId = usize;
const INVALID_ELEMENT_ID: OldElementId = usize::MAX;

#[derive(Clone)]
struct OldComponentElement {
    child: OldElementId,  // Uses sentinel value
}

impl OldComponentElement {
    fn new() -> Self {
        Self {
            child: INVALID_ELEMENT_ID,
        }
    }

    fn has_child(&self) -> bool {
        self.child != INVALID_ELEMENT_ID
    }

    fn child(&self) -> Option<OldElementId> {
        if self.child == INVALID_ELEMENT_ID {
            None
        } else {
            Some(self.child)
        }
    }

    fn set_child(&mut self, id: OldElementId) {
        self.child = id;
    }

    fn clear_child(&mut self) {
        self.child = INVALID_ELEMENT_ID;
    }
}

// ============================================================================
// New Implementation
// ============================================================================

#[derive(Clone)]
struct NewComponentElement {
    child: Option<ElementId>,  // Niche-optimized
}

impl NewComponentElement {
    fn new() -> Self {
        Self {
            child: None,
        }
    }

    fn has_child(&self) -> bool {
        self.child.is_some()
    }

    fn child(&self) -> Option<ElementId> {
        self.child
    }

    fn set_child(&mut self, id: ElementId) {
        self.child = Some(id);
    }

    fn clear_child(&mut self) {
        self.child = None;
    }
}

// ============================================================================
// Benchmarks
// ============================================================================

fn bench_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("element_creation");

    group.bench_function("old_sentinel", |b| {
        b.iter(|| {
            let elem = OldComponentElement::new();
            black_box(elem);
        });
    });

    group.bench_function("new_option", |b| {
        b.iter(|| {
            let elem = NewComponentElement::new();
            black_box(elem);
        });
    });

    group.finish();
}

fn bench_has_child_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("has_child_check");

    // With child
    let old_with = {
        let mut elem = OldComponentElement::new();
        elem.set_child(42);
        elem
    };

    let new_with = {
        let mut elem = NewComponentElement::new();
        elem.set_child(ElementId::new(42));
        elem
    };

    group.bench_function("old_sentinel_with_child", |b| {
        b.iter(|| {
            black_box(old_with.has_child());
        });
    });

    group.bench_function("new_option_with_child", |b| {
        b.iter(|| {
            black_box(new_with.has_child());
        });
    });

    // Without child
    let old_without = OldComponentElement::new();
    let new_without = NewComponentElement::new();

    group.bench_function("old_sentinel_without_child", |b| {
        b.iter(|| {
            black_box(old_without.has_child());
        });
    });

    group.bench_function("new_option_without_child", |b| {
        b.iter(|| {
            black_box(new_without.has_child());
        });
    });

    group.finish();
}

fn bench_get_child(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_child");

    let old_elem = {
        let mut elem = OldComponentElement::new();
        elem.set_child(42);
        elem
    };

    let new_elem = {
        let mut elem = NewComponentElement::new();
        elem.set_child(ElementId::new(42));
        elem
    };

    group.bench_function("old_sentinel", |b| {
        b.iter(|| {
            if let Some(id) = old_elem.child() {
                black_box(id);
            }
        });
    });

    group.bench_function("new_option", |b| {
        b.iter(|| {
            if let Some(id) = new_elem.child() {
                black_box(id);
            }
        });
    });

    group.finish();
}

fn bench_set_child(c: &mut Criterion) {
    let mut group = c.benchmark_group("set_child");

    group.bench_function("old_sentinel", |b| {
        let mut elem = OldComponentElement::new();
        b.iter(|| {
            elem.set_child(black_box(42));
        });
    });

    group.bench_function("new_option", |b| {
        let mut elem = NewComponentElement::new();
        b.iter(|| {
            elem.set_child(black_box(ElementId::new(42)));
        });
    });

    group.finish();
}

fn bench_clear_child(c: &mut Criterion) {
    let mut group = c.benchmark_group("clear_child");

    group.bench_function("old_sentinel", |b| {
        let mut elem = OldComponentElement::new();
        elem.set_child(42);
        b.iter(|| {
            elem.clear_child();
            elem.set_child(42); // Reset for next iteration
        });
    });

    group.bench_function("new_option", |b| {
        let mut elem = NewComponentElement::new();
        elem.set_child(ElementId::new(42));
        b.iter(|| {
            elem.clear_child();
            elem.set_child(ElementId::new(42)); // Reset for next iteration
        });
    });

    group.finish();
}

fn bench_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("clone_element");

    let old_elem = {
        let mut elem = OldComponentElement::new();
        elem.set_child(42);
        elem
    };

    let new_elem = {
        let mut elem = NewComponentElement::new();
        elem.set_child(ElementId::new(42));
        elem
    };

    group.bench_function("old_sentinel", |b| {
        b.iter(|| {
            black_box(old_elem.clone());
        });
    });

    group.bench_function("new_option", |b| {
        b.iter(|| {
            black_box(new_elem.clone());
        });
    });

    group.finish();
}

fn bench_vec_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("vec_operations");

    // Test with different sizes
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("old_sentinel_push", size), size, |b, &size| {
            b.iter(|| {
                let mut vec = Vec::new();
                for i in 0..size {
                    vec.push(OldComponentElement::new());
                    if i % 2 == 0 {
                        vec.last_mut().unwrap().set_child(i);
                    }
                }
                black_box(vec);
            });
        });

        group.bench_with_input(BenchmarkId::new("new_option_push", size), size, |b, &size| {
            b.iter(|| {
                let mut vec = Vec::new();
                for i in 0..size {
                    vec.push(NewComponentElement::new());
                    if i % 2 == 0 {
                        vec.last_mut().unwrap().set_child(ElementId::new(i));
                    }
                }
                black_box(vec);
            });
        });
    }

    group.finish();
}

fn bench_pattern_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_matching");

    let old_with = {
        let mut elem = OldComponentElement::new();
        elem.set_child(42);
        elem
    };

    let new_with = {
        let mut elem = NewComponentElement::new();
        elem.set_child(ElementId::new(42));
        elem
    };

    group.bench_function("old_sentinel_match", |b| {
        b.iter(|| {
            match old_with.child() {
                Some(id) => black_box(id * 2),
                None => black_box(0),
            }
        });
    });

    group.bench_function("new_option_match", |b| {
        b.iter(|| {
            match new_with.child() {
                Some(id) => black_box(id.get() * 2),
                None => black_box(0),
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_creation,
    bench_has_child_check,
    bench_get_child,
    bench_set_child,
    bench_clear_child,
    bench_clone,
    bench_vec_operations,
    bench_pattern_matching,
);

criterion_main!(benches);
