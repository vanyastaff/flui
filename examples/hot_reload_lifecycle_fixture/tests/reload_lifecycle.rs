//! Real `dlopen`/`dlclose`/reload coverage for the plugin macros, through a
//! cargo-built plugin: the lifecycle `flui-hot-reload`'s own tests cannot
//! reach, because building a plugin from inside a test is environment-fragile.
//!
//! Build the plugin before running these tests:
//! `cargo build -p flui-hot-reload-lifecycle-fixture --lib --locked`.
//! The repository's `test` task does this automatically.
//! Cargo publishes that directly built library to its artifact directory, and
//! Cargo and nextest pass that directory in the dynamic-library search path.
//! The tests consume that path rather than infer Cargo's private build layout
//! from their executable. No test starts a nested Cargo build.
//!
//! The tests live in the fixture crate, not in `flui-hot-reload`'s `tests/`:
//! there the fixture, a path dev-dependency that enables `app-plugin`, would
//! unify that feature into every `flui-hot-reload` test build.
//!
//! # What they prove
//!
//! A `dlclose` must unmap the plugin, so that a reload maps fresh code. A
//! droppable thread-local in the plugin registers a TLS destructor tied to
//! the image, which makes `dlclose` defer the unmap (glibc) or run the
//! destructor after the unmap (a non-deferring runtime). `app_plugin!` keeps
//! its whole thread-local state drop-glue-free, which `plugin.rs`'s
//! compile-time `needs_drop` assertion checks statically, and `flui-view`'s
//! `key::registry` keeps its registry stack drop-glue-free under its own
//! assertions. Its affinity guard uses a monotonic token in that state instead
//! of `std::thread::current`, which would register a pthread-key destructor in
//! the image.
//!
//! - [`dlclose_then_reload_keeps_the_lifecycle_working`] drives a real
//!   load → build → dlclose → reload → build cycle on every platform.
//! - [`dlclose_then_reload_same_path_serves_a_fresh_image`] reloads the same
//!   unchanged path and checks the image is fresh by `fixture_tick`, a counter
//!   independent of the macro's symbols: a fresh mapping answers 1 on its first
//!   tick, a stale one continues from the previous session. The test thread stays
//!   alive across the reload, the condition under which a TLS-destructor hold
//!   would keep the old mapping. macOS's dyld never unmaps a `dlclose`d dylib,
//!   so the test is `#[cfg(not(target_os = "macos"))]`.
//! - [`dlclose_then_reload_distinct_paths_serve_fresh_images`] covers what
//!   macOS relies on instead: a reload at a distinct path, as the CLI's
//!   content-addressed worker staging produces, is a fresh mapping on every
//!   runtime.
//!
//! Two byte-different fixture builds, which would let a test assert "serves
//! new code" by content, would need either `-Z bindeps` (unstable) or a nested
//! build; the tick counter stands in for it.

use std::path::{Path, PathBuf};

use flui_hot_reload::PluginKind;
use flui_hot_reload::ScenePlugin;
use flui_hot_reload::dynlib::DynLib;
use std::ffi::c_void;

/// A self-cleaning temp file path unique to this test process.
struct TempPath(PathBuf);

impl TempPath {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flui_hot_reload_reload_lifecycle_{tag}_{}.bin",
            std::process::id()
        ));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The directly built plugin on Cargo's dynamic-library search path.
/// Preserve the runner's lookup order, including Cargo's artifact directory
/// before dependency directories. No private hash directory or mtime is used.
fn fixture_artifact_path() -> PathBuf {
    let search_path = if cfg!(windows) {
        std::env::var_os("PATH")
    } else if cfg!(target_os = "macos") {
        // nextest preserves this value when macOS SIP sanitizes DYLD variables.
        std::env::var_os("NEXTEST_DYLD_FALLBACK_LIBRARY_PATH")
            .or_else(|| std::env::var_os("DYLD_FALLBACK_LIBRARY_PATH"))
    } else {
        std::env::var_os("LD_LIBRARY_PATH")
    }
    .expect("Cargo or nextest must provide a dynamic-library search path");
    let filename = format!(
        "{}flui_hot_reload_lifecycle_fixture{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX,
    );
    std::env::split_paths(&search_path)
        .map(|directory| directory.join(&filename))
        .find(|path| path.is_file())
        .expect("fixture missing from the runner search path; run `cargo build -p flui-hot-reload-lifecycle-fixture --lib --locked` first")
}

