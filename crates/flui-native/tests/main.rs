//! Single-binary consolidation of flui-native's root integration tests.
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
#[path = "android_keys.rs"]
mod android_keys;
#[path = "android_motion.rs"]
mod android_motion;
#[path = "backend_conformance.rs"]
mod backend_conformance;
#[path = "contract.rs"]
mod contract;
#[path = "direct_manipulation.rs"]
mod direct_manipulation;
#[path = "displays.rs"]
mod displays;
#[cfg(not(target_arch = "wasm32"))]
#[path = "executor.rs"]
mod executor;
#[path = "file_store.rs"]
mod file_store;
#[path = "handler_reentry.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod handler_reentry;
#[path = "headless_drag.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod headless_drag;
#[path = "headless_redraw.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod headless_redraw;
#[path = "input_capabilities.rs"]
mod input_capabilities;
#[path = "input_regions.rs"]
mod input_regions;
#[path = "native_recording/mod.rs"]
mod native_recording;
#[path = "pointer_assembly.rs"]
mod pointer_assembly;
#[path = "preferences.rs"]
mod preferences;
#[path = "web_pinch.rs"]
mod web_pinch;
#[path = "window_callback_unwind.rs"]
#[cfg(not(target_arch = "wasm32"))]
mod window_callback_unwind;
#[path = "window_state.rs"]
mod window_state;
#[path = "windows_host_events.rs"]
#[cfg(target_os = "windows")]
mod windows_host_events;
#[path = "windows_window_commands.rs"]
#[cfg(target_os = "windows")]
mod windows_window_commands;

/// Runs every case even after one fails, then panics listing the failing case names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}
