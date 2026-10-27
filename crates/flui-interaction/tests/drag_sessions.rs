//! Drag-and-drop sessions (ADR-0211): in-window drags, escalation to the OS, delivery to
//! receivers and the identity that keeps a drag's value in its own runtime.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::{Offset, Point, Size};
use flui_foundation::{DragSessionId, IdMint, PresentationId, RenderId, UiRuntimeId};
use flui_interaction::drag::{
    AllowedActions, Autoscroll, DragError, DragFetch, DragHandle, DragItem, DragObserver,
    DragOutcome, DragOwner, DragPoint, DragReach, DragScope, DragSession, DragStage, DragStart,
    DragView, DropReceiver, DropReceiverSlot, MoveOutcome, PendingOsDrag, ProcessNonce,
    SpringLoading,
};
use flui_interaction::events::{
    PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::{
    GestureBinding, HitTestEntry, HitTestHandle, HitTestProbe, HitTestResult,
    InteractionDispatchError, InteractionDispatchHandle, InteractionLane, LocalPayloadTarget,
    PenTool, PointerEvent, PointerId, PointerInfo,
};
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, RepresentationIndex, TransferActions,
    TransferCompleter, TransferFormat, TransferLimits, TransferPayload, TransferRequest,
};
use flui_platform::drag::{OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform::{
    DragDropEvent, EventTime, PlatformWindow, Unsupported, UnsupportedReason, WindowId,
};

/// A drag the OS cannot take: the window has no drag source.
const NO_OS_DRAG: DragError =
    DragError::Unsupported(Unsupported::new(UnsupportedReason::NotOnThisPlatform));
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
    /// Concluding a drop panics, as a failing backend would.
    fail_conclude: std::sync::atomic::AtomicBool,
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
        let (request, completer) = TransferRequest::pending();
        self.requests.lock().push((id, representation, completer));
        request
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        self.feedback.lock().push((id, feedback));
    }

    fn conclude_drop(&self, id: DataTransferId) {
        self.concluded.lock().push(id);
        assert!(
            !self.fail_conclude.load(std::sync::atomic::Ordering::SeqCst),
            "conclude failure"
        );
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
    fn request_redraw(&self) {}
    fn set_cursor(&self, _: flui_platform::CursorIcon) -> Result<(), flui_platform::CursorError> {
        Ok(())
    }
    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        self.source.clone()
    }
    fn state(&self) -> flui_platform::WindowState {
        flui_platform::WindowState::new(
            flui_platform::WindowGeometry::new(None, self.size),
            self.size,
        )
    }
    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        command.against(&flui_platform::WindowCapabilities::NONE)?;
        Ok(())
    }
    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
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
    on_drop: RefCell<Option<Box<dyn Fn()>>>,
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
        if let Some(hook) = &*self.on_drop.borrow() {
            hook();
        }
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
    /// How many offers the simulated OS minted: the next one's slot.
    offers: Cell<u32>,
    presentation: PresentationId,
    windows: RefCell<Vec<Arc<dyn PlatformWindow>>>,
    /// The pointer kinds an OS drag's hit tests asked for, in order.
    hit_kinds: Rc<RefCell<Vec<PointerKind>>>,
    /// How many times the owner was woken for a drop it can now conclude.
    wakes: Arc<AtomicUsize>,
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
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake = Arc::clone(&wakes);
        let owner = DragOwner::new(
            nonce,
            UiRuntimeId::new(runtime),
            source.clone(),
            Arc::new(move || {
                wake.fetch_add(1, Ordering::SeqCst);
            }),
        );
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
            offers: Cell::new(0),
            presentation: presentation(1),
            windows: RefCell::default(),
            hit_kinds: Rc::default(),
            wakes,
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
            on_drop: RefCell::new(None),
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
        let representations: Arc<[_]> = Arc::from(vec![
            flui_platform::data_transfer::RepresentationDescriptor {
                format: TransferFormat::Text,
                declared_len: None,
            },
        ]);
        DataTransferOffer::new(self.mint(), representations)
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
        DataTransferOffer::new(self.mint(), Arc::from(request.representations.clone()))
    }

    /// A fresh offer id, as the OS's one table mints it: never one minted
    /// before.
    fn mint(&self) -> DataTransferId {
        let slot = self.offers.get();
        self.offers
            .set(slot.checked_add(1).expect("the test mints few offers"));
        DataTransferId::new_gen(slot, std::num::NonZeroU32::MIN)
    }

    fn deliver(&self, presentation: PresentationId, event: DragDropEvent) {
        let probe = Rc::clone(&self.probe);
        let kinds = Rc::clone(&self.hit_kinds);
        let mut hit = move |position: Offset<f64>, kind: PointerKind| {
            kinds.borrow_mut().push(kind);
            let mut result = HitTestResult::new();
            probe
                .probe(position, kind, &mut result)
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

/// Enter `pending` on a backend that accepts it: the request the OS now holds.
fn enter_os(pending: PendingOsDrag) -> OsDragRequest {
    pending
        .start(Some(&FakeDragSource(OsDragStart::OnLeavingWindow)))
        .expect("the backend accepts");
    take_started().pop().expect("started")
}

/// The single OS session `f`'s drags decided on, entered on a backend that
/// accepts it.
fn enter_one(f: &Fixture) -> OsDragRequest {
    let mut pending = f.owner.take_pending_os_drags();
    assert_eq!(pending.len(), 1, "one OS session");
    enter_os(pending.remove(0))
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
    let owner = DragOwner::new(
        nonce(),
        UiRuntimeId::new(1),
        f.source.clone(),
        Arc::new(|| {}),
    )
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
            let mut pending = f.owner.take_pending_os_drags();
            let name = case.name;
            match case.expect {
                Expect::InWindow => {
                    assert!(pending.is_empty(), "{name}");
                    assert_eq!(outcome, MoveOutcome::Tracking(None), "{name}");
                    assert_eq!(session.stage(), Ok(DragStage::InWindow), "{name}");
                }
                Expect::Refused => {
                    assert!(pending.is_empty(), "{name}");
                    assert_eq!(outcome, MoveOutcome::Refused(NO_OS_DRAG), "{name}");
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
                    let request = enter_os(pending.remove(0));
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
                    let foreign = flui_platform::drag::DragSessionKey::new(
                        key.nonce(),
                        UiRuntimeId::new(9),
                        key.session(),
                    );
                    f.owner.os_drag_refused(foreign, NO_OS_DRAG);
                    assert_eq!(
                        session.stage(),
                        Ok(DragStage::System),
                        "{name}: foreign refusal"
                    );
                    f.owner.os_drag_refused(key, NO_OS_DRAG);
                    assert_eq!(session.stage(), Ok(DragStage::InWindow), "{name}: refused");
                }
                Expect::IdentityOnly => {
                    assert_eq!(outcome, MoveOutcome::Escalating, "{name}");
                    assert_eq!(pending.len(), 1, "{name}");
                    let request = enter_os(pending.remove(0));
                    assert_eq!(request.allowed, MOVE, "within_app is advertised: {name}");
                    assert_eq!(request.representations.len(), 1, "identity only: {name}");
                    assert_eq!(session.stage(), Ok(DragStage::System), "{name}");
                }
            }
        });
    }
}

