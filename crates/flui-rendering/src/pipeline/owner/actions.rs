//! Owner-local accessibility action resolution.
//!
//! Platform input is committed only while the pipeline is [`Idle`]. This
//! module intentionally returns a detached invocation instead of calling user
//! code under the pipeline owner's outer lock.

use flui_semantics::{SemanticsActionError, SemanticsActionInvocation, SemanticsActionRequest};

use super::PipelineOwner;
use crate::pipeline::phase::Idle;

impl PipelineOwner<Idle> {
    /// Resolve an accessibility request against this presentation's current
    /// semantics tree.
    ///
    /// The returned invocation contains a cloned handler. Callers holding an
    /// `RwLock<PipelineOwner>` must release its guard before invoking it.
    pub fn resolve_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<SemanticsActionInvocation, SemanticsActionError> {
        self.semantics_owner
            .as_ref()
            .ok_or(SemanticsActionError::SemanticsUnavailable)?
            .resolve_action(request)
    }
}
