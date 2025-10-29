//! Hot reload support for FLUI applications
//!
//! Watches source files for changes and triggers automatic rebuilds.
//! This enables rapid development iteration without restarting the application.
//!
//! **Note:** This module is only available with the `hot-reload` feature flag.
//!
//! # Example
//!
//! ```rust
//! #[cfg(feature = "hot-reload")]
//! use flui_devtools::hot_reload::HotReloader;
//! use std::path::Path;
//!
//! #[cfg(feature = "hot-reload")]
//! {
//!     let mut reloader = HotReloader::new();
//!
//!     // Watch a directory
//!     reloader.watch("./src").expect("Failed to watch directory");
//!
//!     // Set up change callback
//!     reloader.on_change(|path| {
//!         println!("File changed: {:?}", path);
//!         // Trigger rebuild here
//!     });
//!
//!     // Start watching (blocking)
//!     // reloader.watch_blocking().expect("Watch failed");
//!
//!     // Or watch in background
//!     let handle = reloader.watch_async();
//! }
//! ```

use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher,
};
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Hot reloader for watching file changes
///
/// Monitors specified directories for file changes and triggers callbacks.
/// Useful for implementing hot reload functionality in development.
pub struct HotReloader {
    /// Paths being watched
    watched_paths: Arc<RwLock<Vec<PathBuf>>>,
    /// Callback function for file changes
    on_change_callback: Arc<RwLock<Option<Box<dyn Fn(&Path) + Send + Sync>>>>,
    /// File watcher
    watcher: Arc<RwLock<Option<RecommendedWatcher>>>,
    /// Debounce duration (to avoid triggering multiple times)
    debounce_duration: Duration,
}

impl HotReloader {
    /// Create a new hot reloader
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    ///
    /// let reloader = HotReloader::new();
    /// # }
    /// ```
    pub fn new() -> Self {
        Self {
            watched_paths: Arc::new(RwLock::new(Vec::new())),
            on_change_callback: Arc::new(RwLock::new(None)),
            watcher: Arc::new(RwLock::new(None)),
            debounce_duration: Duration::from_millis(500),
        }
    }

    /// Create a hot reloader with custom debounce duration
    ///
    /// # Arguments
    ///
    /// - `debounce`: How long to wait after a change before triggering the callback
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    /// use std::time::Duration;
    ///
    /// let reloader = HotReloader::with_debounce(Duration::from_millis(1000));
    /// # }
    /// ```
    pub fn with_debounce(debounce: Duration) -> Self {
        Self {
            debounce_duration: debounce,
            ..Self::new()
        }
    }

    /// Watch a directory for changes
    ///
    /// # Arguments
    ///
    /// - `path`: Directory or file to watch
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    ///
    /// let mut reloader = HotReloader::new();
    /// reloader.watch("./src").expect("Failed to watch");
    /// # }
    /// ```
    pub fn watch(&mut self, path: impl AsRef<Path>) -> NotifyResult<()> {
        let path = path.as_ref().to_path_buf();

        // Add to watched paths
        self.watched_paths.write().push(path.clone());

        // If watcher is already initialized, add the path
        if let Some(watcher) = self.watcher.write().as_mut() {
            watcher.watch(&path, RecursiveMode::Recursive)?;
        }

        Ok(())
    }

    /// Set the callback function for file changes
    ///
    /// # Arguments
    ///
    /// - `callback`: Function to call when a file changes
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    ///
    /// let mut reloader = HotReloader::new();
    /// reloader.on_change(|path| {
    ///     println!("File changed: {:?}", path);
    /// });
    /// # }
    /// ```
    pub fn on_change<F>(&mut self, callback: F)
    where
        F: Fn(&Path) + Send + Sync + 'static,
    {
        *self.on_change_callback.write() = Some(Box::new(callback));
    }

