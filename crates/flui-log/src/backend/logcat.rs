//! Android logcat backend.
//!
//! The parts that decide *what* is written — the level mapping, the tag
//! fallback chain, and the NUL handling that the C boundary forces — are
//! compiled on every target and unit-tested on the ordinary CI host. Only the
//! FFI call itself is `cfg(target_os = "android")`, so a change to the mapping
//! is caught by a test rather than by an Android device.
//!
//! # Viewing
//!
//! ```bash
//! adb logcat                 # everything
//! adb logcat flui_view:* *:S # one FLUI module
//! adb logcat *:W             # warnings and above
//! ```

// The tag and message helpers below are *called* only by the Android sink, but
// they are compiled on every target on purpose: keeping the decision logic off
// the `cfg(target_os = "android")` island is exactly what lets its tests run on
// the ordinary CI host. On Android every item here is live, so a genuinely dead
// item is still caught where it matters.
#![cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "compiled on every target so the logic stays testable"
    )
)]

use std::ffi::CString;

use tracing::Level;

/// Message substituted when an event's text cannot cross the C boundary at all.
const UNRENDERABLE_MESSAGE: &str = "(unrenderable message)";

/// Tag used when neither the event target nor the display name can be a C
/// string. Chosen to be NUL-free by construction.
const FALLBACK_TAG: &str = "flui";

/// Android log priorities, as `android_log-sys` numbers them.
///
/// Mirrored here rather than re-exported so the mapping is testable on hosts
/// that have no NDK. The `android_priorities_match_the_ndk` test below pins the
/// two together on Android itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum LogcatPriority {
    /// `TRACE` — logcat `V`.
    Verbose = 2,
    /// `DEBUG` — logcat `D`.
    Debug = 3,
    /// `INFO` — logcat `I`.
    Info = 4,
    /// `WARN` — logcat `W`.
    Warn = 5,
    /// `ERROR` — logcat `E`.
    Error = 6,
}

impl LogcatPriority {
    /// The logcat priority a `tracing` level maps to.
    #[must_use]
    pub fn for_level(level: Level) -> Self {
        match level {
            Level::TRACE => Self::Verbose,
            Level::DEBUG => Self::Debug,
            Level::INFO => Self::Info,
            Level::WARN => Self::Warn,
            Level::ERROR => Self::Error,
        }
    }
}

/// Choose the logcat tag for an event.
///
/// The event target (usually the module path, and the same string `RUST_LOG`
/// filters on) is preferred so a logcat filter and a `RUST_LOG` directive name
/// the same thing. An empty target, or one holding a NUL, falls back to the
/// application display name and then to a literal that cannot fail.
pub(crate) fn logcat_tag(target: &str, display_name: &str) -> CString {
    if !target.is_empty()
        && let Ok(tag) = CString::new(target)
    {
        return tag;
    }
    if let Ok(tag) = CString::new(display_name) {
        return tag;
    }
    CString::new(FALLBACK_TAG).expect("BUG: the fallback tag literal contains no NUL")
}

