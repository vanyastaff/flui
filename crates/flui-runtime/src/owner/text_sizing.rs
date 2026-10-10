//! Detached numeric preparation delivered after the runtime checkout returns.

use std::rc::{Rc, Weak};

use flui_foundation::{PresentationAddress, TextSizeRequest};
use flui_painting::TextSizingSource;

use super::{Delivery, DispatchError, OwnerCore, OwnerEffects, RuntimeWork};
use crate::ui_runtime::{TextSizingSettlement, TextSizingWork};

/// One presentation's owned preparation request, without a runtime borrow.
/// Native captures and numeric admission remain with the application host.
pub struct TextSizingFrontier {
    owner: Weak<OwnerCore>,
    address: PresentationAddress,
    work: TextSizingWork,
}

impl std::fmt::Debug for TextSizingFrontier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextSizingFrontier")
            .field("address", &self.address)
            .field("source", self.source())
            .field("requests", &self.requests())
            .finish_non_exhaustive()
    }
}

impl TextSizingFrontier {
    pub(super) fn new(
        owner: &Rc<OwnerCore>,
        runtime: flui_foundation::UiRuntimeId,
        work: TextSizingWork,
    ) -> Self {
        Self {
            owner: Rc::downgrade(owner),
            address: PresentationAddress {
                ui_runtime_id: runtime,
                presentation_id: work.presentation(),
            },
            work,
        }
    }

    /// Exact installed presentation that requested these answers.
    #[must_use]
    pub fn address(&self) -> PresentationAddress {
        self.address
    }

    /// Numeric authority selected by the actual text consumer.
    #[must_use]
    pub fn source(&self) -> &TextSizingSource {
        self.work.source()
    }

    /// Finite unresolved requests from the retained layout attempt.
    #[must_use]
    pub fn requests(&self) -> &[TextSizeRequest] {
        self.work.requests()
    }

    /// Return the preparation result through the same owner FIFO as input.
    /// A ready result requires the host to have admitted the numeric answers.
    ///
    /// # Errors
    /// Refuses an expired or closing presentation, withdrawn host or publication.
    pub fn settle(
        self,
        outcome: TextSizingSettlement,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(
            RuntimeWork::TextSizingSettled(self.address, self.work, outcome),
            effects,
        )
    }
}
