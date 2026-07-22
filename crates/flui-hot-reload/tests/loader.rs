//! Deterministic coverage for the hot-reload loader's edge handling and the
//! mtime-based update-detection that `ScenePlugin::has_update` is built on.
//!
//! The happy-path "load a real plugin and call `build_scene`" flow needs a
//! *built* `scene_plugin!` cdylib (see `examples/desktop_scene`); that end-to-end
//! is exercised by the desktop example, not here, because building + loading a
//! shared library inside a unit test is environment-fragile (nested cargo, target
//! locks) and would violate the no-flaky-tests rule. These tests pin the parts
//! that ARE deterministic: the loader rejects bad inputs, and `file_mtime`
//! reflects and detects an on-disk change (the reload trigger).

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use flui_hot_reload::ScenePlugin;
use flui_hot_reload::dynlib::{DynLib, file_mtime};

/// A self-cleaning temp file path unique to each test (no `tempfile` dep).
struct TempPath(PathBuf);

impl TempPath {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        // Unique per (test, process) without Date/rand: the tag is distinct per
        // call site and the pid disambiguates concurrent test runs.
        path.push(format!(
            "flui_hot_reload_test_{tag}_{}.bin",
            std::process::id()
        ));
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn dynlib_open_missing_file_is_none() {
    let missing = std::env::temp_dir().join("flui_hot_reload_definitely_absent.dll");
    assert!(
        DynLib::open(&missing).is_none(),
        "opening a non-existent path must return None, not panic or a dangling handle",
    );
}

#[test]
fn dynlib_open_non_library_file_is_none() {
    // A plain text file is not a loadable shared library on any platform.
    let temp = TempPath::new("not_a_lib");
    fs::write(temp.path(), b"this is not a shared library").unwrap();
    assert!(
        DynLib::open(temp.path()).is_none(),
        "a non-library file must fail to load and return None",
    );
}

#[test]
fn scene_plugin_load_of_non_plugin_is_none() {
    // Even if the file exists, without the plugin symbols `load` yields None.
    let temp = TempPath::new("non_plugin");
    fs::write(temp.path(), b"\x7fELF not really").unwrap();
    assert!(
        ScenePlugin::load(temp.path()).is_none(),
        "a file without flui_app_build / flui_scene_build symbols is not a plugin",
    );
}

#[test]
fn file_mtime_of_missing_path_is_zero() {
    let missing = std::env::temp_dir().join("flui_hot_reload_absent_mtime.bin");
    assert_eq!(
        file_mtime(&missing),
        0,
        "a missing file reports mtime 0 (the documented sentinel)",
    );
}

#[test]
fn file_mtime_detects_an_on_disk_modification() {
    // This is the foundation of `ScenePlugin::has_update`: a rebuilt library has
    // a newer mtime, so the host knows to reload. mtime is second-resolution, so
    // set the two timestamps explicitly (10s apart) rather than relying on
    // wall-clock spacing between writes — deterministic, not flaky.
    let temp = TempPath::new("mtime_change");
    fs::write(temp.path(), b"v1").unwrap();

    let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    // Setting the modified time needs a write-capable handle on Windows.
    fs::OpenOptions::new()
        .write(true)
        .open(temp.path())
        .unwrap()
        .set_modified(t1)
        .unwrap();
    let m1 = file_mtime(temp.path());
    assert_eq!(
        m1, 1_700_000_000,
        "mtime reflects the set modification time"
    );

    let t2 = t1 + Duration::from_secs(10);
    fs::OpenOptions::new()
        .write(true)
        .open(temp.path())
        .unwrap()
        .set_modified(t2)
        .unwrap();
    let m2 = file_mtime(temp.path());

    assert_eq!(m2, 1_700_000_010);
    assert!(
        m2 != m1,
        "a changed mtime must be observable — this is exactly what has_update compares ({m1} vs {m2})",
    );
}
