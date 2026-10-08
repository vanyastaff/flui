use flui_platform_api::text_store::{
    InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
};

use super::*;

/// A concrete headless recorder and the same value viewed through the
/// platform capability supplied to the presentation.
fn headless_text_input() -> (
    Arc<flui_platform::FakeTextInput>,
    Arc<dyn flui_platform::traits::PlatformTextInput>,
) {
    let fake = Arc::new(flui_platform::FakeTextInput::new());
    let capability: Arc<dyn flui_platform::traits::PlatformTextInput> = fake.clone();
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

impl flui_platform_api::TextStoreHost for FocusLog {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        self.0.borrow_mut().push(store.is_some());
    }

    fn complete_composition(
        &self,
        _: &Rc<dyn TextStore>,
    ) -> Result<
        flui_platform_api::text_store::CompositionEnd,
        flui_platform_api::text_store::TextStoreHostError,
    > {
        Ok(flui_platform_api::text_store::CompositionEnd::Committed)
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
    let host: Rc<dyn flui_platform_api::TextStoreHost> = log.clone();
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
