//! The Core Haptics engine's state as one atomic word, shared by the main
//! thread and the engine's handlers (ADR-0210 §8).
//!
//! Core Haptics reports a stop or a reset on its own queue while the main
//! thread may be restarting the engine. Each stop is a new restart obligation,
//! so the word carries an epoch beside the state: a restart answers the stop
//! it was started for, and a completion that arrives after a newer stop leaves
//! the engine owed a restart instead of reporting it running.

use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
mod tests;

/// Where the engine is.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Engine {
    /// Not created yet: the first pattern creates and starts it.
    NotCreated = 0,
    /// Started; patterns play.
    Running = 1,
    /// Stopped or reset, or a start failed: owed a restart.
    Restarting = 2,
    /// No Core Haptics on this hardware, or the engine could not be created.
    Unsupported = 3,
}

impl Engine {
    const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::NotCreated,
            1 => Self::Running,
            2 => Self::Restarting,
            _ => Self::Unsupported,
        }
    }
}

/// A restart started for the stop of one epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartTicket {
    epoch: u64,
}

/// The engine's state and the epoch of the last stop, in one word: the state
/// in the low byte, the epoch above it.
#[derive(Debug)]
pub(crate) struct EngineState(AtomicU64);

const STATE_BITS: u32 = 8;
const STATE_MASK: u64 = (1 << STATE_BITS) - 1;
/// The epoch is 56 bits: it wraps after 2^56 stops, which no engine lives to
/// see, and a wrapped epoch only matters if a restart is in flight across
/// exactly that many stops.
const EPOCH_MASK: u64 = u64::MAX >> STATE_BITS;

const fn pack(epoch: u64, engine: Engine) -> u64 {
    ((epoch & EPOCH_MASK) << STATE_BITS) | engine as u64
}

const fn epoch_of(word: u64) -> u64 {
    word >> STATE_BITS
}

const fn engine_of(word: u64) -> Engine {
    // The mask keeps the low byte, so the cast is lossless.
    Engine::from_bits((word & STATE_MASK) as u8)
}

impl EngineState {
    /// An engine not created yet.
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(pack(0, Engine::NotCreated)))
    }

    /// Where the engine is now.
    pub(crate) fn get(&self) -> Engine {
        engine_of(self.0.load(Ordering::Acquire))
    }

    /// Set the state the main thread found while creating the engine; the
    /// epoch is kept.
    pub(crate) fn set(&self, engine: Engine) {
        self.0.update(Ordering::AcqRel, Ordering::Acquire, |word| {
            pack(epoch_of(word), engine)
        });
    }

    /// The engine created on first use reported started. It is running only if
    /// its handlers reported no stop meanwhile; `false` when one did, and the
    /// engine is owed a restart.
    pub(crate) fn started(&self) -> bool {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                (engine_of(word) == Engine::NotCreated)
                    .then(|| pack(epoch_of(word), Engine::Running))
            })
            .is_ok()
    }

    /// The engine stopped, reset or refused a pattern: a new obligation to
    /// restart, which outdates any restart already in flight.
    pub(crate) fn down(&self) {
        self.0.update(Ordering::AcqRel, Ordering::Acquire, |word| {
            pack(epoch_of(word).wrapping_add(1), Engine::Restarting)
        });
    }

    /// The restart to start now, while one is owed.
    pub(crate) fn restart_ticket(&self) -> Option<RestartTicket> {
        let word = self.0.load(Ordering::Acquire);
        (engine_of(word) == Engine::Restarting).then(|| RestartTicket {
            epoch: epoch_of(word),
        })
    }

    /// The restart of `ticket` reported the engine started. It is running
    /// only if no stop came after the restart began; `false` when one did,
    /// and the engine is still owed a restart.
    pub(crate) fn restarted(&self, ticket: RestartTicket) -> bool {
        self.0
            .compare_exchange(
                pack(ticket.epoch, Engine::Restarting),
                pack(ticket.epoch, Engine::Running),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}
