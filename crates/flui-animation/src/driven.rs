//! The owner of a controller's registration on a frame registry.

use crate::animation::{Retirement, Terminal};
use crate::{AnimationController, Vsync, VsyncRegistration, VsyncRegistrationError};
use flui_foundation::panic::RecoveryScope;

enum Seat {
    Bound {
        vsync: Vsync,
        registration: VsyncRegistration,
    },
    Unbound,
    Retired,
}

/// An animation controller together with the registry seat that drives it.
///
/// Clones obtained through `controller()` observe the same controller, while
/// this value owns its lifetime. Dropping it unregisters before cancellation.
#[must_use = "dropping a DrivenController cancels its animation"]
pub struct DrivenController {
    controller: AnimationController,
    seat: Seat,
}

/// Logical closure is committed before this custody invokes or retires code.
pub(crate) struct DrivenRetirement {
    publication: Option<crate::controller::ValuePublication>,
    registry: Terminal<Option<Vsync>>,
}

impl DrivenRetirement {
    pub(crate) fn publish(self, recovery: &mut RecoveryScope<'_>) {
        if let Some(publication) = self.publication {
            publication.publish(recovery);
        }
        recovery.retire(self.registry);
    }
}

impl DrivenController {
    pub(crate) fn new(controller: AnimationController, vsync: Option<&Vsync>) -> Self {
        let mut driven = Self {
            controller,
            seat: Seat::Unbound,
        };
        if let Err(error) = driven.rebind(vsync) {
            tracing::error!(%error, "animation controller has no clock");
        }
        driven
    }

    /// The controller observed and driven by this owner.
    #[must_use]
    pub const fn controller(&self) -> &AnimationController {
        &self.controller
    }

    /// Whether this owner currently holds a registry seat.
    #[must_use]
    pub const fn is_bound(&self) -> bool {
        matches!(self.seat, Seat::Bound { .. })
    }

    /// Move to another registry, preserving the last sampled run elapsed time.
    /// Rebinding to the same registry is a no-op; a retired owner stays retired.
    ///
    /// # Errors
    /// Returns permanent registration exhaustion after releasing the old seat.
    pub fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError> {
        let mut recovery = Retirement::new();
        let (publication, result) = self.prepare_rebind(vsync);
        publication.publish(&mut recovery.scope());
        recovery.finish();
        result
    }

    pub(crate) fn prepare_rebind(
        &mut self,
        vsync: Option<&Vsync>,
    ) -> (DrivenRetirement, Result<(), VsyncRegistrationError>) {
        if matches!(self.seat, Seat::Retired) {
            return (
                DrivenRetirement {
                    publication: None,
                    registry: Terminal::new(None),
                },
                Ok(()),
            );
        }
        if let (Seat::Bound { vsync: old, .. }, Some(new)) = (&self.seat, vsync)
            && old.is_same(new)
        {
            return (
                DrivenRetirement {
                    publication: None,
                    registry: Terminal::new(None),
                },
                Ok(()),
            );
        }
        let admission = vsync.map(|vsync| {
            vsync
                .try_register_resuming(&self.controller, self.controller.last_elapsed())
                .map(|registration| Seat::Bound {
                    vsync: vsync.clone(),
                    registration,
                })
        });
        let (next, result) = match admission {
            Some(Ok(seat)) => (seat, Ok(())),
            Some(Err(error)) => (Seat::Unbound, Err(error)),
            None => (Seat::Unbound, Ok(())),
        };
        let outgoing = std::mem::replace(&mut self.seat, next);
        let outgoing_registry = match outgoing {
            Seat::Bound {
                vsync,
                registration,
            } => {
                vsync.unregister(&registration);
                Some(vsync)
            }
            Seat::Unbound | Seat::Retired => None,
        };
        (
            DrivenRetirement {
                publication: Some(self.controller.prepare_clock_bound(self.is_bound())),
                registry: Terminal::new(outgoing_registry),
            },
            result,
        )
    }

    /// Release the registry seat, then dispose the controller. Idempotent.
    /// Callouts run only after this owner has committed its retired state.
    pub fn dispose(&mut self) {
        let mut recovery = Retirement::new();
        self.dispose_with_recovery(&mut recovery.scope());
        recovery.finish();
    }

    pub(crate) fn dispose_with_recovery(&mut self, recovery: &mut RecoveryScope<'_>) {
        self.prepare_dispose().publish(recovery);
    }

    pub(crate) fn prepare_dispose(&mut self) -> DrivenRetirement {
        let outgoing = std::mem::replace(&mut self.seat, Seat::Retired);
        let outgoing_registry = match outgoing {
            Seat::Bound {
                vsync,
                registration,
            } => {
                // The owning controller remains strong here. Removing its
                // registry clone and weak route cannot retire user captures.
                vsync.unregister(&registration);
                Some(vsync)
            }
            Seat::Unbound | Seat::Retired => None,
        };
        DrivenRetirement {
            publication: self.controller.prepare_dispose(),
            registry: Terminal::new(outgoing_registry),
        }
    }
}

impl AsRef<AnimationController> for DrivenController {
    fn as_ref(&self) -> &AnimationController {
        self.controller()
    }
}

impl Drop for DrivenController {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl std::fmt::Debug for DrivenController {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DrivenController")
            .field("controller", &self.controller)
            .field("bound", &self.is_bound())
            .field("retired", &matches!(self.seat, Seat::Retired))
            .finish()
    }
}
