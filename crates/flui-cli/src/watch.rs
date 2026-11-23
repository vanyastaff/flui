//! Debounced source-file watcher behind `flui run`.
//!
//! Wraps `notify-debouncer-mini` with a small, channel-based API. This is
//! dev-machine code: the app being reloaded never watches files, so the
//! watcher lives in the CLI rather than in `flui-hot-reload` (whose runtime
//! half is what the app links).

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::Duration,
};

use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind, Debouncer, new_debouncer};

/// How long the watcher waits after a change before it reports the batch.
///
/// An editor's save is a burst of events within milliseconds (a temporary
/// file, a rename, a metadata write), so a short window still reports one
/// save once; every millisecond of it is added to each edit's rebuild, which
/// is why it is not longer.
const SOURCE_DEBOUNCE: Duration = Duration::from_millis(80);

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
/// On change, returns the affected paths through [`recv`](Self::recv) or
/// [`recv_timeout`](Self::recv_timeout). This is layer 1 of the hot-reload stack;
/// callers are responsible for running `cargo build` and/or restarting processes.
pub(crate) struct SourceWatcher {
    rx: Receiver<Result<Vec<DebouncedEvent>, notify_debouncer_mini::notify::Error>>,
    debouncer: Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>,
}

impl SourceWatcher {
    /// Create a watcher that reports a save once it is [`SOURCE_DEBOUNCE`] old.
    pub(crate) fn try_new() -> Result<Self, WatchError> {
        let (tx, rx) = mpsc::channel();
        let debouncer = new_debouncer(SOURCE_DEBOUNCE, tx).map_err(WatchError::create)?;
        Ok(Self { rx, debouncer })
    }

    /// Watch a path for changes.
    pub(crate) fn watch(
        &mut self,
        path: impl AsRef<Path>,
        recursive: bool,
    ) -> Result<(), WatchError> {
        let path = path.as_ref();
        let mode = if recursive {
            notify_debouncer_mini::notify::RecursiveMode::Recursive
        } else {
            notify_debouncer_mini::notify::RecursiveMode::NonRecursive
        };

        self.debouncer
            .watcher()
            .watch(path, mode)
            .map_err(|e| WatchError::watch(path, e))
    }

    /// Wait up to `timeout` for changed paths.
    pub(crate) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Option<Vec<PathBuf>>, RecvTimeoutError> {
        match self.rx.recv_timeout(timeout) {
            Ok(Ok(events)) => Ok(Self::paths_from_events(&events)),
            Ok(Err(errors)) => {
                let _ = crate::ui::warning(format!("source watch errors: {errors:?}"));
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    fn paths_from_events(events: &[DebouncedEvent]) -> Option<Vec<PathBuf>> {
        let paths: Vec<PathBuf> = events
            .iter()
            .filter(|event| event.kind == DebouncedEventKind::Any)
            .map(|event| event.path.clone())
            .collect();

        if paths.is_empty() { None } else { Some(paths) }
    }
}

impl std::fmt::Debug for SourceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceWatcher").finish_non_exhaustive()
    }
}
