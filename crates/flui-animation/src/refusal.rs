//! Diagnostics for refused animation commands (ADR-0220).
//!
//! A built-in driver can discard a command's `Result`, so every refusal is
//! also reported: on [`REFUSAL_TARGET`], with the command's `operation`, the
//! error's stable [`kind`](crate::AnimationError::kind) and the error itself,
//! plus the refusing controller's debug `label` and `created_at` site.

use crate::AnimationError;
use crate::origin::ControllerOrigin;

/// The `tracing` target every refused animation command reports on. Like
/// [`AnimationError::kind`], it is a stability boundary: filters select
/// refusals by this name.
pub const REFUSAL_TARGET: &str = "flui_animation::refused";

/// The `tracing` target a panic reports on when it is caught while the
/// animation is already recovering from an earlier one. The first failure is
/// the one resumed; each later one is reported here instead of being dropped.
pub const CONTAINED_PANIC_TARGET: &str = "flui_animation::contained_panic";

/// The `tracing` target a simulation run reports on once it has run for a
/// minute of its own time without settling: a custom simulation whose
/// `is_done` never holds keeps a frame loop alive. Debug and `diagnostics`
/// builds only.
pub const NEVER_SETTLES_TARGET: &str = "flui_animation::never_settles";

/// Finish `retirement`, reporting each panic it caught after the first on
/// [`CONTAINED_PANIC_TARGET`] with the `phase` it was caught in and the
/// controller's `label`. A failing report cannot replace the first failure.
pub(crate) fn finish_reporting(
    retirement: crate::animation::Retirement,
    phase: &'static str,
    label: Option<&str>,
) {
    retirement.finish_reporting(|competing| {
        for payload in competing {
            tracing::warn!(
                target: CONTAINED_PANIC_TARGET,
                phase,
                payload = payload.as_str(),
                label,
                "a panic was contained while recovering from an earlier one",
            );
        }
    });
}

/// The controller a diagnostic names: its display fields and, with the
/// `diagnostics` feature, the span its events are parented to.
pub(crate) struct Subject<'a> {
    pub(crate) origin: &'a ControllerOrigin,
    /// The controller span, with the `diagnostics` feature.
    #[cfg(feature = "diagnostics")]
    pub(crate) span: Option<tracing::Span>,
}

impl Subject<'_> {
    /// Run `event` with the controller span's id as its parent, under the
    /// dispatcher that span belongs to: an id means nothing to another
    /// subscriber. Without a span, `event` runs under the current dispatcher
    /// with no explicit parent.
    #[cfg_attr(
        not(feature = "diagnostics"),
        expect(
            clippy::unused_self,
            reason = "only a `diagnostics` subject carries a span"
        )
    )]
    pub(crate) fn emit(&self, event: impl FnOnce(Option<tracing::Id>)) {
        #[cfg(feature = "diagnostics")]
        if let Some(span) = &self.span {
            let mut event = Some(event);
            span.with_subscriber(|(id, dispatch)| {
                if let Some(event) = event.take() {
                    tracing::dispatcher::with_default(dispatch, || event(Some(id.clone())));
                }
            });
            if let Some(event) = event {
                event(None);
            }
            return;
        }
        event(None);
    }

    pub(crate) fn label(&self) -> Option<&str> {
        self.origin.label.as_deref()
    }

    pub(crate) fn created_at(&self) -> Option<&'static std::panic::Location<'static>> {
        self.origin.site.location()
    }
}

/// Emit one refusal event at `$level`, parented to the subject's span when
/// it has one.
macro_rules! refusal_event {
    ($level:ident, $subject:expr, $operation:expr, $error:expr, $message:literal) => {{
        let subject: Option<&Subject<'_>> = $subject;
        let error: &AnimationError = $error;
        let label = subject.and_then(Subject::label);
        let created_at = subject
            .and_then(Subject::created_at)
            .map(tracing::field::display);
        let event = |parent: Option<tracing::Id>| match parent {
            Some(parent) => tracing::$level!(
                target: REFUSAL_TARGET,
                parent: parent,
                operation = $operation,
                kind = error.kind(),
                error = %error,
                label,
                created_at,
                $message,
            ),
            None => tracing::$level!(
                target: REFUSAL_TARGET,
                operation = $operation,
                kind = error.kind(),
                error = %error,
                label,
                created_at,
                $message,
            ),
        };
        match subject {
            Some(subject) => subject.emit(event),
            None => event(None),
        }
    }};
}

/// Report a refusal of `operation` at `warn` and pass `result` on.
///
/// Call once the command has released every controller borrow: a
/// subscriber is user code and can reenter the controller.
pub(crate) fn report<T>(
    operation: &'static str,
    subject: Option<&Subject<'_>>,
    result: Result<T, AnimationError>,
) -> Result<T, AnimationError> {
    if let Err(error) = &result {
        refusal_event!(warn, subject, operation, error, "animation command refused");
    }
    result
}

/// Report a teardown command that changed nothing, such as `stop` on a
/// disposed controller, at `debug`: tearing down twice is ordinary, not misuse.
pub(crate) fn report_teardown<T>(
    operation: &'static str,
    subject: Option<&Subject<'_>>,
    result: Result<T, AnimationError>,
) -> Result<T, AnimationError> {
    if let Err(error) = &result {
        refusal_event!(
            debug,
            subject,
            operation,
            error,
            "animation teardown had nothing to do"
        );
    }
    result
}
