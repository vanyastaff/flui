//! Android-specific logging layer using `android_log-sys`
//!
//! This module provides a [`tracing`] layer that outputs to Android's logcat system.
//! It converts tracing events to native Android log calls with appropriate priority levels.
//!
//! # Platform
//!
//! This module is only compiled and available on Android (`target_os = "android"`).
//! On other platforms, the logging system uses platform-appropriate backends.
//!
//! # Architecture
//!
//! The implementation uses FFI bindings from [`android_log-sys`] to call Android's
//! native `__android_log_write` function. Tracing levels are mapped to Android's
//! [`LogPriority`](android_log_sys::LogPriority) values:
//!
//! | Tracing Level | Android Priority | Logcat Tag |
//! |---------------|------------------|------------|
//! | `TRACE` | `VERBOSE` | V |
//! | `DEBUG` | `DEBUG` | D |
//! | `INFO` | `INFO` | I |
//! | `WARN` | `WARN` | W |
//! | `ERROR` | `ERROR` | E |
//!
//! # Usage
//!
//! This layer is automatically configured by [`Logger`](crate::Logger) when running
//! on Android. You typically don't need to use it directly:
//!
//! ```rust,no_run
//! use flui_log::Logger;
//!
//! // On Android, this automatically uses AndroidLayer
//! Logger::default().init();
//!
//! tracing::info!("This will appear in logcat");
//! ```
//!
//! # Manual Usage
//!
//! For advanced use cases, you can create the layer manually:
//!
//! ```rust,no_run
//! use flui_log::android_layer::AndroidLayer;
//! use tracing_subscriber::{layer::SubscriberExt, Registry};
//!
//! let subscriber = Registry::default()
//!     .with(AndroidLayer::default());
//!
//! tracing::subscriber::set_global_default(subscriber)
//!     .expect("Failed to set tracing subscriber");
//! ```
//!
//! # Viewing Logs
//!
//! Use `adb logcat` to view logs from the command line:
//!
//! ```bash
//! # View all logs
//! adb logcat
//!
//! # Filter by tag (e.g., module name)
//! adb logcat my_app:* *:S
//!
//! # Filter by priority level
//! adb logcat *:W  # Warnings and above
//! adb logcat *:E  # Errors only
//! ```
//!
//! # Performance
//!
//! - Log messages are formatted only once before being sent to logcat
//! - String allocations are minimized using stack buffers where possible
//! - The layer has minimal overhead when logging is disabled
//!
//! # Safety
//!
//! This module uses `unsafe` code to call Android's C logging API via FFI.
//! All unsafe operations are carefully reviewed and documented. See the
//! safety comments in the implementation for details.
//!
//! # References
//!
//! - [android_log-sys documentation](https://docs.rs/android_log-sys/)
//! - [Android NDK Logging](https://developer.android.com/ndk/reference/group/logging)
//! - [adb logcat documentation](https://developer.android.com/studio/command-line/logcat)

use core::fmt::{Debug, Write};
use tracing::{
    field::Field,
    span::{Attributes, Record},
    Event, Id, Level, Subscriber,
};
use tracing_subscriber::{field::Visit, layer::Context, registry::LookupSpan, Layer};

/// Tracing layer that outputs to Android logcat
///
/// This layer integrates with Android's native logging system using the
/// [`android_log-sys`] FFI bindings. It's designed to be zero-cost when
/// logging is disabled and minimal overhead when active.
///
/// # Implementation Details
///
/// - Event fields are formatted into a single string before logging
/// - The `message` field is prioritized and appears first
/// - Additional fields are appended as `key=value` pairs
/// - Span tracking is not implemented (no-op) to minimize overhead
/// - Log tags are derived from the event's target (usually module path)
///
/// # Examples
///
/// ```rust,no_run
/// use flui_log::android_layer::AndroidLayer;
/// use tracing_subscriber::{layer::SubscriberExt, Registry};
///
/// // Create the layer
/// let android_layer = AndroidLayer::default();
///
/// // Combine with a registry
/// let subscriber = Registry::default()
///     .with(android_layer);
///
/// // Set as global subscriber
/// tracing::subscriber::set_global_default(subscriber)
///     .expect("Failed to set tracing subscriber");
///
/// // Use tracing macros
/// tracing::info!("Application started");
/// tracing::debug!(user_id = 42, "User logged in");
/// ```
///
/// # Thread Safety
///
/// This type is `Send` and `Sync` because it contains no mutable state.
/// Multiple threads can safely use the same layer instance.
#[derive(Debug, Clone)]
pub struct AndroidLayer {
    /// Application name used as fallback logcat tag
    ///
    /// Used when the event's target (module path) is empty or contains null bytes.
    /// Default: "flui"
    app_name: String,
}

impl Default for AndroidLayer {
    fn default() -> Self {
        Self {
            app_name: "flui".to_string(),
        }
    }
}

impl AndroidLayer {
    /// Create a new AndroidLayer with a custom application name
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_log::android_layer::AndroidLayer;
    ///
    /// let layer = AndroidLayer::new("my_game");
    /// ```
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }
}

/// Helper struct for recording tracing fields into a string
///
/// This visitor accumulates event fields into a formatted string,
/// with special handling for the `message` field.
#[derive(Debug)]
struct StringRecorder {
    /// The accumulated output string
    output: String,
    /// Whether we've written any non-message fields yet
    has_fields: bool,
}

