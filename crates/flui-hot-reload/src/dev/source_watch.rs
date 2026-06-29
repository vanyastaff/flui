//! Debounced source-file watcher for dev-time build orchestration.
//!
//! Wraps `notify-debouncer-mini` with a small, channel-based API shared by
//! `flui-cli` and `flui-devtools`.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::Duration,
};

use notify_debouncer_mini::{DebouncedEvent, DebouncedEventKind, Debouncer, new_debouncer};

use crate::strategy::timing;

/// Error creating or configuring a [`SourceWatcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchError {
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
pub struct SourceWatcher {
    rx: Receiver<Result<Vec<DebouncedEvent>, notify_debouncer_mini::notify::Error>>,
    debouncer: Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>,
}

impl SourceWatcher {
    /// Create a watcher with the default desktop debounce interval.
    pub fn new() -> Result<Self, WatchError> {
        Self::with_debounce(timing::SOURCE_DEBOUNCE)
    }

    /// Create a watcher with a custom debounce interval.
    pub fn with_debounce(debounce: Duration) -> Result<Self, WatchError> {
        let (tx, rx) = mpsc::channel();
        let debouncer = new_debouncer(debounce, tx).map_err(WatchError::create)?;
        Ok(Self { rx, debouncer })
    }

    /// Watch a path for changes.
    pub fn watch(&mut self, path: impl AsRef<Path>, recursive: bool) -> Result<(), WatchError> {
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

    /// Block until the next batch of changed paths is available.
    ///
    /// Returns `None` when the watcher channel is closed.
    pub fn recv(&self) -> Option<Vec<PathBuf>> {
        loop {
            match self.rx.recv() {
                Ok(Ok(events)) => {
                    if let Some(paths) = Self::paths_from_events(&events) {
                        return Some(paths);
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!("source watch error: {error:?}");
                }
                Err(_) => return None,
            }
        }
    }

    /// Wait up to `timeout` for changed paths.
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Option<Vec<PathBuf>>, RecvTimeoutError> {
        match self.rx.recv_timeout(timeout) {
            Ok(Ok(events)) => Ok(Self::paths_from_events(&events)),
            Ok(Err(errors)) => {
                tracing::warn!("source watch errors: {errors:?}");
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

impl Default for SourceWatcher {
    fn default() -> Self {
        Self::new().expect("default source watcher should initialize")
    }
}

impl std::fmt::Debug for SourceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceWatcher").finish_non_exhaustive()
    }
}
