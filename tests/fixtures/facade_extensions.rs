#![cfg(test)]

use flui::geometry::px;
use flui::painting::{Canvas, CustomPainter, DrawOp, Paint};
use flui::prelude::*;
use flui::rendering::{
    BoxConstraints, BoxDryLayoutCtx, BoxLayoutContext, BoxParentData, BoxProtocol, Leaf, PaintCx,
    RenderBox, RenderUpdateImpact, Single,
};
use flui::testing::widgets::{lay_out, loose};
use flui::types::{Point, Rect, Size};
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

    fn repaint(&self) -> Option<Arc<dyn flui::foundation::Listenable>> {
        None
    }

    fn semantics_builder(&self) -> Option<flui::painting::SemanticsBuilder> {
        None
    }
}

#[test]
fn custom_painter_records_a_rectangle() {
    let mut canvas = Canvas::new();
    RectanglePainter.paint(&mut canvas, Size::new(px(24.0), px(16.0)));
    let recording = canvas.finish();
    assert_eq!(recording.commands().len(), 1);
    let DrawOp::Rect { rect, paint } = &recording.commands()[0].op else {
        panic!("painter must record a rectangle");
    };
    assert_eq!(
        *rect,
        Rect::from_origin_size(Point::ZERO, Size::new(px(24.0), px(16.0)))
    );
    assert_eq!(paint.color, Color::rgb(10, 20, 30));
}

#[derive(Debug, flui::Diagnosticable)]
struct SolidLeaf {
    paints: Arc<AtomicUsize>,
}

impl RenderBox for SolidLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        ctx.constrain(Size::new(px(24.0), px(16.0)))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        constraints.constrain(Size::new(px(24.0), px(16.0)))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
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
    let tree = lay_out(
        LeafView {
            paints: paints.clone(),
        },
        loose(100.0),
    );
    assert_eq!(tree.size(tree.root()), Size::new(px(24.0), px(16.0)));
    assert_eq!(paints.load(Ordering::SeqCst), 1);
}

#[derive(Debug, flui::Diagnosticable)]
struct TransparentProxy {
    has_child: bool,
}

impl RenderBox for TransparentProxy {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui::rendering::forward_single_child_box_layout!();
    flui::rendering::forward_single_child_box_queries!();
    flui::rendering::forward_single_child_box_hit_test!();

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
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
    .with_constraints(loose(100.0))
    .run_layout();
    let root = run.root();
    assert_eq!(run.box_geometry(root), Size::new(px(24.0), px(16.0)));
    let forced = Size::new(px(37.0), px(19.0));
    assert_eq!(run.dry_layout(root, BoxConstraints::tight(forced)), forced);
}

#[test]
fn gesture_recognizer_uses_headless_virtual_time() {
    use flui::interaction::{
        GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
    };
    use flui::types::Offset;
    let mut binding = flui::testing::HeadlessBinding::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let callback = fired.clone();
    let recognizer = LongPressGestureRecognizer::with_settings(
        binding.arena().clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
    )
    .with_on_long_press_start(move |_details: flui::interaction::LongPressStartDetails| {
        callback.fetch_add(1, Ordering::SeqCst);
    });
    let position = Offset::new(px(8.0), px(8.0));
    recognizer.add_pointer(
        PointerId::new(1).expect("nonzero pointer"),
        position,
        position,
    );
    binding.pump_frame(Duration::from_millis(300));
    assert_eq!(fired.load(Ordering::SeqCst), 0);
    binding.pump_frame(Duration::from_millis(250));
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

#[test]
fn pointer_input_schedules_a_widget_rebuild() {
    let changed = Arc::new(flui::foundation::ChangeNotifier::new());
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
            .on_tap(move || {
                count.fetch_add(1, Ordering::SeqCst);
                notification.notify_listeners();
            })
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(
                40.0 + read.load(Ordering::SeqCst) as f32,
                30.0,
            ))
    });
    let mut tree = lay_out(view, loose(100.0));
    let initial_builds = builds.load(Ordering::SeqCst);
    assert_eq!(tree.size(tree.root()).width, px(40.0));
    tree.dispatch_pointer_down(10.0, 10.0);
    tree.dispatch_pointer_up(10.0, 10.0);
    tree.tick();
    assert_eq!(value.load(Ordering::SeqCst), 1);
    assert!(builds.load(Ordering::SeqCst) > initial_builds);
    assert_eq!(tree.size(tree.root()).width, px(41.0));
}

