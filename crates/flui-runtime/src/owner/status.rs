//! Owned observations of host state; no runtime or registry borrow escapes.

use flui_foundation::{PresentationAddress, UiRuntimeId};
use flui_scheduler::SchedulerPhase;

use super::{DispatchError, OwnerHost, Residence, RuntimeEntry};

/// Current scheduling state of one published UI runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuntimeStatus {
    /// Current primary presentation, including while a close is awaiting delivery.
    pub primary: PresentationAddress,
    /// Observed scheduler phase, also available during runtime checkout.
    pub phase: SchedulerPhase,
    /// Whether the runtime's lifecycle currently allows frames.
    pub frames_enabled: bool,
    /// Whether the runtime is leased to an operation now: its own code is
    /// running, so none of its callbacks (subscribers, close handlers) can
    /// run until the lease returns.
    pub checked_out: bool,
}

impl RuntimeEntry {
    fn status(&self) -> RuntimeStatus {
        let scheduler = self
            .scheduler
            .upgrade()
            .expect("BUG: a resident or leased runtime owns its scheduler");
        RuntimeStatus {
            primary: PresentationAddress {
                ui_runtime_id: self.id,
                presentation_id: *self
                    .presentations
                    .first()
                    .expect("BUG: a published runtime has a presentation"),
            },
            phase: scheduler.phase(),
            frames_enabled: scheduler.frames_enabled(),
            checked_out: !matches!(self.residence, Residence::Resident(_)),
        }
    }
}

impl OwnerHost {
    /// Whether a runtime is currently leased to an executing operation.
    /// Registry commits and completion effects run after this becomes false.
    ///
    /// # Errors
    /// A pure publication currently holds the registry.
    pub fn is_executing(&self) -> Result<bool, DispatchError> {
        self.core
            .state
            .try_borrow()
            .map(|state| state.active_runtime.is_some())
            .map_err(|_| DispatchError::Busy)
    }

    /// Snapshot runtime identities in installation order. Closed hosts return none.
    ///
    /// # Errors
    /// A pure publication currently holds the registry.
    pub fn runtime_ids(&self) -> Result<Vec<UiRuntimeId>, DispatchError> {
        let state = self
            .core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?;
        Ok(state
            .runtimes
            .iter()
            .filter(|entry| {
                !state.closed && !matches!(entry.residence, Residence::RetiringCheckedOut)
            })
            .map(|entry| entry.id)
            .collect())
    }

    /// Read scheduling state without borrowing the runtime, including during a frame.
    ///
    /// # Errors
    /// Refuses an absent runtime, closed host, or active publication.
    pub fn runtime_status(&self, id: UiRuntimeId) -> Result<RuntimeStatus, DispatchError> {
        let state = self
            .core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?;
        let index = state.runtime_index(id).map_err(DispatchError::from)?;
        Ok(state.runtimes[index].status())
    }

    /// The refresh interval presentation `address` paces by (its display's
    /// period, or the 60 Hz default while that is unknown), read between owner
    /// turns for the wake-deadline hook that re-arms the desktop pacing fallback.
    ///
    /// # Errors
    /// Refuses an absent presentation or closed host, and `Busy` while the
    /// runtime is leased to an operation or a publication holds the registry:
    /// its presentation is mid-turn and the hook is not consulted then.
    pub fn presentation_refresh_interval(
        &self,
        address: PresentationAddress,
    ) -> Result<std::time::Duration, DispatchError> {
        let state = self
            .core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?;
        let index = state
            .presentation_index(address)
            .map_err(DispatchError::from)?;
        match &state.runtimes[index].residence {
            Residence::Resident(runtime) => Ok(runtime.refresh_interval(address.presentation_id)),
            Residence::CheckedOut | Residence::RetiringCheckedOut => Err(DispatchError::Busy),
        }
    }

    /// Observe the active runtime's phase, otherwise the first frame transaction
    /// phase or first installed runtime's phase. A retiring active lease remains
    /// visible until it returns, even after host shutdown withdraws membership.
    ///
    /// # Errors
    /// A pure publication currently holds the registry.
    pub fn phase(&self) -> Result<Option<SchedulerPhase>, DispatchError> {
        let state = self
            .core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?;
        if let Some(id) = state.active_runtime {
            let entry = state
                .runtimes
                .iter()
                .find(|entry| entry.id == id)
                .expect("BUG: active lease retains its registry entry");
            return Ok(entry
                .scheduler
                .upgrade()
                .expect("BUG: active lease owns its scheduler")
                .phase()
                .into());
        }
        let mut first = None;
        for entry in &state.runtimes {
            if state.closed || matches!(entry.residence, Residence::RetiringCheckedOut) {
                continue;
            }
            let phase = entry.status().phase;
            if matches!(
                phase,
                SchedulerPhase::TransientCallbacks
                    | SchedulerPhase::MidFrameMicrotasks
                    | SchedulerPhase::PersistentCallbacks
            ) {
                return Ok(Some(phase));
            }
            first.get_or_insert(phase);
        }
        Ok(first)
    }

    /// Earliest cached runtime-owned wall-clock deadline after all checkouts return.
    /// Never reports an incomplete minimum by silently skipping an active runtime.
    /// Deadline extension queries and diagnostics run at owner execution frontiers;
    /// this observation never calls foreign code while holding the registry borrow.
    ///
    /// # Errors
    /// Returns `Busy` during runtime execution or a pure publication.
    pub fn next_wake(&self) -> Result<Option<web_time::Instant>, DispatchError> {
        let state = self
            .core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?;
        if state.active_runtime.is_some() {
            return Err(DispatchError::Busy);
        }
        Ok(state
            .runtimes
            .iter()
            .filter_map(|entry| match &entry.residence {
                Residence::Resident(runtime) if !state.closed => runtime.next_wake(),
                _ => None,
            })
            .min())
    }
}
