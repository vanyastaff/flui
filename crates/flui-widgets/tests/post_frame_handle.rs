//! `PostFrameHandle` targets the **binding's** scheduler.
//!
//! `HeadlessBinding` owns a binding-local `Scheduler`; production drives the
//! `Scheduler::instance()` singleton. A capability that silently
//! fell back to the singleton would leave headless callbacks undrained *and* let a
//! headless test "prove" a production path it never touched.
//!
//! The capability is acquired in `init_state` — a lifecycle hook, never `build`
//! (port-check trigger #22) — and fired by the real `pump_frame` frame order.

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, loose, tight};
use flui_foundation::HasInstance;
use flui_scheduler::Scheduler;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use parking_lot::Mutex;

/// What the probe observed about the capability it was handed.
#[derive(Clone, Default)]
struct Observations {
    /// Times the probe's own post-frame callback ran.
    fired: Arc<AtomicUsize>,
    /// Whether the handed-out handle (wrongly) names the process-global singleton.
    targets_singleton: Arc<Mutex<Option<bool>>>,
    /// The handle the widget actually received, so the test can check its identity
    /// against the binding the harness built.
    handle: Arc<Mutex<Option<flui_scheduler::PostFrameHandle>>>,
}

/// Acquires `PostFrameHandle` in `init_state` and schedules one callback with it.
#[derive(Clone)]
struct PostFrameProbe {
    observations: Observations,
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
        }
    }
}

struct PostFrameProbeState {
    observations: Observations,
}

#[derive(Clone)]
struct LocalPostFrameProbe {
    pipeline: Arc<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>,
    observed_committed_geometry: Arc<AtomicBool>,
    desired_width: Arc<AtomicUsize>,
    rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
}

impl View for LocalPostFrameProbe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for LocalPostFrameProbe {
    type State = LocalPostFrameProbeState;

    fn create_state(&self) -> Self::State {
        LocalPostFrameProbeState {
            pipeline: Arc::clone(&self.pipeline),
            observed_committed_geometry: Arc::clone(&self.observed_committed_geometry),
            desired_width: Arc::clone(&self.desired_width),
            rebuild: Arc::clone(&self.rebuild),
        }
    }
}

struct LocalPostFrameProbeState {
    pipeline: Arc<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>,
    observed_committed_geometry: Arc<AtomicBool>,
    desired_width: Arc<AtomicUsize>,
    rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
}

impl ViewState<LocalPostFrameProbe> for LocalPostFrameProbeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        *self.rebuild.lock() = Some(ctx.rebuild_handle());
        let handle = ctx
            .post_frame_handle()
            .expect("the binding must install a PostFrameHandle");
        let pipeline = Arc::clone(&self.pipeline);
        let observed = Arc::clone(&self.observed_committed_geometry);
        let owner_local = Rc::new(Cell::new(false));
        let callback_local = Rc::clone(&owner_local);

        handle
            .schedule_local(move |_timing| {
                callback_local.set(true);
                let owner = pipeline.read();
                let render_tree = owner.render_tree();
                let root = render_tree
                    .iter()
                    .map(|(id, _)| id)
                    .find(|id| render_tree.parent(*id).is_none())
                    .expect("the mounted subtree should have a render root");
                observed.store(
                    callback_local.get()
                        && owner.box_size(root)
                            == Some(flui_types::Size::new(
                                flui_types::geometry::px(64.0),
                                flui_types::geometry::px(18.0),
                            )),
                    Ordering::SeqCst,
                );
            })
            .expect("init_state runs inside the headless owner scope");
    }

    fn build(&self, _view: &LocalPostFrameProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(self.desired_width.load(Ordering::SeqCst) as f32, 18.0)
    }
}

