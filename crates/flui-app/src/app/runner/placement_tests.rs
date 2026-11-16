//! A window's remembered placement through the desktop runner: an
//! application run on the headless platform, whose placement lives in the
//! storage the run's capability registry provides, on a clock the test turns.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::ManualClock;
use flui_foundation::geometry::{Point, Size};
use flui_platform::capability::ProviderContext;
use flui_platform::{
    PlatformWindow, Storage, StorageName, Unsupported, WindowInteraction, WindowMode,
    WindowPlacement,
};
use flui_testing::storage::MemoryStorage;
use flui_view::persist::{StorageCapability, StorageHandle};

use crate::app::placement_recorder::RECALL_BOUND;
use crate::app::runner::host::APP_RUNTIME;
use crate::app::{AppConfig, Application};
use crate::testing::{HeadlessRun, HeadlessRunHandle};

const NAME: StorageName = StorageName::machine_local("window-main");

/// What the earlier run left: a window of 640 × 480, which the configuration's
/// default size is not.
fn stored() -> WindowPlacement {
    WindowPlacement::new(
        None,
        Some(Point::new(40.0, 30.0)),
        Size::new(640.0, 480.0),
        WindowMode::Normal,
        1.0,
    )
}

/// An application that remembers its main window's placement, its storage
/// the application's own `storage`, with no data directory of the host's.
fn remembering(storage: &MemoryStorage) -> Application {
    let provided = storage.clone();
    Application::new()
        .config(AppConfig::new().restore_placement(NAME))
        .capability::<StorageCapability>(move |_: &ProviderContext<'_>| {
            Ok(StorageHandle::new(Arc::new(provided.clone())))
        })
}

/// Start `application` with no window, turn the run's placement clock into
/// a manual one, and ask for the main window.
fn start_on_manual_clock(application: Application) -> (HeadlessRunHandle, ManualClock) {
    let run = HeadlessRun::start_resident(application, |_| flui_widgets::SizedBox::shrink());
    let clock = ManualClock::new();
    let placement_clock = clock.clone();
    APP_RUNTIME.with(|slot| {
        slot.borrow().placement.set_clock(Rc::new(placement_clock));
    });
    let ingress = APP_RUNTIME
        .with(|slot| slot.borrow().main_ingress.clone())
        .expect("a running loop has its main window's ingress");
    assert!(
        ingress.request_native_show(),
        "the running loop takes the reopen"
    );
    run.run_owner_turn();
    (run, clock)
}

fn content_size(run: &HeadlessRunHandle) -> Size<f64> {
    let windows = run.open_windows();
    let [window] = windows.as_slice() else {
        panic!("one window is open, not {}", windows.len());
    };
    window.state().content_size.get()
}

/// The headless loop's idle wait woken at the manual clock's time: the
/// host gives the owner the turn its turn deadline asks for, if due.
fn turn_at(run: &HeadlessRunHandle, clock: &ManualClock) {
    run.run_owner_turn_at(flui_foundation::MonotonicClock::now(clock));
}

fn recording() -> usize {
    APP_RUNTIME.with(|slot| slot.borrow().placement.shared().live.borrow().len())
}

/// A window that remembers its placement reads the stored one once, before
/// it opens: installing the window, which starts the recorder from that
/// placement, asks storage for nothing more.
#[test]
fn a_remembered_placement_is_read_once_before_the_window_opens() {
    let storage = MemoryStorage::new();
    storage.put(&NAME, stored().to_bytes());
    let (run, _clock) = start_on_manual_clock(remembering(&storage));

    assert_eq!(run.open_windows().len(), 1, "the main window opened");
    assert_eq!(recording(), 1, "the installed window records its placement");
    assert_eq!(
        storage.reads(),
        1,
        "the placement is read before the window opens and not again"
    );
    run.join().expect("the run ends");
}

