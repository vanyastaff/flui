#![cfg(test)]

use flui::animation::Animation;
use flui::geometry::{Offset, Point, Rect, Size};
use flui::painting::{Canvas, CustomPainter, DrawOp, paint::Paint};
use flui::prelude::*;
use flui::rendering::{
    BoxConstraints, BoxDryLayoutContext, BoxLayoutContext, BoxParentData, BoxProtocol, Leaf, PaintContext,
    RenderBox, RenderUpdateImpact, Single,
};
use flui::testing::{Tester, find};
use flui::view::{RenderObjectContext, RenderView};
use flui::widgets::AnimatedBuilder;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Debug)]
struct RectanglePainter;

impl CustomPainter for RectanglePainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        canvas.draw_rect(
            Rect::from_origin_size(Point::ZERO, size),
            &Paint::fill(Color::rgb(10, 20, 30)),
        );
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn repaint(&self) -> Option<Rc<dyn flui::foundation::Listenable>> {
        None
    }

    fn semantics_regions(&self, size: Size) -> Vec<flui::painting::SemanticsRegion> {
        let mut config = flui::widgets::SemanticsConfiguration::new();
        config.set_label("painted rectangle");
        vec![flui::painting::SemanticsRegion::new(
            flui::painting::SemanticsRegionKey(1),
            Rect::from_origin_size(Point::ZERO, size),
            config,
        )]
    }

}

#[test]
fn custom_painter_records_a_rectangle() {
    let mut canvas = Canvas::new();
    RectanglePainter.paint(&mut canvas, Size::new(24.0, 16.0));
    let recording = canvas.finish();
    assert_eq!(recording.iter().len(), 1);
    let Some(DrawOp::Rect { rect, paint }) = recording.get(0).map(|command| command.op) else {
        panic!("painter must record a rectangle");
    };
    assert_eq!(
        *rect,
        Rect::from_origin_size(Point::ZERO, Size::new(24.0, 16.0))
    );
    assert_eq!(paint.color, Color::rgb(10, 20, 30));

    let mut t = Tester::mount(Center::new().child(
        flui::widgets::CustomPaint::new().painter(RectanglePainter)
            .size(Size::new(24.0, 16.0)),
    ));
    let bounds = t.rect(&find::text("painted rectangle"));
    assert_eq!((bounds.width, bounds.height), (24.0, 16.0));
}

#[derive(Debug, Clone, flui::animation::MotionValue)]
struct Appearance {
    position: Offset,
    color: Color,
}

#[derive(Debug, Clone, flui::animation::MotionValue)]
struct CardMotion(Appearance, f64);

#[derive(Debug)]
struct MotionPainter(flui::animation::AnimatedValueView<CardMotion>);

impl CustomPainter for MotionPainter {
    fn paint(&self, canvas: &mut Canvas, _size: Size) {
        let sample = self.0.value();
        canvas.draw_rect(
            Rect::from_origin_size(
                Point::ZERO + sample.0.position,
                Size::new(10.0 + sample.1 * 10.0, 10.0),
            ),
            &Paint::fill(sample.0.color),
        );
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool { false }
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn repaint(&self) -> Option<Rc<dyn flui::foundation::Listenable>> {
        Some(Rc::new(self.0.clone()))
    }
}

/// A card whose motion its state owns, on the frame registry of the
/// subtree, as an application writes one: the state is born with its
/// motion. The test drives the motion through the shared owner and counts
/// the card's builds.
#[derive(Clone, StatefulView)]
struct MotionCard {
    owner: Rc<RefCell<Option<flui::animation::AnimatedValue<CardMotion>>>>,
    builds: Rc<std::cell::Cell<usize>>,
}

struct MotionCardState {
    animation: flui::animation::AnimatedValueView<CardMotion>,
}

impl StatefulView for MotionCard {
    type State = MotionCardState;
    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        use flui::animation::{AnimatedValue, Linear, MotionSpec, SharedCurve};
        let vsync = flui::view::environment::VsyncScope::maybe_of(ctx);
        let owner = AnimatedValue::try_new(
            CardMotion(Appearance { position: Offset::ZERO, color: Color::rgb(255, 0, 0) }, 0.0),
            MotionSpec::Curve { duration: Duration::from_secs(1), curve: SharedCurve::new(Linear) },
            vsync.as_ref(),
        ).expect("finite custom motion");
        let animation = owner.animation();
        *self.owner.borrow_mut() = Some(owner);
        MotionCardState { animation }
    }
}

