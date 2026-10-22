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

use std::collections::VecDeque;
use std::sync::{Arc, Weak};

use flui_platform::SystemNotice;
use parking_lot::Mutex;

use crate::owner::call_point::{Field, call, install};

/// Something a platform caches about displays or windows, and can mark stale.
pub(crate) trait StaleOnWake: Send + Sync {
    /// Mark the cached value stale: the next read asks the operating system.
    fn mark_stale(&self);
}

/// The callback a platform calls with each system notice.
pub(crate) type SystemNoticeHook = Box<dyn FnMut(SystemNotice) + Send>;

/// A platform's system-notice channel: the host's hook, what a wake marks
/// stale before the hook hears of it, and the notices accepted but not yet
/// heard.
#[derive(Default)]
pub(crate) struct SystemNotices {
    state: Mutex<Delivery>,
    cached: Mutex<Vec<Weak<dyn StaleOnWake>>>,
    /// Runs once, on the dispatching thread, after a walk's last look at the
    /// queue and before its guard is disarmed: the instant a notice from
    /// another thread can arrive once the walk has stopped looking.
    #[cfg(test)]
    walk_ending: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[derive(Default)]
struct Delivery {
    hook: Option<SystemNoticeHook>,
    /// Accepted, in order, and not yet handed to the hook.
    pending: VecDeque<SystemNotice>,
    /// Whether a dispatch is walking `pending` right now.
    delivering: bool,
}

impl Delivery {
    /// The next notice for the hook. With none left, or no hook to hear it,
    /// the walk is over, under this same lock: a notice accepted from here on
    /// finds `delivering` false and walks itself.
    fn next(&mut self) -> Option<SystemNotice> {
        let next = if self.hook.is_some() {
            self.pending.pop_front()
        } else {
            // Nobody to tell: what was accepted has no listener.
            self.pending.clear();
            None
        };
        if next.is_none() {
            self.delivering = false;
        }
        next
    }
}

/// Ends a walk that unwinds out of the hook, so the next dispatch walks again.
/// A walk that runs out of notices has already ended under the lock and
/// disarms this: a newer walk may have begun since, and its flag is not this
/// one's to clear.
struct Walk<'a>(&'a SystemNotices);

impl Walk<'_> {
    fn disarm(self) {
        std::mem::forget(self);
    }
}

impl Drop for Walk<'_> {
    fn drop(&mut self) {
        self.0.state.lock().delivering = false;
    }
}

