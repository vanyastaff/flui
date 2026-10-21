//! The display state every native backend keeps: the registry that issues its
//! [`DisplayId`]s, the snapshot it last published, and the rules for when that
//! snapshot is read from the OS again.

use flui_platform::{DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot};
use parking_lot::Mutex;

use crate::error::PlatformError;

/// One platform instance's display identities and last published snapshot.
///
/// A backend mints ids for the outputs it sampled, hands the resulting
/// [`DisplayInfo`] list to [`publish`](Self::publish) and returns what that
/// answers from `Platform::display_snapshot`. An unchanged sample answers the
/// previous snapshot with its revision, so a consumer that compares revisions
/// sees no change.
///
/// `display_snapshot` runs on every owner wake, so a backend that has events to
/// learn of a change reads the OS through
/// `snapshot_with` instead: the last snapshot is
/// answered until `invalidate` marks it stale.
pub(crate) struct DisplaySource {
    registry: DisplayRegistry,
    last: Mutex<DisplaySnapshot>,
    /// Set from any thread when the displays may differ from `last`; cleared
    /// before a sample reads the OS, so an invalidation that lands during the
    /// read survives it.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    stale: std::sync::atomic::AtomicBool,
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        test
    ))]
    misses: Mutex<MissLedger>,
}

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32",
    test
))]
impl super::system_notice::StaleOnWake for DisplaySource {
    fn mark_stale(&self) {
        self.invalidate();
    }
}

impl DisplaySource {
    /// Whether the next read samples the operating system again.
    #[cfg(test)]
    pub(crate) fn is_stale(&self) -> bool {
        self.stale.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn new() -> Self {
        Self {
            registry: DisplayRegistry::new(),
            last: Mutex::new(DisplaySnapshot::empty()),
            #[cfg(any(
                windows,
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "android",
                target_arch = "wasm32",
                test
            ))]
            stale: std::sync::atomic::AtomicBool::new(true),
            #[cfg(any(
                windows,
                target_os = "linux",
                target_os = "macos",
                target_os = "ios",
                target_os = "android",
                test
            ))]
            misses: Mutex::new(MissLedger::default()),
        }
    }

    /// The id for `key`, or `None` once the id space is used up: that output is
    /// then left out of the snapshot and its window reports no display.
    pub(crate) fn mint(&self, key: &DisplayKey) -> Option<DisplayId> {
        match self.registry.mint(key) {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::warn!(%error, "an output is left out of the display snapshot");
                None
            }
        }
    }

    /// Publish a fresh sample: the previous snapshot when nothing differs,
    /// otherwise the next revision.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the revision space is used up; the last
    /// snapshot stays published.
    pub(crate) fn publish(
        &self,
        displays: Vec<DisplayInfo>,
    ) -> Result<DisplaySnapshot, PlatformError> {
        let mut last = self.last.lock();
        let next = last
            .successor(displays)
            .map_err(|error| PlatformError::Displays {
                message: error.to_string(),
            })?;
        *last = next.clone();
        Ok(next)
    }

    /// Record that the displays may have changed. One atomic store, so a
    /// window procedure, a notification block or a foreign thread may call it
    /// at any point; the next `snapshot_with` samples
    /// again.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn invalidate(&self) {
        self.stale.store(true, std::sync::atomic::Ordering::Release);
    }

    /// The last published snapshot, without reading the OS.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn published(&self) -> DisplaySnapshot {
        self.last.lock().clone()
    }

    /// The snapshot: the last published one while nothing has invalidated it,
    /// otherwise the result of `sample` published.
    ///
    /// `sample` runs with no lock held, so it may call the OS and mint ids. A
    /// sample that fails, or whose result cannot be published, leaves the
    /// source stale and the last snapshot published.
    ///
    /// # Errors
    /// What `sample` fails with, or [`PlatformError::Displays`] when the
    /// revision space is used up.
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn snapshot_with(
        &self,
        sample: impl FnOnce() -> Result<Vec<DisplayInfo>, PlatformError>,
    ) -> Result<DisplaySnapshot, PlatformError> {
        self.snapshot_with_aux(|| sample().map(|displays| (displays, ())), |()| {})
    }

    /// As `snapshot_with`, for a sample that also yields
    /// data a reader resolves against the snapshot (Windows' monitor handles).
    /// `commit` receives that data only after the snapshot it belongs to is
    /// published, so a reader on another thread never sees data for displays
    /// the published snapshot lacks. A sample that fails or cannot be published
    /// commits nothing.
    ///
    /// # Errors
    /// As `snapshot_with`.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn snapshot_with_aux<T>(
        &self,
        sample: impl FnOnce() -> Result<(Vec<DisplayInfo>, T), PlatformError>,
        commit: impl FnOnce(T),
    ) -> Result<DisplaySnapshot, PlatformError> {
        use std::sync::atomic::Ordering;
        if !self.stale.load(Ordering::Acquire) {
            return Ok(self.published());
        }
        self.stale.store(false, Ordering::Release);
        let published = sample().and_then(|(displays, aux)| {
            let snapshot = self.publish(displays)?;
            commit(aux);
            Ok(snapshot)
        });
        if published.is_err() {
            self.invalidate();
        }
        published
    }

    /// Note that a display the published snapshot lacks was asked for (a window
    /// on a monitor plugged in since the last sample). The first miss of
    /// `handle` in a snapshot revision invalidates the snapshot; the same miss
    /// again, however often it repeats, costs nothing until the revision
    /// changes.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        test
    ))]
    pub(crate) fn note_miss(&self, handle: u64) {
        self.note_miss_with(handle, || {});
    }

    /// As [`note_miss`](Self::note_miss), and `wake` runs when this miss is the
    /// first of its handle in the revision, after the source was invalidated.
    /// A backend whose owner would otherwise sleep until something else wakes
    /// it passes the call that asks for an owner turn, so the sample the miss
    /// owes is not left waiting; a backend with OS change notifications that
    /// already wake the owner uses `note_miss`.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        test
    ))]
    pub(crate) fn note_miss_with(&self, handle: u64, wake: impl FnOnce()) {
        let revision = self.last.lock().revision().get();
        let first = self.misses.lock().first_miss(handle, revision);
        if first {
            self.invalidate();
            wake();
        }
    }
}