impl ViewState<MotionCard> for MotionCardState {
    fn build(&self, view: &MotionCard, _: &dyn BuildContext) -> impl IntoView {
        view.builds.set(view.builds.get() + 1);
        flui::widgets::CustomPaint::new()
            .size(Size::new(100.0, 100.0))
            .painter(MotionPainter(self.animation.clone()))
    }
}

#[test]
fn nested_custom_motion_repaints_and_retargets_through_the_facade() {
    fn sample(t: &mut Tester) -> (Rect, Color) {
        let rectangles: Vec<_> = t.draw_commands(&find::id("card")).into_iter().filter_map(|command| {
            match command.op {
                DrawOp::Rect { rect, paint } => Some((rect, paint.color)),
                _ => None,
            }
        }).collect();
        assert_eq!(rectangles.len(), 1, "one actual rectangle in the submitted scene");
        rectangles[0]
    }

    let card = MotionCard { owner: Rc::default(), builds: Rc::default() };
    let mut t = Tester::mount(Center::new().child(
        flui::widgets::Semantics::new().container(true).role(SemanticsRole::Group).identifier("card").child(card.clone()),
    ));
    assert_eq!(sample(&mut t), (
        Rect::from_origin_size(Point::ZERO, Size::new(10.0, 10.0)),
        Color::rgb(255, 0, 0),
    ));
    let builds = card.builds.get();
    let first = card.owner.borrow_mut().as_mut().expect("mounted").animate_to(CardMotion(
        Appearance { position: Offset::new(20.0, 40.0), color: Color::rgba(0, 0, 255, 0) },
        1.0,
    )).expect("first custom motion");
    // A read runs the frame the motion asked for at once, which anchors the
    // run before the presentation's time advances.
    let _ = sample(&mut t);
    t.pump_for(Duration::from_millis(500));
    assert_eq!(card.builds.get(), builds, "custom motion only invalidates paint");
    let seam = sample(&mut t);
    assert_eq!(seam.0, Rect::from_origin_size(Point::new(10.0, 20.0), Size::new(15.0, 10.0)));
    let [red, _, blue, alpha] = seam.1.to_rgba_f32_array();
    assert!(red >= 0.999 && blue <= 0.001 && (alpha - 0.5).abs() <= 1e-6);
    let (velocity, second) = {
        let mut owner = card.owner.borrow_mut();
        let owner = owner.as_mut().expect("mounted");
        let velocity = owner.velocity();
        let second = owner.animate_to(CardMotion(
            Appearance { position: Offset::new(-20.0, -40.0), color: Color::rgb(0, 255, 0) },
            2.0,
        )).expect("interrupted custom motion");
        (velocity, second)
    };
    assert!(first.is_canceled() && second.is_pending());
    assert_eq!(sample(&mut t), seam, "the submitted frame keeps its seam");
    t.pump_for(Duration::from_micros(1));
    assert_eq!(card.builds.get(), builds, "custom motion only invalidates paint");
    let after = sample(&mut t);
    for (delta, incoming) in [
        (after.0.min.x - seam.0.min.x, velocity[0]),
        (after.0.min.y - seam.0.min.y, velocity[1]),
    ] {
        assert!((delta / 1e-6 - incoming).abs() < 0.001);
    }
    t.pump_for(Duration::from_secs(2));
    assert_eq!(card.builds.get(), builds, "custom motion only invalidates paint");
    assert!(second.is_complete());
    assert_eq!(sample(&mut t), (
        Rect::from_origin_size(Point::new(-20.0, -40.0), Size::new(30.0, 10.0)),
        Color::rgb(0, 255, 0),
    ));
}