    /// Start watching (blocking)
    ///
    /// This will block the current thread and handle file change events.
    /// Use `watch_async()` if you want non-blocking behavior.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    ///
    /// let mut reloader = HotReloader::new();
    /// reloader.watch("./src").expect("Failed to watch");
    /// reloader.on_change(|path| {
    ///     println!("Changed: {:?}", path);
    /// });
    ///
    /// // This blocks forever
    /// reloader.watch_blocking().expect("Watch failed");
    /// # }
    /// ```
    pub fn watch_blocking(&mut self) -> NotifyResult<()> {
        let callback = self.on_change_callback.clone();
        let debounce = self.debounce_duration;
        let last_event_time = Arc::new(RwLock::new(std::time::Instant::now()));

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Filter for modify events
                        if matches!(
                            event.kind,
                            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                        ) {
                            // Debounce
                            let now = std::time::Instant::now();
                            let mut last_time = last_event_time.write();
                            if now.duration_since(*last_time) < debounce {
                                return;
                            }
                            *last_time = now;

                            // Call callback for each path
                            if let Some(ref callback) = *callback.read() {
                                for path in event.paths {
                                    callback(&path);
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Watch error: {:?}", e),
                }
            },
            Config::default(),
        )?;

        // Store watcher
        *self.watcher.write() = Some(watcher);

        // Watch all paths
        let paths = self.watched_paths.read().clone();
        for path in paths {
            if let Some(ref mut watcher) = *self.watcher.write() {
                watcher.watch(&path, RecursiveMode::Recursive)?;
            }
        }

        // Block forever
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// Start watching in background (non-blocking)
    ///
    /// Returns a handle that keeps the watcher alive.
    /// When the handle is dropped, watching stops.
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    ///
    /// let mut reloader = HotReloader::new();
    /// reloader.watch("./src").expect("Failed to watch");
    /// reloader.on_change(|path| {
    ///     println!("Changed: {:?}", path);
    /// });
    ///
    /// let handle = reloader.watch_async();
    ///
    /// // Do other work...
    ///
    /// // Keep handle alive to continue watching
    /// drop(handle);
    /// # }
    /// ```
    pub fn watch_async(&mut self) -> WatchHandle {
        let callback = self.on_change_callback.clone();
        let debounce = self.debounce_duration;
        let last_event_time = Arc::new(RwLock::new(std::time::Instant::now()));

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Filter for modify events
                        if matches!(
                            event.kind,
                            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                        ) {
                            // Debounce
                            let now = std::time::Instant::now();
                            let mut last_time = last_event_time.write();
                            if now.duration_since(*last_time) < debounce {
                                return;
                            }
                            *last_time = now;

                            // Call callback for each path
                            if let Some(ref callback) = *callback.read() {
                                for path in event.paths {
                                    callback(&path);
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Watch error: {:?}", e),
                }
            },
            Config::default(),
        )
        .expect("Failed to create watcher");

        // Watch all paths
        let paths = self.watched_paths.read().clone();
        let mut watcher_guard = self.watcher.write();
        *watcher_guard = Some(watcher);

        for path in paths {
            if let Some(ref mut watcher) = *watcher_guard {
                watcher
                    .watch(&path, RecursiveMode::Recursive)
                    .expect("Failed to watch path");
            }
        }

        drop(watcher_guard);

        WatchHandle {
            _watcher: self.watcher.clone(),
        }
    }

    /// Stop watching all paths
    ///
    /// # Example
    ///
    /// ```rust
    /// # #[cfg(feature = "hot-reload")]
    /// # {
    /// use flui_devtools::hot_reload::HotReloader;
    ///
    /// let mut reloader = HotReloader::new();
    /// reloader.watch("./src").expect("Failed to watch");
    ///
    /// // Later...
    /// reloader.stop();
    /// # }
    /// ```
    pub fn stop(&mut self) {
        *self.watcher.write() = None;
        self.watched_paths.write().clear();
    }

    /// Get the list of watched paths
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.read().clone()
    }

    /// Check if currently watching
    pub fn is_watching(&self) -> bool {
        self.watcher.read().is_some()
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

/// Handle that keeps the watcher alive
///
/// When dropped, the watcher stops watching.
pub struct WatchHandle {
    _watcher: Arc<RwLock<Option<RecommendedWatcher>>>,
}

impl std::fmt::Debug for WatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHandle").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_hot_reloader_creation() {
        let reloader = HotReloader::new();
        assert!(!reloader.is_watching());
        assert_eq!(reloader.watched_paths().len(), 0);
    }

    #[test]
    fn test_watch_path() {
        let mut reloader = HotReloader::new();

        // Create a temp directory
        let temp_dir = std::env::temp_dir().join("flui_test_watch");
        fs::create_dir_all(&temp_dir).ok();

        reloader.watch(&temp_dir).expect("Failed to watch");

        assert_eq!(reloader.watched_paths().len(), 1);
        assert_eq!(reloader.watched_paths()[0], temp_dir);

        // Clean up
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

        // We can't easily test the callback without actually triggering file changes
        // Just verify it was set
        assert!(reloader.on_change_callback.read().is_some());
    }

    #[test]
    fn test_stop() {
        let mut reloader = HotReloader::new();

        let temp_dir = std::env::temp_dir().join("flui_test_stop");
        fs::create_dir_all(&temp_dir).ok();

        reloader.watch(&temp_dir).expect("Failed to watch");
        assert_eq!(reloader.watched_paths().len(), 1);

        reloader.stop();
        assert_eq!(reloader.watched_paths().len(), 0);
        assert!(!reloader.is_watching());

        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_custom_debounce() {
        let reloader = HotReloader::with_debounce(Duration::from_secs(2));
        assert_eq!(reloader.debounce_duration, Duration::from_secs(2));
    }

    #[test]
    fn test_watch_async() {
        let mut reloader = HotReloader::new();

        let temp_dir = std::env::temp_dir().join("flui_test_async");
        fs::create_dir_all(&temp_dir).ok();

        reloader.watch(&temp_dir).expect("Failed to watch");

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        reloader.on_change(move |_path| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let _handle = reloader.watch_async();

        // Give it a moment to start
        std::thread::sleep(Duration::from_millis(100));

        assert!(reloader.is_watching());

        // Clean up
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_multiple_paths() {
        let mut reloader = HotReloader::new();

        let temp_dir1 = std::env::temp_dir().join("flui_test_multi1");
        let temp_dir2 = std::env::temp_dir().join("flui_test_multi2");

        fs::create_dir_all(&temp_dir1).ok();
        fs::create_dir_all(&temp_dir2).ok();

        reloader.watch(&temp_dir1).expect("Failed to watch");
        reloader.watch(&temp_dir2).expect("Failed to watch");

        assert_eq!(reloader.watched_paths().len(), 2);

        fs::remove_dir_all(&temp_dir1).ok();
        fs::remove_dir_all(&temp_dir2).ok();
    }
}
