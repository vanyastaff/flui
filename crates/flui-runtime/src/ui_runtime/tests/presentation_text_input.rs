use flui_interaction::testing::InMemoryTextStore;
use flui_platform::text_store::{LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError};

use super::*;

/// A concrete headless recorder and the same value viewed through the
/// platform capability supplied to the presentation.
fn headless_text_input() -> (
    Arc<flui_native::FakeTextInput>,
    Arc<dyn flui_platform::PlatformTextInput>,
) {
    let fake = Arc::new(flui_native::FakeTextInput::new());
    let capability: Arc<dyn flui_platform::PlatformTextInput> = fake.clone();
    (fake, capability)
}

/// A client over a fresh in-memory store, and the store.
fn in_memory_client(text: &str) -> (Rc<InMemoryTextStore>, flui_interaction::TextInputClient) {
    let store = InMemoryTextStore::new(text);
    let erased: Rc<dyn TextStore> = store.clone(); // the presentation holds the field's store through the erased contract.
    (store, flui_interaction::TextInputClient::new(erased))
}

/// Asks `store` for an async read-write lock on every build, logging around
/// it; the grant logs the scheduler phase it ran in, then runs `on_grant`.
#[derive(Clone)]
struct LockRequester {
    store: Rc<dyn TextStore>,
    scheduler: flui_scheduler::UpdateScheduler,
    on_grant: Rc<dyn Fn()>,
    log: Rc<std::cell::RefCell<Vec<&'static str>>>,
    phases: Rc<std::cell::RefCell<Vec<SchedulerPhase>>>,
    outcomes: Rc<std::cell::RefCell<Vec<Result<LockOutcome, TextStoreError>>>>,
}

impl flui_view::View for LockRequester {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for LockRequester {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.log.borrow_mut().push("build");
        let (log, phases, scheduler, on_grant) = (
            Rc::clone(&self.log),
            Rc::clone(&self.phases),
            self.scheduler.clone(),
            Rc::clone(&self.on_grant),
        );
        let outcome = self.store.request_lock(
            LockGrant::read_write(move |_| {
                log.borrow_mut().push("grant");
                phases.borrow_mut().push(scheduler.phase());
                on_grant();
            }),
            LockTiming::Async,
        );
        self.outcomes.borrow_mut().push(outcome);
        self.log.borrow_mut().push("build ends");
        SizedBox::square(10.0)
    }
}

/// A UI runtime with a text-input presentation and a [`LockRequester`] as its
/// root, after one frame driven the way the runners drive one
/// (`UiRuntime::pump`).
fn drive_one_frame_with(
    on_grant: impl FnOnce(&UiRuntime) -> Rc<dyn Fn()>,
) -> (UiRuntime, LockRequester) {
    let (_fake, text_input) = headless_text_input();
    let mut ui_runtime = UiRuntime::for_test_with_text_input(Some(text_input));
    let (concrete, client) = in_memory_client("abc");
    let store: Rc<dyn TextStore> = concrete; // the requester asks through the erased contract, as an input method does.
    let _token = ui_runtime
        .text_input_handle()
        .attach(client)
        .expect("headless presentation supports text input");
    let requester = LockRequester {
        store,
        scheduler: ui_runtime.scheduler().clone(),
        on_grant: on_grant(&ui_runtime),
        log: Rc::default(),
        phases: Rc::default(),
        outcomes: Rc::default(),
    };
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&requester))
        .expect("attach succeeds");
    assert!(
        requester.log.borrow().is_empty(),
        "attaching schedules the build; the frame runs it"
    );
    let _ = ui_runtime.pump(
        &mut flui_foundation::ManualClock::new(),
        &mut ScriptedSink::always_presents(),
    );
    (ui_runtime, requester)
}

