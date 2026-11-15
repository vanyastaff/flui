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
    PlatformWindow, Storage, StorageName, Unsupported, WindowMode, WindowPlacement,
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
        slot.borrow_mut()
            .placement
            .set_clock(Rc::new(placement_clock));
    });
    let ingress = APP_RUNTIME
        .with(|slot| slot.borrow().main_ingress.clone())
        .expect("a running loop has its main window's ingress");
    ingress.request_native_show();
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

/// The owner's turn at the read's bound, as the bound's wake gives it
/// (`wake_owner_at_bound`, whose thread sleeps on the real clock the test
/// does not wait for).
fn wake_at_bound(run: &HeadlessRunHandle) {
    crate::app::runner::host::with_owner_platform(|owner| owner.proxy().wake())
        .expect("a running loop has an owner")
        .expect("the owner takes a wake");
    run.run_owner_turn();
}

fn recording() -> usize {
    APP_RUNTIME.with(|slot| slot.borrow().placement.live.len())
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
    assert!(
        run.wake_deadline()
            .is_some_and(|deadline| deadline <= started + RECALL_BOUND),
        "the loop wakes by the read's bound"
    );

    clock.advance(RECALL_BOUND);
    wake_at_bound(&run);
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
