//! A `LeaderWatch` hears its leader's geometry change after the frame that
//! painted it, and only then: a steady layout delivers nothing.

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_foundation::notifier::Listenable;
use flui_layer::LayerLink;
use flui_objects::{RenderFlex, RenderLeaderLayer, RenderSizedBox};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::{LeaderWatch, PipelineCell, PipelineOwner},
    testing::{box_node, tree, update_render_object},
};
use flui_scheduler::{IdleDeadline, Instant, OwnerFrame, UpdateScheduler};

/// A pipeline painting one leader around a box `height` tall, and the
/// scheduler whose post-frame lane its watches deliver through.
struct Scene {
    cell: PipelineCell,
    scheduler: UpdateScheduler,
    frame: OwnerFrame,
    sized: flui_foundation::RenderId,
    link: LayerLink,
}

impl Scene {
    fn new(height: f64) -> Self {
        let link = LayerLink::new();
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let (root, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderLeaderLayer::new(link))
                    .child(box_node(RenderSizedBox::fixed(10.0, height)).label("sized")),
            ),
        );
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
        let scheduler = UpdateScheduler::new();
        let frame = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
        Self {
            cell: PipelineCell::new(owner),
            scheduler,
            frame,
            sized: registry.get("sized").expect("labelled"),
            link,
        }
    }

    fn watch(&self, link: LayerLink) -> LeaderWatch {
        self.cell.watch_leader(link, self.frame.post_frame_handle())
    }

    /// Lays out and paints whatever is dirty.
    fn paint(&self) {
        self.cell.run_frame().expect("the frame runs");
    }

    /// Completes a frame of the scheduler, which runs its post-frame lane.
    fn complete(&self) {
        let now = Instant::now();
        self.frame
            .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
            .expect("live owner frame");
    }

    fn resize(&self, height: f64) {
        self.cell.with_mut(|owner| {
            update_render_object::<RenderSizedBox, _>(owner, self.sized, |sized| {
                sized.set_size(Some(10.0), Some(height))
            });
        });
    }
}

fn counted(watch: &LeaderWatch) -> Rc<Cell<usize>> {
    let count = Rc::new(Cell::new(0));
    let counter = Rc::clone(&count);
    watch.add_listener(Rc::new(move || counter.set(counter.get() + 1)));
    count
}

fn height(watch: &LeaderWatch) -> Option<f64> {
    watch.geometry().map(|geometry| geometry.size.height)
}

/// The first paint publishes the leader and a resize changes it, each heard
/// once; frames that paint the same geometry deliver nothing and leave no
/// frame scheduled.
///
/// Red-check: queue every watch after each paint (the steady frame notifies).
#[test]
fn a_watch_hears_only_changes() {
    let scene = Scene::new(20.0);
    let watch = scene.watch(scene.link);
    let heard = counted(&watch);

    scene.paint();
    assert_eq!(heard.get(), 0, "listeners wait for the post-frame lane");
    scene.complete();
    assert_eq!((heard.get(), height(&watch)), (1, Some(20.0)));

    for _ in 0..3 {
        scene
            .cell
            .with_mut(|owner| owner.mark_needs_paint(owner.root_id().expect("root")));
        scene.paint();
        scene.complete();
    }
    assert_eq!(heard.get(), 1, "a steady layout delivers nothing");
    assert!(!scene.scheduler.is_frame_scheduled());

    scene.resize(30.0);
    scene.paint();
    scene.complete();
    assert_eq!((heard.get(), height(&watch)), (2, Some(30.0)));
}

/// A link no leader publishes under reads `None` and delivers nothing.
///
/// Red-check: report a watch's last value as a change on every paint.
#[test]
fn a_watch_without_a_leader_reads_none() {
    let scene = Scene::new(20.0);
    let watch = scene.watch(LayerLink::new());
    let heard = counted(&watch);
    for _ in 0..2 {
        scene.paint();
        scene.complete();
    }
    assert_eq!((heard.get(), watch.geometry()), (0, None));
}

/// Changes painted before the lane runs coalesce into one delivery of the
/// newest value.
///
/// Red-check: schedule a delivery for each change (two notifications).
#[test]
fn changes_before_the_lane_runs_deliver_once() {
    let scene = Scene::new(20.0);
    let watch = scene.watch(scene.link);
    let heard = counted(&watch);
    scene.paint();
    scene.resize(40.0);
    scene.paint();
    scene.complete();
    assert_eq!((heard.get(), height(&watch)), (1, Some(40.0)));
}

/// A dropped watch releases its listeners and later frames run as before.
///
/// Red-check: hold watches strongly in the pipeline (the capture stays).
#[test]
fn a_dropped_watch_releases_its_listeners() {
    let scene = Scene::new(20.0);
    let watch = scene.watch(scene.link);
    let capture = Rc::new(());
    let held = Rc::clone(&capture);
    watch.add_listener(Rc::new(move || {
        let _ = &held;
    }));
    let copy = watch.clone();
    drop(watch);
    assert_eq!(Rc::strong_count(&capture), 2, "a clone keeps the watch");
    drop(copy);
    assert_eq!(Rc::strong_count(&capture), 1);
    scene.resize(30.0);
    scene.paint();
    scene.complete();
}

/// A panicking listener leaves the others heard, and the next change is
/// delivered again.
///
/// Red-check: clear the queued flag after the listeners run (the panic
/// leaves it set and the second change is never delivered).
#[test]
fn a_panicking_listener_keeps_the_next_delivery() {
    let scene = Scene::new(20.0);
    let watch = scene.watch(scene.link);
    watch.add_listener(Rc::new(|| panic!("a listener fails")));
    let heard = counted(&watch);

    scene.paint();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.complete()));
    assert!(failed.is_err(), "the listener's panic resumes");
    assert_eq!(heard.get(), 1, "the listener after it still ran");

    scene.resize(30.0);
    scene.paint();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.complete()));
    assert_eq!((heard.get(), height(&watch)), (2, Some(30.0)));
}
