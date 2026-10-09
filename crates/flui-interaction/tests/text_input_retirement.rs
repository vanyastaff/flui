//! Owner retirement through the same public handles used by mounted text fields.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Bounds;
use flui_interaction::{
    ClientToken, TextInputBackend, TextInputClient, TextInputError, TextInputOwner,
};
use flui_platform_api::PlatformTextInput;
use flui_platform_api::text_store::{
    CommitGate, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
    TextStoreObserver, TextStoreStatus,
};

#[derive(Default)]
struct Platform {
    allowed: parking_lot::Mutex<Vec<bool>>,
}

impl PlatformTextInput for Platform {
    fn set_ime_allowed(&self, allowed: bool) {
        self.allowed.lock().push(allowed);
    }

    fn set_ime_cursor_area(&self, _: Bounds<f64>) {}
}

struct OnDrop(Box<dyn Fn()>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        (self.0)();
    }
}

fn owner() -> (Rc<TextInputOwner>, Arc<Platform>) {
    let platform = Arc::new(Platform::default());
    (
        TextInputOwner::new(TextInputBackend::Push(platform.clone())),
        platform,
    )
}

fn client() -> TextInputClient {
    TextInputClient::new(InMemoryTextStore::new(""))
}

fn callback_client(on_drop: impl Fn() + 'static) -> TextInputClient {
    let probe = OnDrop(Box::new(on_drop));
    client().on_session_start(move || {
        let _keep_alive = &probe;
    })
}

fn replacement_destructor_may_replace_the_new_client() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let reentrant = handle.clone();
    let installed = Rc::new(Cell::new(None::<ClientToken>));
    let observed = installed.clone();
    let calls = platform.clone();
    handle
        .attach(callback_client(move || {
            assert_eq!(*calls.allowed.lock(), [true]);
            observed.set(Some(reentrant.attach(client()).expect("reentrant attach")));
        }))
        .expect("initial attach");
    let superseded = handle.attach(client()).expect("replacement");
    assert!(!owner.is_attached(superseded));
    assert!(owner.is_attached(installed.get().expect("destructor ran")));
    owner.dispatch(&flui_platform_api::ImeEvent::Commit("ok".into()));
    assert_eq!(*platform.allowed.lock(), [true]);
}

fn detached_destructor_can_open_the_next_connection() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let reentrant = handle.clone();
    let installed = Rc::new(Cell::new(None::<ClientToken>));
    let observed = installed.clone();
    let token = handle
        .attach(callback_client(move || {
            observed.set(Some(
                reentrant.attach(client()).expect("attach after detach"),
            ));
        }))
        .expect("initial attach");
    let _detached = handle.detach(token).expect("detach");
    assert!(owner.is_attached(installed.get().expect("destructor ran")));
    assert_eq!(*platform.allowed.lock(), [true, false, true]);
}

fn frame_retirement_releases_callback_without_borrowing_the_owner() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let reentrant = handle.clone();
    let dropped = Rc::new(Cell::new(false));
    let observed = dropped.clone();
    handle
        .attach(callback_client(move || {
            assert_eq!(reentrant.ensure_open(), Ok(()));
            observed.set(true);
        }))
        .expect("initial attach");
    owner.set_transaction_open(true);
    handle.attach(client()).expect("replacement in frame");
    assert!(dropped.get());
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
}

fn closed_owner_is_visible_during_callback_retirement() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let reentrant = handle.clone();
    let observed = Rc::new(Cell::new(false));
    let dropped = observed.clone();
    handle
        .attach(callback_client(move || {
            assert_eq!(reentrant.ensure_open(), Err(TextInputError::Closed));
            assert_eq!(reentrant.attach(client()), Err(TextInputError::Closed));
            dropped.set(true);
        }))
        .expect("initial attach");
    owner.close();
    assert!(observed.get());
    assert_eq!(*platform.allowed.lock(), [true, false]);
    owner.close();
}

