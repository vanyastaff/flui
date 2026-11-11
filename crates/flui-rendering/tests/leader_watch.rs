//! A `LeaderWatch` hears its leader's geometry change after the frame that
//! painted it, and only then: a steady layout delivers nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_foundation::notifier::Listenable;
use flui_layer::LayerLink;
use flui_objects::{RenderFlex, RenderLeaderLayer, RenderSizedBox};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::{LeaderWatch, PipelineCell, PipelineOwner},
    testing::{SceneHost, box_node, tree, update_render_object},
};
use flui_scheduler::{IdleDeadline, Instant, OwnerFrame, UpdateScheduler};
// Lower-crate test drives a frame through the composition-root seam
// (ADR-0081 §4) until the raw-owner suites mount under the UI runtime's
// root scopes (ADR-0083 6b).
use flui_scheduler::__runtime::DriveFrame as _;

/// A pipeline painting one leader around a box `height` tall, and the
/// scheduler whose post-frame lane its watches deliver through.
struct Scene {
    cell: PipelineCell,
    host: RefCell<SceneHost>,
    scheduler: UpdateScheduler,
    frame: OwnerFrame,
    sized: flui_foundation::RenderId,
    /// A box before the leader, whose width moves it.
    lead: flui_foundation::RenderId,
    link: LayerLink,
}

impl Scene {
    fn new(height: f64) -> Self {
        let link = LayerLink::new();
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let (root, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row())
                .child(box_node(RenderSizedBox::fixed(0.0, 10.0)).label("lead"))
                .child(
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
            host: RefCell::new(SceneHost::new()),
            scheduler,
            frame,
            sized: registry.get("sized").expect("labelled"),
            lead: registry.get("lead").expect("labelled"),
            link,
        }
    }

    fn watch(&self, link: LayerLink) -> LeaderWatch {
        self.cell.watch_leader(link, self.frame.post_frame_handle())
    }

    /// Lays out and paints whatever is dirty.
    fn paint(&self) {
        self.host
            .borrow_mut()
            .run_cell_frame(&self.cell)
            .expect("the frame runs");
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

    /// Moves the leader `width` along the row, leaving its size.
    fn shift(&self, width: f64) {
        self.cell.with_mut(|owner| {
            update_render_object::<RenderSizedBox, _>(owner, self.lead, |lead| {
                lead.set_size(Some(width), Some(10.0))
            });
        });
    }
}

fn counted(watch: &LeaderWatch) -> (Rc<Cell<usize>>, flui_foundation::Subscription) {
    let count = Rc::new(Cell::new(0));
    let counter = Rc::clone(&count);
    let listening = watch.listen(Rc::new(move || counter.set(counter.get() + 1)));
    (count, listening)
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
    let (heard, _listening) = counted(&watch);

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
    let (heard, _listening) = counted(&watch);
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
    let (heard, _listening) = counted(&watch);
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
    let _listening = watch.listen(Rc::new(move || {
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
    let _listening = watch.listen(Rc::new(|| panic!("a listener fails")));
    let (heard, _listening) = counted(&watch);

    scene.paint();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.complete()));
    assert!(failed.is_err(), "the listener's panic resumes");
    assert_eq!(heard.get(), 1, "the listener after it still ran");

    scene.resize(30.0);
    scene.paint();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.complete()));
    assert_eq!((heard.get(), height(&watch)), (2, Some(30.0)));
}

/// A watch made after its leader painted starts from the committed geometry,
/// though no later frame paints.
///
/// Red-check: start every watch at `None` (it stays `None` while the scene
/// holds).
#[test]
fn a_late_watch_starts_from_the_committed_leader() {
    let scene = Scene::new(20.0);
    scene.paint();
    scene.complete();
    let watch = scene.watch(scene.link);
    let (heard, _listening) = counted(&watch);
    scene.paint();
    scene.complete();
    assert_eq!((heard.get(), height(&watch)), (0, Some(20.0)));
}

/// A leader moved without a resize is heard, with where it now stands: a
/// follower placed against the window learns its leader scrolled.
///
/// Red-check: leave the origin out of the compared geometry (the move is
/// not heard).
#[test]
fn a_moved_leader_is_heard() {
    let scene = Scene::new(20.0);
    let watch = scene.watch(scene.link);
    let (heard, _listening) = counted(&watch);
    scene.paint();
    scene.complete();
    let origin = |watch: &LeaderWatch| watch.geometry().and_then(|geometry| geometry.origin);
    assert_eq!(heard.get(), 1);
    assert_eq!(origin(&watch).map(|point| point.x), Some(0.0));

    scene.shift(30.0);
    scene.paint();
    scene.complete();
    assert_eq!(heard.get(), 2, "the move is heard");
    assert_eq!(origin(&watch).map(|point| point.x), Some(30.0));
    assert_eq!(height(&watch), Some(20.0), "the size stays");
}