impl ViewState<PostFrameProbe> for PostFrameProbeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let handle = ctx
            .post_frame_handle()
            .expect("the binding must install a PostFrameHandle");

        *self.observations.targets_singleton.lock() =
            Some(handle.targets_same_scheduler(Scheduler::instance()));
        *self.observations.handle.lock() = Some(handle.clone());

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
/// other than `self.scheduler` (the singleton, or a fresh one). The identity
/// assertion flips and `fired` stays 0 — nothing else drives frames here.
#[test]
fn a_widgets_post_frame_callback_lands_on_the_binding_scheduler_not_the_singleton() {
    // A canary on the singleton: if the seam leaks, this is where it lands.
    let singleton_fired = Arc::new(AtomicBool::new(false));
    let singleton_canary = Arc::clone(&singleton_fired);
    Scheduler::instance().add_post_frame_callback(Box::new(move |_| {
        singleton_canary.store(true, Ordering::SeqCst);
    }));

    let observations = Observations::default();
    let mut laid = lay_out(
        PostFrameProbe {
            observations: observations.clone(),
        },
        tight(100.0, 100.0),
    );

    assert_eq!(
        *observations.targets_singleton.lock(),
        Some(false),
        "the handle a widget receives must not name the process-global singleton"
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
            .targets_same_scheduler(Scheduler::instance()),
        "sanity: the binding's scheduler is not the singleton"
    );

    // One real frame. The probe's callback is never invoked by this test.
    laid.pump_for(Duration::from_millis(16));

    assert_eq!(
        observations.fired.load(Ordering::SeqCst),
        1,
        "pump_frame must drain the callback the widget scheduled"
    );
    assert!(
        !singleton_fired.load(Ordering::SeqCst),
        "pump_frame must not drive the singleton's post-frame queue"
    );
}

/// The capability is genuinely absent when no binding installed one, rather than
/// silently defaulting to a global.
#[test]
fn post_frame_handle_is_none_when_no_binding_installed_one() {
    let owner = flui_view::BuildOwner::new();
    assert!(
        owner.post_frame_handle().is_none(),
        "a bare BuildOwner must not conjure a scheduler"
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

    flui_scheduler::PostFrameHandle::new(&laid.binding_scheduler()).schedule(move |_| {
        saw.store(pipeline.read().box_size(root).is_some(), Ordering::SeqCst);
    });

    laid.pump_for(Duration::from_millis(16));

    assert!(
        saw_committed_layout.load(Ordering::SeqCst),
        "a post-frame callback must see geometry this frame's pipeline committed"
    );
}

/// Owner-local callbacks may capture `Rc` state and still observe geometry committed
/// by the same real headless frame. Registration happens in `init_state`, proving the
/// binding activates its owner scope around lifecycle work rather than only around a
/// test helper call immediately before scheduling.
#[test]
fn an_owner_local_post_frame_callback_observes_committed_geometry() {
    let pipeline = Arc::new(parking_lot::RwLock::new(
        flui_rendering::pipeline::PipelineOwner::new(),
    ));
    let observed = Arc::new(AtomicBool::new(false));
    let desired_width = Arc::new(AtomicUsize::new(32));
    let rebuild = Arc::new(Mutex::new(None));
    let mut laid = common::lay_out_with_pipeline_owner(
        LocalPostFrameProbe {
            pipeline: Arc::clone(&pipeline),
            observed_committed_geometry: Arc::clone(&observed),
            desired_width: Arc::clone(&desired_width),
            rebuild: Arc::clone(&rebuild),
        },
        loose(100.0),
        Arc::clone(&pipeline),
    );

    assert_eq!(
        pipeline.read().box_size(laid.root()),
        Some(flui_types::Size::new(
            flui_types::geometry::px(32.0),
            flui_types::geometry::px(18.0),
        )),
        "bootstrap geometry must differ from the geometry expected by the callback"
    );
    desired_width.store(64, Ordering::SeqCst);
    rebuild
        .lock()
        .as_ref()
        .expect("init_state captured a rebuild handle")
        .schedule(flui_view::RebuildReason::StateChange);

    laid.pump_for(Duration::from_millis(16));

    assert!(
        observed.load(Ordering::SeqCst),
        "the owner-local callback must run after this frame commits geometry"
    );
}
