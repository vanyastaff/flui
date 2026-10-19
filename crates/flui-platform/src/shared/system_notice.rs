//! The one path every backend's system notices take to the host.
//!
//! A backend that hears the operating system say the machine woke, slept, locked
//! or wants memory back hands the notice to [`SystemNotices::dispatch`]. For
//! [`SystemNotice::Wake`] the dispatch first marks everything the platform
//! cached about the world stale: the displays it sampled and each window's
//! published state. Both can have changed while the machine slept without an
//! event reaching the process, and a backend that forgot to mark them would
//! answer from before the sleep until some unrelated event happened to
//! invalidate them. Putting the marking here, in the dispatch, makes a Wake that
//! reaches the host without it impossible.

use std::sync::{Arc, Weak};

use flui_platform_api::SystemNotice;
use parking_lot::Mutex;

/// Something a platform caches about displays or windows, and can mark stale.
pub(crate) trait StaleOnWake: Send + Sync {
    /// Mark the cached value stale: the next read asks the operating system.
    fn mark_stale(&self);
}

/// The callback a platform calls with each system notice.
pub(crate) type SystemNoticeHook = Box<dyn FnMut(SystemNotice) + Send>;

/// A platform's system-notice channel: the host's hook, and what a wake marks
/// stale before the hook hears of it.
#[derive(Default)]
pub(crate) struct SystemNotices {
    hook: Mutex<Option<SystemNoticeHook>>,
    cached: Mutex<Vec<Weak<dyn StaleOnWake>>>,
}

impl SystemNotices {
    /// Install the host's hook, replacing an earlier one.
    pub(crate) fn set_hook(&self, hook: SystemNoticeHook) {
        let replaced = self.hook.lock().replace(hook);
        drop(replaced);
    }

    /// Mark `cache` stale whenever the machine wakes, for as long as it lives.
    pub(crate) fn mark_stale_on_wake<T: StaleOnWake + 'static>(&self, cache: &Arc<T>) {
        let weak = Arc::downgrade(cache);
        let weak: Weak<dyn StaleOnWake> = weak;
        let mut cached = self.cached.lock();
        cached.retain(|entry| entry.strong_count() > 0);
        cached.push(weak);
    }

    /// Deliver `notice` to the host. A wake marks every cache registered with
    /// [`mark_stale_on_wake`](Self::mark_stale_on_wake) first.
    ///
    /// The hook runs with no lock of this channel held, and a notice that
    /// arrives from inside the hook is delivered after it returns the hook to
    /// its slot (the hook is out of the slot while it runs, so a nested notice
    /// finds none and is dropped with a diagnostic rather than recursed into).
    pub(crate) fn dispatch(&self, notice: SystemNotice) {
        if notice == SystemNotice::Wake {
            let live: Vec<Arc<dyn StaleOnWake>> = self
                .cached
                .lock()
                .iter()
                .filter_map(Weak::upgrade)
                .collect();
            for cache in live {
                cache.mark_stale();
            }
        }
        let Some(mut hook) = self.hook.lock().take() else {
            return;
        };
        hook(notice);
        let mut slot = self.hook.lock();
        if slot.is_none() {
            *slot = Some(hook);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::memory::MemoryPressureLevel;

    use super::*;

    struct Cache(AtomicUsize);

    impl StaleOnWake for Cache {
        fn mark_stale(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn marked(cache: &Cache) -> usize {
        cache.0.load(Ordering::SeqCst)
    }

    #[test]
    fn a_wake_marks_the_registered_caches_before_the_host_hears_it() {
        let notices = SystemNotices::default();
        let cache = Arc::new(Cache(AtomicUsize::new(0)));
        notices.mark_stale_on_wake(&cache);
        let seen_by_hook = Arc::new(AtomicUsize::new(usize::MAX));
        let (observed, cache_in_hook) = (Arc::clone(&seen_by_hook), Arc::clone(&cache));
        notices.set_hook(Box::new(move |_| {
            observed.store(marked(&cache_in_hook), Ordering::SeqCst);
        }));

        notices.dispatch(SystemNotice::Wake);

        assert_eq!(marked(&cache), 1);
        assert_eq!(
            seen_by_hook.load(Ordering::SeqCst),
            1,
            "the cache was already stale when the host was told"
        );
    }

    #[test]
    fn other_notices_leave_the_caches_alone() {
        let notices = SystemNotices::default();
        let cache = Arc::new(Cache(AtomicUsize::new(0)));
        notices.mark_stale_on_wake(&cache);
        for notice in [
            SystemNotice::Sleep,
            SystemNotice::SessionLocked,
            SystemNotice::SessionUnlocked,
            SystemNotice::MemoryPressure(MemoryPressureLevel::Critical),
        ] {
            notices.dispatch(notice);
        }
        assert_eq!(marked(&cache), 0);
    }

    /// The platform's own caches, not a stand-in: a wake leaves the display
    /// source and the window's state cell stale, so the next read of either asks
    /// the operating system.
    #[test]
    fn a_wake_leaves_a_real_display_source_and_window_state_cell_stale() {
        use flui_foundation::geometry::Size;
        use flui_platform_api::{WindowGeometry, WindowState};

        use super::super::display_source::DisplaySource;
        use super::super::window_state::WindowStateCell;

        let displays = Arc::new(DisplaySource::new());
        displays
            .snapshot_with(|| Ok(Vec::new()))
            .expect("an empty sample publishes");
        let window = Arc::new(WindowStateCell::new(WindowState::new(
            WindowGeometry::new(None, Size::new(10.0, 10.0)),
            Size::new(10.0, 10.0),
        )));
        let _ = window.state_with(|| {
            WindowState::new(
                WindowGeometry::new(None, Size::new(10.0, 10.0)),
                Size::new(10.0, 10.0),
            )
        });
        assert!(
            !displays.is_stale() && !window.is_stale(),
            "fresh to begin with"
        );

        let notices = SystemNotices::default();
        notices.mark_stale_on_wake(&displays);
        notices.mark_stale_on_wake(&window);
        notices.dispatch(SystemNotice::Sleep);
        assert!(
            !displays.is_stale() && !window.is_stale(),
            "sleep leaves them"
        );

        notices.dispatch(SystemNotice::Wake);
        assert!(displays.is_stale(), "the display source reads the OS again");
        assert!(window.is_stale(), "the window state reads the OS again");
    }

    #[test]
    fn a_dropped_cache_is_forgotten_and_a_wake_without_a_hook_still_marks() {
        let notices = SystemNotices::default();
        let kept = Arc::new(Cache(AtomicUsize::new(0)));
        notices.mark_stale_on_wake(&kept);
        notices.mark_stale_on_wake(&Arc::new(Cache(AtomicUsize::new(0))));
        notices.dispatch(SystemNotice::Wake);
        assert_eq!(marked(&kept), 1);
        assert_eq!(
            notices.cached.lock().len(),
            2,
            "pruned on the next registration"
        );
        notices.mark_stale_on_wake(&Arc::new(Cache(AtomicUsize::new(0))));
        assert_eq!(notices.cached.lock().len(), 2);
    }
}
