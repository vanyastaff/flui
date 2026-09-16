//! Criterion benchmark: `AsyncDriver::poll_ready`'s per-pump cost, before vs.
//! after issue #1056's ready-index rewrite (separating discovery from task
//! ownership so an idle pump drains an index instead of scanning every
//! resident task).
//!
//! **Evidence only, not a merge gate; not run or linked by CI.** CI's
//! `clippy`/`feature-matrix` jobs pass `--all-targets`/`--benches`, so this
//! file IS type-checked and lint-checked on every PR — `cargo clippy
//! --all-targets` lints it. What CI never does is link or run it:
//! `bench-compile` (`cargo bench --no-run`) only targets `flui-rendering`.
//! Run it locally, on demand: `cargo bench -p flui-scheduler --bench
//! async_driver_pump`. `tests/async_driver_ready_index_allocation.rs` (a
//! counting-allocator test) and `an_empty_pump_touches_no_dormant_task_flags`
//! (a flag-load counter, in the crate's own `#[cfg(test)]` unit tests) are
//! the actual CI-run complexity proofs for issue #1056's acceptance
//! criteria. Numbers here are local measurements, reported in the PR body
//! alongside a before/after table, not asserted.
//!
//! Every driver is constructed OUTSIDE `b.iter`; each closure re-arms
//! whatever it consumed on the previous iteration (a completed/removed task
//! would otherwise make iteration 2 measure a different, and shrinking,
//! `N`/`R`), and there is one untimed warm-up pump per group so the first
//! iteration's spawn-time poll doesn't leak into the measured distribution.
//! Compare runs with `critcmp` when it's available; otherwise attach raw
//! criterion output.
//!
//! Run with `cargo bench -p flui-scheduler --bench async_driver_pump`.

// Benchmark harness functions are internal measurement scaffolding, not a
// public API surface — exempt from the crate's missing-docs lint, matching
// this crate's other bench target (`frame_clock_gate.rs`).

use std::hint::black_box;
use std::task::Poll;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_scheduler::AsyncDriver;

/// `R=0`: every task is dormant (spawned once, never woken again). Measures
/// the claim issue #1056 exists for — an idle pump must cost O(R), not O(N).
fn empty_pump(c: &mut Criterion) {
    let mut group = c.benchmark_group("empty_pump");

    for dormant in [0usize, 100_000] {
        let driver = AsyncDriver::new();
        let mut tokens = Vec::with_capacity(dormant);
        for _ in 0..dormant {
            tokens.push(driver.spawn_local(Box::pin(std::future::pending::<()>())));
        }
        // Untimed warm-up: the first pump polls every freshly spawned task
        // once (spawn seeds `ready`), making all of them dormant. Every
        // measured iteration after this one polls zero tasks — nothing to
        // re-arm between iterations for this group.
        assert_eq!(driver.poll_ready(), dormant);
        assert_eq!(driver.ready_task_count(), 0);

        group.bench_with_input(
            BenchmarkId::from_parameter(dormant),
            &driver,
            |b, driver| {
                b.iter(|| black_box(driver.poll_ready()));
            },
        );

        drop(tokens);
    }

    group.finish();
}

/// `R=N`: every task self-re-wakes on every poll, forever — a pump never
/// idles. Measures the steady-state cost of discovering and polling ready
/// work once R is no longer zero, and (via `ready_heavy` vs `empty_pump`'s
/// `N=0` case) the per-task marginal cost of the ready-index mechanism
/// itself.
fn ready_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("ready_heavy");

    for ready in [1_000usize, 10_000] {
        let driver = AsyncDriver::new();
        let mut tokens = Vec::with_capacity(ready);
        for _ in 0..ready {
            tokens.push(driver.spawn_local(Box::pin(std::future::poll_fn(|cx| {
                // Re-arms itself every poll: `R` stays `ready` for every
                // iteration criterion runs, not just the first.
                cx.waker().wake_by_ref();
                Poll::<()>::Pending
            }))));
        }
        // Untimed warm-up pump, excluded from the measured distribution.
        assert_eq!(driver.poll_ready(), ready);

        group.bench_with_input(BenchmarkId::from_parameter(ready), &driver, |b, driver| {
            b.iter(|| black_box(driver.poll_ready()));
        });

        drop(tokens);
    }

    group.finish();
}

criterion_group!(benches, empty_pump, ready_heavy);
criterion_main!(benches);
