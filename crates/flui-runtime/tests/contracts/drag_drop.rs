//! OS drags reach the receivers under the pointer, a drag's value stays in its runtime, and the
//! host starts an OS session only once its callback is over (ADR-0211), exercised through the
//! headless platform's OS drag session and an [`OwnerHost`].

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::task::{Context, Poll, Waker};

use flui_foundation::geometry::{Offset, Point};
use flui_foundation::{PresentationAddress, PresentationId};
use flui_interaction::LocalPayloadTarget;
use flui_interaction::drag::{
    AllowedActions, DragFetch, DragHandle, DragItem, DragOutcome, DragPoint, DragScope,
    DragSession, DragStart, DragView, DropReceiver, DropReceiverSlot, MoveOutcome, ProcessNonce,
};
use flui_objects::RenderMetaData;
use flui_platform::{HeadlessDragAndDrop, HeadlessPlatform, HostWindow, Platform, WindowOptions};
use flui_platform_api::data_transfer::{
    DropFeedback, TransferActions, TransferFormat, TransferLimits, TransferPayload,
};
use flui_platform_api::drag::{DragError, OsDragStart};
use flui_platform_api::{DispatchEventResult, PlatformInput, PlatformWindow, WindowExecutionState};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_runtime::owner::{OwnerEffects, OwnerHost, PresentationDispatcher, RuntimeOperation};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_view::{Child, RenderObjectContext, RenderView, impl_render_view};
use flui_widgets::SizedBox;

use crate::HostInputExt as _;

const COPY: TransferActions = TransferActions::COPY;
const MOVE: TransferActions = TransferActions::MOVE;

/// What a test receiver saw of each drag it was asked about.
#[derive(Default)]
struct Seen {
    events: RefCell<Vec<String>>,
    scopes: RefCell<Vec<DragScope>>,
    values: RefCell<Vec<Option<i32>>>,
    fetches: RefCell<Vec<DragFetch>>,
}

struct Receiver {
    seen: Rc<Seen>,
    answer: Cell<Option<TransferActions>>,
    fetch_text: bool,
}

impl DropReceiver for Receiver {
    fn drag_entered(&self, drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        self.seen.events.borrow_mut().push("enter".to_owned());
        self.seen.scopes.borrow_mut().push(drag.scope());
        self.seen
            .values
            .borrow_mut()
            .push(drag.value::<i32>().copied());
        if self.fetch_text
            && let Some(fetch) = drag.request(&TransferFormat::Text, TransferLimits::default())
        {
            self.seen.fetches.borrow_mut().push(fetch);
        }
        DropFeedback {
            accept: self.answer.get(),
        }
    }

    fn drag_moved(&self, _drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        DropFeedback {
            accept: self.answer.get(),
        }
    }

    fn drag_left(&self, _drag: &DragView<'_>) {
        self.seen.events.borrow_mut().push("left".to_owned());
    }

    fn dropped(&self, drag: &DragView<'_>, _at: DragPoint, action: TransferActions) {
        self.seen
            .events
            .borrow_mut()
            .push(format!("dropped {}", action.bits()));
        self.seen
            .values
            .borrow_mut()
            .push(drag.value::<i32>().copied());
    }
}

/// A box that registers a receiver where hit tests find it.
#[derive(Clone)]
struct ReceiverAnchor {
    slot: Rc<DropReceiverSlot>,
    child: Child,
}

impl RenderView for ReceiverAnchor {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMetaData;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderMetaData::new();
        render_object.set_behavior(HitTestBehavior::Translucent);
        let target = ctx
            .register_local_payload(Rc::clone(&self.slot) as Rc<dyn std::any::Any>)
            .expect("the runtime's lane is active while mounting");
        render_object.set_shared_metadata(Some(Arc::new(target)));
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        debug_assert!(render_object.metadata_as::<LocalPayloadTarget>().is_some());
        flui_rendering::RenderUpdateImpact::NONE
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ReceiverAnchor);

