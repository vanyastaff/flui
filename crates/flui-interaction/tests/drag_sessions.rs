//! Drag-and-drop sessions (ADR-0211): in-window drags, escalation to the OS, delivery to
//! receivers and the identity that keeps a drag's value in its own runtime.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::{Offset, Point, Size};
use flui_foundation::{DragSessionId, IdMint, PresentationId, RenderId, UiRuntimeId};
use flui_interaction::drag::{
    AllowedActions, Autoscroll, DragError, DragFetch, DragHandle, DragItem, DragObserver,
    DragOutcome, DragOwner, DragPoint, DragReach, DragScope, DragSession, DragStage, DragStart,
    DragView, DropReceiver, DropReceiverSlot, MoveOutcome, ProcessNonce, SpringLoading,
};
use flui_interaction::events::{
    PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::{
    GestureBinding, HitTestEntry, HitTestHandle, HitTestProbe, HitTestResult,
    InteractionDispatchError, InteractionDispatchHandle, InteractionLane, LocalPayloadTarget,
    PenTool, PointerEvent, PointerId, PointerInfo,
};
use flui_platform_api::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationIndex, TransferActions, TransferCompleter, TransferFormat, TransferLimits,
    TransferPayload, TransferRequest,
};
use flui_platform_api::drag::{OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform_api::{DragDropEvent, EventTime, PlatformWindow, WindowId};
use parking_lot::Mutex;

const COPY: TransferActions = TransferActions::COPY;
const MOVE: TransferActions = TransferActions::MOVE;

fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}

type Log = Rc<RefCell<Vec<String>>>;

fn action_name(actions: TransferActions) -> String {
    actions
        .iter_names()
        .map(|(name, _)| name)
        .collect::<Vec<_>>()
        .join("|")
}

fn logged(log: &Log) -> Vec<String> {
    log.borrow().clone()
}

// --- The OS side: a data source and a window ---------------------------------------------------

/// What the OS side saw of the transport.
#[derive(Default)]
struct RecordingSource {
    feedback: Mutex<Vec<(DataTransferId, DropFeedback)>>,
    concluded: Mutex<Vec<DataTransferId>>,
    requests: Mutex<Vec<(DataTransferId, RepresentationIndex, TransferCompleter)>>,
}

impl DataTransferSource for RecordingSource {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        _limits: TransferLimits,
    ) -> TransferRequest {
        let (request, completer) = TransferRequest::channel();
        self.requests.lock().push((id, representation, completer));
        request
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        self.feedback.lock().push((id, feedback));
    }

    fn conclude_drop(&self, id: DataTransferId) {
        self.concluded.lock().push(id);
    }
}

thread_local! {
    static STARTED: RefCell<Vec<OsDragRequest>> = const { RefCell::new(Vec::new()) };
}

struct FakeDragSource(OsDragStart);

impl PlatformDragSource for FakeDragSource {
    fn start_timing(&self) -> OsDragStart {
        self.0
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        STARTED.with(|started| started.borrow_mut().push(request));
        Ok(())
    }
}

struct DragWindow {
    size: Size<f64>,
    source: Option<Arc<dyn PlatformDragSource>>,
}