fn retirement_can_release_the_last_external_owner() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let external = Rc::new(RefCell::new(Some(owner.clone())));
    let release = external.clone();
    let reentrant = handle.clone();
    handle
        .attach(callback_client(move || {
            drop(release.borrow_mut().take());
            assert_eq!(reentrant.ensure_open(), Ok(()));
        }))
        .expect("initial attach");
    drop(owner);
    handle
        .attach(client())
        .expect("replacement pins owner during retirement");
    assert!(external.borrow().is_none());
    assert_eq!(handle.ensure_open(), Err(TextInputError::OwnerGone));
    assert_eq!(*platform.allowed.lock(), [true, false]);
}

fn owner_drop_contains_callback_failure_with_a_hostile_payload() {
    struct Hostile;
    impl Drop for Hostile {
        fn drop(&mut self) {
            panic!("panic payload retirement");
        }
    }
    let (owner, platform) = owner();
    let handle = owner.handle();
    handle
        .attach(callback_client(|| std::panic::panic_any(Hostile)))
        .expect("initial attach");
    drop(owner);
    assert_eq!(handle.ensure_open(), Err(TextInputError::OwnerGone));
    assert_eq!(*platform.allowed.lock(), [true, false]);
}

struct DroppingStore<P = OnDrop> {
    inner: Rc<InMemoryTextStore>,
    _probe: P,
}

impl<P> TextStore for DroppingStore<P> {
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

fn store_failure_wins_callback_failure_and_the_owner_recovers() {
    struct CallbackFailure;
    impl Drop for CallbackFailure {
        fn drop(&mut self) {
            panic!("callback failure payload retirement");
        }
    }
    struct StoreFailure;
    impl Drop for StoreFailure {
        fn drop(&mut self) {
            panic!("store failure payload retirement");
        }
    }
    let (owner, platform) = owner();
    let handle = owner.handle();
    let store: Rc<dyn TextStore> = Rc::new(DroppingStore {
        inner: InMemoryTextStore::new(""),
        _probe: OnDrop(Box::new(|| std::panic::panic_any(StoreFailure))),
    });
    let probe = OnDrop(Box::new(|| std::panic::panic_any(CallbackFailure)));
    let token = handle
        .attach(TextInputClient::new(store).on_session_start(move || {
            let _keep_alive = &probe;
        }))
        .expect("initial attach");
    let failure =
        catch_unwind(AssertUnwindSafe(|| handle.detach(token))).expect_err("retirement panics");
    // The payloads panic when dropped: retain the failure before asserting.
    let store_won = failure.is::<StoreFailure>();
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(
        store_won,
        "the store retires first and its opaque payload wins"
    );
    let next = handle.attach(client()).expect("recovery attach");
    assert!(owner.is_attached(next));
    owner.dispatch(&flui_platform_api::ImeEvent::Commit("recovered".into()));
    assert_eq!(*platform.allowed.lock(), [true, false, true]);
}

/// A client whose store and session callback record their destruction.
fn recording_client(log: &Rc<RefCell<Vec<&'static str>>>) -> TextInputClient {
    let store_log = Rc::clone(log);
    let store: Rc<dyn TextStore> = Rc::new(DroppingStore {
        inner: InMemoryTextStore::new(""),
        _probe: OnDrop(Box::new(move || store_log.borrow_mut().push("store"))),
    });
    let callback_log = Rc::clone(log);
    let probe = OnDrop(Box::new(move || callback_log.borrow_mut().push("callback")));
    TextInputClient::new(store).on_session_start(move || {
        let _keep_alive = &probe;
    })
}

fn client_retires_its_store_before_its_callback() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let log = Rc::new(RefCell::new(Vec::new()));
    let token = handle.attach(recording_client(&log)).expect("attach");
    let _detached = handle.detach(token).expect("detach");
    assert_eq!(*log.borrow(), ["store", "callback"], "detach");
    log.borrow_mut().clear();
    handle.attach(recording_client(&log)).expect("attach");
    handle.attach(client()).expect("replacement");
    assert_eq!(*log.borrow(), ["store", "callback"], "replacement");
    log.borrow_mut().clear();
    handle.attach(recording_client(&log)).expect("attach");
    owner.close();
    assert_eq!(*log.borrow(), ["store", "callback"], "close");
}

