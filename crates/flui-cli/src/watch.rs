//! Debounced source-file watcher behind `flui run`.
//!
//! Wraps a `notify` watcher with a small, channel-based API. This is
//! dev-machine code: the app being reloaded never watches files, so the
//! watcher lives in the CLI rather than in `flui-hot-reload` (whose runtime
//! half is what the app links).

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// How long the sources must be quiet after a change before it is reported.
///
/// An editor's save is a burst of events within milliseconds (a temporary
/// file, a rename, a metadata write), so a short window still reports one
/// save once; every millisecond of it is added to each edit's rebuild, which
/// is why it is not longer.
const SOURCE_DEBOUNCE: Duration = Duration::from_millis(80);

/// The longest a change waits for the sources to go quiet: a tool that keeps
/// writing is reported once this has passed since its first write.
const SOURCE_BURST_LIMIT: Duration = Duration::from_secs(2);

/// Error creating or configuring a [`SourceWatcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WatchError {
    message: String,
}

impl WatchError {
    fn create(reason: impl std::fmt::Display) -> Self {
        Self {
            message: format!("failed to create watcher: {reason}"),
        }
    }

    fn watch(path: &Path, reason: impl std::fmt::Display) -> Self {
        Self {
            message: format!("failed to watch {}: {reason}", path.display()),
        }
    }
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WatchError {}

/// Debounced watcher for source directories and files.
///
/// On change, returns the affected paths through
/// [`recv_timeout`](Self::recv_timeout). This is layer 1 of the hot-reload
/// stack; callers are responsible for running `cargo build` and/or restarting
/// processes.
pub(crate) struct SourceWatcher {
    rx: Receiver<notify::Result<Event>>,
    watcher: RecommendedWatcher,
}

impl SourceWatcher {
    /// Create a watcher that reports a save once the sources have been quiet
    /// for [`SOURCE_DEBOUNCE`].
    pub(crate) fn try_new() -> Result<Self, WatchError> {
        let (tx, rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            // The receiver is gone only once the watcher is being dropped.
            let _ = tx.send(event);
        })
        .map_err(WatchError::create)?;
        Ok(Self { rx, watcher })
    }

    /// Watch a path for changes.
    pub(crate) fn watch(
        &mut self,
        path: impl AsRef<Path>,
        recursive: bool,
    ) -> Result<(), WatchError> {
        let path = path.as_ref();
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        self.watcher
            .watch(path, mode)
            .map_err(|e| WatchError::watch(path, e))
    }

    /// Wait up to `timeout` for a change, then until the sources have been
    /// quiet for [`SOURCE_DEBOUNCE`], and return the paths that changed.
    ///
    /// Every event already waiting joins the change returned: the saves made
    /// while the caller was busy rebuilding are one change to it, rebuilt
    /// once. An open or a close without a write is no change: a build reads
    /// every source and the manifest, and Linux reports each read, so taking
    /// one for a change would make every build start the next.
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<Vec<PathBuf>, RecvTimeoutError> {
        let mut paths = Vec::new();
        let deadline = Instant::now() + timeout;
        while paths.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self.rx.recv_timeout(left)?;
            Self::collect(event, &mut paths);
        }
        let burst_ends = Instant::now() + SOURCE_BURST_LIMIT;
        loop {
            let quiet = SOURCE_DEBOUNCE.min(burst_ends.saturating_duration_since(Instant::now()));
            match self.rx.recv_timeout(quiet) {
                Ok(event) => Self::collect(event, &mut paths),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
            if Instant::now() >= burst_ends {
                break;
            }
        }
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }

    /// Add the paths `event` changed to `paths`, or report its error.
    fn collect(event: notify::Result<Event>, paths: &mut Vec<PathBuf>) {
        match event {
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
            Ok(event) => paths.extend(event.paths),
            Err(error) => {
                let _ = crate::ui::warning(format!("source watch error: {error}"));
            }
        }
    }
}

impl std::fmt::Debug for SourceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceWatcher").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::SourceWatcher;

    /// Two saves reported while the caller was busy (a rebuild) come back as
    /// one change naming both files.
    #[test]
    fn the_batches_waiting_for_a_busy_caller_are_one_change() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = std::fs::canonicalize(dir.path()).expect("the directory's path");
        let mut watcher = SourceWatcher::try_new().expect("a watcher");
        watcher.watch(&root, true).expect("the directory watched");
        let first = root.join("first.rs");
        let second = root.join("second.rs");
        std::fs::write(&first, "fn a() {}\n").expect("the first save");
        // Long enough for the debouncer to report the first save on its own.
        std::thread::sleep(Duration::from_millis(400));
        std::fs::write(&second, "fn b() {}\n").expect("the second save");
        std::thread::sleep(Duration::from_millis(400));

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while !(seen.contains(&first) && seen.contains(&second)) && Instant::now() < deadline {
            if let Ok(paths) = watcher.recv_timeout(Duration::from_millis(100)) {
                assert!(seen.is_empty(), "one change, not {seen:?} then {paths:?}");
                seen = paths;
            }
        }
        assert!(
            seen.contains(&first) && seen.contains(&second),
            "both saves in one change: {seen:?}"
        );
    }
}
