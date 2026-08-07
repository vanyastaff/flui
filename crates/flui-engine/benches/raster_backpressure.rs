//! Criterion benchmark: the reliable in-flight counter's cost — issue
//! #556's raster-backpressure accounting seam
//! (`flui_engine::raster_owner::RasterOwner`).
//!
//! Two shapes:
//! - `submit_pump_retire_cycle`: the uncontended submit→pump→retire cost,
//!   with `SceneSnapshot`/`Scene`/`Layer` construction moved to
//!   `iter_batched`'s untimed setup closure so the ALLOCATION half of that
//!   construction is excluded from the timed routine. Honest caveat: the
//!   matching DEALLOCATION still runs inside the timed routine, since
//!   `RasterOwner::pump` takes ownership of the pending frame and drops it
//!   as part of retiring — there is no way to hand that ownership back out
//!   through `pump`'s public signature without changing it, which is out of
//!   this bench's scope. So this is not a pure zero-construction number; it
//!   is the closest delta this API shape allows, not a whole-cycle figure
//!   mislabeled as plumbing-only. For context: constructing AND dropping
//!   one `bench_frame` in its own timed closure, with nothing else
//!   running, measures at ~70ns on its own. No-op backend, no wake hook
//!   registered (the common case today: nothing has wired one yet).
//! - `in_flight_read_under_retire_storm`: a background thread continuously
//!   submits and pumps ("a retire storm") while the timed loop repeatedly
//!   reads `RasterHandle::in_flight()` from a different thread — the
//!   concurrency shape the reliable, atomics-backed counter exists to
//!   survive: an owner-side poll loop reading capacity while the raster
//!   thread is actively retiring frames.
//!
//! Run with `cargo bench -p flui-engine --bench raster_backpressure`.

// Benchmark harness functions are internal measurement scaffolding, not a
// public API surface — exempt from the crate's missing-docs lint, matching
// this crate's other bench targets.
#![allow(missing_docs)]

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_engine::{
    CanvasLayer, DamageRegion, EngineError, Layer, RasterBackend, RasterOwner, Scene, SceneSnapshot,
};
use flui_foundation::{
    FrameEpoch, FrameStamp, PresentationAddress, PresentationId, RealmId, SurfaceGeneration,
};
use flui_types::Size;
use flui_types::geometry::{Pixels, Rect};

/// A backend that does the minimum possible work per frame — isolates the
/// mailbox/accounting overhead this bench measures from any real GPU cost.
#[derive(Default)]
struct NoOpBackend;

impl RasterBackend for NoOpBackend {
    fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
        Ok(true)
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn is_device_lost(&self) -> bool {
        false
    }

    fn mark_dirty(&mut self, _rect: Rect<Pixels>) {}

    fn mark_full_repaint(&mut self) {}

    fn has_damage(&self) -> bool {
        true
    }

    fn size(&self) -> (u32, u32) {
        (0, 0)
    }

    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

fn bench_address() -> PresentationAddress {
    PresentationAddress {
        realm_id: RealmId::new(1),
        presentation_id: PresentationId::new(1),
    }
}

fn bench_frame(epoch: FrameEpoch) -> SceneSnapshot {
    let stamp = FrameStamp::builder()
        .address(bench_address())
        .epoch(epoch)
        .surface_generation(SurfaceGeneration::ZERO)
        .build();
    SceneSnapshot::builder()
        .stamp(stamp)
        .damage(DamageRegion::Full)
        .scene(Scene::from_layer(
            Size::ZERO,
            Layer::from(CanvasLayer::new()),
            0,
        ))
        .build()
}

/// Uncontended baseline: one thread doing submit→pump→retire in a tight
/// loop, no wake hook registered. `iter_batched` separates the untimed
/// per-batch setup (constructing the `SceneSnapshot` `submit` will consume)
/// from the timed routine (`submit` + `pump` alone). That moves construction
/// out of the measurement but does NOT yield a marginal plumbing cost: the
/// timed region still drops the frame `pump` consumed, and there is no
/// pre-accounting baseline to subtract, so the figure went UP versus the
/// unbatched form (`iter_batched`'s per-iteration overhead exceeds the
/// construction allocation it removed). Read it as a whole-cycle number —
/// the closest delta this API shape allows — exactly as the module doc above
/// says, not as the cost the accounting added.
fn submit_pump_retire_cycle(c: &mut Criterion) {
    let (mut owner, handle, _ack_rx, _shutdown_complete_rx) =
        RasterOwner::new(NoOpBackend, bench_address());
    let mut epoch = FrameEpoch::ZERO;

    c.bench_function("submit_pump_retire_cycle", |b| {
        b.iter_batched(
            || {
                epoch = epoch.next();
                bench_frame(epoch)
            },
            |frame| {
                handle.submit(frame).expect("submit");
                black_box(owner.pump())
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Contended: a background thread runs a continuous submit/pump/retire
/// storm on its own `RasterOwner` while the timed loop reads
/// `RasterHandle::in_flight()` from the main thread — the counter's
/// `Ordering::Acquire` read racing the storm's `Ordering::AcqRel`
/// increments/decrements on real, unsynchronized threads.
fn in_flight_read_under_retire_storm(c: &mut Criterion) {
    let (owner, handle, _ack_rx, _shutdown_complete_rx) =
        RasterOwner::new(NoOpBackend, bench_address());

    let stop = Arc::new(AtomicBool::new(false));
    let storm_handle = handle.clone();
    let storm_stop = Arc::clone(&stop);
    let storm_thread = thread::spawn(move || {
        let mut owner = owner;
        let mut epoch = FrameEpoch::ZERO;
        while !storm_stop.load(Ordering::Relaxed) {
            epoch = epoch.next();
            let _ = storm_handle.submit(bench_frame(epoch));
            let _ = owner.pump();
        }
    });

    c.bench_function("in_flight_read_under_retire_storm", |b| {
        b.iter(|| black_box(handle.in_flight()));
    });

    stop.store(true, Ordering::Relaxed);
    storm_thread.join().expect("storm thread must not panic");
}

criterion_group!(
    benches,
    submit_pump_retire_cycle,
    in_flight_read_under_retire_storm
);
criterion_main!(benches);
