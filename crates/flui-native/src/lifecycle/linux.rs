//! The Linux host's system notices: sleep and wake, shutdown, the screen lock
//! and memory pressure, gathered from the login manager and the kernel and
//! handed to the owner. Every Linux host reads them: the Wayland and X11 hosts
//! and the winit host.
//!
//! Two sources run beside the owner and never run application code:
//!
//! - **logind** ([`logind`]): an async task on the platform's background
//!   executor listens on the system bus. It holds a *delay inhibitor* so the
//!   machine waits for the application before it sleeps or shuts down.
//! - **memory pressure** ([`pressure`]): a thread polls kernel pressure
//!   triggers.
//!
//! Each posts a `HostEvent` to the `Inbox` and wakes the owner; the owner
//! drains the inbox in its next turn (`HostNoticeSource::take`) and delivers
//! each event. An event the machine waits on (sleep, shutdown) carries a
//! completion the source awaits before it lets go of the inhibitor, so the
//! application has run its subscribers, and so saved, by then. The wait is
//! bounded: logind itself gives up after `InhibitDelayMaxSec`, 5 s by default,
//! and a stalled owner must not hold a shutdown.
//!
//! A source that cannot start (no system bus, a kernel without pressure
//! information) leaves the others running and says so once in the log.
//!
//! The sources name no host; how the native loops and the winit loop deliver
//! what they queue sits apart, in a block each. What each source's answers
//! mean is a pure rule beside it, which every host checks.

#[cfg(linux_host)]
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

#[cfg(linux_host)]
use flui_platform::SystemNotice;
#[cfg(linux_host)]
use parking_lot::Mutex;
#[cfg(linux_host)]
use tokio::sync::oneshot;

#[cfg(linux_host)]
use crate::{
    executor::BackgroundExecutor,
    owner::{panic_boundary::contain_owner_callback, signal::OwnerSignal},
};

mod logind;
mod pressure;

/// What every Linux host does for the application when the session ends:
/// logind cannot be refused, but its delay inhibitor waits for the owner up to
/// the delivery budget.
#[cfg(linux_host)]
pub(crate) const SESSION_END_SUPPORT: flui_platform::SessionEndSupport =
    flui_platform::SessionEndSupport::Delay {
        max: logind::DELIVERY_BUDGET,
    };

/// The most events waiting for the owner. Sleep, lock and shutdown are rare
/// and pressure is limited to one per window, so a full inbox means the owner
/// has stopped; the newest event is then the one dropped.
#[cfg(linux_host)]
const MAX_PENDING: usize = 256;

/// What a source tells the owner.
#[cfg(linux_host)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostEvent {
    /// A notice for the host.
    Notice(SystemNotice),
    /// The machine is shutting down: ask the application, then tell it.
    SessionEnding,
    /// The shutdown that began was cancelled.
    SessionEndCancelled,
}

/// An event with the completion its source may be waiting on.
#[cfg(linux_host)]
pub(crate) struct Delivery {
    event: HostEvent,
    done: Option<oneshot::Sender<()>>,
}

#[cfg(linux_host)]
impl Delivery {
    /// Run `handle` on the event, then tell the source it was delivered. A
    /// `handle` that panics drops the completion too, which the source takes
    /// as delivered: a panic in the application must not hold a shutdown.
    pub(crate) fn deliver_with(self, handle: impl FnOnce(HostEvent)) {
        let Self { event, done } = self;
        handle(event);
        if let Some(done) = done {
            // A source that stopped waiting has dropped its end.
            let _ = done.send(());
        }
    }
}

/// The events waiting for the owner, and the way to wake it.
#[cfg(linux_host)]
struct Inbox {
    queue: Mutex<VecDeque<Delivery>>,
    signal: Weak<OwnerSignal>,
}

#[cfg(linux_host)]
impl Inbox {
    /// Queue `event` and wake the owner.
    fn post(&self, event: HostEvent) {
        self.push(event, None);
    }

    /// Queue `event` and wake the owner. The receiver completes once the owner
    /// has delivered the event; `None` when the inbox was full and dropped it.
    fn post_awaited(&self, event: HostEvent) -> Option<oneshot::Receiver<()>> {
        let (done, receiver) = oneshot::channel();
        self.push(event, Some(done)).then_some(receiver)
    }

