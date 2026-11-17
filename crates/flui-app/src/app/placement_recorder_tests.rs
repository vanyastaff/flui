//! A window's placement through storage: what a run recalls, when a recorder
//! writes, what a refused write leaves owed, and what the closing and ending
//! flushes add.

use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Waker};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use flui_platform::{AppTitleBar, WindowFrame, WindowOptions};
use flui_platform::{
    DisplayDescription, DisplayKey, DisplayRegistry, DisplaySnapshot, Storage, StorageError,
    StorageFuture, StorageName, Stored, WindowCapabilities, WindowGeometry, WindowInteraction,
    WindowMode, WindowPlacement, WindowState,
};
use flui_testing::storage::MemoryStorage;
use flui_view::persist::StorageHandle;

use super::{
    FlushState, HandOver, PlacementRecall, PlacementRecorder, RECALL_BOUND, Recalled,
    StoredPlacement, WRITE_ATTEMPTS, WRITE_INTERVAL, earliest_due, restore_into, settle,
};

const NAME: StorageName = StorageName::machine_local("window-main");

fn displays() -> DisplaySnapshot {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let display = DisplayDescription::new(
        id,
        "main",
        Bounds::new(Point::new(0, 0), Size::new(1920, 1080)),
        DevicePixelRatio::ONE,
    )
    .with_primary(true)
    .with_persistent_key("key-main");
    DisplaySnapshot::empty()
        .successor(vec![display])
        .expect("revision")
}

fn at(x: f64, y: f64) -> WindowState {
    WindowState::new(
        WindowGeometry::try_new(Some(Point::new(x, y)), Size::new(816.0, 639.0))
            .expect("a finite rectangle"),
        flui_platform::WindowExtent::try_new(Size::new(800.0, 600.0)).expect("a finite size"),
    )
}

struct Fixture {
    storage: MemoryStorage,
    recorder: PlacementRecorder,
    start: Instant,
    displays: DisplaySnapshot,
}

impl Fixture {
    fn new() -> Self {
        let storage = MemoryStorage::new();
        let recorder = PlacementRecorder::new(
            handle(&storage),
            NAME,
            StoredPlacement::Nothing,
            Arc::new(|| {}),
        );
        Self {
            storage,
            recorder,
            start: Instant::now(),
            displays: displays(),
        }
    }

    fn after(&self, by: Duration) -> Instant {
        self.start + by
    }

    fn observe(&self, state: &WindowState, after: Duration) {
        self.recorder
            .observe(state, &self.displays, self.after(after));
    }

    fn stored_offset(&self) -> Option<Point<f64>> {
        let bytes = self.storage.contents(&NAME)?;
        WindowPlacement::from_bytes(&bytes)
            .expect("a placement was stored")
            .offset
    }
}

const NO_FRAME: Size<f64> = Size::new(0.0, 0.0);

/// How long the settling tests wait for storage at most.
const BUDGET: Duration = Duration::from_secs(4);

fn handle(storage: &MemoryStorage) -> StorageHandle {
    StorageHandle::new(Arc::new(storage.clone()))
}

fn start_recall(storage: &MemoryStorage, now: Instant) -> PlacementRecall {
    PlacementRecall::start(&handle(storage), NAME, now, Waker::noop().clone())
}

/// A store whose `read` panics, as an application's override may.
struct PanickingReads;

impl Storage for PanickingReads {
    fn read(&self, _name: &StorageName, _limit: u64) -> StorageFuture<Stored> {
        panic!("the application's store fails to read");
    }

    fn publish(
        &self,
        _name: &StorageName,
        _bytes: Vec<u8>,
        _mode: flui_platform::WriteMode,
    ) -> StorageFuture<flui_platform::StoredVersion> {
        panic!("the application's store fails to write");
    }

    fn copy_aside(&self, _name: &StorageName) -> StorageFuture<flui_platform::CopiedAside> {
        Box::pin(std::future::ready(Err(StorageError::NotFound)))
    }
}

/// A recall case: its name, what storage holds, what a read fails with, and
/// what the recall settles as.
type RecallCase<'a> = (
    &'static str,
    Option<&'a [u8]>,
    Option<StorageError>,
    Recalled,
);

fn a_recall_reads_what_storage_answers_and_checks_it() {
    let now = Instant::now();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(40.0, 30.0)),
        Size::new(800.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    let stored_bytes = placement.to_bytes();
    let too_large = vec![b'x'; 5000];
    let cases: [RecallCase<'_>; 6] = [
        ("a first run has nothing", None, None, Recalled::Absent),
        (
            "an earlier run's placement",
            Some(&stored_bytes),
            None,
            Recalled::Placement(placement),
        ),
        (
            "another version of the format",
            Some(b"flui-window-placement/9\nnot ours\n"),
            None,
            Recalled::Unreadable,
        ),
        (
            "a value cut short",
            Some(b"flui-window-pla"),
            None,
            Recalled::Unreadable,
        ),
        (
            "a value past the read limit",
            Some(&too_large),
            None,
            Recalled::Unreadable,
        ),
        (
            "a refused read",
            None,
            Some(StorageError::Busy),
            Recalled::Refused(StorageError::Busy),
        ),
    ];
    for (case, stored, refusal, expected) in cases {
        let storage = MemoryStorage::new();
        if let Some(bytes) = stored {
            storage.put(&NAME, bytes.to_vec());
        }
        storage.fail_reads(refusal);
        let recall = start_recall(&storage, now);
        assert_eq!(recall.outcome(), Some(&expected), "{case}");
        assert_eq!(
            recall.deadline(),
            None,
            "{case}: a store that answers at once"
        );
    }
}