fn failed_anchor_preserves_other_retired_store_grants() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let first = InMemoryTextStore::new("");
    let second = InMemoryTextStore::new("");
    handle
        .attach(TextInputClient::new(first.clone()))
        .expect("first attach");
    owner.set_transaction_open(true);
    let _queued = first
        .request_lock(
            LockGrant::read(|_| panic!("first grant")),
            LockTiming::Async,
        )
        .expect("queued failure");
    handle
        .attach(TextInputClient::new(second.clone()))
        .expect("second attach");
    let ran = Rc::new(Cell::new(false));
    let observed = ran.clone();
    let _queued = second
        .request_lock(
            LockGrant::read(move |_| observed.set(true)),
            LockTiming::Async,
        )
        .expect("queued tail");
    handle.attach(client()).expect("retire second store");
    owner.set_transaction_open(false);
    let failure = catch_unwind(AssertUnwindSafe(|| owner.run_deferred_grants()))
        .expect_err("first grant panics");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("first grant")
    );
    assert!(!ran.get());
    owner.run_deferred_grants();
    assert!(ran.get(), "the next anchor delivers the accepted tail");
}

fn reopened_frame_preserves_older_grants_before_new_retirements() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let first = InMemoryTextStore::new("");
    handle
        .attach(TextInputClient::new(first.clone()))
        .expect("first attach");
    owner.set_transaction_open(true);
    let log = Rc::new(RefCell::new(Vec::new()));
    let reentrant = handle.clone();
    let opening = Rc::downgrade(&owner);
    let observed = log.clone();
    let _queued = first
        .request_lock(
            LockGrant::read(move |_| {
                opening
                    .upgrade()
                    .expect("live owner")
                    .set_transaction_open(true);
                let newer = InMemoryTextStore::new("");
                reentrant
                    .attach(TextInputClient::new(newer.clone()))
                    .expect("new attach");
                let _queued = newer
                    .request_lock(
                        LockGrant::read(move |_| observed.borrow_mut().push("newer")),
                        LockTiming::Async,
                    )
                    .expect("new queue");
                reentrant.attach(client()).expect("retire newer");
            }),
            LockTiming::Async,
        )
        .expect("queued frame opener");
    let observed = log.clone();
    let _queued = first
        .request_lock(
            LockGrant::read(move |_| observed.borrow_mut().push("first tail")),
            LockTiming::Async,
        )
        .expect("first tail");
    for label in ["older retired", "older active"] {
        let older = InMemoryTextStore::new("");
        handle
            .attach(TextInputClient::new(older.clone()))
            .expect("older attach");
        let observed = log.clone();
        let _queued = older
            .request_lock(
                LockGrant::read(move |_| observed.borrow_mut().push(label)),
                LockTiming::Async,
            )
            .expect("older queue");
    }
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
    assert!(log.borrow().is_empty());
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
    assert_eq!(
        *log.borrow(),
        ["first tail", "older retired", "older active", "newer"]
    );
}

struct CountingStore {
    inner: Rc<InMemoryTextStore>,
    runs: Cell<usize>,
}

impl TextStore for CountingStore {
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
        self.runs.set(self.runs.get() + 1);
        self.inner.run_deferred_grants()
    }
    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

fn active_store_that_reopens_the_frame_runs_once_per_anchor() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let inner = InMemoryTextStore::new("");
    let store = Rc::new(CountingStore {
        inner: inner.clone(),
        runs: Cell::new(0),
    });
    handle
        .attach(TextInputClient::new(store.clone()))
        .expect("attach");
    owner.set_transaction_open(true);
    let opening = Rc::downgrade(&owner);
    let _queued = inner
        .request_lock(
            LockGrant::read(move |_| {
                opening
                    .upgrade()
                    .expect("live owner")
                    .set_transaction_open(true);
            }),
            LockTiming::Async,
        )
        .expect("queued frame opener");
    let tail = Rc::new(Cell::new(0));
    let observed = tail.clone();
    let _queued = inner
        .request_lock(
            LockGrant::read(move |_| observed.set(observed.get() + 1)),
            LockTiming::Async,
        )
        .expect("queued tail");
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
    assert_eq!((store.runs.get(), tail.get()), (1, 0));
    owner.set_transaction_open(false);
    owner.run_deferred_grants();
    assert_eq!(
        (store.runs.get(), tail.get()),
        (2, 1),
        "the still-active store runs once at the next anchor"
    );
}

