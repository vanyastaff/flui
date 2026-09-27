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

/// An in-memory store whose commit gate is the presentation's, read through
/// its handle the way `EditableText`'s store reads it.
struct GatedStore {
    inner: Rc<InMemoryTextStore>,
    handle: flui_interaction::TextInputHandle,
}

impl GatedStore {
    fn gate(&self) {
        self.inner
            .set_commits_allowed(self.handle.may_commit().unwrap_or(false));
    }
}

impl TextStore for GatedStore {
    fn status(&self) -> flui_platform_api::text_store::TextStoreStatus {
        self.inner.status()
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, flui_platform_api::text_store::TextStoreError> {
        self.gate();
        self.inner.request_lock(grant, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.gate();
        self.inner.run_deferred_grants()
    }

    fn set_observer(
        &self,
        observer: Option<Rc<dyn flui_platform_api::text_store::TextStoreObserver>>,
    ) {
        self.inner.set_observer(observer);
    }
}

/// Asks `store` for an async read lock on every build, logging around it.
#[derive(Clone)]
struct LockRequester {
    store: Rc<dyn TextStore>,
    log: Rc<std::cell::RefCell<Vec<&'static str>>>,
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
        let log = Rc::clone(&self.log);
        let outcome = self.store.request_lock(
            LockGrant::read(move |_| log.borrow_mut().push("grant")),
            LockTiming::Async,
        );
        self.outcomes.borrow_mut().push(outcome);
        self.log.borrow_mut().push("build ends");
        SizedBox::square(10.0)
    }
}

/// A lock an input method asks for while the realm draws a frame waits for
/// the frame to return (ADR-0027 §3), then runs once.
///
/// Red-check: drop the `TextCommitsClosed::close` guard from
/// `draw_frame_entered` — the grant runs inside `build` and the outcome is
/// `Granted`.
#[test]
fn a_text_store_lock_requested_during_draw_frame_is_granted_after_it_returns() {
    let (_fake, text_input) = headless_text_input();
    let realm = UiRealm::for_test_with_text_input(Some(text_input));
    let handle = realm.text_input_handle();
    let store: Rc<dyn TextStore> = Rc::new(GatedStore {
        inner: InMemoryTextStore::new("abc"),
        handle: handle.clone(),
    }); // the presentation holds the gated test store through the erased contract.
    let _token = handle
        .attach(flui_interaction::TextInputClient::new(Rc::clone(&store)))
        .expect("headless presentation supports text input");
    let requester = LockRequester {
        store,
        log: Rc::default(),
        outcomes: Rc::default(),
    };

    realm
        .enter(|realm| realm.attach_root_widget(&requester))
        .expect("attach succeeds");
    assert!(
        requester.log.borrow().is_empty(),
        "attaching schedules the build; the frame runs it"
    );
    let _ = realm.draw_frame(test_constraints());

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
    assert_eq!(handle.may_commit(), Ok(true), "the frame reopened commits");
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