/// The application's own storage, which it provides as the
/// `StorageCapability`, is where its window's placement comes from and goes
/// to: the run has no data directory, so the host's storage has none to give.
#[test]
fn an_application_storage_keeps_its_window_placement() {
    let storage = MemoryStorage::new();
    storage.put(&NAME, stored().to_bytes());
    let (run, _clock) = start_on_manual_clock(remembering(&storage));
    assert_eq!(
        content_size(&run),
        Size::new(640.0, 480.0),
        "the window opened at the placement the application's storage held"
    );

    storage.put(&NAME, b"overwritten".to_vec());
    let windows = run.open_windows();
    windows[0].simulate_move(Point::new(200.0, 150.0));
    windows[0].simulate_close();
    run.run_owner_turn();
    let written = storage
        .contents(&NAME)
        .expect("the close wrote a placement");
    assert!(
        WindowPlacement::from_bytes(&written).is_ok(),
        "the closing window's placement went to the application's storage"
    );
    run.join().expect("the run ends");
}

/// A read storage has not answered holds the window until the read's bound
/// on the run's placement clock; the window then opens from its
/// configuration, and the answer that comes after is never applied to it.
#[test]
fn a_window_opens_at_its_default_when_the_read_does_not_answer_in_time() {
    let storage = MemoryStorage::new();
    storage.put(&NAME, stored().to_bytes());
    let held = storage.hold_reads();
    let (run, clock) = start_on_manual_clock(remembering(&storage));
    assert!(
        run.open_windows().is_empty(),
        "the window waits for its read"
    );
    let started = flui_foundation::MonotonicClock::now(&clock);
    assert_eq!(
        run.turn_deadline(),
        Some(started + RECALL_BOUND),
        "the owner gets a turn at the read's bound"
    );
    assert_eq!(
        run.wake_deadline(),
        None,
        "and no window is asked for a frame for it"
    );

    clock.advance(RECALL_BOUND);
    turn_at(&run, &clock);
    let default = content_size(&run);
    assert_ne!(default, Size::new(640.0, 480.0), "it opened at its default");

    drop(held);
    run.run_owner_turn();
    assert_eq!(
        content_size(&run),
        default,
        "an answer after the bound does not move the window"
    );
    run.join().expect("the run ends");
}

/// A window that opened at its default because its read did not answer
/// keeps what storage holds against a move the host does not report as the
/// user's, and writes the one whose report says the user is placing it.
#[test]
fn a_window_that_could_not_read_its_placement_writes_only_what_the_user_places() {
    let storage = MemoryStorage::new();
    storage.put(&NAME, stored().to_bytes());
    let held = storage.hold_reads();
    let (run, clock) = start_on_manual_clock(remembering(&storage));
    clock.advance(RECALL_BOUND);
    turn_at(&run, &clock);
    drop(held);
    run.run_owner_turn();
    let window = &run.open_windows()[0];
    let flushed = || {
        super::flush_all_by(std::time::Instant::now() + std::time::Duration::from_secs(1));
        storage
            .contents(&NAME)
            .and_then(|bytes| WindowPlacement::from_bytes(&bytes).ok())
    };

    window.simulate_move(Point::new(222.0, 111.0));
    assert_eq!(
        flushed(),
        Some(stored()),
        "a move the host does not report as the user's leaves storage alone"
    );

    window.simulate_interaction(WindowInteraction::UserPlacing);
    window.simulate_move(Point::new(333.0, 99.0));
    window.simulate_interaction(WindowInteraction::None);
    let written = flushed().expect("a placement is stored");
    assert_ne!(written, stored(), "the user's move is written");
    run.join().expect("the run ends");
}

/// A read answered inside the bound opens the window where it was, at the
/// turn the answer wakes.
#[test]
fn a_window_opens_where_it_was_when_the_read_answers_inside_its_bound() {
    let storage = MemoryStorage::new();
    storage.put(&NAME, stored().to_bytes());
    let held = storage.hold_reads();
    let (run, clock) = start_on_manual_clock(remembering(&storage));
    assert!(
        run.open_windows().is_empty(),
        "the window waits for its read"
    );

    clock.advance(RECALL_BOUND / 2);
    drop(held);
    run.run_owner_turn();
    assert_eq!(content_size(&run), Size::new(640.0, 480.0));
    run.join().expect("the run ends");
}