impl SystemNotices {
    /// The host's hook, as a slot of the delivery state.
    fn hook(&self) -> Field<'_, Mutex<Delivery>, SystemNoticeHook> {
        Field::new(&self.state, |delivery| &mut delivery.hook)
    }

    /// Install the host's hook, replacing an earlier one; the earlier one is
    /// dropped outside the channel's lock.
    pub(crate) fn set_hook(&self, hook: SystemNoticeHook) {
        install(&self.hook(), hook);
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
    /// [`mark_stale_on_wake`](Self::mark_stale_on_wake) first, at once, even
    /// when the host is busy with an earlier notice.
    ///
    /// The hook runs through the call point, with no lock of this channel
    /// held, and goes back to its slot between notices, so a replacement
    /// installed meanwhile is not displaced. A notice that arrives from inside
    /// the hook (or while another thread's dispatch is running it) is queued
    /// behind the one being delivered and heard, in order, as soon as the hook
    /// returns. A walk ends under the same lock that finds nothing left to
    /// deliver, so a notice from another thread is taken by the walk or starts
    /// one of its own; it never waits for a later notice to carry it. A hook
    /// that panics keeps its place and its panic reaches the caller: the next
    /// dispatch calls it again, and the notices queued behind the failure are
    /// still delivered then.
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
        {
            let mut state = self.state.lock();
            state.pending.push_back(notice);
            if state.delivering {
                return;
            }
            state.delivering = true;
        }
        let walk = Walk(self);
        loop {
            let next = self.state.lock().next();
            let Some(notice) = next else { break };
            call(&self.hook(), |hook| hook(notice)).or_resume(());
        }
        #[cfg(test)]
        {
            let walk_ending = self.walk_ending.lock().take();
            if let Some(walk_ending) = walk_ending {
                walk_ending();
            }
        }
        walk.disarm();
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
        use flui_platform::{WindowGeometry, WindowState};

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
    fn a_hook_that_panics_keeps_its_place_and_the_queue_behind_it() {
        use std::sync::Mutex as StdMutex;

        let notices = SystemNotices::default();
        let heard = Arc::new(StdMutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        let mut first = true;
        notices.set_hook(Box::new(move |notice| {
            log.lock().expect("log").push(notice);
            assert!(
                !std::mem::take(&mut first),
                "the first notice fails the hook"
            );
        }));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            notices.dispatch(SystemNotice::Sleep);
        }));
        assert!(outcome.is_err(), "the hook's panic reaches the dispatcher");

        notices.dispatch(SystemNotice::SessionLocked);
        assert_eq!(
            *heard.lock().expect("log"),
            [SystemNotice::Sleep, SystemNotice::SessionLocked],
            "the hook was not lost with the unwind"
        );
    }

    #[test]
    fn a_notice_from_inside_the_hook_is_heard_after_it_in_order() {
        use std::sync::Mutex as StdMutex;

        let notices = Arc::new(SystemNotices::default());
        let heard = Arc::new(StdMutex::new(Vec::new()));
        let (log, again) = (Arc::clone(&heard), Arc::clone(&notices));
        notices.set_hook(Box::new(move |notice| {
            log.lock().expect("log").push(("start", notice));
            if notice == SystemNotice::Sleep {
                again.dispatch(SystemNotice::Wake);
                log.lock().expect("log").push(("end", notice));
            }
        }));

        notices.dispatch(SystemNotice::Sleep);
        assert_eq!(
            *heard.lock().expect("log"),
            [
                ("start", SystemNotice::Sleep),
                ("end", SystemNotice::Sleep),
                ("start", SystemNotice::Wake),
            ],
            "a nested notice is queued, not dropped and not recursed into"
        );
    }

    /// A notice another thread dispatches in the instant a walk ends is not
    /// left queued for whichever notice comes next: the walk takes it, or it
    /// walks itself.
    #[test]
    fn a_notice_accepted_as_a_walk_ends_is_heard_without_waiting_for_the_next() {
        use std::sync::Mutex as StdMutex;

        let notices = Arc::new(SystemNotices::default());
        let heard = Arc::new(StdMutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        notices.set_hook(Box::new(move |notice| {
            log.lock().expect("log").push(notice);
        }));
        let other = Arc::clone(&notices);
        *notices.walk_ending.lock() = Some(Box::new(move || {
            std::thread::spawn(move || other.dispatch(SystemNotice::Wake))
                .join()
                .expect("the second dispatch returns");
        }));

        notices.dispatch(SystemNotice::Sleep);
        assert_eq!(
            *heard.lock().expect("log"),
            [SystemNotice::Sleep, SystemNotice::Wake],
            "the wake was heard before any later notice arrived"
        );
    }

    /// A walk that begins as an earlier one ends is still delivering once the
    /// earlier one is over: a notice dispatched while its hook runs queues
    /// behind it, instead of starting a second walk that finds the hook out of
    /// its slot and drops the notice.
    #[test]
    fn the_end_of_a_walk_leaves_the_walk_begun_after_it_delivering() {
        use std::sync::Mutex as StdMutex;
        use std::sync::mpsc;

        /// What the second thread reaches first.
        enum Second {
            InTheHook,
            Returned,
        }

        let notices = Arc::new(SystemNotices::default());
        let heard = Arc::new(StdMutex::new(Vec::new()));
        let (reached_tx, reached) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let (log, in_the_hook) = (Arc::clone(&heard), reached_tx.clone());
        notices.set_hook(Box::new(move |notice| {
            log.lock().expect("log").push(notice);
            if notice == SystemNotice::Wake {
                in_the_hook
                    .send(Second::InTheHook)
                    .expect("the first walk's end waits");
                held.recv().expect("the test releases the hook");
            }
        }));
        let (other, (handoff_tx, handoff)) = (Arc::clone(&notices), mpsc::channel());
        *notices.walk_ending.lock() = Some(Box::new(move || {
            let second = std::thread::spawn(move || {
                other.dispatch(SystemNotice::Wake);
                // Unheard once the first walk's end has its answer.
                let _ = reached_tx.send(Second::Returned);
            });
            let first = reached.recv().expect("the second thread reports");
            handoff_tx.send((second, first)).expect("the test waits");
        }));

        notices.dispatch(SystemNotice::Sleep);
        let (second, first) = handoff.recv().expect("the first walk ended");
        assert!(
            matches!(first, Second::InTheHook),
            "the wake accepted as the first walk ended walked itself"
        );
        notices.dispatch(SystemNotice::SessionLocked);
        release.send(()).expect("the hook is held");
        second.join().expect("the second walk ends");
        assert_eq!(
            *heard.lock().expect("log"),
            [
                SystemNotice::Sleep,
                SystemNotice::Wake,
                SystemNotice::SessionLocked
            ],
            "the notice dispatched during the second walk queued behind it"
        );
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