fn receiver(answer: Option<TransferActions>, fetch_text: bool) -> (Rc<Seen>, ReceiverAnchor) {
    let seen = Rc::new(Seen::default());
    let slot = Rc::new(DropReceiverSlot(Rc::new(Receiver {
        seen: Rc::clone(&seen),
        answer: Cell::new(answer),
        fetch_text,
    })));
    (
        seen,
        ReceiverAnchor {
            slot,
            child: Child::some(SizedBox::new(300.0, 200.0)),
        },
    )
}

struct Sink;

impl flui_runtime::sink::FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }
    fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

/// Native effects these rows never need beyond what the owner asks of every host.
struct Effects;

impl OwnerEffects for Effects {
    fn finish_install(
        &self,
        _: PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
    fn frame(&self, _: PresentationAddress, _: &mut UiRuntime) {}
    fn commit_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        panic!("no pending install");
    }
    fn retire_host(&self, _: &[PresentationAddress], _: flui_runtime::owner::RecoveryState) {}
    fn cancel_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) {
        panic!("no pending install");
    }
    fn resize_surface(
        &self,
        _: PresentationAddress,
        _: flui_foundation::geometry::Size<f64>,
        _: f64,
    ) {
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
    fn request_continuation(&self) -> bool {
        true
    }
}

fn nonce() -> ProcessNonce {
    ProcessNonce::from_raw(NonZeroU64::new(0x000d_15c0).expect("nonzero"))
}

fn open(platform: &HeadlessPlatform) -> (Arc<dyn HostWindow>, PresentationWindow) {
    let window = platform
        .open_window(WindowOptions::default())
        .expect("headless window");
    let accessibility = window.accessibility();
    (
        Arc::clone(&window),
        PresentationWindow::new(window, accessibility),
    )
}

/// A runtime over `window` that answers the OS through `platform`'s one source, in the
/// platform instance `nonce` names.
fn runtime_over(
    platform: &HeadlessPlatform,
    window: PresentationWindow,
    nonce: ProcessNonce,
) -> UiRuntime {
    UiRuntime::new(
        window,
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        )
        .with_data_transfer(platform.data_transfer())
        .with_process_nonce(nonce),
    )
    .expect("runtime")
}

/// Mount `root` in `presentation` and lay it out, so hit tests find its receiver.
fn mount(runtime: &mut UiRuntime, presentation: PresentationId, root: &ReceiverAnchor) {
    runtime
        .attach_root_widget_with_size_to(presentation, root, 400.0, 300.0)
        .expect("root");
    runtime.synchronize_window_snapshot(presentation, WindowExecutionState::Running, true, true);
}

/// Publish `runtime` and route each window's input to its presentation through the owner.
fn publish(
    owner: &OwnerHost,
    mut runtime: UiRuntime,
    windows: &[(&Arc<dyn HostWindow>, PresentationId)],
) -> Vec<PresentationDispatcher> {
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
        &mut Sink,
    );
    runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    windows
        .iter()
        .map(|(window, presentation)| {
            let dispatcher = owner
                .presentation_dispatcher(PresentationAddress {
                    presentation_id: *presentation,
                    ..address
                })
                .expect("input authority");
            let routed = dispatcher.clone();
            window.on_input(Box::new(move |input: PlatformInput| {
                let _ = routed.input(input, &Effects);
                DispatchEventResult::resolved(false, false)
            }));
            dispatcher
        })
        .collect()
}

struct World {
    owner: OwnerHost,
    dnd: HeadlessDragAndDrop,
    handle: DragHandle,
    runtime: flui_foundation::UiRuntimeId,
    primary: PresentationId,
    first: Arc<dyn HostWindow>,
    second: Arc<dyn HostWindow>,
    dispatchers: Vec<PresentationDispatcher>,
    _platform: HeadlessPlatform,
}

