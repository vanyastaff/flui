//! Which `tracing` fields a device sink may publish, and which it must redact.
//!
//! # Why a classification exists
//!
//! Android's logcat and Apple's unified log are *archives*: anyone holding the
//! device (or a sysdiagnose) can read every line ever written to them. A
//! `tracing` field that reaches one of those sinks verbatim is therefore
//! world-readable, which is the wrong default for a framework that routinely
//! sits between user content and the screen.
//!
//! Apple's own logging API answers this with per-value privacy: *"By default,
//! the system doesn't redact integer, floating-point and Boolean values, but it
//! does redact the contents of dynamic strings and complex dynamic objects"*,
//! overridable per value with `%{public}` / `%{private}` (or `privacy: .public`
//! in Swift) — see "Generating Log Messages from Your Code" in the `os`
//! framework documentation. Android has no equivalent knob: logcat is a plain
//! line transport, so whatever policy exists has to be applied *before* the
//! line is written.
//!
//! FLUI adopts Apple's model as the cross-platform contract and enforces it in
//! front of **both** device sinks, so a producer states a field's privacy once,
//! in the field name, without knowing which backend is installed:
//!
//! | Recorded as | Default | Override |
//! |---|---|---|
//! | scalar (`i64`, `u64`, `i128`, `u128`, `f64`, `bool`) | published | `.private` suffix redacts |
//! | dynamic (`&str`, `Debug`, `Display`, errors, bytes) | redacted to [`REDACTED_VALUE`] | `.public` suffix publishes |
//! | `message`, [native](EventOrigin::Native) `tracing` event | published | — |
//! | `message`, [bridged](EventOrigin::LogBridge) from the `log` facade | redacted | — (deliberately none) |
//!
//! ```rust
//! tracing::info!(
//!     frame = 7_u64,                    // scalar: published
//!     path = %std::path::Path::new("/home/u/doc.txt").display(), // dynamic: redacted
//!     phase.public = "commit",          // dynamic, opted in: published
//!     latitude.private = 52.52_f64,     // scalar, opted out: redacted
//!     "frame committed",                // message: published
//! );
//! ```
//!
//! # The message is the format string — for first-party events only
//!
//! Apple redacts interpolated values but always publishes the compile-time
//! format string around them. `tracing` pre-formats the message at the
//! callsite, so the two cannot be separated here; a **native** `tracing`
//! message is treated as the format-string analogue and published verbatim.
//! That leaves one residual hole this module cannot close: first-party code
//! interpolating a user-provided value *into the message*
//! (`info!("loading {path}")`) publishes it. STYLE.md §17 already requires
//! machine-readable values to be structured fields rather than message
//! interpolations, which is what keeps this hole theoretical *for code this
//! workspace reviews*.
//!
//! No such rule binds a dependency. A record arriving through the `log`
//! compatibility bridge is a third party's `log::info!("loading {}", path)`
//! with the interpolation already flattened into `message` — free-form text
//! from code that cannot carry a marker and was never reviewed against
//! STYLE.md. Native `os_log` would have redacted exactly that dynamic string,
//! so a bridged message is classified [`Private`](FieldPrivacy::Private), with
//! deliberately no opt-out: the third party cannot classify its own text, and
//! the embedding application should not vouch for text it does not produce.
//! The record itself still ships — its level, its `log.*` provenance fields,
//! and therefore its logcat tag / target grouping all publish — so a device
//! log still shows *that* `wgpu` warned, just not the un-reviewable sentence.
//! A developer who needs the sentences has the desktop sinks, which do not
//! redact.
//!
//! # Marker mechanics
//!
//! The marker is a trailing dotted segment of the field name (`path.public`,
//! `latitude.private`), because the field name is the only channel that
//! survives `tracing`'s field system unchanged from an instrumentation site in
//! a library crate — which depends on `tracing` alone — to a sink assembled
//! here. Names render as written; the marker is not stripped, so the
//! classification decision stays visible in the log line. A field literally
//! named `public` or `private` (no dot) carries no marker.
//!
//! # What is *not* classified
//!
//! The desktop and web-console sinks write to the developer's own terminal or
//! `DevTools`, not to a device archive, and publish fields verbatim; host-side
//! tooling (`flui-build`, `flui-cli`) may keep logging full paths. The four
//! `log.*` normalization fields the `log` bridge attaches (`log.target`,
//! `log.module_path`, `log.file`, `log.line`) name compile-time source
//! locations already embedded in the binary, and the logcat tag is derived
//! from `log.target`; they are published so bridged records keep grouping
//! correctly.

