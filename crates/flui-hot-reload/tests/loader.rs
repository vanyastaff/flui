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
//!
//! `examples/hot_reload_lifecycle_fixture/tests/reload_lifecycle.rs` is the
//! ONE deliberate exception to "no built plugin inside a test": it needs a
//! real `dlopen`/`dlclose` cycle to observe the unload hazard `app_plugin!`'s
//! `ManuallyDrop` thread-local storage exists to avoid. It lives in that
//! fixture crate itself (not here) precisely so it needs no `cargo build`
//! invocation, nested or otherwise, and no dependency edge back onto this
//! crate — see that file's module doc.

use std::fs;
use std::path::PathBuf;

use flui_hot_reload::dynlib::DynLib;
#[cfg(unix)]
use flui_testing::log_capture::capture;
#[cfg(unix)]
use tracing::Level;

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
fn dynlib_open_non_library_file_is_none() {
    // A plain text file is not a loadable shared library on any platform.
    let temp = TempPath::new("not_a_lib");
    fs::write(temp.path(), b"this is not a shared library").unwrap();
    assert!(
        DynLib::open(temp.path()).is_none(),
        "a non-library file must fail to load and return None",
    );
}
