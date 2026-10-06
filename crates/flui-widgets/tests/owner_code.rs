//! Owner code at every point a text store, its arbiter and its presentation
//! run code they do not control, all contained by
//! `flui_platform_api::text_store::OwnerCalls` (ADR-0090 amendment, "Owner
//! code"; its module doc lists the points).
//!
//! One row per point and failure shape: a panic in the owner code, a
//! snapshot whose last owner the code released and whose captured value
//! panics when destroyed, and an ownership change (moving the store, a
//! rebuild, a detach, a cleared observer) followed by a panic. Each row then
//! performs the next operation on the same owner and checks it succeeds.
//! A row can abort the process when its point is not contained, so each runs
//! in a child process.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_interaction::routing::FocusNode;
use flui_interaction::{TextInputClient, TextInputOwner};
use flui_platform_api::text_store::{
    CommitGate, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextChange, TextStore,
    TextStoreError, TextStoreObserver, TextStoreStatus,
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

fn focused(view: impl flui_view::View, node: &Rc<FocusNode>) -> Harness {
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

/// Counts text changes; on the first it edits the field again, and on a
/// later one it panics.
struct EditsThenPanics {
    controller: TextEditingController,
    heard: Cell<usize>,
}

impl TextStoreObserver for EditsThenPanics {
    fn text_changed(&self, _: TextChange) {
        self.heard.set(self.heard.get() + 1);
        if self.heard.get() == 1 {
            self.controller.set_text("observer edit");
        } else {
            panic!("observer failure after the session");
        }
    }
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

/// `on_changed` edits and panics, so settle parks its failure; the observer
/// hears that edit and edits again, and the flush after the request's
/// grants hears the second edit and panics. The parked failure came first,
/// so it is the one the request raises.
fn observer_panicking_after_a_parked_failure() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("observer after a parked failure");
    let app = controller.clone();
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, text| {
            if text == "a" {
                app.set_text("on_changed edit");
                panic!("on_changed failure");
            }
        }),
        &node,
    );
    let field = field(&harness);
    field.set_observer(Some(Rc::new(EditsThenPanics {
        controller: controller.clone(),
        heard: Cell::new(0),
    })));
    assert_eq!(
        raised(|| {
            let _ = edit(&*field, "a");
        })
        .as_deref(),
        Some("on_changed failure"),
        "the failure parked by the grant's settle came first"
    );
    field.set_observer(None);
    the_field_keeps_working(&mut harness, &field);
    assert_eq!(controller.text(), "observer editz");
}

/// A failure parked in a presentation's gate whose payload panics when
/// destroyed, and the presentation and store gone before any turn took it.
fn gate_dropped_with_a_parked_failure() {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("");
    store.set_commit_gate(gate.clone());
    store.set_owner_listener(Some(Rc::new(|| {
        std::panic::panic_any(PanicsOnDrop("parked payload destroyed"));
    })));
    assert_eq!(
        edit(&*store, "a"),
        Ok(LockOutcome::Granted),
        "the grant stands"
    );
    store.set_owner_listener(None);
    assert_eq!(
        raised(move || {
            drop(gate);
            drop(store);
        }),
        None,
        "an untaken payload is retained, not destroyed, with its gate"
    );
    the_owner_keeps_working(&owner());
}

