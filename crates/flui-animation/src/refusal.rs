//! Diagnostics for refused animation commands (ADR-0220).
//!
//! A built-in driver can discard a command's `Result`, so every refusal is
//! also reported: on [`REFUSAL_TARGET`], with the command's `operation`, the
//! error's stable [`kind`](crate::AnimationError::kind) and the error itself.

use crate::AnimationError;

/// The `tracing` target every refused animation command reports on. Like
/// [`AnimationError::kind`], it is a stability boundary: filters select
/// refusals by this name.
pub const REFUSAL_TARGET: &str = "flui_animation::refused";

/// Report a refusal of `operation` at `warn` and pass `result` on.
///
/// Call once the command has released every controller borrow: a
/// subscriber is user code and can reenter the controller.
pub(crate) fn report<T>(
    operation: &'static str,
    result: Result<T, AnimationError>,
) -> Result<T, AnimationError> {
    if let Err(error) = &result {
        tracing::warn!(
            target: REFUSAL_TARGET,
            operation,
            kind = error.kind(),
            error = %error,
            "animation command refused",
        );
    }
    result
}

/// Report a teardown command that changed nothing, such as `stop` on a
/// disposed controller, at `debug`: tearing down twice is ordinary, not misuse.
pub(crate) fn report_teardown<T>(
    operation: &'static str,
    result: Result<T, AnimationError>,
) -> Result<T, AnimationError> {
    if let Err(error) = &result {
        tracing::debug!(
            target: REFUSAL_TARGET,
            operation,
            kind = error.kind(),
            error = %error,
            "animation teardown had nothing to do",
        );
    }
    result
}
