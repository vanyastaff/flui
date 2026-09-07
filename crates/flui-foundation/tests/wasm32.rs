//! Assertions that only mean anything when they EXECUTE on wasm32.
//!
//! Until this file landed, the workspace only ever compiled for wasm32 —
//! `just wasm-check` is `cargo check` plus `cargo clippy`, and there was no
//! `wasm_bindgen_test` anywhere in the tree, so every "works on the web" claim
//! rested on the linker succeeding (issue #985). The whole point of the file is
//! that a green here is produced by running code in a wasm VM.
//!
//! Run it with `just wasm-test`, which resolves the `wasm-bindgen-cli` version
//! out of `Cargo.lock` — it must match the locked `wasm-bindgen` exactly or the
//! runner refuses to start.
//!
//! What belongs here: behaviour that DIFFERS on wasm32, or a native-target
//! substitution whose whole purpose is to keep wasm32 working. A test that
//! would pass identically on native belongs in the crate's ordinary suite —
//! running it here costs a wasm build and proves nothing extra.

#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::{ManualClock, MonotonicClock, SystemClock, WasmNotSendSync};
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

/// `WasmNotSendSync` exists to be `Send + Sync` on native and empty on wasm32.
/// The oracle has to be a type that VIOLATES the native bound, or the test is
/// silent about whether the relaxation happened at all: `Rc<Cell<u32>>` is
/// neither `Send` nor `Sync`, so this function does not compile for a native
/// target and does compile — and run — here.
#[wasm_bindgen_test]
fn the_wasm_blanket_impl_accepts_a_type_that_is_neither_send_nor_sync() {
    fn takes<T: WasmNotSendSync>(value: T) -> T {
        value
    }

    let shared: Rc<Cell<u32>> = Rc::new(Cell::new(7));
    let returned = takes(Rc::clone(&shared));
    returned.set(returned.get() + 1);
    assert_eq!(shared.get(), 8, "the value did not survive the bound");
}