fn a_recall_waits_until_its_bound_and_then_never_takes_the_answer() {
    let storage = MemoryStorage::new();
    let placement = WindowPlacement::new(
        None,
        Some(Point::new(40.0, 30.0)),
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    storage.put(&NAME, placement.to_bytes());
    let held = storage.hold_reads();
    let now = Instant::now();
    let mut recall = start_recall(&storage, now);
    assert_eq!(recall.deadline(), Some(now + RECALL_BOUND));
    assert_eq!(
        recall.poll(now + RECALL_BOUND.saturating_sub(Duration::from_millis(1))),
        None,
        "the window waits inside the bound"
    );
    assert_eq!(recall.poll(now + RECALL_BOUND), Some(&Recalled::TimedOut));
    assert_eq!(
        storage.live_requests(),
        0,
        "the read is dropped at the bound"
    );

    drop(held);
    assert_eq!(
        recall.poll(now + RECALL_BOUND * 2),
        Some(&Recalled::TimedOut),
        "an answer after the bound has nothing left to arrive at"
    );
}

fn a_recall_answered_inside_its_bound_settles_with_the_answer() {
    let storage = MemoryStorage::new();
    let held = storage.hold_reads();
    let now = Instant::now();
    let mut recall = start_recall(&storage, now);
    assert_eq!(recall.poll(now), None);
    drop(held);
    assert_eq!(
        recall.poll(now + Duration::from_millis(10)),
        Some(&Recalled::Absent)
    );
}

fn a_storage_that_panics_costs_the_window_its_placement_only() {
    let storage = StorageHandle::new(Arc::new(PanickingReads));
    let recall = PlacementRecall::start(&storage, NAME, Instant::now(), Waker::noop().clone());
    assert_eq!(recall.outcome(), Some(&Recalled::Panicked));

    let recorder = PlacementRecorder::new(storage, NAME, StoredPlacement::Nothing, Arc::new(|| {}));
    let now = Instant::now();
    recorder.observe(&at(10.0, 10.0), &displays(), now);
    assert_eq!(
        recorder.poll_flush(Waker::noop(), now),
        FlushState::Refused,
        "a write that panics is a refused one"
    );
}

fn a_window_that_could_not_read_its_placement_overwrites_it_only_when_it_moves() {
    let storage = MemoryStorage::new();
    storage.put(&NAME, b"what an earlier run kept".to_vec());
    let recorder = PlacementRecorder::new(
        handle(&storage),
        NAME,
        Recalled::TimedOut.stored(),
        Arc::new(|| {}),
    );
    let now = Instant::now();
    recorder.observe(&at(10.0, 10.0), &displays(), now);
    assert_eq!(
        storage.contents(&NAME).as_deref(),
        Some(&b"what an earlier run kept"[..]),
        "the default it opened at does not replace what storage may hold"
    );
    recorder.observe(&user_placing(at(300.0, 200.0)), &displays(), now);
    assert!(
        storage
            .contents(&NAME)
            .is_some_and(|bytes| WindowPlacement::from_bytes(&bytes).is_ok()),
        "a move the user makes is written"
    );
}

fn the_end_of_the_loop_reports_a_placement_storage_did_not_take() {
    let fixture = Fixture::new();
    let barrier = fixture.storage.hold_commits();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    let now = fixture.after(Duration::from_millis(1));
    let lost = settle(&[Rc::new(fixture.recorder)], now, Duration::ZERO, || {});
    drop(barrier);
    assert_eq!(lost, [NAME]);
}

fn a_closed_window_finishes_its_last_write_at_owner_turns() {
    let fixture = Fixture::new();
    let barrier = fixture.storage.hold_commits();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    let now = fixture.after(Duration::from_millis(5));
    fixture.recorder.begin_flush(now);
    assert_eq!(fixture.recorder.continue_flush(now), FlushState::Waiting);
    barrier.commit();
    assert_eq!(fixture.recorder.continue_flush(now), FlushState::Stored);
}

fn a_display_change_alone_measures_the_window_again() {
    let fixture = Fixture::new();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    // The taskbar moved to the left: the usable area starts 60 pixels in.
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let moved = DisplaySnapshot::empty()
        .successor(vec![
            DisplayDescription::new(
                id,
                "main",
                Bounds::new(Point::new(60, 0), Size::new(1860, 1080)),
                DevicePixelRatio::ONE,
            )
            .with_primary(true)
            .with_persistent_key("key-main"),
        ])
        .expect("revision");
    fixture
        .recorder
        .redisplay(&moved, fixture.after(WRITE_INTERVAL));
    assert_eq!(fixture.stored_offset(), Some(Point::new(40.0, 80.0)));
}

/// The displays the host sampled, with the usable area starting `left`
/// pixels in.
fn displays_with_left_edge(left: i32) -> DisplaySnapshot {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
    DisplaySnapshot::empty()
        .successor(vec![
            DisplayDescription::new(
                id,
                "main",
                Bounds::new(Point::new(left, 0), Size::new(1920 - left, 1080)),
                DevicePixelRatio::ONE,
            )
            .with_primary(true)
            .with_persistent_key("key-main"),
        ])
        .expect("revision")
}

fn scaled(state: WindowState, scale: f64) -> WindowState {
    state.with_metrics(Some(
        flui_foundation::geometry::WindowMetrics::try_new(
            flui_foundation::geometry::DeviceSize::new(800, 600),
            scale,
        )
        .expect("a drawable area"),
    ))
}

/// A case of [`an_unread_placement_is_overwritten_only_by_the_user`]: its
/// name and what changes the window after it opened.
type ChangeSinceOpen = (&'static str, fn(&PlacementRecorder, Instant));

fn user_placing(state: WindowState) -> WindowState {
    state.with_interaction(WindowInteraction::UserPlacing)
}

/// A window whose stored placement could not be read keeps what storage may
/// hold against everything the report carrying the change does not state was
/// the user: where it opened, a display change, a scale change, a window
/// manager's adjustment after the window opened, an application's command in
/// two reports, and a change in the report after one by the user. A move, a
/// resize or a mode change whose own report says the user's is written.
fn an_unread_placement_is_overwritten_only_by_the_user() {
    let not_the_user: [ChangeSinceOpen; 6] = [
        ("a display change", |recorder, now| {
            recorder.redisplay(&displays_with_left_edge(60), now);
        }),
        ("a scale change and the resize after it", |recorder, now| {
            recorder.observe(&scaled(at(100.0, 80.0), 1.5), &displays(), now);
            recorder.observe(&scaled(at(150.0, 80.0), 1.5), &displays(), now);
        }),
        (
            "a window manager's adjustment after the open",
            |recorder, now| {
                recorder.observe(&scaled(at(108.0, 110.0), 1.0), &displays(), now);
            },
        ),
        (
            "an application's command in two reports",
            |recorder, now| {
                recorder.observe(&scaled(at(300.0, 80.0), 1.0), &displays(), now);
                recorder.observe(
                    &scaled(at(300.0, 80.0), 1.0).with_mode(WindowMode::Maximized),
                    &displays(),
                    now,
                );
            },
        ),
        (
            "a scale change in the report after the user's",
            |recorder, now| {
                recorder.observe(
                    &user_placing(scaled(at(100.0, 80.0), 1.0)).with_hovered(true),
                    &displays(),
                    now,
                );
                recorder.observe(&scaled(at(150.0, 80.0), 1.5), &displays(), now);
            },
        ),
        ("a move in the report after the user's", |recorder, now| {
            recorder.observe(
                &user_placing(scaled(at(100.0, 80.0), 1.0)),
                &displays(),
                now,
            );
            recorder.observe(&scaled(at(400.0, 80.0), 1.0), &displays(), now);
        }),
    ];
    let the_user: [ChangeSinceOpen; 3] = [
        ("a drag of the frame", |recorder, now| {
            recorder.observe(
                &user_placing(scaled(at(400.0, 80.0), 1.0)),
                &displays(),
                now,
            );
        }),
        ("a resize by the frame", |recorder, now| {
            let resized = WindowState::new(
                WindowGeometry::try_new(Some(Point::new(100.0, 80.0)), Size::new(1016.0, 739.0))
                    .expect("a finite rectangle"),
                flui_platform::WindowExtent::try_new(Size::new(1000.0, 700.0))
                    .expect("a finite size"),
            );
            recorder.observe(
                &scaled(resized, 1.0).with_interaction(WindowInteraction::UserResizing),
                &displays(),
                now,
            );
        }),
        ("a caption button", |recorder, now| {
            recorder.observe(
                &user_placing(scaled(at(100.0, 80.0), 1.0).with_mode(WindowMode::Maximized)),
                &displays(),
                now,
            );
        }),
    ];
    let kept = b"what an earlier run kept".to_vec();
    let start = Instant::now();
    let run = |change: fn(&PlacementRecorder, Instant)| {
        let storage = MemoryStorage::new();
        storage.put(&NAME, kept.clone());
        let recorder = PlacementRecorder::new(
            handle(&storage),
            NAME,
            Recalled::TimedOut.stored(),
            Arc::new(|| {}),
        );
        recorder.observe(&scaled(at(100.0, 80.0), 1.0), &displays(), start);
        change(&recorder, start + WRITE_INTERVAL);
        recorder.begin_flush(start + WRITE_INTERVAL * 2);
        storage.contents(&NAME)
    };
    for (case, change) in not_the_user {
        assert_eq!(
            run(change),
            Some(kept.clone()),
            "{case}: not the user's, so not written, a close included"
        );
    }
    for (case, change) in the_user {
        assert_ne!(
            run(change),
            Some(kept.clone()),
            "{case}: the user's, so written"
        );
    }
}

/// A storage that keeps refusing is tried [`WRITE_ATTEMPTS`] times, each
/// after twice the wait of the one before, then not again until the window's
/// placement changes: an idle application asks its owner for no more turns.
fn a_refusing_storage_is_tried_a_bounded_number_of_times() {
    let fixture = Fixture::new();
    fixture.storage.fail_next(StorageError::Busy);
    fixture.storage.fail_next(StorageError::Busy);
    fixture.storage.fail_next(StorageError::Busy);
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    let mut attempts = 1;
    let mut at_time = Duration::ZERO;
    while let Some(due) = fixture.recorder.next_due() {
        let wait = due.saturating_duration_since(fixture.after(at_time));
        assert_eq!(
            wait,
            WRITE_INTERVAL * (1 << attempts.min(WRITE_ATTEMPTS)),
            "the wait doubles after each refusal"
        );
        at_time += wait;
        fixture.recorder.tick(fixture.after(at_time));
        attempts += 1;
        assert!(attempts <= WRITE_ATTEMPTS, "no more than the bound");
    }
    assert_eq!(attempts, WRITE_ATTEMPTS, "every attempt was made");
    assert_eq!(fixture.stored_offset(), None, "nothing stored");
    assert_eq!(fixture.recorder.next_due(), None, "no deadline remains");

    fixture.observe(&at(200.0, 80.0), at_time + WRITE_INTERVAL);
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(200.0, 80.0)),
        "a new placement is tried again"
    );
}

