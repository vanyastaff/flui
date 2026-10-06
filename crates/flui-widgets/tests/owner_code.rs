//! Owner code at every point a text store, its arbiter and its presentation
//! run code they do not control, all contained by
//! `flui_platform_api::text_store::OwnerCalls` (ADR-0090 amendment, "Owner
//! code"; its module doc lists the points).
//!
//! One row per point and failure shape: a panic in the owner code, a
//! snapshot whose last owner the code released and whose captured value
//! panics when destroyed, and an ownership change (moving the store, a
//! rebuild, a detach, a cleared observer, a close) followed by a panic. A
//! pull host's calls are platform code that reaches application code, so
//! they are points here too. Each row then
//! performs the next operation on the same owner and checks it succeeds.
//! A row can abort the process when its point is not contained, so each runs
//! in a child process.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_interaction::routing::FocusNode;
use flui_interaction::{TextInputBackend, TextInputClient, TextInputOwner};
use flui_platform_api::text_store::{
    CommitGate, CompositionEnd, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextChange,
    TextStore, TextStoreError, TextStoreHost, TextStoreHostError, TextStoreObserver,
    TextStoreStatus, project_ime_event,
};
use flui_platform_api::{ImeEvent, PlatformTextInput};
use flui_widgets::{EditableText, TextEditingController};

use crate::common::child_process;
use crate::common::harness::{Harness, mount_with_ime};

/// A captured value whose destruction panics.
struct PanicsOnDrop(&'static str);

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        panic!("{}", self.0);
    }
}

/// The text of the panic `run` raised, if it raised one.
fn raised(run: impl FnOnce()) -> Option<String> {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).err()?;
    Some(text_of(payload))
}

fn text_of(payload: Box<dyn std::any::Any + Send>) -> String {
    let text = flui_foundation::panic::payload_text(&*payload)
        .unwrap_or("an opaque payload")
        .to_owned();
    flui_foundation::panic::retain_opaque_payload(payload);
    text
}

fn parked(gate: &CommitGate) -> Option<String> {
    gate.take_failure().map(text_of)
}

fn insert(text: &'static str) -> LockGrant {
    LockGrant::read_write(move |session| {
        session.insert_at_selection(text).expect("in range");
    })
}

fn edit(store: &dyn TextStore, text: &'static str) -> Result<LockOutcome, TextStoreError> {
    store.request_lock(insert(text), LockTiming::Sync)
}

// ----------------------------------------------------------------------------
// The arbiter: the gate a settle failure belongs to
// ----------------------------------------------------------------------------

/// An in-memory store behind `admitting`, whose owner listener panics.
fn store_behind(admitting: &CommitGate, listener: impl Fn() + 'static) -> Rc<InMemoryTextStore> {
    let store = InMemoryTextStore::new("");
    store.set_commit_gate(admitting.clone());
    store.set_owner_listener(Some(Rc::new(listener)));
    store
}

