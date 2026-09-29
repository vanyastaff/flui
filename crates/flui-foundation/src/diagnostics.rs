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

/// What a redacted value renders as, matching Apple's own placeholder so a
/// reader of either platform's log recognises it.
///
/// `flui-log`'s privacy layer substitutes it for a field it redacts, and a
/// framework type that withholds text it may not publish (a caught panic's
/// payload, for one) formats as it, so both read the same in a log. It is
/// here, not in `flui-log`, because only composition roots may depend on
/// `flui-log`; that crate re-exports it at `flui_log::REDACTED_VALUE`.
pub const REDACTED_VALUE: &str = "<private>";
