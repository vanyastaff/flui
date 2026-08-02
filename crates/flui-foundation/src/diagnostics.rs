//! Shared names for structured `tracing` fields.
//!
//! # The rule
//!
//! Instrument with an identifier that names the **real owner or operation** an
//! event belongs to — not the runtime's current internal topology. A field name
//! is read by log sinks, devtools, exporters, crash reports, and by filters
//! people write by hand, so it becomes a consumer-visible format far faster
//! than the type it was named after becomes stable. Naming a field for an
//! internal construct turns that construct's removal from a refactor into a
//! format migration for everyone downstream.
//!
//! A constant lives here when, and only when, the concept it names is durable
//! *and* something in the workspace already emits it. A name with no emitter is
//! a guess about a design that has not been made yet; the crate that eventually
//! owns the concept can add the constant once it has something real to name.
//!
//! There is deliberately no exported list of "all the correlation fields". A
//! closed enumeration of field names *is* the wire contract, and publishing one
//! freezes the set at whatever the runtime happened to look like on the day it
//! was written.
//!
//! # Why constants at all
//!
//! One spelling per concept. Without a shared constant, one crate writes
//! `presentation` where another writes `presentation_id`, and correlation
//! silently stops working across the seam without anything failing to build.
//! The constant is what makes the name a single fact.
//!
//! ```rust
//! use flui_foundation::diagnostics;
//!
//! # let presentation = 1_u64;
//! tracing::info!(
//!     { diagnostics::PRESENTATION_ID } = presentation,
//!     "presentation committed"
//! );
//! ```
//!
//! Installing a subscriber to *receive* these events is a composition-root
//! decision and lives in `flui-log`, not here.

/// The logical presented root an event concerns.
///
/// A presentation may currently map to a native window, but the identifier does
/// not encode that topology: a future texture or external render target remains
/// a presentation without pretending to be a window or GPU surface.
pub const PRESENTATION_ID: &str = "presentation_id";

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, SubscriberExt as _};
    use tracing_subscriber::registry::LookupSpan;
    use tracing_subscriber::{Layer, Registry};

    use super::*;

    /// Emit one event through a thread-local subscriber and return its fields.
    fn recorded_fields(emit: impl FnOnce()) -> Vec<(String, String)> {
        #[derive(Clone, Default)]
        struct FieldCapture(Arc<Mutex<Vec<(String, String)>>>);

        struct FieldCollector<'a>(&'a mut Vec<(String, String)>);

        impl Visit for FieldCollector<'_> {
            fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
                self.0.push((field.name().to_owned(), format!("{value:?}")));
            }
        }

        impl<S> Layer<S> for FieldCapture
        where
            S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
        {
            fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
                let mut fields = Vec::new();
                event.record(&mut FieldCollector(&mut fields));
                self.0
                    .lock()
                    .expect("BUG: the capture mutex is only locked by this test's thread")
                    .extend(fields);
            }
        }

        let capture = FieldCapture::default();
        tracing::subscriber::with_default(Registry::default().with(capture.clone()), emit);

        capture
            .0
            .lock()
            .expect("BUG: the capture mutex is only locked by this test's thread")
            .clone()
    }

    #[test]
    fn the_constant_really_becomes_the_emitted_field_name() {
        // The constant is only worth having if using it at a call site produces
        // exactly that name in the recorded event. One that documented a name
        // without controlling it would let call sites drift while looking
        // correct.
        let fields = recorded_fields(|| {
            tracing::info!({ PRESENTATION_ID } = 3_u64, "presentation committed");
        });

        assert!(
            fields.iter().any(|(name, _)| name == PRESENTATION_ID),
            "`{PRESENTATION_ID}` was not the recorded field name; got {fields:?}"
        );
    }

    #[test]
    fn a_real_presentation_id_uses_its_packed_numeric_value() {
        let id = crate::PresentationId::new(1);
        let fields = recorded_fields(|| {
            tracing::info!({ PRESENTATION_ID } = id.as_u64(), "presentation committed");
        });

        let value = fields
            .iter()
            .find_map(|(name, value)| (name == PRESENTATION_ID).then_some(value.as_str()));
        let expected = id.as_u64().to_string();
        assert_eq!(value, Some(expected.as_str()));
    }

    #[test]
    fn the_name_is_a_snake_case_identifier() {
        // `tracing` accepts dotted field names, but a dot makes the field
        // unusable as a bare identifier at a call site and renders
        // inconsistently across the sinks that flatten fields into text.
        assert!(
            PRESENTATION_ID
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '_'),
            "`{PRESENTATION_ID}` is not a snake_case identifier"
        );
    }
}