    /// Queue `event` with the completion to signal once delivered, and wake
    /// the owner. Whether it was queued.
    fn push(&self, event: HostEvent, done: Option<oneshot::Sender<()>>) -> bool {
        {
            let mut queue = self.queue.lock();
            if queue.len() >= MAX_PENDING {
                tracing::warn!(?event, "the owner is not taking host notices; dropping one");
                return false;
            }
            queue.push_back(Delivery { event, done });
        }
        // The upgraded handle can be the last owner of the signal, and
        // releasing it drops the owner-turn callback, which is application
        // code.
        contain_owner_callback(|| {
            if let Some(signal) = self.signal.upgrade() {
                // A refused wake means the owner is closing.
                let _ = signal.wake();
            }
        });
        true
    }
}

/// The sources of system notices on Linux, running until dropped.
#[cfg(linux_host)]
pub(crate) struct HostNoticeSource {
    inbox: Arc<Inbox>,
    logind: tokio::task::JoinHandle<()>,
    // Dropping the watcher stops its thread.
    _pressure: Option<pressure::PressureWatcher>,
}

#[cfg(linux_host)]
impl std::fmt::Debug for HostNoticeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostNoticeSource").finish_non_exhaustive()
    }
}

#[cfg(linux_host)]
impl HostNoticeSource {
    /// Start the sources. `signal` is woken when an event is waiting; it is
    /// held weakly.
    pub(crate) fn start(executor: &BackgroundExecutor, signal: &Arc<OwnerSignal>) -> Self {
        let inbox = Arc::new(Inbox {
            queue: Mutex::new(VecDeque::new()),
            signal: Arc::downgrade(signal),
        });
        let logind = executor.handle().spawn(logind::watch(Arc::clone(&inbox)));
        let pressure = pressure::PressureWatcher::start(&inbox);
        Self {
            inbox,
            logind,
            _pressure: pressure,
        }
    }

    /// Take the events waiting for the owner, oldest first.
    pub(crate) fn take(&self) -> Vec<Delivery> {
        self.inbox.queue.lock().drain(..).collect()
    }
}

#[cfg(linux_host)]
impl Drop for HostNoticeSource {
    fn drop(&mut self) {
        self.logind.abort();
    }
}

/// The winit host on Linux starts the sources once its owner can deliver what
/// they queue, and delivers it on each owner turn.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
mod winit {
    use std::sync::Arc;

    use flui_foundation::panic::FirstPanic;
    use flui_platform::{SessionEndAnswer, SessionEndPhase};

    use super::{HostEvent, HostNoticeSource};
    use crate::executor::BackgroundExecutor;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::winit::{HostNoticeStart, HostNotices, WinitLoop};

    /// The Linux sources of system notices as the winit loop starts them.
    #[derive(Debug)]
    pub(crate) struct WinitNotices;

    impl HostNoticeStart for WinitNotices {
        fn start(
            &self,
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Box<dyn HostNotices> {
            Box::new(HostNoticeSource::start(executor, signal))
        }
    }