fn the_next_edit_runs_behind(store: &Rc<InMemoryTextStore>, gate: &CommitGate) {
    store.set_owner_listener(None);
    assert_eq!(
        edit(&**store, "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(store.text(), "ab");
    assert_eq!(parked(gate), None, "the next edit fails nothing");
}

fn gate_a_grant_that_moves_the_store() {
    let (admitting, next) = (CommitGate::new(), CommitGate::new());
    let store = store_behind(&admitting, || panic!("owner failure"));
    let (weak, moved_to) = (Rc::downgrade(&store), next.clone());
    let outcome = store.request_lock(
        LockGrant::read_write(move |session| {
            session.insert_at_selection("a").expect("in range");
            if let Some(store) = weak.upgrade() {
                store.set_commit_gate(moved_to);
            }
        }),
        LockTiming::Sync,
    );
    assert_eq!(outcome, Ok(LockOutcome::Granted));
    assert_eq!(
        parked(&next),
        None,
        "nothing waits at the gate the grant moved to"
    );
    assert_eq!(
        parked(&admitting).as_deref(),
        Some("owner failure"),
        "the failure waits at the gate that admitted the grant"
    );
    the_next_edit_runs_behind(&store, &next);
}

fn gate_a_settle_that_moves_the_store() {
    let (admitting, next) = (CommitGate::new(), CommitGate::new());
    let moved_to = next.clone();
    let slot: Rc<RefCell<Weak<InMemoryTextStore>>> = Rc::new(RefCell::new(Weak::new()));
    let reach = Rc::clone(&slot);
    let store = store_behind(&admitting, move || {
        if let Some(store) = reach.borrow().upgrade() {
            store.set_commit_gate(moved_to.clone());
        }
        panic!("owner failure after moving");
    });
    *slot.borrow_mut() = Rc::downgrade(&store);
    assert_eq!(edit(&*store, "a"), Ok(LockOutcome::Granted));
    assert_eq!(
        parked(&next),
        None,
        "nothing waits at the gate settle moved to"
    );
    assert_eq!(
        parked(&admitting).as_deref(),
        Some("owner failure after moving"),
        "the failure waits at the gate that admitted the grant"
    );
    the_next_edit_runs_behind(&store, &next);
}

// ----------------------------------------------------------------------------
// The in-memory store's owner listener
// ----------------------------------------------------------------------------

/// A listener holding a capture that panics when destroyed, which first
/// replaces itself with `replacement` (`None` removes it) and then panics.
fn listener_replacing_itself(
    replacement: Option<Rc<dyn Fn()>>,
) -> (Rc<InMemoryTextStore>, CommitGate) {
    let gate = CommitGate::new();
    let slot: Rc<RefCell<Weak<InMemoryTextStore>>> = Rc::new(RefCell::new(Weak::new()));
    let reach = Rc::clone(&slot);
    let capture = PanicsOnDrop("listener capture destroyed");
    let store = store_behind(&gate, move || {
        let _keep_alive = &capture;
        if let Some(store) = reach.borrow().upgrade() {
            store.set_owner_listener(replacement.clone());
        }
        panic!("listener failure");
    });
    *slot.borrow_mut() = Rc::downgrade(&store);
    assert_eq!(edit(&*store, "a"), Ok(LockOutcome::Granted));
    assert_eq!(
        parked(&gate).as_deref(),
        Some("listener failure"),
        "the listener's failure, not its capture's"
    );
    (store, gate)
}

fn in_memory_listener_removed_then_panicking() {
    let (store, gate) = listener_replacing_itself(None);
    the_next_edit_runs_behind(&store, &gate);
}

fn in_memory_listener_replaced_then_panicking() {
    let heard = Rc::new(Cell::new(0));
    let counted = Rc::clone(&heard);
    let (store, gate) =
        listener_replacing_itself(Some(Rc::new(move || counted.set(counted.get() + 1))));
    assert_eq!(
        edit(&*store, "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(heard.get(), 1, "the replacement hears the next session");
    assert_eq!(parked(&gate), None);
}

// ----------------------------------------------------------------------------
// EditableText: on_changed, the controller's listeners and the observer
// ----------------------------------------------------------------------------

fn focused(view: EditableText, node: &Rc<FocusNode>) -> Harness {
    let mut harness = mount_with_ime(view);
    node.request_focus();
    harness.tick();
    harness
}

fn field(harness: &Harness) -> Rc<dyn TextStore> {
    harness
        .active_text_store()
        .expect("the focused field is the active IME client")
}

/// Logs what the platform heard and the grants that ran, in order.
struct Logged(Rc<RefCell<Vec<&'static str>>>);

impl TextStoreObserver for Logged {
    fn text_changed(&self, _: TextChange) {
        self.0.borrow_mut().push("platform heard the owner's edit");
    }
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

/// Queue an edit and a logging grant inside a frame, so one anchor runs
/// them back to back, and check the platform heard what the owner code
/// edited before the second grant ran, though that code panicked.
fn the_owner_edit_reaches_the_platform_first(
    harness: &mut Harness,
    field: &Rc<dyn TextStore>,
    failure: &str,
) {
    let log = Rc::new(RefCell::new(Vec::new()));
    field.set_observer(Some(Rc::new(Logged(Rc::clone(&log)))));
    let (queued, second) = (Rc::clone(field), Rc::clone(&log));
    harness
        .local_post_frame_handle()
        .schedule_local(move |_| {
            for grant in [
                insert("a"),
                LockGrant::read_write(move |_| second.borrow_mut().push("second grant")),
            ] {
                assert_eq!(
                    queued.request_lock(grant, LockTiming::Async),
                    Ok(LockOutcome::Deferred)
                );
            }
        })
        .expect("post-frame handle installed");
    assert_eq!(raised(|| harness.tick()).as_deref(), Some(failure));
    field.set_observer(None);
    assert_eq!(
        *log.borrow(),
        ["platform heard the owner's edit", "second grant"],
        "the owner's edit is reported before the next grant"
    );
}

fn the_field_keeps_working(harness: &mut Harness, field: &Rc<dyn TextStore>) {
    assert_eq!(raised(|| harness.tick()), None, "nothing is reported twice");
    assert_eq!(
        edit(&**field, "z"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
}

fn on_changed_editing_then_panicking() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("on_changed panics");
    let app = controller.clone();
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, text| {
            if text == "a" {
                app.set_text("app");
                panic!("on_changed failure");
            }
        }),
        &node,
    );
    let field = field(&harness);
    the_owner_edit_reaches_the_platform_first(&mut harness, &field, "on_changed failure");
    the_field_keeps_working(&mut harness, &field);
    assert_eq!(controller.text(), "appz");
}

fn on_changed_rebuilt_away_then_panicking() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("on_changed rebuilt away");
    let harness: Rc<RefCell<Option<Harness>>> = Rc::new(RefCell::new(None));
    let (nested, rebuilt, rebuilt_node) = (
        Rc::downgrade(&harness),
        controller.clone(),
        Rc::clone(&node),
    );
    let capture = PanicsOnDrop("on_changed capture destroyed");
    let view = EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, _| {
        let _keep_alive = &capture;
        if let Some(harness) = nested.upgrade() {
            // The rebuild drops the field's own `on_changed`.
            harness
                .borrow_mut()
                .as_mut()
                .expect("mounted")
                .swap_root(EditableText::new(rebuilt.clone(), Rc::clone(&rebuilt_node)));
            panic!("on_changed failure");
        }
    });
    *harness.borrow_mut() = Some(focused(view, &node));
    let field = field(harness.borrow().as_ref().expect("mounted"));
    assert_eq!(edit(&*field, "a"), Ok(LockOutcome::Granted));
    let mut harness = harness.borrow_mut().take().expect("mounted");
    assert_eq!(
        raised(|| harness.tick()).as_deref(),
        Some("on_changed failure"),
        "the callback's failure, not its capture's"
    );
    the_field_keeps_working(&mut harness, &field);
    assert_eq!(controller.text(), "az");
}

/// The controller's listeners are a `ChangeNotifier`'s, which contains and
/// reports each listener's panic itself (ADR-0104), so their point has no
/// panic row; the store's part there is the obligation it reads first.
fn controller_listener_session_is_its_own_on_changed() {
    crate::editable_text::text_store::a_listener_session_inside_settle_is_its_own_on_changed();
}

/// An observer that, on a text change, may clear itself (then holding a
/// capture that panics when destroyed), then panics; it counts selection
/// changes.
struct Hostile {
    store: RefCell<Weak<dyn TextStore>>,
    clears_itself: bool,
    selections: Rc<Cell<usize>>,
    _capture: Option<PanicsOnDrop>,
}

impl TextStoreObserver for Hostile {
    fn text_changed(&self, _: TextChange) {
        if self.clears_itself
            && let Some(store) = self.store.borrow().upgrade()
        {
            store.set_observer(None);
        }
        panic!("observer failure");
    }
    fn selection_changed(&self) {
        self.selections.set(self.selections.get() + 1);
    }
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

fn observer_panicking(clears_itself: bool) {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("hostile observer");
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)),
        &node,
    );
    let field = field(&harness);
    let selections = Rc::new(Cell::new(0));
    field.set_observer(Some(Rc::new(Hostile {
        store: RefCell::new(Rc::downgrade(&field)),
        clears_itself,
        selections: Rc::clone(&selections),
        _capture: clears_itself.then(|| PanicsOnDrop("observer capture destroyed")),
    })));
    controller.set_text("app");
    // The store reports the application's edit before it grants a lock.
    assert_eq!(
        raised(|| {
            let _ = field.request_lock(LockGrant::read(|_| {}), LockTiming::Sync);
        })
        .as_deref(),
        Some("observer failure"),
        "the observer's failure, not its capture's"
    );
    if !clears_itself {
        assert_eq!(
            selections.get(),
            1,
            "the rest of the notifications were sent"
        );
        field.set_observer(None);
    }
    the_field_keeps_working(&mut harness, &field);
}

fn observer_panicking_still_hears_the_rest() {
    observer_panicking(false);
}

fn observer_cleared_then_panicking() {
    observer_panicking(true);
}

// ----------------------------------------------------------------------------
// TextInputOwner: the session-start callback and the dispatched client
// ----------------------------------------------------------------------------

struct Platform;

impl PlatformTextInput for Platform {
    fn set_ime_allowed(&self, _: bool) {}
    fn set_ime_cursor_area(&self, _: Bounds<f64>) {}
}

fn owner() -> Rc<TextInputOwner> {
    TextInputOwner::new(TextInputBackend::Push(Arc::new(Platform)))
}

/// A store that runs `on_drop` when destroyed.
struct DropHook {
    inner: Rc<InMemoryTextStore>,
    on_drop: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl TextStore for DropHook {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        self.inner.request_lock(grant, timing)
    }
    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }
    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

impl Drop for DropHook {
    fn drop(&mut self) {
        if let Some(on_drop) = self.on_drop.get_mut().take() {
            on_drop();
        }
    }
}

/// Park `failure` in `owner`'s gate through a direct grant whose owner
/// listener panics, outside any dispatch.
fn park_through(store: &Rc<InMemoryTextStore>, failure: &'static str) {
    store.set_owner_listener(Some(Rc::new(move || panic!("{failure}"))));
    assert_eq!(
        edit(&**store, "a"),
        Ok(LockOutcome::Granted),
        "the grant stands"
    );
    store.set_owner_listener(None);
}

fn the_owner_keeps_working(owner: &Rc<TextInputOwner>) {
    let store = InMemoryTextStore::new("");
    let _next = owner
        .handle()
        .attach(TextInputClient::new(store.clone()))
        .expect("the next attach");
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Commit("x".into()))),
        None,
        "the next dispatch"
    );
    assert_eq!(store.text(), "x");
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        }),
        None
    );
}