impl PlatformWindow for DragWindow {
    fn id(&self) -> WindowId {
        WindowId(1)
    }
    fn physical_size(&self) -> Size<i32> {
        Size::new(100, 100)
    }
    fn logical_size(&self) -> Size<f64> {
        self.size
    }
    fn scale_factor(&self) -> f64 {
        1.0
    }
    fn request_redraw(&self) {}
    fn is_focused(&self) -> bool {
        true
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn set_cursor(
        &self,
        _: flui_platform_api::CursorIcon,
    ) -> Result<(), flui_platform_api::CursorError> {
        Ok(())
    }
    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        self.source.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn window(source: Option<OsDragStart>) -> Arc<dyn PlatformWindow> {
    Arc::new(DragWindow {
        size: Size::new(100.0, 100.0),
        source: source
            .map(|timing| Arc::new(FakeDragSource(timing)) as Arc<dyn PlatformDragSource>),
    })
}

fn desktop() -> Arc<dyn PlatformWindow> {
    window(Some(OsDragStart::OnLeavingWindow))
}

// --- Receivers and the hit test ------------------------------------------------------------------

struct Spot {
    name: &'static str,
    log: Log,
    answer: Cell<Option<TransferActions>>,
    on_enter: RefCell<Option<Box<dyn Fn()>>>,
    on_leave: RefCell<Option<Box<dyn Fn()>>>,
    spring: Option<SpringLoading>,
    scopes: RefCell<Vec<DragScope>>,
    values: RefCell<Vec<Option<String>>>,
    /// Ask for the text representation on entry, keeping the request.
    fetch_text: bool,
    fetched: RefCell<Vec<DragFetch>>,
    on_spring: RefCell<Option<Box<dyn Fn()>>>,
    on_autoscroll: RefCell<Option<AutoscrollHook>>,
}

type AutoscrollHook = Box<dyn Fn(Duration) -> Autoscroll>;

impl Spot {
    fn note(&self, what: &str) {
        self.log.borrow_mut().push(format!("{}:{what}", self.name));
    }

    fn feedback(&self) -> DropFeedback {
        DropFeedback {
            accept: self.answer.get(),
        }
    }
}

impl DropReceiver for Spot {
    fn drag_entered(&self, drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        self.note("enter");
        self.scopes.borrow_mut().push(drag.scope());
        self.values
            .borrow_mut()
            .push(drag.value::<String>().cloned());
        if self.fetch_text
            && let Some(fetch) = drag.request(&TransferFormat::Text, TransferLimits::default())
        {
            self.fetched.borrow_mut().push(fetch);
        }
        if let Some(hook) = &*self.on_enter.borrow() {
            hook();
        }
        self.feedback()
    }

    fn drag_moved(&self, _drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        self.note("move");
        self.feedback()
    }

    fn drag_left(&self, _drag: &DragView<'_>) {
        self.note("left");
        if let Some(hook) = &*self.on_leave.borrow() {
            hook();
        }
    }

    fn dropped(&self, drag: &DragView<'_>, _at: DragPoint, action: TransferActions) {
        self.note(&format!("dropped {}", action_name(action)));
        self.values
            .borrow_mut()
            .push(drag.value::<String>().cloned());
    }

    fn spring_loading(&self) -> Option<SpringLoading> {
        self.spring
    }

    fn spring_loaded(&self, _drag: &DragView<'_>) {
        self.note("spring");
        if let Some(hook) = &*self.on_spring.borrow() {
            hook();
        }
    }

    fn autoscroll(&self, _drag: &DragView<'_>, _at: DragPoint, elapsed: Duration) -> Autoscroll {
        let hook = self.on_autoscroll.borrow();
        let Some(hook) = hook.as_ref() else {
            return Autoscroll::Idle;
        };
        self.note(&format!("autoscroll {}ms", elapsed.as_millis()));
        hook(elapsed)
    }
}

/// Reports the receivers whose x range holds the position, in registration order.
struct Probe {
    spots: RefCell<Vec<(f64, f64, LocalPayloadTarget)>>,
}

impl HitTestProbe for Probe {
    fn probe(
        &self,
        position: Offset<f64>,
        _kind: PointerKind,
        result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        for (low, high, target) in &*self.spots.borrow() {
            if (*low..*high).contains(&position.dx) {
                result.add(HitTestEntry::new(RenderId::new(1)).metadata(Arc::new(*target)));
            }
        }
        Ok(())
    }
}

struct Observer(Log);

impl DragObserver for Observer {
    fn moved(&self, _at: DragPoint) {
        self.0.borrow_mut().push("observer:moved".to_owned());
    }
    fn escalation_refused(&self, error: DragError) {
        self.0
            .borrow_mut()
            .push(format!("observer:refused {error}"));
    }
    fn ended(&self, outcome: DragOutcome) {
        self.0
            .borrow_mut()
            .push(format!("observer:ended {outcome:?}"));
    }
}

struct Fixture {
    dispatch: InteractionDispatchHandle,
    owner: DragOwner,
    handle: DragHandle,
    log: Log,
    source: Arc<RecordingSource>,
    probe: Rc<Probe>,
    hit: HitTestHandle,
    offers: RefCell<OfferTable>,
    presentation: PresentationId,
    windows: RefCell<Vec<Arc<dyn PlatformWindow>>>,
}

fn presentation(n: usize) -> PresentationId {
    PresentationId::new(n)
}

fn nonce() -> ProcessNonce {
    ProcessNonce::from_raw(NonZeroU64::new(0xa11ce).expect("nonzero"))
}

impl Fixture {
    fn new(lane: &InteractionLane) -> Self {
        Self::in_runtime(lane, 1, nonce())
    }

    fn in_runtime(lane: &InteractionLane, runtime: usize, nonce: ProcessNonce) -> Self {
        let dispatch = lane.dispatch_handle();
        let source = Arc::new(RecordingSource::default());
        let owner = DragOwner::new(nonce, UiRuntimeId::new(runtime), source.clone());
        let probe = Rc::new(Probe {
            spots: RefCell::new(Vec::new()),
        });
        let hit = HitTestHandle::new(dispatch.clone(), probe.clone());
        let handle = owner.handle();
        owner.attach_presentation(presentation(1), WindowId(1), hit.clone());
        Self {
            dispatch,
            owner,
            handle,
            log: Rc::default(),
            source,
            probe,
            hit,
            offers: RefCell::new(OfferTable::new()),
            presentation: presentation(1),
            windows: RefCell::default(),
        }
    }

    /// A receiver over x in `low..high` answering `answer` to every hover.
    fn spot(
        &self,
        name: &'static str,
        low: f64,
        high: f64,
        answer: Option<TransferActions>,
    ) -> Rc<Spot> {
        self.spring_spot(name, low, high, answer, None)
    }

    /// [`Self::spot`] with a spring-loading policy.
    fn spring_spot(
        &self,
        name: &'static str,
        low: f64,
        high: f64,
        answer: Option<TransferActions>,
        spring: Option<SpringLoading>,
    ) -> Rc<Spot> {
        let spot = self.unregistered(name, answer, spring);
        self.register(&spot, low, high);
        spot
    }

    /// A receiver no node carries yet.
    fn unregistered(
        &self,
        name: &'static str,
        answer: Option<TransferActions>,
        spring: Option<SpringLoading>,
    ) -> Rc<Spot> {
        Rc::new(Spot {
            name,
            log: Rc::clone(&self.log),
            answer: Cell::new(answer),
            on_enter: RefCell::new(None),
            on_leave: RefCell::new(None),
            spring,
            scopes: RefCell::default(),
            values: RefCell::default(),
            fetch_text: false,
            fetched: RefCell::default(),
            on_spring: RefCell::new(None),
            on_autoscroll: RefCell::new(None),
        })
    }

    fn register(&self, spot: &Rc<Spot>, low: f64, high: f64) -> LocalPayloadTarget {
        let target = self
            .dispatch
            .register_local_payload(Rc::new(DropReceiverSlot(spot.clone())))
            .expect("active lane");
        self.probe.spots.borrow_mut().push((low, high, target));
        target
    }

    /// An OS offer of plain text, as another application would make it.
    fn text_offer(&self) -> DataTransferOffer {
        let mut table = self.offers.borrow_mut();
        let representations: Arc<[_]> = Arc::from(vec![
            flui_platform_api::data_transfer::RepresentationDescriptor {
                format: TransferFormat::Text,
                declared_len: None,
            },
        ]);
        let id = table.mint(OfferRecord::new(Arc::clone(&representations)));
        DataTransferOffer::new(id, representations)
    }

    fn start(&self, window: Arc<dyn PlatformWindow>, allowed: AllowedActions) -> DragSession {
        self.start_with(
            window,
            allowed,
            None,
            vec![DragItem::new().with_value("payload".to_owned())],
        )
    }

    fn start_with(
        &self,
        window: Arc<dyn PlatformWindow>,
        allowed: AllowedActions,
        reach: Option<DragReach>,
        items: Vec<DragItem>,
    ) -> DragSession {
        let mut start = DragStart::new(self.presentation, items, allowed)
            .pointer(PointerInfo::new(
                PointerId::try_from(1_u64).expect("pointer"),
                PointerKind::Touch,
            ))
            .window(Arc::downgrade(&window))
            .hit_test(self.hit.clone())
            .observer(Rc::new(Observer(Rc::clone(&self.log))));
        if let Some(reach) = reach {
            start = start.reach(reach);
        }
        // The session holds its window weakly; the fixture keeps it open.
        self.windows.borrow_mut().push(window);
        self.handle.start(start).expect("a valid drag starts")
    }

    fn offer_for(&self, request: &OsDragRequest) -> DataTransferOffer {
        let mut table = self.offers.borrow_mut();
        let id = table.mint(OfferRecord::new(Arc::from(request.representations.clone())));
        DataTransferOffer::new(
            id,
            Arc::clone(table.get(id).expect("live").representations()),
        )
    }

    fn deliver(&self, presentation: PresentationId, event: DragDropEvent) {
        let probe = Rc::clone(&self.probe);
        let mut hit = move |position: Offset<f64>| {
            let mut result = HitTestResult::new();
            probe
                .probe(position, PointerKind::Mouse, &mut result)
                .ok()
                .map(|()| result.path().to_vec())
        };
        self.owner.deliver(presentation, event, &mut hit);
    }
}

fn in_lane(body: impl FnOnce(&Fixture)) {
    let lane = InteractionLane::try_new().expect("owner lane");
    let fixture = Fixture::new(&lane);
    lane.enter(|| body(&fixture));
}

fn at(x: f64) -> Offset<f64> {
    Offset::new(x, 10.0)
}

fn moved(session: &DragSession, x: f64) -> MoveOutcome {
    session.pointer_moved(at(x), None).expect("a live session")
}

fn take_started() -> Vec<OsDragRequest> {
    STARTED.with(|started| std::mem::take(&mut *started.borrow_mut()))
}

// --- drop_reaches_only_last_accepting_target -----------------------------------------------------

fn drop_reaches_only_last_accepting_target() {
    in_lane(|f| {
        let a = f.spot("A", 0.0, 50.0, Some(COPY));
        let _b = f.spot("B", 50.0, 100.0, Some(COPY));
        let w = desktop();
        let session = f.start(Arc::clone(&w), AllowedActions::within(COPY));
        assert_eq!(moved(&session, 10.0), MoveOutcome::Tracking(Some(COPY)));
        assert_eq!(moved(&session, 80.0), MoveOutcome::Tracking(Some(COPY)));
        let outcome = session.pointer_released(at(80.0), None).expect("live");
        assert_eq!(outcome, Some(DragOutcome::Dropped { action: COPY }));
        assert_eq!(
            logged(&f.log),
            [
                "A:enter",
                "observer:moved",
                "A:left",
                "B:enter",
                "observer:moved",
                "B:move",
                "observer:moved",
                "B:dropped COPY",
                "observer:ended Dropped { action: TransferActions(COPY) }",
            ],
            "A left when the pointer moved on and never saw the drop"
        );
        drop(a);
    });
}

fn a_receiver_that_accepted_and_then_declined_gets_no_drop() {
    in_lane(|f| {
        let w = desktop();
        let c = f.spot("C", 0.0, 50.0, Some(COPY));
        let session = f.start(Arc::clone(&w), AllowedActions::within(COPY));
        moved(&session, 10.0);
        c.answer.set(None);
        assert_eq!(moved(&session, 20.0), MoveOutcome::Tracking(None));
        assert_eq!(
            session.pointer_released(at(20.0), None).expect("live"),
            Some(DragOutcome::Cancelled)
        );
        let log = logged(&f.log);
        assert!(
            !log.iter().any(|entry| entry.starts_with("C:dropped")),
            "{log:?}"
        );
    });
}

// --- declined_target_under_pointer_receives_leave -------------------------------------------------

fn declined_target_under_pointer_receives_leave() {
    in_lane(|f| {
        let declining = f.spot("D", 0.0, 100.0, None);
        let accepting = f.spot("E", 0.0, 100.0, Some(MOVE));
        let w = desktop();
        let session = f.start(Arc::clone(&w), AllowedActions::within(MOVE));
        moved(&session, 10.0);
        session.pointer_released(at(10.0), None).expect("live");
        let log = logged(&f.log);
        let count = |entry: &str| log.iter().filter(|line| *line == entry).count();
        assert_eq!(
            count("D:left"),
            1,
            "the declined receiver left once: {log:?}"
        );
        assert_eq!(count("E:left"), 0);
        assert_eq!(count("E:dropped MOVE"), 1);
        drop((declining, accepting));

        // Nothing accepts: every entered receiver leaves exactly once, none is dropped on.
        f.log.borrow_mut().clear();
        let g = f.spot("G", 0.0, 100.0, None);
        let session = f.start(Arc::clone(&w), AllowedActions::within(MOVE));
        moved(&session, 10.0);
        moved(&session, 11.0);
        session.cancel().expect("live");
        let log = logged(&f.log);
        assert_eq!(
            log.iter().filter(|line| *line == "G:left").count(),
            1,
            "{log:?}"
        );
        assert!(!log.iter().any(|line| line.contains("G:dropped")));
        drop(g);
    });
}

// --- concurrent_sessions_are_independent -----------------------------------------------------------

fn concurrent_sessions_are_independent() {
    in_lane(|f| {
        let _a = f.spot("A", 0.0, 50.0, Some(COPY));
        let _b = f.spot("B", 50.0, 100.0, Some(COPY));
        let w = desktop();
        let first = f.start(Arc::clone(&w), AllowedActions::within(COPY));
        let second = f.start(Arc::clone(&w), AllowedActions::within(COPY));
        assert_ne!(first.id(), second.id());
        moved(&first, 10.0);
        moved(&second, 80.0);
        first.cancel().expect("live");
        assert_eq!(
            second.stage(),
            Ok(DragStage::InWindow),
            "cancelling one leaves the other"
        );
        assert_eq!(first.stage(), Ok(DragStage::Concluded));
        assert_eq!(
            second.pointer_released(at(80.0), None).expect("live"),
            Some(DragOutcome::Dropped { action: COPY })
        );
        let log = logged(&f.log);
        assert!(log.contains(&"A:left".to_owned()), "{log:?}");
        assert!(log.contains(&"B:dropped COPY".to_owned()), "{log:?}");
        assert!(!log.contains(&"A:dropped COPY".to_owned()));
        assert_eq!(first.outcome(), Some(DragOutcome::Cancelled));
    });
}

// --- session_id_exhaustion_refuses_permanently ----------------------------------------------------

fn session_id_exhaustion_refuses_permanently() {
    let lane = InteractionLane::try_new().expect("lane");
    let f = Fixture::new(&lane);
    let owner = DragOwner::new(nonce(), UiRuntimeId::new(1), f.source.clone())
        .with_session_mint(IdMint::<DragSessionId>::starting_after(u64::MAX - 1));
    let handle = owner.handle();
    let begin = || {
        handle.start(DragStart::new(
            f.presentation,
            vec![DragItem::new().with_value(1_u8)],
            AllowedActions::within(COPY),
        ))
    };
    let last = begin().expect("one id remains");
    assert_eq!(last.id().get(), u64::MAX);
    last.cancel().expect("live");
    drop(last);
    for _ in 0..3 {
        assert!(
            matches!(begin(), Err(DragError::IdExhausted)),
            "the mint wrapped or reissued"
        );
    }
}

// --- refusals --------------------------------------------------------------------------------------

fn refusals_at_start() {
    in_lane(|f| {
        let start = |items: Vec<DragItem>, allowed, reach: Option<DragReach>| {
            let mut start = DragStart::new(f.presentation, items, allowed);
            if let Some(reach) = reach {
                start = start.reach(reach);
            }
            f.handle.start(start).err()
        };
        let valued = || vec![DragItem::new().with_value(1_u8)];
        assert_eq!(
            start(valued(), AllowedActions::default(), None),
            Some(DragError::NoAllowedActions),
            "a_drag_with_no_allowed_actions_is_refused_at_start"
        );
        assert_eq!(
            start(valued(), AllowedActions::new(MOVE, COPY), None),
            Some(DragError::NoSerializedRepresentation),
            "outside_actions_without_a_serialized_representation_are_refused"
        );
        assert_eq!(
            start(
                valued(),
                AllowedActions::within(MOVE),
                Some(DragReach::System)
            ),
            Some(DragError::NoOutsideActions)
        );
        assert_eq!(
            start(
                vec![DragItem::new().with_text("hi")],
                AllowedActions::new(MOVE, COPY),
                None
            ),
            None,
            "outside actions with a serialized representation start"
        );
        let session = f
            .handle
            .start(DragStart::new(
                f.presentation,
                valued(),
                AllowedActions::within(MOVE),
            ))
            .expect("starts");
        session.cancel().expect("live");
        assert_eq!(session.cancel(), Err(DragError::SessionClosed));
        assert_eq!(
            session.pointer_moved(at(1.0), None),
            Err(DragError::SessionClosed)
        );
    });
}

// --- escalation ------------------------------------------------------------------------------------

fn escalates_only_outside_window_with_outside_actions_or_sibling_presentation() {
    struct Case {
        name: &'static str,
        window: fn() -> Arc<dyn PlatformWindow>,
        allowed: AllowedActions,
        reach: Option<DragReach>,
        sibling: bool,
        outside: bool,
        expect: Expect,
    }
    enum Expect {
        InWindow,
        Full,
        IdentityOnly,
        Refused,
    }
    let text = || vec![DragItem::new().with_text("hi").with_value(1_u8)];
    let cases = [
        Case {
            name: "inside the window nothing escalates",
            window: desktop,
            allowed: AllowedActions::new(MOVE, COPY),
            reach: None,
            sibling: true,
            outside: false,
            expect: Expect::InWindow,
        },
        Case {
            name: "outside with outside actions offers everything",
            window: desktop,
            allowed: AllowedActions::new(MOVE, COPY),
            reach: None,
            sibling: false,
            outside: true,
            expect: Expect::Full,
        },
        Case {
            name: "outside with no outside actions and no sibling is a move with no target",
            window: desktop,
            allowed: AllowedActions::within(MOVE),
            reach: None,
            sibling: false,
            outside: true,
            expect: Expect::InWindow,
        },
        Case {
            name: "a sibling presentation makes an identity-only session",
            window: desktop,
            allowed: AllowedActions::within(MOVE),
            reach: None,
            sibling: true,
            outside: true,
            expect: Expect::IdentityOnly,
        },
        Case {
            name: "window reach never leaves the window",
            window: desktop,
            allowed: AllowedActions::within(MOVE),
            reach: Some(DragReach::Window),
            sibling: true,
            outside: true,
            expect: Expect::InWindow,
        },
        Case {
            name: "a backend without a drag source refuses",
            window: || window(None),
            allowed: AllowedActions::new(MOVE, COPY),
            reach: None,
            sibling: false,
            outside: true,
            expect: Expect::Refused,
        },
    ];
    for case in cases {
        let lane = InteractionLane::try_new().expect("lane");
        let f = Fixture::new(&lane);
        lane.enter(|| {
            if case.sibling {
                f.owner
                    .attach_presentation(presentation(2), WindowId(2), f.hit.clone());
            }
            let w = (case.window)();
            let session = f.start_with(Arc::clone(&w), case.allowed, case.reach, text());
            let x = if case.outside { 150.0 } else { 50.0 };
            let outcome = moved(&session, x);
            let pending = f.owner.take_pending_os_drags();
            let name = case.name;
            match case.expect {
                Expect::InWindow => {
                    assert!(pending.is_empty(), "{name}");
                    assert_eq!(outcome, MoveOutcome::Tracking(None), "{name}");
                    assert_eq!(session.stage(), Ok(DragStage::InWindow), "{name}");
                }
                Expect::Refused => {
                    assert!(pending.is_empty(), "{name}");
                    assert_eq!(
                        outcome,
                        MoveOutcome::Refused(DragError::Unavailable),
                        "{name}"
                    );
                    assert_eq!(session.stage(), Ok(DragStage::InWindow), "{name}");
                    assert_eq!(
                        logged(&f.log)
                            .iter()
                            .filter(|l| l.starts_with("observer:refused"))
                            .count(),
                        1,
                        "{name}"
                    );
                    moved(&session, 160.0);
                    assert_eq!(
                        logged(&f.log)
                            .iter()
                            .filter(|l| l.starts_with("observer:refused"))
                            .count(),
                        1,
                        "{name}: told once"
                    );
                }
                Expect::Full => {
                    assert_eq!(outcome, MoveOutcome::Escalating, "{name}");
                    assert_eq!(pending.len(), 1, "{name}");
                    let request = &pending[0].request;
                    assert_eq!(request.allowed, COPY | MOVE, "{name}");
                    assert_eq!(
                        request.representations.len(),
                        2,
                        "text and identity: {name}"
                    );
                    assert_eq!(session.stage(), Ok(DragStage::System), "{name}");
                    // A refusal names the session by its full identity: the same session
                    // number in another runtime is not this drag.
                    let key = session.key().expect("live");
                    let foreign = flui_platform_api::drag::DragSessionKey::new(
                        key.nonce(),
                        UiRuntimeId::new(9),
                        key.session(),
                    );
                    f.owner.os_drag_refused(foreign, DragError::Unavailable);
                    assert_eq!(
                        session.stage(),
                        Ok(DragStage::System),
                        "{name}: foreign refusal"
                    );
                    f.owner.os_drag_refused(key, DragError::Unavailable);
                    assert_eq!(session.stage(), Ok(DragStage::InWindow), "{name}: refused");
                }
                Expect::IdentityOnly => {
                    assert_eq!(outcome, MoveOutcome::Escalating, "{name}");
                    assert_eq!(pending.len(), 1, "{name}");
                    let request = &pending[0].request;
                    assert_eq!(request.allowed, MOVE, "within_app is advertised: {name}");
                    assert_eq!(request.representations.len(), 1, "identity only: {name}");
                    assert_eq!(session.stage(), Ok(DragStage::System), "{name}");
                }
            }
        });
    }
}

fn an_at_start_backend_decides_at_the_start_and_never_escalates_later() {
    let lane = InteractionLane::try_new().expect("lane");
    let f = Fixture::new(&lane);
    lane.enter(|| {
        f.owner
            .attach_presentation(presentation(2), WindowId(2), f.hit.clone());
        let ios = window(Some(OsDragStart::AtStart { eager_data: false }));
        let items = || vec![DragItem::new().with_value(1_u8)];
        let session = f.start_with(
            Arc::clone(&ios),
            AllowedActions::within(MOVE),
            None,
            items(),
        );
        assert_eq!(
            session.stage(),
            Ok(DragStage::EscalationPending),
            "decided at the start"
        );
        assert_eq!(f.owner.take_pending_os_drags().len(), 1);

        // Window reach is not an OS session even at the start.
        let reorder = f.start_with(
            Arc::clone(&ios),
            AllowedActions::within(MOVE),
            Some(DragReach::Window),
            items(),
        );
        assert_eq!(reorder.stage(), Ok(DragStage::Armed));
        assert_eq!(
            moved(&reorder, 150.0),
            MoveOutcome::Tracking(None),
            "never mid-drag"
        );
        assert!(f.owner.take_pending_os_drags().is_empty());

        // No sibling: nothing at the start, nothing later.
        let lone = Fixture::new(&InteractionLane::try_new().expect("lane"));
        let session = lone.start_with(
            Arc::clone(&ios),
            AllowedActions::within(MOVE),
            None,
            items(),
        );
        assert_eq!(session.stage(), Ok(DragStage::Armed));
        // An unavailable backend refuses a start that needs the OS.
        let none = window(None);
        let refused = f.handle.start(
            DragStart::new(
                f.presentation,
                vec![DragItem::new().with_text("x")],
                AllowedActions::new(MOVE, COPY),
            )
            .window(Arc::downgrade(&none)),
        );
        assert!(
            refused.is_ok(),
            "an at-leave backend without a source starts in-window"
        );
        drop(none);
    });
}

// --- the OS delivers the drag to a sibling presentation of the same runtime -----------------------

fn drag_between_presentations_of_one_runtime_redeems_the_value() {
    let lane = InteractionLane::try_new().expect("lane");
    let f = Fixture::new(&lane);
    lane.enter(|| {
        let sibling = presentation(2);
        f.owner
            .attach_presentation(sibling, WindowId(2), f.hit.clone());
        let target = f.spot("S", 0.0, 100.0, Some(MOVE));
        let w = desktop();
        let session = f.start(Arc::clone(&w), AllowedActions::within(MOVE));
        assert_eq!(moved(&session, 150.0), MoveOutcome::Escalating);
        let mut pending = f.owner.take_pending_os_drags();
        let request = pending.remove(0).request;
        assert_eq!(
            request.allowed, MOVE,
            "the identity-only session advertises within_app"
        );
        let offer = f.offer_for(&request);
        let id = offer.id();

        f.deliver(
            sibling,
            DragDropEvent::Entered {
                offer,
                allowed: MOVE,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        assert_eq!(
            *target.values.borrow(),
            [Some("payload".to_owned())],
            "the sibling redeems the value"
        );
        assert_eq!(*target.scopes.borrow(), [DragScope::Runtime]);
        assert_eq!(
            *f.source.feedback.lock(),
            [(id, DropFeedback { accept: Some(MOVE) })],
            "the answer reaches the OS as drop feedback"
        );

        f.deliver(
            sibling,
            DragDropEvent::Dropped {
                id,
                action: MOVE,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        assert_eq!(
            target.values.borrow().last(),
            Some(&Some("payload".to_owned()))
        );
        assert!(logged(&f.log).contains(&"S:dropped MOVE".to_owned()));
        assert_eq!(
            *f.source.concluded.lock(),
            [id],
            "the OS is told the target is done"
        );

        (request.on_end)(OsDragEnd::Dropped(MOVE));
        assert_eq!(
            session.outcome(),
            Some(DragOutcome::Dropped { action: MOVE })
        );
        assert!(
            logged(&f.log)
                .contains(&"observer:ended Dropped { action: TransferActions(MOVE) }".to_owned())
        );
    });
}

// --- a drop stays open at the OS while a request made from it is outstanding --------------------

fn a_drop_is_concluded_once_its_requests_resolve() {
    in_lane(|f| {
        let target = Rc::new(Spot {
            name: "T",
            log: Rc::clone(&f.log),
            answer: Cell::new(Some(COPY)),
            on_enter: RefCell::new(None),
            on_leave: RefCell::new(None),
            spring: None,
            scopes: RefCell::default(),
            values: RefCell::default(),
            fetch_text: true,
            fetched: RefCell::default(),
            on_spring: RefCell::new(None),
            on_autoscroll: RefCell::new(None),
        });
        f.register(&target, 0.0, 100.0);
        let offer = {
            let mut table = f.offers.borrow_mut();
            let representations: Arc<[_]> = Arc::from(vec![
                flui_platform_api::data_transfer::RepresentationDescriptor {
                    format: TransferFormat::Text,
                    declared_len: None,
                },
            ]);
            let id = table.mint(OfferRecord::new(Arc::clone(&representations)));
            DataTransferOffer::new(id, representations)
        };
        let id = offer.id();
        let here = Some(Point::new(5.0, 5.0));
        f.deliver(
            f.presentation,
            DragDropEvent::Entered {
                offer,
                allowed: COPY,
                position: here,
            },
        );
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id,
                action: COPY,
                position: here,
            },
        );
        assert!(logged(&f.log).contains(&"T:dropped COPY".to_owned()));
        assert!(
            f.source.concluded.lock().is_empty(),
            "the fetch made on entry is outstanding"
        );
        assert_eq!(
            f.owner.next_deadline(),
            None,
            "nothing to do until it resolves"
        );
        f.owner.tick(EventTime::from_nanos(1));
        assert!(f.source.concluded.lock().is_empty());

        drop(
            target
                .fetched
                .borrow_mut()
                .pop()
                .expect("a request was made"),
        );
        assert_eq!(
            f.owner.next_deadline(),
            Some(EventTime::from_nanos(0)),
            "the resolved drop asks for a turn at once"
        );
        f.owner.tick(EventTime::from_nanos(2));
        assert_eq!(
            *f.source.concluded.lock(),
            [id],
            "the OS hears the target is done"
        );
        assert_eq!(f.owner.next_deadline(), None);
    });
}

// --- the value stays in the source runtime ---------------------------------------------------------

fn value_is_redeemed_only_in_source_runtime() {
    let lane_a = InteractionLane::try_new().expect("lane");
    let lane_b = InteractionLane::try_new().expect("lane");
    let a = Fixture::in_runtime(&lane_a, 1, nonce());
    let b = Fixture::in_runtime(&lane_b, 2, nonce());
    let sibling = presentation(7);
    b.owner
        .attach_presentation(sibling, WindowId(7), b.hit.clone());

    let (request, session) = lane_a.enter(|| {
        let w = desktop();
        let session = a.start_with(
            Arc::clone(&w),
            AllowedActions::new(MOVE, COPY),
            None,
            vec![
                DragItem::new()
                    .with_value("secret".to_owned())
                    .with_text("hello"),
            ],
        );
        assert_eq!(moved(&session, 150.0), MoveOutcome::Escalating);
        let request = a.owner.take_pending_os_drags().remove(0).request;
        drop(w);
        (request, session)
    });
    assert_eq!(request.representations.len(), 2);

    lane_b.enter(|| {
        let target = Rc::new(Spot {
            name: "B",
            log: Rc::clone(&b.log),
            answer: Cell::new(Some(COPY)),
            on_enter: RefCell::new(None),
            on_leave: RefCell::new(None),
            spring: None,
            scopes: RefCell::default(),
            values: RefCell::default(),
            fetch_text: true,
            fetched: RefCell::default(),
            on_spring: RefCell::new(None),
            on_autoscroll: RefCell::new(None),
        });
        b.register(&target, 0.0, 100.0);
        let offer = a.offer_for(&request);
        let id = offer.id();
        b.deliver(
            sibling,
            DragDropEvent::Entered {
                offer,
                allowed: COPY | MOVE,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        assert_eq!(
            *target.values.borrow(),
            [None],
            "another runtime redeems no value"
        );
        assert_eq!(*target.scopes.borrow(), [DragScope::OtherRuntime]);

        // Its serialized representations are all it has: the text is asked of the OS,
        // which asks the source runtime's provider.
        let (asked, index, completer) = b
            .source
            .requests
            .lock()
            .pop()
            .expect("the text was requested");
        assert_eq!((asked, index), (id, RepresentationIndex(0)));
        let mut provider = request.provider;
        completer.complete(provider(index));
        let fetch = target
            .fetched
            .borrow_mut()
            .pop()
            .expect("a request was made");
        let mut fetch = std::pin::pin!(fetch);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let std::task::Poll::Ready(Ok(TransferPayload::Text(text))) =
            fetch.as_mut().poll(&mut context)
        else {
            panic!("the text was delivered");
        };
        assert_eq!(text, "hello");
        let identity = provider(RepresentationIndex(1)).expect("identity");
        assert!(matches!(identity, TransferPayload::Custom { .. }));
    });
    assert!(a.owner.is_live(session.key().expect("key")));
    assert!(
        !b.owner.is_live(session.key().expect("key")),
        "the other runtime's table does not hold the session"
    );
}

// --- reentrancy and containment ----------------------------------------------------------------------

fn a_receiver_that_cancels_the_session_while_entered_still_leaves_once() {
    in_lane(|f| {
        let spot = f.spot("R", 0.0, 100.0, Some(COPY));
        let w = desktop();
        let session = Rc::new(f.start(Arc::clone(&w), AllowedActions::within(COPY)));
        let again = Rc::clone(&session);
        *spot.on_enter.borrow_mut() = Some(Box::new(move || {
            again.cancel().expect("live while it runs");
        }));
        moved(&session, 10.0);
        let log = logged(&f.log);
        assert_eq!(log.iter().filter(|l| *l == "R:enter").count(), 1, "{log:?}");
        assert_eq!(log.iter().filter(|l| *l == "R:left").count(), 1, "{log:?}");
        assert_eq!(session.stage(), Ok(DragStage::Concluded));
        let enter = log.iter().position(|l| l == "R:enter").expect("entered");
        let left = log.iter().position(|l| l == "R:left").expect("left");
        assert!(enter < left, "leave follows enter: {log:?}");
    });
}

fn a_panicking_receiver_does_not_strand_the_others() {
    in_lane(|f| {
        let first = f.spot("P", 0.0, 100.0, Some(COPY));
        let _second = f.spot("Q", 0.0, 100.0, Some(COPY));
        let w = desktop();
        let session = f.start(Arc::clone(&w), AllowedActions::within(COPY));
        moved(&session, 10.0);
        *first.on_leave.borrow_mut() = Some(Box::new(|| panic!("receiver leave failure")));
        let result = catch_unwind(AssertUnwindSafe(|| session.cancel()));
        assert!(result.is_err(), "the first failure reaches the caller");
        let log = logged(&f.log);
        assert!(
            log.contains(&"Q:left".to_owned()),
            "the other receiver still left: {log:?}"
        );
        assert_eq!(session.stage(), Ok(DragStage::Concluded));
    });
}

// --- spring-loading hook -------------------------------------------------------------------------------

fn spring_loading_hook_fires_once_per_hover() {
    in_lane(|f| {
        let spot = Rc::new(Spot {
            name: "F",
            log: Rc::clone(&f.log),
            answer: Cell::new(Some(COPY)),
            on_enter: RefCell::new(None),
            on_leave: RefCell::new(None),
            spring: Some(SpringLoading {
                delay: Duration::from_millis(100),
            }),
            scopes: RefCell::default(),
            values: RefCell::default(),
            fetch_text: false,
            fetched: RefCell::default(),
            on_spring: RefCell::new(None),
            on_autoscroll: RefCell::new(None),
        });
        f.register(&spot, 0.0, 50.0);
        let w = desktop();
        let session = f.start(Arc::clone(&w), AllowedActions::within(COPY));
        let t = |ms: u64| EventTime::from_nanos(1_000_000_000 + ms * 1_000_000);
        session.pointer_moved(at(10.0), Some(t(0))).expect("live");
        assert_eq!(f.owner.next_deadline(), Some(t(100)));
        f.owner.tick(t(50));
        f.owner.tick(t(100));
        f.owner.tick(t(500));
        let springs = |log: &Log| log.borrow().iter().filter(|l| *l == "F:spring").count();
        assert_eq!(springs(&f.log), 1, "once per hover, however long it lasts");
        assert_eq!(f.owner.next_deadline(), None);
        // Leaving and returning is a new hover.
        session.pointer_moved(at(80.0), Some(t(600))).expect("live");
        session.pointer_moved(at(10.0), Some(t(700))).expect("live");
        f.owner.tick(t(750));
        assert_eq!(springs(&f.log), 1);
        f.owner.tick(t(800));
        assert_eq!(springs(&f.log), 2);
    });
}

// --- in-window drags keep every other contact --------------------------------------------------------

fn in_window_drag_keeps_second_finger_and_pen() {
    let lane = InteractionLane::try_new().expect("lane");
    let f = Fixture::new(&lane);
    let binding = GestureBinding::new();
    lane.enter(|| {
        let w = desktop();
        let session: Rc<RefCell<Option<DragSession>>> = Rc::default();
        let captured = Rc::new(RefCell::new(Vec::new()));
        let seen: Rc<RefCell<Vec<String>>> = Rc::default();

        let (starter, held, ses) = (f.handle.clone(), Rc::clone(&captured), Rc::clone(&session));
        let (hit, window, presentation) = (f.hit.clone(), Arc::clone(&w), f.presentation);
        let source = f
            .dispatch
            .register_pointer(move |dispatch| match dispatch.global {
                PointerEvent::Down(press) => {
                    let capture = dispatch.capture().expect("a real Down");
                    let began = starter.start(
                        DragStart::new(
                            presentation,
                            vec![DragItem::new().with_value(0_u8)],
                            AllowedActions::within(MOVE),
                        )
                        .pointer(press.pointer)
                        .window(Arc::downgrade(&window))
                        .hit_test(hit.clone())
                        .capture(capture),
                    );
                    *ses.borrow_mut() = Some(began.expect("starts"));
                    held.borrow_mut().push("down");
                }
                PointerEvent::Move(motion) => {
                    let position = motion.current().position.get();
                    ses.borrow()
                        .as_ref()
                        .expect("started")
                        .pointer_moved(Offset::new(position.x, position.y), None)
                        .expect("live");
                    held.borrow_mut().push("move");
                }
                _ => {}
            })
            .expect("source handler");
        let log = Rc::clone(&seen);
        let bystander = f
            .dispatch
            .register_pointer(move |dispatch| {
                log.borrow_mut().push(format!(
                    "{:?}",
                    flui_interaction::pointer_event_kind(dispatch.local)
                ));
            })
            .expect("bystander handler");
        let path_of = |target| {
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            path
        };
        let finger = PointerId::try_from(1_u64).expect("pointer");
        let second = PointerId::try_from(2_u64).expect("pointer");
        let pen = PointerId::try_from(3_u64).expect("pointer");

        let down = make_down_event_for_id(finger, at(10.0), PointerKind::Touch).expect("down");
        binding.handle_pointer_event(&down, |_| path_of(source));
        for x in [20.0, 30.0] {
            let motion = make_move_event_for_id(finger, at(x), PointerKind::Touch).expect("move");
            binding.handle_pointer_event(&motion, |_| path_of(source));
            binding.flush_pending_input();
        }
        let started = take_started();
        assert!(started.is_empty(), "an in-window drag starts no OS session");
        let drag_stage = session.borrow().as_ref().expect("session").stage();
        assert_eq!(drag_stage, Ok(DragStage::InWindow));

        // A second finger and a pen are ordinary contacts while the first drags.
        let touch = make_down_event_for_id(second, at(70.0), PointerKind::Touch).expect("down");
        binding.handle_pointer_event(&touch, |_| path_of(bystander));
        let lift = make_up_event_for_id(second, at(70.0), PointerKind::Touch).expect("up");
        binding.handle_pointer_event(&lift, |_| path_of(bystander));
        let tip = PointerKind::Pen { tool: PenTool::Tip };
        let pen_down = make_down_event_for_id(pen, at(60.0), tip).expect("pen");
        binding.handle_pointer_event(&pen_down, |_| path_of(bystander));
        let pen_up = make_up_event_for_id(pen, at(60.0), tip).expect("pen");
        binding.handle_pointer_event(&pen_up, |_| path_of(bystander));
        assert_eq!(
            seen.borrow().len(),
            4,
            "second finger and pen reached their own target"
        );

        let drag_stage = session.borrow().as_ref().expect("session").stage();
        assert_eq!(drag_stage, Ok(DragStage::InWindow), "still the same drag");
        assert!(take_started().is_empty());
        let end = make_up_event_for_id(finger, at(30.0), PointerKind::Touch).expect("up");
        binding.handle_pointer_event(&end, |_| path_of(source));
        drop(w);
    });
}

// --- hover deadlines, registrations and competing drags ---------------------------------------------

/// `ms` milliseconds into the event timeline.
fn time_at(ms: u64) -> EventTime {
    EventTime::from_nanos(1_000_000_000 + ms * 1_000_000)
}

fn count(log: &Log, entry: &str) -> usize {
    log.borrow().iter().filter(|line| *line == entry).count()
}

fn spring(ms: u64) -> SpringLoading {
    SpringLoading {
        delay: Duration::from_millis(ms),
    }
}

fn an_undated_hover_still_comes_due() {
    in_lane(|f| {
        let _spot = f.spring_spot("U", 0.0, 100.0, Some(COPY), Some(spring(100)));
        // An OS drag carries no event time.
        f.deliver(
            f.presentation,
            DragDropEvent::Entered {
                offer: f.text_offer(),
                allowed: COPY,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        assert_eq!(
            f.owner.next_deadline(),
            Some(EventTime::from_nanos(0)),
            "an undated hover asks for a turn at once"
        );
        f.owner.tick(time_at(0));
        assert_eq!(
            f.owner.next_deadline(),
            Some(time_at(100)),
            "that turn stamped the hover"
        );
        f.owner.tick(time_at(100));
        assert_eq!(count(&f.log, "U:spring"), 1);

        // An in-window move without a time is the same.
        let session = f.start(desktop(), AllowedActions::within(COPY));
        session.pointer_moved(at(10.0), None).expect("live");
        assert_eq!(f.owner.next_deadline(), Some(EventTime::from_nanos(0)));
    });
}

fn a_receiver_removed_mid_hover_leaves_once_and_takes_no_drop() {
    in_lane(|f| {
        let gone = f.unregistered("G", Some(COPY), Some(spring(100)));
        let ticket = f.register(&gone, 0.0, 100.0);
        let session = f.start(desktop(), AllowedActions::within(COPY));
        assert_eq!(
            session.pointer_moved(at(10.0), Some(time_at(0))),
            Ok(MoveOutcome::Tracking(Some(COPY)))
        );
        f.dispatch
            .unregister_local_payload(ticket)
            .expect("registered");
        f.owner.tick(time_at(100));
        assert_eq!(count(&f.log, "G:left"), 1, "{:?}", logged(&f.log));
        assert_eq!(
            count(&f.log, "G:spring"),
            0,
            "an unmounted receiver never spring-loads"
        );
        assert_eq!(
            session.pointer_released(at(10.0), None),
            Ok(Some(DragOutcome::Cancelled))
        );
        assert_eq!(
            count(&f.log, "G:left"),
            1,
            "one leave: {:?}",
            logged(&f.log)
        );

        // An OS drop that names no position, after its accepting receiver went away.
        let target = f.unregistered("H", Some(COPY), None);
        let ticket = f.register(&target, 0.0, 100.0);
        let offer = f.text_offer();
        let id = offer.id();
        f.deliver(
            f.presentation,
            DragDropEvent::Entered {
                offer,
                allowed: COPY,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        f.dispatch
            .unregister_local_payload(ticket)
            .expect("registered");
        f.owner.tick(time_at(200));
        assert_eq!(
            f.source.feedback.lock().last(),
            Some(&(id, DropFeedback { accept: None })),
            "the OS hears that nothing accepts"
        );
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id,
                action: COPY,
                position: None,
            },
        );
        assert_eq!(count(&f.log, "H:dropped COPY"), 0, "{:?}", logged(&f.log));
        assert_eq!(count(&f.log, "H:left"), 1);
        assert_eq!(*f.source.concluded.lock(), [id]);

        // Gone between the last move and the drop, with no turn between them.
        let late = f.unregistered("K", Some(COPY), None);
        let ticket = f.register(&late, 0.0, 100.0);
        let offer = f.text_offer();
        let id = offer.id();
        f.deliver(
            f.presentation,
            DragDropEvent::Entered {
                offer,
                allowed: COPY,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        f.dispatch
            .unregister_local_payload(ticket)
            .expect("registered");
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id,
                action: COPY,
                position: None,
            },
        );
        assert_eq!(count(&f.log, "K:dropped COPY"), 0, "{:?}", logged(&f.log));
        assert_eq!(count(&f.log, "K:left"), 1);
    });
}

fn a_replaced_registration_is_a_new_receiver() {
    in_lane(|f| {
        let first = f.unregistered("R1", Some(COPY), None);
        let ticket = f.register(&first, 0.0, 100.0);
        let session = f.start(desktop(), AllowedActions::within(COPY));
        moved(&session, 10.0);
        let second = f.unregistered("R2", Some(COPY), None);
        f.dispatch
            .replace_local_payload(ticket, Rc::new(DropReceiverSlot(second)))
            .expect("live ticket");
        moved(&session, 20.0);
        assert_eq!(
            session.pointer_released(at(20.0), None),
            Ok(Some(DragOutcome::Dropped { action: COPY }))
        );
        let receivers: Vec<String> = logged(&f.log)
            .into_iter()
            .filter(|line| line.starts_with('R'))
            .collect();
        assert_eq!(
            receivers,
            [
                "R1:enter",
                "R1:left",
                "R2:enter",
                "R2:move",
                "R2:dropped COPY"
            ],
            "the old receiver left and the new one entered before it moved"
        );
    });
}

fn two_sessions_competing_for_one_target() {
    in_lane(|f| {
        let target = f.spot("T", 0.0, 100.0, Some(COPY));
        let w = desktop();
        let value = |name: &str| vec![DragItem::new().with_value(name.to_owned())];
        let first = f.start_with(
            Arc::clone(&w),
            AllowedActions::within(COPY),
            None,
            value("first"),
        );
        let second = f.start_with(
            Arc::clone(&w),
            AllowedActions::within(COPY),
            None,
            value("second"),
        );
        assert_eq!(moved(&first, 10.0), MoveOutcome::Tracking(Some(COPY)));
        assert_eq!(moved(&second, 20.0), MoveOutcome::Tracking(Some(COPY)));
        assert_eq!(count(&f.log, "T:enter"), 2, "one enter per drag");
        assert_eq!(
            first.pointer_released(at(10.0), None),
            Ok(Some(DragOutcome::Dropped { action: COPY }))
        );
        assert_eq!(
            second.stage(),
            Ok(DragStage::InWindow),
            "the other drag still hovers"
        );
        assert_eq!(count(&f.log, "T:left"), 0);
        assert_eq!(
            second.pointer_released(at(20.0), None),
            Ok(Some(DragOutcome::Dropped { action: COPY }))
        );
        let named = |name: &str| Some(name.to_owned());
        assert_eq!(
            *target.values.borrow(),
            [
                named("first"),
                named("second"),
                named("first"),
                named("second")
            ],
            "each drop carried its own drag's value"
        );
        assert_eq!(count(&f.log, "T:dropped COPY"), 2);
        assert_eq!(count(&f.log, "T:left"), 0, "{:?}", logged(&f.log));
    });
}

fn a_receiver_panicking_in_spring_loading_is_contained() {
    in_lane(|f| {
        let failing = f.spring_spot("P", 0.0, 100.0, Some(COPY), Some(spring(100)));
        let _other = f.spring_spot("Q", 0.0, 100.0, None, Some(spring(100)));
        *failing.on_spring.borrow_mut() = Some(Box::new(|| panic!("spring-load failure")));
        let session = f.start(desktop(), AllowedActions::within(COPY));
        session
            .pointer_moved(at(10.0), Some(time_at(0)))
            .expect("live");
        let result = catch_unwind(AssertUnwindSafe(|| f.owner.tick(time_at(100))));
        assert!(result.is_err(), "the failure reaches the caller");
        assert_eq!(count(&f.log, "P:spring"), 1);
        assert_eq!(
            count(&f.log, "Q:spring"),
            1,
            "the other receiver still spring-loaded"
        );
        assert_eq!(f.owner.next_deadline(), None);
        f.owner.tick(time_at(200));
        assert_eq!(count(&f.log, "P:spring"), 1, "once per hover, even failed");
        // The drag goes on: the next move and the drop.
        assert_eq!(moved(&session, 20.0), MoveOutcome::Tracking(Some(COPY)));
        assert_eq!(
            session.pointer_released(at(20.0), None),
            Ok(Some(DragOutcome::Dropped { action: COPY }))
        );
        assert_eq!(count(&f.log, "P:dropped COPY"), 1);
    });
}

fn a_repeated_offer_keeps_one_hover_and_concurrent_offers_stay_apart() {
    in_lane(|f| {
        let _spot = f.spot("O", 0.0, 100.0, Some(COPY));
        let here = Some(Point::new(5.0, 5.0));
        let offer = f.text_offer();
        let id = offer.id();
        // Announced again, now allowing only a move: the hover is the same one, and it
        // answers from the latest OS-permitted set.
        for allowed in [COPY, MOVE] {
            f.deliver(
                f.presentation,
                DragDropEvent::Entered {
                    offer: offer.clone(),
                    allowed,
                    position: here,
                },
            );
        }
        assert_eq!(
            f.source.feedback.lock().last(),
            Some(&(id, DropFeedback { accept: None })),
            "a copy-only receiver accepts nothing once the OS allows only a move"
        );
        f.deliver(f.presentation, DragDropEvent::Exited { id });
        assert_eq!(count(&f.log, "O:enter"), 1, "{:?}", logged(&f.log));
        assert_eq!(count(&f.log, "O:left"), 1);

        // Two offers over one window at once (ADR-0211 §5): neither retires the other.
        let (a, b) = (f.text_offer(), f.text_offer());
        let (a_id, b_id) = (a.id(), b.id());
        for offer in [a, b] {
            f.deliver(
                f.presentation,
                DragDropEvent::Entered {
                    offer,
                    allowed: COPY,
                    position: here,
                },
            );
        }
        f.deliver(f.presentation, DragDropEvent::Exited { id: a_id });
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id: b_id,
                action: COPY,
                position: here,
            },
        );
        assert_eq!(count(&f.log, "O:enter"), 3);
        assert_eq!(count(&f.log, "O:left"), 2);
        assert_eq!(count(&f.log, "O:dropped COPY"), 1);
    });
}

fn autoscroll_frames_ask_innermost_first_until_one_scrolls() {
    in_lane(|f| {
        let frame = Duration::from_nanos(16_666_667);
        let inner = f.spot("I", 0.0, 50.0, None);
        let outer = f.spot("O", 0.0, 100.0, None);
        let inner_scrolls = Rc::new(Cell::new(true));
        let scrolls = Rc::clone(&inner_scrolls);
        *inner.on_autoscroll.borrow_mut() = Some(Box::new(move |_| {
            if scrolls.get() {
                Autoscroll::Scrolling
            } else {
                Autoscroll::Idle
            }
        }));
        *outer.on_autoscroll.borrow_mut() = Some(Box::new(|_| Autoscroll::Idle));
        let frames = |log: &Log| -> Vec<String> {
            log.borrow_mut()
                .drain(..)
                .filter(|line| line.contains("autoscroll"))
                .collect()
        };
        let session = f.start(desktop(), AllowedActions::within(COPY));
        session
            .pointer_moved(at(10.0), Some(time_at(0)))
            .expect("live");
        assert_eq!(
            f.owner.next_deadline(),
            None,
            "the first frame rides the frame the move requests"
        );
        f.owner.tick(time_at(10));
        assert_eq!(
            frames(&f.log),
            ["I:autoscroll 10ms"],
            "the inner one scrolls; the outer one is not asked"
        );
        assert_eq!(
            f.owner.next_deadline(),
            time_at(10).checked_add(frame),
            "a scrolling frame asks for the next"
        );
        f.owner.tick(time_at(26));
        assert_eq!(frames(&f.log), ["I:autoscroll 16ms"]);
        inner_scrolls.set(false);
        f.owner.tick(time_at(42));
        assert_eq!(frames(&f.log), ["I:autoscroll 16ms", "O:autoscroll 16ms"]);
        assert_eq!(f.owner.next_deadline(), None, "nobody scrolls: frames stop");
        f.owner.tick(time_at(60));
        assert_eq!(frames(&f.log), Vec::<String>::new());
        session
            .pointer_moved(at(11.0), Some(time_at(70)))
            .expect("live");
        f.owner.tick(time_at(75));
        assert_eq!(
            frames(&f.log),
            ["I:autoscroll 5ms", "O:autoscroll 5ms"],
            "a move resumes them"
        );

        // A receiver that panics scrolls nothing: the next one out is asked, the
        // failure reaches the caller, and the next move recovers.
        let failed = Rc::new(Cell::new(false));
        let once = Rc::clone(&failed);
        *inner.on_autoscroll.borrow_mut() = Some(Box::new(move |_| {
            assert!(once.replace(true), "autoscroll failure");
            Autoscroll::Scrolling
        }));
        session
            .pointer_moved(at(12.0), Some(time_at(80)))
            .expect("live");
        let result = catch_unwind(AssertUnwindSafe(|| f.owner.tick(time_at(90))));
        assert!(result.is_err());
        assert_eq!(frames(&f.log), ["I:autoscroll 10ms", "O:autoscroll 10ms"]);
        session
            .pointer_moved(at(13.0), Some(time_at(100)))
            .expect("live");
        f.owner.tick(time_at(110));
        assert_eq!(frames(&f.log), ["I:autoscroll 10ms"]);
        assert_eq!(
            session.pointer_released(at(13.0), None),
            Ok(Some(DragOutcome::Cancelled))
        );
        assert_eq!(
            f.owner.next_deadline(),
            None,
            "a finished drag asks for no frame"
        );
    });
}

#[test]
fn drag_sessions() {
    run_table(
        "drag sessions",
        &[
            (
                "an_undated_hover_still_comes_due",
                an_undated_hover_still_comes_due,
            ),
            (
                "a_receiver_removed_mid_hover_leaves_once_and_takes_no_drop",
                a_receiver_removed_mid_hover_leaves_once_and_takes_no_drop,
            ),
            (
                "a_replaced_registration_is_a_new_receiver",
                a_replaced_registration_is_a_new_receiver,
            ),
            (
                "two_sessions_competing_for_one_target",
                two_sessions_competing_for_one_target,
            ),
            (
                "a_receiver_panicking_in_spring_loading_is_contained",
                a_receiver_panicking_in_spring_loading_is_contained,
            ),
            (
                "a_repeated_offer_keeps_one_hover_and_concurrent_offers_stay_apart",
                a_repeated_offer_keeps_one_hover_and_concurrent_offers_stay_apart,
            ),
            (
                "autoscroll_frames_ask_innermost_first_until_one_scrolls",
                autoscroll_frames_ask_innermost_first_until_one_scrolls,
            ),
            (
                "in_window_drag_keeps_second_finger_and_pen",
                in_window_drag_keeps_second_finger_and_pen,
            ),
            (
                "escalates_only_outside_window_with_outside_actions_or_sibling_presentation",
                escalates_only_outside_window_with_outside_actions_or_sibling_presentation,
            ),
            (
                "an_at_start_backend_decides_at_the_start_and_never_escalates_later",
                an_at_start_backend_decides_at_the_start_and_never_escalates_later,
            ),
            (
                "drag_between_presentations_of_one_runtime_redeems_the_value",
                drag_between_presentations_of_one_runtime_redeems_the_value,
            ),
            (
                "drop_reaches_only_last_accepting_target",
                drop_reaches_only_last_accepting_target,
            ),
            (
                "a_receiver_that_accepted_and_then_declined_gets_no_drop",
                a_receiver_that_accepted_and_then_declined_gets_no_drop,
            ),
            (
                "declined_target_under_pointer_receives_leave",
                declined_target_under_pointer_receives_leave,
            ),
            (
                "concurrent_sessions_are_independent",
                concurrent_sessions_are_independent,
            ),
            (
                "session_id_exhaustion_refuses_permanently",
                session_id_exhaustion_refuses_permanently,
            ),
            (
                "value_is_redeemed_only_in_source_runtime",
                value_is_redeemed_only_in_source_runtime,
            ),
            ("refusals_at_start", refusals_at_start),
            (
                "a_drop_is_concluded_once_its_requests_resolve",
                a_drop_is_concluded_once_its_requests_resolve,
            ),
            (
                "a_receiver_that_cancels_the_session_while_entered_still_leaves_once",
                a_receiver_that_cancels_the_session_while_entered_still_leaves_once,
            ),
            (
                "a_panicking_receiver_does_not_strand_the_others",
                a_panicking_receiver_does_not_strand_the_others,
            ),
            (
                "spring_loading_hook_fires_once_per_hover",
                spring_loading_hook_fires_once_per_hover,
            ),
        ],
    );
}