/// A window reopened from a closed window's placement owes what that window
/// owed: a refused last write leaves storage without the placement, and the
/// new window writes it though it has not moved.
fn a_handed_over_placement_carries_its_debt() {
    let storage = MemoryStorage::new();
    let closed = PlacementRecorder::new(
        handle(&storage),
        NAME,
        StoredPlacement::Nothing,
        Arc::new(|| {}),
    );
    storage.fail_next(StorageError::Busy);
    let now = Instant::now();
    closed.observe(&at(100.0, 80.0), &displays(), now);
    let HandOver::Taken(succession) = closed.hand_over() else {
        panic!("the closed window's placement is handed over");
    };
    let bytes = succession.latest.clone();
    assert!(
        storage.contents(&NAME).is_none(),
        "the last write was refused"
    );

    let reopened =
        PlacementRecorder::succeeding(handle(&storage), NAME, succession, Arc::new(|| {}));
    reopened.observe(&at(100.0, 80.0), &displays(), now);
    assert_eq!(storage.contents(&NAME), Some(bytes), "the debt is paid");
}

/// One recorder writes a name at a time: the closed window's write still out
/// moves to the reopened window's recorder, which takes its answer before it
/// starts a write of its own, so storage ends with the reopened window's
/// placement, whatever order storage would have answered two writers in.
fn a_reopened_window_takes_over_the_write_in_flight() {
    let storage = MemoryStorage::new();
    let closed = PlacementRecorder::new(
        handle(&storage),
        NAME,
        StoredPlacement::Nothing,
        Arc::new(|| {}),
    );
    let now = Instant::now();
    let barrier = storage.hold_commits();
    closed.observe(&at(100.0, 80.0), &displays(), now);
    closed.begin_flush(now);
    let HandOver::Taken(succession) = closed.hand_over() else {
        panic!("the closed window's placement is handed over");
    };
    assert_eq!(
        closed.continue_flush(now),
        FlushState::Stored,
        "the closed window owes nothing and writes nothing from here on"
    );
    let reopened =
        PlacementRecorder::succeeding(handle(&storage), NAME, succession, Arc::new(|| {}));
    // The reopened window opened where the closed one was, then the user
    // moved it.
    reopened.observe(&at(100.0, 80.0), &displays(), now);
    reopened.observe(
        &user_placing(at(500.0, 80.0)),
        &displays(),
        now + WRITE_INTERVAL,
    );
    barrier.commit();
    reopened.tick(now + WRITE_INTERVAL * 2);
    assert_eq!(
        storage
            .contents(&NAME)
            .and_then(|bytes| WindowPlacement::from_bytes(&bytes).ok())
            .and_then(|placement| placement.offset),
        Some(Point::new(500.0, 80.0)),
        "the reopened window's placement is the last one stored"
    );
}

