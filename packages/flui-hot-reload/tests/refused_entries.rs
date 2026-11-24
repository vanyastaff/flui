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
    // SAFETY: the fixture's token matched, and a `None` owns no scene.
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