/// A lock an input method asks for while the UI runtime drives a frame waits
/// for the whole drive to return (ADR-0027 §3), then runs once, with the
/// scheduler back in `Idle`.
///
/// Red-check: drop the `TextCommitsClosed::close` guard from
/// `UiRuntime::drive_frame`, the drive `UiRuntime::pump` runs its frame in — the
/// grant runs inside `build` and the outcome is `Granted`; run the anchor
/// inside the frame — the grant's phase is not `Idle`; have the pump call the
/// scheduler's drive itself — the outcome is `Granted` again.
pub(crate) fn a_text_store_lock_requested_during_a_frame_is_granted_after_the_drive_returns() {
    let (_ui_runtime, requester) = drive_one_frame_with(|_| Rc::new(|| {}));

    assert_eq!(
        *requester.outcomes.borrow(),
        [Ok(LockOutcome::Deferred)],
        "a lock asked for inside the frame is queued, not granted"
    );
    assert_eq!(
        *requester.log.borrow(),
        ["build", "build ends", "grant"],
        "the grant ran once, after the frame returned"
    );
    assert_eq!(
        *requester.phases.borrow(),
        [SchedulerPhase::Idle],
        "the commit anchor runs outside the frame transaction"
    );
    assert_eq!(
        requester
            .store
            .request_lock(LockGrant::read(|_| {}), LockTiming::Sync),
        Ok(LockOutcome::Granted),
        "the drive reopened commits"
    );
}

/// A host that records which store it was told to serve.
#[derive(Default)]
struct FocusLog(std::cell::RefCell<Vec<bool>>);

impl flui_platform::TextStoreHost for FocusLog {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        self.0.borrow_mut().push(store.is_some());
    }

    fn complete_composition(
        &self,
        _: &Rc<dyn TextStore>,
    ) -> Result<
        flui_platform::text_store::CompositionEnd,
        flui_platform::text_store::TextStoreHostError,
    > {
        Ok(flui_platform::text_store::CompositionEnd::Committed)
    }
}

/// A window that carries a text-store host takes text input through it, even
/// though the headless window also offers a push capability: the
/// presentation's owner tells the host which store is focused (ADR-0135).
///
/// Red-check: build the owner from the window's push capability alone — the
/// host hears nothing.
pub(crate) fn a_window_with_a_text_store_host_takes_input_through_it() {
    let log = Rc::new(FocusLog::default());
    let host: Rc<dyn flui_platform::TextStoreHost> = log.clone();
    let ui_runtime = UiRuntime::new(
        super::test_window().with_text_store_host(Some(host)),
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("ui_runtime");
    let (_store, client) = in_memory_client("");
    let handle = ui_runtime.text_input_handle();
    let token = handle.attach(client).expect("a pull window takes input");
    let _ = handle.detach(token).expect("detach");
    assert_eq!(*log.0.borrow(), [true, false]);
}

/// A frame whose post-frame callback queued an input method's grant and then
/// panicked, so the drive unwound past its commit anchor; returns the
/// runtime and the scheduler phases the grant ran in so far.
fn unwound_frame_with_a_queued_grant() -> (
    UiRuntime,
    Rc<InMemoryTextStore>,
    Rc<std::cell::RefCell<Vec<SchedulerPhase>>>,
) {
    let (_fake, text_input) = headless_text_input();
    let mut ui_runtime = UiRuntime::for_test_with_text_input(Some(text_input));
    let (store, client) = in_memory_client("abc");
    let _token = ui_runtime
        .text_input_handle()
        .attach(client)
        .expect("headless presentation supports text input");
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&SizedBox::square(10.0)))
        .expect("attach succeeds");
    let phases = Rc::new(std::cell::RefCell::new(Vec::new()));
    let (queued_on, seen, scheduler) = (
        Rc::clone(&store),
        Rc::clone(&phases),
        ui_runtime.scheduler().clone(),
    );
    ui_runtime
        .scheduler()
        .add_post_frame_callback(Box::new(move |_| {
            let outcome = queued_on.request_lock(
                LockGrant::read_write(move |_| seen.borrow_mut().push(scheduler.phase())),
                LockTiming::Async,
            );
            assert_eq!(outcome, Ok(LockOutcome::Deferred), "the frame queues it");
            panic!("the frame's post-frame callback panics");
        }));
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        ui_runtime.pump(
            &mut flui_foundation::ManualClock::new(),
            &mut ScriptedSink::always_presents(),
        )
    }));
    assert!(unwound.is_err(), "the post-frame panic leaves the pump");
    assert!(
        phases.borrow().is_empty(),
        "the unwound drive ran no commit anchor"
    );
    (ui_runtime, store, phases)
}

