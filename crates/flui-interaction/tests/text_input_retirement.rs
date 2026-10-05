//! Owner retirement through the same public handles used by mounted text fields.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Bounds;
use flui_interaction::{ClientToken, TextInputClient, TextInputError, TextInputOwner};
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
    (TextInputOwner::new(Some(platform.clone())), platform)
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

fn callback_failure_wins_store_failure_and_the_owner_recovers() {
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
    assert!(
        failure.is::<CallbackFailure>(),
        "the first opaque payload wins"
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    let next = handle.attach(client()).expect("recovery attach");
    assert!(owner.is_attached(next));
    owner.dispatch(&flui_platform_api::ImeEvent::Commit("recovered".into()));
    assert_eq!(*platform.allowed.lock(), [true, false, true]);
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

fn closing_during_a_grant_cancels_the_tail_and_preserves_the_first_failure() {
    let (owner, _) = owner();
    let handle = owner.handle();
    let first = InMemoryTextStore::new("");
    handle
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
    let delivered = Rc::new(Cell::new(false));
    for label in ["second store", "third store"] {
        let inner = InMemoryTextStore::new("");
        handle
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
    }
    handle.attach(client()).expect("retire third store");
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

#[test]
fn text_input_retirement_allows_reentry_and_preserves_recovery() {
    let cases: &[(&str, fn())] = &[
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
            callback_failure_wins_store_failure_and_the_owner_recovers,
        ),
        (
            "failed anchor",
            failed_anchor_preserves_other_retired_store_grants,
        ),
        (
            "reopened frame",
            reopened_frame_preserves_older_grants_before_new_retirements,
        ),
        (
            "close during grant",
            closing_during_a_grant_cancels_the_tail_and_preserves_the_first_failure,
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

fn store_after_a_failed_callback_is_retained() {
    let (owner, platform) = owner();
    let handle = owner.handle();
    let drops = Arc::new(AtomicUsize::new(0));
    let probe = OnDrop(Box::new(|| panic!("callback retirement")));
    let token = handle
        .attach(
            TextInputClient::new(aggregate_store(&drops)).on_session_start(move || {
                let _keep_alive = &probe;
            }),
        )
        .expect("initial attach");
    let failure =
        catch_unwind(AssertUnwindSafe(|| handle.detach(token))).expect_err("retirement panics");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("callback retirement")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(drops.load(Ordering::Relaxed), 0, "the store is retained");
    let next = handle.attach(client()).expect("recovery attach");
    assert!(owner.is_attached(next));
    assert_eq!(*platform.allowed.lock(), [true, false, true]);
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

/// Selects the single case a child process of the test below runs.
const RETENTION_CHILD: &str = "FLUI_TEXT_INPUT_RETENTION_CHILD";
/// A child that ran its case to completion exits with this status, so a
/// filter that matched no test (status 0) does not pass for one.
const CHILD_COMPLETED: i32 = 86;

#[test]
fn text_input_owners_are_retained_after_a_failure_and_during_unwind() {
    let cases: &[(&str, fn())] = &[
        (
            "store after a failed callback",
            store_after_a_failed_callback_is_retained,
        ),
        (
            "owner drop during unwind",
            owner_dropped_during_an_unwind_retains_its_store,
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
