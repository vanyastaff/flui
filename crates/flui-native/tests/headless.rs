//! Headless platform: selection through `FLUI_HEADLESS`, clipboard, executor, and
//! the application handler's exit policy driven through the public `Platform` surface.

// `std::env::set_var`/`remove_var` are `unsafe` in edition 2024. The safety
// condition is that no other thread reads the environment concurrently, and
// what supplies it here is nextest's process-per-test isolation — NOT
// `--test-threads 1`, which nothing in this repo sets for these tests. Under a
// plain `cargo test -p flui-native` these calls race the other tests in the
// same binary.
#![expect(unsafe_code)]

#[path = "support/app.rs"]
mod app;

use app::{AppCall, AppLog, run_keeping_owner};
use flui_native::{HeadlessPlatform, current_platform};
use flui_platform::WindowOptions;
use flui_testing::window_commands::AdmitAndApply as _;

fn flui_headless_env_var_selects_the_headless_platform() {
    // Set environment variable
    // SAFETY: no other thread reads the environment concurrently — supplied
    // by nextest's process-per-test isolation, not by any `--test-threads`
    // setting. See the file header.
    unsafe { std::env::set_var("FLUI_HEADLESS", "1") };

    let platform = current_platform().expect("Failed to get platform");

    assert_eq!(
        platform.name(),
        "Headless",
        "Expected headless platform when FLUI_HEADLESS=1"
    );

    // Clean up
    // SAFETY: no other thread reads the environment concurrently — supplied
    // by nextest's process-per-test isolation, not by any `--test-threads`
    // setting. See the file header.
    unsafe { std::env::remove_var("FLUI_HEADLESS") };
}

// ============================================================================
// Exit policy (issue #555's live-loop wiring) -- driven entirely through the
// public surface, exactly as a real embedder would: `run` with a handler,
// `open_window` on the owner, the window's own `WindowCommand::Close`. This is
// the same `notify_closed` bookkeeping the winit backend's `CloseRequested`
// handler consults, not a parallel test-only mechanism.
// ============================================================================

/// Closing the last window asks the handler; its veto keeps the platform
/// running, and its consent quits it, which the handler hears once.
fn closing_the_last_window_asks_the_handler_whether_to_quit() {
    for allow in [false, true] {
        let log = AppLog::new();
        log.answer_exit(allow);
        let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
        let window = owner
            .open_window(
                WindowOptions::default(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("headless platform should create a window")
            .try_ready()
            .expect("a headless window opens at once");
        window.close_now();

        assert_eq!(log.count(AppCall::ExitPolicy), 1, "asked once");
        assert_eq!(
            log.count(AppCall::Quit),
            usize::from(allow),
            "a veto keeps the platform, consent quits it"
        );
    }
}

/// A platform that never ran has no handler to ask: closing its last window
/// keeps it running.
fn closing_the_last_window_before_run_never_quits() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let window = flui_native::Platform::open_window(
        &platform,
        WindowOptions::default(),
        flui_testing::window_handler::RecordingWindowHandler::new(),
    )
    .expect("headless platform should create a window");
    window.close_now();

    let log = AppLog::new();
    let _owner = run_keeping_owner(Box::new(platform), &log);
    events.reopen();
    assert_eq!(
        log.calls(),
        [AppCall::Ready, AppCall::Reopen],
        "nothing quit the platform before it ran"
    );
}

/// Headless platform selection and its exit policy: the env var selects it,
/// and the handler decides whether closing the last window quits. The env-var
/// case runs last because it writes process-global state.
#[test]
fn the_headless_platform_is_selected_and_its_exit_policy_is_honoured() {
    closing_the_last_window_asks_the_handler_whether_to_quit();
    closing_the_last_window_before_run_never_quits();
    flui_headless_env_var_selects_the_headless_platform();
}