#[derive(Debug, flui::Inspect)]
struct SolidLeaf {
    #[inspect(skip)]
    paints: Arc<AtomicUsize>,
}

impl RenderBox for SolidLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> flui::rendering::RenderResult<Size> {
        Ok(ctx.constrain(Size::new(24.0, 16.0)))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutContext<'_>,
    ) -> flui::rendering::RenderResult<Size> {
        Ok(constraints.constrain(Size::new(24.0, 16.0)))
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) {
        let size = ctx.size();
        RectanglePainter.paint(ctx.canvas(), size);
        self.paints.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct LeafView {
    paints: Arc<AtomicUsize>,
}

impl RenderView for LeafView {
    type Protocol = BoxProtocol;
    type RenderObject = SolidLeaf;

    fn create_render_object(&self, _: &RenderObjectContext<'_>) -> SolidLeaf {
        SolidLeaf {
            paints: self.paints.clone(),
        }
    }

    fn update_render_object(
        &self,
        _: &RenderObjectContext<'_>,
        _: &mut SolidLeaf,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

flui::view::impl_render_view!(LeafView);

#[test]
fn custom_render_view_mounts_lays_out_and_paints() {
    let paints = Arc::new(AtomicUsize::new(0));
    let mut t = Tester::mount(Center::new().child(
        flui::widgets::Semantics::new()
            .container(true)
            .role(SemanticsRole::Group)
            .identifier("leaf")
            .child(LeafView { paints: paints.clone() }),
    ));
    let bounds = t.rect(&find::id("leaf"));
    assert_eq!((bounds.width, bounds.height), (24.0, 16.0));
    assert_eq!(paints.load(Ordering::SeqCst), 1);
}

#[derive(Debug, flui::Inspect)]
struct TransparentProxy {
    has_child: bool,
}

impl RenderBox for TransparentProxy {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui::rendering::forward_single_child_box_layout!();
    flui::rendering::forward_single_child_box_queries!();
    flui::rendering::forward_single_child_box_hit_test!();

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        ctx.paint_child();
    }
}

#[test]
fn proxy_macros_forward_live_and_dry_layout() {
    use flui::testing::rendering::{BoxQueryRun, Probe, RenderTester, box_node};
    let mut run = RenderTester::mount(box_node(TransparentProxy { has_child: false }).child(
        box_node(SolidLeaf {
            paints: Arc::new(AtomicUsize::new(0)),
        }),
    ))
    .with_constraints(BoxConstraints::loose(Size::new(100.0, 100.0)))
    .run_layout();
    let root = run.root();
    assert_eq!(run.box_geometry(root), Size::new(24.0, 16.0));
    let forced = Size::new(37.0, 19.0);
    assert_eq!(run.dry_layout(root, BoxConstraints::tight(forced)), forced);
}

#[test]
fn gesture_recognizer_uses_headless_virtual_time() {
    let fired = Arc::new(AtomicUsize::new(0));
    let callback = fired.clone();
    let mut t = Tester::mount(Center::new().child(
        flui::widgets::Semantics::new()
            .container(true)
            .role(SemanticsRole::Group)
            .identifier("press")
            .child(
                GestureDetector::new()
                    .on_long_press(move |_ctx| {
                        callback.fetch_add(1, Ordering::SeqCst);
                    })
                    .behavior(HitTestBehavior::Opaque)
                    .child(SizedBox::new(40.0, 30.0)),
            ),
    ));
    // The long-press deadline runs on the test's clock: a press released
    // before 500 ms of it is no long press, one held past it is.
    t.long_press(&find::id("press"), Duration::from_millis(300));
    assert_eq!(fired.load(Ordering::SeqCst), 0);
    t.long_press(&find::id("press"), Duration::from_millis(550));
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

#[test]
fn pointer_input_schedules_a_widget_rebuild() {
    let changed = Rc::new(flui::foundation::ChangeNotifier::new());
    let value = Arc::new(AtomicUsize::new(0));
    let builds = Arc::new(AtomicUsize::new(0));
    let read = value.clone();
    let builds_in_callback = builds.clone();
    let notification = changed.clone();
    let count = value.clone();
    let view = AnimatedBuilder::new(changed, move || {
        builds_in_callback.fetch_add(1, Ordering::SeqCst);
        let notification = notification.clone();
        let count = count.clone();
        GestureDetector::new()
            .on_tap(move |_ctx| {
                count.fetch_add(1, Ordering::SeqCst);
                notification.notify_listeners();
            })
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(
                40.0 + read.load(Ordering::SeqCst) as f64,
                30.0,
            ))
    });
    let mut t = Tester::mount(Center::new().child(
        flui::widgets::Semantics::new().container(true).role(SemanticsRole::Group).identifier("target").child(view),
    ));
    let initial_builds = builds.load(Ordering::SeqCst);
    assert_eq!(t.rect(&find::id("target")).width, 40.0);
    t.tap(&find::id("target"));
    assert_eq!(value.load(Ordering::SeqCst), 1);
    assert!(builds.load(Ordering::SeqCst) > initial_builds);
    assert_eq!(t.rect(&find::id("target")).width, 41.0);
}

#[test]
fn typed_drag_down_callback_is_available_from_the_widget_surface() {
    let downs = Arc::new(AtomicUsize::new(0));
    let observed = downs.clone();
    let mut t = Tester::mount(Center::new().child(
        flui::widgets::Semantics::new().container(true).role(SemanticsRole::Group).identifier("drag").child(
            GestureDetector::new()
                .drag(flui::widgets::DragSlot::horizontal().on_down(
                    move |_ctx, _details: flui::widgets::DragDownDetails| {
                        observed.fetch_add(1, Ordering::SeqCst);
                    },
                ))
                .behavior(HitTestBehavior::Opaque)
                .child(SizedBox::new(40.0, 30.0)),
        ),
    ));
    // A press that never moves: the callback runs on the pointer's down,
    // with no drag accepted.
    t.tap(&find::id("drag"));
    assert_eq!(downs.load(Ordering::SeqCst), 1);
}

struct DistanceRecognizer {
    contact: flui::interaction::PrimaryContact,
    threshold: f64,
    accepted: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
}

impl flui::interaction::GestureArenaMember for DistanceRecognizer {
    fn accept_gesture(&self, contest: flui::interaction::GestureContest) {
        assert!(self.contact.tracks_contest(contest));
        self.accepted.fetch_add(1, Ordering::SeqCst);
    }

    fn reject_gesture(&self, contest: flui::interaction::GestureContest) {
        assert!(self.contact.tracks_contest(contest));
        self.rejected.fetch_add(1, Ordering::SeqCst);
    }
}

impl flui::interaction::GestureRecognizer for DistanceRecognizer {
    fn add_pointer(&self, dispatch: flui::interaction::PointerDispatch<'_>) {
        self.contact
            .begin(dispatch, &flui::interaction::GestureSettings::default())
            .expect("fixture contact admitted");
    }

    fn handle_event(&self, dispatch: flui::widgets::PointerDispatch<'_>) {
        use flui::interaction::{PointerEvent, PointerEventExt};
        if let PointerEvent::Move(_) = dispatch.local {
            let origin = self.contact.current().expect("tracked pointer").local;
            if dispatch.local.position().expect("move position").dx - origin.dx >= self.threshold {
                self.contact.accept();
            }
        }
    }

    fn cancel(&self) -> flui::interaction::CancelOutcome {
        if self.contact.withdraw().is_some() {
            flui::interaction::CancelOutcome::Cancelled
        } else {
            flui::interaction::CancelOutcome::Idle
        }
    }
}

/// Two downstream recognizers, built on the presentation's arena when the
/// state mounts and attached to one listener.
#[derive(Clone, StatefulView)]
struct ArenaProbe {
    recognizers: Rc<RefCell<Vec<Rc<DistanceRecognizer>>>>,
}

struct ArenaProbeState;

impl StatefulView for ArenaProbe {
    type State = ArenaProbeState;
    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        use flui::interaction::{ArenaMembership, PrimaryContact};
        let arena = flui::view::environment::GestureArenaScope::of(ctx);
        let recognizer = |threshold| {
            Rc::new_cyclic(|this: &std::rc::Weak<DistanceRecognizer>| DistanceRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(arena.clone(), this.clone())),
                threshold,
                accepted: Arc::new(AtomicUsize::new(0)),
                rejected: Arc::new(AtomicUsize::new(0)),
            })
        };
        // The arena's sweep on the pointer's release awards the first
        // member, so the one that must win by distance is the second.
        *self.recognizers.borrow_mut() = vec![recognizer(20.0), recognizer(10.0)];
        ArenaProbeState
    }
}

