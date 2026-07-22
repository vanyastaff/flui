//! Hot reload support for FLUI applications.
//!
//! Callback-oriented wrapper around [`flui_hot_reload::dev::SourceWatcher`].
//! For direct channel access, use `SourceWatcher` from `flui_hot_reload::dev`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use flui_hot_reload::dev::{SourceWatcher, WatchError};
use flui_hot_reload::strategy::timing;
use parking_lot::RwLock;

/// Callback invoked with the changed path when a watched file changes.
type OnChangeCallback = Box<dyn Fn(&Path) + Send + Sync>;

/// Hot reloader for watching file changes.
///
/// Monitors directories for changes and invokes a callback. Internally uses
/// [`SourceWatcher`] (layer 1 of the hot-reload stack).
pub struct HotReloader {
    watched_paths: Arc<RwLock<Vec<PathBuf>>>,
    on_change_callback: Arc<RwLock<Option<OnChangeCallback>>>,
    watcher: Arc<RwLock<Option<SourceWatcher>>>,
    debounce_duration: Duration,
    watch_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl HotReloader {
    /// Create a new hot reloader with the default debounce interval.
    pub fn new() -> Self {
        Self::with_debounce(timing::SOURCE_DEBOUNCE)
    }

    /// Create a hot reloader with a custom debounce duration.
    pub fn with_debounce(debounce: Duration) -> Self {
        Self {
            watched_paths: Arc::new(RwLock::new(Vec::new())),
            on_change_callback: Arc::new(RwLock::new(None)),
            watcher: Arc::new(RwLock::new(None)),
            debounce_duration: debounce,
            watch_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Watch a directory or file for changes.
    pub fn watch(&mut self, path: impl AsRef<Path>) -> Result<(), WatchError> {
        let path = path.as_ref().to_path_buf();
        self.watched_paths.write().push(path);
        Ok(())
    }

    /// Set the callback for file changes.
    pub fn on_change<F>(&mut self, callback: F)
    where
        F: Fn(&Path) + Send + Sync + 'static,
    {
        *self.on_change_callback.write() = Some(Box::new(callback));
    }

    /// Start watching (blocking). Runs until the process is interrupted.
    pub fn watch_blocking(&mut self) -> Result<(), WatchError> {
        self.start_watcher()?;
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }

    /// Start watching in a background thread.
    pub fn watch_async(&mut self) -> WatchHandle {
        self.start_watcher()
            .expect("failed to start source watcher");

        WatchHandle {
            _watch_handle: Arc::clone(&self.watch_handle),
        }
    }

    /// Stop watching all paths.
    pub fn stop(&mut self) {
        *self.watch_handle.write() = None;
        *self.watcher.write() = None;
        self.watched_paths.write().clear();
    }

    /// Paths registered via [`watch`](Self::watch).
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.read().clone()
    }

    /// Whether a background watcher thread is active.
    pub fn is_watching(&self) -> bool {
        self.watch_handle.read().is_some()
    }

    fn start_watcher(&mut self) -> Result<(), WatchError> {
        if self.is_watching() {
            return Ok(());
        }

        let mut source = SourceWatcher::with_debounce(self.debounce_duration)?;
        for path in self.watched_paths.read().iter() {
            source.watch(path, true)?;
        }

        let callback = self.on_change_callback.clone();
        let handle = thread::spawn(move || {
            while let Some(paths) = source.recv() {
                if let Some(ref cb) = *callback.read() {
                    for path in paths {
                        cb(path.as_path());
                    }
                }
            }
        });

        *self.watcher.write() = None;
        *self.watch_handle.write() = Some(handle);
        Ok(())
    }
}

impl Default for HotReloader {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HotReloader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotReloader")
            .field("watched_paths", &self.watched_paths.read().len())
            .field("is_watching", &self.is_watching())
            .finish()
    }
}

/// Handle that keeps the background watcher thread alive.
pub struct WatchHandle {
    _watch_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl std::fmt::Debug for WatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHandle").finish()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn test_hot_reloader_creation() {
        let reloader = HotReloader::new();
        assert!(!reloader.is_watching());
        assert_eq!(reloader.watched_paths().len(), 0);
    }

    #[test]
    fn test_watch_path() {
        let mut reloader = HotReloader::new();
        let temp_dir = std::env::temp_dir().join("flui_test_watch");
        fs::create_dir_all(&temp_dir).ok();

        reloader.watch(&temp_dir).expect("Failed to watch");
        assert_eq!(reloader.watched_paths().len(), 1);

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_on_change_callback() {
        let mut reloader = HotReloader::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        reloader.on_change(move |_path| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert!(reloader.on_change_callback.read().is_some());
    }

    #[test]
    fn test_stop() {
        let mut reloader = HotReloader::new();
        let temp_dir = std::env::temp_dir().join("flui_test_stop");
        fs::create_dir_all(&temp_dir).ok();

        reloader.watch(&temp_dir).expect("Failed to watch");
        reloader.stop();
        assert_eq!(reloader.watched_paths().len(), 0);
        assert!(!reloader.is_watching());

        fs::remove_dir_all(&temp_dir).ok();
    }
}