/// A presentation closes with a failure a store parked for its next turn:
/// the close is that turn, and reports it once its own work is done.
fn close_with_a_parked_failure() {
    let owner = owner();
    let store = InMemoryTextStore::new("");
    let _client = owner
        .handle()
        .attach(TextInputClient::new(store.clone()))
        .expect("attach");
    park_through(&store, "parked before the close");
    assert_eq!(
        raised(|| owner.close()).as_deref(),
        Some("parked before the close"),
        "the close reports the failure that waited for it"
    );
    assert_eq!(
        owner
            .handle()
            .attach(TextInputClient::new(InMemoryTextStore::new("")))
            .err(),
        Some(flui_interaction::TextInputError::Closed),
        "the close completed"
    );
    the_owner_keeps_working(&self::owner());
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
    TextInputOwner::new(Some(Arc::new(Platform)))
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
    let _detached = owner.handle().detach(token).expect("detach");
    // The store still follows the presentation's gate.
    park_through(&store, "parked owner failure");
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
// The matrix
// ----------------------------------------------------------------------------

const ROWS: &[(&str, fn())] = &[
    (
        "gate: dropped with a parked failure",
        gate_dropped_with_a_parked_failure,
    ),
    ("close: with a parked failure", close_with_a_parked_failure),
    (
        "observer: panicking after a parked failure",
        observer_panicking_after_a_parked_failure,
    ),
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
        "arbiter: a refused grant during an unwind",
        arbiter_refusing_a_grant_during_an_unwind,
    ),
    (
        "arbiter: an async grant behind a failing one",
        arbiter_keeping_an_async_grant_behind_a_failing_one,
    ),
    (
        "arbiter: a sync grant behind a failing one",
        arbiter_retaining_a_sync_grant_behind_a_failing_one,
    ),
    (
        "store: dropped during an unwind with a queued grant",
        store_dropped_during_an_unwind_with_a_queued_grant,
    ),
    (
        "store: a request whose flush fails",
        store_refusing_a_grant_when_its_flush_fails,
    ),
    (
        "in-memory settle: retiring after the listener failed",
        in_memory_settle_retiring_after_the_listener_failed,
    ),
    (
        "editable: a refused grant once unmounted",
        editable_refusing_a_grant_once_unmounted,
    ),
    (
        "editable: a grant unmounting its field",
        editable_grant_unmounting_its_field,
    ),
    (
        "editable: on_changed failure ahead of a nested one",
        editable_on_changed_failure_ahead_of_a_nested_one,
    ),
    (
        "editable settle: retiring after on_changed failed",
        editable_settle_retiring_after_on_changed_failed,
    ),
    (
        "editable: a key edit whose listener retirement fails",
        editable_key_edit_whose_listener_retirement_fails,
    ),
    (
        "editable: unmounting with a parked failure",
        editable_unmount_with_a_parked_failure,
    ),
    (
        "attach: a store failing to take the gate",
        attach_with_a_store_failing_to_take_the_gate,
    ),
    (
        "attach: replacing a client whose destruction parks",
        attach_replacing_a_client_whose_destruction_parks,
    ),
    (
        "dispatch: a diagnostic that panics",
        dispatch_whose_diagnostic_panics,
    ),
    (
        "editable: a key edit whose on_changed panics",
        editable_key_edit_whose_on_changed_panics,
    ),
    (
        "editable: a semantic edit whose on_changed panics",
        editable_semantic_set_text_whose_on_changed_panics,
    ),
    (
        "editable: a store outliving its field",
        editable_store_outliving_its_field,
    ),
    (
        "in-memory: a grant reading and editing its store",
        in_memory_grant_reading_and_editing_its_store,
    ),
    (
        "arbiter: a full queue refusing a grant during an unwind",
        arbiter_refusing_a_grant_when_the_queue_is_full_during_an_unwind,
    ),
    (
        "anchor: retiring stores whose drops panic",
        anchor_retiring_stores_whose_drops_panic,
    ),
    (
        "attach: rejected by a closed owner",
        attach_rejected_by_a_closed_owner,
    ),
    (
        "attach: a platform enable that panics",
        attach_whose_platform_enable_panics,
    ),
    (
        "attach and detach: diagnostics that panic",
        attach_and_detach_whose_diagnostics_panic,
    ),
    (
        "cursor area: a platform closing the owner and panicking",
        cursor_area_whose_platform_closes_the_owner_and_panics,
    ),
    (
        "in-memory: a request behind a queued grant that moves the store",
        in_memory_request_behind_a_queued_gate_move,
    ),
    (
        "in-memory: an anchor behind a queued grant that moves the store",
        in_memory_anchor_behind_a_queued_gate_move,
    ),
    (
        "editable: a request behind a queued grant that moves the store",
        editable_request_behind_a_queued_gate_move,
    ),
    (
        "editable: an anchor behind a queued grant that moves the store",
        editable_anchor_behind_a_queued_gate_move,
    ),
    (
        "editable: an update whose observer and focus listener panic",
        editable_update_whose_observer_and_focus_listener_panic,
    ),
    (
        "editable: an update to a node attached elsewhere",
        editable_update_to_a_node_attached_elsewhere,
    ),
    (
        "attach: a store parking a failure while taking the gate, then panicking",
        attach_with_a_store_parking_then_failing_to_take_the_gate,
    ),
    (
        "attach: a store parking a failure while taking the gate",
        attach_with_a_store_parking_while_taking_the_gate,
    ),
    (
        "detach: a stale token whose diagnostic closes the owner and panics",
        stale_detach_whose_diagnostic_closes_the_owner_and_panics,
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

// ----------------------------------------------------------------------------
// Grants the arbiter refuses, queues or loses, and stores going away
// ----------------------------------------------------------------------------

/// Requests a synchronous grant of `store` when dropped; the grant holds a
/// capture that panics when destroyed.
struct RequestsWhenDropped(Rc<dyn TextStore>);

impl Drop for RequestsWhenDropped {
    fn drop(&mut self) {
        let capture = PanicsOnDrop("refused grant capture destroyed");
        let _ = self.0.request_lock(
            LockGrant::read(move |_| {
                let _keep_alive = &capture;
            }),
            LockTiming::Sync,
        );
    }
}

fn arbiter_refusing_a_grant_during_an_unwind() {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("");
    store.set_commit_gate(gate.clone());
    gate.set_open(false);
    let requester: Rc<dyn TextStore> = store.clone();
    assert_eq!(
        raised(move || {
            let _requests = RequestsWhenDropped(requester);
            panic!("unwinding");
        })
        .as_deref(),
        Some("unwinding"),
        "the refused grant is retained, not destroyed during the unwind"
    );
    gate.set_open(true);
    assert_eq!(
        edit(&*store, "a"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
}

/// A grant queued behind a shut gate that panics when it runs.
fn store_with_a_failing_queued_grant() -> (Rc<InMemoryTextStore>, CommitGate) {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("");
    store.set_commit_gate(gate.clone());
    gate.set_open(false);
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(|_| panic!("queued grant failure")),
            LockTiming::Async,
        ),
        Ok(LockOutcome::Deferred)
    );
    gate.set_open(true);
    (store, gate)
}

fn arbiter_keeping_an_async_grant_behind_a_failing_one() {
    let (store, _gate) = store_with_a_failing_queued_grant();
    assert_eq!(
        raised(|| {
            let _ = store.request_lock(insert("b"), LockTiming::Async);
        })
        .as_deref(),
        Some("queued grant failure")
    );
    assert_eq!(
        store.run_deferred_grants(),
        1,
        "the accepted grant still runs"
    );
    assert_eq!(store.text(), "b");
}

fn arbiter_retaining_a_sync_grant_behind_a_failing_one() {
    let (store, gate) = store_with_a_failing_queued_grant();
    let capture = PanicsOnDrop("sync grant capture destroyed");
    assert_eq!(
        raised(|| {
            let _ = store.request_lock(
                LockGrant::read(move |_| {
                    let _keep_alive = &capture;
                }),
                LockTiming::Sync,
            );
        })
        .as_deref(),
        Some("queued grant failure"),
        "the grant that could not run is retained, not destroyed during the unwind"
    );
    assert_eq!(
        edit(&*store, "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(store.text(), "b");
    assert_eq!(parked(&gate), None);
}

fn store_dropped_during_an_unwind_with_a_queued_grant() {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("");
    store.set_commit_gate(gate.clone());
    gate.set_open(false);
    let capture = PanicsOnDrop("queued grant capture destroyed");
    assert_eq!(
        store.request_lock(
            LockGrant::read(move |_| {
                let _keep_alive = &capture;
            }),
            LockTiming::Async,
        ),
        Ok(LockOutcome::Deferred)
    );
    assert_eq!(
        raised(move || {
            let _store = store;
            panic!("unwinding");
        })
        .as_deref(),
        Some("unwinding"),
        "the queued grant is retained, not destroyed during the unwind"
    );
    let next = InMemoryTextStore::new("");
    assert_eq!(edit(&*next, "a"), Ok(LockOutcome::Granted));
}

/// An observer that panics on a text change.
struct FailsOnText;

impl TextStoreObserver for FailsOnText {
    fn text_changed(&self, _: TextChange) {
        panic!("observer failure");
    }
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

fn store_refusing_a_grant_when_its_flush_fails() {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("ab");
    store.set_commit_gate(gate.clone());
    store.set_observer(Some(Rc::new(FailsOnText)));
    gate.set_open(false);
    store.app_replace(
        flui_platform_api::text_store::Utf16Range::new(
            flui_platform_api::text_store::Utf16Offset::new(0),
            flui_platform_api::text_store::Utf16Offset::new(1),
        )
        .expect("ordered"),
        "x",
    );
    gate.set_open(true);
    let capture = PanicsOnDrop("refused grant capture destroyed");
    assert_eq!(
        raised(|| {
            let _ = store.request_lock(
                LockGrant::read(move |_| {
                    let _keep_alive = &capture;
                }),
                LockTiming::Sync,
            );
        })
        .as_deref(),
        Some("observer failure"),
        "the grant the request could not admit is retained"
    );
    store.set_observer(None);
    assert_eq!(
        edit(&*store, "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(store.text(), "xbb");
    assert_eq!(parked(&gate), None);
}

/// Counts its drops.
struct CountsDrops(Rc<Cell<usize>>);

impl Drop for CountsDrops {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

/// An observer that clears itself on a status change, holding a capture.
struct ClearsItself {
    store: Weak<InMemoryTextStore>,
    _capture: CountsDrops,
}

impl TextStoreObserver for ClearsItself {
    fn text_changed(&self, _: TextChange) {}
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {
        if let Some(store) = self.store.upgrade() {
            store.set_observer(None);
        }
    }
}

fn in_memory_settle_retiring_after_the_listener_failed() {
    let gate = CommitGate::new();
    let store = store_behind(&gate, || panic!("listener failure"));
    let drops = Rc::new(Cell::new(0));
    store.set_observer(Some(Rc::new(ClearsItself {
        store: Rc::downgrade(&store),
        _capture: CountsDrops(Rc::clone(&drops)),
    })));
    let protects = Rc::downgrade(&store);
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(move |session| {
                session.insert_at_selection("a").expect("in range");
                // Reported once the lock is released, inside the settle.
                if let Some(store) = protects.upgrade() {
                    store.set_protected(true);
                }
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(parked(&gate).as_deref(), Some("listener failure"));
    assert_eq!(
        drops.get(),
        0,
        "after the settle failed, the observer it released is retained, not destroyed"
    );
    store.set_protected(false);
    the_next_edit_runs_behind(&store, &gate);
}

// ----------------------------------------------------------------------------
// EditableText: the session, its settle, a key edit, dispose
// ----------------------------------------------------------------------------

fn editable_refusing_a_grant_once_unmounted() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("unmounted field");
    let mut harness = focused(EditableText::new(controller, Rc::clone(&node)), &node);
    let field = field(&harness);
    harness.swap_root(flui_widgets::SizedBox::new(1.0, 1.0));
    assert_eq!(
        raised(move || {
            let _requests = RequestsWhenDropped(field);
            panic!("unwinding");
        })
        .as_deref(),
        Some("unwinding"),
        "the refused grant is retained, not destroyed during the unwind"
    );
    assert_eq!(raised(|| harness.tick()), None, "the next frame");
}

fn editable_grant_unmounting_its_field() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("unmounting grant");
    let harness = Rc::new(RefCell::new(focused(
        EditableText::new(controller.clone(), Rc::clone(&node)),
        &node,
    )));
    let field = field(&harness.borrow());
    let nested = Rc::downgrade(&harness);
    assert_eq!(
        field.request_lock(
            LockGrant::read_write(move |session| {
                session.insert_at_selection("ime").expect("in range");
                if let Some(harness) = nested.upgrade() {
                    harness
                        .borrow_mut()
                        .swap_root(flui_widgets::SizedBox::new(1.0, 1.0));
                }
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(
        controller.text(),
        "",
        "a session the field was unmounted under is not written back"
    );
    assert_eq!(
        edit(&*field, "z"),
        Err(TextStoreError::Detached),
        "the store stays detached"
    );
    assert_eq!(
        raised(|| harness.borrow_mut().tick()),
        None,
        "the next frame"
    );
}

thread_local! {
    /// The field a controller listener reaches: a listener is `Send + Sync`
    /// and the store is not.
    static NESTING_FIELD: RefCell<Option<Rc<dyn TextStore>>> = const { RefCell::new(None) };
}

fn editable_on_changed_failure_ahead_of_a_nested_one() {
    use flui_foundation::Listenable as _;
    use std::sync::atomic::{AtomicBool, Ordering};

    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("nested failures");
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(|_cx, text| {
            assert!(!matches!(text, "a" | "ab"), "on_changed failure on {text}");
        }),
        &node,
    );
    let field = field(&harness);
    NESTING_FIELD.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&field)));
    let once = Arc::new(AtomicBool::new(false));
    let listener = controller.add_listener(Arc::new(move || {
        if once.swap(true, Ordering::SeqCst) {
            return;
        }
        let field = NESTING_FIELD.with(|slot| slot.borrow().clone());
        if let Some(field) = field {
            let _ = edit(&*field, "b");
        }
    }));
    assert_eq!(edit(&*field, "a"), Ok(LockOutcome::Granted));
    controller.remove_listener(listener);
    NESTING_FIELD.with(|slot| slot.borrow_mut().take());
    assert_eq!(controller.text(), "ab", "both sessions stand");
    assert_eq!(
        raised(|| harness.tick()).as_deref(),
        Some("on_changed failure on a"),
        "the outer session's failure came first"
    );
    the_field_keeps_working(&mut harness, &field);
}

fn editable_key_edit_whose_listener_retirement_fails() {
    use flui_foundation::Listenable as _;
    use std::sync::Mutex;

    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("key edit");
    let heard = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&heard);
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node))
            .on_changed(move |_cx, text| sink.borrow_mut().push(text.to_owned())),
        &node,
    );
    // A listener that removes itself; the notification's snapshot is then
    // its last owner, and its capture panics when destroyed.
    let own_id = Arc::new(Mutex::new(None));
    let (remover, own) = (controller.clone(), Arc::clone(&own_id));
    let capture = Arc::new(Mutex::new(Some(PanicsOnDropSend(
        "listener capture destroyed",
    ))));
    let id = controller.add_listener(Arc::new(move || {
        let _keep_alive = &capture;
        let id = own.lock().expect("unpoisoned").take();
        if let Some(id) = id {
            remover.remove_listener(id);
        }
    }));
    *own_id.lock().expect("unpoisoned") = Some(id);
    let key = flui_interaction::testing::input::KeyEventBuilder::new(
        flui_interaction::events::Code::KeyA,
    )
    .with_key(flui_interaction::events::Key::Character("a".to_owned()))
    .with_state(flui_interaction::events::KeyState::Down)
    .build();
    let _ = raised(|| {
        let _ = harness.focus_manager().dispatch_key_event(&key);
    });
    assert_eq!(controller.text(), "a");
    assert_eq!(
        *heard.borrow(),
        ["a"],
        "the owner hears of the edit though a listener's retirement failed"
    );
    let _ = raised(|| harness.tick());
    assert_eq!(
        edit(&*field(&harness), "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
}

/// [`PanicsOnDrop`] for a `Send + Sync` controller listener.
struct PanicsOnDropSend(&'static str);

impl Drop for PanicsOnDropSend {
    fn drop(&mut self) {
        panic!("{}", self.0);
    }
}

fn editable_unmount_with_a_parked_failure() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("dispose");
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(|_cx, text| {
            assert!(text.is_empty(), "on_changed failure on {text}");
        }),
        &node,
    );
    let field = field(&harness);
    assert_eq!(edit(&*field, "a"), Ok(LockOutcome::Granted));
    // The failure waits in the presentation's gate for its next turn;
    // detaching the client on unmount leaves it there, and the frame reports it.
    assert_eq!(
        raised(|| harness.swap_root(flui_widgets::SizedBox::new(1.0, 1.0))).as_deref(),
        Some("on_changed failure on a")
    );
    assert_eq!(
        edit(&*field, "z"),
        Err(TextStoreError::Detached),
        "dispose detached the store"
    );
    assert_eq!(
        controller.text(),
        "a",
        "the disposed field's controller is untouched"
    );
    assert_eq!(raised(|| harness.tick()), None, "the next frame");
}

// ----------------------------------------------------------------------------
// TextInputOwner: installing a gate, replacing a client, diagnostics
// ----------------------------------------------------------------------------

/// A store that panics when given a gate.
struct RefusesGate(Rc<InMemoryTextStore>);

impl TextStore for RefusesGate {
    fn status(&self) -> TextStoreStatus {
        self.0.status()
    }
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        self.0.request_lock(grant, timing)
    }
    fn run_deferred_grants(&self) -> usize {
        self.0.run_deferred_grants()
    }
    fn set_commit_gate(&self, _: CommitGate) {
        panic!("store failure installing the gate");
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.0.set_observer(observer);
    }
}

fn attach_with_a_store_failing_to_take_the_gate() {
    let owner = owner();
    let capture = PanicsOnDrop("session callback capture destroyed");
    let client = TextInputClient::new(Rc::new(RefusesGate(InMemoryTextStore::new(""))))
        .on_session_start(move || {
            let _keep_alive = &capture;
        });
    assert_eq!(
        raised(|| {
            let _ = owner.handle().attach(client);
        })
        .as_deref(),
        Some("store failure installing the gate"),
        "the rejected client is retained, not destroyed during the unwind"
    );
    the_owner_keeps_working(&owner);
}

/// A store that, given a gate, edits `other` (whose owner listener panics,
/// parking that failure in the gate `other` follows), then panics if it
/// `refuses`, and otherwise takes the gate.
struct ParksTakingTheGate {
    inner: Rc<InMemoryTextStore>,
    other: Rc<InMemoryTextStore>,
    refuses: bool,
}

impl TextStore for ParksTakingTheGate {
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
        park_through(&self.other, "parked while taking the gate");
        assert!(!self.refuses, "store failure installing the gate");
        self.inner.set_commit_gate(gate);
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

/// An owner with an attached in-memory store, and a client whose store edits
/// that one while taking the owner's gate.
fn owner_and_a_store_parking_through(
    refuses: bool,
) -> (Rc<TextInputOwner>, Rc<InMemoryTextStore>, TextInputClient) {
    let owner = owner();
    let other = InMemoryTextStore::new("");
    let _other = owner
        .handle()
        .attach(TextInputClient::new(other.clone()))
        .expect("attach");
    let client = TextInputClient::new(Rc::new(ParksTakingTheGate {
        inner: InMemoryTextStore::new(""),
        other: other.clone(),
        refuses,
    }));
    (owner, other, client)
}

/// The failure the store's grant parked in the presentation's gate came
/// before the store's own panic, so attach raises it, and nothing is left
/// for the owner's next turn.
fn attach_with_a_store_parking_then_failing_to_take_the_gate() {
    let (owner, other, client) = owner_and_a_store_parking_through(true);
    assert_eq!(
        raised(|| {
            let _ = owner.handle().attach(client);
        })
        .as_deref(),
        Some("parked while taking the gate"),
        "the failure parked inside the store's call came before the call's own"
    );
    assert_eq!(other.text(), "a", "the store's grant stands");
    the_owner_keeps_working(&owner);
}

/// A store that took the gate is admitted, though a grant it requested
/// meanwhile parked a failure: that failure is the owner's next turn's.
fn attach_with_a_store_parking_while_taking_the_gate() {
    let (owner, other, client) = owner_and_a_store_parking_through(false);
    let mut attached = None;
    assert_eq!(
        raised(|| attached = Some(owner.handle().attach(client))),
        None,
        "the store took the gate: attach returns its token"
    );
    assert!(matches!(attached, Some(Ok(_))), "the client is admitted");
    assert_eq!(
        raised(|| owner.dispatch(&ImeEvent::Commit("b".into()))).as_deref(),
        Some("parked while taking the gate"),
        "the owner's next turn reports it"
    );
    assert_eq!(other.text(), "a", "the store's grant stands");
    the_owner_keeps_working(&owner);
}

fn attach_replacing_a_client_whose_destruction_parks() {
    let owner = owner();
    let inner = InMemoryTextStore::new("");
    let dropped = Rc::clone(&inner);
    let capture = PanicsOnDrop("replaced callback capture destroyed");
    let _first = owner
        .handle()
        .attach(
            TextInputClient::new(Rc::new(DropHook {
                inner,
                on_drop: RefCell::new(Some(Box::new(move || {
                    park_through(&dropped, "parked by the replaced store");
                }))),
            }))
            .on_session_start(move || {
                let _keep_alive = &capture;
            }),
        )
        .expect("attach");
    let replacement = InMemoryTextStore::new("");
    let token = owner
        .handle()
        .attach(TextInputClient::new(replacement))
        .expect("the replacement is active, and the caller has its token");
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        })
        .as_deref(),
        Some("parked by the replaced store"),
        "the failure the replaced store parked came before its callback's, at the next turn"
    );
    assert_eq!(
        owner.handle().detach(token),
        Ok(flui_interaction::DetachOutcome::Detached),
        "a blur detaches the replacement with its token"
    );
    assert!(!owner.is_attached(token));
    the_owner_keeps_working(&owner);
}

/// A store whose every request is refused.
struct Refuses;

impl TextStore for Refuses {
    fn status(&self) -> TextStoreStatus {
        TextStoreStatus::EDITABLE_SINGLE_LINE
    }
    fn request_lock(&self, _: LockGrant, _: LockTiming) -> Result<LockOutcome, TextStoreError> {
        Err(TextStoreError::Detached)
    }
    fn run_deferred_grants(&self) -> usize {
        0
    }
    fn set_commit_gate(&self, _: CommitGate) {}
    fn set_observer(&self, _: Option<Rc<dyn TextStoreObserver>>) {}
}

/// A subscriber that panics on every event.
struct FailingSubscriber;

impl tracing::Subscriber for FailingSubscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        panic!("diagnostic failure");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn dispatch_whose_diagnostic_panics() {
    let owner = owner();
    let parking = InMemoryTextStore::new("");
    let token = owner
        .handle()
        .attach(TextInputClient::new(parking.clone()))
        .expect("attach");
    let _ = owner.handle().detach(token);
    park_through(&parking, "parked owner failure");
    let _client = owner
        .handle()
        .attach(TextInputClient::new(Rc::new(Refuses)))
        .expect("attach");
    assert_eq!(
        raised(|| {
            tracing::subscriber::with_default(FailingSubscriber, || {
                owner.dispatch(&ImeEvent::Commit("a".into()));
            });
        })
        .as_deref(),
        Some("parked owner failure"),
        "the diagnostic runs inside the dispatch's containment, behind the earlier failure"
    );
    the_owner_keeps_working(&owner);
}

/// A settle in EditableText whose `on_changed` failed and whose observer then
/// clears itself: the observer it released is retained, not destroyed.
fn editable_settle_retiring_after_on_changed_failed() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("settle retirement");
    let app = controller.clone();
    let mut harness = focused(
        EditableText::new(controller, Rc::clone(&node)).on_changed(move |_cx, text| {
            if text == "a" {
                app.set_text("app");
                panic!("on_changed failure");
            }
        }),
        &node,
    );
    let field = field(&harness);
    let drops = Rc::new(Cell::new(0));
    field.set_observer(Some(Rc::new(ClearsOnText {
        store: RefCell::new(Rc::downgrade(&field)),
        _capture: CountsDrops(Rc::clone(&drops)),
    })));
    assert_eq!(edit(&*field, "a"), Ok(LockOutcome::Granted));
    assert_eq!(
        drops.get(),
        0,
        "after the settle failed, the observer it released is retained, not destroyed"
    );
    assert_eq!(
        raised(|| harness.tick()).as_deref(),
        Some("on_changed failure")
    );
    the_field_keeps_working(&mut harness, &field);
}

/// An observer that clears itself on a text change, holding a capture.
struct ClearsOnText {
    store: RefCell<Weak<dyn TextStore>>,
    _capture: CountsDrops,
}

impl TextStoreObserver for ClearsOnText {
    fn text_changed(&self, _: TextChange) {
        if let Some(store) = self.store.borrow().upgrade() {
            store.set_observer(None);
        }
    }
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

// ----------------------------------------------------------------------------
// Operations that complete their own changes before a failure is resumed
// ----------------------------------------------------------------------------

/// Hears text changes, and panics on the first one when `fails` is set.
struct HearsText {
    heard: Rc<Cell<usize>>,
    fails: Cell<bool>,
}

impl TextStoreObserver for HearsText {
    fn text_changed(&self, _: TextChange) {
        self.heard.set(self.heard.get() + 1);
        assert!(!self.fails.replace(false), "observer failure");
    }
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

fn hearing(field: &Rc<dyn TextStore>, fails: bool) -> Rc<Cell<usize>> {
    let heard = Rc::new(Cell::new(0));
    field.set_observer(Some(Rc::new(HearsText {
        heard: Rc::clone(&heard),
        fails: Cell::new(fails),
    })));
    heard
}

fn editable_key_edit_whose_on_changed_panics() {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("key edit, failing owner");
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(|_cx, text| {
            assert!(text != "a", "on_changed failure");
        }),
        &node,
    );
    let field = field(&harness);
    let heard = hearing(&field, false);
    let key = flui_interaction::testing::input::KeyEventBuilder::new(
        flui_interaction::events::Code::KeyA,
    )
    .with_key(flui_interaction::events::Key::Character("a".to_owned()))
    .with_state(flui_interaction::events::KeyState::Down)
    .build();
    let _ = raised(|| {
        let _ = harness.focus_manager().dispatch_key_event(&key);
    });
    assert_eq!(controller.text(), "a");
    assert_eq!(
        heard.get(),
        1,
        "the platform heard of the key's edit though on_changed panicked"
    );
    field.set_observer(None);
    the_field_keeps_working(&mut harness, &field);
}

fn editable_semantic_set_text_whose_on_changed_panics() {
    use flui_testing::a11y::Role;
    use flui_testing::{Action, ActionData, ActionRequest, TreeId};

    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("semantic edit, failing owner");
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(|_cx, text| {
            assert!(text != "said", "on_changed failure");
        }),
        &node,
    );
    harness.enable_semantics();
    harness.tick();
    let field = field(&harness);
    let heard = hearing(&field, false);
    let id = harness
        .a11y_tree()
        .expect("semantics enabled")
        .find(Role::TextInput)
        .expect("the field")
        .id();
    let _ = raised(|| {
        let _ = harness.invoke_semantics_action(ActionRequest {
            action: Action::SetValue,
            target_tree: TreeId::ROOT,
            target_node: id,
            data: Some(ActionData::Value("said".into())),
        });
    });
    assert_eq!(controller.text(), "said");
    assert_eq!(
        heard.get(),
        1,
        "the platform heard of the semantic edit though on_changed panicked"
    );
    field.set_observer(None);
    the_field_keeps_working(&mut harness, &field);
}

// ----------------------------------------------------------------------------
// Inventory points: queue limits, anchors, rejection, the platform
// ----------------------------------------------------------------------------

fn arbiter_refusing_a_grant_when_the_queue_is_full_during_an_unwind() {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("");
    store.set_commit_gate(gate.clone());
    gate.set_open(false);
    for _ in 0..flui_platform_api::text_store::DEFERRED_LOCK_CAPACITY {
        assert_eq!(
            store.request_lock(LockGrant::read(|_| {}), LockTiming::Async),
            Ok(LockOutcome::Deferred)
        );
    }
    /// Requests an asynchronous grant of a full queue when dropped.
    struct RequestsAsync(Rc<InMemoryTextStore>);
    impl Drop for RequestsAsync {
        fn drop(&mut self) {
            let capture = PanicsOnDrop("refused grant capture destroyed");
            let outcome = self.0.request_lock(
                LockGrant::read(move |_| {
                    let _keep_alive = &capture;
                }),
                LockTiming::Async,
            );
            assert_eq!(outcome, Err(TextStoreError::DeferredQueueFull));
        }
    }
    let requester = Rc::clone(&store);
    assert_eq!(
        raised(move || {
            let _requests = RequestsAsync(requester);
            panic!("unwinding");
        })
        .as_deref(),
        Some("unwinding"),
        "the grant the full queue refused is retained, not destroyed during the unwind"
    );
    gate.set_open(true);
    assert_eq!(
        store.run_deferred_grants(),
        flui_platform_api::text_store::DEFERRED_LOCK_CAPACITY
    );
    assert_eq!(
        edit(&*store, "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(store.text(), "b");
    assert_eq!(parked(&gate), None);
}

/// A store that panics with `message` when destroyed.
fn store_panicking_on_drop(message: &'static str) -> Rc<DropHook> {
    Rc::new(DropHook {
        inner: InMemoryTextStore::new(""),
        on_drop: RefCell::new(Some(Box::new(move || panic!("{message}")))),
    })
}

fn anchor_retiring_stores_whose_drops_panic() {
    let owner = owner();
    let _first = owner
        .handle()
        .attach(TextInputClient::new(store_panicking_on_drop(
            "first retired store destroyed",
        )))
        .expect("attach");
    owner.set_transaction_open(true);
    let _second = owner
        .handle()
        .attach(TextInputClient::new(store_panicking_on_drop(
            "second retired store destroyed",
        )))
        .expect("replacement inside the frame");
    let _third = owner
        .handle()
        .attach(TextInputClient::new(InMemoryTextStore::new("")))
        .expect("replacement inside the frame");
    owner.set_transaction_open(false);
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        })
        .as_deref(),
        Some("first retired store destroyed"),
        "the anchor retires its stores inside its scope: the first failure, the second retained"
    );
    the_owner_keeps_working(&owner);
}

