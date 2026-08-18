//! Typed frame-failure reporting — the embedder-visible half of the
//! presentation-frame transaction boundary (ADR-0048).
//!
//! A frame produced for one presentation can fail two ways: the pipeline
//! returns a structured [`flui_rendering::RenderError`], or a panic escapes
//! the presentation's build/layout/paint segment and is caught at the
//! realm's per-presentation `catch_unwind` boundary. Either way the frame
//! is dropped for that presentation only — sibling presentations in the
//! same realm, and every other realm, keep pumping — and the failure is
//! surfaced here as a [`FrameFailureReport`] instead of being a silent
//! skip: `tracing` carries the structured diagnostics, and an embedder
//! that registered a [`FrameFailureHandler`] via
//! [`AppConfig::with_frame_failure_handler`](crate::AppConfig::with_frame_failure_handler)
//! receives the typed report synchronously on the UI thread.
//!
//! The report deliberately carries the presentation's own
//! [`PresentationAddress`] — ownership identity, per issue #561's
//! diagnostics criterion — so a multi-window embedder can tell *which*
//! window's frame failed and decide its own recovery (ignore and let the
//! armed retry run, close the window, or restart the app).

use flui_foundation::PresentationAddress;
use flui_rendering::RenderError;

/// Why one presentation's frame failed.
///
/// Mirrors the failure taxonomy of issue #561: structured pipeline errors
/// (caller validation, recoverable subtree, backend) keep their typed
/// [`RenderError`] shape, while unstructured panics that escaped every
/// inner recovery layer (per-element build recovery, the pipeline's own
/// layout/paint `catch_unwind`) surface as [`Self::SegmentPanic`].
#[derive(Debug)]
#[non_exhaustive]
pub enum FrameFailureKind {
    /// A panic escaped the presentation's build/layout/paint segment and
    /// was caught at the realm's frame-transaction boundary — the
    /// last-resort seam, reached only when every inner containment layer
    /// (build-phase `ErrorView` substitution, the pipeline's
    /// `RenderError::Poisoned` wrapper) did not apply, e.g. a panicking
    /// `ViewState::dispose` during tree finalization.
    SegmentPanic {
        /// The panic payload rendered to text, when the payload was a
        /// string (`panic!("…")`); `None` for non-string payloads.
        ///
        /// May contain anything user code interpolated into its panic
        /// message. The `tracing` emission of this value is a plain
        /// string field, which FLUI's device sinks redact by default
        /// (see `flui-log`'s private-by-default field classification);
        /// an embedder handler receives it verbatim and owns its further
        /// handling.
        message: Option<Box<str>>,
        /// Whether the payload carries the `BUG:` prefix of
        /// `docs/PANIC-POLICY.md`'s invariant convention — a framework
        /// invariant violation rather than an application-code failure.
        /// Reported loudly (error-level tracing names it explicitly) but
        /// still contained: the process survives so sibling
        /// presentations keep their frames.
        internal_invariant: bool,
    },
    /// The build/layout/paint pipeline itself refused the frame with a
    /// structured error. The frame was dropped before submission; the
    /// failed node's dirty state is retained by the pipeline for the
    /// armed retry.
    Pipeline {
        /// The pipeline's own typed error.
        error: RenderError,
    },
}

/// One contained frame failure, addressed to the presentation whose frame
/// was dropped.
///
/// Delivered to the registered [`FrameFailureHandler`] (if any) and
/// mirrored into `tracing` at error level. The last successfully
/// presented frame for this presentation stays on screen — a failed frame
/// never submits a blank or partial scene in its place.
#[derive(Debug)]
#[non_exhaustive]
pub struct FrameFailureReport {
    /// Ownership identity: which realm incarnation and which presentation
    /// within it produced the failed frame.
    pub address: PresentationAddress,
    /// What failed.
    pub kind: FrameFailureKind,
    /// How many frames in a row have now failed for this presentation
    /// (`1` for the first failure; reset by the next cleanly completed
    /// segment). An embedder can key its own escalation off this — e.g.
    /// tear the window down once the count shows the armed retry is not
    /// recovering.
    pub consecutive_failures: u32,
}

/// An embedder-registered callback receiving every [`FrameFailureReport`].
///
/// Register via
/// [`AppConfig::with_frame_failure_handler`](crate::AppConfig::with_frame_failure_handler).
/// Invoked synchronously on the UI thread, from inside the frame pump,
/// immediately after the failure is contained. Keep it lightweight and
/// re-entrancy-free: record/forward the report and return — do not call
/// back into FLUI APIs (opening windows, attaching widgets) from inside
/// the handler; the realm that produced the report is mid-frame.
#[derive(Clone)]
pub struct FrameFailureHandler(std::sync::Arc<dyn Fn(&FrameFailureReport) + Send + Sync>);

impl FrameFailureHandler {
    /// Wrap a callback as a registerable handler.
    pub fn new(handler: impl Fn(&FrameFailureReport) + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(handler))
    }

    /// Deliver one report to the wrapped callback.
    pub(crate) fn call(&self, report: &FrameFailureReport) {
        (self.0)(report);
    }
}

impl std::fmt::Debug for FrameFailureHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The wrapped closure is opaque; identity is all Debug can say.
        f.debug_tuple("FrameFailureHandler").finish()
    }
}
