use std::rc::{Rc, Weak};

use flui_foundation::PresentationAddress;

use super::{
    Delivery, DeliveryClaim, OwnerCore, OwnerEffects, OwnerHost, OwnerWork, PreparedInstall,
    PublicationError,
};

/// Initial observations for a newly published presentation. The owner applies
/// these under one runtime lease before reporting installation completion.
#[derive(Debug)]
pub struct InstallInitialization {
    /// The membership just committed by the native adapter.
    pub address: PresentationAddress,
    /// Native observations sampled after callback registration.
    pub observations: Vec<super::WindowObservation>,
    /// Initial runtime lifecycle, when installing a new runtime.
    /// `None` synchronizes presentations with the runtime's existing lifecycle.
    pub lifecycle: Option<flui_scheduler::AppLifecycleState>,
}

/// Result of executing a published presentation's initialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationOutcome {
    /// Every initial observation completed.
    Ready,
    /// Initialization failed, or the membership was withdrawn before checkout.
    Failed,
}

/// One-shot authority to deliver a prepared install's native/logical bundle.
/// The native host owns the bundle until this token is committed or cancelled.
pub struct InstallToken {
    owner: Weak<OwnerCore>,
    address: PresentationAddress,
    authorizer: Option<PresentationAddress>,
}

impl std::fmt::Debug for InstallToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallToken")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl InstallToken {
    pub(super) fn new(
        core: &Rc<OwnerCore>,
        address: PresentationAddress,
        authorizer: Option<PresentationAddress>,
    ) -> Self {
        Self {
            owner: Rc::downgrade(core),
            address,
            authorizer,
        }
    }

    /// Identity of the complete pending bundle in the native host's store.
    #[must_use]
    pub const fn address(&self) -> PresentationAddress {
        self.address
    }
}

/// Admission refusal returns the linear token for retry or native cancellation.
#[derive(Debug)]
pub struct RefusedInstallToken {
    /// Why the token was not admitted.
    pub error: PublicationError,
    /// Still owned by the caller; it has not joined the FIFO.
    pub token: InstallToken,
}

impl PreparedInstall {
    /// Take this proposal's sole delivery token after storing its complete native bundle.
    /// Further calls return `None`; tokens cannot be cloned or fabricated.
    pub fn take_delivery(&mut self) -> Option<InstallToken> {
        self.delivery.take()
    }
}

impl OwnerHost {
    /// Admit a pending native/logical publication to the common owner FIFO.
    /// Execution asks the native adapter to commit while no runtime is checked out.
    ///
    /// # Errors
    /// Returns the token on foreign owner, closed authorizer/host or active publication.
    pub fn queue_install(
        &self,
        token: InstallToken,
        effects: &dyn OwnerEffects,
    ) -> Result<Delivery, RefusedInstallToken> {
        if !token.owner.ptr_eq(&Rc::downgrade(&self.core)) {
            return Err(RefusedInstallToken {
                error: PublicationError::ForeignOwner,
                token,
            });
        }
        let Ok(callback) = self.core.begin_callback(effects) else {
            return Err(RefusedInstallToken {
                error: PublicationError::Busy,
                token,
            });
        };
        let starts = {
            let mut state = self.core.state.borrow_mut();
            let refusal = if state.closed {
                Some(PublicationError::Closed)
            } else {
                token
                    .authorizer
                    .and_then(|address| state.authorizer(address).err())
            };
            if let Some(error) = refusal {
                drop(state);
                return Err(RefusedInstallToken { error, token });
            }
            state.queue.push_back(OwnerWork::CommitInstall(token));
            if state.claim == DeliveryClaim::Idle && state.callback_remaining != 0 {
                state.claim = DeliveryClaim::Completing;
                true
            } else {
                false
            }
        };
        if starts {
            self.core.drive(effects);
        }
        drop(callback);
        Ok(if starts {
            Delivery::Driven
        } else {
            Delivery::Queued
        })
    }
}