/// A storage provider that panics, or refuses, costs the window its
/// placement: the first window still opens, from its configuration, and is
/// not recorded.
#[test]
fn a_provider_that_fails_still_lets_the_first_window_open() {
    let panicking = Application::new()
        .config(AppConfig::new().restore_placement(NAME))
        .capability::<StorageCapability>(
            |_: &ProviderContext<'_>| -> Result<StorageHandle, Unsupported> {
                panic!("the application's storage provider fails")
            },
        );
    let refusing = Application::new()
        .config(AppConfig::new().restore_placement(NAME))
        .capability::<StorageCapability>(
            |_: &ProviderContext<'_>| -> Result<StorageHandle, Unsupported> {
                Err(Unsupported::of::<StorageCapability>(
                    flui_platform::UnsupportedReason::NotOnThisPlatform,
                ))
            },
        );
    for (case, application) in [("panicking", panicking), ("refusing", refusing)] {
        let (run, _clock) = start_on_manual_clock(application);
        assert_eq!(run.open_windows().len(), 1, "{case}: the window opened");
        assert_eq!(recording(), 0, "{case}: and is not remembered");
        run.join().expect("the run ends");
    }
}

/// A name an open window remembers its placement under is that window's: a
/// second window asking for it opens from its configuration and writes
/// nothing under it, until the first one closes.
#[test]
fn two_windows_never_share_one_placement() {
    let storage = MemoryStorage::new();
    let (run, _clock) = start_on_manual_clock(remembering(&storage));
    assert_eq!(recording(), 1);
    let config = AppConfig::new().restore_placement(NAME);
    assert!(
        super::RememberedPlacement::recall(&config).is_none(),
        "the open window holds the name"
    );

    run.open_windows()[0].simulate_close();
    run.run_owner_turn();
    let reopened = super::RememberedPlacement::recall(&config);
    assert!(reopened.is_some(), "a closed window frees its name");
    drop(reopened);
    run.join().expect("the run ends");
}

/// The provider the host acquires from is asked once per window opening,
/// as a widget's acquisition asks it: the registry keeps no handle.
#[test]
fn the_host_acquires_the_storage_once_per_window() {
    let provided = MemoryStorage::new();
    let asked = Rc::new(Cell::new(0));
    let counted = Rc::clone(&asked);
    let application = Application::new()
        .config(AppConfig::new().restore_placement(NAME))
        .capability::<StorageCapability>(move |_: &ProviderContext<'_>| {
            counted.set(counted.get() + 1);
            Ok(StorageHandle::new(
                Arc::new(provided.clone()) as Arc<dyn Storage>
            ))
        });
    let (run, _clock) = start_on_manual_clock(application);
    assert_eq!(asked.get(), 1);
    run.join().expect("the run ends");
}

/// A close that finds the placement state busy is carried over to the next
/// owner turn, not dropped: the window's name is freed and its last
/// placement written then. A close reached from inside a borrow of the
/// application runtime does not need the runtime at all.
#[test]
fn a_close_that_finds_the_state_busy_is_carried_over() {
    let storage = MemoryStorage::new();
    let (run, _clock) = start_on_manual_clock(remembering(&storage));
    let shared = APP_RUNTIME.with(|slot| slot.borrow().placement.shared());
    let address = shared.live.borrow()[0].address;
    let close = super::PlacementClose::of(Rc::clone(&shared), address);

    let held = shared.live.borrow();
    let closing = close.finish();
    drop(held);
    assert_eq!(closing, super::Closing::Deferred);
    assert_eq!(recording(), 1, "nothing is dropped meanwhile");
    assert_eq!(
        run.turn_deadline(),
        None,
        "a carried close is ready now, so it is no deadline"
    );
    // It asked the owner for a turn through the signal instead.
    run.run_owner_turn();
    assert_eq!(recording(), 0, "the next turn carried the close out");
    assert!(
        shared.claims.0.borrow().is_empty(),
        "and freed the window's name"
    );

    let outcome = APP_RUNTIME.with(|slot| {
        let _held = slot.borrow_mut();
        close.finish()
    });
    assert_eq!(
        outcome,
        super::Closing::NotKept,
        "inside the runtime's borrow the close answers, carried out already"
    );
    run.join().expect("the run ends");
}

