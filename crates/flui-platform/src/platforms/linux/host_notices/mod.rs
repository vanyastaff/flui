//! The Linux host's system notices: sleep and wake, shutdown, the screen lock
//! and memory pressure, gathered from the login manager and the kernel and
//! handed to the owner.
//!
//! Two sources run beside the owner and never run application code:
//!
//! - **logind** ([`logind`]): an async task on the platform's background
//!   executor listens on the system bus. It holds a *delay inhibitor* so the
//!   machine waits for the application before it sleeps or shuts down.
//! - **memory pressure** ([`pressure`]): a thread polls kernel pressure
//!   triggers.
//!
//! Each posts a [`HostEvent`] to the [`Inbox`] and wakes the owner; the owner
//! drains the inbox in its next turn ([`HostNoticeSource::take`]) and delivers
//! each event. An event the machine waits on (sleep, shutdown) carries a
//! completion the source awaits before it lets go of the inhibitor, so the
//! application has run its subscribers, and so saved, by then. The wait is
//! bounded: logind itself gives up after `InhibitDelayMaxSec`, 5 s by default,
//! and a stalled owner must not hold a shutdown.
//!
//! A source that cannot start (no system bus, a kernel without pressure
//! information) leaves the others running and says so once in the log.

use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use flui_platform_api::SystemNotice;
use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::{
    executor::BackgroundExecutor,
    shared::{owner_signal::OwnerSignal, panic_boundary::contain_owner_callback},
};

mod logind;
mod pressure;

/// The most events waiting for the owner. Sleep, lock and shutdown are rare
/// and pressure is limited to one per window, so a full inbox means the owner
/// has stopped; the newest event is then the one dropped.
const MAX_PENDING: usize = 256;

/// What a source tells the owner.
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
pub(crate) struct Delivery {
    event: HostEvent,
    done: Option<oneshot::Sender<()>>,
}

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
struct Inbox {
    queue: Mutex<VecDeque<Delivery>>,
    signal: Weak<OwnerSignal>,
}

impl Inbox {
    /// Queue `event` and wake the owner. With `wait`, the receiver completes
    /// once the owner has delivered the event.
    fn post(&self, event: HostEvent, wait: bool) -> Option<oneshot::Receiver<()>> {
        let (done, receiver) = if wait {
            let (sender, receiver) = oneshot::channel();
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        {
            let mut queue = self.queue.lock();
            if queue.len() >= MAX_PENDING {
                tracing::warn!(?event, "the owner is not taking host notices; dropping one");
                return None;
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
        receiver
    }
}

/// The sources of system notices on Linux, running until dropped.
pub(crate) struct HostNoticeSource {
    inbox: Arc<Inbox>,
    logind: tokio::task::JoinHandle<()>,
    // Dropping the watcher stops its thread.
    _pressure: Option<pressure::PressureWatcher>,
}

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

impl Drop for HostNoticeSource {
    fn drop(&mut self) {
        self.logind.abort();
    }
}

#[cfg(test)]
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
            inbox.post(event, false);
        }
        let seen: Vec<_> = drain(&inbox).into_iter().map(|d| d.event).collect();
        assert_eq!(seen, events);
    }

    #[test]
    fn a_waiting_event_completes_when_the_owner_has_delivered_it() {
        let inbox = inbox();
        let mut waiting = inbox
            .post(HostEvent::Notice(SystemNotice::Sleep), true)
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
            .post(HostEvent::SessionEnding, true)
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
            inbox.post(HostEvent::Notice(SystemNotice::SessionLocked), false);
        }
        assert!(inbox.post(HostEvent::SessionEnding, true).is_none());
        let queued = drain(&inbox);
        assert_eq!(queued.len(), MAX_PENDING);
        assert!(
            queued
                .iter()
                .all(|d| d.event == HostEvent::Notice(SystemNotice::SessionLocked))
        );
    }
}