/// Resolve and call the fixture's `fixture_tick` symbol through an already
/// open handle. Keeping one handle across several calls proves the counter is
/// image-local (1 → 2 → 3); dropping it before opening the next handle then
/// proves whether `dlclose` really unmapped that image.
///
/// The same-path strict freshness test is `not(target_os = "macos")`-gated
/// (dyld does not unmap on `dlclose`), but the distinct-path test below uses
/// this helper on every platform — a distinct path is a fresh mapping even on
/// dyld, which is exactly the property the CLI's versioned staging relies on.
fn tick(lib: &DynLib) -> u32 {
    // SAFETY: `fixture_tick` is the fixture's own exported `extern "C" fn()
    // -> u32` (see examples/hot_reload_lifecycle_fixture/src/lib.rs); the
    // symbol name and signature are this test's own contract with that
    // fixture, both compiled from this same workspace/toolchain.
    #[expect(unsafe_code, reason = "transmutes the symbol to its signature")]
    let value = unsafe {
        let ptr = lib
            .symbol("fixture_tick")
            .expect("fixture must export fixture_tick");
        let f: extern "C" fn() -> u32 =
            std::mem::transmute::<*mut c_void, extern "C" fn() -> u32>(ptr);
        f()
    };
    value
}

/// Drive one full session: load, confirm it's an `app_plugin!`, build a
/// scene (drop it before unload, per `build_scene`'s `# Safety`), then
/// unload. Returns nothing — the point is that none of this panics.
fn load_drive_unload(path: &Path) {
    let plugin = ScenePlugin::load(path).expect("fixture must load as a ScenePlugin");
    assert_eq!(
        plugin.kind(),
        PluginKind::App,
        "the fixture is an app_plugin!, not a scene_plugin!"
    );
    // SAFETY: host and fixture are built by this same workspace/toolchain
    // (the ABI-token handshake `ScenePlugin::load` already performed
    // confirms it), and the returned scene is dropped immediately, before
    // `plugin` (and thus the library) is unloaded below.
    #[expect(unsafe_code, reason = "`build_scene` is an `unsafe fn`")]
    let scene = unsafe { plugin.build_scene(64.0, 64.0) };
    assert!(
        scene.is_some(),
        "build_scene must succeed for a pinned-thread call"
    );
    drop(scene);
    plugin.unload();
}

/// A real load → drive → dlclose → reload → drive cycle at one path does not
/// crash, panic, or leave a non-functional pipeline: a fresh thread-local
/// slot mounts a fresh pipeline whether or not the runtime reused the
/// mapping. Runs on every platform, macOS included.
#[test]
fn dlclose_then_reload_keeps_the_lifecycle_working() {
    let source = fixture_artifact_path();
    let work = TempPath::new("mechanical");
    std::fs::copy(&source, work.path()).expect("copy fixture to the watched work path");

    load_drive_unload(work.path());
    // Reload: same path, unchanged bytes. Must work exactly like the first
    // time — a fresh, empty thread-local slot mounts a fresh pipeline either
    // way, whether or not the underlying mapping was reused (the stricter
    // test below checks that separate question).
    load_drive_unload(work.path());
}

/// Strict freshness check for the GlobalKey registry TLS contract. A stale
/// mapping returns 4 on the final assertion; a freshly unloaded and remapped
/// image restarts its independent counter at 1.
///
/// Excluded on macOS: dyld is a deferred-unmap runtime — it does not unmap a
/// `dlclose`'d dylib, so a same-path reload there serves the retained image
/// and a fresh counter cannot be demonstrated; the test above still covers
/// macOS.
#[cfg(not(target_os = "macos"))]
#[test]
fn dlclose_then_reload_same_path_serves_a_fresh_image() {
    let source = fixture_artifact_path();
    let work = TempPath::new("freshness");
    std::fs::copy(&source, work.path()).expect("copy fixture to the watched work path");

    load_drive_unload(work.path());

    let first_image = DynLib::open(work.path()).expect("fixture must be a loadable library");
    assert_eq!(tick(&first_image), 1);
    assert_eq!(tick(&first_image), 2);
    assert_eq!(tick(&first_image), 3);
    drop(first_image);

    let second_image = DynLib::open(work.path()).expect("fixture must reload from the same path");

    assert_eq!(
        tick(&second_image),
        1,
        "a reload of the same path must start this fixture's tick counter fresh (1), \
         not continue the previous session's count"
    );
}

