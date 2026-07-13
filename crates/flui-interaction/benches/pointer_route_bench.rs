//! Owner-routed pointer delivery benchmarks.
//!
//! Hot paths:
//! - route resolution on Down captures the live owner-local cells and local
//!   transforms once for a pointer sequence;
//! - cached route invocation is the common Move/Up delivery path and must stay
//!   a tight owner-thread loop over already-resolved cells.
//!
//! The target-count baselines intentionally cover the normal leaf case, a
//! typical small stack of interactive ancestors, and a stress path with many
//! targets. Run with:
//!
//! ```text
//! cargo bench -p flui-interaction --bench pointer_route_bench
//! ```

#![allow(missing_docs)]

use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_interaction::events::{PointerType, make_down_event, make_move_event};
use flui_interaction::{
    HitTestEntry, InteractionDispatchHandle, InteractionLane, Offset, PointerTarget, RenderId,
};

fn register_targets(
    handle: &InteractionDispatchHandle,
    count: usize,
) -> (Rc<Cell<usize>>, Vec<PointerTarget>) {
    let deliveries = Rc::new(Cell::new(0));
    let targets = (0..count)
        .map(|_| {
            let deliveries = Rc::clone(&deliveries);
            handle
                .register_pointer(move |_| deliveries.set(deliveries.get() + 1))
                .expect("bench target registration")
        })
        .collect();
    (deliveries, targets)
}

fn path_for(targets: &[PointerTarget]) -> Vec<HitTestEntry> {
    targets
        .iter()
        .enumerate()
        .map(|(index, target)| HitTestEntry::new(RenderId::new(index + 1)).pointer_target(*target))
        .collect()
}

fn bench_resolve_pointer_route(c: &mut Criterion) {
    let mut group = c.benchmark_group("InteractionLane::resolve_pointer_route");
    for target_count in [1_usize, 4, 16] {
        let lane = InteractionLane::try_new().expect("bench lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let (_deliveries, targets) = register_targets(&handle, target_count);
            let path = path_for(&targets);
            group.bench_with_input(
                BenchmarkId::from_parameter(target_count),
                &target_count,
                |b, _| {
                    b.iter(|| {
                        let route = handle
                            .resolve_pointer_route(black_box(&path))
                            .expect("route resolution")
                            .token();
                        handle.release_route(route).expect("release route");
                    });
                },
            );
        });
    }
    group.finish();
}

fn bench_invoke_cached_pointer_route(c: &mut Criterion) {
    let mut group = c.benchmark_group("InteractionLane::invoke_pointer_route/common_move");
    let event = make_move_event(Offset::ZERO, PointerType::Mouse);
    for target_count in [1_usize, 4, 16] {
        let lane = InteractionLane::try_new().expect("bench lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let (deliveries, targets) = register_targets(&handle, target_count);
            let path = path_for(&targets);
            let route = handle
                .resolve_pointer_route(&path)
                .expect("route resolution")
                .token();
            group.bench_with_input(
                BenchmarkId::from_parameter(target_count),
                &target_count,
                |b, _| {
                    b.iter(|| {
                        let panic = handle
                            .invoke_pointer_route(black_box(route), black_box(&event))
                            .expect("cached route invocation");
                        if let Some(panic) = panic {
                            panic.resume();
                        }
                        black_box(deliveries.get());
                    });
                },
            );
            handle.release_route(route).expect("release route");
        });
    }
    group.finish();
}

fn bench_direct_hit_test_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("HitTestResult::dispatch/direct");
    let event = make_down_event(Offset::ZERO, PointerType::Mouse);
    for target_count in [1_usize, 4, 16] {
        let lane = InteractionLane::try_new().expect("bench lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let (deliveries, targets) = register_targets(&handle, target_count);
            let path = path_for(&targets);
            let mut result = flui_interaction::HitTestResult::new();
            for entry in path {
                result.add(entry);
            }
            group.bench_with_input(
                BenchmarkId::from_parameter(target_count),
                &target_count,
                |b, _| {
                    b.iter(|| {
                        result.dispatch(black_box(&event));
                        black_box(deliveries.get());
                    });
                },
            );
        });
    }
    group.finish();
}

criterion_group!(
    pointer_route_benches,
    bench_resolve_pointer_route,
    bench_invoke_cached_pointer_route,
    bench_direct_hit_test_dispatch,
);
criterion_main!(pointer_route_benches);