fn session_start_panicking_after_a_parked_failure() {
    let owner = owner();
    let store = InMemoryTextStore::new("");
    let _client = owner
        .handle()
        .attach(
            TextInputClient::new(store.clone())
                .on_session_start(|| panic!("session start failure")),
        )
        .expect("attach");
    park_through(&store, "parked owner failure");
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Enabled)).as_deref(),
        Some("parked owner failure"),
        "the earlier failure is authoritative"
    );
    the_owner_keeps_working(&owner);
}

fn session_start_parking_then_panicking() {
    let owner = owner();
    let store = InMemoryTextStore::new("");
    store.set_owner_listener(Some(Rc::new(|| panic!("parked by session start"))));
    let edited = Rc::clone(&store);
    let _client = owner
        .handle()
        .attach(
            TextInputClient::new(store.clone()).on_session_start(move || {
                assert_eq!(edit(&*edited, "a"), Ok(LockOutcome::Granted));
                panic!("session start failure");
            }),
        )
        .expect("attach");
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Enabled)).as_deref(),
        Some("parked by session start"),
        "the failure parked inside the callback came before the callback's own"
    );
    store.set_owner_listener(None);
    assert_eq!(store.text(), "a", "the callback's grant stands");
    the_owner_keeps_working(&owner);
}