#[test]
fn typed_drag_down_callback_is_available_from_the_widget_surface() {
    let downs = Arc::new(AtomicUsize::new(0));
    let observed = downs.clone();
    let tree = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_down(move |_details: flui::widgets::DragDownDetails| {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(40.0, 30.0)),
        loose(100.0),
    );
    tree.dispatch_pointer_down(10.0, 10.0);
    assert_eq!(downs.load(Ordering::SeqCst), 1);
    tree.dispatch_pointer_up(10.0, 10.0);
}

#[derive(Clone, Debug)]
struct DistanceRecognizer {
    base: flui::interaction::RecognizerBase,
    threshold: f32,
    accepted: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
}

impl flui::interaction::CustomGestureRecognizer for DistanceRecognizer {
    fn on_arena_accept(&self, pointer: flui::interaction::PointerId) {
        assert_eq!(self.base.primary_pointer(), Some(pointer));
        self.accepted.fetch_add(1, Ordering::SeqCst);
    }

    fn on_arena_reject(&self, pointer: flui::interaction::PointerId) {
        assert_eq!(self.base.primary_pointer(), Some(pointer));
        self.rejected.fetch_add(1, Ordering::SeqCst);
    }
}

impl flui::interaction::GestureRecognizer for DistanceRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: flui::interaction::PointerId,
        position: flui::types::Offset,
        global_position: flui::types::Offset,
    ) {
        self.base
            .start_tracking(pointer, position, global_position, self);
    }

    fn handle_event(&self, dispatch: flui::widgets::PointerDispatch<'_>) {
        use flui::interaction::{PointerEvent, PointerEventExt};
        if let PointerEvent::Move(_) = dispatch.local {
            let origin = self.base.initial_position().expect("tracked pointer");
            if (dispatch.local.position().dx - origin.dx).get() >= self.threshold {
                self.base.accept_tracked();
            }
        }
    }

    fn dispose(&self) {
        self.base.stop_tracking();
        self.base.mark_disposed();
    }

    fn primary_pointer(&self) -> Option<flui::interaction::PointerId> {
        self.base.primary_pointer()
    }
}

