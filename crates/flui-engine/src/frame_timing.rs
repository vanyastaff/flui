//! Portable monotonic timestamps for GPU-path diagnostic spans.
//!
//! Surface acquire and present instrumentation must read a clock that works on
//! every target the renderer itself builds for. `std::time::Instant::now()`
//! panics on `wasm32-unknown-unknown` (`time not implemented on this platform`),
//! so a healthy windowed renderer that reaches acquisition cannot finish the
//! frame. This helper is the single production seam those timers share — keep
//! both call sites on it so a regression cannot reintroduce std timing on one
//! path while the other stays portable.
//!
//! Same approach as [`flui_foundation::SystemClock`]: `web_time::Instant` is a
//! monotonic clock on native targets and uses `performance.now()` on wasm32.

use web_time::Instant;

/// Monotonic instant for engine diagnostic timing (acquire / present spans).
///
/// Units of the derived spans stay microseconds via [`Instant::elapsed`].
#[inline]
pub(crate) fn now() -> Instant {
    Instant::now()
}

/// Assertions that only mean anything when they EXECUTE on wasm32.
///
/// Reaching this helper — not a foundation clock or a copied expression — is
/// what keeps the surface-timing regression from silently returning: swap
/// [`now`] back to `std::time::Instant` and this test panics in the wasm VM
/// while `cargo check --target wasm32-unknown-unknown` stays green (issue #1045).
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::now;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn engine_frame_timing_now_does_not_trap_on_wasm() {
        let started = now();
        let later = now();
        assert!(
            later >= started,
            "engine frame_timing::now went backwards on wasm32"
        );
        let _us = started.elapsed().as_micros();
    }
}