fn session_start_detaching_then_panicking() {
    let owner = owner();
    let handle = owner.handle();
    let token = Rc::new(Cell::new(None));
    let (own, detaching) = (Rc::clone(&token), handle.clone());
    let capture = PanicsOnDrop("session callback capture destroyed");
    token.set(Some(
        handle
            .attach(
                TextInputClient::new(InMemoryTextStore::new("")).on_session_start(move || {
                    let _keep_alive = &capture;
                    if let Some(token) = own.get() {
                        let _ = detaching.detach(token);
                    }
                    panic!("session start failure");
                }),
            )
            .expect("attach"),
    ));
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Enabled)).as_deref(),
        Some("session start failure"),
        "the callback's failure, not its capture's"
    );
    the_owner_keeps_working(&owner);
}

fn dispatch_without_a_client_reports_a_parked_failure() {
    let owner = owner();
    let store = InMemoryTextStore::new("");
    let token = owner
        .handle()
        .attach(TextInputClient::new(store.clone()))
        .expect("attach");
    park_through(&store, "parked owner failure");
    let _detached = owner.handle().detach(token).expect("detach");
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Commit("b".into()))).as_deref(),
        Some("parked owner failure"),
        "a dispatch that finds no client still reports what waited for it"
    );
    the_owner_keeps_working(&owner);
}