fn closing_during_a_grant_cancels_the_tail_and_preserves_the_first_failure() {
    let (owner, _) = owner();
    let handle = owner.handle();
    // Each client is detached rather than replaced: a replacement commits
    // the outgoing composition, a commit the close owes its store, which
    // would run that store's queue.
    let first = InMemoryTextStore::new("");
    let first_token = handle
        .attach(TextInputClient::new(first.clone()))
        .expect("first attach");
    owner.set_transaction_open(true);
    let closing = Rc::downgrade(&owner);
    let _queued = first
        .request_lock(
            LockGrant::read(move |_| {
                closing.upgrade().expect("live owner").close();
                panic!("grant after close");
            }),
            LockTiming::Async,
        )
        .expect("queued close");
    let _ = handle.detach(first_token).expect("retire first store");
    let delivered = Rc::new(Cell::new(false));
    for label in ["second store", "third store"] {
        let inner = InMemoryTextStore::new("");
        let token = handle
            .attach(TextInputClient::new(Rc::new(DroppingStore {
                inner: inner.clone(),
                _probe: OnDrop(Box::new(move || panic!("{label}"))),
            })))
            .expect("attach tail store");
        let observed = delivered.clone();
        let _queued = inner
            .request_lock(
                LockGrant::read(move |_| observed.set(true)),
                LockTiming::Async,
            )
            .expect("queued tail");
        let _ = handle.detach(token).expect("retire tail store");
    }
    owner.set_transaction_open(false);
    let failure =
        catch_unwind(AssertUnwindSafe(|| owner.run_deferred_grants())).expect_err("grant failure");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("grant after close")
    );
    assert!(!delivered.get(), "terminal close cancels accepted tail");
    assert_eq!(handle.ensure_open(), Err(TextInputError::Closed));
    assert_eq!(owner.run_deferred_grants(), 0);
}

/// A platform capability that records whether its destructor ran during an
/// unwind.
struct UnwindProbePlatform(Arc<parking_lot::Mutex<Option<bool>>>);

impl PlatformTextInput for UnwindProbePlatform {
    fn set_ime_allowed(&self, _: bool) {}
    fn set_ime_cursor_area(&self, _: Bounds<f64>) {}
}

impl Drop for UnwindProbePlatform {
    fn drop(&mut self) {
        *self.0.lock() = Some(std::thread::panicking());
    }
}

/// A store that closes its owner while the owner installs its commit gate.
struct ClosingStore {
    inner: Rc<InMemoryTextStore>,
    owner: std::rc::Weak<TextInputOwner>,
}

impl TextStore for ClosingStore {
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
        if let Some(owner) = self.owner.upgrade() {
            owner.close();
        }
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

/// The close the store runs releases the platform capability: attach holds no
/// clone of its own across the store, so a failing rejection of the client
/// cannot destroy the backend during its unwind.
fn store_closing_its_owner_while_gated_releases_the_platform_outside_the_unwind() {
    let released = Arc::new(parking_lot::Mutex::new(None));
    let platform: Arc<dyn PlatformTextInput> = Arc::new(UnwindProbePlatform(Arc::clone(&released)));
    let owner = TextInputOwner::new(TextInputBackend::Push(platform));
    let handle = owner.handle();
    let store = Rc::new(ClosingStore {
        inner: InMemoryTextStore::new(""),
        owner: Rc::downgrade(&owner),
    });
    let failure = catch_unwind(AssertUnwindSafe(|| {
        handle.attach(callback_client_with_store(store, || {
            panic!("rejected client retirement");
        }))
    }))
    .expect_err("the rejected client's retirement fails");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("rejected client retirement")
    );
    assert_eq!(
        *released.lock(),
        Some(false),
        "the close released the platform, not the unwind"
    );
    assert_eq!(handle.ensure_open(), Err(TextInputError::Closed));
}

/// A store that closes its owner when it is destroyed.
struct CloseOnDropStore {
    inner: Rc<InMemoryTextStore>,
    owner: std::rc::Weak<TextInputOwner>,
}

impl TextStore for CloseOnDropStore {
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

impl Drop for CloseOnDropStore {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.close();
        }
    }
}

