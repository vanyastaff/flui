//! Exercise the actual generated allocation boundary without loading a DSO.
#![expect(
    unsafe_code,
    reason = "tests the documented raw scene ownership protocol"
)]

use flui_hot_reload::Scene;
use flui_layer::{AnnotatedRegionLayer, Layer, LayerTree};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Payload;
impl Drop for Payload {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

fn build_scene(_: f64, _: f64) -> Scene {
    Scene::new(LayerTree::new(Layer::AnnotatedRegion(
        AnnotatedRegionLayer::sized_by_parent(Arc::new(Payload)),
    )))
}

flui_hot_reload::scene_plugin!(build_scene);

#[test]
fn both_ownership_paths_destroy_the_payload_once_and_null_is_a_noop() {
    let original = flui_scene_build(10.0, 20.0);
    assert!(!original.is_null());
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);
    // SAFETY: unique initialized allocation from this image, never moved or consumed.
    unsafe { flui_scene_drop(original) };
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);

    let allocation = flui_scene_build(30.0, 40.0);
    assert!(!allocation.is_null());
    // SAFETY: this aligned allocation contains one initialized Scene; read exactly once.
    let moved = unsafe { allocation.cast::<Scene>().read() };
    // SAFETY: the scene was moved out once and this live allocation is uniquely owned.
    unsafe { flui_scene_free(allocation) };
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(moved.tree().len(), 1);
    drop(moved);
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);

    // SAFETY: both entry points explicitly accept null without any allocation ownership.
    unsafe {
        flui_scene_drop(std::ptr::null_mut());
        flui_scene_free(std::ptr::null_mut());
    }
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}

#[test]
fn polling_an_unavailable_plugin_reports_no_change() {
    let path = std::env::temp_dir().join(format!(
        "flui-missing-plugin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current time follows the epoch")
            .as_nanos(),
    ));
    let mut driver =
        flui_hot_reload::HotReloadDriver::new(path).with_poll_interval(std::time::Duration::ZERO);
    let changed: bool = driver.poll();
    assert!(!changed);
    assert!(!driver.is_loaded());
    assert!(!driver.poll());
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn a_native_byte_library_path_can_be_loaded() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
    // SAFETY: malloc is mapped for this process; dladdr initializes info on
    // success, and its filename remains valid while that image is loaded.
    let found = unsafe { libc::dladdr((libc::malloc as *const ()).cast(), info.as_mut_ptr()) };
    assert_ne!(found, 0, "the linked libc image must be discoverable");
    // SAFETY: the successful dladdr call initialized info and its filename.
    let image = unsafe {
        let info = info.assume_init();
        std::ffi::CStr::from_ptr(info.dli_fname).to_bytes().to_vec()
    };
    let image = std::path::PathBuf::from(std::ffi::OsString::from_vec(image));
    let directory = std::env::temp_dir().join(format!(
        "flui-native-library-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current time follows the epoch")
            .as_nanos(),
    ));
    std::fs::create_dir(&directory).expect("create temporary library directory");
    struct RemoveDirectory(std::path::PathBuf);
    impl Drop for RemoveDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let cleanup = RemoveDirectory(directory.clone());
    let path = directory.join(std::ffi::OsString::from_vec(b"library-\xff.so".to_vec()));
    symlink(image, &path).expect("link the already-loaded native image");
    let library =
        flui_hot_reload::dynlib::DynLib::open(&path).expect("a native filename need not be UTF-8");
    // SAFETY: only check resolution; no function pointer is invoked.
    assert!(unsafe { library.symbol("malloc") }.is_some());
    drop(library);
    drop(cleanup);
}

#[test]
fn same_path_subsecond_revisions_and_recreation_are_detected() {
    use std::fs::FileTimes;
    use std::time::{Duration, SystemTime};

    let directory = std::env::temp_dir().join(format!(
        "flui-artifact-stamp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("current time follows the epoch")
            .as_nanos(),
    ));
    std::fs::create_dir(&directory).expect("create artifact directory");
    let path = directory.join("artifact.bin");
    let file = std::fs::File::create(&path).expect("create artifact");
    let second = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    file.set_times(FileTimes::new().set_modified(second + Duration::from_millis(100)))
        .expect("set initial native timestamp");
    let first = flui_hot_reload::worker_artifact_stamp(&path);
    let seconds_only = first
        .1
        .expect("metadata available")
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("timestamp follows epoch")
        .as_secs();
    file.set_times(FileTimes::new().set_modified(second + Duration::from_millis(900)))
        .expect("set replacement native timestamp");
    let second_revision = flui_hot_reload::worker_artifact_stamp(&path);
    assert_eq!(
        second_revision
            .1
            .expect("metadata available")
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp follows epoch")
            .as_secs(),
        seconds_only
    );
    assert_ne!(
        first, second_revision,
        "same-second rebuild must change native stamp"
    );
    drop(file);
    std::fs::remove_file(&path).expect("remove artifact");
    let unavailable = flui_hot_reload::worker_artifact_stamp(&path);
    assert!(unavailable.1.is_none());
    assert_ne!(second_revision, unavailable);
    std::fs::write(&path, b"recreated").expect("recreate artifact");
    assert_ne!(flui_hot_reload::worker_artifact_stamp(&path), unavailable);
    std::fs::remove_dir_all(path.parent().expect("scratch directory"))
        .expect("remove scratch directory");
}