/// A client whose store's owner listener detaches it on the first session
/// (and fails when `listener_fails`), so the dispatch's clone retires it; its
/// destruction then runs `on_drop`.
fn client_detached_by_its_listener(
    owner: &Rc<TextInputOwner>,
    listener_fails: bool,
    on_drop: impl FnOnce(&Rc<InMemoryTextStore>) + 'static,
) {
    let inner = InMemoryTextStore::new("");
    let handle = owner.handle();
    let token = Rc::new(Cell::new(None));
    let own = Rc::clone(&token);
    inner.set_owner_listener(Some(Rc::new(move || {
        if let Some(token) = own.take() {
            let _ = handle.detach(token);
            assert!(!listener_fails, "listener failure");
        }
    })));
    let dropped = Rc::clone(&inner);
    let store = Rc::new(DropHook {
        inner,
        on_drop: RefCell::new(Some(Box::new(move || on_drop(&dropped)))),
    });
    token.set(Some(
        owner
            .handle()
            .attach(TextInputClient::new(store))
            .expect("attach"),
    ));
}

fn dispatched_client_retirement_parking_a_failure() {
    let owner = owner();
    client_detached_by_its_listener(&owner, false, |inner: &Rc<InMemoryTextStore>| {
        park_through(inner, "parked by retirement");
    });
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Commit("a".into()))).as_deref(),
        Some("parked by retirement"),
        "the dispatch reports what retiring its client parked"
    );
    the_owner_keeps_working(&owner);
}

fn dispatched_client_retirement_after_a_failure() {
    let owner = owner();
    client_detached_by_its_listener(&owner, true, |_: &Rc<InMemoryTextStore>| {
        panic!("store destroyed after the failure")
    });
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Commit("a".into()))).as_deref(),
        Some("listener failure"),
        "the client is retained, not destroyed, after the failure"
    );
    the_owner_keeps_working(&owner);
}

