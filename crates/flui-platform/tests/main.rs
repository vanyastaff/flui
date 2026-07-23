//! Single-binary consolidation of flui-platform's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `platform_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! and manifest-relative paths keep working unchanged.
//!
//! Convention: tests that WRITE process-global state (e.g. the
//! `FLUI_HEADLESS` env var) live in their own [[test]] target instead —
//! process isolation beats opt-in locking. See headless.

#[path = "contract.rs"]
mod contract;
#[path = "display_enumeration.rs"]
mod display_enumeration;
#[path = "event_contracts.rs"]
mod event_contracts;
#[path = "event_handling.rs"]
mod event_handling;
#[path = "executor_tests.rs"]
mod executor_tests;
#[path = "integration_template.rs"]
mod integration_template;
#[path = "performance.rs"]
mod performance;
#[path = "window_lifecycle.rs"]
mod window_lifecycle;
#[path = "window_modes.rs"]
mod window_modes;
#[path = "windows_event_conversion.rs"]
mod windows_event_conversion;
