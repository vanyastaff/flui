//! flui-app's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
// `Application` is the desktop entry point.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[path = "capability_registration.rs"]
mod capability_registration;

#[path = "config_options.rs"]
mod config_options;

#[path = "data_transfer_transport.rs"]
mod data_transfer_transport;

#[path = "execution_public_paths.rs"]
mod execution_public_paths;

#[path = "runner_frame_ordering.rs"]
mod runner_frame_ordering;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[path = "runner_keyboard.rs"]
mod runner_keyboard;

// The runner's test kit drives a headless desktop loop, which Android, iOS
// and the web have no counterpart for.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[path = "runner_teardown.rs"]
mod runner_teardown;

// The same kit, with a main window that renders with its stand-in renderer.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[path = "runner_close.rs"]
mod runner_close;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[path = "headless_storage.rs"]
mod headless_storage;

// The driver agent's threads are read from `/proc`; the property is the runner's, the same on
// every desktop.
#[cfg(target_os = "linux")]
#[path = "agent_unlaunched.rs"]
mod agent_unlaunched;

/// Runs every case even after one fails, then panics listing the failing case names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[test]
fn capability_registration() {
    run_table(
        "capability_registration",
        &[
            (
                "capability_registration::conflicting_registrations_fail_run_before_the_platform_starts",
                capability_registration::conflicting_registrations_fail_run_before_the_platform_starts
                    as fn(),
            ),
            (
                "capability_registration::a_conflicting_launch_fails_run_app_before_the_platform_starts",
                capability_registration::a_conflicting_launch_fails_run_app_before_the_platform_starts
                    as fn(),
            ),
        ],
    );
}

// An application on Linux runs on the native host of its session.
#[cfg(target_os = "linux")]
#[path = "native_linux.rs"]
mod native_linux;

#[cfg(target_os = "linux")]
#[test]
fn native_linux() {
    run_table(
        "native_linux",
        &[
            (
                "native_linux::an_app_runs_on_the_native_wayland_host",
                native_linux::an_app_runs_on_the_native_wayland_host as fn(),
            ),
            (
                "native_linux::an_app_runs_on_the_native_x11_host",
                native_linux::an_app_runs_on_the_native_x11_host as fn(),
            ),
        ],
    );
}

/// The application `native_linux` runs in a child process of this binary;
/// a no-op unless that case started it.
#[cfg(target_os = "linux")]
#[test]
fn native_linux_child() {
    native_linux::child();
}
