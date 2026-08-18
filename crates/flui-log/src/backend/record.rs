//! Flattening a `tracing` event's fields into one line of text.
//!
//! Android's logcat and any other line-oriented native sink take a single
//! string, so the structured fields have to be rendered somewhere. This module
//! is compiled on **every** target, not just Android, so its behaviour is
//! covered by tests that run on the ordinary CI host — the historical version
//! lived inside a `cfg(target_os = "android")` module, which meant its tests
//! never compiled anywhere (they called a `tracing::field::Field::new` that no
//! released `tracing-core` exposes).
//!
//! This renderer holds no privacy opinion: field classification happens
//! upstream in [`super::redact`], which the shipped Android sink sits behind,
//! so a private value arrives here already replaced by the placeholder.

// Only the Android sink renders fields this way in a shipped build; the unit
// tests exercise it on every target. Same reasoning as `logcat.rs`.
#![cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "compiled on every target so the logic stays testable"
    )
)]

use core::fmt::{Debug, Write as _};

use tracing::field::{Field, Visit};

/// Typical rendered event size; avoids a reallocation for most events.
const DEFAULT_CAPACITY: usize = 128;

/// Separator between the message and the remaining fields.
const MESSAGE_SEPARATOR: &str = " | ";

/// Renders an event's fields as `message | key=value key=value`.
///
/// The `message` field is hoisted to the front regardless of the order the
/// fields arrive in, so a reader scanning logcat sees the sentence first and
/// the correlation fields after it.
#[derive(Debug)]
pub(crate) struct FieldRecorder {
    output: String,
    has_fields: bool,
}

impl FieldRecorder {
    pub(crate) fn new() -> Self {
        Self {
            output: String::with_capacity(DEFAULT_CAPACITY),
            has_fields: false,
        }
    }

    /// The rendered line, consuming the recorder.
    pub(crate) fn into_string(self) -> String {
        self.output
    }

    /// Borrow the rendered fields without consuming the recorder.
    pub(crate) fn as_str(&self) -> &str {
        &self.output
    }

    /// Append fields that another recorder already rendered.
    pub(crate) fn push_rendered_fields(&mut self, fields: &str) {
        if fields.is_empty() {
            return;
        }
        self.begin_field();
        self.output.push_str(fields);
    }

    /// Place the event's message ahead of everything recorded so far.
    pub(crate) fn push_message(&mut self, message: &str) {
        if self.output.is_empty() {
            self.output.push_str(message);
        } else {
            // Fields arrived first. Prepend rather than rebuild, so the buffer
            // allocated in `new` is still the only allocation.
            self.output.insert_str(0, MESSAGE_SEPARATOR);
            self.output.insert_str(0, message);
        }
    }

    /// Append a `name=value` pair with an already-rendered value.
    pub(crate) fn push_field(&mut self, name: &str, value: &str) {
        self.begin_field();
        // Writing into a String is infallible.
        let _ = write!(self.output, "{name}={value}");
    }

    fn begin_field(&mut self) {
        if self.has_fields {
            self.output.push(' ');
        } else if !self.output.is_empty() {
            // A message is already down and this is the first field, so the
            // two need separating. The historical implementation only ever
            // added the separator on the prepend path, which ran the message
            // straight into the first field whenever the message arrived
            // first — the usual order.
            self.output.push_str(MESSAGE_SEPARATOR);
        }
        self.has_fields = true;
    }
}

impl Visit for FieldRecorder {
    /// String values render unquoted — logcat is read by humans, and `Debug`
    /// quoting turns every message into `"…"`.
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.push_message(value);
        } else {
            self.push_field(field.name(), value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            self.push_message(&format!("{value:?}"));
        } else {
            self.begin_field();
            let _ = write!(self.output, "{}={value:?}", field.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::capture_rendered_events;

    #[test]
    fn message_only() {
        let mut recorder = FieldRecorder::new();
        recorder.push_message("Hello, world!");
        assert_eq!(recorder.into_string(), "Hello, world!");
    }

    #[test]
    fn message_then_fields() {
        let mut recorder = FieldRecorder::new();
        recorder.push_message("presentation committed");
        recorder.push_field("batch", "42");
        recorder.push_field("phase", "commit");

        assert_eq!(
            recorder.into_string(),
            "presentation committed | batch=42 phase=commit"
        );
    }

    #[test]
    fn fields_before_the_message_still_read_message_first() {
        let mut recorder = FieldRecorder::new();
        recorder.push_field("phase", "commit");
        recorder.push_field("batch", "7");
        recorder.push_message("late message");

        assert_eq!(
            recorder.into_string(),
            "late message | phase=commit batch=7"
        );
    }

    #[test]
    fn rendered_span_fields_can_be_prefixed_to_an_event() {
        let mut span = FieldRecorder::new();
        span.push_field("presentation_id", "42");

        let mut event = FieldRecorder::new();
        event.push_rendered_fields(span.as_str());
        event.push_message("frame committed");
        event.push_field("frame_id", "7");

        assert_eq!(
            event.into_string(),
            "frame committed | presentation_id=42 frame_id=7"
        );
    }

    #[test]
    fn buffer_is_preallocated() {
        assert!(FieldRecorder::new().output.capacity() >= DEFAULT_CAPACITY);
    }

    // --- through a real `tracing` event, so the `Visit` wiring is covered too

    #[test]
    fn a_real_event_renders_message_first_then_its_fields() {
        let rendered = capture_rendered_events(|| {
            tracing::info!(phase = "commit", batch = 7_u64, "frame committed");
        });

        assert_eq!(
            rendered,
            vec!["frame committed | phase=commit batch=7".to_owned()]
        );
    }

    #[test]
    fn non_string_values_keep_debug_formatting() {
        let rendered = capture_rendered_events(|| {
            tracing::info!(size = ?(2_u32, 3_u32), enabled = true);
        });

        assert_eq!(rendered, vec!["size=(2, 3) enabled=true".to_owned()]);
    }

    #[test]
    fn arbitrary_structured_fields_survive_rendering() {
        let rendered = capture_rendered_events(|| {
            tracing::info!(
                batch = 1_u64,
                generation = 2_u64,
                attempt = 3_u64,
                "frame committed"
            );
        });

        let line = rendered.first().expect("exactly one event was emitted");
        for (name, value) in [("batch", 1), ("generation", 2), ("attempt", 3)] {
            assert!(
                line.contains(&format!("{name}={value}")),
                "structured field `{name}` was dropped from {line:?}"
            );
        }
    }
}