// ----------------------------------------------------------------------------
// TextInputOwner on a pull host: the host's calls from the owner's queue
// ----------------------------------------------------------------------------

type Log = Rc<RefCell<Vec<&'static str>>>;

/// What a row runs inside the host's focus call, given the focused store.
type InsideFocus = Box<dyn FnOnce(&Rc<dyn TextStore>)>;

/// A pull host that logs its calls, runs what a row hands it inside them (as
/// a text service reaching application code does), then panics when told.
/// It abandons every composition it is asked to end, so the owner commits
/// in place whenever a completion returns.
#[derive(Default)]
struct Host {
    log: Log,
    inside_focus: RefCell<Option<InsideFocus>>,
    inside_complete: RefCell<Option<Box<dyn FnOnce()>>>,
    /// Calls (`"focus"`, `"complete"`) that panic once each, after their
    /// effect.
    panics: RefCell<Vec<&'static str>>,
    _capture: Option<PanicsOnDrop>,
}

impl Host {
    fn panic_if_told(&self, call: &'static str) {
        let told = {
            let mut panics = self.panics.borrow_mut();
            let position = panics.iter().position(|&told| told == call);
            position.map(|position| panics.remove(position))
        };
        if let Some(call) = told {
            panic!("{call} failure");
        }
    }
}

impl TextStoreHost for Host {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        self.log
            .borrow_mut()
            .push(if store.is_some() { "focus" } else { "unfocus" });
        let inside = self.inside_focus.borrow_mut().take();
        if let (Some(inside), Some(store)) = (inside, &store) {
            inside(store);
        }
        self.panic_if_told("focus");
    }

    fn complete_composition(
        &self,
        _: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        self.log.borrow_mut().push("complete");
        let inside = self.inside_complete.borrow_mut().take();
        if let Some(inside) = inside {
            inside();
        }
        self.panic_if_told("complete");
        Ok(CompositionEnd::Abandoned)
    }
}

fn pull_owner(host: Rc<Host>) -> Rc<TextInputOwner> {
    TextInputOwner::new(TextInputBackend::Pull(host))
}

/// Type "かな" after "ab", leaving it composing.
fn composing_store() -> Rc<InMemoryTextStore> {
    let store = InMemoryTextStore::new("ab");
    store.set_commit_gate(CommitGate::new());
    let applied = project_ime_event(
        &*store,
        &ImeEvent::Preedit {
            text: "かな".to_owned(),
            cursor: Some((0, 0)),
        },
    );
    assert_eq!(applied, Ok(LockOutcome::Granted), "preedit applies");
    store
}

fn the_pull_owner_keeps_working(owner: &Rc<TextInputOwner>, log: &Log) {
    log.borrow_mut().clear();
    let store = composing_store();
    let token = owner
        .handle()
        .attach(TextInputClient::new(store.clone()))
        .expect("the next attach");
    assert_eq!(
        raised(|| owner.complete_composition()),
        None,
        "the next completion"
    );
    assert!(store.composition().is_none(), "the next completion commits");
    let _ = owner.handle().detach(token).expect("the next detach");
    assert_eq!(*log.borrow(), ["focus", "complete", "unfocus"]);
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        }),
        None,
        "nothing is reported twice"
    );
}