/// Whether a device sink may publish a field's value.
///
/// The vocabulary is Apple's (`%{public}` / `%{private}`), reused verbatim so
/// the cross-platform contract and the platform mechanism it maps onto read as
/// one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldPrivacy {
    /// The value is published verbatim.
    Public,
    /// The value is replaced by [`REDACTED_VALUE`] before it reaches the sink.
    Private,
}

/// How a field's value arrived at the sink, per `tracing`'s `Visit` protocol.
///
/// This is the axis Apple's default turns on: scalars are safe to publish
/// because they cannot embed free-form user content; anything rendered from a
/// string or a `Debug`/`Display` implementation can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// A numeric or boolean value (`record_i64`, `record_u64`, `record_i128`,
    /// `record_u128`, `record_f64`, `record_bool`).
    Scalar,
    /// A string, `Debug`/`Display` rendering, error, or byte payload — every
    /// `Visit` callback that can carry free-form text.
    Dynamic,
}

/// Where an event entered `tracing`.
///
/// The axis decides only the `message` field's default: a native message is
/// the compile-time sentence a reviewed callsite wrote; a bridged message is a
/// third party's fully interpolated string. Named fields classify identically
/// under both origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventOrigin {
    /// Emitted through `tracing`'s own macros. The callsite can carry field
    /// markers and is subject to this workspace's fields-not-messages rule, so
    /// its message publishes.
    Native,
    /// Forwarded from the `log` facade by the compatibility bridge
    /// (`tracing_log::LogTracer`). The message is un-reviewable third-party
    /// text with its interpolations already flattened in, so it redacts —
    /// with no marker escape, because the third party cannot classify its own
    /// text and the embedder should not vouch for text it does not produce.
    LogBridge,
}

/// Trailing field-name marker that publishes a dynamic value.
pub const PUBLIC_FIELD_SUFFIX: &str = ".public";

/// Trailing field-name marker that redacts a scalar value.
pub const PRIVATE_FIELD_SUFFIX: &str = ".private";

/// What a redacted value renders as, matching Apple's own placeholder so a
/// reader of either platform's log recognises it.
pub const REDACTED_VALUE: &str = "<private>";

/// The event field `tracing`'s macros store the formatted message under.
pub(crate) const MESSAGE_FIELD: &str = "message";

/// Normalization fields attached by the `log` compatibility bridge.
///
/// They carry compile-time source locations (already embedded in the binary by
/// `file!()`/`module_path!()`) and the logcat layer derives its tag from
/// `log.target`, so redacting them would break record grouping while hiding
/// nothing that is not in the executable.
const LOG_BRIDGE_FIELDS: [&str; 4] = ["log.target", "log.module_path", "log.file", "log.line"];

