//! How the owner thread blocks while it waits for a thread of the
//! application's own (ADR-0305 §9).

use std::time::Duration;

/// How the thread that owns the windows waits for another thread of the
/// application, where the platform asks that thread to keep serving while it
/// waits.
///
/// Win32 is the one such platform: a DXGI present or resize on a raster
/// thread can send a message to the thread its window was made on, and a
/// window thread that waits on the raster thread without serving sent
/// messages deadlocks with it (Microsoft's "DXGI: multithread
/// considerations"). Its wait serves sent messages only, so no input,
/// timer or posted message runs on the owner while it waits. Every other
/// platform has none, and a plain wait on a condition variable does.
///
/// The waiting thread is the one that calls [`wait`](Self::wait); the other
/// thread calls [`notify`](Self::notify).
pub trait OwnerWait: Send + Sync + 'static {
    /// End a wait under way, or the next one when none is: a notification
    /// before the wait is not lost.
    fn notify(&self);

    /// Block the calling thread until notified, or for at most `timeout`,
    /// serving what the platform asks of it meanwhile. It may return early;
    /// the caller reads what it waits for again.
    fn wait(&self, timeout: Duration);
}
