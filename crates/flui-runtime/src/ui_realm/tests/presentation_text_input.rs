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

/// A realm with a text-input presentation and a [`LockRequester`] as its
/// root, after one frame driven the way the runners drive one
/// (`UiRealm::pump`).
fn drive_one_frame_with(
    on_grant: impl FnOnce(&UiRealm) -> Rc<dyn Fn()>,
) -> (UiRealm, LockRequester) {
    let (_fake, text_input) = headless_text_input();
    let mut realm = UiRealm::for_test_with_text_input(Some(text_input));
    let (concrete, client) = in_memory_client("abc");
    let store: Rc<dyn TextStore> = concrete; // the requester asks through the erased contract, as an input method does.
    let _token = realm
        .text_input_handle()
        .attach(client)
        .expect("headless presentation supports text input");
    let requester = LockRequester {
        store,
        scheduler: realm.scheduler().clone(),
        on_grant: on_grant(&realm),
        log: Rc::default(),
        phases: Rc::default(),
        outcomes: Rc::default(),
    };
    realm
        .enter(|realm| realm.attach_root_widget(&requester))
        .expect("attach succeeds");
    assert!(
        requester.log.borrow().is_empty(),
        "attaching schedules the build; the frame runs it"
    );
    let _ = realm.pump(
        &mut flui_foundation::ManualClock::new(),
        &mut ScriptedSink::always_presents(),
    );
    (realm, requester)
}

/// A lock an input method asks for while the realm drives a frame waits
/// for the whole drive to return (ADR-0027 §3), then runs once, with the
/// scheduler back in `Idle`.
///
/// Red-check: drop the `TextCommitsClosed::close` guard from
/// `UiRealm::drive_frame`, the drive `UiRealm::pump` runs its frame in — the
/// grant runs inside `build` and the outcome is `Granted`; run the anchor
/// inside the frame — the grant's phase is not `Idle`; have the pump call the
/// scheduler's drive itself — the outcome is `Granted` again.
#[test]
fn a_text_store_lock_requested_during_a_frame_is_granted_after_the_drive_returns() {
    let (_realm, requester) = drive_one_frame_with(|_| Rc::new(|| {}));

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
