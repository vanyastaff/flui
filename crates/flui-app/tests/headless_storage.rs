//! Storage selected by the headless platform, reached through a mounted widget.

use std::cell::OnceCell;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use flui_app::testing::{HeadlessRun, HeadlessRunHandle, RunExit};
use flui_app::{AppConfig, AppId, AppIdentity, Application, DeterministicExecutors};
use flui_native::{HeadlessPlatform, Platform};
use flui_platform::{
    AppDataDir, StorageError, StorageFuture, StorageName, StoredVersion, Unsupported,
    UnsupportedReason, WriteMode,
};
use flui_view::persist::{StorageCapability, StorageHandle};
use flui_view::prelude::*;

const DOCUMENT: StorageName = StorageName::from_static("document");

#[derive(Clone, StatefulView)]
struct StorageRoot {
    acquired: Rc<OnceCell<Result<StorageHandle, Unsupported>>>,
}

impl StatefulView for StorageRoot {
    type State = Self;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        self.clone()
    }
}

impl ViewState<StorageRoot> for StorageRoot {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.acquired
            .set(ctx.capability::<StorageCapability>())
            .expect("the root acquires storage once, in init_state");
    }

    fn build(&self, _view: &StorageRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::shrink()
    }
}

fn start(
    config: AppConfig,
    platform: HeadlessPlatform,
    executors: &DeterministicExecutors,
) -> (HeadlessRunHandle, Result<StorageHandle, Unsupported>) {
    let acquired = Rc::new(OnceCell::new());
    let root = StorageRoot {
        acquired: Rc::clone(&acquired),
    };
    let application = Application::new(move |_| root.clone())
        .config(config.executors(executors.host_executors()));
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();
    let storage = acquired
        .get()
        .expect("the owner turn mounted the root")
        .clone();
    (run, storage)
}

fn complete<T>(
    executors: &DeterministicExecutors,
    mut request: StorageFuture<T>,
) -> Result<T, StorageError> {
    executors.run_until_idle();
    let Poll::Ready(result) = request
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    else {
        panic!("the storage request must complete after its IO lane runs");
    };
    result
}

fn identity() -> AppIdentity {
    AppIdentity::try_new("Headless storage acceptance").expect("a valid display name")
}

fn directory(name: &str) -> AppDataDir {
    AppDataDir::try_new(name).expect("a valid application directory")
}

#[test]
fn headless_storage_reaches_app_lifecycle() {
    super::run_table(
        "headless_storage_reaches_app_lifecycle",
        &[
            (
                "directory selection and shared memory",
                selected_memory_reaches_app,
            ),
            (
                "platform isolation and retirement",
                memory_isolated_across_runs,
            ),
            (
                "explicit disk root survives a fresh platform",
                disk_survives_fresh_platform,
            ),
        ],
    );
}

fn selected_memory_reaches_app() {
    let app_id = AppId::try_new("dev.flui.storage_acceptance").expect("a valid app id");
    let explicit = directory("explicit-storage");
    let identified = directory(app_id.as_str());
    let cases = [
        (
            AppConfig::new()
                .application_identity(identity())
                .storage_dir(explicit.clone()),
            explicit.clone(),
        ),
        (
            AppConfig::new().application_identity(identity().with_app_id(app_id.clone())),
            identified.clone(),
        ),
        (
            AppConfig::new()
                .application_identity(identity().with_app_id(app_id))
                .storage_dir(explicit.clone()),
            explicit,
        ),
    ];
    for (config, selected) in cases {
        let platform = HeadlessPlatform::new();
        let store = platform.data_store(&selected).expect("headless storage");
        let other = if selected == identified {
            directory("another-application")
        } else {
            identified.clone()
        };
        let other_store = platform
            .data_store(&other)
            .expect("another application's storage");
        let seed = store
            .write(&DOCUMENT, b"platform seed", WriteMode::Replace)
            .expect("seed written");
        other_store
            .write(&DOCUMENT, b"other application", WriteMode::Replace)
            .expect("other seed written");

        let executors = DeterministicExecutors::new();
        let (run, storage) = start(config, platform, &executors);
        let storage = storage.expect("the configured capability is available");
        let read = complete(&executors, storage.read(&DOCUMENT, 64)).expect("app reads seed");
        assert_eq!(read.bytes.as_deref(), Some(b"platform seed".as_slice()));
        assert_eq!(read.version, seed);
        let published = complete(
            &executors,
            storage.publish(
                &DOCUMENT,
                b"app edit".to_vec(),
                WriteMode::IfUnchanged(seed),
            ),
        )
        .expect("app publishes through its IO lane");
        let readback = store.read(&DOCUMENT, 64).expect("platform reads app edit");
        assert_eq!(readback.bytes.as_deref(), Some(b"app edit".as_slice()));
        assert_eq!(readback.version, published);
        assert_eq!(
            other_store
                .read(&DOCUMENT, 64)
                .expect("other app reads")
                .bytes
                .as_deref(),
            Some(b"other application".as_slice())
        );
        assert_eq!(run.join().expect("run joins").exit, RunExit::Returned);
    }
}