/// A replaced or detached client whose store closes the owner and whose
/// callback then panics: the owner's close releases the platform, and no
/// clone attach or detach held is left for that unwind to destroy.
fn retired_store_closing_its_owner_releases_the_platform_outside_the_unwind() {
    for replace in [true, false] {
        let released = Arc::new(parking_lot::Mutex::new(None));
        let platform: Arc<dyn PlatformTextInput> =
            Arc::new(UnwindProbePlatform(Arc::clone(&released)));
        let owner = TextInputOwner::new(TextInputBackend::Push(platform));
        let handle = owner.handle();
        let store = Rc::new(CloseOnDropStore {
            inner: InMemoryTextStore::new(""),
            owner: Rc::downgrade(&owner),
        });
        let token = handle
            .attach(callback_client_with_store(store, || {
                panic!("retired client callback");
            }))
            .expect("first client attaches");
        let failure = catch_unwind(AssertUnwindSafe(|| {
            if replace {
                handle.attach(client()).map(|_| ())
            } else {
                handle.detach(token).map(|_| ())
            }
        }))
        .expect_err("the retired client's callback fails");
        assert_eq!(
            flui_foundation::panic::payload_text(&*failure),
            Some("retired client callback")
        );
        flui_foundation::panic::retain_opaque_payload(failure);
        assert_eq!(
            *released.lock(),
            Some(false),
            "replace={replace}: the close released the platform, not the unwind"
        );
        assert_eq!(handle.ensure_open(), Err(TextInputError::Closed));
    }
}

/// The text-store host a pull-model window offers, recording what it was told.
#[derive(Default)]
struct PullHost(RefCell<Vec<bool>>);