/// The host could not start the OS session a drag escalated to: back in its
/// window the drag targets and drops as before, and it does not ask the
/// backend that refused again.
fn a_refused_os_drag_still_drops_in_its_window() {
    in_lane(|f| {
        let _target = f.spot("T", 0.0, 50.0, Some(COPY));
        let w = desktop();
        let session = f.start_with(
            Arc::clone(&w),
            AllowedActions::new(COPY, COPY),
            None,
            vec![DragItem::new().with_text("payload")],
        );
        assert_eq!(moved(&session, 150.0), MoveOutcome::Escalating);
        let mut pending = f.owner.take_pending_os_drags();
        assert_eq!(pending.len(), 1);
        let pending = pending.remove(0);
        let key = pending.key();
        assert_eq!(
            pending.start(None),
            Err(NO_OS_DRAG),
            "a window with no drag source refuses"
        );
        assert_eq!(
            session.stage(),
            Ok(DragStage::System),
            "until the host says so"
        );
        f.owner.os_drag_refused(key, NO_OS_DRAG);
        assert_eq!(
            moved(&session, 10.0),
            MoveOutcome::Tracking(Some(COPY)),
            "the receiver under the pointer is entered again"
        );
        assert_eq!(moved(&session, 150.0), MoveOutcome::Refused(NO_OS_DRAG));
        assert!(
            !f.owner.has_pending_os_drags(),
            "the refused start is not asked for again"
        );
        moved(&session, 10.0);
        assert_eq!(
            session.pointer_released(at(10.0), None).expect("live"),
            Some(DragOutcome::Dropped { action: COPY })
        );
        assert!(logged(&f.log).contains(&"T:dropped COPY".to_owned()));
    });
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
        let request = enter_one(&f);
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
            *f.hit_kinds.borrow(),
            [PointerKind::Touch],
            "the hit test asks with the touch the session started from, so a touch gets \
             touch-sized targets"
        );
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
            on_drop: RefCell::new(None),
        });
        f.register(&target, 0.0, 100.0);
        let offer = f.text_offer();
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
        assert_eq!(
            *f.hit_kinds.borrow(),
            [PointerKind::Unknown, PointerKind::Unknown],
            "another application's drag names no pointer"
        );
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

/// Accepts a move, and panics taking the drop.
struct FailingTake;