impl ViewState<ArenaProbe> for ArenaProbeState {
    fn build(&self, view: &ArenaProbe, _: &dyn BuildContext) -> impl IntoView {
        let recognizers = view.recognizers.borrow();
        flui::widgets::Semantics::new().container(true).role(SemanticsRole::Group).identifier("arena").child(
            flui::widgets::Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .recognizer(&recognizers[0])
                .recognizer(&recognizers[1])
                .child(SizedBox::new(100.0, 30.0)),
        )
    }
}

#[test]
fn downstream_custom_recognizer_competes_in_the_arena() {
    use flui::interaction::GestureRecognizer;

    let probe = ArenaProbe { recognizers: Rc::default() };
    let mut t = Tester::mount(Center::new().child(probe.clone()));
    // Past the near recognizer's threshold and short of the far one's: the
    // near one claims the contact and the arena rejects the far one, though
    // the far one is the member a sweep would award.
    t.drag(&find::id("arena"), 15.0, 0.0, Duration::from_millis(100));
    let recognizers = probe.recognizers.borrow();
    let [far, near] = &recognizers[..] else {
        panic!("the state built two recognizers");
    };
    assert_eq!(near.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(near.rejected.load(Ordering::SeqCst), 0);
    assert_eq!(far.accepted.load(Ordering::SeqCst), 0);
    assert_eq!(far.rejected.load(Ordering::SeqCst), 1);
    for candidate in [far, near] {
        candidate.cancel();
        assert!(candidate.contact.current().is_none());
    }
}


use flui::view::{
    AsyncDriver, BoxedTask, FrameId, FrameTiming, Instant, PostFrameHandle,
    PostFrameScheduleError, TaskToken,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct LifecycleCapabilities {
    driver: Option<AsyncDriver>,
    task: Option<TaskToken>,
    pending: Option<TaskToken>,
    post_frame: Option<PostFrameHandle>,
}

#[derive(Clone, StatefulView)]
struct CapabilityView {
    capabilities: Rc<RefCell<LifecycleCapabilities>>,
    completed: Arc<AtomicUsize>,
    pending_polled: Arc<AtomicUsize>,
    pending_dropped: Arc<AtomicUsize>,
    callbacks: Rc<RefCell<Vec<&'static str>>>,
    shared_callback: Arc<AtomicUsize>,
}

struct CapabilityState(CapabilityView);
impl StatefulView for CapabilityView {
    type State = CapabilityState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        CapabilityState(self.clone())
    }
}

fn inspect_timing(timing: &FrameTiming) {
    let _: FrameId = timing.id;
    let _: Instant = timing.start_time;
}

struct PendingCapabilityTask {
    polled: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}
impl std::future::Future for PendingCapabilityTask {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        self.polled.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Pending
    }
}
impl Drop for PendingCapabilityTask {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl ViewState<CapabilityView> for CapabilityState {
    fn init_state(&mut self, context: &dyn LifecycleContext) {
        let driver: AsyncDriver = context.async_driver().expect("bound async driver");
        let post_frame: PostFrameHandle = context.post_frame_handle().expect("post-frame handle");
        let done = self.0.completed.clone();
        let task: BoxedTask = Box::pin(async move {
            done.fetch_add(1, Ordering::SeqCst);
        });
        let token = driver.spawn_local(task);
        let pending = driver.spawn_local(Box::pin(PendingCapabilityTask {
            polled: self.0.pending_polled.clone(),
            dropped: self.0.pending_dropped.clone(),
        }));
        let shared = self.0.shared_callback.clone();
        post_frame
            .schedule(move |timing: &FrameTiming| {
                inspect_timing(timing);
                shared.fetch_add(1, Ordering::SeqCst);
            })
            .expect("live owner");
        let callbacks = self.0.callbacks.clone();
        let scheduled: Result<(), PostFrameScheduleError> =
            post_frame.schedule(move |timing: &FrameTiming| {
                inspect_timing(timing);
                callbacks.borrow_mut().push("local");
            });
        scheduled.expect("live local lane");
        *self.0.capabilities.borrow_mut() = LifecycleCapabilities {
            driver: Some(driver),
            task: Some(token),
            pending: Some(pending),
            post_frame: Some(post_frame),
        };
    }
    fn build(&self, _: &CapabilityView, _: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(20.0, 20.0)
    }
}

#[test]
fn lifecycle_capabilities_are_named_and_run_through_the_facade() {
    let view = CapabilityView {
        capabilities: Rc::default(),
        completed: Arc::default(),
        pending_polled: Arc::default(),
        pending_dropped: Arc::default(),
        callbacks: Rc::default(),
        shared_callback: Arc::default(),
    };
    let mut t = Tester::mount(Center::new().child(view.clone()));
    t.pump();
    assert_eq!(view.completed.load(Ordering::SeqCst), 1);
    assert_eq!(view.pending_polled.load(Ordering::SeqCst), 1);
    assert_eq!(view.shared_callback.load(Ordering::SeqCst), 1);
    assert_eq!(&*view.callbacks.borrow(), &["local"]);
    let capabilities = view.capabilities.borrow();
    assert!(capabilities.task.is_some());
    assert!(capabilities.post_frame.is_some());
    let pending = capabilities.pending.as_ref().expect("retained token");
    pending.cancel();
    assert!(pending.is_cancelled());
    assert_eq!(view.pending_dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        capabilities
            .driver
            .as_ref()
            .expect("driver")
            .pending_task_count(),
        0
    );
    drop(capabilities);
    t.pump();
    assert_eq!(view.pending_polled.load(Ordering::SeqCst), 1);
}

use flui::interaction::{
    ClientToken, Code, DetachOutcome, FocusAttachment, FocusChangeCallback, FocusDetachOutcome,
    FocusManager, FocusNode, FocusNodeChangeCallback, FocusNodeId, FocusNodeRegistration,
    FocusRequestOutcome, FocusScopeNode, FocusTraversalPolicy, FocusTreeError, HitTestEntry,
    HitTestHandle, HitTestSnapshot, InteractionDispatchError, Key, KeyCancelHandler, KeyEvent,
    KeyEventHandler, KeyEventResult, KeyState, Location, Modifiers, NamedKey, ReadingOrderPolicy, RectProvider,
    ResolvedStep, TextInputClient, TextInputError, TextInputHandle, TraversalEdgeBehavior,
};

#[test]
fn interaction_callback_vocabulary_is_nameable_through_the_facade() {
    fn nameable<T: ?Sized>() {
        assert!(!std::any::type_name::<T>().is_empty());
    }
    nameable::<ClientToken>();
    nameable::<DetachOutcome>();
    nameable::<TextInputClient>();
    nameable::<TextInputError>();
    nameable::<FocusAttachment>();
    nameable::<FocusChangeCallback>();
    nameable::<FocusDetachOutcome>();
    nameable::<FocusNodeChangeCallback>();
    nameable::<FocusNodeId>();
    nameable::<FocusNodeRegistration>();
    nameable::<FocusScopeNode>();
    nameable::<dyn FocusTraversalPolicy>();
    nameable::<FocusTreeError>();
    nameable::<HitTestEntry>();
    nameable::<HitTestSnapshot>();
    nameable::<InteractionDispatchError>();
    nameable::<KeyCancelHandler>();
    nameable::<KeyEventHandler>();
    nameable::<KeyEventResult>();
    nameable::<ReadingOrderPolicy>();
    nameable::<RectProvider>();
    nameable::<ResolvedStep>();
    nameable::<TraversalEdgeBehavior>();
    nameable::<KeyEvent>();
    nameable::<Code>();
    nameable::<Key>();
    nameable::<KeyState>();
    nameable::<Location>();
    nameable::<Modifiers>();
    nameable::<NamedKey>();
}

#[derive(Clone, StatefulView)]
struct FocusCapabilityView {
    node: Rc<FocusNode>,
    handles: Rc<RefCell<Option<InteractionCapabilities>>>,
    changes: Rc<RefCell<Vec<bool>>>,
}
struct InteractionCapabilities {
    focus: Rc<FocusManager>,
    hit_test: Option<HitTestHandle>,
    text_input: Option<TextInputHandle>,
}
struct FocusCapabilityState(FocusCapabilityView);
impl StatefulView for FocusCapabilityView {
    type State = FocusCapabilityState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        FocusCapabilityState(self.clone())
    }
}
impl ViewState<FocusCapabilityView> for FocusCapabilityState {
    fn init_state(&mut self, context: &dyn LifecycleContext) {
        *self.0.handles.borrow_mut() = Some(InteractionCapabilities {
            focus: context.focus_manager(),
            hit_test: context.hit_test_handle(),
            text_input: context.text_input_handle(),
        });
    }
    fn build(&self, _: &FocusCapabilityView, _: &dyn BuildContext) -> impl IntoView {
        let changes = self.0.changes.clone();
        flui::widgets::Focus::new(SizedBox::new(30.0, 30.0))
            .focus_node(self.0.node.clone())
            .on_focus_change(move |_ctx, focused| changes.borrow_mut().push(focused))
    }
}

