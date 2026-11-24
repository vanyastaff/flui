//! Full mounted CPU-frame allocation observations, separate from timing.
//! Run `cargo bench -p flui-widgets --bench mounted_motion_allocations`.
//! Add `-- --normal` to exclude the separate fractional-alpha control.
//! Counts allocation requests and requested bytes, not peak live memory.

use std::hint::black_box;
use std::time::Duration;

use flui_foundation::testing::heap::{CountingAllocator, HeapCounts};

#[path = "support/mounted_motion.rs"]
mod mounted;
use mounted::{
    BATCH, FRAME, FrameWork, MountedMotion, MountedScroll, RUN, Workload, assert_population,
    assert_progress, verify_scroll,
};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Default, Debug)]
struct Allocations {
    requests: u64,
    bytes: u64,
}

impl Allocations {
    fn measure(f: impl FnOnce()) -> Self {
        let (spent, ()) = HeapCounts::during(f);
        Self {
            requests: spent.allocations,
            bytes: spent.allocated_bytes,
        }
    }

    fn add(&mut self, other: Self) {
        self.requests += other.requests;
        self.bytes += other.bytes;
    }
}

fn observe(count: usize, workload: Workload) {
    let mut motion = MountedMotion::new(count, workload);
    motion.observe_builds();
    let mut active = Allocations::default();
    let mut active_work = FrameWork::default();
    for _ in 0..3 {
        motion.prepare_active();
        let mut previous = motion.scene();
        assert_population(&previous, count);
        for _ in 0..BATCH {
            let before = motion.work_before();
            active.add(Allocations::measure(|| motion.laid.pump_for(FRAME)));
            let work = motion.work_since(before);
            assert_eq!(work.submitted, 1);
            active_work.add(work);
            let current = motion.scene();
            assert_population(&current, count);
            assert_progress(&previous, &current, workload);
            previous = current;
        }
    }
    motion.prepare_active();
    let before = motion.work_before();
    let admission = Allocations::measure(|| motion.retarget());
    let admission_work = motion.work_since(before);
    assert!(admission_work.builds > 0);
    motion.laid.pump_for(RUN + Duration::from_secs(1));
    let before = motion.work_before();
    let idle = Allocations::measure(|| {
        for _ in 0..8 {
            motion.laid.pump_for(FRAME);
        }
    });
    let idle_work = motion.work_since(before);
    assert_eq!(idle_work.submitted, 0);
    assert_eq!(idle_work.builds, 0);
    assert_eq!(idle_work.layout_roots, 0);
    assert_population(&motion.scene(), count);
    println!(
        "{workload:?}/{count}: active96={active:?} {active_work:?}; retarget+admission={admission:?} {admission_work:?}; idle8={idle:?} {idle_work:?}"
    );
}

fn main() {
    let control = Allocations::measure(|| {
        black_box(Box::new(black_box([0_u8; 4096])));
    });
    assert!(
        control.requests > 0 && control.bytes >= 4096,
        "positive allocation control"
    );
    for workload in [
        Workload::Color,
        Workload::Opacity,
        Workload::ColorAndOpacity,
    ] {
        for count in [1, 100, 1000] {
            observe(count, workload);
        }
    }
    if !std::env::args().any(|arg| arg == "--normal") {
        for count in [1, 100, 1000] {
            observe(count, Workload::FractionalAlpha);
        }
    }
    verify_scroll();
    let mut scroll = MountedScroll::new();
    let mut active = Allocations::default();
    for _ in 0..3 {
        scroll.prepare_active();
        for _ in 0..BATCH {
            let before = scroll.pixels();
            active.add(Allocations::measure(|| scroll.laid.pump_for(FRAME)));
            assert!(scroll.pixels() > before);
            scroll.assert_marker();
        }
    }
    scroll.prepare_active();
    let before = scroll.pixels();
    let admission = Allocations::measure(|| scroll.animate());
    assert_eq!(scroll.pixels(), before);
    for _ in 0..3 {
        scroll.laid.pump_for(FRAME);
    }
    assert!(scroll.pixels() > before);
    scroll.assert_marker();
    scroll.prepare_active();
    let stopped = scroll.pixels();
    let cancellation = Allocations::measure(|| scroll.interrupt());
    let idle = Allocations::measure(|| {
        for _ in 0..8 {
            scroll.laid.pump_for(FRAME);
        }
    });
    assert_eq!(scroll.pixels(), stopped);
    scroll.assert_marker();
    println!(
        "Scroll: active96={active:?}; animate+admission={admission:?}; cancel+admission={cancellation:?}; cancelled_idle8={idle:?}"
    );
}
