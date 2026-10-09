//! The owner of a controller's registration on a frame registry.

use crate::animation::Retirement;
use crate::{AnimationController, Vsync, VsyncRegistration, VsyncRegistrationError};

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
        if matches!(self.seat, Seat::Retired) {
            return Ok(());
        }
        if let (Seat::Bound { vsync: old, .. }, Some(new)) = (&self.seat, vsync)
            && old.is_same(new)
        {
            return Ok(());
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
        let mut retirement = Retirement::new();
        let outgoing_registry = match outgoing {
            Seat::Bound {
                vsync,
                registration,
            } => {
                retirement.run(|| vsync.unregister(&registration));
                Some(vsync)
            }
            Seat::Unbound | Seat::Retired => None,
        };
        retirement.run_with(|retirement| {
            self.controller.set_clock_bound(self.is_bound(), retirement);
        });
        retirement.retire(outgoing_registry);
        retirement.finish();
        result
    }

    /// Release the registry seat, then dispose the controller. Idempotent.
    /// Callouts run only after this owner has committed its retired state.
    pub fn dispose(&mut self) {
        let outgoing = std::mem::replace(&mut self.seat, Seat::Retired);
        if matches!(outgoing, Seat::Retired) {
            return;
        }
        let mut recovery = Retirement::new();
        let outgoing_registry = match outgoing {
            Seat::Bound {
                vsync,
                registration,
            } => {
                recovery.run(|| vsync.unregister(&registration));
                Some(vsync)
            }
            Seat::Unbound | Seat::Retired => None,
        };
        recovery.run_with(|recovery| self.controller.dispose_with_retirement(recovery));
        recovery.retire(outgoing_registry);
        recovery.finish();
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