impl DropReceiver for FailingTake {
    fn drag_entered(&self, _: &DragView<'_>, _: DragPoint) -> DropFeedback {
        DropFeedback { accept: Some(MOVE) }
    }
    fn drag_moved(&self, _: &DragView<'_>, _: DragPoint) -> DropFeedback {
        DropFeedback { accept: Some(MOVE) }
    }
    fn drag_left(&self, _: &DragView<'_>) {}
    fn dropped(&self, _: &DragView<'_>, _: DragPoint, _: TransferActions) {
        panic!("take failure");
    }
}

/// A held cut is pasted into a receiver that panics taking it: the cut ends
/// cancelled, as a drop whose receiver failed does, and a second paste cannot
/// move it again.
fn a_paste_whose_receiver_panics_ends_the_held_cut() {
    in_lane(|f| {
        let target = f
            .dispatch
            .register_local_payload(Rc::new(DropReceiverSlot(Rc::new(FailingTake))))
            .expect("active lane");
        let w = desktop();
        let tracker = flui_interaction::pointer::MouseTracker::new();
        let drag_and_drop = flui_interaction::drag::DragAndDrop::new(
            f.handle.clone(),
            f.presentation,
            Arc::downgrade(&w),
            &tracker,
        );
        let start = drag_and_drop
            .drag(
                vec![DragItem::new().with_value("payload".to_owned())],
                AllowedActions::within(MOVE),
            )
            .observer(Rc::new(Observer(Rc::clone(&f.log))));
        let cut = drag_and_drop.hold(start, MOVE).expect("a cut is held");
        let point = DragPoint {
            global: at(10.0),
            local: at(10.0),
            time: None,
        };
        let pasted = catch_unwind(AssertUnwindSafe(|| drag_and_drop.paste(target, point)));
        assert!(pasted.is_err(), "the receiver's panic reaches the caller");
        assert_eq!(cut.outcome(), Some(DragOutcome::Cancelled));
        assert_eq!(
            drag_and_drop.paste(target, point),
            None,
            "nothing is held any more"
        );
        assert_eq!(
            logged(&f.log)
                .iter()
                .filter(|line| line.starts_with("observer:ended"))
                .count(),
            1,
            "the source hears the end once"
        );
    });
}

/// Panics hearing a session end.
struct FailingObserver;

impl DragObserver for FailingObserver {
    fn ended(&self, _: DragOutcome) {
        panic!("observer failure");
    }
}

/// A cancel whose receiver fails to leave and whose observer then fails to
/// hear the end reports the receiver's failure, the first one.
fn a_cancel_whose_observer_panics_keeps_the_first_failure() {
    in_lane(|f| {
        let spot = f.spot("P", 0.0, 100.0, Some(COPY));
        *spot.on_leave.borrow_mut() = Some(Box::new(|| panic!("receiver leave failure")));
        let w = desktop();
        f.windows.borrow_mut().push(Arc::clone(&w));
        let session = f
            .handle
            .start(
                DragStart::new(
                    f.presentation,
                    vec![DragItem::new().with_value("payload".to_owned())],
                    AllowedActions::within(COPY),
                )
                .window(Arc::downgrade(&w))
                .hit_test(f.hit.clone())
                .observer(Rc::new(FailingObserver)),
            )
            .expect("a valid drag starts");
        moved(&session, 10.0);
        let payload = catch_unwind(AssertUnwindSafe(|| session.cancel()))
            .expect_err("the failure reaches the caller");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"receiver leave failure"),
            "the first failure is the one reported"
        );
        assert_eq!(session.stage(), Ok(DragStage::Concluded));
    });
}

/// The window an OS drop landed in closes while a request made from the drop
/// is outstanding: the offer is concluded then, once.
fn closing_the_window_that_took_a_drop_waits_for_its_fetch() {
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
            on_drop: RefCell::new(None),
        });
        f.register(&target, 0.0, 100.0);
        let offer = f.text_offer();
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
        assert!(
            f.source.concluded.lock().is_empty(),
            "a fetch is outstanding"
        );
        f.owner.detach_presentation(f.presentation);
        assert!(
            f.source.concluded.lock().is_empty(),
            "the fetch its window made is still outstanding"
        );
        drop(
            target
                .fetched
                .borrow_mut()
                .pop()
                .expect("a request was made"),
        );
        f.owner.tick(EventTime::from_nanos(1));
        assert_eq!(
            *f.source.concluded.lock(),
            [id],
            "concluded once it resolves"
        );
        f.owner.tick(EventTime::from_nanos(2));
        assert_eq!(*f.source.concluded.lock(), [id], "concluded once");
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
        let request = enter_one(&a);
        drop(w);
        (request, session)
    });
    assert_eq!(request.representations.len(), 2);
    let offer = a.offer_for(&request);
    // The OS holds the end hook until the drag ends.
    let OsDragRequest {
        mut provider,
        on_end,
        ..
    } = request;

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
            on_drop: RefCell::new(None),
        });
        b.register(&target, 0.0, 100.0);
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
    drop(on_end);
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

