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

#[test]
fn dropping_an_attached_hook_joins_its_watcher() {
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

#[test]
fn scene_frame_without_a_loaded_plugin_draws_nothing() {
    let path = scratch_worker("no-scene").with_file_name(ScenePluginHook::LIBRARY_NAME);
    let mut hook = ScenePluginHook::new(&path);
    let mut renders = 0;
    assert!(!hook.scene_frame(100.0, 100.0, &mut |_scene| renders += 1));
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