/// What a re-entering storage runs, told where it runs from.
type Reentry = Rc<dyn Fn(&str)>;

std::thread_local! {
    /// What a re-entering storage runs from its `publish`, its future's
    /// `poll` and that future's drop.
    static REENTRY: std::cell::RefCell<Option<Reentry>> =
        const { std::cell::RefCell::new(None) };
}

fn reenter(at: &str) {
    let call = REENTRY.with(|slot| slot.borrow().clone());
    if let Some(call) = call {
        call(at);
    }
}

/// Where a [`ReenteringStore`] panics, once.
type PanicOnce = Arc<std::sync::Mutex<Option<&'static str>>>;

fn panics_here(once: &PanicOnce, at: &str) -> bool {
    let mut armed = once
        .lock()
        .expect("BUG: the flag is never held across a panic");
    if *armed == Some(at) {
        *armed = None;
        return true;
    }
    false
}

/// A store that calls back into the recorder from each piece of its code
/// the recorder runs, and panics once in the piece `panic_at` names.
struct ReenteringStore {
    inner: MemoryStorage,
    panic_at: PanicOnce,
}

struct ReenteringWrite {
    inner: StorageFuture<flui_platform::StoredVersion>,
    panic_at: PanicOnce,
}

impl std::future::Future for ReenteringWrite {
    type Output = Result<flui_platform::StoredVersion, StorageError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        reenter("poll");
        assert!(
            !panics_here(&self.panic_at, "poll"),
            "the store panics in poll"
        );
        self.inner.as_mut().poll(ctx)
    }
}

impl Drop for ReenteringWrite {
    fn drop(&mut self) {
        reenter("drop");
        let panics = panics_here(&self.panic_at, "drop") && !std::thread::panicking();
        assert!(!panics, "the store panics in drop");
    }
}

impl Storage for ReenteringStore {
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored> {
        self.inner.read(name, limit)
    }

    fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        mode: flui_platform::WriteMode,
    ) -> StorageFuture<flui_platform::StoredVersion> {
        reenter("publish");
        assert!(
            !panics_here(&self.panic_at, "publish"),
            "the store panics in publish"
        );
        Box::pin(ReenteringWrite {
            inner: self.inner.publish(name, bytes, mode),
            panic_at: Arc::clone(&self.panic_at),
        })
    }

    fn copy_aside(&self, name: &StorageName) -> StorageFuture<flui_platform::CopiedAside> {
        self.inner.copy_aside(name)
    }
}

/// A store that calls back into the recorder from its `publish`, its
/// future's `poll` and its future's drop finds the recorder working and
/// starts no second write; one that panics in any of them costs that write,
/// and the next write the window owes still goes through.
fn storage_code_runs_with_nothing_borrowed() {
    for panic_at in [None, Some("publish"), Some("poll"), Some("drop")] {
        let inner = MemoryStorage::new();
        let recorder = Rc::new(PlacementRecorder::new(
            StorageHandle::new(Arc::new(ReenteringStore {
                inner: inner.clone(),
                panic_at: Arc::new(std::sync::Mutex::new(panic_at)),
            })),
            NAME,
            StoredPlacement::Nothing,
            Arc::new(|| {}),
        ));
        let now = Instant::now();
        let weak = Rc::downgrade(&recorder);
        let reentries = Rc::new(std::cell::Cell::new(0));
        let counted = Rc::clone(&reentries);
        REENTRY.with(|slot| {
            slot.replace(Some(Rc::new(move |_| {
                if let Some(recorder) = weak.upgrade() {
                    counted.set(counted.get() + 1);
                    recorder.tick(now);
                    let _ = recorder.next_due();
                }
            })));
        });
        recorder.observe(&at(100.0, 80.0), &displays(), now);
        assert!(reentries.get() > 0, "{panic_at:?}: the store came back in");
        if panic_at.is_none() {
            let stored = inner
                .contents(&NAME)
                .and_then(|bytes| WindowPlacement::from_bytes(&bytes).ok())
                .and_then(|placement| placement.offset);
            assert_eq!(
                stored,
                Some(Point::new(100.0, 80.0)),
                "a store that comes back in finds nothing borrowed: its write goes through"
            );
        }
        // The next placement the window reports is written whatever the last
        // attempt did.
        recorder.observe(&at(300.0, 80.0), &displays(), now + WRITE_INTERVAL * 8);
        REENTRY.with(|slot| slot.replace(None));
        let stored = inner
            .contents(&NAME)
            .and_then(|bytes| WindowPlacement::from_bytes(&bytes).ok())
            .and_then(|placement| placement.offset);
        assert_eq!(
            stored,
            Some(Point::new(300.0, 80.0)),
            "{panic_at:?}: the write after the panicking one goes through"
        );
    }
}

fn restore_opens_the_window_where_it_was_and_back_onto_a_display() {
    let mut options = WindowOptions::default();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(1800.0, 900.0)),
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    restore_into(
        &mut options,
        &placement,
        &displays(),
        &WindowCapabilities::ALL,
        |_| NO_FRAME,
    );
    assert_eq!(options.position, Some(Point::new(1120.0, 480.0)));
    assert_eq!(options.size, Size::new(800.0, 600.0));
    assert_eq!(options.mode, WindowMode::Normal);
    assert_eq!(options.normal_geometry, None, "a normal window needs none");
    assert!(options.display.is_some(), "the position is on that display");
}

fn restore_fits_the_outer_size_and_hands_a_maximized_window_its_normal_bounds() {
    let mut options = WindowOptions::default();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(100.0, 80.0)),
        Size::new(5000.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    restore_into(
        &mut options,
        &placement,
        &displays(),
        &WindowCapabilities::ALL,
        |_| Size::new(16.0, 39.0),
    );
    // 1920 outer, so 1904 of content.
    assert_eq!(options.size, Size::new(1904.0, 600.0));
    assert_eq!(options.mode, WindowMode::Maximized);
    let normal = options
        .normal_geometry
        .expect("a maximized window gets bounds to return to");
    assert_eq!(
        (normal.position(), normal.size()),
        (options.position, Size::new(1920.0, 639.0)),
        "the window returns to its outer bounds"
    );
    assert!(options.position.is_some(), "the bounds are placed");
}

/// A frame measured as no extent gives no bounds to return to: the window
/// keeps the ones its configuration gave, rather than losing them.
fn restore_keeps_the_configured_normal_bounds_for_a_frame_that_is_no_extent() {
    let configured = WindowGeometry::try_new(Some(Point::new(40.0, 30.0)), Size::new(640.0, 480.0))
        .expect("a rectangle");
    let mut options = WindowOptions::default().normal_geometry(Some(configured));
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(100.0, 80.0)),
        Size::new(800.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    restore_into(
        &mut options,
        &placement,
        &displays(),
        &WindowCapabilities::ALL,
        // A frame measured negative: the placement fits without it, and the
        // outer bounds it would give are no rectangle.
        |_| Size::new(-5000.0, 39.0),
    );
    assert_eq!(options.mode, WindowMode::Maximized);
    assert_eq!(options.normal_geometry, Some(configured));
}

