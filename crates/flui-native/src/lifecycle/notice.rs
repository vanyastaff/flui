//! The one path every host's system notices take to the application: the
//! owner's [`NoticeChannel`], which each host's table makes for its loop over
//! the loop's application delivery.
//!
//! The channel reports each notice to the application's handler through the
//! delivery, which queues what the handler cannot hear yet and delivers in
//! order. For [`SystemNotice::Wake`] the dispatch first marks everything the
//! platform cached about the world stale: the displays it sampled and each
//! window's published state. Both can have changed while the machine slept
//! without an event reaching the process, and a backend that forgot to mark
//! them would answer from before the sleep until some unrelated event happened
//! to invalidate them. Putting the marking here, in the dispatch, makes a Wake
//! that reaches the application without it impossible.

use std::sync::{Arc, Weak};

use flui_platform::SystemNotice;
use parking_lot::Mutex;

use crate::owner::app_delivery::AppDelivery;
use crate::owner::notices::{NoticeChannel, StaleOnWake};

/// The system-notice channel each host's table makes for its loop: what a
/// wake marks stale, and the loop's delivery to the application's handler.
pub(crate) struct SystemNotices {
    app: Arc<AppDelivery>,
    cached: Mutex<Vec<Weak<dyn StaleOnWake>>>,
}

impl SystemNotices {
    /// The channel that reports to `app`, the loop's delivery.
    pub(crate) fn new(app: Arc<AppDelivery>) -> Self {
        Self {
            app,
            cached: Mutex::new(Vec::new()),
        }
    }

    /// The channel's tests register caches by their own type, as a loop does
    /// through `dyn NoticeChannel`.
    #[cfg(test)]
    fn mark_stale_on_wake<T: StaleOnWake + 'static>(&self, cache: &Arc<T>) {
        let cache: Weak<T> = Arc::downgrade(cache);
        self.mark_held_stale_on_wake(cache);
    }
}

impl NoticeChannel for SystemNotices {
    fn mark_held_stale_on_wake(&self, cache: Weak<dyn StaleOnWake>) {
        let mut cached = self.cached.lock();
        cached.retain(|entry| entry.strong_count() > 0);
        cached.push(cache);
    }

    fn dispatch(&self, notice: SystemNotice) {
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
        self.app.host_notice(notice).or_resume(());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::memory::MemoryPressureLevel;

    use super::*;
    use crate::owner::app_delivery::testing::{AppCall, RecordingApp, bound};

    struct Cache(AtomicUsize);

    impl StaleOnWake for Cache {
        fn mark_stale(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn marked(cache: &Cache) -> usize {
        cache.0.load(Ordering::SeqCst)
    }

    fn unbound() -> SystemNotices {
        SystemNotices::new(Arc::new(AppDelivery::new()))
    }

    #[test]
    fn a_wake_marks_the_registered_caches_before_the_handler_hears_it() {
        let app = RecordingApp::new();
        let run = bound(app.clone());
        let notices = SystemNotices::new(Arc::clone(&run.delivery));
        let cache = Arc::new(Cache(AtomicUsize::new(0)));
        notices.mark_stale_on_wake(&cache);
        let seen_by_handler = Arc::new(AtomicUsize::new(usize::MAX));
        let (observed, cache_in_handler) = (Arc::clone(&seen_by_handler), Arc::clone(&cache));
        app.react(move |_, _| {
            observed.store(marked(&cache_in_handler), Ordering::SeqCst);
        });

        notices.dispatch(SystemNotice::Wake);

        assert_eq!(marked(&cache), 1);
        assert_eq!(
            app.calls(),
            [AppCall::Ready, AppCall::Notice(SystemNotice::Wake)]
        );
        assert_eq!(
            seen_by_handler.load(Ordering::SeqCst),
            1,
            "the cache was already stale when the application was told"
        );
    }

    #[test]
    fn other_notices_leave_the_caches_alone() {
        let notices = unbound();
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
        use flui_platform::{WindowGeometry, WindowState};

        use crate::display::source::DisplaySource;
        use crate::window::state::WindowStateCell;

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

        let notices = unbound();
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
    fn a_dropped_cache_is_forgotten_and_a_wake_without_a_handler_still_marks() {
        let notices = unbound();
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