/// Convert a rendered event into a C string.
///
/// An interior NUL truncates at the first NUL rather than dropping the event:
/// losing the tail of a message is a smaller loss than losing the fact that it
/// happened.
pub(crate) fn logcat_message(rendered: &str) -> CString {
    match CString::new(rendered) {
        Ok(message) => message,
        Err(error) => {
            let truncated = &rendered[..error.nul_position()];
            CString::new(truncated).unwrap_or_else(|_| {
                CString::new(UNRENDERABLE_MESSAGE)
                    .expect("BUG: the unrenderable-message literal contains no NUL")
            })
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) use android::LogcatLayer;

#[cfg(target_os = "android")]
mod android {
    use tracing::{Event, Subscriber};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::Context;
    use tracing_subscriber::registry::LookupSpan;

    use super::{LogcatPriority, logcat_message, logcat_tag};
    use crate::backend::record::FieldRecorder;

    /// Writes events to Android's logcat.
    ///
    /// Spans are deliberately not tracked: logcat has no span concept, and the
    /// per-span bookkeeping would be pure overhead on the platform least able
    /// to absorb it. Span *fields* still reach logcat when an event carries
    /// them.
    #[derive(Debug, Clone)]
    pub(crate) struct LogcatLayer {
        display_name: String,
    }

    impl LogcatLayer {
        pub(crate) fn new(display_name: impl Into<String>) -> Self {
            Self {
                display_name: display_name.into(),
            }
        }
    }

    impl<S> Layer<S> for LogcatLayer
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut recorder = FieldRecorder::new();
            event.record(&mut recorder);
            let rendered = recorder.into_string();
            if rendered.is_empty() {
                return;
            }

            let metadata = event.metadata();
            let priority = LogcatPriority::for_level(*metadata.level());
            let tag = logcat_tag(metadata.target(), &self.display_name);
            let message = logcat_message(&rendered);

            // The workspace warns on `unsafe_code`; logcat has no safe binding.
            #[allow(unsafe_code, reason = "logcat is only reachable through NDK FFI")]
            // SAFETY: `__android_log_write` takes a priority integer and two
            // NUL-terminated C strings, reads them, and returns. `tag` and
            // `message` are owned `CString`s that outlive the call, so both
            // pointers are non-null, aligned, and NUL-terminated for its whole
            // duration. The function is thread-safe and does not retain or
            // mutate either pointer.
            unsafe {
                android_log_sys::__android_log_write(
                    priority as i32,
                    tag.as_ptr(),
                    message.as_ptr(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_map_to_logcat_priorities() {
        assert_eq!(
            LogcatPriority::for_level(Level::TRACE),
            LogcatPriority::Verbose
        );
        assert_eq!(
            LogcatPriority::for_level(Level::DEBUG),
            LogcatPriority::Debug
        );
        assert_eq!(LogcatPriority::for_level(Level::INFO), LogcatPriority::Info);
        assert_eq!(LogcatPriority::for_level(Level::WARN), LogcatPriority::Warn);
        assert_eq!(
            LogcatPriority::for_level(Level::ERROR),
            LogcatPriority::Error
        );
    }

    #[test]
    fn priority_numbers_are_the_ones_logcat_expects() {
        // Pinning the discriminants matters because the FFI call passes them
        // as a bare `i32`: a renumbering would silently misfile every event.
        assert_eq!(LogcatPriority::Verbose as i32, 2);
        assert_eq!(LogcatPriority::Debug as i32, 3);
        assert_eq!(LogcatPriority::Info as i32, 4);
        assert_eq!(LogcatPriority::Warn as i32, 5);
        assert_eq!(LogcatPriority::Error as i32, 6);
    }

    #[cfg(target_os = "android")]
    #[test]
    fn android_priorities_match_the_ndk() {
        assert_eq!(
            LogcatPriority::Verbose as i32,
            android_log_sys::LogPriority::VERBOSE as i32
        );
        assert_eq!(
            LogcatPriority::Debug as i32,
            android_log_sys::LogPriority::DEBUG as i32
        );
        assert_eq!(
            LogcatPriority::Info as i32,
            android_log_sys::LogPriority::INFO as i32
        );
        assert_eq!(
            LogcatPriority::Warn as i32,
            android_log_sys::LogPriority::WARN as i32
        );
        assert_eq!(
            LogcatPriority::Error as i32,
            android_log_sys::LogPriority::ERROR as i32
        );
    }

    #[test]
    fn the_event_target_becomes_the_tag() {
        assert_eq!(
            logcat_tag("flui_view::element", "My Game").to_str(),
            Ok("flui_view::element")
        );
    }

    #[test]
    fn an_empty_target_falls_back_to_the_display_name() {
        assert_eq!(logcat_tag("", "My Game").to_str(), Ok("My Game"));
    }

    #[test]
    fn a_nul_in_both_falls_back_to_the_literal() {
        assert_eq!(logcat_tag("a\0b", "c\0d").to_str(), Ok(FALLBACK_TAG));
    }

    #[test]
    fn a_message_with_an_interior_nul_is_truncated_not_dropped() {
        assert_eq!(
            logcat_message("visible\0hidden").to_str(),
            Ok("visible"),
            "a NUL must cost the tail of the message, not the whole event"
        );
    }

    #[test]
    fn a_leading_nul_still_produces_a_message() {
        assert_eq!(logcat_message("\0rest").to_str(), Ok(""));
    }
}
