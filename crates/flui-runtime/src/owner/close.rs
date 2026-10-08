use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::PresentationAddress;

use super::{OwnerCore, OwnerEffects, Residence, finish_retirement};
use crate::ui_runtime::UiRuntime;

impl OwnerCore {
    pub(super) fn close_presentation(
        &self,
        runtime: &mut UiRuntime,
        address: PresentationAddress,
        effects: &dyn OwnerEffects,
    ) {
        let surviving =
            runtime
                .primary_id_excluding(address.presentation_id)
                .map(|presentation_id| PresentationAddress {
                    ui_runtime_id: address.ui_runtime_id,
                    presentation_id,
                });
        {
            let mut state = self.state.borrow_mut();
            let index = state
                .presentation_index(address)
                .expect("BUG: close owns an admitted presentation lease");
            let entry = &mut state.runtimes[index];
            entry
                .presentations
                .retain(|id| *id != address.presentation_id);
            entry.closing.retain(|id| *id != address.presentation_id);
            if surviving.is_none() {
                entry.residence = Residence::RetiringCheckedOut;
            }
        }
        // Withdraw native routing before terminal observers. A caught native
        // failure remains authoritative throughout logical close and retention.
        let failure = catch_unwind(AssertUnwindSafe(|| {
            effects.retire_presentation(address, surviving);
        }))
        .err();
        let failure = catch_unwind(AssertUnwindSafe(|| {
            if surviving.is_some() {
                runtime.close_presentation_after_failure(address.presentation_id, failure);
            } else {
                runtime.stop_presentations_after_failure(failure);
            }
        }))
        .err();
        finish_retirement(failure);
    }
}