impl flui_platform_api::text_store::TextStoreHost for PullHost {
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

/// Issue #1052 on a pull-model owner: a retired client's capture detaches
/// its own, already stale, token during replacement, detach and close. Each
/// sees `Stale` or `Closed`, not a borrow panic, and the host still hears
/// every focus change once, in order.
fn pull_host_hears_each_focus_change_when_a_capture_detaches_on_retirement() {
    let host = Rc::new(PullHost::default());
    let owner = TextInputOwner::new(TextInputBackend::Pull(host.clone()));
    let handle = owner.handle();
    let detaching_client = |answer: Rc<Cell<Option<Result<(), TextInputError>>>>| {
        let token = Rc::new(Cell::new(None::<ClientToken>));
        let reentrant = handle.clone();
        let own = Rc::clone(&token);
        let client = callback_client(move || {
            if let Some(token) = own.get() {
                answer.set(Some(reentrant.detach(token).map(|_| ())));
            }
        });
        (client, token)
    };

    let replaced = Rc::new(Cell::new(None));
    let (first, first_token) = detaching_client(Rc::clone(&replaced));
    first_token.set(Some(handle.attach(first).expect("first")));
    let second = handle.attach(client()).expect("replacement");
    assert_eq!(replaced.get(), Some(Ok(())), "the stale detach is answered");
    assert!(owner.is_attached(second));

    let detached = Rc::new(Cell::new(None));
    let (third, third_token) = detaching_client(Rc::clone(&detached));
    let third = handle.attach(third).expect("third");
    third_token.set(Some(third));
    let _ = handle.detach(third).expect("detach");
    assert_eq!(detached.get(), Some(Ok(())));

    let closed = Rc::new(Cell::new(None));
    let (last, last_token) = detaching_client(Rc::clone(&closed));
    last_token.set(Some(handle.attach(last).expect("last")));
    owner.close();
    assert_eq!(closed.get(), Some(Err(TextInputError::Closed)));
    assert_eq!(
        *host.0.borrow(),
        [true, true, true, false, true, false],
        "focus A, B, C, none, D, and none on close"
    );
}

fn callback_client_with_store(
    store: Rc<dyn TextStore>,
    on_drop: impl Fn() + 'static,
) -> TextInputClient {
    let probe = OnDrop(Box::new(on_drop));
    TextInputClient::new(store).on_session_start(move || {
        let _keep_alive = &probe;
    })
}

struct ReentrantCommitStore {
    inner: Rc<InMemoryTextStore>,
    callback: RefCell<Option<Box<dyn FnOnce()>>>,
}
impl TextStore for ReentrantCommitStore {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let result = self.inner.request_lock(grant, timing);
        let callback = self.callback.borrow_mut().take();
        if let Some(callback) = callback {
            callback();
        }
        result
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
struct FailingGateStore(Rc<InMemoryTextStore>);

impl TextStore for FailingGateStore {
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
        panic!("nested gate installation failure");
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.0.set_observer(observer);
    }
}

fn failed_nested_gate_reservation_preserves_the_valid_outer_attachment() {
    for competing_retirement in [false, true] {
        let (owner, _) = owner();
        let handle = owner.handle();
        let outgoing = Rc::new(ReentrantCommitStore {
            inner: InMemoryTextStore::new("outgoing"),
            callback: RefCell::new(None),
        });
        let capture = OnDrop(Box::new(move || {
            assert!(!competing_retirement, "later outgoing capture failure");
        }));
        handle
            .attach(
                TextInputClient::new(outgoing.clone()).on_session_start(move || {
                    let _keep_alive = &capture;
                }),
            )
            .expect("initial outgoing client");
        let reentrant = handle.clone();
        *outgoing.callback.borrow_mut() = Some(Box::new(move || {
            let _ = reentrant.attach(TextInputClient::new(Rc::new(FailingGateStore(
                InMemoryTextStore::new("rejected"),
            ))));
        }));
        let incoming = InMemoryTextStore::new("incoming");
        let token = handle
            .attach(TextInputClient::new(incoming.clone()))
            .expect("failed reservation did not admit a newer replacement");
        assert!(
            owner.is_attached(token),
            "valid outer client remains admitted"
        );
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner.dispatch(&flui_platform_api::ImeEvent::Commit("first".into()));
        }))
        .expect_err("owner reports contained gate failure on its next turn");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"nested gate installation failure"),
            "failed nested installation precedes competing outgoing retirement"
        );
        assert!(owner.is_attached(token));
        owner.dispatch(&flui_platform_api::ImeEvent::Commit("healthy".into()));
        let text = Rc::new(RefCell::new(String::new()));
        let read = Rc::clone(&text);
        incoming
            .request_lock(
                LockGrant::read(move |session| {
                    *read.borrow_mut() = session
                        .text(
                            flui_platform_api::text_store::Utf16Range::new(
                                flui_platform_api::text_store::Utf16Offset::ZERO,
                                session.document_len(),
                            )
                            .expect("document range"),
                        )
                        .expect("document text");
                }),
                LockTiming::Sync,
            )
            .expect("read admitted store");
        assert!(
            text.borrow().contains("healthy"),
            "later IME reaches valid outer store"
        );
    }
}

fn commit_reentry_preserves_the_newer_attachment() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let nested = Rc::new(Cell::new(None));
    let nested_store = InMemoryTextStore::new("newer");
    let incoming = nested_store.clone();
    let observed = nested.clone();
    let reentrant = handle.clone();
    let store = Rc::new(ReentrantCommitStore {
        inner: InMemoryTextStore::new("outgoing"),
        callback: RefCell::new(None),
    });
    handle
        .attach(TextInputClient::new(store.clone()))
        .expect("outgoing");
    *store.callback.borrow_mut() = Some(Box::new(move || {
        observed.set(Some(
            reentrant
                .attach(TextInputClient::new(incoming))
                .expect("nested attach"),
        ));
    }));
    assert_eq!(handle.attach(client()), Err(TextInputError::Superseded));
    assert!(owner.is_attached(nested.get().expect("nested token")));
    owner.dispatch(&flui_platform_api::ImeEvent::Commit("!".into()));
    let text = Rc::new(RefCell::new(String::new()));
    let read = text.clone();
    let _ = nested_store
        .request_lock(
            LockGrant::read(move |session| {
                *read.borrow_mut() = session
                    .text(
                        flui_platform_api::text_store::Utf16Range::new(
                            flui_platform_api::text_store::Utf16Offset::ZERO,
                            session.document_len(),
                        )
                        .expect("ordered range"),
                    )
                    .expect("document text");
            }),
            LockTiming::Sync,
        )
        .expect("read");
    assert!(
        text.borrow().contains('!'),
        "subsequent IME events reach the newer client"
    );
}