/// A receiver panics in the hover update an OS drop carries: the drop still
/// runs, every receiver entered hears its end, the OS hears the target is
/// done, and then the panic propagates.
fn a_panicking_hover_before_an_os_drop_still_concludes_it() {
    in_lane(|f| {
        let _a = f.spot("A", 0.0, 50.0, Some(COPY));
        let b = f.spot("B", 50.0, 100.0, Some(COPY));
        *b.on_enter.borrow_mut() = Some(Box::new(|| panic!("hover failure")));
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
        let dropped = catch_unwind(AssertUnwindSafe(|| {
            f.deliver(
                f.presentation,
                DragDropEvent::Dropped {
                    id,
                    action: COPY,
                    position: Some(Point::new(80.0, 5.0)),
                },
            );
        }));
        assert!(dropped.is_err(), "the receiver's panic reaches the caller");
        assert!(
            logged(&f.log).contains(&"B:left".to_owned()),
            "the receiver entered by the drop's move hears its end: {:?}",
            logged(&f.log)
        );
        assert_eq!(*f.source.concluded.lock(), [id], "the offer is concluded");
        assert_eq!(
            f.source.feedback.lock().last(),
            Some(&(id, DropFeedback { accept: None })),
            "the OS hears that nothing accepts the failed hover"
        );
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
            on_drop: RefCell::new(None),
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

// --- the end of an OS session --------------------------------------------------------------------

/// Accepts or refuses every start, dropping the request either way.
struct ForgetfulDragSource(Result<(), DragError>);

impl PlatformDragSource for ForgetfulDragSource {
    fn start_timing(&self) -> OsDragStart {
        OsDragStart::OnLeavingWindow
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        drop(request);
        self.0
    }
}

/// Keeps every request it refuses.
struct HoardingDragSource;

impl PlatformDragSource for HoardingDragSource {
    fn start_timing(&self) -> OsDragStart {
        OsDragStart::OnLeavingWindow
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        STARTED.with(|started| started.borrow_mut().push(request));
        Err(NO_OS_DRAG)
    }
}

/// A session that left its window and waits for the host to enter its OS session.
fn escalated(f: &Fixture) -> DragSession {
    let session = f.start_with(
        desktop(),
        AllowedActions::new(COPY, COPY),
        None,
        vec![DragItem::new().with_text("payload")],
    );
    assert_eq!(moved(&session, 150.0), MoveOutcome::Escalating);
    session
}

fn the_one_pending(f: &Fixture) -> PendingOsDrag {
    let mut pending = f.owner.take_pending_os_drags();
    assert_eq!(pending.len(), 1, "one OS session");
    pending.remove(0)
}

const ENDED_CANCELLED: &str = "observer:ended Cancelled";

/// A session the OS took ends however its backend lets go of the end hook:
/// called, dropped uncalled while the OS runs it or inside a start that
/// succeeded, or never started at all. A hook dropped inside a start that
/// failed is the refusal's, and one a refusing backend kept means nothing.
fn an_os_session_ends_however_its_end_hook_goes() {
    in_lane(|f| {
        let session = escalated(f);
        let request = enter_os(the_one_pending(f));
        assert_eq!(session.stage(), Ok(DragStage::System));
        drop(request);
        assert_eq!(
            session.outcome(),
            Some(DragOutcome::Cancelled),
            "dropped uncalled"
        );
        assert_eq!(count(&f.log, ENDED_CANCELLED), 1);
    });
    in_lane(|f| {
        let session = escalated(f);
        assert_eq!(
            the_one_pending(f).start(Some(&ForgetfulDragSource(Ok(())))),
            Ok(())
        );
        assert_eq!(
            session.outcome(),
            Some(DragOutcome::Cancelled),
            "dropped inside a start that succeeded"
        );
        assert_eq!(count(&f.log, ENDED_CANCELLED), 1);
    });
    in_lane(|f| {
        let session = escalated(f);
        drop(the_one_pending(f));
        assert_eq!(
            session.outcome(),
            Some(DragOutcome::Cancelled),
            "never started"
        );
        assert_eq!(count(&f.log, ENDED_CANCELLED), 1);
    });
    in_lane(|f| {
        let session = escalated(f);
        let pending = the_one_pending(f);
        let key = pending.key();
        assert_eq!(
            pending.start(Some(&ForgetfulDragSource(Err(NO_OS_DRAG)))),
            Err(NO_OS_DRAG)
        );
        assert_eq!(
            session.stage(),
            Ok(DragStage::System),
            "a refusal, not an end"
        );
        f.owner.os_drag_refused(key, NO_OS_DRAG);
        assert_eq!(session.stage(), Ok(DragStage::InWindow));
        assert_eq!(count(&f.log, ENDED_CANCELLED), 0);
    });
    in_lane(|f| {
        let session = escalated(f);
        let pending = the_one_pending(f);
        let key = pending.key();
        assert_eq!(pending.start(Some(&HoardingDragSource)), Err(NO_OS_DRAG));
        f.owner.os_drag_refused(key, NO_OS_DRAG);
        let kept = take_started().pop().expect("kept");
        (kept.on_end)(OsDragEnd::Dropped(COPY));
        assert_eq!(
            session.stage(),
            Ok(DragStage::InWindow),
            "the refused session's old hook ends nothing"
        );
        assert!(
            !logged(&f.log)
                .iter()
                .any(|line| line.starts_with("observer:ended")),
            "{:?}",
            logged(&f.log)
        );
    });
}

/// A value whose destructor fails.
struct PanicsOnDrop;

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        panic!("value dropped");
    }
}

/// The OS ends a session whose handle is gone: the entry goes with it, and the
/// drag's value failing as it is dropped does not keep the observer from
/// hearing the end.
fn an_os_end_whose_value_fails_to_drop_still_tells_the_observer() {
    in_lane(|f| {
        let session = f.start_with(
            desktop(),
            AllowedActions::new(COPY, COPY),
            None,
            vec![
                DragItem::new()
                    .with_text("payload")
                    .with_value(PanicsOnDrop),
            ],
        );
        assert_eq!(moved(&session, 150.0), MoveOutcome::Escalating);
        let OsDragRequest {
            provider, on_end, ..
        } = enter_os(the_one_pending(f));
        drop(provider);
        drop(session);
        let failure = catch_unwind(AssertUnwindSafe(|| on_end(OsDragEnd::Dropped(COPY))))
            .expect_err("the value's failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("value dropped")
        );
        assert_eq!(
            count(
                &f.log,
                "observer:ended Dropped { action: TransferActions(COPY) }"
            ),
            1,
            "{:?}",
            logged(&f.log)
        );
    });
}

/// A receiver fails with an OS drop, and so does telling the OS the target is
/// done: the receiver's failure is the one that propagates.
fn a_drop_whose_receiver_and_conclusion_fail_reports_the_receiver() {
    in_lane(|f| {
        let spot = f.spot("D", 0.0, 100.0, Some(COPY));
        *spot.on_drop.borrow_mut() = Some(Box::new(|| panic!("drop failure")));
        f.source
            .fail_conclude
            .store(true, std::sync::atomic::Ordering::SeqCst);
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
        let failure = catch_unwind(AssertUnwindSafe(|| {
            f.deliver(
                f.presentation,
                DragDropEvent::Dropped {
                    id,
                    action: COPY,
                    position: None,
                },
            );
        }))
        .expect_err("the drop fails");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("drop failure")
        );
        assert_eq!(*f.source.concluded.lock(), [id], "the OS was still told");
    });
}

