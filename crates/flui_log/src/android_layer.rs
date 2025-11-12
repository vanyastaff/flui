//! Android-specific logging layer using android_log-sys
//!
//! This module provides a tracing layer that outputs to Android's logcat system.
//! It's based on Bevy's implementation and converts tracing events to appropriate
//! Android log levels.

use core::fmt::{Debug, Write};
use tracing::{
    field::Field,
    span::{Attributes, Record},
    Event, Id, Level, Subscriber,
};
use tracing_subscriber::{field::Visit, layer::Context, registry::LookupSpan, Layer};

/// Tracing layer that outputs to Android logcat
///
/// This layer is automatically used when running on Android platform.
/// It converts tracing events to android_log calls with appropriate priority levels.
#[derive(Default)]
pub struct AndroidLayer;

/// Helper struct for recording tracing fields into a string
struct StringRecorder(String, bool);

impl StringRecorder {
    fn new() -> Self {
        StringRecorder(String::new(), false)
    }
}

impl Visit for StringRecorder {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            // Message field goes first
            if !self.0.is_empty() {
                self.0 = format!("{:?}\n{}", value, self.0)
            } else {
                self.0 = format!("{:?}", value)
            }
        } else {
            // Other fields are appended as key=value pairs
            if self.1 {
                write!(self.0, " ").unwrap();
            } else {
                self.1 = true;
            }
            write!(self.0, "{} = {:?}", field.name(), value).unwrap();
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for AndroidLayer {
    fn on_new_span(&self, _attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        // No-op: We don't track spans on Android
    }

    fn on_record(&self, _span: &Id, _values: &Record<'_>, _ctx: Context<'_, S>) {
        // No-op: We don't track span records on Android
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Record all fields into a string
        let mut recorder = StringRecorder::new();
        event.record(&mut recorder);

        let metadata = event.metadata();
        let level = metadata.level();

        // Convert tracing level to android_log priority
        let priority = match *level {
            Level::TRACE => android_log_sys::LogPriority::VERBOSE,
            Level::DEBUG => android_log_sys::LogPriority::DEBUG,
            Level::INFO => android_log_sys::LogPriority::INFO,
            Level::WARN => android_log_sys::LogPriority::WARN,
            Level::ERROR => android_log_sys::LogPriority::ERROR,
        };

        // Use the event target as the log tag
        let tag = std::ffi::CString::new(metadata.target()).unwrap();
        let message = std::ffi::CString::new(recorder.0).unwrap();

        // SAFETY: android_log_sys expects valid C strings, which we've ensured above
        unsafe {
            android_log_sys::__android_log_write(priority as i32, tag.as_ptr(), message.as_ptr());
        }
    }
}
