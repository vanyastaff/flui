//! Code macOS calls back into on a thread the lane does not own (the guard's event tap, a
//! ScreenCaptureKit completion handler), held to one rule: a panic in it never unwinds into the
//! system's frame. Each call runs under [`flui_foundation::panic::FirstPanic`] (ADR-0287), its
//! payload is discarded under containment when the collector drops, and the callback answers the
//! value its caller chose for a failure instead, so the side waiting on it hears of it.

use flui_foundation::panic::FirstPanic;

/// `run`'s value, or `failed` when it panics; the panic's payload is discarded under containment.
pub fn contained_or<T>(failed: T, run: impl FnOnce() -> T) -> T {
    let mut first = FirstPanic::new();
    let value = first.contain(run);
    drop(first);
    value.unwrap_or(failed)
}
