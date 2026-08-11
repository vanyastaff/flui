//! Real `dlopen`/`dlclose`/reload coverage for `app_plugin!` — the lifecycle
//! `flui-hot-reload`'s own `tests/loader.rs` deliberately leaves untested
//! ("environment-fragile... would violate the no-flaky-tests rule").
//!
//! That rule is about *nested `cargo build` from inside a test* — spawning a
//! second cargo process, racing target-dir locks, non-deterministic compile
//! times. This test avoids all of that by living where its plugin already
//! is: `flui-hot-reload-lifecycle-fixture` is `crate-type = ["rlib",
//! "cdylib"]` (see `Cargo.toml`), so building THIS test target — an ordinary
//! integration test of the crate it ships in — necessarily also produces the
//! crate's own `cdylib` artifact in the same rustc invocation, no separate
//! dependency edge or nested cargo required.
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

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use flui_hot_reload::PluginKind;
use flui_hot_reload::ScenePlugin;
use flui_hot_reload::dynlib::DynLib;

/// A self-cleaning temp file path unique to this test process (mirrors
/// `tests/loader.rs`'s `TempPath` — duplicated rather than shared, since
/// each `tests/*.rs` file compiles as its own independent crate).
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

/// This crate's own `cdylib` build artifact, somewhere under
/// `target/<profile>/` — [`std::env::current_exe`]'s directory is
/// `target/<profile>/deps/`, right next to this test binary itself.
/// Building this integration test target compiles the crate under test
/// (`flui-hot-reload-lifecycle-fixture`) with all of its declared
/// crate-types in one rustc invocation, so the `cdylib` output always
/// exists by the time this function runs — no separate dependency edge, no
/// nested cargo.
///
/// Resolution order (first hit wins), confirmed by direct probe under both
/// `cargo nextest run -p flui-hot-reload-lifecycle-fixture` on its own and
/// `just ci`'s workspace-wide build:
///
/// 1. `target/<profile>/<lib_prefix>flui_hot_reload_lifecycle_fixture<lib_suffix>`
///    — cargo uplifts a package's `cdylib` here for a plain `cargo
///    build`/`check` that treats the crate as a directly-built unit. Neither
///    invocation above is that shape (both are `--tests`-driven, which only
///    needs the `cdylib` as a link artifact, not a directly-built one), so
///    this tier costs one extra `is_file` check and doesn't resolve for this
///    test's own invocations today — kept for a build shape that IS a plain
///    `cargo build`.
/// 2. The same unhashed filename inside `target/<profile>/deps/` — the tier
///    that actually resolves for this test's own invocations, confirmed by
///    direct probe under both of the above.
/// 3. The hash-suffixed candidates cargo always writes to `deps/`
///    (`<stem>-<hash>.<ext>`), newest by mtime — a defensive fallback for a
///    hypothetical cargo/platform combination where neither unhashed copy
///    exists.
fn fixture_artifact_path() -> PathBuf {
    let exe = std::env::current_exe().expect("BUG: a running test binary must have a path");
    let deps_dir = exe
        .parent()
        .expect("BUG: a test binary always has a containing directory");
    let profile_dir = deps_dir
        .parent()
        .expect("BUG: target/<profile>/deps/ always has a target/<profile>/ parent");

    #[cfg(target_os = "windows")]
    let (lib_prefix, lib_ext) = ("", "dll");
    #[cfg(target_os = "macos")]
    let (lib_prefix, lib_ext) = ("lib", "dylib");
    #[cfg(all(unix, not(target_os = "macos")))]
    let (lib_prefix, lib_ext) = ("lib", "so");

    let stem = format!("{lib_prefix}flui_hot_reload_lifecycle_fixture");
    let unhashed_name = format!("{stem}.{lib_ext}");

    // 1. Preferred: unhashed, uplifted to the profile root.
    let top_level = profile_dir.join(&unhashed_name);
    if top_level.is_file() {
        return top_level;
    }

    // 2. Fallback: unhashed, still inside `deps/`.
    let in_deps = deps_dir.join(&unhashed_name);
    if in_deps.is_file() {
        return in_deps;
    }

    // 3. Last resort: newest hash-suffixed candidate in `deps/`.
    let hashed_prefix = format!("{stem}-");
    let hashed_suffix = format!(".{lib_ext}");
    let newest_hashed = std::fs::read_dir(deps_dir)
        .expect("BUG: a test binary's own deps/ directory must be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&hashed_prefix) && name.ends_with(&hashed_suffix)
                })
        })
        .max_by_key(|path| {
            path.metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });

    newest_hashed.unwrap_or_else(|| {
        panic!(
            "fixture artifact not found under {} (checked the unhashed name at the profile \
             root, the unhashed name in deps/, and hash-suffixed candidates in deps/) — \
             expected building this test target to have produced this crate's own cdylib \
             before this test ran (see this file's module doc)",
            profile_dir.display()
        )
    })
}

/// Resolve and call the fixture's `fixture_tick` symbol through an already
/// open handle. Keeping one handle across several calls proves the counter is
/// image-local (1 → 2 → 3); dropping it before opening the next handle then
/// proves whether `dlclose` really unmapped that image.
fn tick(lib: &DynLib) -> u32 {
    // SAFETY: `fixture_tick` is the fixture's own exported `extern "C" fn()
    // -> u32` (see examples/hot_reload_lifecycle_fixture/src/lib.rs); the
    // symbol name and signature are this test's own contract with that
    // fixture, both compiled from this same workspace/toolchain.
    #[allow(unsafe_code)]
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
    #[allow(unsafe_code)]
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
