//! Move independent property owners before exposing clock migration to callouts.

use super::{AnimatedValue, Terminal};
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
        for publication in publications {
            recovery.run_with(|scope| publication.into_inner().publish(scope));
        }
        for binding in update.bindings {
            recovery.retire(binding.registry);
        }
        recovery.finish();
        result
    }

    /// Stage migration, including `None` to release a clock and settle unbound.
    /// Staging does not call hooks or change the owner's registration.
    pub fn rebind<T: TwoWayConverter + 'static>(
        &mut self,
        owner: &'owners mut AnimatedValue<T>,
        registry: Option<&Vsync>,
    ) {
        self.bindings.push(Binding {
            owner: owner.driven.get_mut(),
            registry: Terminal::new(registry.cloned()),
        });
    }
}