/// One OS offer over two windows of the runtime is two hovers: closing one
/// window leaves its receivers, and the other window's hover takes the drop.
fn one_offer_over_two_windows_is_two_hovers() {
    in_lane(|f| {
        let second = presentation(2);
        f.owner
            .attach_presentation(second, WindowId(2), f.hit.clone());
        let _first_spot = f.spot("A", 0.0, 50.0, Some(COPY));
        let _second_spot = f.spot("B", 50.0, 100.0, Some(COPY));
        let offer = f.text_offer();
        let id = offer.id();
        for (over, x) in [(f.presentation, 5.0), (second, 80.0)] {
            f.deliver(
                over,
                DragDropEvent::Entered {
                    offer: offer.clone(),
                    allowed: COPY,
                    position: Some(Point::new(x, 5.0)),
                },
            );
        }
        f.owner.detach_presentation(second);
        assert_eq!(
            logged(&f.log),
            ["A:enter", "B:enter", "B:left"],
            "the closing window's hover left, the other's stays"
        );
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id,
                action: COPY,
                position: None,
            },
        );
        assert_eq!(count(&f.log, "A:dropped COPY"), 1, "{:?}", logged(&f.log));
    });
}

/// A receiver over one window fetches from an OS offer, and the offer drops
/// over another window: the OS is not told the target is done until that
/// fetch resolves.
fn a_drop_waits_for_a_fetch_made_over_another_window() {
    in_lane(|f| {
        let (id, fetch) = dropped_while_fetching_elsewhere(f);
        drop(fetch);
        f.owner.tick(time_at(0));
        assert_eq!(*f.source.concluded.lock(), [id]);
        assert_eq!(
            count(&f.log, "B:left"),
            1,
            "the other window's hover leaves with the offer"
        );
    });
}