fn attach_rejected_by_a_closed_owner() {
    let owner = owner();
    owner.close();
    let capture = PanicsOnDrop("rejected callback capture destroyed");
    let client = TextInputClient::new(store_panicking_on_drop("rejected store destroyed"))
        .on_session_start(move || {
            let _keep_alive = &capture;
        });
    assert_eq!(
        raised(|| {
            let _ = owner.handle().attach(client);
        })
        .as_deref(),
        Some("rejected store destroyed"),
        "the rejected client retires inside the close's containment: store first, the callback retained"
    );
    the_owner_keeps_working(&self::owner());
}

/// A platform whose first `set_ime_allowed` panics.
struct FailsToEnable(std::sync::atomic::AtomicBool);

impl PlatformTextInput for FailsToEnable {
    fn set_ime_allowed(&self, _: bool) {
        assert!(
            self.0.swap(true, std::sync::atomic::Ordering::SeqCst),
            "platform failure enabling input"
        );
    }
    fn set_ime_cursor_area(&self, _: Bounds<f64>) {}
}

fn attach_whose_platform_enable_panics() {
    let owner = TextInputOwner::new(Some(Arc::new(FailsToEnable(
        std::sync::atomic::AtomicBool::new(false),
    ))));
    let token = owner
        .handle()
        .attach(TextInputClient::new(InMemoryTextStore::new("")))
        .expect("the client is active, and the caller has its token");
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        })
        .as_deref(),
        Some("platform failure enabling input"),
        "the failure is reported at the owner's next turn"
    );
    assert_eq!(
        owner.handle().detach(token),
        Ok(flui_interaction::DetachOutcome::Detached),
        "the token detaches the client"
    );
    the_owner_keeps_working(&owner);
}

