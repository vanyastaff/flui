//! `PostFrameHandle` targets the **binding's own** scheduler, never some
//! other one.
//!
//! `HeadlessBinding` owns a binding-local `UpdateScheduler`. A capability that
//! silently named a different scheduler would leave headless callbacks
//! undrained *and* let a headless test "prove" a path it never actually
//! touched.
//!
//! The capability is acquired in `init_state` — a lifecycle hook, never `build`
//! — and fired by the real `pump_frame` frame order.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{lay_out, tight};
use flui_scheduler::UpdateScheduler;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use parking_lot::Mutex;

/// What the probe observed about the capability it was handed.
#[derive(Clone, Default)]
struct Observations {
    /// Times the probe's own post-frame callback ran.
    fired: Arc<AtomicUsize>,
    /// Whether the handed-out handle (wrongly) names an unrelated scheduler.
    targets_unrelated_scheduler: Arc<Mutex<Option<bool>>>,
    /// The handle the widget actually received, so the test can check its identity
    /// against the binding the harness built.
    handle: Arc<Mutex<Option<flui_scheduler::PostFrameHandle>>>,
}

/// Acquires `PostFrameHandle` in `init_state` and schedules one callback with it.
#[derive(Clone)]
struct PostFrameProbe {
    observations: Observations,
    /// An unrelated scheduler this probe compares its handle against — stands
    /// in for "any scheduler that is not this binding's own".
    unrelated_scheduler: UpdateScheduler,
}

impl View for PostFrameProbe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for PostFrameProbe {
    type State = PostFrameProbeState;

    fn create_state(&self) -> Self::State {
        PostFrameProbeState {
            observations: self.observations.clone(),
            unrelated_scheduler: self.unrelated_scheduler.clone(),
        }
    }
}

struct PostFrameProbeState {
    observations: Observations,
    unrelated_scheduler: UpdateScheduler,
}

impl ViewState<PostFrameProbe> for PostFrameProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let handle = ctx
            .post_frame_handle()
            .expect("the binding must install a PostFrameHandle");

        *self.observations.targets_unrelated_scheduler.lock() =
            Some(handle.targets_same_scheduler(&self.unrelated_scheduler));
        let _prev = self.observations.handle.lock().replace(handle.clone());

        let fired = Arc::clone(&self.observations.fired);
        handle.schedule(move |_timing| {
            fired.fetch_add(1, Ordering::SeqCst);
        });
    }

    fn build(&self, _view: &PostFrameProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// A widget's post-frame callback is drained by `pump_frame`, because the handle it
/// received names the binding's own scheduler.
///
/// Red-check: make `HeadlessBinding::install_build_capabilities` name any scheduler
/// other than `self.scheduler` (e.g. a fresh, unrelated one). The identity
/// assertion flips and `fired` stays 0 — nothing else drives frames here.
#[test]
fn a_widgets_post_frame_callback_lands_on_the_binding_scheduler_not_an_unrelated_one() {
    // A canary on an unrelated scheduler: if the seam leaks, this is where it lands.
    let unrelated_scheduler = UpdateScheduler::new();
    let unrelated_fired = Arc::new(AtomicBool::new(false));
    let unrelated_canary = Arc::clone(&unrelated_fired);
    unrelated_scheduler.add_post_frame_callback(Box::new(move |_| {
        unrelated_canary.store(true, Ordering::SeqCst);
    }));

    let observations = Observations::default();
    let mut laid = lay_out(
        PostFrameProbe {
            observations: observations.clone(),
            unrelated_scheduler: unrelated_scheduler.clone(),
        },
        tight(100.0, 100.0),
    );

    assert_eq!(
        *observations.targets_unrelated_scheduler.lock(),
        Some(false),
        "the handle a widget receives must not name an unrelated scheduler"
    );

    let binding_scheduler = laid.binding_scheduler();
    assert!(
        observations
            .handle
            .lock()
            .as_ref()
            .expect("init_state acquired a handle")
            .targets_same_scheduler(&binding_scheduler),
        "the handle a widget receives must name THIS binding's scheduler"
    );
    assert!(
        !flui_scheduler::PostFrameHandle::new(&binding_scheduler)
            .targets_same_scheduler(&unrelated_scheduler),
        "sanity: the binding's scheduler is not the unrelated one"
    );

    // One real frame. The probe's callback is never invoked by this test.
    laid.pump_for(Duration::from_millis(16));

    assert_eq!(
        observations.fired.load(Ordering::SeqCst),
        1,
        "pump_frame must drain the callback the widget scheduled"
    );
    assert!(
        !unrelated_fired.load(Ordering::SeqCst),
        "pump_frame must not drive the unrelated scheduler's post-frame queue"
    );
}

/// The scheduled callback observes **this** frame's committed layout — the
/// ordering `HeroController` depends on (`heroes.dart:964-968`).
#[test]
fn the_scheduled_callback_observes_this_frames_committed_layout() {
    let mut laid = lay_out(SizedBox::new(40.0, 24.0), tight(100.0, 100.0));

    let root = laid.root();
    let pipeline = laid.pipeline_owner();
    let saw_committed_layout = Arc::new(AtomicBool::new(false));
    let saw = Arc::clone(&saw_committed_layout);

    // `PipelineCell` is `!Send`, so this callback cannot go through
    // `PostFrameHandle::schedule` (its `Send` bound is for cross-thread
    // wake). `schedule_local` takes a `!Send` handle, so same-thread use is
    // a compile-time guarantee — same pattern as `editable_text.rs`'s IME
    // cursor loop.
    let post_frame_handle = laid.local_post_frame_handle();
    laid.enter_owner_scope(|| {
        post_frame_handle
            .schedule_local(move |_| {
                saw.store(
                    pipeline.with(|owner| owner.box_size(root).is_some()),
                    Ordering::SeqCst,
                );
            })
            .expect("schedule_local must succeed on the owner thread");
    });

    laid.pump_for(Duration::from_millis(16));

    assert!(
        saw_committed_layout.load(Ordering::SeqCst),
        "a post-frame callback must see geometry this frame's pipeline committed"
    );
}