    /// The source holds a sleep or shutdown until its event has been
    /// delivered, so each runs to the end here before the next.
    impl HostNotices for HostNoticeSource {
        fn deliver(&self, platform: &WinitLoop) {
            // A subscriber that panics must not keep the rest from hearing, nor
            // keep logind waiting for the events behind it: every delivery
            // runs, and the first panic is raised once all have.
            let mut panics = FirstPanic::new();
            for delivery in self.take() {
                panics.contain(|| {
                    delivery.deliver_with(|event| match event {
                        HostEvent::Notice(notice) => platform.notices.dispatch(notice),
                        // Linux cannot hold a shutdown for the user, so the
                        // answer to the query is not read; the application
                        // hears the query so that it can start saving, then
                        // the shutdown.
                        HostEvent::SessionEnding => {
                            let mut phases = FirstPanic::new();
                            let answer =
                                phases.contain(|| platform.session_end(SessionEndPhase::Query));
                            if answer == Some(SessionEndAnswer::Block) {
                                tracing::info!(
                                    "the application asked to hold the shutdown; logind cannot wait for the user, so it goes on"
                                );
                            }
                            phases.contain(|| platform.session_end(SessionEndPhase::Ending));
                            phases.resume();
                        }
                        HostEvent::SessionEndCancelled => {
                            platform.session_end(SessionEndPhase::Cancelled);
                        }
                    });
                });
            }
            panics.resume();
        }
    }
}

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
pub(crate) use winit::WinitNotices;

/// The native Linux loops, Wayland's and X11's, start the sources once their
/// owner can deliver what they queue, and deliver it on each of their turns.
#[cfg(any(wayland_host, x11_host))]
impl crate::owner::linux::LinuxNotices for HostNoticeSource {
    /// The source holds a sleep or shutdown until its event has been
    /// delivered, so each runs to the end here before the next; a handler that
    /// panics keeps neither the rest nor logind waiting, and the first panic
    /// is raised once all have run.
    fn deliver(&self, owner: &dyn crate::owner::linux::SessionLoop) {
        use flui_foundation::panic::FirstPanic;
        use flui_platform::{SessionEndAnswer, SessionEndPhase};

        let mut panics = FirstPanic::new();
        for delivery in self.take() {
            panics.contain(|| {
                delivery.deliver_with(|event| match event {
                    HostEvent::Notice(notice) => owner.notice(notice),
                    // Linux cannot hold a shutdown for the user: the query is
                    // asked so the application can start saving, and its
                    // answer is not read.
                    HostEvent::SessionEnding => {
                        let mut phases = FirstPanic::new();
                        let answer =
                            phases.contain(|| owner.session_end(SessionEndPhase::Query));
                        if answer == Some(SessionEndAnswer::Block) {
                            tracing::info!(
                                "the application asked to hold the shutdown; logind cannot wait for the user, so it goes on"
                            );
                        }
                        phases.contain(|| owner.session_end(SessionEndPhase::Ending));
                        phases.resume();
                    }
                    HostEvent::SessionEndCancelled => {
                        owner.session_end(SessionEndPhase::Cancelled);
                    }
                });
            });
        }
        panics.resume();
    }
}

#[cfg(all(test, linux_host))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::memory::MemoryPressureLevel;

    use super::*;

    fn inbox() -> Inbox {
        Inbox {
            queue: Mutex::new(VecDeque::new()),
            signal: Weak::new(),
        }
    }

    fn drain(inbox: &Inbox) -> Vec<Delivery> {
        inbox.queue.lock().drain(..).collect()
    }

    #[test]
    fn host_events_reach_the_owner_in_order() {
        let inbox = inbox();
        let events = [
            HostEvent::Notice(SystemNotice::Sleep),
            HostEvent::SessionEnding,
            HostEvent::Notice(SystemNotice::MemoryPressure(MemoryPressureLevel::Critical)),
            HostEvent::SessionEndCancelled,
        ];
        for event in events {
            inbox.post(event);
        }
        let seen: Vec<_> = drain(&inbox).into_iter().map(|d| d.event).collect();
        assert_eq!(seen, events);
    }

    #[test]
    fn a_waiting_event_completes_when_the_owner_has_delivered_it() {
        let inbox = inbox();
        let mut waiting = inbox
            .post_awaited(HostEvent::Notice(SystemNotice::Sleep))
            .expect("a waiting post returns its completion");
        assert!(waiting.try_recv().is_err(), "not delivered yet");

        let delivered = AtomicUsize::new(0);
        for delivery in drain(&inbox) {
            delivery.deliver_with(|_| {
                delivered.fetch_add(1, Ordering::SeqCst);
                assert!(waiting.try_recv().is_err(), "completes after the handler");
            });
        }
        assert_eq!(delivered.load(Ordering::SeqCst), 1);
        assert_eq!(waiting.try_recv(), Ok(()));
    }

    #[test]
    fn a_handler_that_panics_still_releases_the_waiting_source() {
        let inbox = inbox();
        let mut waiting = inbox
            .post_awaited(HostEvent::SessionEnding)
            .expect("a waiting post returns its completion");
        let delivery = drain(&inbox).remove(0);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            delivery.deliver_with(|_| panic!("a subscriber panicked"));
        }));
        assert!(outcome.is_err());
        // The sender went with the unwind: the source sees a closed channel,
        // not a wait that runs to its budget.
        assert_eq!(
            waiting.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        );
    }

    #[test]
    fn a_full_inbox_drops_the_newest_event() {
        let inbox = inbox();
        for _ in 0..MAX_PENDING {
            inbox.post(HostEvent::Notice(SystemNotice::SessionLocked));
        }
        assert!(inbox.post_awaited(HostEvent::SessionEnding).is_none());
        let queued = drain(&inbox);
        assert_eq!(queued.len(), MAX_PENDING);
        assert!(
            queued
                .iter()
                .all(|d| d.event == HostEvent::Notice(SystemNotice::SessionLocked))
        );
    }
}