/// One runtime with two presentations over two headless windows, each holding a receiver.
fn world(first: &ReceiverAnchor, second: &ReceiverAnchor, timing: OsDragStart) -> World {
    let platform = HeadlessPlatform::new().with_os_drag(timing);
    let dnd = platform.drag_and_drop();
    let (window_a, presentation_a) = open(&platform);
    let (window_b, presentation_b) = open(&platform);
    let mut runtime = runtime_over(&platform, presentation_a, nonce());
    let primary = runtime.presentation_id();
    let assembled = runtime.assemble_presentation(presentation_b);
    let secondary = runtime.install_presentation(assembled);
    mount(&mut runtime, primary, first);
    mount(&mut runtime, secondary, second);
    let handle = runtime.drag_handle();
    let id = runtime.id();
    let owner = OwnerHost::new();
    let dispatchers = publish(
        &owner,
        runtime,
        &[(&window_a, primary), (&window_b, secondary)],
    );
    World {
        owner,
        dnd,
        handle,
        runtime: id,
        primary,
        first: window_a,
        second: window_b,
        dispatchers,
        _platform: platform,
    }
}

impl World {
    /// Run `body` in an owner callback of the first presentation, as pointer dispatch runs.
    fn in_callback(&self, body: impl FnOnce() + 'static) {
        self.dispatchers[0]
            .test_callback(Box::new(move |runtime| runtime.enter(|_| body())), &Effects)
            .expect("the owner admits the callback");
    }

    /// A drag of `items` from the first window.
    fn start(&self, items: Vec<DragItem>, allowed: AllowedActions) -> Rc<DragSession> {
        let window = as_platform(&self.first);
        Rc::new(
            self.handle
                .start(DragStart::new(self.primary, items, allowed).window(Arc::downgrade(&window)))
                .expect("a drag starts"),
        )
    }

    /// Move `session`'s pointer out of the first window inside an owner callback.
    fn leave_window(&self, session: &Rc<DragSession>) -> MoveOutcome {
        // Read inside the callback, asserted outside it: the owner contains a callback's panic.
        let seen = Rc::new(Cell::new(None));
        let (session, inside) = (Rc::clone(session), Rc::clone(&seen));
        let dnd = self.dnd.clone();
        self.in_callback(move || {
            let outcome = session
                .pointer_moved(Offset::new(100_000.0, 10.0), None)
                .ok();
            inside.set(Some((outcome, dnd.is_active())));
        });
        let (outcome, started_inside) = seen.get().expect("the callback ran");
        assert!(
            !started_inside,
            "the OS session never starts inside the callback that decided on it"
        );
        outcome.expect("live")
    }
}

fn as_platform(window: &Arc<dyn HostWindow>) -> Arc<dyn PlatformWindow> {
    window.clone()
}

fn poll_text(fetch: DragFetch) -> String {
    let mut fetch = std::pin::pin!(fetch);
    let mut context = Context::from_waker(Waker::noop());
    match std::future::Future::poll(fetch.as_mut(), &mut context) {
        Poll::Ready(Ok(TransferPayload::Text(text))) => text,
        other => panic!("the text was not delivered: {other:?}"),
    }
}

fn a_drag_from_another_application_reaches_the_receiver_under_it() {
    let (first_seen, first) = receiver(Some(COPY), true);
    let (_second_seen, second) = receiver(None, false);
    let world = world(&first, &second, OsDragStart::OnLeavingWindow);
    world.dnd.start_foreign(
        vec![(
            TransferFormat::Text,
            TransferPayload::Text("from afar".to_owned()),
        )],
        COPY,
    );
    world.dnd.hover(world.first.id(), Point::new(10.0, 10.0));
    assert_eq!(*first_seen.scopes.borrow(), [DragScope::External]);
    assert_eq!(
        world.dnd.cached_feedback(),
        Some(DropFeedback { accept: Some(COPY) }),
        "the receiver's answer is what the OS is told"
    );
    assert_eq!(world.dnd.drop_here(), Some(COPY));
    assert_eq!(
        *first_seen.events.borrow(),
        ["enter".to_owned(), format!("dropped {}", COPY.bits())]
    );
    let fetch = first_seen
        .fetches
        .borrow_mut()
        .pop()
        .expect("asked for the text");
    assert_eq!(poll_text(fetch), "from afar");
    assert!(!world.dnd.is_active());
}