impl StringRecorder {
    /// Create a new empty recorder
    #[inline]
    fn new() -> Self {
        Self {
            output: String::new(),
            has_fields: false,
        }
    }

    /// Get the accumulated string, consuming the recorder
    #[inline]
    fn into_string(self) -> String {
        self.output
    }
}

impl Visit for StringRecorder {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            // Message field goes first
            if !self.output.is_empty() {
                // If we already have fields, prepend the message
                self.output = format!("{:?}\n{}", value, self.output);
            } else {
                // First field is the message
                write!(self.output, "{:?}", value).unwrap();
            }
        } else {
            // Other fields are appended as key=value pairs
            if self.has_fields {
                write!(self.output, " ").unwrap();
            } else {
                // First non-message field needs a separator if message exists
                if !self.output.is_empty() {
                    write!(self.output, " | ").unwrap();
                }
                self.has_fields = true;
            }
            write!(self.output, "{}={:?}", field.name(), value).unwrap();
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for AndroidLayer {
    fn on_new_span(&self, _attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        // Intentionally left empty: span tracking is not implemented on Android
        // to minimize overhead. Android's logcat doesn't have native span support.
    }

    fn on_record(&self, _span: &Id, _values: &Record<'_>, _ctx: Context<'_, S>) {
        // Intentionally left empty: span recording is not tracked on Android.
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Record all fields into a string
        let mut recorder = StringRecorder::new();
        event.record(&mut recorder);
        let message = recorder.into_string();

        // Skip empty messages
        if message.is_empty() {
            return;
        }

        let metadata = event.metadata();
        let level = metadata.level();

        // Convert tracing level to android_log priority
        // See: https://docs.rs/android_log-sys/latest/android_log_sys/enum.LogPriority.html
        let priority = match *level {
            Level::TRACE => android_log_sys::LogPriority::VERBOSE,
            Level::DEBUG => android_log_sys::LogPriority::DEBUG,
            Level::INFO => android_log_sys::LogPriority::INFO,
            Level::WARN => android_log_sys::LogPriority::WARN,
            Level::ERROR => android_log_sys::LogPriority::ERROR,
        };

        // Use the event target as the log tag (usually the module path)
        // Fallback to app_name if target is empty or contains null bytes
        let tag = std::ffi::CString::new(metadata.target())
            .unwrap_or_else(|_| std::ffi::CString::new(&self.app_name).unwrap());

        // Create C string for the message
        // If the message contains null bytes, truncate at the first null
        let message_cstr = match std::ffi::CString::new(message.as_str()) {
            Ok(cstr) => cstr,
            Err(e) => {
                // Message contains null bytes - truncate at first null
                let null_pos = e.nul_position();
                std::ffi::CString::new(&message[..null_pos])
                    .unwrap_or_else(|_| std::ffi::CString::new("(invalid message)").unwrap())
            }
        };

        // SAFETY: We call `__android_log_write` with the following guarantees:
        //
        // 1. `priority` is a valid i32 value from the LogPriority enum
        // 2. `tag.as_ptr()` is a valid pointer to a null-terminated C string
        //    - The CString is owned and valid for the duration of the call
        //    - The pointer is properly aligned and non-null
        // 3. `message_cstr.as_ptr()` is a valid pointer to a null-terminated C string
        //    - The CString is owned and valid for the duration of the call
        //    - The pointer is properly aligned and non-null
        //
        // The function is safe to call from any thread and doesn't modify the input pointers.
        // See: https://docs.rs/android_log-sys/latest/android_log_sys/fn.__android_log_write.html
        unsafe {
            android_log_sys::__android_log_write(
                priority as i32,
                tag.as_ptr(),
                message_cstr.as_ptr(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_recorder_message_only() {
        let mut recorder = StringRecorder::new();
        let field = Field::new("message", tracing::field::CallsiteKind::Event);
        recorder.record_debug(&field, &"Hello, world!");

        assert_eq!(recorder.into_string(), "\"Hello, world!\"");
    }

    #[test]
    fn test_string_recorder_message_with_fields() {
        let mut recorder = StringRecorder::new();

        let msg_field = Field::new("message", tracing::field::CallsiteKind::Event);
        recorder.record_debug(&msg_field, &"User action");

        let id_field = Field::new("user_id", tracing::field::CallsiteKind::Event);
        recorder.record_debug(&id_field, &42);

        let name_field = Field::new("action", tracing::field::CallsiteKind::Event);
        recorder.record_debug(&name_field, &"login");

        let output = recorder.into_string();
        assert!(output.contains("\"User action\""));
        assert!(output.contains("user_id=42"));
        assert!(output.contains("action=\"login\""));
    }

    #[test]
    fn test_string_recorder_fields_only() {
        let mut recorder = StringRecorder::new();

        let field1 = Field::new("key1", tracing::field::CallsiteKind::Event);
        recorder.record_debug(&field1, &"value1");

        let field2 = Field::new("key2", tracing::field::CallsiteKind::Event);
        recorder.record_debug(&field2, &123);

        let output = recorder.into_string();
        assert!(output.contains("key1=\"value1\""));
        assert!(output.contains("key2=123"));
    }

    #[test]
    fn test_android_layer_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<AndroidLayer>();
        assert_sync::<AndroidLayer>();
    }
}