fn memory_isolated_across_runs() {
    let app = directory("isolated-storage");
    let platform = HeadlessPlatform::new();
    let previous = platform.data_store(&app).expect("headless storage");
    let executors = DeterministicExecutors::new();
    let config = AppConfig::new()
        .application_identity(identity())
        .storage_dir(app);
    let (run, storage) = start(config.clone(), platform, &executors);
    let retired = storage.expect("first app storage");
    complete(
        &executors,
        retired.publish(&DOCUMENT, b"first platform".to_vec(), WriteMode::Replace),
    )
    .expect("published");
    assert_eq!(run.join().expect("first run joins").exit, RunExit::Returned);
    assert_eq!(
        complete(&executors, retired.read(&DOCUMENT, 64)),
        Err(StorageError::Cancelled)
    );

    let (run, storage) = start(config, HeadlessPlatform::new(), &executors);
    let storage = storage.expect("fresh platform storage");
    let empty = complete(&executors, storage.read(&DOCUMENT, 64)).expect("read fresh platform");
    assert_eq!(empty.bytes, None);
    assert_eq!(empty.version, StoredVersion::ABSENT);
    complete(
        &executors,
        storage.publish(&DOCUMENT, b"second platform".to_vec(), WriteMode::Replace),
    )
    .expect("second platform writes");
    assert_eq!(
        previous
            .read(&DOCUMENT, 64)
            .expect("first platform readback")
            .bytes
            .as_deref(),
        Some(b"first platform".as_slice())
    );
    assert_eq!(
        run.join().expect("second run joins").exit,
        RunExit::Returned
    );

    let (run, unconfigured) = start(
        AppConfig::new().application_identity(identity()),
        HeadlessPlatform::new(),
        &executors,
    );
    assert_eq!(
        unconfigured
            .expect_err("no storage inherited from a previous run")
            .reason,
        UnsupportedReason::NotRegistered
    );
    assert_eq!(
        run.join().expect("unconfigured run joins").exit,
        RunExit::Returned
    );
}

fn disk_survives_fresh_platform() {
    let root = tempfile::tempdir().expect("temporary disk root");
    let app = directory("disk-storage");
    let config = AppConfig::new()
        .application_identity(identity())
        .storage_dir(app.clone());
    let executors = DeterministicExecutors::new();
    let (run, storage) = start(
        config.clone(),
        HeadlessPlatform::new().with_data_root(root.path()),
        &executors,
    );
    let storage = storage.expect("disk capability");
    let scopes = [
        (DOCUMENT, "roaming", b"document".as_slice()),
        (
            StorageName::machine_local("document"),
            "local",
            b"session".as_slice(),
        ),
    ];
    for (name, scope, bytes) in scopes {
        let version = complete(
            &executors,
            storage.publish(&name, bytes.to_vec(), WriteMode::Replace),
        )
        .expect("disk publish completes");
        assert_eq!(version, StoredVersion::of_bytes(bytes));
        let mut directory = root.path().join(scope).join(app.as_str());
        if name.is_machine_local() {
            directory.push(".machine-local");
        }
        assert_eq!(
            std::fs::read(directory.join(name.as_str())).expect("raw file exists"),
            bytes
        );
    }
    assert_eq!(
        run.join().expect("disk writer joins").exit,
        RunExit::Returned
    );
    let (run, storage) = start(
        config,
        HeadlessPlatform::new().with_data_root(root.path()),
        &executors,
    );
    let storage = storage.expect("fresh platform disk capability");
    for (name, _, bytes) in scopes {
        let read = complete(&executors, storage.read(&name, 64)).expect("fresh app reads disk");
        assert_eq!(read.bytes.as_deref(), Some(bytes));
        assert_eq!(read.version, StoredVersion::of_bytes(bytes));
    }
    assert_eq!(
        run.join().expect("disk reader joins").exit,
        RunExit::Returned
    );
}
