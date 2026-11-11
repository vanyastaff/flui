use flui_foundation::{PresentationAddress, panic::FirstPanic};

use super::{OwnerCore, OwnerEffects, Residence};
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
        // The window outlives the presentation, so a high refresh rate it was
        // given is released before the host stops following it.
        let mut failure = FirstPanic::new();
        self.release_frame_rate_demand(effects, address, &mut failure);
        // Withdraw native routing before terminal observers. A caught native
        // failure remains authoritative throughout logical close and retention.
        failure.contain(|| {
            effects.retire_presentation(address, surviving);
        });
        // Terminal observers run contained so close completes.
        let mut observers = FirstPanic::new();
        observers.contain(|| {
            if surviving.is_some() {
                runtime.close_presentation_after_failure(address.presentation_id, failure);
            } else {
                runtime.stop_presentations_after_failure(failure);
            }
        });
        observers.resume();
    }
}