fn attach_and_detach_whose_diagnostics_panic() {
    let owner = owner();
    let token = tracing::subscriber::with_default(FailingSubscriber, || {
        owner
            .handle()
            .attach(TextInputClient::new(InMemoryTextStore::new("")))
    })
    .expect("the client is active, and the caller has its token");
    assert_eq!(
        raised(|| {
            let _ = owner.run_deferred_grants();
        })
        .as_deref(),
        Some("diagnostic failure")
    );
    assert_eq!(
        raised(|| {
            tracing::subscriber::with_default(FailingSubscriber, || {
                let _ = owner.handle().detach(token);
            });
        })
        .as_deref(),
        Some("diagnostic failure"),
        "detach completes, then reports its diagnostic's failure"
    );
    assert!(!owner.is_attached(token), "the client was detached");
    the_owner_keeps_working(&owner);
}

thread_local! {
    /// The owner a platform closes from inside its own call.
    static CLOSING_OWNER: RefCell<Option<Weak<TextInputOwner>>> = const { RefCell::new(None) };
}

/// A platform that closes its owner, then panics, from its cursor-area
/// call, and panics when destroyed.
struct ClosesOnCursor;

impl PlatformTextInput for ClosesOnCursor {
    fn set_ime_allowed(&self, _: bool) {}
    fn set_ime_cursor_area(&self, _: Bounds<f64>) {
        let owner = CLOSING_OWNER.with(|slot| slot.borrow().as_ref().and_then(Weak::upgrade));
        if let Some(owner) = owner {
            owner.close();
        }
        panic!("platform failure placing the cursor");
    }
}

