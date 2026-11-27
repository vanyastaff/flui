//! A refused plugin library says why, and a plugin entry that answers with its
//! refusal value, as one does when the developer's code behind it panicked, is
//! a skipped frame or a refused load on the host, through the public loaders
//! and real dynamic images.

use std::path::{Path, PathBuf};

use crate::worker_lifetime::compile_fixture;

/// A fixture directory unique to this process and test, removed on drop.
struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("flui-refused-entries-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir(&path).expect("create fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Fails when the host treats a scene plugin's null answer as a contract
/// violation: the build then panics in the host instead of skipping the frame.
#[test]
fn a_scene_plugin_answering_null_skips_the_frame() {
    let directory = FixtureDirectory::new("scene");
    let library = compile_fixture(
        directory.path(),
        "null_scene",
        r#"
#[unsafe(no_mangle)]
pub extern "C" fn flui_scene_build(_width: f64, _height: f64) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flui_scene_free(_ptr: *mut std::ffi::c_void) {}
#[unsafe(no_mangle)]
pub extern "C" fn flui_scene_abi_token() -> u64 { TOKEN }
"#,
    );
    let plugin = flui_hot_reload::ScenePlugin::load(&library).expect("the scene fixture loads");
    // SAFETY: nothing here clones a payload out of the scene.
    #[expect(unsafe_code, reason = "`build_scene` is an `unsafe fn`")]
    let scene = unsafe { plugin.build_scene(64.0, 64.0) };
    assert!(scene.is_none(), "a null answer is a skipped frame");
    plugin.unload();
}

/// Fails when the loader admits a worker whose init answered `false`, or
/// keeps the registration that init made before it failed.
#[test]
fn a_worker_whose_init_fails_is_refused_with_its_registrations() {
    const FINGERPRINT: u64 = 0xFA11_ED01;
    let directory = FixtureDirectory::new("worker");
    let library = compile_fixture(
        directory.path(),
        "failed_worker",
        r#"
fn build() {}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_init(register: extern "C" fn(u64, *const ()), _request_rebuild: extern "C-unwind" fn()) -> bool {
    register(0xFA11_ED01, build as *const ());
    false
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_abi_token() -> u64 { TOKEN }
"#,
    );
    assert!(
        matches!(
            flui_hot_reload::WorkerPlugin::load(&library),
            Err(flui_hot_reload::LoadError::InitFailed { .. })
        ),
        "a worker whose init failed is refused"
    );
    assert!(
        flui_hot_reload::get_worker_build_ptr(FINGERPRINT).is_none(),
        "the registration its init made is pruned"
    );
}

/// Each refusal names its reason, so a developer whose plugin does not load
/// learns why. Fails when the loader's own text is dropped, or a refusal is
/// reported under another reason.
#[test]
fn a_refused_library_says_why() {
    use flui_hot_reload::{LoadError, ScenePlugin};

    let directory = FixtureDirectory::new("reasons");
    let missing = directory.path().join("not_built_yet.so");
    assert!(matches!(
        ScenePlugin::load(&missing),
        Err(LoadError::Missing { path }) if path == missing
    ));

    let garbage = directory.path().join("garbage.so");
    std::fs::write(&garbage, b"not a library").expect("write a file that is no library");
    let Err(LoadError::Open { message, .. }) = ScenePlugin::load(&garbage) else {
        panic!("a file that is no library is refused by the platform loader");
    };
    // Every Unix loader's text names the file it refused.
    #[cfg(unix)]
    assert!(
        message.contains("garbage.so"),
        "the loader's own text is kept: {message}"
    );
    #[cfg(not(unix))]
    assert!(!message.is_empty(), "the loader's own text is kept");

    let unrelated = compile_fixture(
        directory.path(),
        "unrelated",
        "#[unsafe(no_mangle)] pub extern \"C\" fn unrelated() {}",
    );
    assert!(matches!(
        ScenePlugin::load(&unrelated),
        Err(LoadError::NotAPlugin { .. })
    ));

    let foreign = compile_fixture(
        directory.path(),
        "foreign",
        r#"
#[unsafe(no_mangle)]
pub extern "C" fn flui_scene_build(_width: f64, _height: f64) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_scene_abi_token() -> u64 { 1 }
"#,
    );
    assert!(matches!(
        ScenePlugin::load(&foreign),
        Err(LoadError::AbiMismatch { plugin: 1, .. })
    ));

    let unfreeable = compile_fixture(
        directory.path(),
        "unfreeable",
        r#"
#[unsafe(no_mangle)]
pub extern "C" fn flui_scene_build(_width: f64, _height: f64) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_scene_abi_token() -> u64 { TOKEN }
"#,
    );
    assert!(matches!(
        ScenePlugin::load(&unfreeable),
        Err(LoadError::MissingSymbol { symbol, .. }) if symbol == "flui_scene_free"
    ));
}

/// A worker driver's count is the images it has loaded, the first included,
/// and a refused reload in between does not restart it. Fails when the load
/// that follows a refusal counts from one again.
#[test]
fn a_worker_driver_counts_each_image_it_loads_across_a_refusal() {
    use flui_hot_reload::{WorkerPollOutcome, WorkerReloadDriver};

    let directory = FixtureDirectory::new("counted");
    let worker = |name: &str| {
        compile_fixture(
            directory.path(),
            name,
            r#"
fn build() {}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_init(register: extern "C" fn(u64, *const ()), _request_rebuild: extern "C-unwind" fn()) -> bool {
    register(0xC0_07ED, build as *const ());
    true
}
#[unsafe(no_mangle)]
pub extern "C" fn flui_worker_abi_token() -> u64 { TOKEN }
"#,
        )
    };
    let first = worker("counted_first");
    let second = worker("counted_second");
    let third = worker("counted_third");
    let refused = directory.path().join("counted_refused.so");
    std::fs::write(&refused, b"not a library").expect("write a file that is no library");
    // The sidecar manifest beside the watched path names the image to load,
    // as `flui run` stages each build under a path of its own.
    let watched = directory.path().join("worker.so");
    let mut revision = 0;
    let mut stage = |image: &Path| {
        revision += 1;
        let manifest = flui_hot_reload::WorkerManifest::new(
            image.to_path_buf(),
            flui_sdk::view::dev_reload::CodeVersion::new(
                flui_sdk::view::dev_reload::LayoutEpoch::try_new(1).expect("not zero"),
                flui_sdk::view::dev_reload::Revision::try_new(revision).expect("not zero"),
            ),
        );
        std::fs::write(
            flui_hot_reload::WorkerManifest::beside(&watched),
            serde_json::to_vec(&manifest).expect("a manifest serializes"),
        )
        .expect("write the manifest");
    };

    stage(&first);
    let mut driver =
        WorkerReloadDriver::new(&watched).with_poll_interval(std::time::Duration::ZERO);
    assert_eq!(
        driver
            .loaded_version()
            .map(|version| version.revision.get()),
        Some(1),
        "the loaded image's version is the one its manifest named"
    );
    stage(&refused);
    assert_eq!(driver.poll(), WorkerPollOutcome::ReloadFailed);
    stage(&second);
    assert_eq!(
        driver.poll(),
        WorkerPollOutcome::Reloaded { reload_count: 2 },
        "the second image loaded is the second counted"
    );
    assert_eq!(
        driver
            .loaded_version()
            .map(|version| version.revision.get()),
        Some(3),
        "a reload reads the version of the image it loaded"
    );
    stage(&third);
    assert_eq!(
        driver.poll(),
        WorkerPollOutcome::Reloaded { reload_count: 3 },
        "a loaded image is replaced by the next"
    );
    assert_eq!(
        driver
            .loaded_version()
            .map(|version| version.revision.get()),
        Some(4),
        "the replacing image's version"
    );

    // The hook a host installs reports the image loaded at `attach` once, so
    // the host's record learns which layout it was composed from.
    drop(driver);
    #[cfg(feature = "host-hook")]
    {
        use flui_sdk::view::dev_reload::{DevReloadHook as _, ReloadEvent, ReloadWake};
        let mut hook = flui_hot_reload::WorkerReloadHook::new(&watched);
        hook.attach(ReloadWake::new(|| {}));
        let ReloadEvent::Loaded(version) = hook.poll() else {
            panic!("the first poll after attach reports the loaded image's version");
        };
        assert_eq!(version.revision.get(), 4);
        assert_eq!(hook.poll(), ReloadEvent::Unchanged, "once");
        hook.detach();
    }
}
