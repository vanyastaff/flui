//! The redaction stage that sits in front of every device sink.
//!
//! [`RedactLayer`] wraps a sink and re-records events, span attributes, and
//! late span records with each field classified by
//! [`FieldPrivacy::classify`](super::privacy::FieldPrivacy::classify) before
//! the sink sees them. The sink itself stays policy-free: logcat and the Apple
//! layer render whatever values arrive, and what arrives for a private field
//! is the [`REDACTED_VALUE`] placeholder rather than the value.
//!
//! Enforcing the policy *here*, rather than inside each sink's renderer, is
//! what makes it one mechanism instead of a per-sink convention: a new device
//! sink gets the contract by being constructed wrapped, and a field can only
//! bypass classification by not being a `tracing` field at all.
//!
//! When nothing in a payload needs redaction the original event or attribute
//! set is forwarded untouched — typed values, message `Arguments`, and parent
//! linkage all reach the sink exactly as recorded. Synthesis happens only on
//! the paths where a value must be replaced, and the synthesized payload keeps
//! the original callsite metadata, field order, scalar types, and explicit /
//! contextual / root parent kind.
//!
//! Compiled on **every** target for the same reason as [`super::record`]: only
//! the sinks it wraps are target-gated, so the behaviour is covered by unit
//! tests that run on the ordinary CI host.

// Wrapped only by the Android and Apple sinks in a shipped build; the unit
// tests exercise it on every target. Same reasoning as `logcat.rs`.
#![cfg_attr(
    not(any(
        target_os = "android",
        target_os = "ios",
        all(target_os = "macos", feature = "apple-unified-logging")
    )),
    allow(
        dead_code,
        reason = "compiled on every target so the logic stays testable"
    )
)]

use core::fmt;
use std::borrow::Cow;

use tracing::field::{DebugValue, Field, Value, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};
use tracing_log::NormalizeEvent as _;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use super::privacy::{EventOrigin, FieldKind, FieldPrivacy, REDACTED_VALUE};

/// Capacity of the synthesized value set.
///
/// `tracing`'s macros stop well short of this many fields per callsite. If a
/// hand-built event with a redacted field somehow exceeds it, the tail fields
/// are dropped from the forwarded payload — the same shape of loss policy as
/// logcat's NUL truncation: losing the tail is a smaller loss than losing the
/// event, and forwarding the original instead would leak what was meant to be
/// redacted.
const MAX_FIELDS: usize = 32;

/// Redacts private fields before `inner` can observe them.
///
/// See the module docs for the contract. The wrapper forwards every other
/// `Layer` callback verbatim, so filtering, span lifecycle, and downcasting
/// behave as if the sink were installed bare.
pub(crate) struct RedactLayer<L> {
    inner: L,
}

impl<L> RedactLayer<L> {
    pub(crate) fn new(inner: L) -> Self {
        Self { inner }
    }
}

// Manual impl: the wrapped sink (e.g. `tracing_oslog::OsLogger`) does not
// implement `Debug`, and the wrapper's identity is what matters here.
impl<L> fmt::Debug for RedactLayer<L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RedactLayer")
            .finish_non_exhaustive()
    }
}

