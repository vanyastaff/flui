//! A malformed environment override must not leave the process with no
//! subscriber.
//!
//! This is the failure an Android entry point can hit for real: `FilterConfig`
//! honours `RUST_LOG`, a device can carry a malformed value, and `setup` alone
//! returns `Err` in that case. An entry point that discards the error then has
//! no subscriber at all, so every diagnostic after it — including the one
//! explaining why — vanishes.
//!
//! Exactly one test in this binary. It writes the process-global subscriber
//! slot *and* an environment variable, and both need to be the only such write
//! in the process — under `cargo test` (one process per file) as much as under
//! `cargo nextest` (one per test).

mod support;

use flui_log::{FilterConfig, LogConfig, SubscriberOwnership, SubscriberPolicy};
use support::HostSubscriber;

/// Not `RUST_LOG`: an ambient value in the developer's shell must not be able
/// to change what this test measures.
const TEST_ENV_VAR: &str = "FLUI_LOG_TEST_REJECTED_OVERRIDE";

/// Rejected by `EnvFilter`, and not merely by whitespace trimming.
const MALFORMED: &str = "=not a directive=";

#[test]
fn a_rejected_override_falls_back_and_still_installs_a_subscriber() {
    // SAFETY: `set_var` requires that no other thread is concurrently reading
    // or writing the environment. This is the only test in this binary, the
    // variable name is unique to it, and no thread has been spawned — the test
    // harness's own threads do not touch the environment.
    #[expect(
        unsafe_code,
        reason = "std::env::set_var is unsafe in edition 2024; the single-threaded precondition holds here"
    )]
    unsafe {
        std::env::set_var(TEST_ENV_VAR, MALFORMED);
    }

    let config = LogConfig::builder()
        .filter(FilterConfig::new("info,wgpu=warn").with_env_var(TEST_ENV_VAR))
        .build();

    // Precondition: the plain path really does fail here, so the fallback below
    // is exercising a genuine failure rather than a no-op. Without this the
    // test would still pass with the fallback logic deleted.
    let error = flui_log::setup(&config, SubscriberPolicy::Auto)
        .expect_err("`setup` alone must reject a malformed override");
    assert!(matches!(
        error,
        flui_log::SetupError::Filter(flui_log::FilterError::Environment { .. })
    ));

    // The resilient path: retried with the override disabled, and reported.
    let setup = flui_log::setup_with_env_fallback(&config, SubscriberPolicy::Auto)
        .expect("the configured directives are valid, so this cannot fail");

    assert_eq!(
        setup.installation.ownership,
        SubscriberOwnership::Installed,
        "the fallback must leave a subscriber installed, not nothing"
    );

    let rejected = setup
        .rejected_env_override
        .expect("the rejection must be reported, not swallowed");
    match rejected {
        flui_log::FilterError::Environment { env_var, .. } => {
            assert_eq!(
                env_var, TEST_ENV_VAR,
                "the report must name the variable that was rejected"
            );
        }
        other => panic!("expected `FilterError::Environment`, got {other:?}"),
    }

    // `Installed` has to mean the slot is genuinely taken.
    assert!(
        tracing::subscriber::set_global_default(HostSubscriber::new()).is_err(),
        "`Installed` must mean a subscriber actually holds the slot"
    );

    // And the installed subscriber has to work: the built-in `info` directive
    // admits this, which it could not do if nothing were installed.
    tracing::info!("the fallback subscriber is live");
}
