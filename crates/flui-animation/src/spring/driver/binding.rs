//! Move independent property owners before exposing clock migration to callouts.

use super::{AnimatedValue, Terminal};
use crate::driven::DrivenRetirement;
use crate::{DrivenController, TwoWayConverter, Vsync, VsyncRegistrationError};
use flui_foundation::panic::PanicRecovery;

struct Binding<'owners> {
    owner: &'owners mut DrivenController,
    registry: Terminal<Option<Vsync>>,
}

/// Coordinates registry migration of independently owned property motion.
/// Every owner's seat and clock state commit before the first wake or settle
/// callback. Registration exhaustion releases that owner's old seat and
/// settles it unbound, retaining the individual owner's migration contract.
pub struct VsyncUpdate<'owners> {
    bindings: Vec<Binding<'owners>>,
}

/// Delivery for a committed group of registry migrations.
///
/// No owner borrow survives preparation. Restore temporarily extracted owners
/// to their storage before publishing wakes and unbound settlement. Dropping
/// this value also finishes delivery; during unwind it preserves the incoming
/// failure while completing the accepted tail.
#[must_use = "publish the committed migration after restoring owner storage"]
pub struct VsyncPublication {
    recovery: Option<PanicRecovery>,
    publications: Vec<Terminal<DrivenRetirement>>,
    registries: Vec<Terminal<Option<Vsync>>>,
    result: Result<(), VsyncRegistrationError>,
}

impl std::fmt::Debug for VsyncPublication {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VsyncPublication")
            .finish_non_exhaustive()
    }
}

impl VsyncPublication {
    /// Deliver every committed wake and settlement, preserving the first failure
    /// while completing the accepted tail and retiring outgoing registries.
    ///
    /// # Errors
    /// Returns the first registration refusal after delivering the whole group.
    ///
    /// # Panics
    /// Propagates the first preparation or delivery panic after the accepted
    /// delivery tail finishes. During incoming unwind, drop preserves that failure.
    pub fn publish(mut self) -> Result<(), VsyncRegistrationError> {
        self.deliver();
        self.recovery
            .take()
            .expect("BUG: live migration owns recovery")
            .finish();
        std::mem::replace(&mut self.result, Ok(()))
    }

    fn deliver(&mut self) {
        let recovery = self
            .recovery
            .as_mut()
            .expect("BUG: live migration owns recovery");
        for publication in self.publications.drain(..) {
            recovery.run_with(|scope| publication.into_inner().publish(scope));
        }
        for registry in self.registries.drain(..) {
            recovery.retire(registry);
        }
    }
}

impl Drop for VsyncPublication {
    fn drop(&mut self) {
        if self.recovery.is_none() {
            return;
        }
        if std::thread::panicking() {
            // An intervening owner-storage failure takes precedence over a
            // preparation failure. Its payload must not invoke user Drop.
            let outgoing = self.recovery.replace(PanicRecovery::new());
            drop(outgoing);
        }
        self.deliver();
        self.recovery
            .take()
            .expect("BUG: live migration owns recovery")
            .finish();
    }
}

impl std::fmt::Debug for VsyncUpdate<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VsyncUpdate")
            .finish_non_exhaustive()
    }
}

impl<'owners> VsyncUpdate<'owners> {
    /// Move all staged owners, then deliver their wakes and unbound settles.
    /// Failures in delivery retain the first failure and complete the accepted
    /// tail before propagating. No owner stays on its former registry.
    ///
    /// # Errors
    /// Returns the first registration refusal after processing every owner.
    /// Failed registrations settle unbound; successful ones use their new clock.
    pub fn run(action: impl FnOnce(&mut Self)) -> Result<(), VsyncRegistrationError> {
        Self::prepare(action).publish()
    }

    /// Commit all staged registrations without invoking wake or settlement hooks.
    /// The returned publication owns delivery and retirement without borrowing
    /// the controllers, allowing their owner storage to be restored first.
    pub fn prepare(action: impl FnOnce(&mut Self)) -> VsyncPublication {
        let mut recovery = PanicRecovery::new();
        let mut update = Self {
            bindings: Vec::new(),
        };
        let mut publications = Vec::new();
        let mut result = Ok(());
        recovery.run(|| action(&mut update));
        if !recovery.has_failure() {
            recovery.run(|| {
                for binding in &mut update.bindings {
                    let (publication, admission) = binding
                        .owner
                        .prepare_rebind(binding.registry.get().as_ref());
                    publications.push(Terminal::new(publication));
                    if result.is_ok() {
                        result = admission;
                    }
                }
            });
        }
        VsyncPublication {
            recovery: Some(recovery),
            publications,
            registries: update
                .bindings
                .into_iter()
                .map(|binding| binding.registry)
                .collect(),
            result,
        }
    }

    /// Stage migration, including `None` to release a clock and settle unbound.
    /// Staging does not call hooks or change the owner's registration.
    pub fn rebind<T: TwoWayConverter + 'static>(
        &mut self,
        owner: &'owners mut AnimatedValue<T>,
        registry: Option<&Vsync>,
    ) {
        self.rebind_controller(owner.driven.get_mut(), registry);
    }

    /// Stage a controller owner alongside other motion owners. Staging neither
    /// invokes callbacks nor changes registration; preparation commits the group.
    pub fn rebind_controller(
        &mut self,
        owner: &'owners mut DrivenController,
        registry: Option<&Vsync>,
    ) {
        self.bindings.push(Binding {
            owner,
            registry: Terminal::new(registry.cloned()),
        });
    }
}
