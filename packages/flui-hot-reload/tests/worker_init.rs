//! A worker's `flui_worker_init` contains a panic in the worker's own init.

fn panicking_init(_register: flui_hot_reload::RegisterWorkerBuildFn) {
    panic!("the worker's init panicked");
}

flui_hot_reload::hot_reload_worker!(panicking_init);

extern "C-unwind" fn no_rebuild() {}

/// A panic cannot unwind out of the `extern "C"` entry: it aborts the host's
/// process. Fails, by aborting the test binary, when the entry runs the
/// worker's init outside containment.
#[test]
fn a_panicking_worker_init_answers_false() {
    assert!(
        !flui_worker_init(flui_hot_reload::host_register_fn(), no_rebuild),
        "a worker whose init panicked answers false"
    );
}