/// The window the drop landed in closes while a fetch made over another
/// window is outstanding: the OS is still not told until that fetch resolves.
fn closing_the_dropped_window_waits_for_a_fetch_made_over_another() {
    in_lane(|f| {
        let (id, fetch) = dropped_while_fetching_elsewhere(f);
        f.owner.detach_presentation(f.presentation);
        assert!(
            f.source.concluded.lock().is_empty(),
            "the fetch over the other window is outstanding"
        );
        drop(fetch);
        f.owner.tick(time_at(0));
        assert_eq!(*f.source.concluded.lock(), [id]);
    });
}

/// The same offer hovers two windows, each over an accepting receiver, and the
/// pointer is over the first: the second window's receiver going away does not
/// withdraw the first window's answer.
fn another_windows_hover_keeps_the_answer_standing() {
    in_lane(|f| {
        let second = presentation(2);
        f.owner
            .attach_presentation(second, WindowId(2), f.hit.clone());
        let _first_spot = f.spot("A", 0.0, 50.0, Some(COPY));
        let leaving = f.unregistered("B", Some(COPY), None);
        let ticket = f.register(&leaving, 50.0, 100.0);
        let offer = f.text_offer();
        let id = offer.id();
        for (over, x) in [(second, 80.0), (f.presentation, 5.0)] {
            f.deliver(
                over,
                DragDropEvent::Entered {
                    offer: offer.clone(),
                    allowed: COPY,
                    position: Some(Point::new(x, 5.0)),
                },
            );
        }
        f.dispatch
            .unregister_local_payload(ticket)
            .expect("registered");
        f.owner.tick(time_at(0));
        assert_eq!(count(&f.log, "B:left"), 1, "{:?}", logged(&f.log));
        assert_eq!(
            f.source.feedback.lock().last(),
            Some(&(id, DropFeedback { accept: Some(COPY) })),
            "the window under the pointer still accepts"
        );
    });
}

/// A receiver fetches from an OS offer over one window, and the pointer moves
/// on: that window hears the offer exit, and the drop lands over another. The
/// OS is not told the target is done until the fetch resolves.
fn a_fetch_from_a_hover_that_left_holds_the_drop_open() {
    in_lane(|f| {
        let (offer, fetch) = fetched_over_a_window_that_left(f, true);
        let id = offer.id();
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id,
                action: COPY,
                position: None,
            },
        );
        assert!(
            f.source.concluded.lock().is_empty(),
            "the fetch made over the window the pointer left is outstanding"
        );
        drop(fetch);
        f.owner.tick(time_at(0));
        assert_eq!(*f.source.concluded.lock(), [id]);
    });
}

/// The only hover of an offer leaves while a fetch it made is in flight, and
/// the offer comes back over another window and drops: the earlier fetch
/// still holds the drop open.
fn an_offer_outlives_its_last_hover_while_a_fetch_is_in_flight() {
    in_lane(|f| {
        let (offer, fetch) = fetched_over_a_window_that_left(f, false);
        let id = offer.id();
        f.deliver(
            f.presentation,
            DragDropEvent::Entered {
                offer,
                allowed: COPY,
                position: Some(Point::new(5.0, 5.0)),
            },
        );
        f.deliver(
            f.presentation,
            DragDropEvent::Dropped {
                id,
                action: COPY,
                position: None,
            },
        );
        assert!(
            f.source.concluded.lock().is_empty(),
            "the fetch made before the offer left is outstanding"
        );
        drop(fetch);
        f.owner.tick(time_at(0));
        assert_eq!(*f.source.concluded.lock(), [id]);
    });
}

/// An offer enters a second window, whose receiver fetches from it, and then
/// exits that window; with `stay`, the offer is still over the first window
/// meanwhile, otherwise the second window was its only hover.
fn fetched_over_a_window_that_left(f: &Fixture, stay: bool) -> (DataTransferOffer, DragFetch) {
    let second = presentation(2);
    f.owner
        .attach_presentation(second, WindowId(2), f.hit.clone());
    let _first_spot = f.spot("A", 0.0, 50.0, Some(COPY));
    let fetcher = Rc::new(Spot {
        name: "B",
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
        on_drop: RefCell::new(None),
    });
    f.register(&fetcher, 50.0, 100.0);
    let offer = f.text_offer();
    let id = offer.id();
    let mut over = vec![(second, 80.0)];
    if stay {
        over.push((f.presentation, 5.0));
    }
    for (window, x) in over {
        f.deliver(
            window,
            DragDropEvent::Entered {
                offer: offer.clone(),
                allowed: COPY,
                position: Some(Point::new(x, 5.0)),
            },
        );
    }
    let fetch = fetcher.fetched.borrow_mut().pop().expect("B fetched");
    f.deliver(second, DragDropEvent::Exited { id });
    assert_eq!(count(&f.log, "B:left"), 1);
    (offer, fetch)
}

