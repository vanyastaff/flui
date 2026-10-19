//! Memory pressure: how much memory the system wants back, and the receivers
//! that give it back (ADR-0237 decision 9).
//!
//! The host reports a [`MemoryPressureLevel`]; the framework asks every cache
//! it owns to [`reclaim`](ReclaimMemory::reclaim) for that level before any
//! application subscriber hears the notice, so the application finds the easy
//! memory already freed.

use crate::panic::FirstPanic;

/// How much memory the system wants back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MemoryPressureLevel {
    /// Free what is easy to restore.
    Moderate,
    /// Free everything the current frame does not need.
    Critical,
}

/// A holder of memory it can give back and later rebuild.
///
/// `reclaim` is synchronous and runs on the thread that owns the receiver,
/// inside an owner turn: it frees, it does not wait. A receiver that lives on
/// another thread (the raster thread's pools) is reached through a message sent
/// by a receiver on the owner thread instead.
pub trait ReclaimMemory {
    /// Give back memory for `level`. Reclaiming twice for one level frees
    /// nothing the first did not.
    fn reclaim(&self, level: MemoryPressureLevel);
}

impl<T: ReclaimMemory + ?Sized> ReclaimMemory for std::sync::Arc<T> {
    fn reclaim(&self, level: MemoryPressureLevel) {
        (**self).reclaim(level);
    }
}

/// Ask every receiver in `receivers`, in order, to reclaim for `level`.
///
/// A receiver that panics does not keep the later ones from running; the first
/// panic is held in `failure` for the caller to raise once it has finished its
/// own work.
pub fn reclaim_each<'a, R>(
    receivers: impl IntoIterator<Item = &'a R>,
    level: MemoryPressureLevel,
    failure: &mut FirstPanic,
) where
    R: ReclaimMemory + ?Sized + 'a,
{
    for receiver in receivers {
        failure.contain(|| receiver.reclaim(level));
    }
}
