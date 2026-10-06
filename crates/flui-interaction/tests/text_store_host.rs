//! A presentation's text-input owner driving a pull-model host (ADR-0135):
//! which store the host serves, in what order, and how a composition is
//! committed when the host can or cannot do it.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_interaction::{TextInputBackend, TextInputClient, TextInputError, TextInputOwner};
use flui_platform_api::text_store::{
    CompositionEnd, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore,
    TextStoreHost, TextStoreHostError, project_ime_event,
};
use flui_platform_api::{ImeEvent, PlatformTextInput};

type Log = Rc<RefCell<Vec<&'static str>>>;

/// A host that logs every call, answers completions as told, and can run a
/// callback inside its first focus call, as a text service reaching
/// application code does.
struct Host {
    log: Log,
    focused: RefCell<Option<Rc<dyn TextStore>>>,
    depth: Cell<u32>,
    nested: Cell<bool>,
    answer: Cell<Result<CompositionEnd, TextStoreHostError>>,
    inside_focus: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Host {
    fn new(log: &Log) -> Rc<Self> {
        Rc::new(Self {
            log: Rc::clone(log),
            focused: RefCell::new(None),
            depth: Cell::new(0),
            nested: Cell::new(false),
            answer: Cell::new(Ok(CompositionEnd::Committed)),
            inside_focus: RefCell::new(None),
        })
    }

    fn enter(&self, call: &'static str) {
        if self.depth.get() > 0 {
            self.nested.set(true);
        }
        self.depth.set(self.depth.get() + 1);
        self.log.borrow_mut().push(call);
    }

    fn leave(&self) {
        self.depth.set(self.depth.get() - 1);
    }

    fn focuses(&self, store: &Rc<InMemoryTextStore>) -> bool {
        let store: Rc<dyn TextStore> = store.clone(); // compared through the erased contract the host holds.
        self.focused
            .borrow()
            .as_ref()
            .is_some_and(|focused| Rc::ptr_eq(focused, &store))
    }
}

impl TextStoreHost for Host {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        self.enter(if store.is_some() { "focus" } else { "unfocus" });
        *self.focused.borrow_mut() = store;
        let inside = self.inside_focus.borrow_mut().take();
        if let Some(inside) = inside {
            inside();
        }
        self.leave();
    }

    fn complete_composition(&self) -> Result<CompositionEnd, TextStoreHostError> {
        self.enter("complete");
        self.leave();
        self.answer.get()
    }
}

fn pull_owner() -> (Rc<TextInputOwner>, Rc<Host>, Log) {
    let log = Log::default();
    let host = Host::new(&log);
    let backend = TextInputBackend::Pull(host.clone());
    (TextInputOwner::new(backend), host, log)
}

fn client(store: &Rc<InMemoryTextStore>) -> TextInputClient {
    TextInputClient::new(store.clone())
}

/// Type "かな" after "ab" through the push projection, leaving it composing.
fn composing_store() -> Rc<InMemoryTextStore> {
    let store = InMemoryTextStore::new("ab");
    store.set_commit_gate(flui_platform_api::text_store::CommitGate::new());
    let applied = project_ime_event(
        &*store,
        &ImeEvent::Preedit {
            text: "かな".to_owned(),
            cursor: Some((0, 0)),
        },
    );
    assert_eq!(applied, Ok(LockOutcome::Granted), "preedit applies");
    assert!(store.composition().is_some(), "test setup composes");
    store
}

fn calls(log: &Log) -> Vec<&'static str> {
    log.borrow().clone()
}

fn attach_replace_and_detach_move_the_host_focus() {
    let (owner, host, log) = pull_owner();
    let handle = owner.handle();
    let (a, b) = (InMemoryTextStore::new("a"), InMemoryTextStore::new("b"));
    handle.attach(client(&a)).expect("pull owner attaches");
    assert!(host.focuses(&a));
    let token = handle.attach(client(&b)).expect("replacement");
    assert!(host.focuses(&b));
    handle
        .set_cursor_area(Bounds::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0)))
        .expect("a pull owner accepts the area and has nowhere to send it");
    let _ = handle.detach(token).expect("detach");
    assert!(host.focused.borrow().is_none());
    assert_eq!(calls(&log), ["focus", "focus", "unfocus"]);
}

fn host_operations_in_a_frame_wait_for_the_anchor_and_run_first() {
    let (owner, host, log) = pull_owner();
    let store = InMemoryTextStore::new("");
    owner.set_transaction_open(true);
    owner
        .handle()
        .attach(client(&store))
        .expect("attach in frame");
    let granted = Rc::clone(&log);
    let outcome = store
        .request_lock(
            LockGrant::read(move |_| granted.borrow_mut().push("grant")),
            LockTiming::Async,
        )
        .expect("queued behind the frame");
    assert_eq!(outcome, LockOutcome::Deferred);
    assert!(calls(&log).is_empty(), "nothing reaches the host mid-frame");
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
    assert_eq!(calls(&log), ["focus", "grant"]);
    assert!(host.focuses(&store));
}