/// The last fetch of a dropped offer ends on another thread while the owner's
/// loop sleeps: releasing it wakes the owner, which then has the drop to
/// conclude. Without the wake nothing would tick the owner until unrelated
/// input arrived, and the OS would wait on the target all that time.
fn the_last_fetch_resolved_off_thread_wakes_the_owner() {
    in_lane(|f| {
        let (id, fetch) = dropped_while_fetching_elsewhere(f);
        let (_, index, completer) = f
            .source
            .requests
            .lock()
            .pop()
            .expect("the receiver's request reached the source");
        assert_eq!(index, RepresentationIndex(0));
        assert_eq!(
            f.wakes.load(Ordering::SeqCst),
            0,
            "the fetch is outstanding"
        );
        assert_eq!(f.owner.next_deadline(), None, "nothing to conclude yet");

        completer.complete(Ok(TransferPayload::Text("hello".into())));
        std::thread::spawn(move || {
            let mut fetch = std::pin::pin!(fetch);
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(matches!(
                fetch.as_mut().poll(&mut context),
                std::task::Poll::Ready(Ok(_))
            ));
        })
        .join()
        .expect("the fetch resolves off the owner's thread");

        assert_eq!(
            f.wakes.load(Ordering::SeqCst),
            1,
            "the owner is woken when the last request of the drop resolves"
        );
        assert_eq!(f.owner.next_deadline(), Some(EventTime::from_nanos(0)));
        f.owner.tick(time_at(0));
        assert_eq!(*f.source.concluded.lock(), [id]);
    });
}

/// A fetch dropped on another thread before it resolves releases the drop the
/// same way.
fn the_last_fetch_dropped_off_thread_wakes_the_owner() {
    in_lane(|f| {
        let (id, fetch) = dropped_while_fetching_elsewhere(f);
        std::thread::spawn(move || drop(fetch))
            .join()
            .expect("the fetch is dropped off the owner's thread");

        assert_eq!(f.wakes.load(Ordering::SeqCst), 1);
        f.owner.tick(time_at(0));
        assert_eq!(*f.source.concluded.lock(), [id]);
    });
}

/// An offer over two windows, a receiver over the second fetching from it,
/// dropped over the first: the drop and the outstanding fetch.
fn dropped_while_fetching_elsewhere(f: &Fixture) -> (DataTransferId, DragFetch) {
    let second = presentation(2);
    f.owner
        .attach_presentation(second, WindowId(2), f.hit.clone());
    let _first_spot = f.spot("A", 0.0, 50.0, Some(COPY));
    let fetcher = Rc::new(Spot {
        name: "B",
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
        on_drop: RefCell::new(None),
    });
    f.register(&fetcher, 50.0, 100.0);
    let offer = f.text_offer();
    let id = offer.id();
    for (over, x) in [(second, 80.0), (f.presentation, 5.0)] {
        f.deliver(
            over,
            DragDropEvent::Entered {
                offer: offer.clone(),
                allowed: COPY,
                position: Some(Point::new(x, 5.0)),
            },
        );
    }
    let fetch = fetcher.fetched.borrow_mut().pop().expect("B fetched");
    f.deliver(
        f.presentation,
        DragDropEvent::Dropped {
            id,
            action: COPY,
            position: None,
        },
    );
    assert!(
        f.source.concluded.lock().is_empty(),
        "the fetch over the other window is outstanding"
    );
    (id, fetch)
}

/// A receiver drawn through a perspective tilt.
struct TiltedPlane {
    seen: RefCell<Vec<Offset<f64>>>,
}