#[test]
fn retained_focus_node_uses_context_capabilities_through_the_facade() {
    let view = FocusCapabilityView {
        node: FocusNode::new(),
        handles: Rc::default(),
        changes: Rc::default(),
    };
    let mut t = Tester::mount(Center::new().child(view.clone()));
    let outcome: FocusRequestOutcome = view.node.request_focus();
    assert_eq!(outcome, FocusRequestOutcome::Focused);
    t.pump();
    assert!(view.node.has_primary_focus());
    assert!(view.changes.borrow().contains(&true));
    let handles = view.handles.borrow();
    let handles = handles.as_ref().expect("init_state capabilities");
    assert!(handles.focus.primary_focus().is_some());
    assert!(handles.hit_test.is_some());
    // The ui_runtime installs its presentation's text-input capability, as on
    // screen; the headless window offers no platform input method, so this
    // checks typed capability access, not operating-system text-input behavior.
    assert!(handles.text_input.is_some());
    view.node.unfocus();
    t.pump();
    assert!(!view.node.has_primary_focus());
    assert!(view.changes.borrow().contains(&false));
}

#[derive(Clone, StatefulView)]
struct LifecycleProbe {
    captured: Rc<RefCell<Option<flui::view::LifecycleHandle>>>,
    events: Rc<RefCell<Vec<flui::view::AppLifecycleState>>>,
}
/// Born subscribed: the subscription lives as long as the state.
struct LifecycleProbeState {
    _subscription: flui::view::LifecycleSubscription,
}
impl StatefulView for LifecycleProbe {
    type State = LifecycleProbeState;
    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let handle = ctx.lifecycle_handle().expect("a mounted application has a lifecycle");
        let events = self.events.clone();
        let (initial, subscription) = handle
            .subscribe(move |state| events.borrow_mut().push(state))
            .expect("open");
        // The window opens focused and shown.
        assert_eq!(initial, Some(flui::view::AppLifecycleState::Resumed));
        assert!(
            self.events.borrow().is_empty(),
            "no callback before token storage"
        );
        *self.captured.borrow_mut() = Some(handle);
        LifecycleProbeState { _subscription: subscription }
    }
}
impl ViewState<LifecycleProbe> for LifecycleProbeState {
    fn build(&self, _: &LifecycleProbe, _: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(20.0, 20.0)
    }
}