/// Queue a focus change and a completion inside a frame, with a deferred
/// grant behind them, and fail the host calls `panics` names at the anchor:
/// `failure` is reported, the queue behind it and the grant still run.
fn host_operations_panicking_at_the_anchor(panics: &[&'static str], failure: &str) {
    let host = Rc::new(Host::default());
    let log = Rc::clone(&host.log);
    let owner = pull_owner(Rc::clone(&host));
    let store = composing_store();
    owner.set_transaction_open(true);
    let _token = owner
        .handle()
        .attach(TextInputClient::new(store.clone()))
        .expect("attach in a frame");
    owner.complete_composition();
    let granted = Rc::new(Cell::new(false));
    let grant = Rc::clone(&granted);
    assert_eq!(
        store.request_lock(LockGrant::read(move |_| grant.set(true)), LockTiming::Async),
        Ok(LockOutcome::Deferred)
    );
    host.panics.borrow_mut().extend(panics);
    owner.set_transaction_open(false);
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        })
        .as_deref(),
        Some(failure),
        "the first failure is authoritative"
    );
    assert_eq!(
        *log.borrow(),
        ["focus", "complete"],
        "the completion behind the failure ran"
    );
    assert_eq!(
        store.composition().is_some(),
        panics.contains(&"complete"),
        "a completion with no answer is not committed in its place"
    );
    assert!(granted.get(), "the deferred grant ran");
    the_pull_owner_keeps_working(&owner, &log);
}

fn host_focus_panicking_the_queue_behind_it_runs() {
    host_operations_panicking_at_the_anchor(&["focus"], "focus failure");
}

fn host_focus_and_completion_panicking_at_the_anchor() {
    host_operations_panicking_at_the_anchor(&["focus", "complete"], "focus failure");
}

fn host_panicking_after_a_parked_failure() {
    let host = Rc::new(Host::default());
    let log = Rc::clone(&host.log);
    let owner = pull_owner(Rc::clone(&host));
    let store = InMemoryTextStore::new("");
    let _token = owner
        .handle()
        .attach(TextInputClient::new(store.clone()))
        .expect("attach");
    park_through(&store, "parked owner failure");
    host.panics.borrow_mut().push("complete");
    assert_eq!(
        raised(|| owner.complete_composition()).as_deref(),
        Some("parked owner failure"),
        "the failure parked before the host call is authoritative"
    );
    the_pull_owner_keeps_working(&owner, &log);
}

fn host_parking_then_panicking() {
    let host = Rc::new(Host::default());
    let log = Rc::clone(&host.log);
    let owner = pull_owner(Rc::clone(&host));
    let store = InMemoryTextStore::new("");
    store.set_owner_listener(Some(Rc::new(|| panic!("parked by the host"))));
    *host.inside_focus.borrow_mut() = Some(Box::new(|store: &Rc<dyn TextStore>| {
        assert_eq!(edit(&**store, "a"), Ok(LockOutcome::Granted));
    }));
    host.panics.borrow_mut().push("focus");
    assert_eq!(
        raised(|| {
            let _ = owner.handle().attach(TextInputClient::new(store.clone()));
        })
        .as_deref(),
        Some("parked by the host"),
        "the failure parked inside the host call came before the call's own"
    );
    store.set_owner_listener(None);
    assert_eq!(store.text(), "a", "the host's grant stands");
    the_pull_owner_keeps_working(&owner, &log);
}

fn host_detaching_then_panicking() {
    let host = Rc::new(Host::default());
    let log = Rc::clone(&host.log);
    let owner = pull_owner(Rc::clone(&host));
    let store = Rc::new(DropHook {
        inner: InMemoryTextStore::new(""),
        on_drop: RefCell::new(Some(Box::new(|| {
            panic!("store destroyed after the failure")
        }))),
    });
    let token = owner
        .handle()
        .attach(TextInputClient::new(store))
        .expect("attach");
    let handle = owner.handle();
    *host.inside_complete.borrow_mut() = Some(Box::new(move || {
        let _ = handle.detach(token);
    }));
    host.panics.borrow_mut().push("complete");
    assert_eq!(
        raised(|| owner.complete_composition()).as_deref(),
        Some("complete failure"),
        "the completed store is retained, not destroyed, after the failure"
    );
    assert_eq!(
        *log.borrow(),
        ["focus", "complete", "unfocus"],
        "the detach reached the host once its call returned"
    );
    the_pull_owner_keeps_working(&owner, &log);
}

