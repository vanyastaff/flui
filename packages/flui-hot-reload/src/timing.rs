//! How often a host reads its plugin artifact. A reload waits for the next
//! read, so the interval is part of every edit's latency; a read is a few
//! `stat` calls (and the worker's sidecar manifest), and only a development
//! host with a reload hook installed reads at all. Source watching and its
//! debounce are the CLI's (`flui run`), not the host's.

use std::time::Duration;

/// How often [`crate::HotReloadDriver`] and [`crate::WorkerReloadDriver`]
/// read the plugin artifact's stamp at a frame boundary.
pub(crate) const ARTIFACT_POLL: Duration = Duration::from_millis(50);

/// How often the host's background worker-artifact watcher checks for a
/// rebuild. Same scale as [`ARTIFACT_POLL`]; kept separate because that one
/// paces an owner-thread poll at a frame boundary, while this paces a
/// standalone watcher thread that must notice a rebuild while the owner is
/// idle (see [`crate::worker_artifact_stamp`]).
#[cfg(any(test, feature = "host-hook"))]
pub(crate) const WATCHER_POLL: Duration = Duration::from_millis(50);
