//! Headless platform: selection through `FLUI_HEADLESS`, clipboard, executor, and
//! the exit-policy hook driven through the public `Platform` surface.

// `std::env::set_var`/`remove_var` are `unsafe` in edition 2024. The safety
// condition is that no other thread reads the environment concurrently, and
// what supplies it here is nextest's process-per-test isolation — NOT
// `--test-threads 1`, which nothing in this repo sets for these tests. Under a
// plain `cargo test -p flui-platform` these calls race the other tests in the
// same binary.
#![expect(unsafe_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_platform::{
    HeadlessPlatform, Platform as _, SessionEnd, SessionEndAnswer, WindowOptions, current_platform,
    headless_platform,
};

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
// Exit-policy hook (issue #555's live-loop wiring) -- driven entirely through
// the PUBLIC `Platform`/`PlatformWindow` surface, exactly as a real embedder
// would: `open_window`, the returned window's own `close()`, `on_quit`,
// `set_exit_policy_hook`. No internal `MockWindow`/`HeadlessState` access.
// This is "the real platform loop path" for the headless backend: the same
// `PlatformHandlers::exit_policy` slot and `notify_closed` bookkeeping the
// winit backend's `CloseRequested` handler consults, not a parallel
// test-only mechanism.
// ============================================================================

/// An unset hook must not change existing behavior at all: closing every
/// tracked window is a no-op today (no `quit()` call), and this test pins
/// that every embedder/test written before this mechanism existed keeps
/// seeing exactly that.
fn closing_the_only_window_without_a_hook_never_calls_quit() {
    let platform = headless_platform();
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_handler = Arc::clone(&quit_calls);
    platform.on_quit(Box::new(move || {
        quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
    }));

    let window = platform
        .open_window(WindowOptions::default())
        .expect("headless platform should create a window");
    window.close();

    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "no exit-policy hook was installed -- closing the last window must not invoke quit"
    );
}

/// The hook's veto is honored: `quit` must not fire even after every window
/// has closed, when the hook returns `false` (the drain-before-decide
/// scenario `AppRuntime::should_exit` implements one layer up).
fn exit_policy_hook_veto_prevents_quit_even_after_the_last_window_closes() {
    let platform = headless_platform();
    platform.set_exit_policy_hook(Box::new(|| false));
    let quit_calls = Arc::new(AtomicUsize::new(0));
    let quit_calls_for_handler = Arc::clone(&quit_calls);
    platform.on_quit(Box::new(move || {
        quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
    }));

    let window = platform
        .open_window(WindowOptions::default())
        .expect("headless platform should create a window");
    window.close();

    assert_eq!(
        quit_calls.load(Ordering::SeqCst),
        0,
        "a hook that vetoes the exit must prevent quit from firing"
    );
}

/// Headless platform selection and its exit policy: the env var selects it,
/// no hook never calls quit, and a vetoing hook prevents quit. The env-var
/// case runs last because it writes process-global state.
#[test]
fn the_headless_platform_is_selected_and_its_exit_policy_is_honoured() {
    closing_the_only_window_without_a_hook_never_calls_quit();
    exit_policy_hook_veto_prevents_quit_even_after_the_last_window_closes();
    flui_headless_env_var_selects_the_headless_platform();
}

/// A simulated session end asks the registered callback once per phase, in
/// order, and returns its answer to the query.
#[test]
#[ignore = "contract: a simulated session end reaches the registered callback"]
fn simulated_session_end_reaches_the_hook() {
    let platform = HeadlessPlatform::new();
    let phases = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&phases);
    platform.on_session_end(Box::new(move |phase| {
        seen.lock().expect("phase log").push(phase);
        SessionEndAnswer::Block
    }));

    let answer = platform.simulate_session_end(SessionEnd::Query);
    platform.simulate_session_end(SessionEnd::Cancelled);

    assert_eq!(
        *phases.lock().expect("phase log"),
        [SessionEnd::Query, SessionEnd::Cancelled],
        "each simulated phase reaches the callback once, in order"
    );
    assert_eq!(
        answer,
        SessionEndAnswer::Block,
        "the query returns the callback's answer"
    );
}
