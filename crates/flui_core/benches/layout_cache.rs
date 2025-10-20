//! Benchmarks for layout cache performance
//!
//! Run with: cargo bench --bench layout_cache

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use flui_core::{BoxConstraints, ElementId, Size};
use flui_core::cache::{LayoutCache, LayoutCacheKey, LayoutResult};

/// Simulate an expensive layout calculation
fn expensive_layout(constraints: BoxConstraints) -> Size {
    // Simulate some work
    let mut sum = 0.0;
    for i in 0..1000 {
        sum += (i as f32 * constraints.min_width).sin();
    }
    Size::new(
        constraints.max_width.min(sum.abs()),
        constraints.max_height,
    )
}

/// Benchmark layout without caching
fn bench_layout_no_cache(c: &mut Criterion) {
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

    c.bench_function("layout_no_cache", |b| {
        b.iter(|| {
            black_box(expensive_layout(black_box(constraints)))
        });
    });
}

/// Benchmark layout with cache hit
fn bench_layout_cache_hit(c: &mut Criterion) {
    let cache = LayoutCache::new();
    let element_id = ElementId::new();
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    let key = LayoutCacheKey::new(element_id, constraints);

    // Pre-populate cache
    cache.insert(key.clone(), LayoutResult::new(Size::new(100.0, 100.0)));

    c.bench_function("layout_cache_hit", |b| {
        b.iter(|| {
            black_box(cache.get(&black_box(key.clone())).unwrap())
        });
    });
}

/// Benchmark layout with cache miss (first time)
fn bench_layout_cache_miss(c: &mut Criterion) {
    c.bench_function("layout_cache_miss", |b| {
        b.iter(|| {
            let cache = LayoutCache::new();
            let element_id = ElementId::new();
            let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
            let key = LayoutCacheKey::new(element_id, constraints);

            black_box(cache.get_or_compute(black_box(key), || {
                LayoutResult::new(expensive_layout(constraints))
            }))
        });
    });
}

/// Benchmark layout cache with varying number of cached entries
fn bench_layout_cache_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout_cache_scaling");

    for cache_size in [10, 100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(cache_size),
            cache_size,
            |b, &size| {
                let cache = LayoutCache::new();

                // Pre-populate cache
                for i in 0..size {
                    let element_id = ElementId::new();
                    let constraints = BoxConstraints::tight(Size::new(100.0 + i as f32, 100.0));
                    let key = LayoutCacheKey::new(element_id, constraints);
                    cache.insert(key, LayoutResult::new(Size::new(100.0, 100.0)));
                }

                // Benchmark lookup
                let test_id = ElementId::new();
                let test_constraints = BoxConstraints::tight(Size::new(150.0, 100.0));
                let test_key = LayoutCacheKey::new(test_id, test_constraints);

                b.iter(|| {
                    black_box(cache.get_or_compute(black_box(test_key.clone()), || {
                        LayoutResult::new(expensive_layout(test_constraints))
                    }))
                });
            },
        );
    }

    group.finish();
}

/// Benchmark cache invalidation
fn bench_layout_cache_invalidate(c: &mut Criterion) {
    c.bench_function("layout_cache_invalidate", |b| {
        b.iter(|| {
            let cache = LayoutCache::new();
            let element_id = ElementId::new();

            // Add some entries
            for i in 0..100 {
                let constraints = BoxConstraints::tight(Size::new(100.0 + i as f32, 100.0));
                let key = LayoutCacheKey::new(element_id, constraints);
                cache.insert(key, LayoutResult::new(Size::new(100.0, 100.0)));
            }

            // Invalidate
            black_box(cache.invalidate_element(black_box(element_id)))
        });
    });
}

/// Benchmark string interning
fn bench_string_interning(c: &mut Criterion) {
    use flui_core::foundation::string_cache::{intern, resolve};

    c.bench_function("string_intern", |b| {
        b.iter(|| {
            black_box(intern(black_box("MyWidgetType")))
        });
    });

    c.bench_function("string_intern_cached", |b| {
        // Pre-intern
        let _handle = intern("MyWidgetType");

        b.iter(|| {
            black_box(intern(black_box("MyWidgetType")))
        });
    });

    c.bench_function("string_resolve", |b| {
        let handle = intern("MyWidgetType");

        b.iter(|| {
            black_box(resolve(black_box(handle)))
        });
    });

    c.bench_function("string_comparison", |b| {
        let handle1 = intern("MyWidgetType");
        let handle2 = intern("MyWidgetType");

        b.iter(|| {
            black_box(black_box(handle1) == black_box(handle2))
        });
    });
}

criterion_group!(
    layout_benches,
    bench_layout_no_cache,
    bench_layout_cache_hit,
    bench_layout_cache_miss,
    bench_layout_cache_scaling,
    bench_layout_cache_invalidate,
);

criterion_group!(
    string_benches,
    bench_string_interning,
);

criterion_main!(layout_benches, string_benches);