#[test]
fn presentation_lifecycle_subscription_runs_through_the_facade() {
    use flui::view::{AppLifecycleState, LifecycleClosed};
    let view = LifecycleProbe {
        captured: Rc::default(),
        events: Rc::default(),
    };
    let mut t = Tester::mount(Center::new().child(view.clone()));
    let handle = view.captured.borrow().as_ref().expect("captured").clone();
    t.set_window_focused(false);
    t.set_window_visible(false);
    let extra = Rc::new(RefCell::new(Vec::new()));
    let recorded = extra.clone();
    let (snapshot, token) = handle
        .subscribe(move |state| recorded.borrow_mut().push(state))
        .expect("second subscriber");
    assert_eq!(snapshot, Some(AppLifecycleState::Hidden));
    assert!(extra.borrow().is_empty());
    t.set_window_visible(true);
    assert_eq!(handle.snapshot(), Ok(Some(AppLifecycleState::Inactive)));
    assert_eq!(*extra.borrow(), [AppLifecycleState::Inactive]);
    drop(token);
    t.set_window_visible(false);
    assert!(t.request_close(), "nothing holds the window open");
    assert_eq!(
        *extra.borrow(),
        [AppLifecycleState::Inactive],
        "dropped token suppresses later events"
    );
    assert_eq!(
        *view.events.borrow(),
        [
            AppLifecycleState::Inactive,
            AppLifecycleState::Hidden,
            AppLifecycleState::Inactive,
            AppLifecycleState::Hidden,
            AppLifecycleState::Paused,
            AppLifecycleState::Detached
        ]
    );
    assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    assert!(handle.subscribe(|_| {}).is_err());
}