fn restore_asks_the_platform_for_what_it_can_place() {
    let mut options = WindowOptions::default();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(100.0, 80.0)),
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    let mut capabilities = WindowCapabilities::ALL.into_parts();
    capabilities.places_at_open = flui_platform::Availability::Unavailable;
    let capabilities = capabilities.into_capabilities();
    restore_into(&mut options, &placement, &displays(), &capabilities, |_| {
        NO_FRAME
    });
    assert_eq!(options.position, None, "the platform cannot place at open");
    assert_eq!(
        options.size,
        Size::new(800.0, 600.0),
        "but it keeps the size"
    );
}

/// The fit takes the frame the window opens with: a title bar of the
/// application's that the platform cannot take opens as the system's frame, and
/// no frame takes no inset.
fn restore_fits_the_frame_the_window_opens_with() {
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(100.0, 80.0)),
        Size::new(5000.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    let mut no_app_title_bar = WindowCapabilities::ALL.into_parts();
    no_app_title_bar.app_title_bar = flui_platform::WindowCapability::Absent(
        flui_platform::CapabilityAbsence::NOT_YET_ON_THIS_HOST,
    );
    let no_app_title_bar = no_app_title_bar.into_capabilities();
    // (case, frame asked for, content width)
    let table = [
        (
            "an app title bar the platform cannot take",
            WindowFrame::AppTitleBar(AppTitleBar::new()),
            1904.0,
        ),
        ("the system frame", WindowFrame::System, 1904.0),
        ("no frame", WindowFrame::None, 1920.0),
    ];
    for (case, frame, width) in table {
        let mut options = WindowOptions::default().frame(frame);
        restore_into(
            &mut options,
            &placement,
            &displays(),
            &no_app_title_bar,
            |_| Size::new(16.0, 39.0),
        );
        assert_eq!(options.size.width, width, "{case}");
    }
}

fn the_first_state_is_written_at_once() {
    let fixture = Fixture::new();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));
}

/// A window whose window system reported no rectangle yet (its first reads
/// were refused) and that reads maximized has no place to remember: nothing
/// is written, never a 1x1 placement.
fn a_window_with_no_rectangle_writes_no_placement() {
    let fixture = Fixture::new();
    let nowhere = WindowState::new(WindowGeometry::EMPTY, flui_platform::WindowExtent::ZERO)
        .with_mode(WindowMode::Maximized);
    fixture.observe(&nowhere, Duration::ZERO);
    fixture.recorder.tick(fixture.after(WRITE_INTERVAL * 2));
    assert_eq!(fixture.storage.contents(&NAME), None);
}

fn changes_inside_the_interval_are_held_and_written_when_it_is_over() {
    let fixture = Fixture::new();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    fixture.observe(&at(200.0, 80.0), WRITE_INTERVAL / 2);
    fixture.observe(&at(300.0, 80.0), WRITE_INTERVAL / 2);
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(100.0, 80.0)),
        "at most one write every interval"
    );
    assert_eq!(
        fixture.recorder.next_due(),
        Some(fixture.after(WRITE_INTERVAL)),
        "the owner is asked to wake when the interval is over"
    );

    fixture
        .recorder
        .tick(fixture.after(WRITE_INTERVAL * 99 / 100));
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(100.0, 80.0)),
        "not before"
    );

    fixture.recorder.tick(fixture.after(WRITE_INTERVAL));
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(300.0, 80.0)),
        "the held change is written with no further change to prompt it"
    );
    assert_eq!(
        fixture.recorder.next_due(),
        None,
        "nothing is owed once it is stored"
    );
}

fn a_refused_write_is_owed_again_and_not_counted_as_stored() {
    let fixture = Fixture::new();
    fixture.storage.fail_next(StorageError::Full);
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    assert_eq!(fixture.stored_offset(), None, "the disk refused it");
    assert_eq!(
        fixture.recorder.next_due(),
        Some(fixture.after(WRITE_INTERVAL * 2)),
        "still owed: tried again once twice the interval is over"
    );

    // The same placement: it must not be skipped as already written.
    fixture.recorder.tick(fixture.after(WRITE_INTERVAL * 2));
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));
}

fn a_write_in_flight_waits_for_storage_to_wake_the_owner() {
    let storage = MemoryStorage::new();
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let recorder = PlacementRecorder::new(
        handle(&storage),
        NAME,
        StoredPlacement::Nothing,
        Arc::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    );
    let now = Instant::now();
    let barrier = storage.hold_commits();
    recorder.observe(&at(100.0, 80.0), &displays(), now);
    assert_eq!(
        recorder.next_due(),
        None,
        "a write out has no deadline: the answer wakes the owner"
    );
    assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 0);

    barrier.commit();
    assert_eq!(
        wakes.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "storage's answer wakes the owner"
    );
    assert_eq!(
        recorder.next_due(),
        None,
        "an answer is ready now, so it is no deadline: the wake brings the turn"
    );

    recorder.tick(now);
    assert_eq!(recorder.next_due(), None, "nothing is owed once answered");
}

fn a_write_in_flight_is_not_doubled_and_counts_once_storage_answers() {
    let fixture = Fixture::new();
    let barrier = fixture.storage.hold_commits();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    assert_eq!(fixture.stored_offset(), None, "held at the barrier");
    fixture.observe(&at(200.0, 80.0), WRITE_INTERVAL * 2);
    assert_eq!(
        fixture.storage.live_requests(),
        1,
        "no second write while the first has no answer"
    );

    barrier.commit();
    fixture.recorder.tick(fixture.after(WRITE_INTERVAL * 3));
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(200.0, 80.0)),
        "answered, then the newer placement goes out"
    );
}