/// Freshness by **distinct path** — the property the CLI's content-addressed
/// staging depends on, and the only reload-freshness proof that holds on
/// macOS.
///
/// dyld is a deferred-unmap runtime, so a *same-path* reload can serve the
/// retained image (why the test above is `not(target_os = "macos")`). A
/// different path is a different mapping on every runtime, so loading the
/// fixture from two distinct paths must yield two independent images, each
/// starting its counter at 1. This is what makes staged names carry a content
/// hash: a rebuilt worker lands at a new path, so the host cannot be served the
/// stale one.
#[test]
fn dlclose_then_reload_distinct_paths_serve_fresh_images() {
    let source = fixture_artifact_path();

    let first = TempPath::new("distinct_first");
    let second = TempPath::new("distinct_second");
    std::fs::copy(&source, first.path()).expect("copy fixture to the first path");
    std::fs::copy(&source, second.path()).expect("copy fixture to the second path");
    assert_ne!(
        first.path(),
        second.path(),
        "the two work paths must differ — that is the property under test"
    );

    load_drive_unload(first.path());

    // Same bytes, different path: a fresh image with a fresh counter.
    let image = DynLib::open(second.path()).expect("the second path must load");
    assert_eq!(
        tick(&image),
        1,
        "a reload at a DISTINCT path must start this fixture's tick counter fresh (1) — \
         this is the property macOS content-addressed staging relies on"
    );
    assert_eq!(tick(&image), 2, "and the counter is image-local (1 → 2)");
}

/// A worker image links its own copy of `flui-hot-reload`, rebuild slot
/// included, so its `request_rebuild` reaches the hook the host registered
/// only through the entry `flui_worker_init` hands it. Fails when the
/// worker's init does not bind that entry: the call then finds the image's
/// own empty slot and only warns. The worker loads from a copy at a path of
/// its own, because an admitted worker image stays mapped (ADR-0111) and the
/// other tests reload the fixture's own path.
#[test]
fn a_worker_images_rebuild_request_runs_the_hosts_hook() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let copy = TempPath::new("worker_rebuild");
    std::fs::copy(fixture_artifact_path(), copy.path())
        .expect("copy the fixture for the worker load");
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    let registration = flui_hot_reload::register_request_rebuild(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
    });
    let worker =
        flui_hot_reload::WorkerPlugin::load(copy.path()).expect("the fixture loads as a worker");

    let image = DynLib::open(copy.path()).expect("the worker image opens again by its path");
    // SAFETY: `fixture_request_rebuild` is the fixture's own exported
    // `extern "C-unwind" fn()` (examples/hot_reload_lifecycle_fixture/src/lib.rs),
    // compiled from this workspace with this toolchain, and `image` keeps it
    // mapped through the call.
    #[expect(unsafe_code, reason = "transmutes the symbol to its signature")]
    unsafe {
        let ptr = image
            .symbol("fixture_request_rebuild")
            .expect("fixture must export fixture_request_rebuild");
        let request = std::mem::transmute::<*mut c_void, extern "C-unwind" fn()>(ptr);
        request();
    }

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the worker image's request_rebuild must run the host's hook"
    );
    drop(image);
    drop(worker);
    drop(registration);
}

/// A panic in the developer's root view, its mount or its frame cannot unwind
/// out of `flui_app_build`, an `extern "C"` entry: it aborts the host's
/// process. Fails, by aborting the test binary, when the entry runs that code
/// outside containment. The image loads from a copy at a path of its own, so
/// its root mounts fresh.
#[test]
fn a_panicking_app_root_skips_the_frame_and_the_next_build_mounts() {
    let copy = TempPath::new("panicking_root");
    std::fs::copy(fixture_artifact_path(), copy.path()).expect("copy the fixture");
    let plugin = ScenePlugin::load(copy.path()).expect("the fixture loads as a ScenePlugin");
    let image = DynLib::open(copy.path()).expect("the image opens again by its path");
    // SAFETY: `fixture_panic_next_mount` is the fixture's own exported
    // `extern "C" fn()`, compiled from this workspace with this toolchain, and
    // `image` keeps it mapped through the call.
    #[expect(unsafe_code, reason = "transmutes the symbol to its signature")]
    unsafe {
        let ptr = image
            .symbol("fixture_panic_next_mount")
            .expect("fixture must export fixture_panic_next_mount");
        std::mem::transmute::<*mut c_void, extern "C" fn()>(ptr)();
    }

    // SAFETY: the load's token handshake confirmed the layouts, and each
    // scene is dropped before `plugin` unloads the image below.
    #[expect(unsafe_code, reason = "`build_scene` is an `unsafe fn`")]
    let (refused, mounted) = unsafe {
        (
            plugin.build_scene(64.0, 64.0),
            plugin.build_scene(64.0, 64.0),
        )
    };
    assert!(refused.is_none(), "a panicking root is a skipped frame");
    assert!(mounted.is_some(), "the next build mounts the root again");
    drop(mounted);
    drop(image);
    plugin.unload();
}