#[test]
fn downstream_custom_recognizer_competes_in_the_arena() {
    use flui::interaction::{GestureArenaMember, GestureRecognizer, PointerId, RecognizerBase};
    use flui::testing::replay::{PointerPhase, ScriptedPointer};
    use flui::types::Offset;
    use flui::widgets::PointerDispatch;

    let binding = flui::testing::HeadlessBinding::new();
    let arena = binding.arena();
    let recognizer = |threshold| {
        Arc::new(DistanceRecognizer {
            base: RecognizerBase::new(arena.clone()),
            threshold,
            accepted: Arc::new(AtomicUsize::new(0)),
            rejected: Arc::new(AtomicUsize::new(0)),
        })
    };
    let first = recognizer(10.0);
    let second = recognizer(20.0);
    // The public custom-recognizer contract supplies the sealed arena-member
    // implementation. Registration below uses those exact Arc identities.
    let _member: Arc<dyn GestureArenaMember> = first.clone();
    let pointer = PointerId::new(11).expect("nonzero pointer");
    for candidate in [&first, &second] {
        candidate.add_pointer(pointer, Offset::ZERO, Offset::ZERO);
        assert_eq!(candidate.primary_pointer(), Some(pointer));
    }
    arena.close(pointer);

    let moved = |x| {
        ScriptedPointer::new(
            Duration::ZERO,
            pointer,
            PointerPhase::Move,
            Offset::new(px(x), px(0.0)),
        )
        .to_event()
    };
    let below_threshold = moved(5.0);
    for candidate in [&first, &second] {
        candidate.handle_event(PointerDispatch::at_root(&below_threshold));
        assert_eq!(candidate.accepted.load(Ordering::SeqCst), 0);
        assert_eq!(candidate.rejected.load(Ordering::SeqCst), 0);
    }

    let crossed_threshold = moved(15.0);
    first.handle_event(PointerDispatch::at_root(&crossed_threshold));
    assert_eq!(first.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(first.rejected.load(Ordering::SeqCst), 0);
    assert_eq!(second.accepted.load(Ordering::SeqCst), 0);
    assert_eq!(second.rejected.load(Ordering::SeqCst), 1);
    for candidate in [&first, &second] {
        candidate.dispose();
        assert_eq!(candidate.primary_pointer(), None);
    }
}

use flui::view::{
    AsyncDriver, BoxedTask, BudgetPercentage, FrameDuration, FramePhase, FrameTiming, Instant,
    InvalidDurationConfig, LocalPostFrameHandle, LocalPostFrameScheduleError, Microseconds,
    Milliseconds, PostFrameHandle, Seconds, TaskToken,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct LifecycleCapabilities {
    driver: Option<AsyncDriver>,
    task: Option<TaskToken>,
    pending: Option<TaskToken>,
    post_frame: Option<PostFrameHandle>,
    local_post_frame: Option<LocalPostFrameHandle>,
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
    fn create_state(&self) -> Self::State {
        CapabilityState(self.clone())
    }
}

fn inspect_timing(timing: &FrameTiming) {
    let _: flui::foundation::FrameId = timing.id;
    let _: Instant = timing.start_time;
    let phase: FramePhase = timing.phase;
    let duration: FrameDuration = timing.frame_duration;
    let elapsed: Milliseconds = timing.elapsed();
    let _: Seconds = elapsed.to_seconds();
    let _: Microseconds = elapsed.to_micros();
    let _: BudgetPercentage = timing.utilization();
    assert!(duration.as_ms().value() > 0.0);
    let _: Milliseconds = timing.phase_duration(phase);
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
    fn init_state(&mut self, context: &dyn BuildContext) {
        let driver: AsyncDriver = context.async_driver().expect("bound async driver");
        let post_frame: PostFrameHandle = context.post_frame_handle().expect("post-frame handle");
        let local: LocalPostFrameHandle = context.local_post_frame_handle().expect("local handle");
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
        post_frame.schedule(move |timing: &FrameTiming| {
            inspect_timing(timing);
            shared.fetch_add(1, Ordering::SeqCst);
        });
        let callbacks = self.0.callbacks.clone();
        let scheduled: Result<(), LocalPostFrameScheduleError> =
            local.schedule_local(move |timing: &FrameTiming| {
                inspect_timing(timing);
                callbacks.borrow_mut().push("local");
            });
        scheduled.expect("live local lane");
        *self.0.capabilities.borrow_mut() = LifecycleCapabilities {
            driver: Some(driver),
            task: Some(token),
            pending: Some(pending),
            post_frame: Some(post_frame),
            local_post_frame: Some(local),
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
    let mut tree = lay_out(view.clone(), loose(100.0));
    tree.tick();
    assert_eq!(view.completed.load(Ordering::SeqCst), 1);
    assert_eq!(view.pending_polled.load(Ordering::SeqCst), 1);
    assert_eq!(view.shared_callback.load(Ordering::SeqCst), 1);
    assert_eq!(&*view.callbacks.borrow(), &["local"]);
    let capabilities = view.capabilities.borrow();
    assert!(capabilities.task.is_some());
    assert!(capabilities.post_frame.is_some());
    assert!(capabilities.local_post_frame.is_some());
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
    tree.tick();
    assert_eq!(view.pending_polled.load(Ordering::SeqCst), 1);
    let error: InvalidDurationConfig = FrameDuration::try_from_fps(0).expect_err("zero fps");
    assert!(matches!(error, InvalidDurationConfig::ZeroFps));
}

use flui::interaction::{
    ClientToken, Code, DetachOutcome, FocusAttachment, FocusChangeCallback, FocusDetachOutcome,
    FocusManager, FocusNode, FocusNodeChangeCallback, FocusNodeId, FocusNodeRegistration,
    FocusRequestOutcome, FocusScopeNode, FocusTraversalPolicy, FocusTreeError, HitTestEntry,
    HitTestHandle, HitTestSnapshot, ImeEventCallback, InteractionDispatchError, Key, KeyEvent,
    KeyEventCallback, KeyEventHandler, KeyEventResult, KeyState, KeyboardEvent, Location,
    Modifiers, NamedKey, ReadingOrderPolicy, RectProvider, ResolvedStep, TextInputError,
    TextInputHandle, TraversalEdgeBehavior,
};

#[test]
fn interaction_callback_vocabulary_is_nameable_through_the_facade() {
    fn nameable<T: ?Sized>() {
        assert!(!std::any::type_name::<T>().is_empty());
    }
    nameable::<ClientToken>();
    nameable::<DetachOutcome>();
    nameable::<ImeEventCallback>();
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
    nameable::<KeyEventCallback>();
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
    nameable::<KeyboardEvent>();
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
    fn create_state(&self) -> Self::State {
        FocusCapabilityState(self.clone())
    }
}
impl ViewState<FocusCapabilityView> for FocusCapabilityState {
    fn init_state(&mut self, context: &dyn BuildContext) {
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
            .on_focus_change(move |focused| changes.borrow_mut().push(focused))
    }
}

#[test]
fn retained_focus_node_uses_context_capabilities_through_the_facade() {
    let view = FocusCapabilityView {
        node: FocusNode::new(),
        handles: Rc::default(),
        changes: Rc::default(),
    };
    let mut tree = lay_out(view.clone(), loose(100.0));
    let outcome: FocusRequestOutcome = view.node.request_focus();
    assert_eq!(outcome, FocusRequestOutcome::Focused);
    tree.tick();
    assert!(view.node.has_primary_focus());
    assert!(view.changes.borrow().contains(&true));
    let handles = view.handles.borrow();
    let handles = handles.as_ref().expect("init_state capabilities");
    assert!(handles.focus.primary_focus().is_some());
    assert!(handles.hit_test.is_some());
    // The headless fixture has no native IME owner: this checks typed capability
    // access, not operating-system text-input behavior.
    assert!(handles.text_input.is_none());
    view.node.unfocus();
    tree.tick();
    assert!(!view.node.has_primary_focus());
    assert!(view.changes.borrow().contains(&false));
}

#[derive(Clone, StatefulView)]
struct LifecycleProbe {
    expected: bool,
    initialized: Rc<std::cell::Cell<bool>>,
    captured: Rc<RefCell<Option<flui::view::LifecycleHandle>>>,
    events: Rc<RefCell<Vec<flui::view::AppLifecycleState>>>,
}
struct LifecycleProbeState {
    view: LifecycleProbe,
    subscription: Option<flui::view::LifecycleSubscription>,
}
impl StatefulView for LifecycleProbe {
    type State = LifecycleProbeState;
    fn create_state(&self) -> Self::State {
        LifecycleProbeState { view: self.clone(), subscription: None }
    }
}
impl ViewState<LifecycleProbe> for LifecycleProbeState {
    fn init_state(&mut self, context: &dyn BuildContext) {
        self.view.initialized.set(true);
        let handle = context.lifecycle_handle();
        assert_eq!(handle.is_some(), self.view.expected);
        let Some(handle) = handle else { return; };
        let events = self.view.events.clone();
        let (initial, subscription) = handle.subscribe(move |state| events.borrow_mut().push(state)).expect("open");
        assert_eq!(initial, None);
        assert!(self.view.events.borrow().is_empty(), "no callback before token storage");
        self.subscription = Some(subscription);
        *self.view.captured.borrow_mut() = Some(handle);
    }
    fn build(&self, _: &LifecycleProbe, _: &dyn BuildContext) -> impl IntoView { SizedBox::new(20.0, 20.0) }
}

#[test]
fn presentation_lifecycle_subscription_runs_through_the_facade() {
    use flui::view::{AppLifecycleState, LifecycleClosed};
    let view = LifecycleProbe { expected: true, initialized: Rc::default(), captured: Rc::default(), events: Rc::default() };
    let mut binding = flui::testing::HeadlessBinding::new();
    binding.mount_root(&view, flui::testing::MountOwners::fresh(), flui::testing::MountOptions::loose(100.0));
    let handle = view.captured.borrow().as_ref().expect("captured").clone();
    binding.set_lifecycle_state(AppLifecycleState::Detached).expect("observed detached");
    binding.set_lifecycle_state(AppLifecycleState::Resumed).expect("reversible");
    binding.set_lifecycle_state(AppLifecycleState::Hidden).expect("hide");
    let extra = Rc::new(RefCell::new(Vec::new()));
    let recorded = extra.clone();
    let (snapshot, token) = handle.subscribe(move |state| recorded.borrow_mut().push(state)).expect("second subscriber");
    assert_eq!(snapshot, Some(AppLifecycleState::Hidden));
    assert!(extra.borrow().is_empty());
    binding.set_lifecycle_state(AppLifecycleState::Inactive).expect("show unfocused");
    assert_eq!(handle.snapshot(), Ok(Some(AppLifecycleState::Inactive)));
    assert_eq!(*extra.borrow(), [AppLifecycleState::Inactive]);
    drop(token);
    binding.set_lifecycle_state(AppLifecycleState::Hidden).expect("hide again");
    binding.close_lifecycle();
    assert_eq!(*extra.borrow(), [AppLifecycleState::Inactive], "dropped token suppresses later events");
    assert_eq!(*view.events.borrow(), [AppLifecycleState::Detached, AppLifecycleState::Resumed, AppLifecycleState::Hidden, AppLifecycleState::Inactive, AppLifecycleState::Hidden, AppLifecycleState::Detached]);
    assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    assert!(handle.subscribe(|_| {}).is_err());
}


#[test]
fn presentation_lifecycle_capability_is_absent_when_not_installed() {
    let view = LifecycleProbe { expected: false, initialized: Rc::default(), captured: Rc::default(), events: Rc::default() };
    let mut binding = flui::testing::HeadlessBinding::new();
    binding.mount_root(&view, flui::testing::MountOwners::fresh(), flui::testing::MountOptions::loose(100.0).with_capabilities(flui::testing::BuildCapabilities::AsyncDriverOnly));
    assert!(view.initialized.get());
    assert!(view.captured.borrow().is_none());
}