fn a_drop_stays_open_at_the_os_until_its_request_resolves() {
    let (seen, first) = receiver(Some(COPY), true);
    let (_unused, second) = receiver(None, false);
    let world = world(&first, &second, OsDragStart::OnLeavingWindow);
    world.dnd.start_foreign(
        vec![(
            TransferFormat::Text,
            TransferPayload::Text("later".to_owned()),
        )],
        COPY,
    );
    world.dnd.hover(world.first.id(), Point::new(10.0, 10.0));
    assert_eq!(world.dnd.drop_here(), Some(COPY));
    assert_eq!(
        world.dnd.open_offers(),
        1,
        "the request made on entry is outstanding"
    );
    let fetch = seen.fetches.borrow_mut().pop().expect("asked for the text");
    assert_eq!(poll_text(fetch), "later");
    let background = world
        .owner
        .runtime_dispatcher(world.runtime)
        .expect("background authority");
    background
        .deliver(RuntimeOperation::Background, &Effects)
        .expect("the owner admits the turn");
    assert_eq!(
        world.dnd.open_offers(),
        0,
        "the runtime's next turn tells the OS the target is done"
    );
}

fn a_drag_between_presentations_of_one_runtime_redeems_its_value() {
    let (_first_seen, first) = receiver(None, false);
    let (second_seen, second) = receiver(Some(MOVE), false);
    let world = world(&first, &second, OsDragStart::OnLeavingWindow);
    let session = world.start(
        vec![DragItem::new().with_value(42_i32)],
        AllowedActions::within(MOVE),
    );
    assert_eq!(world.leave_window(&session), MoveOutcome::Escalating);
    assert!(
        world.dnd.is_active(),
        "the session started once the callback was over"
    );
    assert_eq!(
        world.dnd.advertised(),
        Some(MOVE),
        "within_app is advertised"
    );

    world.dnd.hover(world.second.id(), Point::new(10.0, 10.0));
    assert_eq!(*second_seen.scopes.borrow(), [DragScope::Runtime]);
    assert_eq!(
        *second_seen.values.borrow(),
        [Some(42)],
        "the sibling redeems the value"
    );
    assert_eq!(world.dnd.drop_here(), Some(MOVE));
    assert_eq!(
        second_seen.events.borrow().last(),
        Some(&format!("dropped {}", MOVE.bits()))
    );
    assert_eq!(
        session.outcome(),
        Some(DragOutcome::Dropped { action: MOVE }),
        "the OS reports the drop to the source"
    );
}

