//! The configured environment variable is really read, and really trimmed.
//!
//! `filter.rs`'s unit tests drive `env_filter_from` with an explicit value, so
//! they cover the resolution *rules* deterministically. This binary covers the
//! one thing they cannot: that `env_filter` reaches `std::env` at all, and
//! carries the same trimming behaviour when it does.
//!
//! Its own process, and its own variable name. `std::env::set_var` is `unsafe`
//! in edition 2024 because another thread reading the environment concurrently
//! is a data race; a uniquely-named variable written before any other thread
//! exists keeps that provably absent under `cargo test` as well as `nextest`.

use flui_log::FilterConfig;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Not `RUST_LOG`: the developer running the suite may well have that set, and
/// a test whose result depends on the ambient shell is not a test.
const TEST_ENV_VAR: &str = "FLUI_LOG_TEST_DIRECTIVES";

fn max_level(config: &FilterConfig) -> Option<LevelFilter> {
    let filter = config
        .env_filter()
        .expect("the directives under test are valid");
    <EnvFilter as Layer<Registry>>::max_level_hint(&filter)
}

#[test]
fn a_padded_environment_value_is_read_and_trimmed() {
    let config = FilterConfig::new("error").with_env_var(TEST_ENV_VAR);

    // Baseline: with the variable unset, the configured directives apply. This
    // also pins that the variable is not already set in the ambient
    // environment, which would invalidate everything below.
    assert_eq!(
        max_level(&config),
        Some(LevelFilter::ERROR),
        "{TEST_ENV_VAR} must be unset on entry to this binary"
    );

    // SAFETY: `set_var` requires that no other thread is concurrently reading
    // or writing the environment. This is the only test in this binary, the
    // variable name is unique to it, and no thread has been spawned — the test
    // harness's own threads do not touch the environment.
    #[allow(
        unsafe_code,
        reason = "std::env::set_var is unsafe in edition 2024; the single-threaded precondition holds here"
    )]
    unsafe {
        std::env::set_var(TEST_ENV_VAR, " info,flui_view=trace \n");
    }

    assert_eq!(
        max_level(&config),
        Some(LevelFilter::TRACE),
        "a padded value in the real environment must resolve as its trimmed form"
    );

    // Blank is "unset", not "invalid": it must fall through rather than error.
    #[allow(
        unsafe_code,
        reason = "std::env::set_var is unsafe in edition 2024; the single-threaded precondition holds here"
    )]
    unsafe {
        std::env::set_var(TEST_ENV_VAR, "   \t  ");
    }

    assert_eq!(
        max_level(&config),
        Some(LevelFilter::ERROR),
        "a blank value in the real environment must not override, and must not error"
    );
}
