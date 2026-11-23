//! Debounced source-file watcher behind `flui run`.
//!
//! Wraps a `notify` watcher with a small, channel-based API. This is
//! dev-machine code: the app being reloaded never watches files, so the
//! watcher lives in the CLI rather than in `flui-hot-reload` (whose runtime
//! half is what the app links).

use std::{
    ffi::OsString,
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
    /// The paths as [`watch`](Self::watch) was given them, reported whole
    /// when the backend lost events and asks for a rescan.
    watched: Vec<PathBuf>,
    /// The files watched through their directory: `(directory, file name)`,
    /// the directory canonical. The other entries of that directory are not
    /// changes.
    files: Vec<(PathBuf, OsString)>,
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
        Ok(Self {
            rx,
            watcher,
            watched: Vec::new(),
            files: Vec::new(),
        })
    }

    /// Watch a path for changes.
    ///
    /// A file (a non-recursive path that is not a directory) is watched
    /// through its directory: a watch on the file itself sits on its inode,
    /// which an editor that saves by renaming a new file over it replaces, so
    /// every later save would go unseen.
    pub(crate) fn watch(
        &mut self,
        path: impl AsRef<Path>,
        recursive: bool,
    ) -> Result<(), WatchError> {
        let path = path.as_ref();
        let (target, mode) = if recursive {
            (path.to_path_buf(), RecursiveMode::Recursive)
        } else if let (false, Some(name)) = (path.is_dir(), path.file_name()) {
            let directory = std::fs::canonicalize(path)
                .ok()
                .and_then(|file| file.parent().map(Path::to_path_buf))
                .ok_or_else(|| WatchError::watch(path, "its directory cannot be resolved"))?;
            self.files.push((directory.clone(), name.to_owned()));
            (directory, RecursiveMode::NonRecursive)
        } else {
            (path.to_path_buf(), RecursiveMode::NonRecursive)
        };
        self.watcher
            .watch(&target, mode)
            .map_err(|e| WatchError::watch(path, e))?;
        self.watched.push(path.to_path_buf());
        Ok(())
    }

    /// Wait up to `timeout` for a change, then until no change has come for
    /// [`SOURCE_DEBOUNCE`], and return the paths that changed.
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
            self.collect(event, &mut paths);
        }
        let burst_ends = Instant::now() + SOURCE_BURST_LIMIT;
        let mut quiet_from = Instant::now();
        loop {
            let quiet_ends = (quiet_from + SOURCE_DEBOUNCE).min(burst_ends);
            let left = quiet_ends.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.rx.recv_timeout(left) {
                Ok(event) => {
                    if self.collect(event, &mut paths) {
                        quiet_from = Instant::now();
                    }
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }

    /// Add the paths `event` changed to `paths`, or report its error, and
    /// say whether it was a change.
    fn collect(&self, event: notify::Result<Event>, paths: &mut Vec<PathBuf>) -> bool {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                let _ = crate::ui::warning(format!("source watch error: {error}"));
                return false;
            }
        };
        if event.need_rescan() {
            // Events were lost (a full inotify queue): any watched path may
            // have changed.
            paths.extend(self.watched.iter().cloned());
            return true;
        }
        if matches!(event.kind, EventKind::Access(_)) {
            return false;
        }
        let before = paths.len();
        paths.extend(event.paths.into_iter().filter(|path| self.is_watched(path)));
        paths.len() > before
    }

    /// Whether `path` is a change to what was watched: not a sibling of a
    /// file watched through its directory.
    fn is_watched(&self, path: &Path) -> bool {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return true;
        };
        let mut siblings = self.files.iter().filter(|(directory, _)| {
            parent == directory
                || std::fs::canonicalize(parent).is_ok_and(|parent| parent == *directory)
        });
        match siblings.next() {
            None => true,
            Some(first) => std::iter::once(first)
                .chain(siblings)
                .any(|(_, file)| file == name),
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
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    use super::SourceWatcher;

    /// The changes `watcher` reports within `within`, one entry per change.
    fn changes(watcher: &SourceWatcher, within: Duration) -> Vec<Vec<PathBuf>> {
        let deadline = Instant::now() + within;
        let mut changes = Vec::new();
        while Instant::now() < deadline {
            match watcher.recv_timeout(Duration::from_millis(100)) {
                Ok(paths) => changes.push(paths),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        changes
    }

    fn canonical(dir: &Path) -> PathBuf {
        std::fs::canonicalize(dir).expect("the directory's path")
    }

    /// Two saves reported while the caller was busy (a rebuild) come back as
    /// one change naming both files.
    #[test]
    fn the_batches_waiting_for_a_busy_caller_are_one_change() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = canonical(dir.path());
        let mut watcher = SourceWatcher::try_new().expect("a watcher");
        watcher.watch(&root, true).expect("the directory watched");
        let first = root.join("first.rs");
        let second = root.join("second.rs");
        std::fs::write(&first, "fn a() {}\n").expect("the first save");
        // Longer than the quiet window: two changes, had the caller asked.
        std::thread::sleep(Duration::from_millis(400));
        std::fs::write(&second, "fn b() {}\n").expect("the second save");
        std::thread::sleep(Duration::from_millis(400));

        let seen = changes(&watcher, Duration::from_secs(2));
        assert_eq!(seen.len(), 1, "one change: {seen:?}");
        assert!(
            seen[0].contains(&first) && seen[0].contains(&second),
            "both saves in it: {seen:?}"
        );
    }

    /// A build reads every source and the manifest: those reads are no
    /// change, or each build would start the next.
    #[test]
    fn reading_the_sources_is_no_change() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = canonical(dir.path());
        std::fs::create_dir(root.join("src")).expect("a source directory");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("a source");
        std::fs::write(root.join("Cargo.toml"), "[package]\n").expect("a manifest");
        let mut watcher = SourceWatcher::try_new().expect("a watcher");
        watcher
            .watch(root.join("src"), true)
            .expect("the sources watched");
        watcher
            .watch(root.join("Cargo.toml"), false)
            .expect("the manifest watched");

        for _ in 0..3 {
            std::fs::read(root.join("src/main.rs")).expect("a source read");
            std::fs::read(root.join("Cargo.toml")).expect("the manifest read");
        }
        assert_eq!(
            changes(&watcher, Duration::from_millis(500)),
            Vec::<Vec<PathBuf>>::new()
        );
    }

    /// The manifest is watched through its directory: a save that renames a
    /// new file over it is seen, and so is the next one, while the build's
    /// own writes beside it (`Cargo.lock`, `target/`) are not changes.
    #[test]
    fn a_watched_file_saved_by_rename_stays_watched() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = canonical(dir.path());
        let manifest = root.join("Cargo.toml");
        std::fs::write(&manifest, "[package]\n").expect("a manifest");
        let mut watcher = SourceWatcher::try_new().expect("a watcher");
        watcher
            .watch(&manifest, false)
            .expect("the manifest watched");

        for save in ["[package]\nname = \"a\"\n", "[package]\nname = \"b\"\n"] {
            let staged = root.join(".Cargo.toml.swp");
            std::fs::write(&staged, save).expect("the editor's new file");
            std::fs::rename(&staged, &manifest).expect("renamed over the manifest");
            let seen = changes(&watcher, Duration::from_millis(600));
            assert!(
                seen.iter().flatten().any(|path| path == &manifest),
                "the save is a change: {seen:?}"
            );
            assert!(
                seen.iter().flatten().all(|path| path == &manifest),
                "only the manifest: {seen:?}"
            );
        }
        std::fs::write(root.join("Cargo.lock"), "version = 4\n").expect("the lock file");
        std::fs::create_dir(root.join("target")).expect("the build directory");
        assert_eq!(
            changes(&watcher, Duration::from_millis(500)),
            Vec::<Vec<PathBuf>>::new()
        );
    }
}
