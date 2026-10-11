//! Run admission and typed value-space refusals.

use std::cell::RefMut;

use super::{AnimationController, AnimationControllerInner};
use crate::{AnimationError, AnimationInput};

impl AnimationController {
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

    /// Built-in drivers can discard a refusal, so keep its operation and cause
    /// observable. Release the borrow before a diagnostic subscriber reenters.
    pub(super) fn warn_refused_input(
        inner: RefMut<'_, AnimationControllerInner>,
        operation: &str,
        error: AnimationError,
    ) -> AnimationError {
        drop(inner);
        tracing::warn!("{operation}: {error}");
        error
    }
}
