//! Run admission and typed value-space refusals.

use super::{AnimationController, AnimationControllerInner};
use crate::refusal::{self, Subject};
use crate::{AnimationError, AnimationInput};

impl AnimationController {
    /// What this controller's diagnostics name it by. Reads the state only
    /// to find the controller span, and never while a command holds it.
    pub(crate) fn subject(&self) -> Subject<'_> {
        #[cfg(feature = "diagnostics")]
        let parent = self
            .inner
            .try_borrow()
            .ok()
            .and_then(|inner| inner.diagnostics.span_id());
        #[cfg(not(feature = "diagnostics"))]
        let parent = None;
        Subject {
            origin: &self.origin,
            parent,
        }
    }

    /// [`refusal::report`] naming this controller.
    pub(crate) fn reported<T>(
        &self,
        operation: &'static str,
        result: Result<T, AnimationError>,
    ) -> Result<T, AnimationError> {
        refusal::report(operation, Some(&self.subject()), result)
    }

    /// [`refusal::report_teardown`] naming this controller.
    pub(crate) fn reported_teardown<T>(
        &self,
        operation: &'static str,
        result: Result<T, AnimationError>,
    ) -> Result<T, AnimationError> {
        refusal::report_teardown(operation, Some(&self.subject()), result)
    }

    /// Direct assignment canonicalizes bounded non-finite values and ignores
    /// them when unbounded. A validated range always has two finite endpoints;
    /// an unbounded configuration has two infinite endpoints.
    pub(super) fn canonicalize_direct_value(raw: f64, lower: f64, upper: f64) -> Option<f64> {
        if !raw.is_finite() && !lower.is_finite() {
            return None;
        }
        Some(if raw.is_nan() { lower } else { raw }.clamp(lower, upper))
    }

    pub(super) fn check_disposed(inner: &AnimationControllerInner) -> Result<(), AnimationError> {
        if inner.disposed {
            Err(AnimationError::Disposed)
        } else {
            Ok(())
        }
    }

    pub(super) fn check_run_admission(
        inner: &AnimationControllerInner,
    ) -> Result<(), AnimationError> {
        Self::check_disposed(inner)?;
        if inner.run_generation == u64::MAX || inner.sample_epoch == u64::MAX {
            Err(AnimationError::IdentityExhausted)
        } else {
            Ok(())
        }
    }

    /// NaN refuses; infinities clamp only toward an existing finite bound.
    /// Admission carries the input kind and original value, rather than
    /// requiring the caller to parse a diagnostic message.
    pub(super) fn canonicalize_value_target(
        input: AnimationInput,
        raw: f64,
        lower_bound: f64,
        upper_bound: f64,
    ) -> Result<f64, AnimationError> {
        if raw.is_nan() {
            return Err(AnimationError::NonFiniteInput { input, value: raw });
        }
        let clamped = raw.clamp(lower_bound, upper_bound);
        if !clamped.is_finite() {
            return Err(AnimationError::UnboundedTarget { input, value: raw });
        }
        Ok(clamped)
    }
}