fn a_placement_already_stored_is_not_written_again() {
    let storage = MemoryStorage::new();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(100.0, 80.0)),
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    // A sentinel in place of the stored placement: any write replaces it.
    storage.put(&NAME, b"sentinel".to_vec());
    let recorder = PlacementRecorder::new(
        handle(&storage),
        NAME,
        StoredPlacement::Holds(placement.to_bytes()),
        Arc::new(|| {}),
    );
    let now = Instant::now();
    recorder.observe(&at(100.0, 80.0), &displays(), now);
    assert_eq!(storage.contents(&NAME).as_deref(), Some(&b"sentinel"[..]));

    recorder.observe(&at(500.0, 80.0), &displays(), now + WRITE_INTERVAL);
    assert_ne!(
        storage.contents(&NAME).as_deref(),
        Some(&b"sentinel"[..]),
        "a window that moved is written"
    );
}

fn a_minimized_window_that_was_maximized_comes_back_maximized() {
    let fixture = Fixture::new();
    let stored = |fixture: &Fixture| {
        WindowPlacement::from_bytes(&fixture.storage.contents(&NAME).expect("stored"))
            .expect("a placement")
    };
    fixture.observe(
        &at(100.0, 80.0).with_mode(WindowMode::Maximized),
        Duration::ZERO,
    );
    fixture.observe(
        &at(100.0, 80.0).with_mode(WindowMode::Minimized),
        WRITE_INTERVAL,
    );
    let placement = stored(&fixture);
    assert_eq!(placement.mode, WindowMode::Minimized);
    assert!(placement.maximized_before_minimize);

    fixture.observe(
        &at(100.0, 80.0).with_mode(WindowMode::Normal),
        WRITE_INTERVAL * 2,
    );
    fixture.observe(
        &at(100.0, 80.0).with_mode(WindowMode::Minimized),
        WRITE_INTERVAL * 3,
    );
    assert!(
        !stored(&fixture).maximized_before_minimize,
        "a window restored to normal is not maximized when minimized again"
    );
}

fn closing_writes_the_held_placement_without_waiting_for_the_interval() {
    let fixture = Fixture::new();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    fixture.observe(&at(250.0, 90.0), Duration::from_millis(10));
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));

    let storage = fixture.storage.clone();
    let now = fixture.after(Duration::from_millis(20));
    settle(&[Rc::new(fixture.recorder)], now, BUDGET, || {});
    let stored = WindowPlacement::from_bytes(&storage.contents(&NAME).expect("stored"))
        .expect("a placement");
    assert_eq!(stored.offset, Some(Point::new(250.0, 90.0)));
}

fn a_flush_keeps_going_until_storage_holds_the_newest_placement() {
    let fixture = Fixture::new();
    let barrier = fixture.storage.hold_commits();
    // The throttle handed the first position to storage, which has not answered;
    // the window then moved on.
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    fixture.observe(&at(250.0, 90.0), Duration::from_millis(10));
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        barrier.commit();
    });

    let storage = fixture.storage.clone();
    let now = fixture.after(Duration::from_millis(20));
    settle(&[Rc::new(fixture.recorder)], now, BUDGET, || {});

    let stored = WindowPlacement::from_bytes(&storage.contents(&NAME).expect("stored"))
        .expect("a placement");
    assert_eq!(
        stored.offset,
        Some(Point::new(250.0, 90.0)),
        "the answer to the older write is not the end of the flush"
    );
}

fn several_windows_share_one_budget_when_the_session_ends() {
    let budget = Duration::from_millis(150);
    let storage = MemoryStorage::new();
    let names = [
        StorageName::machine_local("window-one"),
        StorageName::machine_local("window-two"),
        StorageName::machine_local("window-three"),
    ];
    let recorders: Vec<_> = names
        .iter()
        .map(|name| {
            Rc::new(PlacementRecorder::new(
                handle(&storage),
                *name,
                StoredPlacement::Nothing,
                Arc::new(|| {}),
            ))
        })
        .collect();
    let now = Instant::now();
    let barrier = storage.hold_commits();
    for recorder in &recorders {
        recorder.observe(&at(10.0, 10.0), &displays(), now);
    }

    let started = Instant::now();
    settle(&recorders, now, budget, || {});
    let spent = started.elapsed();
    drop(barrier);

    assert!(spent >= budget, "waited for the answers: {spent:?}");
    assert!(
        spent < budget * 2,
        "three windows that never answer cost one budget, not three: {spent:?}"
    );
}

fn the_earliest_due_of_several_recorders_is_what_the_owner_wakes_for() {
    let storage = MemoryStorage::new();
    let first = Rc::new(PlacementRecorder::new(
        handle(&storage),
        NAME,
        StoredPlacement::Nothing,
        Arc::new(|| {}),
    ));
    let second = Rc::new(PlacementRecorder::new(
        handle(&storage),
        StorageName::machine_local("window-two"),
        StoredPlacement::Nothing,
        Arc::new(|| {}),
    ));
    let now = Instant::now();
    first.observe(&at(1.0, 1.0), &displays(), now);
    first.observe(&at(2.0, 2.0), &displays(), now);
    second.observe(&at(1.0, 1.0), &displays(), now + Duration::from_millis(500));
    second.observe(&at(2.0, 2.0), &displays(), now + Duration::from_millis(600));
    assert_eq!(
        earliest_due([&first, &second]),
        Some(now + WRITE_INTERVAL),
        "the first window's interval ends first"
    );
}