impl Drop for ClosesOnCursor {
    fn drop(&mut self) {
        panic!("platform destroyed");
    }
}

fn cursor_area_whose_platform_closes_the_owner_and_panics() {
    let owner = TextInputOwner::new(Some(Arc::new(ClosesOnCursor)));
    CLOSING_OWNER.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&owner)));
    let _client = owner
        .handle()
        .attach(TextInputClient::new(InMemoryTextStore::new("")))
        .expect("attach");
    let rect = Bounds::new(
        flui_foundation::geometry::Point::new(0.0, 0.0),
        flui_foundation::geometry::Size::new(1.0, 1.0),
    );
    assert_eq!(
        raised(|| {
            let _ = owner.handle().set_cursor_area(rect);
        })
        .as_deref(),
        Some("platform failure placing the cursor"),
        "the platform clone is released inside the scope, not during the unwind"
    );
    CLOSING_OWNER.with(|slot| slot.borrow_mut().take());
    the_owner_keeps_working(&self::owner());
}

fn editable_store_outliving_its_field() {
    use flui_foundation::Listenable as _;

    let controller = TextEditingController::new();
    // The store ends up holding the controller's last handle; a listener
    // capture of it panics when destroyed.
    let listener_capture = std::sync::Mutex::new(PanicsOnDropSend("listener capture destroyed"));
    let _listener = controller.add_listener(Arc::new(move || {
        let _keep_alive = &listener_capture;
    }));
    let node = FocusNode::with_debug_label("outlived field");
    let capture = PanicsOnDrop("on_changed capture destroyed");
    let mut harness = focused(
        EditableText::new(controller, Rc::clone(&node)).on_changed(move |_cx, _| {
            let _keep_alive = &capture;
        }),
        &node,
    );
    let field = field(&harness);
    harness.swap_root(flui_widgets::SizedBox::new(1.0, 1.0));
    assert_eq!(
        raised(move || drop(field)).as_deref(),
        Some("on_changed capture destroyed"),
        "the store retires its last handles in order; the controller is retained after the failure"
    );
    assert_eq!(raised(|| harness.tick()), None, "the next frame");
}

