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
//! Recoverable owner callbacks use a different boundary: the call point
//! (`super::call_point`) settles a handler's slot independently of its call, and
//! callback invocation and capture cleanup are contained separately. This does
//! not relax the abort policy for a torn visible-window WNDPROC transaction
//! described above.
//!
//! The payload-to-message rule is the pure, decidable part; it lives here,
//! compiled under `test` on every host, so the Linux-executed suite pins it
//! (the Win32 shell that applies it is lint-only in CI).

/// Fallback text for a panic payload that is neither a `&'static str` nor a
/// `String` (someone used `std::panic::panic_any` with an arbitrary type).
#[cfg(any(
    target_os = "windows",
    all(target_os = "linux", any(feature = "wayland", feature = "x11")),
    test
))]
pub(crate) const OPAQUE_PANIC_PAYLOAD: &str = "non-string panic payload";

/// Extracts the human-readable message from a caught panic payload.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{x}")`
/// and every formatted variant produce a `String`; anything else (via
/// `panic_any`) is opaque and reported as [`OPAQUE_PANIC_PAYLOAD`].
#[cfg(any(
    target_os = "windows",
    all(target_os = "linux", any(feature = "wayland", feature = "x11")),
    test
))]
#[must_use]
pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    flui_foundation::panic::payload_text(payload).unwrap_or(OPAQUE_PANIC_PAYLOAD)
}

/// Contain owner callback and capture cleanup panics without dropping hostile payloads.
pub(crate) fn contain_owner_callback(body: impl FnOnce()) {
    let _ = owner_callback_panicked(body);
}

/// [`contain_owner_callback`], answering whether `body` panicked, so a caller
/// retiring several values retains the rest after the first failure
/// (ADR-0127 §2).
pub(crate) fn owner_callback_panicked(body: impl FnOnce()) -> bool {
    let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) else {
        return false;
    };
    std::mem::forget(payload);
    if let Err(payload) = std::panic::catch_unwind(|| {
        tracing::error!("contained owner callback or cleanup panic");
    }) {
        std::mem::forget(payload);
    }
    true
}

/// Run the body of an entry the OS calls and that must not unwind (a COM
/// method, a callback behind an `extern "system"` pointer): `None` when it
/// panicked. The panic is logged with its text, under `entry`, the log
/// contained like any owner callback, and its payload is then retained
/// rather than dropped, since a payload's destructor is user code that would
/// unwind into the OS (ADR-0127 §2).
#[cfg(any(target_os = "windows", test))]
pub(crate) fn contain_os_entry<R>(entry: &'static str, body: impl FnOnce() -> R) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(payload) => {
            contain_owner_callback(|| {
                tracing::error!(
                    entry,
                    panic = panic_payload_message(&*payload),
                    "an OS entry panicked; its caller is answered with a failure"
                );
            });
            flui_foundation::panic::retain_opaque_payload(payload);
            None
        }
    }
}

/// Keep an owner-affine resource whose final owner was dropped on another thread.
/// Retention precedes diagnostics: a subscriber panic must not retire the resource
/// on the wrong thread, including while the caller is already unwinding.
#[cfg(any(
    all(feature = "a11y", any(target_os = "windows", target_os = "macos")),
    test
))]
pub(crate) fn retain_off_owner_resource<T>(resource: T, report: impl FnOnce()) {
    std::mem::forget(resource);
    contain_owner_callback(report);
}

// A panic on wasm32 aborts instead of unwinding, so there is nothing to catch.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::contain_os_entry;

    /// A panic payload whose destructor is user code: it records that it ran.
    struct Payload(Arc<AtomicBool>);

    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// An entry's answer passes through; a panic is `None` and an opaque
    /// payload is retained, never destroyed inside the OS's call. Red-check:
    /// drop the payload instead of retaining it.
    #[test]
    fn an_os_entry_contains_its_panic_and_retains_its_payload() {
        assert_eq!(contain_os_entry("a healthy entry", || 7), Some(7));
        assert_eq!(
            contain_os_entry("a formatted panic", || -> u8 { panic!("entry {}", 7) }),
            None
        );

        let destroyed = Arc::new(AtomicBool::new(false));
        let payload = Payload(Arc::clone(&destroyed));
        let answered = contain_os_entry("an opaque panic", move || -> u8 {
            std::panic::panic_any(payload)
        });
        assert_eq!(answered, None);
        assert!(
            !destroyed.load(Ordering::SeqCst),
            "the payload's destructor ran inside the entry"
        );
    }
}
