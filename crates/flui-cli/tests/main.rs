//! Single-binary consolidation of flui-cli's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `cli_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`). All tests here
//! drive the built `flui` binary in subprocesses via assert_cmd, so
//! consolidation does not change any process-level isolation.
//!
//! Convention: tests that WRITE process-global state live in their own
//! [[test]] target instead — process isolation beats opt-in locking.
//! (flui-cli currently has none; see flui-view's error_view_recovery
//! for the reference case.)

#[path = "cli_completions.rs"]
mod cli_completions;
#[path = "cli_create.rs"]
mod cli_create;
#[path = "cli_doctor.rs"]
mod cli_doctor;
#[path = "cli_errors.rs"]
mod cli_errors;
#[path = "cli_platform.rs"]
mod cli_platform;
