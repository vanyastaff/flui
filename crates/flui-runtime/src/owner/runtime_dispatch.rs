use std::cell::Cell;
use std::rc::{Rc, Weak};

use flui_foundation::{PresentationAddress, UiRuntimeId};
use flui_scheduler::AppLifecycleState;

use super::{
    Delivery, DispatchError, OwnerCore, OwnerEffects, OwnerHost, OwnerState, WindowObservation,
};
use crate::ui_runtime::UiRuntime;

/// Work belonging to the runtime independently of any presentation's lifetime.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum RuntimeOperation {
    /// Invalidate text and layout after the host font collection changes.
    FontsChanged,
    /// Drain accepted commands and poll ready async work without producing a frame.
    Background,
    /// Apply the host lifecycle to all presentations in this runtime.
    Lifecycle(AppLifecycleState),
    /// Irreversibly stop every presentation in this runtime without removing it.
    Stop,
    /// Reassemble the runtime's mounted presentations at the requested tier.
    #[cfg(feature = "hot-reload")]
    Reload(crate::reload::ReloadTier),
}

/// Weak runtime authority, independent of the window that originally hosted it.
#[derive(Clone)]
pub struct RuntimeDispatcher {
    owner: Weak<OwnerCore>,
    id: UiRuntimeId,
}

impl std::fmt::Debug for RuntimeDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeDispatcher")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl RuntimeDispatcher {
    /// Admit a runtime operation to the same FIFO as frame deliveries.
    ///
    /// # Errors
    /// Refuses an expired runtime or host, or an active publication.
    pub fn deliver(
        &self,
        operation: RuntimeOperation,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, DispatchError> {
        let core = self.owner.upgrade().ok_or(DispatchError::OwnerGone)?;
        core.deliver(RuntimeWork::Runtime(self.id, operation), effects)
    }
}

impl OwnerHost {
    /// Acquire authority for runtime-wide work without choosing a representative window.
    ///
    /// # Errors
    /// Refuses an absent runtime, a closed host, or an active publication.
    pub fn runtime_dispatcher(&self, id: UiRuntimeId) -> Result<RuntimeDispatcher, DispatchError> {
        self.core
            .state
            .try_borrow()
            .map_err(|_| DispatchError::Busy)?
            .runtime_index(id)?;
        Ok(RuntimeDispatcher {
            owner: Rc::downgrade(&self.core),
            id,
        })
    }
}

pub(super) enum RuntimeWork {
    Initialize(super::InstallInitialization),
    SurfaceRestored(PresentationAddress),
    Close(PresentationAddress),
    Observation(PresentationAddress, WindowObservation),
    WindowState(
        PresentationAddress,
        super::presentation_dispatch::PendingWindowState,
    ),
    Frame(PresentationAddress),
    Runtime(UiRuntimeId, RuntimeOperation),
    Preferences(UiRuntimeId, super::SystemPreferencesSnapshot),
    #[cfg(feature = "test-support")]
    TestCallback(PresentationAddress, Box<dyn FnOnce(&UiRuntime)>),
    Input {
        address: PresentationAddress,
        input: flui_platform_api::PlatformInput,
        reply: Rc<Cell<Option<bool>>>,
    },
}

impl RuntimeWork {
    pub(super) fn merge_pending_state(&mut self, newer: &Self) -> bool {
        if let (Self::WindowState(address, state), Self::WindowState(next_address, next)) =
            (self, newer)
            && address == next_address
        {
            state.merge(next);
            true
        } else {
            false
        }
    }
    pub(super) fn validate_admission(&self, state: &OwnerState) -> Result<usize, DispatchError> {
        match self {
            Self::Initialize(initial) => state.authorizer(initial.address),
            Self::Frame(address)
            | Self::SurfaceRestored(address)
            | Self::Close(address)
            | Self::Observation(address, _)
            | Self::WindowState(address, _)
            | Self::Input { address, .. } => state.authorizer(*address),
            Self::Runtime(id, _) | Self::Preferences(id, _) => state.runtime_index(*id),
            #[cfg(feature = "test-support")]
            Self::TestCallback(address, _) => state.authorizer(*address),
        }
        .map_err(DispatchError::from)
    }

    pub(super) fn validate(&self, state: &OwnerState) -> Result<usize, DispatchError> {
        match self {
            Self::Initialize(initial) => state.presentation_index(initial.address),
            Self::Frame(address)
            | Self::SurfaceRestored(address)
            | Self::Close(address)
            | Self::Observation(address, _)
            | Self::WindowState(address, _)
            | Self::Input { address, .. } => state.presentation_index(*address),
            Self::Runtime(id, _) | Self::Preferences(id, _) => state.runtime_index(*id),
            #[cfg(feature = "test-support")]
            Self::TestCallback(address, _) => state.presentation_index(*address),
        }
        .map_err(DispatchError::from)
    }

    pub(super) fn run(self, core: &OwnerCore, runtime: &mut UiRuntime, effects: &dyn OwnerEffects) {
        match self {
            Self::Initialize(initial) => runtime.enter(|runtime| {
                let preferences = core.state.borrow().preferences.current.clone();
                if let Some(snapshot) = preferences {
                    runtime.apply_preferences(snapshot);
                }
                for observation in initial.observations {
                    observation.apply(runtime, initial.address, effects);
                }
                if let Some(lifecycle) = initial.lifecycle {
                    effects.runtime_lifecycle(initial.address.ui_runtime_id, lifecycle);
                    runtime.update_host_lifecycle(lifecycle);
                } else {
                    runtime.synchronize_window_lifecycle();
                }
            }),
            Self::Preferences(_, snapshot) => {
                runtime.enter(|runtime| runtime.apply_preferences(snapshot));
            }
            #[cfg(feature = "test-support")]
            Self::TestCallback(_, run) => runtime.enter(run),
            Self::SurfaceRestored(address) => {
                runtime.enter(|runtime| runtime.surface_restored(address.presentation_id));
            }
            Self::Close(address) => core.close_presentation(runtime, address, effects),
            Self::Observation(address, observation) => {
                runtime.enter(|runtime| observation.apply(runtime, address, effects));
            }
            Self::WindowState(address, state) => {
                runtime.enter(|runtime| state.apply(runtime, address, effects));
            }
            Self::Frame(address) => effects.frame(address, runtime),
            Self::Input {
                address,
                input,
                reply,
            } => {
                let handled = runtime.enter(|runtime| {
                    runtime.handle_input_addressed(address.presentation_id, input)
                });
                reply.set(Some(handled));
            }
            Self::Runtime(_, RuntimeOperation::FontsChanged) => {
                runtime.enter(UiRuntime::fonts_changed);
            }
            Self::Runtime(_, RuntimeOperation::Background) => {
                runtime.enter(|runtime| {
                    runtime.drain_owner_inbox();
                });
                runtime.pump_background();
            }
            Self::Runtime(id, RuntimeOperation::Lifecycle(state)) => {
                effects.runtime_lifecycle(id, state);
                runtime.enter(|runtime| runtime.update_host_lifecycle(state));
            }
            Self::Runtime(_, RuntimeOperation::Stop) => {
                runtime.enter(UiRuntime::stop_presentations);
            }
            #[cfg(feature = "hot-reload")]
            Self::Runtime(_, RuntimeOperation::Reload(tier)) => {
                runtime.enter(|runtime| runtime.perform_hot_reload_entered(tier));
            }
        }
    }
}
