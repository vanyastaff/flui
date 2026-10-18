//! A window's placement through storage: what a run recalls, when a keeper
//! writes, and what the closing and ending flushes add.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::WindowOptions;
use flui_platform_api::{
    DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, StorageName, WindowGeometry,
    WindowMode, WindowPlacement, WindowState,
};
use flui_testing::storage::MemoryStorage;

use super::{PlacementKeeper, WRITE_INTERVAL, recall, restore_into};

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
    keeper: PlacementKeeper,
    clock: Rc<Cell<Instant>>,
}

impl Fixture {
    fn new() -> Self {
        let storage = MemoryStorage::new();
        let clock = Rc::new(Cell::new(Instant::now()));
        let now = Rc::clone(&clock);
        let keeper = PlacementKeeper::new(
            Arc::new(storage.clone()),
            NAME,
            None,
            move || now.get(),
            displays,
        );
        Self {
            storage,
            keeper,
            clock,
        }
    }

    fn advance(&self, by: Duration) {
        self.clock.set(self.clock.get() + by);
    }

    fn stored(&self) -> Option<WindowPlacement> {
        let bytes = self.storage.contents(&NAME)?;
        Some(WindowPlacement::decode(&bytes).expect("a placement was stored"))
    }

    fn stored_offset(&self) -> Option<Point<f64>> {
        self.stored().map(|placement| placement.offset)
    }
}

fn recall_reads_what_an_earlier_run_stored() {
    let storage = MemoryStorage::new();
    assert_eq!(recall(&storage, &NAME), None, "a first run has nothing");

    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Point::new(40.0, 30.0),
        Size::new(800.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    storage.put(&NAME, placement.encode());
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

fn restore_moves_the_window_where_it_was_and_back_onto_a_display() {
    let mut options = WindowOptions::default();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Point::new(1800.0, 900.0),
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    restore_into(&mut options, &placement, &displays());
    assert_eq!(options.position, Some(Point::new(1120.0, 480.0)));
    assert_eq!(options.size, Size::new(800.0, 600.0));
    assert_eq!(options.mode, WindowMode::Normal);
}

fn the_first_state_is_written_at_once() {
    let fixture = Fixture::new();
    fixture.keeper.observe(&at(100.0, 80.0));
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));
}

fn changes_inside_the_interval_wait_for_the_next_write() {
    let fixture = Fixture::new();
    fixture.keeper.observe(&at(100.0, 80.0));
    fixture.advance(WRITE_INTERVAL / 2);
    fixture.keeper.observe(&at(200.0, 80.0));
    fixture.keeper.observe(&at(300.0, 80.0));
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(100.0, 80.0)),
        "at most one write every interval"
    );

    fixture.advance(WRITE_INTERVAL);
    fixture.keeper.observe(&at(400.0, 80.0));
    assert_eq!(
        fixture.stored_offset(),
        Some(Point::new(400.0, 80.0)),
        "the first change after the interval writes the newest"
    );
}

fn flush_writes_the_held_placement_without_waiting_for_the_interval() {
    let fixture = Fixture::new();
    fixture.keeper.observe(&at(100.0, 80.0));
    fixture.advance(Duration::from_millis(10));
    fixture.keeper.observe(&at(250.0, 90.0));
    assert_eq!(fixture.stored_offset(), Some(Point::new(100.0, 80.0)));

    fixture.keeper.flush();
    assert_eq!(fixture.stored_offset(), Some(Point::new(250.0, 90.0)));
}

fn a_placement_already_stored_is_not_written_again() {
    let storage = MemoryStorage::new();
    let placement = WindowPlacement::new(
        Some("key-main".into()),
        Point::new(100.0, 80.0),
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        1.0,
    );
    // A sentinel in place of the stored placement: any write replaces it.
    storage.put(&NAME, b"sentinel".to_vec());
    let keeper = PlacementKeeper::new(
        Arc::new(storage.clone()),
        NAME,
        Some(&placement),
        Instant::now,
        displays,
    );
    keeper.observe(&at(100.0, 80.0));
    keeper.flush();
    assert_eq!(storage.contents(&NAME).as_deref(), Some(&b"sentinel"[..]));

    keeper.observe(&at(500.0, 80.0));
    keeper.flush();
    assert_ne!(
        storage.contents(&NAME).as_deref(),
        Some(&b"sentinel"[..]),
        "a window that moved is written"
    );
}

fn a_minimized_window_that_was_maximized_comes_back_maximized() {
    let fixture = Fixture::new();
    fixture
        .keeper
        .observe(&at(100.0, 80.0).with_mode(WindowMode::Maximized));
    fixture.advance(WRITE_INTERVAL);
    fixture
        .keeper
        .observe(&at(100.0, 80.0).with_mode(WindowMode::Minimized));
    let stored = fixture.stored().expect("stored");
    assert_eq!(stored.mode, WindowMode::Minimized);
    assert!(stored.restores_maximized);

    fixture.advance(WRITE_INTERVAL);
    fixture
        .keeper
        .observe(&at(100.0, 80.0).with_mode(WindowMode::Normal));
    fixture.advance(WRITE_INTERVAL);
    fixture
        .keeper
        .observe(&at(100.0, 80.0).with_mode(WindowMode::Minimized));
    assert!(
        !fixture.stored().expect("stored").restores_maximized,
        "a window restored to normal is not maximized when minimized again"
    );
}

#[test]
fn placement_keeper() {
    crate::table_test::run_table(
        "placement_keeper",
        &[
            (
                "recall_reads_what_an_earlier_run_stored",
                recall_reads_what_an_earlier_run_stored as fn(),
            ),
            (
                "restore_moves_the_window_where_it_was_and_back_onto_a_display",
                restore_moves_the_window_where_it_was_and_back_onto_a_display as fn(),
            ),
            (
                "the_first_state_is_written_at_once",
                the_first_state_is_written_at_once as fn(),
            ),
            (
                "changes_inside_the_interval_wait_for_the_next_write",
                changes_inside_the_interval_wait_for_the_next_write as fn(),
            ),
            (
                "flush_writes_the_held_placement_without_waiting_for_the_interval",
                flush_writes_the_held_placement_without_waiting_for_the_interval as fn(),
            ),
            (
                "a_placement_already_stored_is_not_written_again",
                a_placement_already_stored_is_not_written_again as fn(),
            ),
            (
                "a_minimized_window_that_was_maximized_comes_back_maximized",
                a_minimized_window_that_was_maximized_comes_back_maximized as fn(),
            ),
        ],
    );
}
