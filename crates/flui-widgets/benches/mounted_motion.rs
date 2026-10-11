//! Complete headless CPU-frame work with semantics enabled: color, group opacity
//! and both, at 1/100/1000 visible cells. Excludes GPU presentation latency.
//! Steady timing covers fresh interior runs; admission and idle frames are separate.
//! Run `cargo bench -p flui-widgets --bench mounted_motion -- 'mounted/' --noplot`.
//! `--test` verifies workloads and the separate fractional-alpha correctness control.

use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

#[path = "support/mounted_motion.rs"]
mod mounted;
use mounted::{
    BATCH, FRAME, FrameWork, MountedMotion, MountedScroll, RUN, Workload, assert_population,
    assert_progress, verify_scroll,
};

fn verify(count: usize, workload: Workload) {
    let mut motion = MountedMotion::new(count, workload);
    motion.observe_builds();
    let mut active = FrameWork::default();
    for _ in 0..3 {
        motion.prepare_active();
        let mut previous = motion.scene();
        assert_population(&previous, count);
        for _ in 0..BATCH {
            let before = motion.work_before();
            motion.laid.pump_for(FRAME);
            let work = motion.work_since(before);
            assert_eq!(work.submitted, 1, "every active frame submits a scene");
            active.add(work);
            let current = motion.scene();
            assert_population(&current, count);
            assert_progress(&previous, &current, workload);
            previous = current;
        }
    }
    motion.prepare_active();
    let before = motion.work_before();
    motion.retarget();
    let admission = motion.work_since(before);
    assert!(
        admission.builds > 0,
        "retarget includes author rebuild work"
    );
    motion.laid.pump_for(RUN + Duration::from_secs(1));
    let mut idle = FrameWork::default();
    for _ in 0..8 {
        let before = motion.work_before();
        motion.laid.pump_for(FRAME);
        idle.add(motion.work_since(before));
    }
    assert_eq!(idle.submitted, 0, "idle frames must not force repaint");
    assert_eq!(idle.builds, 0, "idle frames must not force rebuilding");
    assert_eq!(idle.layout_roots, 0, "idle frames must not force layout");
    assert_population(&motion.scene(), count);
    let a11y = motion.laid.a11y_tree().expect("semantics enabled");
    assert!(
        a11y.raw().nodes.len() >= count,
        "every labelled cell participates"
    );
    println!(
        "{workload:?}/{count}: active96={active:?}; retarget+admission={admission:?}; idle8={idle:?}"
    );
}

fn mounted_frames(criterion: &mut Criterion) {
    for workload in [
        Workload::Color,
        Workload::Opacity,
        Workload::ColorAndOpacity,
    ] {
        let mut group = criterion.benchmark_group(format!("mounted/{workload:?}"));
        for count in [1, 100, 1000] {
            verify(count, workload);
            group.bench_with_input(
                BenchmarkId::new("active_interior", count),
                &count,
                |b, &count| {
                    let mut motion = MountedMotion::new(count, workload);
                    b.iter_custom(|iterations| {
                        let mut elapsed = Duration::ZERO;
                        let mut remaining = iterations;
                        while remaining > 0 {
                            motion.prepare_active();
                            let frames = remaining.min(BATCH);
                            let start = Instant::now();
                            for _ in 0..frames {
                                motion.laid.pump_for(FRAME);
                            }
                            elapsed += start.elapsed();
                            remaining -= frames;
                        }
                        elapsed
                    });
                },
            );
            group.bench_with_input(BenchmarkId::new("idle", count), &count, |b, &count| {
                let mut motion = MountedMotion::new(count, workload);
                motion.prepare_active();
                motion.laid.pump_for(RUN + Duration::from_secs(1));
                b.iter(|| motion.laid.pump_for(FRAME));
            });
            group.bench_with_input(
                BenchmarkId::new("retarget_admission", count),
                &count,
                |b, &count| {
                    let mut motion = MountedMotion::new(count, workload);
                    motion.prepare_active();
                    b.iter_custom(|iterations| {
                        let mut elapsed = Duration::ZERO;
                        for _ in 0..iterations {
                            motion.prepare_active();
                            let start = Instant::now();
                            motion.retarget();
                            elapsed += start.elapsed();
                        }
                        elapsed
                    });
                },
            );
        }
        group.finish();
    }
}

fn fractional_precision(criterion: &mut Criterion) {
    criterion.bench_function("precision/fractional_alpha", |b| {
        // Kept separate from the comparable throughput workloads: a quantized
        // baseline omits intended paint work and must fail this control.
        verify(100, Workload::FractionalAlpha);
        let mut motion = MountedMotion::new(100, Workload::FractionalAlpha);
        b.iter_custom(|iterations| {
            let mut remaining = iterations;
            let mut elapsed = Duration::ZERO;
            while remaining > 0 {
                motion.prepare_active();
                let frames = remaining.min(BATCH);
                let start = Instant::now();
                for _ in 0..frames {
                    motion.laid.pump_for(FRAME);
                }
                elapsed += start.elapsed();
                remaining -= frames;
            }
            elapsed
        });
    });
}

fn mounted_scroll(criterion: &mut Criterion) {
    criterion.bench_function("mounted/Scroll/active_interior", |b| {
        verify_scroll();
        let mut scroll = MountedScroll::new();
        b.iter_custom(|iterations| {
            let mut remaining = iterations;
            let mut elapsed = Duration::ZERO;
            while remaining > 0 {
                scroll.prepare_active();
                let frames = remaining.min(BATCH);
                let start = Instant::now();
                for _ in 0..frames {
                    scroll.laid.pump_for(FRAME);
                }
                elapsed += start.elapsed();
                remaining -= frames;
            }
            elapsed
        });
    });
    criterion.bench_function("mounted/Scroll/idle", |b| {
        let mut scroll = MountedScroll::new();
        scroll.prepare_active();
        scroll.interrupt();
        b.iter(|| scroll.laid.pump_for(FRAME));
    });
    criterion.bench_function("mounted/Scroll/cancel_admission", |b| {
        let mut scroll = MountedScroll::new();
        b.iter_custom(|iterations| {
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                scroll.prepare_active();
                let start = Instant::now();
                scroll.interrupt();
                elapsed += start.elapsed();
            }
            elapsed
        });
    });
    criterion.bench_function("mounted/Scroll/animate_admission", |b| {
        let mut scroll = MountedScroll::new();
        b.iter_custom(|iterations| {
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                scroll.prepare_active();
                let start = Instant::now();
                scroll.animate();
                elapsed += start.elapsed();
            }
            elapsed
        });
    });
}

criterion_group!(
    benches,
    mounted_frames,
    fractional_precision,
    mounted_scroll
);
criterion_main!(benches);