#[test]
fn placement_recorder() {
    crate::table_test::run_table(
        "placement_recorder",
        &[
            (
                "a_recall_reads_what_storage_answers_and_checks_it",
                a_recall_reads_what_storage_answers_and_checks_it as fn(),
            ),
            (
                "a_recall_waits_until_its_bound_and_then_never_takes_the_answer",
                a_recall_waits_until_its_bound_and_then_never_takes_the_answer as fn(),
            ),
            (
                "a_recall_answered_inside_its_bound_settles_with_the_answer",
                a_recall_answered_inside_its_bound_settles_with_the_answer as fn(),
            ),
            (
                "a_storage_that_panics_costs_the_window_its_placement_only",
                a_storage_that_panics_costs_the_window_its_placement_only as fn(),
            ),
            (
                "a_window_that_could_not_read_its_placement_overwrites_it_only_when_it_moves",
                a_window_that_could_not_read_its_placement_overwrites_it_only_when_it_moves
                    as fn(),
            ),
            (
                "the_end_of_the_loop_reports_a_placement_storage_did_not_take",
                the_end_of_the_loop_reports_a_placement_storage_did_not_take as fn(),
            ),
            (
                "a_closed_window_finishes_its_last_write_at_owner_turns",
                a_closed_window_finishes_its_last_write_at_owner_turns as fn(),
            ),
            (
                "a_display_change_alone_measures_the_window_again",
                a_display_change_alone_measures_the_window_again as fn(),
            ),
            (
                "an_unread_placement_is_overwritten_only_by_the_user",
                an_unread_placement_is_overwritten_only_by_the_user as fn(),
            ),
            (
                "a_refusing_storage_is_tried_a_bounded_number_of_times",
                a_refusing_storage_is_tried_a_bounded_number_of_times as fn(),
            ),
            (
                "a_handed_over_placement_carries_its_debt",
                a_handed_over_placement_carries_its_debt as fn(),
            ),
            (
                "a_reopened_window_takes_over_the_write_in_flight",
                a_reopened_window_takes_over_the_write_in_flight as fn(),
            ),
            (
                "storage_code_runs_with_nothing_borrowed",
                storage_code_runs_with_nothing_borrowed as fn(),
            ),
            (
                "restore_opens_the_window_where_it_was_and_back_onto_a_display",
                restore_opens_the_window_where_it_was_and_back_onto_a_display as fn(),
            ),
            (
                "restore_fits_the_outer_size_and_hands_a_maximized_window_its_normal_bounds",
                restore_fits_the_outer_size_and_hands_a_maximized_window_its_normal_bounds
                    as fn(),
            ),
            (
                "restore_keeps_the_configured_normal_bounds_for_a_frame_that_is_no_extent",
                restore_keeps_the_configured_normal_bounds_for_a_frame_that_is_no_extent as fn(),
            ),
            (
                "restore_fits_the_frame_the_window_opens_with",
                restore_fits_the_frame_the_window_opens_with as fn(),
            ),
            (
                "restore_asks_the_platform_for_what_it_can_place",
                restore_asks_the_platform_for_what_it_can_place as fn(),
            ),
            (
                "the_first_state_is_written_at_once",
                the_first_state_is_written_at_once as fn(),
            ),
            (
                "a_window_with_no_rectangle_writes_no_placement",
                a_window_with_no_rectangle_writes_no_placement as fn(),
            ),
            (
                "changes_inside_the_interval_are_held_and_written_when_it_is_over",
                changes_inside_the_interval_are_held_and_written_when_it_is_over as fn(),
            ),
            (
                "a_refused_write_is_owed_again_and_not_counted_as_stored",
                a_refused_write_is_owed_again_and_not_counted_as_stored as fn(),
            ),
            (
                "a_write_in_flight_waits_for_storage_to_wake_the_owner",
                a_write_in_flight_waits_for_storage_to_wake_the_owner as fn(),
            ),
            (
                "a_write_in_flight_is_not_doubled_and_counts_once_storage_answers",
                a_write_in_flight_is_not_doubled_and_counts_once_storage_answers as fn(),
            ),
            (
                "a_placement_already_stored_is_not_written_again",
                a_placement_already_stored_is_not_written_again as fn(),
            ),
            (
                "a_minimized_window_that_was_maximized_comes_back_maximized",
                a_minimized_window_that_was_maximized_comes_back_maximized as fn(),
            ),
            (
                "closing_writes_the_held_placement_without_waiting_for_the_interval",
                closing_writes_the_held_placement_without_waiting_for_the_interval as fn(),
            ),
            (
                "a_flush_keeps_going_until_storage_holds_the_newest_placement",
                a_flush_keeps_going_until_storage_holds_the_newest_placement as fn(),
            ),
            (
                "several_windows_share_one_budget_when_the_session_ends",
                several_windows_share_one_budget_when_the_session_ends as fn(),
            ),
            (
                "the_earliest_due_of_several_recorders_is_what_the_owner_wakes_for",
                the_earliest_due_of_several_recorders_is_what_the_owner_wakes_for as fn(),
            ),
        ],
    );
}