#[test]
fn text_input_retirement_allows_reentry_and_preserves_recovery() {
    let cases: &[(&str, fn())] = &[
        (
            "failed nested gate reservation",
            failed_nested_gate_reservation_preserves_the_valid_outer_attachment,
        ),
        (
            "commit reentry",
            commit_reentry_preserves_the_newer_attachment,
        ),
        (
            "replacement",
            replacement_destructor_may_replace_the_new_client,
        ),
        ("detach", detached_destructor_can_open_the_next_connection),
        (
            "frame retirement",
            frame_retirement_releases_callback_without_borrowing_the_owner,
        ),
        ("close", closed_owner_is_visible_during_callback_retirement),
        (
            "owner release",
            retirement_can_release_the_last_external_owner,
        ),
        (
            "owner drop",
            owner_drop_contains_callback_failure_with_a_hostile_payload,
        ),
        (
            "competing failures",
            store_failure_wins_callback_failure_and_the_owner_recovers,
        ),
        ("field order", client_retires_its_store_before_its_callback),
        (
            "failed anchor",
            failed_anchor_preserves_other_retired_store_grants,
        ),
        (
            "reopened frame",
            reopened_frame_preserves_older_grants_before_new_retirements,
        ),
        (
            "active store reopens frame",
            active_store_that_reopens_the_frame_runs_once_per_anchor,
        ),
        (
            "close during grant",
            closing_during_a_grant_cancels_the_tail_and_preserves_the_first_failure,
        ),
        (
            "store closes owner while gated",
            store_closing_its_owner_while_gated_releases_the_platform_outside_the_unwind,
        ),
        (
            "retired store closes owner",
            retired_store_closing_its_owner_releases_the_platform_outside_the_unwind,
        ),
        (
            "pull host and reentrant retirement",
            pull_host_hears_each_focus_change_when_a_capture_detaches_on_retirement,
        ),
    ];
    let failed = RefCell::new(Vec::new());
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
            failed.borrow_mut().push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(
        failed.borrow().is_empty(),
        "failed cases: {:?}",
        failed.borrow()
    );
}

/// One field of a store whose destruction panics twice: dropping it at all
/// after a failure, or during an unwind, aborts the process.
struct AggregateField(Arc<AtomicUsize>);

impl Drop for AggregateField {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("aggregate store field");
    }
}

fn aggregate_store(drops: &Arc<AtomicUsize>) -> Rc<dyn TextStore> {
    Rc::new(DroppingStore {
        inner: InMemoryTextStore::new(""),
        _probe: (
            AggregateField(Arc::clone(drops)),
            AggregateField(Arc::clone(drops)),
        ),
    })
}

/// A session callback whose capture double-panics when destroyed.
fn aggregate_callback(client: TextInputClient, drops: &Arc<AtomicUsize>) -> TextInputClient {
    let capture = (
        AggregateField(Arc::clone(drops)),
        AggregateField(Arc::clone(drops)),
    );
    client.on_session_start(move || {
        let _keep_alive = &capture;
    })
}

fn panicking_store(message: &'static str) -> Rc<dyn TextStore> {
    Rc::new(DroppingStore {
        inner: InMemoryTextStore::new(""),
        _probe: OnDrop(Box::new(move || panic!("{message}"))),
    })
}

fn callback_after_a_failed_store_is_retained() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let drops = Arc::new(AtomicUsize::new(0));
    let token = handle
        .attach(aggregate_callback(
            TextInputClient::new(panicking_store("store retirement")),
            &drops,
        ))
        .expect("initial attach");
    let failure =
        catch_unwind(AssertUnwindSafe(|| handle.detach(token))).expect_err("retirement panics");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("store retirement")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(drops.load(Ordering::Relaxed), 0, "the callback is retained");
    let next = handle.attach(client()).expect("recovery attach");
    assert!(owner.is_attached(next));
    assert_eq!(*platform.allowed.lock(), [true, false, true]);
}

/// Closes its owner while the owner installs the commit gate.
struct ProbedClosingStore {
    inner: Rc<InMemoryTextStore>,
    owner: std::rc::Weak<TextInputOwner>,
    _probe: OnDrop,
}

