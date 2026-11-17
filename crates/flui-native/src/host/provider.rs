//! How much of another application's content a host reads at once. It lives
//! beside the table because the owner thread, which names no feature, opens
//! an intent's documents, and the drag feature reads a drop's.

/// The most reads of another application's content provider the Android
/// host runs at once, each on a thread of its own: a dropped URI's bytes, and
/// the workers that open an intent's documents. A provider can hold a read
/// for as long as it likes and ignore its cancellation, so more threads would
/// only be more held; the reads past them wait for one to come back.
#[cfg(any(target_os = "android", test))]
pub(crate) const MAX_PROVIDER_READS: usize = 4;
