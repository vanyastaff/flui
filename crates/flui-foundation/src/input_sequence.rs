//! Owner-local identities for admitted input and its resulting motion.

use std::{cell::Cell, num::NonZeroU64};

/// Identifies one binding-admitted input sequence, including its ensuing fling.
///
/// Numeric pointer IDs may be reused; this identity is never reissued by its
/// mint. Compare it only within the owning presentation. It is intentionally
/// neither default-constructible nor constructible from a numeric label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputSequenceId(NonZeroU64);

impl InputSequenceId {
    /// The nonzero numeric identity, for presentation-local routing metadata.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// The owner has permanently exhausted its input-sequence identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("input sequence identities exhausted")]
pub struct InputSequenceIdExhausted;

/// Issues identities for one presentation. Moving the owner does not reset it;
/// cloning a mint would duplicate authority, so this type is not `Clone`.
#[derive(Debug)]
pub struct InputSequenceIdMint {
    last: Cell<u64>,
}

impl InputSequenceIdMint {
    /// Creates a fresh presentation-local authority.
    #[must_use]
    pub const fn new() -> Self {
        Self { last: Cell::new(0) }
    }

    /// Issues the next identity before input admission invokes authored code.
    ///
    /// # Errors
    /// Exhaustion is permanent: subsequent calls refuse rather than wrapping
    /// or issuing an identity held by an older sequence.
    pub fn next(&self) -> Result<InputSequenceId, InputSequenceIdExhausted> {
        let next = self
            .last
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(InputSequenceIdExhausted)?;
        self.last.set(next.get());
        Ok(InputSequenceId(next))
    }
}

impl Default for InputSequenceIdMint {
    fn default() -> Self {
        Self::new()
    }
}

// Only the terminal counter state requires private construction; public
// admission and stale-contact behavior are exercised through GestureBinding.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_identity_exhaustion_is_permanent_and_preserves_older_ids() {
        let mint = InputSequenceIdMint {
            last: Cell::new(u64::MAX - 2),
        };
        let earlier = mint.next().expect("penultimate identity");
        let last = mint.next().expect("terminal identity");
        assert!(earlier < last);
        for _ in 0..3 {
            assert_eq!(mint.next(), Err(InputSequenceIdExhausted));
        }
        assert_ne!(earlier, last);
    }
}