/// A store whose answers reach the owner as posted jobs: the provider hands
/// it the owner's poster, and a write lands when the job runs there.
struct PostedStore {
    inner: MemoryStorage,
    poster: flui_platform::capability::OwnerPoster,
}

/// A write's answer and the waker of whoever waits for it.
type WriteSlot = Arc<
    std::sync::Mutex<(
        Option<Result<flui_platform::StoredVersion, flui_platform::StorageError>>,
        Option<std::task::Waker>,
    )>,
>;

struct PostedWrite(WriteSlot);

impl std::future::Future for PostedWrite {
    type Output = Result<flui_platform::StoredVersion, flui_platform::StorageError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut slot = self
            .0
            .lock()
            .expect("BUG: the slot is never held across a panic");
        if let Some(answer) = slot.0.take() {
            std::task::Poll::Ready(answer)
        } else {
            slot.1 = Some(ctx.waker().clone());
            std::task::Poll::Pending
        }
    }
}

impl Storage for PostedStore {
    fn read(
        &self,
        name: &StorageName,
        limit: u64,
    ) -> flui_platform::StorageFuture<flui_platform::Stored> {
        self.inner.read(name, limit)
    }

    fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        _mode: flui_platform::WriteMode,
    ) -> flui_platform::StorageFuture<flui_platform::StoredVersion> {
        let slot: WriteSlot = Arc::default();
        let mut job = Some((self.inner.clone(), *name, bytes, Arc::clone(&slot)));
        let poster = self.poster.clone();
        std::thread::spawn(move || {
            let _ = poster.post(move || {
                let Some((inner, name, bytes, answer)) = job.take() else {
                    return;
                };
                let version = flui_platform::StoredVersion::of_bytes(&bytes);
                inner.put(&name, bytes);
                let waker = {
                    let mut slot = answer
                        .lock()
                        .expect("BUG: the slot is never held across a panic");
                    slot.0 = Some(Ok(version));
                    slot.1.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
        });
        Box::pin(PostedWrite(slot))
    }

    fn copy_aside(
        &self,
        name: &StorageName,
    ) -> flui_platform::StorageFuture<flui_platform::CopiedAside> {
        self.inner.copy_aside(name)
    }
}

/// The last writes at the end of the loop take the answers a provider posts
/// back to the owner: the wait runs the host's posted jobs, so a store that
/// answers that way is written within its time, not reported lost at the
/// deadline.
#[test]
fn the_final_flush_takes_answers_posted_to_the_owner() {
    let inner = MemoryStorage::new();
    let provided = inner.clone();
    let application = Application::new()
        .config(AppConfig::new().restore_placement(NAME))
        .capability::<StorageCapability>(move |ctx: &ProviderContext<'_>| {
            Ok(StorageHandle::new(Arc::new(PostedStore {
                inner: provided.clone(),
                poster: ctx.owner().clone(),
            }) as Arc<dyn Storage>))
        });
    let (run, _clock) = start_on_manual_clock(application);
    run.open_windows()[0].simulate_move(Point::new(321.0, 123.0));

    let started = std::time::Instant::now();
    super::flush_all_by(started + std::time::Duration::from_secs(3));
    let spent = started.elapsed();
    let stored = inner
        .contents(&NAME)
        .and_then(|bytes| WindowPlacement::from_bytes(&bytes).ok());
    assert!(stored.is_some(), "the posted answer was taken");
    assert!(
        spent < std::time::Duration::from_secs(2),
        "within its time, not at the deadline: {spent:?}"
    );
    run.join().expect("the run ends");
}

/// The window root of a session-end test: says whether it heard the
/// session end.
#[derive(Clone)]
struct HearsSessionEnd(Arc<std::sync::atomic::AtomicBool>);

struct HearsSessionEndState {
    heard: Arc<std::sync::atomic::AtomicBool>,
    notices: Option<flui_view::HostNoticeSubscription>,
}

impl flui_view::StatefulView for HearsSessionEnd {
    type State = HearsSessionEndState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> Self::State {
        HearsSessionEndState {
            heard: Arc::clone(&self.0),
            notices: None,
        }
    }
}

impl flui_view::ViewState<HearsSessionEnd> for HearsSessionEndState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let heard = Arc::clone(&self.heard);
        self.notices = Some(
            ctx.lifecycle_handle()
                .expect("a bound lifecycle")
                .subscribe_notices(move |notice| {
                    if matches!(notice, flui_platform::HostNotice::SessionEnded) {
                        heard.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                })
                .expect("the lifecycle is open"),
        );
    }

    fn build(
        &self,
        _view: &HearsSessionEnd,
        _ctx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(10.0)
    }
}

