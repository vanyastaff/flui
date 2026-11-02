//! Bounds configured once for an application run.
use std::num::NonZeroUsize;

/// Admission limits for independent owners and pending application commands.
/// Starting and retiring owners count until their native resources are released.
#[derive(Clone, Copy, Debug)]
pub struct OwnerLimits {
    pub(crate) owners: NonZeroUsize,
    pub(crate) launches: NonZeroUsize,
    pub(crate) commands: NonZeroUsize,
    pub(crate) payload_bytes: NonZeroUsize,
}

impl Default for OwnerLimits {
    fn default() -> Self {
        Self {
            owners: NonZeroUsize::new(8).expect("BUG: nonzero limit"),
            launches: NonZeroUsize::new(16).expect("BUG: nonzero limit"),
            commands: NonZeroUsize::new(256).expect("BUG: nonzero limit"),
            payload_bytes: NonZeroUsize::new(4 * 1024 * 1024).expect("BUG: nonzero limit"),
        }
    }
}

impl OwnerLimits {
    /// Maximum dedicated owners, excluding the initial application owner.
    #[must_use]
    pub fn with_owners(mut self, owners: NonZeroUsize) -> Self {
        self.owners = owners;
        self
    }
    /// Maximum unsettled dedicated launches; abandoning a receipt does not free a slot.
    #[must_use]
    pub fn with_launches(mut self, launches: NonZeroUsize) -> Self {
        self.launches = launches;
        self
    }
    /// Maximum ordinary commands retained by an owner.
    #[must_use]
    pub fn with_commands(mut self, commands: NonZeroUsize) -> Self {
        self.commands = commands;
        self
    }
    /// Maximum retained payload bytes in an owner's ordinary lane.
    #[must_use]
    pub fn with_payload_bytes(mut self, bytes: NonZeroUsize) -> Self {
        self.payload_bytes = bytes;
        self
    }
}