#[test]
fn secondary_window_entry_point_requires_a_running_application() {
    assert!(
        flui::app::open_secondary_window(
            flui::app::AppConfig::default(),
            flui::app::WindowPolicy::Isolated,
        )
        .is_err()
    );
}

/// The arguments `SemanticsConfiguration`'s setters take are named through `flui::semantics`, the
/// way a render object's `describe_semantics_configuration` writes them.
#[test]
fn semantics_vocabulary_is_named_through_the_facade() {
    use flui::semantics::{
        SemanticsAnchor, SemanticsCollection, SemanticsCollectionKind, SemanticsConfiguration,
        SemanticsRelation, SemanticsSortKey,
    };

    let mut config = SemanticsConfiguration::new();
    config.set_sort_key(SemanticsSortKey::named(2.0, "toolbar"));
    config.set_collection(SemanticsCollection::new(SemanticsCollectionKind::List, Some(3)));
    let anchor = SemanticsAnchor::new();
    config.add_relation(SemanticsRelation::LabelledBy, anchor.clone());

    assert_eq!(config.sort_key(), Some(&SemanticsSortKey::named(2.0, "toolbar")));
    assert_eq!(
        config.collection().map(SemanticsCollection::count),
        Some(Some(3))
    );
    assert_eq!(config.relations(), &[(SemanticsRelation::LabelledBy, anchor)]);
}