/// A grant's body reads the in-memory store and edits it as the
/// application: no borrow of the store is held across the body, and the
/// application's edit wins over the session.
fn in_memory_grant_reading_and_editing_its_store() {
    let gate = CommitGate::new();
    let store = InMemoryTextStore::new("ab");
    store.set_commit_gate(gate.clone());
    let reentered = Rc::downgrade(&store);
    let read = Rc::new(RefCell::new(String::new()));
    let sink = Rc::clone(&read);
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(move |session| {
                session.insert_at_selection("x").expect("in range");
                if let Some(store) = reentered.upgrade() {
                    *sink.borrow_mut() = store.text();
                    store.app_replace(
                        flui_platform_api::text_store::Utf16Range::new(
                            flui_platform_api::text_store::Utf16Offset::new(0),
                            flui_platform_api::text_store::Utf16Offset::new(0),
                        )
                        .expect("ordered"),
                        "app ",
                    );
                }
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(*read.borrow(), "ab", "the body read the store as it was");
    assert_eq!(store.text(), "app ab", "the application's edit wins");
    assert_eq!(
        edit(&*store, "!"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(
        store.text(),
        "app !ab",
        "the next edit lands at the caret the application left"
    );
    assert_eq!(parked(&gate), None);
}

// ----------------------------------------------------------------------------
// A queued grant that moves its store before a later grant settles
// ----------------------------------------------------------------------------

/// Queue, behind `queued_behind` shut, a grant that moves `store` to
/// `moved_to`, then the grants in `then`; reopen the gate. They run in order
/// at the next request or anchor, the later ones behind `moved_to`.
fn queue_a_gate_move(
    store: &Rc<dyn TextStore>,
    queued_behind: &CommitGate,
    moved_to: &CommitGate,
    then: Vec<LockGrant>,
) {
    store.set_commit_gate(queued_behind.clone());
    queued_behind.set_open(false);
    let (weak, moved_to) = (Rc::downgrade(store), moved_to.clone());
    let moving = LockGrant::read(move |_| {
        if let Some(store) = weak.upgrade() {
            store.set_commit_gate(moved_to);
        }
    });
    for grant in std::iter::once(moving).chain(then) {
        assert_eq!(
            store.request_lock(grant, LockTiming::Async),
            Ok(LockOutcome::Deferred)
        );
    }
    queued_behind.set_open(true);
}

/// Run an edit inserting "a" behind a queued grant that moves `store` to
/// `moved_to`: as a request after it, or with it at an anchor. What the run
/// raised.
fn edit_behind_a_queued_gate_move(
    store: &Rc<dyn TextStore>,
    moved_to: &CommitGate,
    through_anchor: bool,
) -> Option<String> {
    let queued_behind = CommitGate::new();
    let outcome = if through_anchor {
        queue_a_gate_move(store, &queued_behind, moved_to, vec![insert("a")]);
        raised(|| {
            let _ = store.run_deferred_grants();
        })
    } else {
        queue_a_gate_move(store, &queued_behind, moved_to, Vec::new());
        raised(|| {
            let _ = edit(&**store, "a");
        })
    };
    assert_eq!(
        parked(&queued_behind),
        None,
        "nothing waits at the gate the store left"
    );
    outcome
}

/// An application edit of `store` behind `gate` shut for its duration, so
/// the observer hears of it only at the store's next flush.
fn app_edit_held_back(store: &Weak<InMemoryTextStore>, gate: &CommitGate) {
    if let Some(store) = store.upgrade() {
        gate.set_open(false);
        store.app_replace(
            flui_platform_api::text_store::Utf16Range::new(
                flui_platform_api::text_store::Utf16Offset::new(0),
                flui_platform_api::text_store::Utf16Offset::new(0),
            )
            .expect("ordered"),
            "app ",
        );
        gate.set_open(true);
    }
}

/// On its first text change, makes an application edit held back to the
/// store's next flush; panics on the next text change.
struct AppEditsThenPanics {
    store: Weak<InMemoryTextStore>,
    gate: CommitGate,
    heard: Cell<usize>,
}

impl TextStoreObserver for AppEditsThenPanics {
    fn text_changed(&self, _: TextChange) {
        self.heard.set(self.heard.get() + 1);
        assert!(self.heard.get() == 1, "observer failure after the session");
        app_edit_held_back(&self.store, &self.gate);
    }
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {}
}

/// The last grant settles behind the gate an earlier queued grant moved the
/// store to: its listener edits and panics, parking its failure there; the
/// observer hears that edit in the settle and edits again, and the flush
/// after the grants hears the second edit and panics. The parked failure
/// came first, so it is raised.
fn in_memory_behind_a_queued_gate_move(through_anchor: bool) {
    let moved_to = CommitGate::new();
    let store = InMemoryTextStore::new("");
    let (edited, behind) = (Rc::downgrade(&store), moved_to.clone());
    store.set_owner_listener(Some(Rc::new(move || {
        app_edit_held_back(&edited, &behind);
        panic!("owner failure");
    })));
    store.set_observer(Some(Rc::new(AppEditsThenPanics {
        store: Rc::downgrade(&store),
        gate: moved_to.clone(),
        heard: Cell::new(0),
    })));
    let moved: Rc<dyn TextStore> = store.clone();
    assert_eq!(
        edit_behind_a_queued_gate_move(&moved, &moved_to, through_anchor).as_deref(),
        Some("owner failure"),
        "the failure the last grant's settle parked came before the flush's"
    );
    store.set_observer(None);
    store.set_owner_listener(None);
    assert_eq!(parked(&moved_to), None, "raised, not left parked");
    assert_eq!(
        edit(&*store, "b"),
        Ok(LockOutcome::Granted),
        "the next edit"
    );
    assert_eq!(parked(&moved_to), None, "the next edit fails nothing");
}

fn in_memory_request_behind_a_queued_gate_move() {
    in_memory_behind_a_queued_gate_move(false);
}

fn in_memory_anchor_behind_a_queued_gate_move() {
    in_memory_behind_a_queued_gate_move(true);
}

/// [`in_memory_behind_a_queued_gate_move`] for `EditableText`: its
/// `on_changed` edits and panics, and the observer's own edit reaches only
/// the flush after the grants, which panics.
fn editable_behind_a_queued_gate_move(through_anchor: bool) {
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("queued gate move");
    let app = controller.clone();
    let mut harness = focused(
        EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(move |_cx, text| {
            if text == "a" {
                app.set_text("on_changed edit");
                panic!("on_changed failure");
            }
        }),
        &node,
    );
    let field = field(&harness);
    field.set_observer(Some(Rc::new(EditsThenPanics {
        controller: controller.clone(),
        heard: Cell::new(0),
    })));
    let moved_to = CommitGate::new();
    assert_eq!(
        edit_behind_a_queued_gate_move(&field, &moved_to, through_anchor).as_deref(),
        Some("on_changed failure"),
        "the failure the last grant's settle parked came before the flush's"
    );
    field.set_observer(None);
    assert_eq!(parked(&moved_to), None, "raised, not left parked");
    the_field_keeps_working(&mut harness, &field);
    assert_eq!(controller.text(), "observer editz");
}

fn editable_request_behind_a_queued_gate_move() {
    editable_behind_a_queued_gate_move(false);
}

fn editable_anchor_behind_a_queued_gate_move() {
    editable_behind_a_queued_gate_move(true);
}

// ----------------------------------------------------------------------------
// An update replacing a focused node
// ----------------------------------------------------------------------------

/// An observer whose status change panics.
struct FailsOnStatus;

impl TextStoreObserver for FailsOnStatus {
    fn text_changed(&self, _: TextChange) {}
    fn selection_changed(&self) {}
    fn layout_changed(&self) {}
    fn status_changed(&self) {
        panic!("observer failure on status");
    }
}

/// One rebuild obscures the field, whose observer panics on the status
/// change, and replaces its focused node, whose listener panics on the
/// focus loss and so cuts the focus notifications short. The update still
/// moves the field onto the replacement node and ends the old node's IME
/// session before it raises the observer's failure, the first. The frame
/// recovers from the update's panic by retiring the field, which releases
/// the replacement node it now holds; the harness does not raise a
/// recovered panic, so the row checks what the update left behind.
fn editable_update_whose_observer_and_focus_listener_panic() {
    let controller = TextEditingController::new();
    let (old, new) = (
        FocusNode::with_debug_label("replaced node"),
        FocusNode::with_debug_label("replacement node"),
    );
    let mut harness = focused(EditableText::new(controller.clone(), Rc::clone(&old)), &old);
    let field = field(&harness);
    // Behind a gate of its own, open during the frame, the store tells its
    // observer of the update's status change at once.
    field.set_commit_gate(CommitGate::new());
    field.set_observer(Some(Rc::new(FailsOnStatus)));
    let heard = Rc::new(Cell::new(false));
    let listener = Rc::clone(&heard);
    let _listening = old.add_listener(Rc::new(move || {
        assert!(listener.replace(true), "focus listener failure");
    }));
    harness.swap_root(EditableText::new(controller.clone(), Rc::clone(&new)).obscure_text(true));
    field.set_observer(None);
    assert!(heard.get(), "the old node heard its focus loss");
    assert!(!old.is_attached(), "the old node was replaced");
    assert!(
        !new.is_attached(),
        "the field holds the replacement's attachment, so recovering from the \
         update's failure releases it"
    );
    assert!(
        harness.active_text_store().is_none(),
        "the old node's IME session ended with its focus"
    );
    assert_eq!(raised(|| harness.tick()), None, "the next frame");
    let next = FocusNode::with_debug_label("next field");
    harness.swap_root(EditableText::new(controller, Rc::clone(&next)));
    next.request_focus();
    harness.tick();
    let field = self::field(&harness);
    the_field_keeps_working(&mut harness, &field);
}

/// Two fields side by side, the first on `first`, the second on `second`.
fn two_fields(
    controllers: &(TextEditingController, TextEditingController),
    first: &Rc<FocusNode>,
    second: &Rc<FocusNode>,
) -> impl flui_view::View {
    flui_widgets::Column::new(flui_widgets::column![
        EditableText::new(controllers.0.clone(), Rc::clone(first)),
        EditableText::new(controllers.1.clone(), Rc::clone(second)),
    ])
}

/// One rebuild hands the focused first field the second field's node,
/// which `replace_node` rejects as already attached. The rejection is the
/// update's failure, and the frame recovers by retiring the first field;
/// the second field keeps its node where it was, attached through the
/// handle it holds.
fn editable_update_to_a_node_attached_elsewhere() {
    let controllers = (TextEditingController::new(), TextEditingController::new());
    let (first, second) = (
        FocusNode::with_debug_label("first field"),
        FocusNode::with_debug_label("second field"),
    );
    let mut harness = focused(two_fields(&controllers, &first, &second), &first);
    let parent = second
        .parent()
        .expect("the second field's node is attached");
    // The first field asks for the second field's node.
    harness.swap_root(two_fields(&controllers, &second, &second));
    assert!(
        !first.is_attached(),
        "the rejection was the update's failure: the first field was retired"
    );
    assert!(
        second.is_attached(),
        "the second field's node stays attached"
    );
    assert!(
        second
            .parent()
            .is_some_and(|held| Rc::ptr_eq(&held, &parent)),
        "under its own parent"
    );
    assert_eq!(raised(|| harness.tick()), None, "the next frame");
    harness.swap_root(flui_widgets::SizedBox::new(1.0, 1.0));
    assert!(
        !second.is_attached(),
        "the second field's handle still owned its node, so its dispose detached it"
    );
}

// ----------------------------------------------------------------------------
// A stale detach whose diagnostic closes the owner
// ----------------------------------------------------------------------------

/// A platform that panics when destroyed.
struct PanicsWhenDestroyed;

impl PlatformTextInput for PanicsWhenDestroyed {
    fn set_ime_allowed(&self, _: bool) {}
    fn set_ime_cursor_area(&self, _: Bounds<f64>) {}
}

impl Drop for PanicsWhenDestroyed {
    fn drop(&mut self) {
        panic!("platform destroyed");
    }
}

/// A subscriber that, on an event, closes [`CLOSING_OWNER`] (containing
/// what the close raises) and then panics.
struct ClosesThenFails;

impl tracing::Subscriber for ClosesThenFails {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        let owner = CLOSING_OWNER.with(|slot| slot.borrow().as_ref().and_then(Weak::upgrade));
        if let Some(owner) = owner {
            let _ = raised(|| owner.close());
        }
        panic!("diagnostic failure");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn stale_detach_whose_diagnostic_closes_the_owner_and_panics() {
    let owner = TextInputOwner::new(Some(Arc::new(PanicsWhenDestroyed)));
    let token = owner
        .handle()
        .attach(TextInputClient::new(InMemoryTextStore::new("")))
        .expect("attach");
    assert_eq!(
        owner.handle().detach(token),
        Ok(flui_interaction::DetachOutcome::Detached)
    );
    CLOSING_OWNER.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&owner)));
    assert_eq!(
        raised(|| {
            tracing::subscriber::with_default(ClosesThenFails, || {
                let _ = owner.handle().detach(token);
            });
        })
        .as_deref(),
        Some("diagnostic failure"),
        "the platform clone is released before the diagnostic, not during the unwind"
    );
    CLOSING_OWNER.with(|slot| slot.borrow_mut().take());
    assert_eq!(
        owner
            .handle()
            .attach(TextInputClient::new(InMemoryTextStore::new("")))
            .err(),
        Some(flui_interaction::TextInputError::Closed),
        "the diagnostic's close completed"
    );
    the_owner_keeps_working(&self::owner());
}
