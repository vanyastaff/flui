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

use std::any::Any;
use std::fmt;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::{is_internal_invariant, payload_text};
use flui_rendering::RenderError;

/// Panic text made safe for the application's selected diagnostics policy.
///
/// `Verbatim` retains the exact string supplied by the panic site. `Redacted`
/// retains no source text and formats as [`flui_log::REDACTED_VALUE`].
///
/// # Examples
///
/// ```
/// use flui_app::PanicText;
///
/// assert_eq!(PanicText::Verbatim("boom".into()).to_string(), "boom");
/// assert_eq!(PanicText::Redacted.to_string(), flui_log::REDACTED_VALUE);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PanicText {
    /// The source text is retained exactly.
    Verbatim(Box<str>),
    /// The source text is not retained.
    Redacted,
}

impl fmt::Display for PanicText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verbatim(message) => formatter.write_str(message),
            Self::Redacted => formatter.write_str(flui_log::REDACTED_VALUE),
        }
    }
}

/// How much source detail frame-failure diagnostics retain.
///
/// This policy applies both to escaped panic payloads and to the text form
/// of typed pipeline errors. The typed [`RenderError`] itself remains in a
/// [`FrameFailureReport`] so a registered handler can inspect its variants
/// even when text diagnostics are redacted.
///
/// # Examples
///
/// ```
/// use flui_app::{AppConfig, FrameFailureDetail};
///
/// let config = AppConfig::new().with_frame_failure_detail(FrameFailureDetail::Redacted);
/// assert_eq!(config.frame_failure_detail, FrameFailureDetail::Redacted);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameFailureDetail {
    /// Retain no unstructured failure text.
    Redacted,
    /// Retain the source failure text verbatim.
    Verbatim,
}

impl Default for FrameFailureDetail {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Verbatim
        } else {
            Self::Redacted
        }
    }
}

impl FrameFailureDetail {
    /// Materialize text only for a policy that is allowed to retain it.
    fn materialize(self, verbatim: impl FnOnce() -> Box<str>) -> PanicText {
        match self {
            Self::Redacted => PanicText::Redacted,
            Self::Verbatim => PanicText::Verbatim(verbatim()),
        }
    }

    /// Classify a raw panic payload before applying the retention policy.
    pub(crate) fn panic_text(self, payload: &(dyn Any + Send)) -> (PanicText, bool) {
        let Some(raw_message) = payload_text(payload) else {
            return (PanicText::Redacted, false);
        };
        let internal_invariant = is_internal_invariant(raw_message);
        (
            self.materialize(|| Box::<str>::from(raw_message)),
            internal_invariant,
        )
    }

    /// Apply the same retention policy to a typed pipeline error's text.
    pub(crate) fn pipeline_text(self, error: &RenderError) -> PanicText {
        self.materialize(|| error.to_string().into_boxed_str())
    }
}

/// The last frame segment entered for one presentation.
///
/// A presentation stores this value before it runs the corresponding work.
/// If that work unwinds, the value therefore identifies the segment that
/// failed instead of being restored to an earlier phase. The next attempted
/// frame starts again at [`Self::Build`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SegmentPhase {
    /// Widget-tree build work, including frame-entry bookkeeping.
    Build,
    /// Inactive-element finalization immediately after the build drain.
    Finalize,
    /// Layout, compositing, paint, and semantics pipeline orchestration.
    Pipeline,
    /// Post-pipeline link extraction and lazy-child service work.
    Tail,
    /// Performance-overlay attachment and scene construction.
    Scene,
}

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
        /// The panic payload text allowed by the configured
        /// [`FrameFailureDetail`] policy.
        ///
        /// [`PanicText::Redacted`] retains no raw payload. A registered
        /// handler receives this same policy-filtered value; selecting
        /// [`FrameFailureDetail::Verbatim`] is the explicit opt-in to retain
        /// and deliver string payloads.
        message: PanicText,
        /// The last frame segment entered before the panic escaped.
        phase: SegmentPhase,
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
        /// The pipeline's own typed error. The realm applies
        /// [`FrameFailureDetail`] only when formatting this error for
        /// `tracing`; a registered handler retains the typed value and can
        /// inspect its variants without rendering private text.
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
///
/// A handler that itself panics is contained at the delivery site (its
/// panic cannot re-enter the frame boundary or take down sibling
/// presentations), logged at error level as an embedder bug, and — since
/// the report was already fully traced before delivery — loses no
/// diagnostics. Delivery is one call per report, never a retry loop, and
/// the handler stays registered: a transiently-broken handler resumes
/// receiving reports once it stops panicking.
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{FrameFailureDetail, PanicText};

    #[test]
    fn panic_text_display_obeys_the_selected_detail() {
        assert_eq!(
            PanicText::Verbatim("panic detail".into()).to_string(),
            "panic detail"
        );
        assert_eq!(PanicText::Redacted.to_string(), flui_log::REDACTED_VALUE);
    }

    #[test]
    fn frame_failure_detail_default_matches_the_build_profile() {
        let expected = if cfg!(debug_assertions) {
            FrameFailureDetail::Verbatim
        } else {
            FrameFailureDetail::Redacted
        };
        assert_eq!(FrameFailureDetail::default(), expected);
    }

    #[test]
    fn pipeline_text_uses_the_same_privacy_gate_as_panics() {
        const SENTINEL: &str = "private-pipeline-sentinel";
        let error = flui_rendering::RenderError::semantics(SENTINEL);

        let redacted = FrameFailureDetail::Redacted.pipeline_text(&error);
        assert_eq!(redacted, PanicText::Redacted);
        assert_eq!(redacted.to_string(), flui_log::REDACTED_VALUE);
        assert!(!redacted.to_string().contains(SENTINEL));

        let verbatim = FrameFailureDetail::Verbatim.pipeline_text(&error);
        assert!(
            verbatim.to_string().contains(SENTINEL),
            "verbatim pipeline diagnostics must retain the source error"
        );
    }

    #[test]
    fn redacted_policy_never_evaluates_the_text_materializer() {
        let calls = AtomicUsize::new(0);
        let text = FrameFailureDetail::Redacted.materialize(|| {
            calls.fetch_add(1, Ordering::Relaxed);
            Box::<str>::from("must not be retained")
        });

        assert_eq!(text, PanicText::Redacted);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn non_string_panic_payload_is_never_presented_as_invented_text() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(7_u32);
        let (text, internal_invariant) = FrameFailureDetail::Verbatim.panic_text(payload.as_ref());

        assert_eq!(text, PanicText::Redacted);
        assert!(!internal_invariant);
    }
}
