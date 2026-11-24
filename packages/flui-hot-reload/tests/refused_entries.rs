//! A plugin entry that answers with its refusal value, as one does when the
//! developer's code behind it panicked, is a skipped frame or a refused load
//! on the host, through the public loaders and real dynamic images.

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
        flui_hot_reload::WorkerPlugin::load(&library).is_none(),
        "a worker whose init failed is refused"
    );
    assert!(
        flui_hot_reload::get_worker_build_ptr(FINGERPRINT).is_none(),
        "the registration its init made is pruned"
    );
}