impl DropReceiver for TiltedPlane {
    fn drag_entered(&self, _: &DragView<'_>, at: DragPoint) -> DropFeedback {
        self.seen.borrow_mut().push(at.local);
        DropFeedback { accept: Some(COPY) }
    }
    fn drag_moved(&self, _: &DragView<'_>, at: DragPoint) -> DropFeedback {
        self.seen.borrow_mut().push(at.local);
        DropFeedback { accept: Some(COPY) }
    }
    fn drag_left(&self, _: &DragView<'_>) {}
    fn dropped(&self, _: &DragView<'_>, _: DragPoint, _: TransferActions) {}
}

/// Puts one receiver under a perspective tilt over the whole window.
struct TiltProbe {
    forward: flui_foundation::geometry::Matrix4,
    target: LocalPayloadTarget,
}

impl HitTestProbe for TiltProbe {
    fn probe(
        &self,
        _: Offset<f64>,
        _: PointerKind,
        result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        result
            .with_paint_transform(self.forward, |result| {
                result.add(HitTestEntry::new(RenderId::new(1)).metadata(Arc::new(self.target)));
            })
            .expect("an invertible tilt");
        Ok(())
    }
}

/// A receiver under a perspective ancestor sees the drag at the point of its
/// own plane under the pointer, as a pointer target under it would.
fn a_receiver_under_perspective_sees_its_own_plane() {
    use flui_foundation::geometry::Matrix4;
    in_lane(|f| {
        // Perspective with depth, over a tilt toward the viewer.
        let mut perspective = Matrix4::identity();
        *perspective.get_mut(3, 2) = -0.002;
        let forward = perspective * Matrix4::rotation_x(0.6);
        let plane = Rc::new(TiltedPlane {
            seen: RefCell::new(Vec::new()),
        });
        let target = f
            .dispatch
            .register_local_payload(Rc::new(DropReceiverSlot(plane.clone())))
            .expect("active lane");
        let hit = HitTestHandle::new(f.dispatch.clone(), Rc::new(TiltProbe { forward, target }));
        let window = desktop();
        let session = f
            .handle
            .start(
                DragStart::new(
                    f.presentation,
                    vec![DragItem::new().with_value("payload".to_owned())],
                    AllowedActions::within(COPY),
                )
                .pointer(PointerInfo::new(
                    PointerId::try_from(1_u64).expect("pointer"),
                    PointerKind::Touch,
                ))
                .window(Arc::downgrade(&window))
                .hit_test(hit),
            )
            .expect("a valid drag starts");
        // Where the plane's own point (20, 30) appears on screen.
        let (x, y) = forward
            .project_plane_point(20.0, 30.0)
            .expect("the point is in front of the camera");
        session
            .pointer_moved(Offset::new(x, y), None)
            .expect("a live session");
        let seen = plane.seen.borrow();
        let local = seen.first().expect("the receiver is under the drag");
        assert!(
            (local.dx - 20.0).abs() < 1e-6 && (local.dy - 30.0).abs() < 1e-6,
            "the receiver's own point, not a projection of the screen point: {local:?}"
        );
        drop(seen);
        session.cancel().expect("a live session");
    });
}

#[test]
fn drag_sessions() {
    run_table(
        "drag sessions",
        &[
            (
                "a_receiver_under_perspective_sees_its_own_plane",
                a_receiver_under_perspective_sees_its_own_plane,
            ),
            (
                "a_drop_waits_for_a_fetch_made_over_another_window",
                a_drop_waits_for_a_fetch_made_over_another_window,
            ),
            (
                "the_last_fetch_resolved_off_thread_wakes_the_owner",
                the_last_fetch_resolved_off_thread_wakes_the_owner,
            ),
            (
                "the_last_fetch_dropped_off_thread_wakes_the_owner",
                the_last_fetch_dropped_off_thread_wakes_the_owner,
            ),
            (
                "a_fetch_from_a_hover_that_left_holds_the_drop_open",
                a_fetch_from_a_hover_that_left_holds_the_drop_open,
            ),
            (
                "an_offer_outlives_its_last_hover_while_a_fetch_is_in_flight",
                an_offer_outlives_its_last_hover_while_a_fetch_is_in_flight,
            ),
            (
                "closing_the_dropped_window_waits_for_a_fetch_made_over_another",
                closing_the_dropped_window_waits_for_a_fetch_made_over_another,
            ),
            (
                "another_windows_hover_keeps_the_answer_standing",
                another_windows_hover_keeps_the_answer_standing,
            ),
            (
                "an_os_session_ends_however_its_end_hook_goes",
                an_os_session_ends_however_its_end_hook_goes,
            ),
            (
                "an_os_end_whose_value_fails_to_drop_still_tells_the_observer",
                an_os_end_whose_value_fails_to_drop_still_tells_the_observer,
            ),
            (
                "a_drop_whose_receiver_and_conclusion_fail_reports_the_receiver",
                a_drop_whose_receiver_and_conclusion_fail_reports_the_receiver,
            ),
            (
                "one_offer_over_two_windows_is_two_hovers",
                one_offer_over_two_windows_is_two_hovers,
            ),
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
                "a_refused_os_drag_still_drops_in_its_window",
                a_refused_os_drag_still_drops_in_its_window,
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
                "a_cancel_whose_observer_panics_keeps_the_first_failure",
                a_cancel_whose_observer_panics_keeps_the_first_failure,
            ),
            (
                "a_paste_whose_receiver_panics_ends_the_held_cut",
                a_paste_whose_receiver_panics_ends_the_held_cut,
            ),
            (
                "closing_the_window_that_took_a_drop_waits_for_its_fetch",
                closing_the_window_that_took_a_drop_waits_for_its_fetch,
            ),
            (
                "a_panicking_hover_before_an_os_drop_still_concludes_it",
                a_panicking_hover_before_an_os_drop_still_concludes_it,
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
