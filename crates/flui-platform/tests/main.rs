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
//!
//! macOS invariant: any `platform_it` test that constructs the platform
//! through `current_platform()` (or a helper reaching it) must carry
//! `#[cfg_attr(target_os = "macos", ignore = "requires an AppKit-run-loop-
//! pumping test process (ADR-0039): …")]` — the macOS platform surface
//! asserts the AppKit main thread, which a bare `cargo test` cannot host
//! unbundled (ADR-0039; unbundled NSWindow construction aborts the process).

#[path = "android_exit_path.rs"]
mod android_exit_path;
#[path = "contract.rs"]
mod contract;
#[path = "window_callback_unwind.rs"]
mod window_callback_unwind;