impl TextStore for ProbedClosingStore {
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
        if let Some(owner) = self.owner.upgrade() {
            owner.close();
        }
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

fn client_rejected_by_a_closing_store_is_retained_after_its_failure() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let drops = Arc::new(AtomicUsize::new(0));
    let store: Rc<dyn TextStore> = Rc::new(ProbedClosingStore {
        inner: InMemoryTextStore::new(""),
        owner: Rc::downgrade(&owner),
        _probe: OnDrop(Box::new(|| panic!("rejected store retirement"))),
    });
    let rejected = aggregate_callback(TextInputClient::new(store), &drops);
    let failure = catch_unwind(AssertUnwindSafe(|| handle.attach(rejected)))
        .expect_err("the rejected store's failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("rejected store retirement")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(drops.load(Ordering::Relaxed), 0, "the callback is retained");
    assert_eq!(handle.ensure_open(), Err(TextInputError::Closed));
    assert_eq!(handle.attach(client()), Err(TextInputError::Closed));
    assert!(platform.allowed.lock().is_empty());
}

fn owner_dropped_during_an_unwind_retains_its_store() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let drops = Arc::new(AtomicUsize::new(0));
    handle
        .attach(TextInputClient::new(aggregate_store(&drops)))
        .expect("initial attach");
    let failure = catch_unwind(AssertUnwindSafe(move || {
        let _owner = owner;
        panic!("outer failure");
    }))
    .expect_err("the outer failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("outer failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(drops.load(Ordering::Relaxed), 0, "the store is retained");
    assert_eq!(handle.ensure_open(), Err(TextInputError::OwnerGone));
    assert_eq!(*platform.allowed.lock(), [true, false]);
}

/// A preserving close retains the user's store but releases the platform
/// capability, a framework-owned handle that can keep the native window alive.
fn preserving_close_releases_the_platform() {
    use flui_interaction::__runtime::{CloseMode, close_text_input};

    let (owner, platform) = owner();
    let handle = owner.handle();
    let drops = Arc::new(AtomicUsize::new(0));
    handle
        .attach(TextInputClient::new(aggregate_store(&drops)))
        .expect("initial attach");
    close_text_input(&owner, CloseMode::PreservingFailure);
    assert_eq!(drops.load(Ordering::Relaxed), 0, "the store is retained");
    assert_eq!(*platform.allowed.lock(), [true, false]);
    assert_eq!(
        Arc::strong_count(&platform),
        1,
        "the open owner no longer holds the platform"
    );
    assert_eq!(handle.ensure_open(), Err(TextInputError::Closed));
    drop(owner);
}

/// Selects the single case a child process of the test below runs.
const RETENTION_CHILD: &str = "FLUI_TEXT_INPUT_RETENTION_CHILD";
/// A child that ran its case to completion exits with this status, so a
/// filter that matched no test (status 0) does not pass for one.
const CHILD_COMPLETED: i32 = 86;

#[test]
fn text_input_owners_are_retained_after_a_failure_and_during_unwind() {
    let cases: &[(&str, fn())] = &[
        (
            "callback after a failed store",
            callback_after_a_failed_store_is_retained,
        ),
        (
            "client rejected by a closing store",
            client_rejected_by_a_closing_store_is_retained_after_its_failure,
        ),
        (
            "owner drop during unwind",
            owner_dropped_during_an_unwind_retains_its_store,
        ),
        (
            "preserving close releases the platform",
            preserving_close_releases_the_platform,
        ),
    ];
    if let Ok(selected) = std::env::var(RETENTION_CHILD) {
        let (_, case) = cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("a known retention case");
        case();
        std::process::exit(CHILD_COMPLETED);
    }
    // Without retention each case aborts, so each runs in its own process.
    let mut failures = Vec::new();
    for &(name, _) in cases {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "text_input_retirement::text_input_owners_are_retained_after_a_failure_and_during_unwind",
                "--nocapture",
            ])
            .env(RETENTION_CHILD, name)
            .env("RUST_BACKTRACE", "0")
            .output()
            .expect("run retention child");
        if output.status.code() != Some(CHILD_COMPLETED) {
            failures.push(format!(
                "{name}: {}
{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}
