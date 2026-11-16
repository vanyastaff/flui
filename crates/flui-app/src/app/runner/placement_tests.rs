//! A window's remembered placement through the desktop runner: an
//! application run on the headless platform, whose host storage is a store
//! the test reads back.

use std::sync::Arc;

use flui_foundation::geometry::{Point, Size};
use flui_platform::{Storage, StorageName, WindowMode, WindowPlacement};
use flui_testing::storage::MemoryStorage;

use crate::app::runner::host::APP_RUNTIME;
use crate::app::{AppConfig, Application};
use crate::testing::HeadlessRun;

const NAME: StorageName = StorageName::machine_local("window-main");

/// A window that remembers its placement reads the stored one once, before
/// it opens: installing the window, which starts the recorder from that
/// placement, asks storage for nothing more.
#[test]
fn a_remembered_placement_is_read_once_before_the_window_opens() {
    let storage = MemoryStorage::new();
    storage.put(
        &NAME,
        WindowPlacement::new(
            Some("key-main".into()),
            Some(Point::new(40.0, 30.0)),
            Size::new(640.0, 480.0),
            WindowMode::Normal,
            1.0,
        )
        .to_bytes(),
    );
    let application = Application::new().config(AppConfig::new().restore_placement(NAME));
    let run = HeadlessRun::start_resident(application, |_| flui_widgets::SizedBox::shrink());
    // The run names no data directory, so the host has no storage of its
    // own: from here on it is this store.
    let host_storage: Arc<dyn Storage> = Arc::new(storage.clone());
    APP_RUNTIME.with(|slot| slot.borrow().host_storage.install(Ok(host_storage)));
    assert_eq!(storage.reads(), 0, "nothing reads before a window opens");

    let ingress = APP_RUNTIME
        .with(|slot| slot.borrow().main_ingress.clone())
        .expect("a running loop has its main window's ingress");
    assert!(
        ingress.request_native_show(),
        "the running loop takes the reopen"
    );
    run.run_owner_turn();

    assert_eq!(run.open_windows().len(), 1, "the main window opened");
    let recording = APP_RUNTIME.with(|slot| slot.borrow().placement_recorders.len());
    assert_eq!(recording, 1, "the installed window records its placement");
    assert_eq!(
        storage.reads(),
        1,
        "the placement is read before the window opens and not again"
    );
    run.join().expect("the run ends");
}
