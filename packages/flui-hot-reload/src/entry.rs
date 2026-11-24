//! The guard a plugin image's entries run the developer's code under.
//!
//! Every entry a plugin macro generates is `extern "C"`, and a panic cannot
//! unwind out of one: Rust aborts the process, the host's included. In a
//! development loop the developer's code is the code most likely to panic, so
//! [`contain`] keeps the panic inside the image. It writes the panic's text
//! where the image's platform shows it, since the host's `tracing` subscriber
//! is not reachable from a `dlopen`ed image, and destroys the payload there,
//! because the payload's vtable lives in this image. The entry then gives the
//! host its refusal value: a null scene, which the host treats as a skipped
//! frame, or a failed init, which the host treats as a refused load.

use flui_sdk::foundation::panic::{FirstPanic, discard_payload, payload_text};

/// Runs `code`, the developer's code behind the plugin entry named `entry`.
/// `None` when it panicked: the panic is reported and its payload destroyed
/// in this image.
pub fn contain<R>(entry: &'static str, code: impl FnOnce() -> R) -> Option<R> {
    let mut first = FirstPanic::new();
    let value = first.contain(code);
    if let Some(payload) = first.into_payload() {
        let text = payload_text(payload.as_ref()).unwrap_or("a payload that is not text");
        report(&format!("{entry}: the plugin's code panicked: {text}"));
        discard_payload(payload);
    }
    value
}

/// Writes `message` where a plugin image's platform shows it: logcat on
/// Android, stderr elsewhere.
pub(crate) fn report(message: &str) {
    #[cfg(target_os = "android")]
    {
        let tag = c"flui-hot-reload";
        // A NUL inside the text would end the C string early.
        let message = std::ffi::CString::new(message.replace('\0', "\u{FFFD}"))
            .expect("BUG: every NUL was replaced above");
        // SAFETY: `tag` and `message` are NUL-terminated C strings that outlive
        // the call; liblog copies both before returning and retains neither.
        #[expect(unsafe_code, reason = "liblog's C entry point")]
        unsafe {
            android_log_sys::__android_log_write(
                android_log_sys::LogPriority::WARN as i32,
                tag.as_ptr(),
                message.as_ptr(),
            );
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        eprintln!("[flui-hot-reload] {message}");
    }
}