/// Remembers which display handles missed in the current snapshot revision, so
/// windows on displays the snapshot omits ask for one fresh sample per handle
/// and revision rather than one per owner wake, however many such displays
/// there are.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
#[derive(Debug, Default)]
struct MissLedger {
    revision: u64,
    missed: Vec<u64>,
}

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
impl MissLedger {
    /// Whether this miss is the first of `handle` in snapshot `revision`.
    fn first_miss(&mut self, handle: u64, revision: u64) -> bool {
        if revision != self.revision {
            self.revision = revision;
            self.missed.clear();
        }
        if self.missed.contains(&handle) {
            return false;
        }
        self.missed.push(handle);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flui_foundation::geometry::Bounds;
    use flui_platform::{DisplayInfo, DisplayKey};

    use super::DisplaySource;
    use crate::error::PlatformError;

    fn one_display(source: &DisplaySource, scale: f64) -> Vec<DisplayInfo> {
        let id = source
            .mint(&DisplayKey::Injected(1))
            .expect("ids are not exhausted");
        vec![DisplayInfo::new(id, "Test", Bounds::default(), scale)]
    }

    /// A source with a counted sample that reads one display.
    fn counted(source: &DisplaySource, samples: &Cell<usize>) {
        source
            .snapshot_with(|| {
                samples.set(samples.get() + 1);
                Ok(one_display(source, 1.0))
            })
            .expect("the sample publishes");
    }

    #[test]
    fn invalidation_during_a_sample_stays_stale() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        source
            .snapshot_with(|| {
                samples.set(samples.get() + 1);
                // The OS changes while the screens are being read.
                source.invalidate();
                Ok(one_display(&source, 1.0))
            })
            .expect("the sample publishes");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the next call reads again");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "and then the snapshot is kept");
    }

    #[test]
    fn a_monitor_resolves_only_once_its_snapshot_is_published() {
        let source = DisplaySource::new();
        let resolvable = std::sync::Mutex::new(Vec::<u32>::new());
        let seen_during_sample = Cell::new(None);
        source
            .snapshot_with_aux(
                || {
                    // A reader on another thread, between the OS read and the
                    // publish, must not find the new monitor.
                    seen_during_sample.set(Some(resolvable.lock().expect("lock").clone()));
                    Ok((one_display(&source, 1.0), vec![42]))
                },
                |monitors| {
                    assert_eq!(
                        source.published().displays().len(),
                        1,
                        "the snapshot is published before its monitors resolve"
                    );
                    *resolvable.lock().expect("lock") = monitors;
                },
            )
            .expect("the sample publishes");
        assert_eq!(seen_during_sample.take(), Some(vec![]));
        assert_eq!(*resolvable.lock().expect("lock"), vec![42]);

        // A sample that cannot be published commits nothing.
        source.invalidate();
        let failed = source.snapshot_with_aux(
            || {
                Err::<(Vec<DisplayInfo>, Vec<u32>), _>(PlatformError::Displays {
                    message: "the OS refused".into(),
                })
            },
            |monitors| *resolvable.lock().expect("lock") = monitors,
        );
        assert!(failed.is_err());
        assert_eq!(*resolvable.lock().expect("lock"), vec![42]);
    }

    #[test]
    fn a_failed_sample_stays_stale() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        source.invalidate();
        let failed = source.snapshot_with(|| {
            Err(PlatformError::Displays {
                message: "the OS refused".into(),
            })
        });
        assert!(failed.is_err());
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the failure left the source stale");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2);
    }

    #[test]
    fn a_missed_display_resamples_once_per_revision() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        source.note_miss(7);
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the first miss asks for a sample");
        // The display is still absent from the identical sample: asking again
        // in the same revision must not keep the source stale.
        for _ in 0..5 {
            source.note_miss(7);
            counted(&source, &samples);
        }
        assert_eq!(samples.get(), 2, "a repeated miss costs nothing");
        // A snapshot that changed is a new revision, so the miss may retry.
        source.invalidate();
        source
            .snapshot_with(|| Ok(one_display(&source, 2.0)))
            .expect("the sample publishes");
        source.note_miss(7);
        counted(&source, &samples);
        assert_eq!(samples.get(), 3, "a new revision allows one retry");
    }

    #[test]
    fn a_first_miss_asks_for_an_owner_turn_and_a_repeat_does_not() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        let wakes = Cell::new(0);
        let miss = |handle| source.note_miss_with(handle, || wakes.set(wakes.get() + 1));
        miss(7);
        assert_eq!(wakes.get(), 1, "an idle owner must be woken to sample");
        miss(7);
        assert_eq!(wakes.get(), 1, "the same miss in the same revision is free");
        miss(8);
        assert_eq!(wakes.get(), 2, "another handle owes its own sample");
        counted(&source, &samples);
        assert_eq!(samples.get(), 2, "the sample the wake is for");
    }

    #[test]
    fn alternating_misses_resample_once_each() {
        let source = DisplaySource::new();
        let samples = Cell::new(0);
        counted(&source, &samples);
        for handle in [7, 8, 7, 8, 7, 8] {
            source.note_miss(handle);
            counted(&source, &samples);
        }
        assert_eq!(samples.get(), 3, "one initial sample and one per handle");
    }
}
