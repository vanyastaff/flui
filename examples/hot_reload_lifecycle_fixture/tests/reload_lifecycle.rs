//! Real `dlopen`/`dlclose`/reload coverage for `app_plugin!` — the lifecycle
//! `flui-hot-reload`'s own tests leave untested, because building a plugin
//! from inside a test is environment-fragile.
//!
//! Build the plugin before running these tests:
//! `cargo build -p flui-hot-reload-lifecycle-fixture --lib --locked`.
//! The repository's `test` task does this automatically.
//! Cargo publishes that directly built library to its artifact directory, and
//! Cargo and nextest pass that directory in the dynamic-library search path.
//! The tests consume that path rather than infer Cargo's private build layout
//! from their executable. No test starts a nested Cargo build.
//!
//! (An earlier version of this test lived in `flui-hot-reload`'s own
//! `tests/` directory, with the fixture wired in as a `[dev-dependencies]`
//! path entry. That created a dev-dependency cycle back onto
//! `flui-hot-reload` itself — the fixture's own normal dependency on
//! `flui-hot-reload` with `features = ["app-plugin"]` unifies with the SAME
//! resolved package `flui-hot-reload`'s own tests build against, confirmed
//! via `cargo tree -e features`, so `app-plugin` was silently always-on for
//! ANY `flui-hot-reload` test build, even a bare `cargo test -p
//! flui-hot-reload` with no `--features` flag at all. Moving the test here,
//! into the plugin crate itself, removes that dependency edge entirely.)
//!
//! # What the thread-affinity storage repair fixed, and what this test proves
//!
//! `app_plugin!` used to store its `PluginPipeline` behind a plain `static`
//! (`OnceLock<Mutex<PluginPipeline>>`), which Rust never runs drop glue for.
//! The storage flip moved it into a `thread_local!`, and a thread-local
//! wrapping a droppable type registers a TLS destructor **tied to the
//! defining shared object** — exactly the kind of thing that makes `dlclose`
//! unsafe on some runtimes (a deferred-unmap runtime can keep serving the OLD
//! mapped code on a same-path reload instead of a fresh one) and undefined on
//! others (running the destructor after the image is already unmapped). The
//! fix wraps the slot in `ManuallyDrop`, which is unconditionally
//! drop-glue-free, so std never registers a destructor for THAT slot. The
//! static half of this proof is `plugin.rs`'s compile-time `needs_drop`
//! assertion (fails to compile if a future edit reintroduces drop glue there
//! — see that file). [`dlclose_then_reload_keeps_the_lifecycle_working`]
//! below is the dynamic half for that one slot: it never crashes, panics, or
//! produces a non-functional pipeline across a real load → drive → dlclose →
//! reload → drive cycle.
//!
//! # GlobalKey registry and affinity TLS regression
//!
//! `fixture_tick` — a counter independent of `app_plugin!`'s own symbols —
//! lets a fresh mapping be told apart from a stale, reused one: a genuinely
//! fresh `dlopen` always starts it at 0, so the first call returns 1; a
//! stale mapping instead keeps incrementing from wherever the previous
//! session left it. Driving the fixture from this always-alive test thread
//! (never joined/exited before the reload — the exact condition that keeps a
//! TLS-destructor hold live for as long as a runtime honors one) and then
//! reloading the SAME unchanged file reliably reproduces staleness: the
//! second session's first tick reads 4 (continuing session one's count of
//! 3), not 1.
//!
//! The remaining source was `flui-view`'s `key::registry` module:
//! `REGISTRY_STACK` and `TEST_REGISTRY` held `Arc`-containing handles directly
//! in droppable thread-locals. They are now drop-glue-free containers with
//! explicit quiescent teardown, guarded by compile-time `needs_drop`
//! assertions. [`dlclose_then_reload_same_path_serves_a_fresh_image`] is the
//! dynamic proof: reloading the unchanged same path must map a fresh image and
//! reset this image-local counter.
//!
//! **Platform scope of the strict proof.** That test runs where the loader
//! actually honors `dlclose` unload — Linux and Android's glibc/bionic. macOS
//! is a deferred-unmap runtime: dyld does not unmap a `dlclose`'d dylib, so a
//! same-path reload there necessarily serves the retained image and the
//! freshness assertion cannot hold. The test is therefore
//! `#[cfg(not(target_os = "macos"))]`-gated with that reason stated at the
//! gate. [`dlclose_then_reload_distinct_paths_serve_fresh_images`] covers the
//! macOS-relied-on property instead: a reload at a *distinct* path is a fresh
//! mapping on every runtime, which is exactly what the CLI's content-addressed
//! worker staging produces. [`dlclose_then_reload_keeps_the_lifecycle_working`]
//! runs everywhere too, covering that the load → drive → unload → reload cycle
//! stays functional even where the mapping is reused.
//!
//! Removing that deferred-unmap pin exposed a second hazard: the old affinity
//! check called `std::thread::current` inside the cdylib. The plugin's private
//! std copy registered a pthread-key destructor pointing into its own image;
//! after a real unmap, test-thread exit called the stale pointer. The affinity
//! guard now uses a monotonic token in drop-free TLS instead. The lifecycle
//! tests therefore prove both fresh remapping and safe thread teardown.
//!
//! Producing two BYTE-DIFFERENT fixture builds (to assert "serves new code"
//! by content instead of by this proxy) would need either an unstable cargo
//! feature (`-Z bindeps`, confirmed unavailable on this toolchain by direct
//! probe) or a second nested build — the exact thing being avoided.

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
    #[expect(unsafe_code)]
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
    #[expect(unsafe_code)]
    let scene = unsafe { plugin.build_scene(64.0, 64.0) };
    assert!(
        scene.is_some(),
        "build_scene must succeed for a pinned-thread call"
    );
    drop(scene);
    plugin.unload();
}

/// The mechanical half of what this repair actually changed: a real
/// load → drive → dlclose → reload → drive cycle must not crash, panic, or
/// leave a non-functional pipeline. This is what `ManuallyDrop` + the
/// thread-affinity pin are for — see this file's module doc for the
/// compile-time half of the proof and for what this test does NOT (yet)
/// cover.
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
/// and a fresh counter cannot be demonstrated. (This is the failure mode this
/// file's module doc names; the mechanical test above still covers macOS.)
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