impl FieldPrivacy {
    /// Classify one field by its name, the kind of value recorded for it, and
    /// the origin of the event carrying it.
    ///
    /// An explicit marker wins over the kind default, `.private` over
    /// `.public`. The `message` field publishes for a native event and
    /// redacts for a bridged one; the `log.*` bridge fields are always
    /// published (see the module docs for why).
    #[must_use]
    pub fn classify(name: &str, kind: FieldKind, origin: EventOrigin) -> Self {
        if name == MESSAGE_FIELD {
            return match origin {
                EventOrigin::Native => Self::Public,
                EventOrigin::LogBridge => Self::Private,
            };
        }
        if LOG_BRIDGE_FIELDS.contains(&name) {
            return Self::Public;
        }
        if name.ends_with(PRIVATE_FIELD_SUFFIX) {
            return Self::Private;
        }
        if name.ends_with(PUBLIC_FIELD_SUFFIX) {
            return Self::Public;
        }
        match kind {
            FieldKind::Scalar => Self::Public,
            FieldKind::Dynamic => Self::Private,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_values_are_private_by_default() {
        assert_eq!(
            FieldPrivacy::classify("path", FieldKind::Dynamic, EventOrigin::Native),
            FieldPrivacy::Private
        );
    }

    #[test]
    fn scalar_values_are_public_by_default() {
        assert_eq!(
            FieldPrivacy::classify("frame", FieldKind::Scalar, EventOrigin::Native),
            FieldPrivacy::Public
        );
    }

    #[test]
    fn a_public_marker_publishes_a_dynamic_value() {
        assert_eq!(
            FieldPrivacy::classify("phase.public", FieldKind::Dynamic, EventOrigin::Native),
            FieldPrivacy::Public
        );
    }

    #[test]
    fn a_private_marker_redacts_a_scalar() {
        assert_eq!(
            FieldPrivacy::classify("latitude.private", FieldKind::Scalar, EventOrigin::Native),
            FieldPrivacy::Private
        );
    }

    #[test]
    fn the_private_marker_wins_over_the_public_marker() {
        // `a.public.private` ends with `.private`, and only the trailing
        // segment is the marker; deny beats allow when both could match.
        assert_eq!(
            FieldPrivacy::classify("a.public.private", FieldKind::Scalar, EventOrigin::Native),
            FieldPrivacy::Private
        );
    }

    #[test]
    fn a_native_message_is_public() {
        assert_eq!(
            FieldPrivacy::classify(MESSAGE_FIELD, FieldKind::Dynamic, EventOrigin::Native),
            FieldPrivacy::Public
        );
    }

    #[test]
    fn a_bridged_message_is_private() {
        // A `log`-bridge message is a third party's fully interpolated string;
        // the fields-not-messages rule does not bind a dependency.
        assert_eq!(
            FieldPrivacy::classify(MESSAGE_FIELD, FieldKind::Dynamic, EventOrigin::LogBridge),
            FieldPrivacy::Private
        );
    }

    #[test]
    fn named_fields_classify_identically_under_both_origins() {
        for origin in [EventOrigin::Native, EventOrigin::LogBridge] {
            assert_eq!(
                FieldPrivacy::classify("frame", FieldKind::Scalar, origin),
                FieldPrivacy::Public
            );
            assert_eq!(
                FieldPrivacy::classify("path", FieldKind::Dynamic, origin),
                FieldPrivacy::Private
            );
            assert_eq!(
                FieldPrivacy::classify("log.target", FieldKind::Dynamic, origin),
                FieldPrivacy::Public
            );
        }
    }

    #[test]
    fn a_field_literally_named_public_carries_no_marker() {
        // The marker is the *dotted suffix*; a bare name is just a name, and a
        // dynamic value under it stays private.
        assert_eq!(
            FieldPrivacy::classify("public", FieldKind::Dynamic, EventOrigin::Native),
            FieldPrivacy::Private
        );
        assert_eq!(
            FieldPrivacy::classify("private", FieldKind::Scalar, EventOrigin::Native),
            FieldPrivacy::Public
        );
    }

    #[test]
    fn log_bridge_normalization_fields_are_published() {
        for name in ["log.target", "log.module_path", "log.file", "log.line"] {
            assert_eq!(
                FieldPrivacy::classify(name, FieldKind::Dynamic, EventOrigin::Native),
                FieldPrivacy::Public,
                "bridge field `{name}` must stay publishable or bridged records lose grouping"
            );
        }
    }

    #[test]
    fn a_dotted_field_that_is_not_a_marker_keeps_the_kind_default() {
        assert_eq!(
            FieldPrivacy::classify("log.password", FieldKind::Dynamic, EventOrigin::Native),
            FieldPrivacy::Private,
            "only the four exact bridge names are exempt, not the `log.` prefix"
        );
    }
}