impl flui_view::View for HearsSessionEnd {
    fn create_element(self: Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// A store that never answers costs the session's end no more than the
/// placements' own bound, after the application's work: the application
/// still hears the session end with its budget.
#[test]
fn a_store_that_never_answers_leaves_the_session_end_its_budget() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let inner = MemoryStorage::new();
    let held = inner.clone();
    let heard = Arc::new(AtomicBool::new(false));
    let store = OrderedStore {
        inner,
        ending: Arc::default(),
        heard: Arc::clone(&heard),
        written_before_heard: Arc::default(),
    };
    let (ending, early) = (
        Arc::clone(&store.ending),
        Arc::clone(&store.written_before_heard),
    );
    let provided = Arc::new(store);
    let application = Application::new()
        .config(AppConfig::new().restore_placement(NAME))
        .capability::<StorageCapability>(move |_: &ProviderContext<'_>| {
            Ok(StorageHandle::new(Arc::clone(&provided) as Arc<dyn Storage>))
        });
    let run = HeadlessRun::start(application, HearsSessionEnd(Arc::clone(&heard)));
    run.run_owner_turn();
    assert_eq!(run.open_windows().len(), 1, "the window opened");
    // The placement the window opened at is stored; from here the store never
    // answers. The move falls inside the write interval, so it is held, and
    // the session end is what hands it to storage.
    let barrier = held.hold_commits();
    run.open_windows()[0].simulate_move(Point::new(222.0, 111.0));

    ending.store(true, Ordering::SeqCst);
    let started = std::time::Instant::now();
    run.end_session(flui_platform::SessionEndPhase::Ending);
    let spent = started.elapsed();
    assert!(
        heard.load(Ordering::SeqCst),
        "the application heard the session end"
    );
    assert!(
        !early.load(Ordering::SeqCst),
        "no placement was written at the session end before the application heard it"
    );
    assert!(
        spent < super::SESSION_END_PLACEMENT_BOUND + std::time::Duration::from_secs(1),
        "the placements took their own bound, not the budget: {spent:?}"
    );
    drop(barrier);
    run.join().expect("the run ends");
}

/// A store that says whether a write reached it during a session end before
/// the application heard the session end.
struct OrderedStore {
    inner: MemoryStorage,
    ending: Arc<std::sync::atomic::AtomicBool>,
    heard: Arc<std::sync::atomic::AtomicBool>,
    written_before_heard: Arc<std::sync::atomic::AtomicBool>,
}

impl Storage for OrderedStore {
    fn read(
        &self,
        name: &StorageName,
        limit: u64,
    ) -> flui_platform::StorageFuture<flui_platform::Stored> {
        self.inner.read(name, limit)
    }

    fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        mode: flui_platform::WriteMode,
    ) -> flui_platform::StorageFuture<flui_platform::StoredVersion> {
        use std::sync::atomic::Ordering;
        if self.ending.load(Ordering::SeqCst) && !self.heard.load(Ordering::SeqCst) {
            self.written_before_heard.store(true, Ordering::SeqCst);
        }
        self.inner.publish(name, bytes, mode)
    }

    fn copy_aside(
        &self,
        name: &StorageName,
    ) -> flui_platform::StorageFuture<flui_platform::CopiedAside> {
        self.inner.copy_aside(name)
    }
}
