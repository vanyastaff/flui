//! A window's placement through storage: what a run recalls, when a recorder
//! writes, what a refused write leaves owed, and what the closing and ending
//! flushes add.

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::WindowOptions;
use flui_platform_api::{
    DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, StorageError, StorageName,
    WindowCapabilities, WindowGeometry, WindowMode, WindowPlacement, WindowState,
};
use flui_testing::storage::MemoryStorage;

use super::{
    FLUSH_BUDGET, PlacementRecorder, WRITE_INTERVAL, earliest_due, recall, restore_into, settle,
};

const NAME: StorageName = StorageName::machine_local("window-main");

fn displays() -> DisplaySnapshot {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let display = DisplayInfo::new(
        id,
        "main",
        Bounds::new(Point::new(0, 0), Size::new(1920, 1080)),
        1.0,
    )
    .with_primary(true)
    .with_persistent_key("key-main");
    DisplaySnapshot::empty()
        .successor(vec![display])
        .expect("revision")
}

fn at(x: f64, y: f64) -> WindowState {
    WindowState::new(
        WindowGeometry::new(Some(Point::new(x, y)), Size::new(816.0, 639.0)),
        Size::new(800.0, 600.0),
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
        let recorder = PlacementRecorder::new(Arc::new(storage.clone()), NAME, None);
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

fn recall_reads_what_an_earlier_run_stored() {
    let storage = MemoryStorage::new();
    assert_eq!(recall(&storage, &NAME), None, "a first run has nothing");

    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Some(Point::new(40.0, 30.0)),
        Size::new(800.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    storage.put(&NAME, placement.to_bytes());
    assert_eq!(recall(&storage, &NAME), Some(placement));

    storage.put(&NAME, b"flui-window-placement/9\nnot ours\n".to_vec());
    assert_eq!(recall(&storage, &NAME), None, "another version is ignored");
    storage.put(&NAME, b"flui-window-pla".to_vec());
    assert_eq!(
        recall(&storage, &NAME),
        None,
        "a cut-short value is ignored"
    );
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
    assert_eq!(
        options.normal_geometry,
        Some(WindowGeometry::new(
            options.position,
            Size::new(1920.0, 639.0)
        )),
        "the window returns to its outer bounds"
    );
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
    let mut capabilities = WindowCapabilities::ALL;
    capabilities.places_at_open = false;
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

fn the_first_state_is_written_at_once() {
    let fixture = Fixture::new();
    fixture.observe(&at(100.0, 80.0), Duration::ZERO);
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));
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
        fixture.recorder.next_due(fixture.after(WRITE_INTERVAL / 2)),
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
        fixture.recorder.next_due(fixture.after(WRITE_INTERVAL)),
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
        fixture.recorder.next_due(fixture.after(Duration::ZERO)),
        Some(fixture.after(WRITE_INTERVAL)),
        "still owed: tried again when the interval is over"
    );

    // The same placement again: it must not be skipped as already written.
    fixture.observe(&at(100.0, 80.0), WRITE_INTERVAL);
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));
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
    let recorder = PlacementRecorder::new(Arc::new(storage.clone()), NAME, Some(&placement));
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
    settle(&[Rc::new(fixture.recorder)], now, FLUSH_BUDGET);
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
    settle(&[Rc::new(fixture.recorder)], now, FLUSH_BUDGET);

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
                Arc::new(storage.clone()),
                *name,
                None,
            ))
        })
        .collect();
    let now = Instant::now();
    let barrier = storage.hold_commits();
    for recorder in &recorders {
        recorder.observe(&at(10.0, 10.0), &displays(), now);
    }

    let started = Instant::now();
    settle(&recorders, now, budget);
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
        Arc::new(storage.clone()),
        NAME,
        None,
    ));
    let second = Rc::new(PlacementRecorder::new(
        Arc::new(storage),
        StorageName::machine_local("window-two"),
        None,
    ));
    let now = Instant::now();
    first.observe(&at(1.0, 1.0), &displays(), now);
    first.observe(&at(2.0, 2.0), &displays(), now);
    second.observe(&at(1.0, 1.0), &displays(), now + Duration::from_millis(500));
    second.observe(&at(2.0, 2.0), &displays(), now + Duration::from_millis(600));
    assert_eq!(
        earliest_due(now, [&first, &second]),
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
                "recall_reads_what_an_earlier_run_stored",
                recall_reads_what_an_earlier_run_stored as fn(),
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
                "restore_asks_the_platform_for_what_it_can_place",
                restore_asks_the_platform_for_what_it_can_place as fn(),
            ),
            (
                "the_first_state_is_written_at_once",
                the_first_state_is_written_at_once as fn(),
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
