use flui_platform_api::text_store::{
    InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
};
use flui_types::ImeEvent;
use flui_types::geometry::Bounds;

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

fn test_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
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

/// An edit a deferred grant makes at the anchor asks for a frame the way a
/// field's rebuild does (`ensure_visual_update`), and gets one: at the
/// anchor the scheduler is `Idle` and schedules it. Inside the frame the
/// request is dropped, and the committed text would wait for an unrelated
/// wake to be painted.
///
/// Red-check: run the anchor at the end of `draw_frame_entered` instead —
/// `ensure_visual_update` returns `false` and no frame is scheduled.
#[test]
fn an_edit_made_at_the_commit_anchor_schedules_the_next_frame() {
    let accepted = Rc::new(std::cell::Cell::new(None));
    let seen = Rc::clone(&accepted);
    let (realm, _requester) = drive_one_frame_with(move |realm| {
        let scheduler = realm.scheduler().clone();
        Rc::new(move || seen.set(Some(scheduler.ensure_visual_update())))
    });

    assert_eq!(accepted.get(), Some(true), "the frame request was accepted");
    assert!(
        realm.scheduler().is_frame_scheduled(),
        "the edit's frame request outlived the frame its grant was queued in"
    );
}

/// Attach records `set_ime_allowed(true)`; preedit/commit events
/// routed through `handle_input_entered` are projected onto the
/// attached client's store; detach from the still-active token records
/// `set_ime_allowed(false)`.
#[test]
fn handle_input_entered_projects_ime_onto_the_attached_store() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));
    let handle = realm.text_input_handle();

    let (store, client) = in_memory_client("");
    let token = handle
        .attach(client)
        .expect("headless presentation supports text input");

    assert_eq!(
        fake.last_ime_allowed(),
        Some(true),
        "attach must enable platform IME composition"
    );

    realm.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Ime(ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        }));
        realm.handle_input_entered(PlatformInput::Ime(ImeEvent::Commit("你好".to_string())));
    });

    assert_eq!(
        store.text(),
        "你好",
        "the commit replaces the preedit the same realm call projected"
    );
    assert_eq!(store.composition(), None);

    assert_eq!(
        handle.detach(token).expect("presentation remains open"),
        flui_interaction::DetachOutcome::Detached
    );
    assert_eq!(
        fake.last_ime_allowed(),
        Some(false),
        "detaching the active token must disable platform IME composition"
    );
}

/// The stale-detach race named in `TextInputOwner`'s module doc:
/// field A attaches, field B attaches (replacing A), and A's
/// now-stale detach must record NOTHING on the platform side — only
/// B's later, active-token detach may disable IME.
#[test]
fn a_stale_detach_records_nothing_on_the_platform() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));
    let handle = realm.text_input_handle();

    let token_a = handle
        .attach(in_memory_client("").1)
        .expect("supported presentation");
    assert_eq!(fake.ime_allowed_calls(), vec![true]);

    let token_b = handle
        .attach(in_memory_client("").1)
        .expect("supported presentation");
    assert_eq!(
        fake.ime_allowed_calls(),
        vec![true],
        "replacement on one presentation keeps the already-enabled IME session"
    );

    assert_eq!(
        handle.detach(token_a).expect("presentation remains open"),
        flui_interaction::DetachOutcome::Stale
    );
    assert_eq!(
        fake.ime_allowed_calls(),
        vec![true],
        "a stale detach (token_a, already replaced by token_b) records nothing"
    );

    assert_eq!(
        handle.detach(token_b).expect("presentation remains open"),
        flui_interaction::DetachOutcome::Detached
    );
    assert_eq!(
        fake.ime_allowed_calls(),
        vec![true, false],
        "the active token's detach still disables IME"
    );
}

/// End-to-end proof of a claim `flui-widgets`' own
/// `editable_text::tests` cannot make on their own: a real
/// mounted `EditableText` receives the weak handle of this realm's
/// directly owned platform capability, not a mock or a stand-in.
#[test]
fn a_mounted_editable_text_toggles_platform_ime_on_focus_and_blur() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));

    let controller = flui_widgets::TextEditingController::new();
    let focus_node = flui_interaction::FocusNode::with_debug_label("app-ime-integration");
    realm
        .enter(|realm| {
            realm.attach_root_widget(&flui_widgets::EditableText::new(
                controller.clone(),
                Rc::clone(&focus_node),
            ))
        })
        .expect("attach succeeds");
    let _ = realm.draw_frame(test_constraints());

    assert!(focus_node.is_attached());
    focus_node.request_focus();
    assert!(focus_node.has_primary_focus());
    assert_eq!(
        fake.last_ime_allowed(),
        Some(true),
        "focusing a mounted EditableText must attach through its presentation \
         and enable platform IME composition"
    );

    focus_node.unfocus();
    assert_eq!(
        fake.last_ime_allowed(),
        Some(false),
        "blurring the field must detach and disable platform IME composition"
    );
}

/// `TextInputHandle::set_cursor_area` reaches this realm's exact
/// `PlatformTextInput` capability through the same `PresentationState`
/// ownership path used in production. Deliberately does not mount a
/// widget tree: this test proves the owner forwards an
/// already-computed `Bounds` to the platform, not that a real
/// `EditableText` computes the right one.
#[test]
fn set_ime_cursor_area_reaches_the_presentations_platform_capability() {
    let (fake, text_input) = headless_text_input();

    let realm = UiRealm::for_test_with_text_input(Some(Arc::clone(&text_input)));

    let area = Bounds::new(
        flui_types::Point::new(px(10.0), px(20.0)),
        flui_types::Size::new(px(2.0), px(18.0)),
    );
    realm
        .text_input_handle()
        .set_cursor_area(area)
        .expect("headless presentation supports text input");

    assert_eq!(
        fake.cursor_area_calls(),
        vec![area],
        "set_ime_cursor_area must call through to the presentation-owned \
         PlatformTextInput capability with the exact area"
    );
}

/// A presentation without IME support reports a typed error.
#[test]
fn set_ime_cursor_area_without_platform_support_is_typed() {
    let realm = UiRealm::for_test();
    assert_eq!(
        realm.text_input_handle().set_cursor_area(Bounds::new(
            flui_types::Point::new(px(0.0), px(0.0)),
            flui_types::Size::new(px(1.0), px(1.0)),
        )),
        Err(flui_interaction::TextInputError::Unsupported)
    );
}
