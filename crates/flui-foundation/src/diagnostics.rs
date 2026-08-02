//! Shared vocabulary for structured `tracing` fields.
//!
//! Foundation owns the *names* because every framework crate emits them and
//! none of them may depend on the logging backend. A collector — a log sink, a
//! devtools timeline, a trace exporter — joins events across subsystems by
//! these names, so they are a contract in the same sense a wire format is: one
//! crate spelling `realm` where another spells `realm_id` silently breaks
//! correlation without breaking a build.
//!
//! ```rust
//! use flui_foundation::diagnostics;
//!
//! # let realm = 1_u64;
//! # let frame = 2_u64;
//! tracing::info!(
//!     { diagnostics::REALM_ID } = realm,
//!     { diagnostics::FRAME_ID } = frame,
//!     "presentation committed"
//! );
//! ```
//!
//! Installing a subscriber to *receive* these events is a composition-root
//! decision and lives in `flui-log`, not here.
//!
//! # Which of these are emitted today
//!
//! [`PRESENTATION_ID`] and [`REALM_ID`] have production call sites in
//! `flui-app`. [`RUNTIME_ID`], [`FRAME_ID`], and [`WORKER_ID`] name concepts
//! whose owners — the host-driven runtime, the frame pipeline's per-frame
//! identity, and runtime-owned workers — do not exist yet. The names are fixed
//! here first on purpose: a vocabulary agreed after three subsystems have each
//! invented their own spelling is not a vocabulary. They are not dead code
//! standing in for a feature; a collector can already project on them, and the
//! test below proves a constant really controls the emitted field name rather
//! than merely describing it.

/// The runtime instance an event belongs to.
pub const RUNTIME_ID: &str = "runtime_id";

/// The UI realm — the owner-thread scope holding one element/render tree.
pub const REALM_ID: &str = "realm_id";

/// The presentation (surface/window) an event concerns.
pub const PRESENTATION_ID: &str = "presentation_id";

/// The frame an event was produced during.
pub const FRAME_ID: &str = "frame_id";

/// The worker thread or task an event was produced on.
pub const WORKER_ID: &str = "worker_id";

/// Every correlation field, coarsest scope first.
///
/// A collector can use this to project a correlation key without hard-coding
/// the list, and a test can use it to prove none of them is dropped in transit.
pub const CORRELATION_FIELDS: [&str; 5] =
    [RUNTIME_ID, REALM_ID, PRESENTATION_ID, FRAME_ID, WORKER_ID];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_holds_every_named_field_exactly_once() {
        // The list and the individual constants are two views of one fact;
        // this is what keeps them from drifting apart.
        let mut sorted = CORRELATION_FIELDS;
        sorted.sort_unstable();
        let mut deduplicated = sorted.to_vec();
        deduplicated.dedup();

        assert_eq!(
            deduplicated.len(),
            CORRELATION_FIELDS.len(),
            "a correlation field name is duplicated"
        );

        for name in [RUNTIME_ID, REALM_ID, PRESENTATION_ID, FRAME_ID, WORKER_ID] {
            assert!(
                CORRELATION_FIELDS.contains(&name),
                "`{name}` is defined but missing from CORRELATION_FIELDS"
            );
        }
    }

    /// Emit one event through a thread-local subscriber and return the field
    /// names `tracing` actually recorded.
    fn recorded_field_names(emit: impl FnOnce()) -> Vec<String> {
        use std::sync::{Arc, Mutex};

        use tracing::field::{Field, Visit};
        use tracing_subscriber::layer::{Context, SubscriberExt as _};
        use tracing_subscriber::registry::LookupSpan;
        use tracing_subscriber::{Layer, Registry};

        #[derive(Clone, Default)]
        struct NameCapture(Arc<Mutex<Vec<String>>>);

        struct NameCollector<'a>(&'a mut Vec<String>);

        impl Visit for NameCollector<'_> {
            fn record_debug(&mut self, field: &Field, _value: &dyn core::fmt::Debug) {
                self.0.push(field.name().to_owned());
            }
        }

        impl<S> Layer<S> for NameCapture
        where
            S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
        {
            fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
                let mut names = Vec::new();
                event.record(&mut NameCollector(&mut names));
                self.0
                    .lock()
                    .expect("BUG: the capture mutex is only locked by this test's thread")
                    .extend(names);
            }
        }

        let capture = NameCapture::default();
        tracing::subscriber::with_default(Registry::default().with(capture.clone()), emit);

        capture
            .0
            .lock()
            .expect("BUG: the capture mutex is only locked by this test's thread")
            .clone()
    }

    #[test]
    fn the_constants_really_become_the_emitted_field_names() {
        // The constants are only a contract if using one at a call site
        // produces exactly that name in the recorded event. A constant that
        // documented a name without controlling it would drift silently.
        let names = recorded_field_names(|| {
            tracing::info!(
                { RUNTIME_ID } = 1_u64,
                { REALM_ID } = 2_u64,
                { PRESENTATION_ID } = 3_u64,
                { FRAME_ID } = 4_u64,
                { WORKER_ID } = 5_u64,
                "presentation committed"
            );
        });

        for name in CORRELATION_FIELDS {
            assert!(
                names.iter().any(|recorded| recorded == name),
                "`{name}` was not the recorded field name; got {names:?}"
            );
        }
    }

    #[test]
    fn names_are_snake_case_identifiers() {
        // `tracing`'s field syntax accepts dotted names, but a dot makes the
        // field unusable as a bare identifier at the call site and inconsistent
        // across the sinks that flatten fields into text.
        for name in CORRELATION_FIELDS {
            assert!(
                name.chars()
                    .all(|character| character.is_ascii_lowercase() || character == '_'),
                "`{name}` is not a snake_case identifier"
            );
        }
    }
}
