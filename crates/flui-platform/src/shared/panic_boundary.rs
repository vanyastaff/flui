//! Panic-boundary support for `extern`-ABI OS callbacks.
//!
//! Native backends hand callback pointers straight to the OS — Win32 calls
//! `WNDPROC` (`extern "system"`) for every window message. Rust forbids a
//! panic from unwinding across such a non-unwind ABI boundary: when one
//! reaches it, the process is aborted (Rust Reference, *Panic* chapter,
//! "unwinding across FFI boundaries"; before Rust 1.81 this was undefined
//! behavior rather than a guaranteed abort). That language-level abort is
//! silent about *where* it happened as far as the framework's own
//! diagnostics are concerned, so a backend wraps the callback body in
//! `std::panic::catch_unwind`, logs the caught payload through `tracing`,
//! and then terminates deliberately with `std::process::abort()` — the same
//! outcome the language would impose, made observable first. Swallowing the
//! panic and continuing is *not* an option for these boundaries: a panic
//! mid-`WM_DESTROY` or mid-dispatch leaves framework state torn, and
//! resuming the message loop over torn state converts one crash into
//! arbitrary later misbehavior (see `docs/PANIC-POLICY.md` — a panic is a
//! bug report, never control flow).
//!
//! The payload-to-message rule is the pure, decidable part; it lives here,
//! cfg-free, so the Linux-executed suite pins it (the Win32 shell that
//! applies it is lint-only in CI).

use std::any::Any;

/// Fallback text for a panic payload that is neither a `&'static str` nor a
/// `String` (someone used `std::panic::panic_any` with an arbitrary type).
pub const OPAQUE_PANIC_PAYLOAD: &str = "non-string panic payload";

/// Extracts the human-readable message from a caught panic payload.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{x}")`
/// and every formatted variant produce a `String`; anything else (via
/// `panic_any`) is opaque and reported as [`OPAQUE_PANIC_PAYLOAD`].
#[must_use]
pub fn panic_payload_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        OPAQUE_PANIC_PAYLOAD
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    #[test]
    fn a_literal_panic_payload_is_a_static_str_and_comes_through_verbatim() {
        let payload =
            catch_unwind(|| panic!("literal wndproc failure")).expect_err("the closure must panic");
        assert_eq!(panic_payload_message(&*payload), "literal wndproc failure");
    }

    #[test]
    fn a_formatted_panic_payload_is_a_string_and_comes_through_verbatim() {
        let detail = 42;
        let payload = catch_unwind(AssertUnwindSafe(|| panic!("failure {detail}")))
            .expect_err("the closure must panic");
        assert_eq!(panic_payload_message(&*payload), "failure 42");
    }

    #[test]
    fn a_non_string_payload_is_reported_as_opaque_not_dropped_or_panicking() {
        let payload =
            catch_unwind(|| std::panic::panic_any(1234_i32)).expect_err("the closure must panic");
        assert_eq!(panic_payload_message(&*payload), OPAQUE_PANIC_PAYLOAD);
    }
}