/// A rendered stand-in that prints without `Debug` quoting.
///
/// Public dynamic values that originally arrived through `record_debug` are
/// replayed through `record_debug` again via this adapter, so a sink cannot
/// tell redaction was in the path; replaying them as strings would change the
/// sink's quoting. `Cow` so the redaction placeholder — the common case on a
/// private-by-default sink — borrows the static literal instead of allocating
/// per redacted field.
struct PreRendered(Cow<'static, str>);

impl fmt::Debug for PreRendered {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One field's value, owned so it can outlive the borrowed visit and be
/// replayed into the synthesized value set with its original `Visit` callback.
enum Recorded {
    I64(i64),
    I128(i128),
    U64(u64),
    U128(u128),
    F64(f64),
    Bool(bool),
    /// Public text that arrived through `record_str`.
    Str(String),
    /// Public text that arrived through `record_debug` (including the
    /// message), or the redaction placeholder.
    Debugged(DebugValue<PreRendered>),
}

impl Recorded {
    fn redacted() -> Self {
        Self::Debugged(tracing::field::debug(PreRendered(Cow::Borrowed(
            REDACTED_VALUE,
        ))))
    }

    fn as_value(&self) -> &dyn Value {
        match self {
            Self::I64(value) => value,
            Self::I128(value) => value,
            Self::U64(value) => value,
            Self::U128(value) => value,
            Self::F64(value) => value,
            Self::Bool(value) => value,
            Self::Str(value) => value,
            Self::Debugged(value) => value,
        }
    }
}

/// Visits a payload once, classifying every field.
struct ClassifyingVisitor {
    origin: EventOrigin,
    entries: Vec<(Field, Recorded)>,
    redacted_any: bool,
}

impl ClassifyingVisitor {
    fn for_origin(origin: EventOrigin) -> Self {
        Self {
            origin,
            entries: Vec::new(),
            redacted_any: false,
        }
    }

    fn scalar(&mut self, field: &Field, value: Recorded) {
        let recorded = match FieldPrivacy::classify(field.name(), FieldKind::Scalar, self.origin) {
            FieldPrivacy::Public => value,
            FieldPrivacy::Private => {
                self.redacted_any = true;
                Recorded::redacted()
            }
        };
        self.entries.push((field.clone(), recorded));
    }

    /// `render` is deferred so a redacted value is never even formatted.
    fn dynamic(&mut self, field: &Field, render: impl FnOnce() -> Recorded) {
        let recorded = match FieldPrivacy::classify(field.name(), FieldKind::Dynamic, self.origin) {
            FieldPrivacy::Public => render(),
            FieldPrivacy::Private => {
                self.redacted_any = true;
                Recorded::redacted()
            }
        };
        self.entries.push((field.clone(), recorded));
    }
}

impl Visit for ClassifyingVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.scalar(field, Recorded::I64(value));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.scalar(field, Recorded::I128(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.scalar(field, Recorded::U64(value));
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.scalar(field, Recorded::U128(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.scalar(field, Recorded::F64(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.scalar(field, Recorded::Bool(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.dynamic(field, || Recorded::Str(value.to_owned()));
    }

    // `record_error` and `record_bytes` funnel here through `Visit`'s default
    // methods, so every free-form representation lands in the dynamic bucket.
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.dynamic(field, || {
            Recorded::Debugged(tracing::field::debug(PreRendered(Cow::Owned(format!(
                "{value:?}"
            )))))
        });
    }
}

/// Build the synthesized value set over `metadata`'s field set and hand it to
/// `then`.
///
/// The value set borrows the classified entries, so it can only exist inside a
/// callback.
fn with_value_set<R>(
    metadata: &'static Metadata<'static>,
    entries: &[(Field, Recorded)],
    then: impl FnOnce(&tracing::field::ValueSet<'_>) -> R,
) -> R {
    let (pad_field, _) = entries
        .first()
        .expect("BUG: a payload was synthesized although no field was classified");

    // Unused slots pair a real field with `None`, which `ValueSet` skips.
    let pairs: [(&Field, Option<&dyn Value>); MAX_FIELDS] =
        core::array::from_fn(|index| match entries.get(index) {
            Some((field, value)) => (field, Some(value.as_value())),
            None => (pad_field, None),
        });

    then(&metadata.fields().value_set(&pairs))
}

impl<S, L> Layer<S> for RedactLayer<L>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    L: Layer<S>,
{
    fn on_register_dispatch(&self, subscriber: &tracing::Dispatch) {
        self.inner.on_register_dispatch(subscriber);
    }

    fn on_layer(&mut self, subscriber: &mut S) {
        self.inner.on_layer(subscriber);
    }

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        self.inner.register_callsite(metadata)
    }

    fn enabled(&self, metadata: &Metadata<'_>, context: Context<'_, S>) -> bool {
        self.inner.enabled(metadata, context)
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        self.inner.max_level_hint()
    }

    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        // Spans cannot arrive through the `log` bridge — `log` has no spans —
        // so span payloads are always first-party.
        let mut visitor = ClassifyingVisitor::for_origin(EventOrigin::Native);
        attributes.record(&mut visitor);

        if !visitor.redacted_any {
            self.inner.on_new_span(attributes, id, context);
            return;
        }

        with_value_set(attributes.metadata(), &visitor.entries, |values| {
            let redacted = if attributes.is_contextual() {
                Attributes::new(attributes.metadata(), values)
            } else if let Some(parent) = attributes.parent() {
                Attributes::child_of(parent.clone(), attributes.metadata(), values)
            } else {
                Attributes::new_root(attributes.metadata(), values)
            };
            self.inner.on_new_span(&redacted, id, context);
        });
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, context: Context<'_, S>) {
        let mut visitor = ClassifyingVisitor::for_origin(EventOrigin::Native);
        values.record(&mut visitor);

        if !visitor.redacted_any {
            self.inner.on_record(span, values, context);
            return;
        }

        // `Record` carries no metadata; the span's own field set is the one
        // its fields belong to. A missing span cannot be synthesized against,
        // and forwarding the original would leak the value that was just
        // classified private — so the record is dropped, mirroring the
        // "loss beats leak" policy above.
        let Some(metadata) = context.span(span).map(|span_ref| span_ref.metadata()) else {
            return;
        };

        with_value_set(metadata, &visitor.entries, |value_set| {
            self.inner.on_record(span, &Record::new(value_set), context);
        });
    }

    fn on_follows_from(&self, span: &Id, follows: &Id, context: Context<'_, S>) {
        self.inner.on_follows_from(span, follows, context);
    }

    fn event_enabled(&self, event: &Event<'_>, context: Context<'_, S>) -> bool {
        self.inner.event_enabled(event, context)
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        // `is_log` matches on the bridge's callsite identity, so a first-party
        // event cannot impersonate its way to a different message default.
        let origin = if event.is_log() {
            EventOrigin::LogBridge
        } else {
            EventOrigin::Native
        };
        let mut visitor = ClassifyingVisitor::for_origin(origin);
        event.record(&mut visitor);

        if !visitor.redacted_any {
            self.inner.on_event(event, context);
            return;
        }

        with_value_set(event.metadata(), &visitor.entries, |values| {
            if event.is_contextual() {
                self.inner
                    .on_event(&Event::new(event.metadata(), values), context);
            } else {
                // An explicit parent is preserved; `None` reconstructs a root
                // event (`Event::new_child_of(None, ..)` builds `Parent::Root`).
                self.inner.on_event(
                    &Event::new_child_of(event.parent().cloned(), event.metadata(), values),
                    context,
                );
            }
        });
    }

    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        self.inner.on_enter(id, context);
    }

    fn on_exit(&self, id: &Id, context: Context<'_, S>) {
        self.inner.on_exit(id, context);
    }

    fn on_close(&self, id: Id, context: Context<'_, S>) {
        self.inner.on_close(id, context);
    }

    fn on_id_change(&self, old: &Id, new: &Id, context: Context<'_, S>) {
        self.inner.on_id_change(old, new, context);
    }

    #[allow(
        unsafe_code,
        reason = "forwards tracing-subscriber's type-erased layer lookup without changing the pointer"
    )]
    unsafe fn downcast_raw(&self, id: core::any::TypeId) -> Option<*const ()> {
        // SAFETY: the inner layer owns the downcast contract; this wrapper
        // returns its pointer unchanged and does not dereference it.
        unsafe { self.inner.downcast_raw(id) }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::span::Id;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    /// What a wrapped sink observed for one field, preserving the `Visit`
    /// callback the value arrived through — the oracle for "scalars stay
    /// typed" and "redaction replaces the value, not the callback".
    #[derive(Debug, Clone, PartialEq)]
    enum Seen {
        I64(i64),
        I128(i128),
        U64(u64),
        U128(u128),
        F64(f64),
        Bool(bool),
        Str(String),
        Debugged(String),
    }

    #[derive(Default)]
    struct SeenVisitor(Vec<(String, Seen)>);

    impl Visit for SeenVisitor {
        fn record_i64(&mut self, field: &Field, value: i64) {
            self.0.push((field.name().to_owned(), Seen::I64(value)));
        }
        fn record_i128(&mut self, field: &Field, value: i128) {
            self.0.push((field.name().to_owned(), Seen::I128(value)));
        }
        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.push((field.name().to_owned(), Seen::U64(value)));
        }
        fn record_u128(&mut self, field: &Field, value: u128) {
            self.0.push((field.name().to_owned(), Seen::U128(value)));
        }
        fn record_f64(&mut self, field: &Field, value: f64) {
            self.0.push((field.name().to_owned(), Seen::F64(value)));
        }
        fn record_bool(&mut self, field: &Field, value: bool) {
            self.0.push((field.name().to_owned(), Seen::Bool(value)));
        }
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0
                .push((field.name().to_owned(), Seen::Str(value.to_owned())));
        }
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.push((
                field.name().to_owned(),
                Seen::Debugged(format!("{value:?}")),
            ));
        }
    }

    /// The `(name, value)` pairs one payload delivered to the sink.
    type SeenFields = Vec<(String, Seen)>;

    #[derive(Debug, Clone, PartialEq)]
    struct CapturedEvent {
        fields: SeenFields,
        explicit_parent: Option<Id>,
        is_contextual: bool,
    }

    /// Stand-in sink recording exactly what crossed the redaction stage.
    #[derive(Clone, Default)]
    struct CaptureSink {
        events: Arc<Mutex<Vec<CapturedEvent>>>,
        span_attributes: Arc<Mutex<Vec<SeenFields>>>,
        span_records: Arc<Mutex<Vec<SeenFields>>>,
    }

    impl CaptureSink {
        fn events(&self) -> Vec<CapturedEvent> {
            self.events.lock().expect("BUG: test-local mutex").clone()
        }
        fn span_attributes(&self) -> Vec<SeenFields> {
            self.span_attributes
                .lock()
                .expect("BUG: test-local mutex")
                .clone()
        }
        fn span_records(&self) -> Vec<SeenFields> {
            self.span_records
                .lock()
                .expect("BUG: test-local mutex")
                .clone()
        }
    }

    impl<S> Layer<S> for CaptureSink
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(&self, attributes: &Attributes<'_>, _id: &Id, _context: Context<'_, S>) {
            let mut visitor = SeenVisitor::default();
            attributes.record(&mut visitor);
            self.span_attributes
                .lock()
                .expect("BUG: test-local mutex")
                .push(visitor.0);
        }

        fn on_record(&self, _span: &Id, values: &Record<'_>, _context: Context<'_, S>) {
            let mut visitor = SeenVisitor::default();
            values.record(&mut visitor);
            self.span_records
                .lock()
                .expect("BUG: test-local mutex")
                .push(visitor.0);
        }

        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut visitor = SeenVisitor::default();
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("BUG: test-local mutex")
                .push(CapturedEvent {
                    fields: visitor.0,
                    explicit_parent: event.parent().cloned(),
                    is_contextual: event.is_contextual(),
                });
        }
    }

    /// Run `emit` with a redaction-wrapped capture sink installed
    /// thread-locally, exactly the shape the device sinks are constructed in.
    fn behind_redaction(emit: impl FnOnce()) -> CaptureSink {
        let capture = CaptureSink::default();
        let subscriber = Registry::default().with(RedactLayer::new(capture.clone()));
        tracing::subscriber::with_default(subscriber, emit);
        capture
    }

    fn field<'event>(event: &'event CapturedEvent, name: &str) -> &'event Seen {
        &event
            .fields
            .iter()
            .find(|(field_name, _)| field_name == name)
            .unwrap_or_else(|| panic!("field `{name}` missing from {event:?}"))
            .1
    }

    /// `Seen::Debugged("<private>")` without repeating the literal in every
    /// assertion.
    fn redacted() -> Seen {
        Seen::Debugged(REDACTED_VALUE.to_owned())
    }

    #[test]
    fn a_dynamic_string_field_is_redacted_by_default() {
        let capture = behind_redaction(|| {
            tracing::info!(path = "/home/user/secret.txt", "asset loaded");
        });

        let events = capture.events();
        assert_eq!(events.len(), 1, "exactly one event: {events:?}");
        assert_eq!(*field(&events[0], "path"), redacted());
        assert_eq!(
            *field(&events[0], "message"),
            Seen::Debugged("asset loaded".to_owned()),
            "the message is the format-string analogue and stays published"
        );
    }

    #[test]
    fn a_debug_rendered_field_is_redacted_by_default() {
        let capture = behind_redaction(|| {
            tracing::info!(command = ?("draw_text", "hello"), size = 2_u64);
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(*field(event, "command"), redacted());
        assert_eq!(
            *field(event, "size"),
            Seen::U64(2),
            "a scalar beside a redacted field keeps its type"
        );
    }

    #[test]
    fn an_error_value_is_redacted_by_default() {
        let io_error = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file: /home/user/private.png",
        );
        let capture = behind_redaction(|| {
            tracing::warn!(error = &io_error as &dyn std::error::Error, "load failed");
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(
            *field(event, "error"),
            redacted(),
            "error renderings embed paths and OS strings; they are dynamic values"
        );
    }

    #[test]
    fn scalar_fields_pass_through_with_their_types() {
        let capture = behind_redaction(|| {
            tracing::info!(
                a = -1_i64,
                b = 2_u64,
                c = 3_i128,
                d = 4_u128,
                e = 0.5_f64,
                f = true,
                "scalars"
            );
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(*field(event, "a"), Seen::I64(-1));
        assert_eq!(*field(event, "b"), Seen::U64(2));
        assert_eq!(*field(event, "c"), Seen::I128(3));
        assert_eq!(*field(event, "d"), Seen::U128(4));
        assert_eq!(*field(event, "e"), Seen::F64(0.5));
        assert_eq!(*field(event, "f"), Seen::Bool(true));
        assert!(
            event.is_contextual && event.explicit_parent.is_none(),
            "sanity: the macro emitted a contextual event"
        );
    }

    #[test]
    fn a_public_marker_publishes_a_dynamic_field_verbatim() {
        let capture = behind_redaction(|| {
            tracing::info!(phase.public = "commit", detail.public = ?(1, 2), "ok");
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(
            *field(event, "phase.public"),
            Seen::Str("commit".to_owned()),
            "an opted-in string must arrive through record_str, unchanged"
        );
        assert_eq!(
            *field(event, "detail.public"),
            Seen::Debugged("(1, 2)".to_owned()),
            "an opted-in debug value must keep its record_debug rendering"
        );
    }

    #[test]
    fn a_private_marker_redacts_a_scalar() {
        let capture = behind_redaction(|| {
            tracing::info!(latitude.private = 52.52_f64, frame = 7_u64, "position");
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(*field(event, "latitude.private"), redacted());
        assert_eq!(*field(event, "frame"), Seen::U64(7));
    }

    #[test]
    fn field_order_survives_redaction() {
        let capture = behind_redaction(|| {
            tracing::info!(first = 1_u64, secret = "s", last = 3_u64, "ordered");
        });

        let events = capture.events();
        let names: Vec<&str> = events[0]
            .fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, vec!["message", "first", "secret", "last"]);
    }

    #[test]
    fn log_bridge_normalization_fields_pass_verbatim() {
        // The logcat layer derives its tag from `log.target` on a bridged
        // record; redacting these four would misfile every bridged event.
        let capture = behind_redaction(|| {
            tracing::info!(
                log.target = "wgpu_core",
                log.module_path = "wgpu_core::device",
                secret = "user text",
                "bridged-shaped"
            );
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(
            *field(event, "log.target"),
            Seen::Str("wgpu_core".to_owned())
        );
        assert_eq!(
            *field(event, "log.module_path"),
            Seen::Str("wgpu_core::device".to_owned())
        );
        assert_eq!(*field(event, "secret"), redacted());
    }

    #[test]
    fn span_attributes_are_classified_like_event_fields() {
        let capture = behind_redaction(|| {
            let _span = tracing::info_span!("session", token = "t0k3n", frame = 1_u64);
        });

        let attributes = capture.span_attributes();
        assert_eq!(attributes.len(), 1, "one span: {attributes:?}");
        assert!(
            attributes[0].contains(&("token".to_owned(), redacted())),
            "span attribute `token` must be redacted: {attributes:?}"
        );
        assert!(
            attributes[0].contains(&("frame".to_owned(), Seen::U64(1))),
            "span attribute `frame` must stay typed: {attributes:?}"
        );
    }

    #[test]
    fn late_span_records_are_classified() {
        let capture = behind_redaction(|| {
            let span = tracing::info_span!(
                "session",
                token = tracing::field::Empty,
                frame = tracing::field::Empty
            );
            span.record("token", "t0k3n");
            span.record("frame", 7_u64);
        });

        let records = capture.span_records();
        assert_eq!(records.len(), 2, "two record calls: {records:?}");
        assert_eq!(records[0], vec![("token".to_owned(), redacted())]);
        assert_eq!(records[1], vec![("frame".to_owned(), Seen::U64(7))]);
    }

    #[test]
    fn an_explicit_parent_survives_synthesis() {
        let capture = behind_redaction(|| {
            let span = tracing::info_span!("parent");
            tracing::info!(parent: &span, secret = "s", "child event");
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(*field(event, "secret"), redacted());
        assert!(
            event.explicit_parent.is_some(),
            "the synthesized event must keep its explicit parent: {event:?}"
        );
        assert!(!event.is_contextual);
    }

    #[test]
    fn a_fully_public_event_is_forwarded_untouched() {
        let capture = behind_redaction(|| {
            tracing::info!(frame = 7_u64, phase.public = "commit", "clean");
        });

        let events = capture.events();
        let event = &events[0];
        assert_eq!(*field(event, "frame"), Seen::U64(7));
        assert_eq!(
            *field(event, "phase.public"),
            Seen::Str("commit".to_owned())
        );
        assert!(
            event.is_contextual,
            "the original contextual event passes through"
        );
    }

    #[test]
    fn a_bridged_log_message_is_redacted_by_default() {
        // Writes the process-global `log` logger slot — the only unit test in
        // this crate that touches it (the `tests/` bridge scenarios each own a
        // separate process, per the one-slot-scenario-per-binary rule).
        tracing_log::log::set_boxed_logger(Box::new(tracing_log::LogTracer::new()))
            .expect("BUG: no other unit test may claim the `log` logger slot");
        tracing_log::log::set_max_level(tracing_log::log::LevelFilter::Info);

        let capture = behind_redaction(|| {
            tracing_log::log::info!(
                target: "third_party_dep",
                "loading {}",
                "/home/user/secret.txt"
            );
        });

        let events = capture.events();
        assert_eq!(
            events.len(),
            1,
            "the bridged record must arrive: {events:?}"
        );
        let event = &events[0];
        assert_eq!(
            *field(event, "message"),
            redacted(),
            "a bridged message is third-party interpolated text; STYLE.md's \
             fields-not-messages rule does not bind a dependency, so the \
             message is the leak channel and must not publish"
        );
        assert_eq!(
            *field(event, "log.target"),
            Seen::Str("third_party_dep".to_owned()),
            "the normalization fields must keep passing so the logcat tag survives"
        );
    }

    // --- composed with the logcat renderer, the exact Android line shape ----

    #[test]
    fn the_rendered_logcat_line_replaces_values_not_names() {
        let rendered = crate::test_support::capture_rendered_events_behind_redaction(|| {
            tracing::info!(
                frame = 7_u64,
                path = "/home/user/doc.txt",
                phase.public = "commit",
                "frame committed"
            );
        });

        assert_eq!(
            rendered,
            vec![format!(
                "frame committed | frame=7 path={REDACTED_VALUE} phase.public=commit"
            )]
        );
    }
}
