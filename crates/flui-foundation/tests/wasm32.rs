//! Assertions that only mean anything when they EXECUTE on wasm32.
//!
//! Until this file landed, the workspace only ever compiled for wasm32 —
//! `cargo xtask wasm-check` is `cargo check` plus `cargo clippy`, and there was no
//! `wasm_bindgen_test` anywhere in the tree, so every "works on the web" claim
//! rested on the linker succeeding (issue #985). The whole point of the file is
//! that a green here is produced by running code in a wasm VM.
//!
//! Run it with `cargo xtask wasm-test`, which resolves the `wasm-bindgen-cli` version
//! out of `Cargo.lock` — it must match the locked `wasm-bindgen` exactly or the
//! runner refuses to start.
//!
//! What belongs here: behaviour that DIFFERS on wasm32, or a native-target
//! substitution whose whole purpose is to keep wasm32 working. A test that
//! would pass identically on native belongs in the crate's ordinary suite —
//! running it here costs a wasm build and proves nothing extra.

#![cfg(target_arch = "wasm32")]

use flui_foundation::{ManualClock, MonotonicClock, SystemClock};
use std::time::Duration;
use wasm_bindgen_test::wasm_bindgen_test;

/// `clock.rs` reaches for `web_time::Instant` rather than `std::time::Instant`,
/// and its own comment gives the reason as a runtime fact: *"on wasm32 an
/// `Instant` backed by `performance.now()` — the std one panics there"*.
///
/// That is exactly the class of claim a compile cannot check. Swap the import
/// back to `std::time::Instant` and this test panics inside the wasm VM while
/// `cargo check --target wasm32-unknown-unknown` stays perfectly green.
#[wasm_bindgen_test]
fn the_system_clock_reads_a_real_monotonic_instant_on_wasm() {
    let clock = SystemClock;
    let a = clock.now();
    let b = clock.now();
    assert!(b >= a, "SystemClock went backwards on wasm32");
}

/// The virtual timeline is `base + elapsed`, where `base` is a real
/// `Instant::now()`. On wasm32 that instant is backed by `performance.now()`,
/// which is `f64` milliseconds rather than the nanosecond integer a native
/// `Instant` carries — so an exact-equality round-trip of the advance is a
/// genuine question here in a way it is not on native.
#[wasm_bindgen_test]
fn a_manual_clock_advance_round_trips_exactly_over_a_performance_now_base() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    clock.advance(Duration::from_millis(500));
    assert_eq!(clock.now().duration_since(t0), Duration::from_millis(500));

    // Clones share one timeline — the property the headless/production split
    // depends on, checked here against the wasm-backed base rather than only
    // against a native one.
    let other = clock.clone();
    clock.advance(Duration::from_millis(100));
    assert_eq!(other.elapsed(), Duration::from_millis(600));
}