/// A grant an input method queued in a frame that unwound runs at the UI
/// runtime's next owner turn — an inbox drain, a background pump, or a pump
/// before its frame — with the scheduler in `Idle` (ADR-0142 item 5).
///
/// Red-check: drop `pay_owed_anchor` from `UiRuntime::drain_owner_inbox` —
/// the drain and background rows find the grant still queued; from
/// `UiRuntime::pump` — the pump row sees it run only after the frame.
pub(crate) fn a_grant_deferred_by_a_panicking_frame_runs_without_another_frame() {
    /// An owner turn that runs no frame of its own, or a pump.
    type Turn = fn(&mut UiRuntime);
    let turns: [(&str, Turn); 3] = [
        ("an inbox drain", |ui_runtime| {
            let _ = ui_runtime.drain_owner_inbox();
        }),
        ("a background pump", UiRuntime::pump_background),
        ("a pump, before its frame", |ui_runtime| {
            let _ = ui_runtime.pump(
                &mut flui_foundation::ManualClock::new(),
                &mut ScriptedSink::always_presents(),
            );
        }),
    ];
    let failed: Vec<&str> = turns
        .iter()
        .filter(|(_, turn)| {
            catch_unwind(AssertUnwindSafe(|| {
                let (mut ui_runtime, store, phases) = unwound_frame_with_a_queued_grant();
                turn(&mut ui_runtime);
                assert_eq!(
                    *phases.borrow(),
                    [SchedulerPhase::Idle],
                    "the owed grant ran once, outside a frame"
                );
                assert_eq!(
                    store.request_lock(LockGrant::read(|_| {}), LockTiming::Sync),
                    Ok(LockOutcome::Granted),
                    "commits are open again"
                );
                assert!(!ui_runtime.owes_anchor(), "the debt is paid");
            }))
            .is_err()
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "failing rows: {failed:?}");
}

/// While the anchor is owed, each turn asks for a wake; an anchor that
/// fails keeps the debt, and the turn after it pays it (ADR-0142 item 5).
///
/// Red-check: clear the debt whatever the anchor did in
/// `UiRuntime::run_commit_anchor` — the failed drain leaves no debt; skip
/// `wake_for_owed_anchor` — no wake is asked for.
pub(crate) fn a_failed_anchor_stays_owed_and_asks_for_a_wake() {
    let (ui_runtime, store, phases) = unwound_frame_with_a_queued_grant();
    assert!(
        ui_runtime.owes_anchor(),
        "the unwound frame owes the anchor"
    );
    assert!(
        ui_runtime.needs_redraw(),
        "the unwound pump asked for a wake"
    );
    ui_runtime.mark_rendered();
    // A grant queued behind the owed one panics when the anchor runs it: the
    // turn fails, the debt stays and the turn asks for a wake again.
    let blocked = TextCommitsHeld::shut(&ui_runtime);
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(|_| panic!("the queued grant panics")),
            LockTiming::Async,
        ),
        Ok(LockOutcome::Deferred)
    );
    drop(blocked);
    let failed = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_owner_inbox()));
    assert!(failed.is_err(), "the anchor's failure reaches the turn");
    assert!(ui_runtime.owes_anchor(), "a failed anchor keeps the debt");
    assert!(ui_runtime.needs_redraw(), "and asks for a wake again");
    let _ = ui_runtime.drain_owner_inbox();
    assert!(!ui_runtime.owes_anchor(), "the next turn pays it");
    assert_eq!(*phases.borrow(), [SchedulerPhase::Idle]);
}

/// Shuts one presentation's commits for a test's own frame-like window.
struct TextCommitsHeld<'a>(&'a UiRuntime);