fn host_closing_then_panicking_released_last() {
    let host = Rc::new(Host {
        _capture: Some(PanicsOnDrop("host capture destroyed")),
        ..Host::default()
    });
    let log = Rc::clone(&host.log);
    let owner = pull_owner(Rc::clone(&host));
    let _token = owner
        .handle()
        .attach(TextInputClient::new(InMemoryTextStore::new("")))
        .expect("attach");
    let closing = Rc::downgrade(&owner);
    *host.inside_complete.borrow_mut() = Some(Box::new(move || {
        if let Some(owner) = closing.upgrade() {
            owner.close();
        }
    }));
    host.panics.borrow_mut().push("complete");
    // The owner's clone becomes the host's last owner once the close
    // releases the backend.
    drop(host);
    assert_eq!(
        raised(|| owner.complete_composition()).as_deref(),
        Some("complete failure"),
        "the host call's failure, not its capture's"
    );
    assert_eq!(*log.borrow(), ["focus", "complete", "unfocus"]);
    assert_eq!(
        owner
            .handle()
            .attach(TextInputClient::new(InMemoryTextStore::new("")))
            .err(),
        Some(flui_interaction::TextInputError::Closed),
        "the next operation sees the close"
    );
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        }),
        None
    );
}

// ----------------------------------------------------------------------------
// The matrix
// ----------------------------------------------------------------------------

const ROWS: &[(&str, fn())] = &[
    (
        "gate: a grant that moves the store",
        gate_a_grant_that_moves_the_store,
    ),
    (
        "gate: a settle that moves the store",
        gate_a_settle_that_moves_the_store,
    ),
    (
        "in-memory listener: removed, then panicking",
        in_memory_listener_removed_then_panicking,
    ),
    (
        "in-memory listener: replaced, then panicking",
        in_memory_listener_replaced_then_panicking,
    ),
    (
        "on_changed: editing, then panicking",
        on_changed_editing_then_panicking,
    ),
    (
        "on_changed: rebuilt away, then panicking",
        on_changed_rebuilt_away_then_panicking,
    ),
    (
        "controller listener: a session of its own",
        controller_listener_session_is_its_own_on_changed,
    ),
    (
        "observer: panicking",
        observer_panicking_still_hears_the_rest,
    ),
    (
        "observer: cleared, then panicking",
        observer_cleared_then_panicking,
    ),
    (
        "session start: panicking after a parked failure",
        session_start_panicking_after_a_parked_failure,
    ),
    (
        "session start: parking, then panicking",
        session_start_parking_then_panicking,
    ),
    (
        "session start: detaching, then panicking",
        session_start_detaching_then_panicking,
    ),
    (
        "dispatch: no client, a parked failure",
        dispatch_without_a_client_reports_a_parked_failure,
    ),
    (
        "dispatched client: retirement parking a failure",
        dispatched_client_retirement_parking_a_failure,
    ),
    (
        "dispatched client: retirement after a failure",
        dispatched_client_retirement_after_a_failure,
    ),
    (
        "host: a panicking focus change, the queue behind it runs",
        host_focus_panicking_the_queue_behind_it_runs,
    ),
    (
        "host: focus change and completion panicking at the anchor",
        host_focus_and_completion_panicking_at_the_anchor,
    ),
    (
        "host: panicking after a parked failure",
        host_panicking_after_a_parked_failure,
    ),
    ("host: parking, then panicking", host_parking_then_panicking),
    (
        "host: detaching, then panicking",
        host_detaching_then_panicking,
    ),
    (
        "host: closing, then panicking, released last",
        host_closing_then_panicking_released_last,
    ),
];

#[test]
fn owner_code_is_contained_at_every_point() {
    if let Some(selected) = child_process::selected_case() {
        let (_, row) = ROWS
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("a known row");
        row();
        child_process::pass();
    }
    child_process::run_rows(
        "owner_code::owner_code_is_contained_at_every_point",
        &ROWS.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
    );
}
