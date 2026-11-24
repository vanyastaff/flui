use std::sync::mpsc;
use std::time::Instant;

use super::*;

/// A worker path under a fresh scratch directory, which does not exist yet.
fn scratch_worker(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "flui-hot-reload-hook-{}-{test}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join("worker-that-is-not-a-library.bin")
}

/// A hook watching `path`, polling fast enough for a test.
fn fast_hook(path: &Path) -> WorkerReloadHook {
    let mut hook = WorkerReloadHook::new(path);
    hook.watch_interval = Duration::from_millis(10);
    hook
}

/// When the hook's driver last read the artifact.
fn driver_read(hook: &WorkerReloadHook) -> Instant {
    hook.driver
        .as_ref()
        .expect("the test installed a driver")
        .last_poll
}

/// A wake that reports each call on a channel.
fn channel_wake() -> (ReloadWake, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    (
        ReloadWake::new(move || {
            let _ = sender.send(());
        }),
        receiver,
    )
}

#[test]
fn event_for_maps_each_driver_outcome() {
    use ReloadEvent::{Patched, Unchanged};
    use WorkerPollOutcome::{Degraded, NoChange, ReloadFailed, Reloaded};

    let degraded = Degraded {
        old_fingerprint: 1,
        new_fingerprint: 2,
    };
    let reloaded = Reloaded { reload_count: 2 };
    for (outcome, requested, expected) in [
        (reloaded, false, Patched),
        (reloaded, true, Patched),
        (NoChange, false, Unchanged),
        (NoChange, true, Patched),
        (degraded, false, Unchanged),
        (degraded, true, Patched),
        (ReloadFailed, false, Unchanged),
        (ReloadFailed, true, Patched),
    ] {
        assert_eq!(
            event_for(outcome, requested),
            expected,
            "{outcome:?}, rebuild requested: {requested}"
        );
    }
}

#[cfg(feature = "app-plugin")]
#[test]
fn a_rebuild_request_polls_as_a_patch_and_wakes_once_until_detached() {
    let _registry = crate::dispatch::REBUILD_HOOK_TEST_LOCK.lock();
    let mut hook = WorkerReloadHook::new(scratch_worker("rebuild-request"));
    // No watcher: only the rebuild registration may wake the host here.
    hook.watch_path = PathBuf::new();
    let (wake, wakes) = channel_wake();
    hook.attach(wake);
    assert_eq!(hook.poll(), ReloadEvent::Unchanged, "nothing requested yet");

    // A worker's gesture handler, on any thread.
    std::thread::spawn(crate::request_rebuild)
        .join()
        .expect("the request runs");
    assert!(wakes.try_recv().is_ok(), "the request woke the host");
    assert!(wakes.try_recv().is_err(), "exactly once");
    assert_eq!(
        hook.poll(),
        ReloadEvent::Patched,
        "the next poll reports it"
    );
    assert_eq!(hook.poll(), ReloadEvent::Unchanged, "and only that poll");

    hook.detach();
    crate::request_rebuild();
    assert!(
        wakes.try_recv().is_err(),
        "a detached hook's registration is gone: no wake"
    );
    assert_eq!(hook.poll(), ReloadEvent::Unchanged, "and no patch");
}

#[test]
fn an_artifact_change_wakes_the_host() {
    #[cfg(feature = "app-plugin")]
    let _registry = crate::dispatch::REBUILD_HOOK_TEST_LOCK.lock();
    let path = scratch_worker("artifact-change");
    let mut hook = fast_hook(&path);
    let (wake, wakes) = channel_wake();
    hook.attach(wake);
    std::thread::sleep(Duration::from_millis(50));
    assert!(wakes.try_recv().is_err(), "no change, no wake");

    std::fs::write(&path, b"a rebuilt worker").expect("write the artifact");
    // Timing-bounded: the watcher polls every 10 ms, the bound is generous.
    wakes
        .recv_timeout(Duration::from_secs(10))
        .expect("the watcher woke the host after the artifact appeared");

    hook.detach();
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch dir"));
}

/// An idle host draws one frame for the watcher's wake, so that frame's poll
/// reads the artifact even when a frame polled moments before: held to the
/// driver's poll interval it would report no change, and nothing would draw
/// again to pick the rebuild up.
#[test]
fn the_poll_a_watcher_wakes_reads_the_artifact_inside_the_poll_interval() {
    #[cfg(feature = "app-plugin")]
    let _registry = crate::dispatch::REBUILD_HOOK_TEST_LOCK.lock();
    let path = scratch_worker("woken-poll");
    let mut hook = fast_hook(&path);
    hook.driver =
        Some(WorkerReloadDriver::new(&path).with_poll_interval(Duration::from_secs(3600)));
    let (wake, wakes) = channel_wake();
    hook.attach(wake);
    assert_eq!(hook.poll(), ReloadEvent::Unchanged);
    let read_before = driver_read(&hook);

    // Renamed into place, as a build lands it: the watcher sees one change.
    let staged = path.with_extension("staged");
    std::fs::write(&staged, b"a rebuilt worker").expect("write the artifact");
    std::fs::rename(&staged, &path).expect("land the artifact");
    wakes
        .recv_timeout(Duration::from_secs(10))
        .expect("the watcher woke the host after the artifact appeared");
    hook.poll();
    assert!(
        driver_read(&hook) > read_before,
        "the woken poll read the artifact although the interval has not passed"
    );
    let read_on_wake = driver_read(&hook);
    hook.poll();
    assert_eq!(
        driver_read(&hook),
        read_on_wake,
        "a later poll with no new change keeps to the interval"
    );

    hook.detach();
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch dir"));
}

