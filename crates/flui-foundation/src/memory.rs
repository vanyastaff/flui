//! Memory pressure: how much memory the system wants back, and the receivers
//! that give it back (ADR-0237 decision 9).
//!
//! The host reports a [`MemoryPressureLevel`]; the framework asks every cache
//! it owns to [`trim`](MemoryTrim::trim) for that level before any application
//! subscriber hears the notice, so the application finds the easy memory
//! already freed.

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
/// `trim` is synchronous and runs on the thread that owns the receiver, inside
/// an owner turn: it frees, it does not wait. A receiver that lives on another
/// thread (the raster thread's pools) receives a message from a receiver on the
/// owner thread instead.
pub trait MemoryTrim {
    /// Give back memory for `level`. Trimming twice for one level frees
    /// nothing the first did not.
    fn trim(&self, level: MemoryPressureLevel);
}

/// Ask every receiver in `receivers`, in order, to trim for `level`.
///
/// A receiver that panics does not keep the later ones from running. The first
/// panic's payload is returned for the caller to resume once it has finished its
/// own work; later payloads are leaked rather than dropped, since a payload's
/// destructor may panic too.
pub fn trim_all<'a, R>(
    receivers: impl IntoIterator<Item = &'a R>,
    level: MemoryPressureLevel,
) -> Option<Box<dyn std::any::Any + Send>>
where
    R: MemoryTrim + ?Sized + 'a,
{
    let mut first = None;
    for receiver in receivers {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            receiver.trim(level);
        })) {
            if first.is_none() {
                first = Some(payload);
            } else {
                std::mem::forget(payload);
            }
        }
    }
    first
}