impl<'a> TextCommitsHeld<'a> {
    fn shut(ui_runtime: &'a UiRuntime) -> Self {
        ui_runtime
            .presentations
            .primary()
            .text_input()
            .set_transaction_open(true);
        Self(ui_runtime)
    }
}

impl Drop for TextCommitsHeld<'_> {
    fn drop(&mut self) {
        self.0
            .presentations
            .primary()
            .text_input()
            .set_transaction_open(false);
    }
}

/// A failure the field's owner raised after a grant the platform asked for
/// directly — outside any dispatch, frame or anchor — waits in the
/// presentation's gate; the UI runtime's next owner turn reports it, once
/// (ADR-0142 items 2 and 5).
///
/// Red-check: drop the gate check from `UiRuntime::pay_owed_anchor` — the
/// drain returns normally and the failure stays parked.
pub(crate) fn a_failure_parked_by_a_platform_grant_reaches_the_next_owner_turn() {
    let (_fake, text_input) = headless_text_input();
    let ui_runtime = UiRuntime::for_test_with_text_input(Some(text_input));
    let (store, client) = in_memory_client("abc");
    let _token = ui_runtime
        .text_input_handle()
        .attach(client)
        .expect("headless presentation supports text input");
    store.set_owner_listener(Some(Rc::new(|| panic!("the field's on_changed panics"))));
    let end = flui_platform::text_store::Utf16Offset::new(3);
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(move |session| {
                let at = flui_platform::text_store::Utf16Range::try_new(end, end)
                    .expect("a caret is a range");
                session.replace(at, "d").expect("the end is a position");
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted),
        "the grant stands; its owner's failure is parked"
    );
    let reported = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_owner_inbox()))
        .expect_err("the next owner turn reports the parked failure");
    assert_eq!(
        reported.downcast_ref::<&str>(),
        Some(&"the field's on_changed panics")
    );
    assert_eq!(store.text(), "abcd", "the edit the grant made stays");
    let _ = ui_runtime.drain_owner_inbox();
}

/// What a `Listener` saw of the field's store when a press reached it.
type SeenAtPress = Rc<std::cell::RefCell<Vec<(String, bool)>>>;

/// A press anywhere in the presentation, outside the composing field,
/// commits the composition before the press's handlers run, so a "Save"
/// handler reads the text the user sees (ADR-0142 item 4, R6).
///
/// Red-check: drop the `begins_contact` commit from
/// `UiRuntime::apply_platform_input` — the handler sees the composition
/// still open.
pub(crate) fn a_press_outside_the_field_commits_the_composition_before_its_handlers() {
    let (_fake, text_input) = headless_text_input();
    let ui_runtime = UiRuntime::for_test_with_text_input(Some(text_input));
    let id = ui_runtime.presentation_id();
    let (store, client) = in_memory_client("abc");
    let _token = ui_runtime
        .text_input_handle()
        .attach(client)
        .expect("headless presentation supports text input");
    let seen: SeenAtPress = Rc::default();
    let (read, at_press) = (Rc::clone(&store), Rc::clone(&seen));
    ui_runtime
        .attach_root_widget(
            &flui_widgets::Listener::new()
                .behavior(flui_rendering::hit_testing::HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| {
                    at_press
                        .borrow_mut()
                        .push((read.text(), read.composition().is_some()));
                })
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    ui_runtime.synchronize_window_snapshot(
        id,
        flui_platform::WindowExecutionState::Running,
        true,
        true,
    );
    ui_runtime.enter(|ui_runtime| ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(
            id,
            PlatformInput::Ime(flui_platform::ImeEvent::Preedit {
                text: "とうきょう".into(),
                cursor: None,
            }),
        );
    });
    assert!(store.composition().is_some(), "the field composes");
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(
            id,
            PlatformInput::Pointer(
                flui_interaction::events::make_down_event(
                    flui_foundation::geometry::Offset::new(4.0, 6.0),
                    flui_interaction::events::PointerKind::Mouse,
                )
                .expect("finite test position"),
            ),
        );
    });
    assert_eq!(
        *seen.borrow(),
        [("abcとうきょう".to_owned(), false)],
        "the press's handler sees the committed text"
    );
}