fn an_abandoned_completion_commits_the_composition_in_place() {
    for answer in [
        Ok(CompositionEnd::Abandoned),
        Err(TextStoreHostError::Unavailable),
    ] {
        let (owner, host, log) = pull_owner();
        let store = composing_store();
        let text = store.text();
        owner.handle().attach(client(&store)).expect("attach");
        host.answer.set(answer);
        owner.complete_composition();
        assert_eq!(calls(&log), ["focus", "complete"], "{answer:?}");
        assert_eq!(store.composition(), None, "{answer:?}: committed in place");
        assert_eq!(store.text(), text, "{answer:?}: the composed text stays");
    }
}

fn a_committed_or_deferred_completion_is_left_to_the_host() {
    for answer in [CompositionEnd::Committed, CompositionEnd::Deferred] {
        let (owner, host, log) = pull_owner();
        let store = composing_store();
        owner.handle().attach(client(&store)).expect("attach");
        host.answer.set(Ok(answer));
        owner.complete_composition();
        assert_eq!(calls(&log), ["focus", "complete"], "{answer:?}");
        assert!(
            store.composition().is_some(),
            "{answer:?}: the host owns the outcome"
        );
    }
}

fn a_completion_queued_in_a_frame_keeps_its_store_after_detach() {
    let (owner, host, log) = pull_owner();
    let handle = owner.handle();
    let store = composing_store();
    let token = handle.attach(client(&store)).expect("attach");
    host.answer.set(Ok(CompositionEnd::Abandoned));
    owner.set_transaction_open(true);
    handle.complete_composition(token).expect("owner open");
    let _ = handle.detach(token).expect("detach in frame");
    assert_eq!(calls(&log), ["focus"]);
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
    assert_eq!(calls(&log), ["focus", "complete", "unfocus"]);
    assert_eq!(
        store.composition(),
        None,
        "the detached field's composition is still committed"
    );
}

fn a_host_call_reaching_the_owner_is_queued_until_it_returns() {
    let (owner, host, log) = pull_owner();
    let handle = owner.handle();
    let (a, b) = (InMemoryTextStore::new("a"), InMemoryTextStore::new("b"));
    let reentrant = handle.clone();
    let weak = Rc::downgrade(&owner);
    let second = b.clone();
    *host.inside_focus.borrow_mut() = Some(Box::new(move || {
        weak.upgrade().expect("owner alive").complete_composition();
        reentrant.attach(client(&second)).expect("reentrant attach");
    }));
    handle.attach(client(&a)).expect("attach");
    assert!(!host.nested.get(), "no host call ran inside another");
    assert_eq!(calls(&log), ["focus", "complete", "focus"]);
    assert!(host.focuses(&b));
}

fn close_completes_queued_compositions_then_unfocuses() {
    let (owner, _host, log) = pull_owner();
    let handle = owner.handle();
    let store = composing_store();
    let token = handle.attach(client(&store)).expect("attach");
    owner.set_transaction_open(true);
    owner.complete_composition();
    owner.close();
    assert_eq!(calls(&log), ["focus", "complete", "unfocus"]);
    assert_eq!(
        handle.complete_composition(token),
        Err(TextInputError::Closed)
    );
}

#[derive(Default)]
struct Push(parking_lot::Mutex<Vec<bool>>);

impl PlatformTextInput for Push {
    fn set_ime_allowed(&self, allowed: bool) {
        self.0.lock().push(allowed);
    }
    fn set_ime_cursor_area(&self, _: Bounds<f64>) {}
}

fn push_and_storeless_backends_commit_in_place() {
    let platform = Arc::new(Push::default());
    let owner = TextInputOwner::new(TextInputBackend::Push(platform.clone()));
    let handle = owner.handle();
    let store = composing_store();
    let text = store.text();
    let token = handle.attach(client(&store)).expect("attach");
    owner.complete_composition();
    assert_eq!(store.composition(), None);
    assert_eq!(store.text(), text);
    let _ = handle.detach(token).expect("detach");
    assert_eq!(
        handle.complete_composition(token),
        Ok(()),
        "a stale token is a no-op"
    );
    assert_eq!(*platform.0.lock(), [true, false]);

    let none = TextInputOwner::new(TextInputBackend::None);
    assert_eq!(
        none.handle().attach(client(&InMemoryTextStore::new(""))),
        Err(TextInputError::Unsupported)
    );
}

#[test]
fn the_owner_drives_its_text_store_host() {
    let cases: &[(&str, fn())] = &[
        (
            "focus follows attach",
            attach_replace_and_detach_move_the_host_focus,
        ),
        (
            "frame defers host operations",
            host_operations_in_a_frame_wait_for_the_anchor_and_run_first,
        ),
        (
            "abandoned completion",
            an_abandoned_completion_commits_the_composition_in_place,
        ),
        (
            "host-owned completion",
            a_committed_or_deferred_completion_is_left_to_the_host,
        ),
        (
            "queued completion keeps its store",
            a_completion_queued_in_a_frame_keeps_its_store_after_detach,
        ),
        (
            "reentry is queued",
            a_host_call_reaching_the_owner_is_queued_until_it_returns,
        ),
        ("close", close_completes_queued_compositions_then_unfocuses),
        ("push and none", push_and_storeless_backends_commit_in_place),
    ];
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
            failed.push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failed.is_empty(), "failed cases: {failed:?}");
}
