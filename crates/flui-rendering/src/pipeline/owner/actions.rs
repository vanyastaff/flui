//! Owner-local accessibility action resolution.
//!
//! Platform input is committed only while the pipeline is [`Idle`]. This
//! module intentionally returns a detached invocation instead of calling user
//! code under the pipeline owner's outer lock.

use flui_semantics::{
    ActionDecodeError, SemanticsActionError, SemanticsActionInvocation, SemanticsActionRequest,
};

use super::PipelineOwner;
use crate::pipeline::phase::Idle;

impl PipelineOwner<Idle> {
    /// Decode a platform request against this presentation's semantics tree:
    /// a request on a field's text run goes to the field, and a text
    /// selection's run positions become bytes of its value
    /// ([`SemanticsOwner::semantics_action_request_for`](flui_semantics::SemanticsOwner::semantics_action_request_for)).
    /// Without a semantics owner, as the free decoder does.
    ///
    /// # Errors
    ///
    /// The [`ActionDecodeError`] naming what did not fit.
    pub fn decode_semantics_action(
        &self,
        request: &accesskit::ActionRequest,
    ) -> Result<SemanticsActionRequest, ActionDecodeError> {
        match &self.semantics_owner {
            Some(owner) => owner.semantics_action_request_for(request),
            None => flui_semantics::semantics_action_request_for(request),
        }
    }

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