fn a_drag_between_runtimes_delivers_serialized_representations_only() {
    let platform = HeadlessPlatform::new().with_os_drag(OsDragStart::OnLeavingWindow);
    let dnd = platform.drag_and_drop();
    let (_source_seen, source_root) = receiver(None, false);
    let (target_seen, target_root) = receiver(Some(COPY), true);
    let owner = OwnerHost::new();

    let (window_a, presentation_a) = open(&platform);
    let mut source = runtime_over(&platform, presentation_a, nonce());
    let source_presentation = source.presentation_id();
    mount(&mut source, source_presentation, &source_root);
    let handle = source.drag_handle();
    let source_dispatch = publish(&owner, source, &[(&window_a, source_presentation)]);

    let (window_b, presentation_b) = open(&platform);
    let mut target = runtime_over(&platform, presentation_b, nonce());
    let target_presentation = target.presentation_id();
    mount(&mut target, target_presentation, &target_root);
    let _target_dispatch = publish(&owner, target, &[(&window_b, target_presentation)]);

    let window = as_platform(&window_a);
    let session = Rc::new(
        handle
            .start(
                DragStart::new(
                    source_presentation,
                    vec![DragItem::new().with_value(7_i32).with_text("hello")],
                    AllowedActions::new(MOVE, COPY),
                )
                .window(Arc::downgrade(&window)),
            )
            .expect("a drag starts"),
    );
    let (moved, outcome) = (Rc::clone(&session), Rc::new(Cell::new(None)));
    let inside = Rc::clone(&outcome);
    source_dispatch[0]
        .test_callback(
            Box::new(move |runtime| {
                runtime.enter(|_| {
                    inside.set(Some(
                        moved.pointer_moved(Offset::new(100_000.0, 10.0), None),
                    ));
                });
            }),
            &Effects,
        )
        .expect("the owner admits the callback");
    assert_eq!(outcome.get(), Some(Ok(MoveOutcome::Escalating)));
    assert!(dnd.is_active());

    dnd.hover(window_b.id(), Point::new(10.0, 10.0));
    assert_eq!(*target_seen.scopes.borrow(), [DragScope::OtherRuntime]);
    assert_eq!(
        *target_seen.values.borrow(),
        [None],
        "another runtime redeems no value"
    );
    let fetch = target_seen
        .fetches
        .borrow_mut()
        .pop()
        .expect("asked for the text");
    assert_eq!(
        poll_text(fetch),
        "hello",
        "the serialized representation is what it gets"
    );
    assert_eq!(dnd.drop_here(), Some(COPY));
    assert_eq!(target_seen.values.borrow().last(), Some(&None));
    assert_eq!(
        session.outcome(),
        Some(DragOutcome::Dropped { action: COPY })
    );
}

fn a_window_that_cannot_start_the_session_keeps_the_drag_in_its_window() {
    // No OS drag: the platform is built without `with_os_drag`.
    let platform = HeadlessPlatform::new();
    let (window, presentation) = open(&platform);
    let runtime = runtime_over(&platform, presentation, nonce());
    let session = runtime
        .drag_handle()
        .start(
            DragStart::new(
                runtime.presentation_id(),
                vec![DragItem::new().with_text("x")],
                AllowedActions::new(MOVE, COPY),
            )
            .window(Arc::downgrade(&as_platform(&window))),
        )
        .expect("starts in the window");
    let outcome = runtime.enter(|_| {
        session
            .pointer_moved(Offset::new(100_000.0, 10.0), None)
            .expect("live")
    });
    assert_eq!(outcome, MoveOutcome::Refused(DragError::Unavailable));
    runtime.enter_pending_os_drags();
    assert_eq!(
        session.stage(),
        Ok(flui_interaction::drag::DragStage::InWindow)
    );
}

#[test]
fn drag_and_drop_reaches_the_runtime() {
    crate::table_test::run_table(
        "drag and drop delivery",
        &[
            (
                "a_drag_from_another_application_reaches_the_receiver_under_it",
                a_drag_from_another_application_reaches_the_receiver_under_it,
            ),
            (
                "a_drop_stays_open_at_the_os_until_its_request_resolves",
                a_drop_stays_open_at_the_os_until_its_request_resolves,
            ),
            (
                "a_drag_between_presentations_of_one_runtime_redeems_its_value",
                a_drag_between_presentations_of_one_runtime_redeems_its_value,
            ),
            (
                "a_drag_between_runtimes_delivers_serialized_representations_only",
                a_drag_between_runtimes_delivers_serialized_representations_only,
            ),
            (
                "a_window_that_cannot_start_the_session_keeps_the_drag_in_its_window",
                a_window_that_cannot_start_the_session_keeps_the_drag_in_its_window,
            ),
        ],
    );
}