/// A rebuild that lands between the hook being made and attached, which the
/// watcher's baseline already holds, is read by the first poll after attach.
#[test]
fn the_first_poll_after_attach_reads_a_rebuild_made_before_it() {
    #[cfg(feature = "app-plugin")]
    let _registry = crate::dispatch::REBUILD_HOOK_TEST_LOCK.lock();
    let path = scratch_worker("before-attach");
    let mut hook = fast_hook(&path);
    hook.driver =
        Some(WorkerReloadDriver::new(&path).with_poll_interval(Duration::from_secs(3600)));
    let made = driver_read(&hook);
    std::fs::write(&path, b"a rebuilt worker").expect("write the artifact");

    let (wake, _wakes) = channel_wake();
    hook.attach(wake);
    hook.poll();
    assert!(
        driver_read(&hook) > made,
        "the first poll after attach read the artifact inside the poll interval"
    );

    hook.detach();
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch dir"));
}

#[test]
fn dropping_an_attached_hook_joins_its_watcher() {
    #[cfg(feature = "app-plugin")]
    let _registry = crate::dispatch::REBUILD_HOOK_TEST_LOCK.lock();
    let path = scratch_worker("drop-joins");
    let mut hook = fast_hook(&path);
    let token = Arc::new(());
    let held = Arc::clone(&token);
    hook.attach(ReloadWake::new(move || {
        let _ = &held;
    }));
    assert!(Arc::strong_count(&token) > 1, "the watcher holds the wake");

    let started = Instant::now();
    drop(hook);
    assert_eq!(
        Arc::strong_count(&token),
        1,
        "drop returned only after the watcher thread (and its wake) were gone"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the join is bounded"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch dir"));
}

fn scene_frame_without_a_loaded_plugin_draws_nothing() {
    let path = scratch_worker("no-scene").with_file_name(ScenePluginHook::LIBRARY_NAME);
    let mut hook = ScenePluginHook::new(&path);
    let mut renders = 0;
    // SAFETY: no image is loaded, and this callback retains no scene data.
    #[expect(unsafe_code, reason = "`scene_frame` is an `unsafe fn`")]
    let rendered = unsafe {
        hook.scene_frame(100.0, 100.0, &mut |_scene, _reset_fonts| {
            renders += 1;
            true
        })
    };
    assert!(!rendered);
    assert_eq!(renders, 0, "no plugin, nothing rendered");
    assert_eq!(hook.poll(), ReloadEvent::Unchanged);
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch dir"));
}

#[test]
fn the_device_library_path_is_where_flui_run_scene_pushes_it() {
    assert_eq!(
        ScenePluginHook::device_library_path(Some(Path::new("/data/data/com.example/files"))),
        Path::new("/data/data/com.example/files/libflui_scene.so")
    );
    assert_eq!(
        ScenePluginHook::device_library_path(None),
        Path::new("/data/local/tmp/libflui_scene.so")
    );
}

fn first_render_and_replacement_reset_fonts() {
    let path = scratch_worker("font-reset-generations");
    let mut hook = ScenePluginHook::new(&path);
    let scene = Scene::default();
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(reset, "first image requires its own font namespace");
            true
        }
    ));
    hook.observe_image_update(false);
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(!reset, "successful rendering acknowledged the reset");
            true
        }
    ));
    hook.observe_image_update(true);
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(reset, "replacement image may reuse font identifiers");
            true
        }
    ));
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(!reset, "replacement reset acknowledged exactly once");
            true
        }
    ));
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch directory"));
}

fn failed_render_preserves_font_reset_for_retry() {
    let path = scratch_worker("font-reset-refusal");
    let mut hook = ScenePluginHook::new(&path);
    let scene = Scene::default();
    assert!(!ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(reset);
            false
        }
    ));
    hook.observe_image_update(false);
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(reset, "unchanged image must still retry the failed reset");
            true
        }
    ));
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(!reset);
            true
        }
    ));
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch directory"));
}

fn panicking_render_preserves_font_reset_for_retry() {
    let path = scratch_worker("font-reset-panic");
    let mut hook = ScenePluginHook::new(&path);
    let scene = Scene::default();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ScenePluginHook::render_loaded_scene(
            &mut hook.pending_font_reset,
            &scene,
            &mut |_, reset| {
                assert!(reset);
                panic!("render failure");
            },
        );
    }));
    assert!(outcome.is_err());
    hook.observe_image_update(false);
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(
                reset,
                "unwinding render cannot acknowledge the namespace reset"
            );
            true
        }
    ));
    assert!(ScenePluginHook::render_loaded_scene(
        &mut hook.pending_font_reset,
        &scene,
        &mut |_, reset| {
            assert!(!reset);
            true
        }
    ));
    let _ = std::fs::remove_dir_all(path.parent().expect("scratch directory"));
}

#[test]
fn scene_frame_reset_recovery_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "no_plugin",
            scene_frame_without_a_loaded_plugin_draws_nothing,
        ),
        (
            "first_and_replacement",
            first_render_and_replacement_reset_fonts,
        ),
        (
            "render_refused",
            failed_render_preserves_font_reset_for_retry,
        ),
        (
            "render_panicked",
            panicking_render_preserves_font_reset_for_retry,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed cases: {failures:?}");
}

/// A watcher thread that died on a panic hands its payload to the join.
/// Fails when the join drops the payload outside containment: the payload's
/// destructor panics again, out of `Drop`.
#[test]
fn a_watcher_that_panicked_is_joined_with_its_payload_contained() {
    struct PanicsOnDrop;
    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("the payload's destructor panicked");
        }
    }
    let watcher = ArtifactWatcher {
        stop: Arc::new(AtomicBool::new(false)),
        handle: Some(std::thread::spawn(|| std::panic::panic_any(PanicsOnDrop))),
    };
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(watcher)));
    assert!(dropped.is_ok(), "dropping the watcher does not panic");
}
