//! The logind adapter against a stand-in for logind on a private bus.
//!
//! The test starts a `dbus-daemon` of its own (skipped when none is
//! installed), has a stand-in own `org.freedesktop.login1`, and drives the
//! adapter through that: the signals come from the stand-in, and the delay
//! inhibitor the adapter holds is one end of a socket pair whose other end the
//! stand-in watches, so "released" is observed as the socket closing.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    os::{fd::OwnedFd as StdOwnedFd, unix::net::UnixStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use rustix::event::{PollFd, PollFlags, poll};
use zbus::{
    interface,
    object_server::SignalEmitter,
    zvariant::{OwnedFd, OwnedObjectPath},
};

use super::*;
use crate::lifecycle::linux::Delivery;

const MANAGER: &str = "/org/freedesktop/login1";
const SESSION: &str = "/org/freedesktop/login1/session/_1";

/// A private session bus, stopped when dropped.
struct PrivateBus {
    daemon: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut address = String::new();
        BufReader::new(daemon.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        let address = address.trim().to_owned();
        (!address.is_empty()).then_some(Self { daemon, address })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// What the stand-in manager hands out and keeps.
#[derive(Default)]
struct Held {
    /// How many inhibitors were asked for.
    taken: AtomicUsize,
    /// The stand-in's end of each inhibitor's socket.
    ends: Mutex<Vec<StdOwnedFd>>,
    /// How long the stand-in takes to answer a session lookup.
    session_delay: Mutex<Duration>,
}

impl Held {
    /// Whether the adapter closed its end of the `index`th inhibitor.
    fn released(&self, index: usize) -> bool {
        let ends = self.ends.lock();
        let Some(end) = ends.get(index) else {
            return false;
        };
        let mut fds = [PollFd::new(end, PollFlags::IN | PollFlags::HUP)];
        poll(&mut fds, Some(&rustix::time::Timespec::default())).is_ok()
            && fds[0].revents().intersects(PollFlags::HUP | PollFlags::IN)
    }
}

struct Manager(Arc<Held>, &'static str);

#[interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn inhibit(&self, what: String, who: String, why: String, mode: String) -> OwnedFd {
        assert_eq!(what, "sleep:shutdown");
        assert!(!who.is_empty() && !why.is_empty());
        assert_eq!(mode, "delay");
        let (client, server) = UnixStream::pair().expect("a socket pair");
        self.0.ends.lock().push(StdOwnedFd::from(server));
        self.0.taken.fetch_add(1, Ordering::SeqCst);
        OwnedFd::from(StdOwnedFd::from(client))
    }

    #[zbus(name = "GetSessionByPID")]
    async fn get_session_by_pid(&self, pid: u32) -> OwnedObjectPath {
        let delay = *self.0.session_delay.lock();
        tokio::time::sleep(delay).await;
        assert_eq!(pid, std::process::id(), "the adapter asks about itself");
        OwnedObjectPath::try_from(self.1).expect("a valid path")
    }

    #[zbus(signal)]
    async fn prepare_for_sleep(emitter: &SignalEmitter<'_>, start: bool) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn prepare_for_shutdown(emitter: &SignalEmitter<'_>, start: bool) -> zbus::Result<()>;
}

struct Session(bool);

#[interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.0
    }

    #[zbus(signal)]
    async fn lock(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn unlock(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

fn inbox() -> Arc<Inbox> {
    Arc::new(Inbox {
        queue: Mutex::new(VecDeque::new()),
        signal: std::sync::Weak::new(),
    })
}

/// Wait (bounded, 5 s) for `check`.
async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The next event the owner would be handed, waiting for it.
async fn next_delivery(inbox: &Inbox) -> Delivery {
    let mut found = None;
    until("an event for the owner", || {
        found = inbox.queue.lock().pop_front();
        found.is_some()
    })
    .await;
    found.expect("the wait ended with an event")
}

struct Harness {
    held: Arc<Held>,
    server: zbus::Connection,
    inbox: Arc<Inbox>,
    adapter: tokio::task::JoinHandle<zbus::Result<()>>,
    _bus: PrivateBus,
}

impl Harness {
    async fn start(delivery_budget: Duration, locked: bool) -> Option<Self> {
        Self::start_with(delivery_budget, locked, Duration::ZERO).await
    }

    async fn start_with(
        delivery_budget: Duration,
        locked: bool,
        session_delay: Duration,
    ) -> Option<Self> {
        let bus = PrivateBus::start().or_else(|| {
            eprintln!("skipped: no dbus-daemon to host a stand-in for logind");
            None
        })?;
        let held = Arc::new(Held::default());
        *held.session_delay.lock() = session_delay;
        let server = zbus::connection::Builder::address(bus.address.as_str())
            .expect("an address")
            .name(DESTINATION)
            .expect("a bus name")
            .serve_at(MANAGER, Manager(Arc::clone(&held), SESSION))
            .expect("serve the manager")
            .serve_at(SESSION, Session(locked))
            .expect("serve the session")
            .build()
            .await
            .expect("the stand-in owns the name");
        let client = zbus::connection::Builder::address(bus.address.as_str())
            .expect("an address")
            .build()
            .await
            .expect("the adapter connects");
        let inbox = inbox();
        let adapter = tokio::spawn({
            let inbox = Arc::clone(&inbox);
            async move { serve(&inbox, &client, delivery_budget).await }
        });
        until("the first inhibitor", || {
            held.taken.load(Ordering::SeqCst) == 1
        })
        .await;
        Some(Self {
            held,
            server,
            inbox,
            adapter,
            _bus: bus,
        })
    }

    fn manager_signals(&self) -> SignalEmitter<'_> {
        SignalEmitter::new(&self.server, MANAGER).expect("a valid path")
    }

    fn session_signals(&self) -> SignalEmitter<'_> {
        SignalEmitter::new(&self.server, SESSION).expect("a valid path")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn logind_inhibitor_is_held_until_the_owner_has_delivered_the_sleep() {
    let Some(harness) = Harness::start(Duration::from_secs(5), false).await else {
        return;
    };
    assert!(!harness.held.released(0), "held from the start");

    Manager::prepare_for_sleep(&harness.manager_signals(), true)
        .await
        .expect("the signal is sent");
    let delivery = next_delivery(&harness.inbox).await;
    // The machine is going down but the owner has not yet saved: the inhibitor
    // is still held.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!harness.held.released(0), "held until the owner delivers");

    let mut seen = None;
    delivery.deliver_with(|event| seen = Some(event));
    assert_eq!(seen, Some(HostEvent::Notice(SystemNotice::Sleep)));
    until("the inhibitor to be released", || harness.held.released(0)).await;

    // The machine is back: a new inhibitor is taken for the next sleep.
    Manager::prepare_for_sleep(&harness.manager_signals(), false)
        .await
        .expect("the signal is sent");
    let wake = next_delivery(&harness.inbox).await;
    wake.deliver_with(|event| assert_eq!(event, HostEvent::Notice(SystemNotice::Wake)));
    until("a new inhibitor", || {
        harness.held.taken.load(Ordering::SeqCst) == 2
    })
    .await;
    assert!(!harness.held.released(1));
    harness.adapter.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn logind_inhibitor_is_not_held_longer_than_the_budget_by_an_owner_that_never_delivers() {
    let Some(harness) = Harness::start(Duration::from_millis(300), false).await else {
        return;
    };
    Manager::prepare_for_shutdown(&harness.manager_signals(), true)
        .await
        .expect("the signal is sent");
    let undelivered = next_delivery(&harness.inbox).await;
    assert_eq!(undelivered.event, HostEvent::SessionEnding);
    // The owner holds the event and never delivers it.
    until("the budget to release the inhibitor", || {
        harness.held.released(0)
    })
    .await;
    drop(undelivered);

    // The shutdown is cancelled: the application hears of it, and holds the
    // next one up again.
    Manager::prepare_for_shutdown(&harness.manager_signals(), false)
        .await
        .expect("the signal is sent");
    let cancelled = next_delivery(&harness.inbox).await;
    assert_eq!(cancelled.event, HostEvent::SessionEndCancelled);
    until("a new inhibitor", || {
        harness.held.taken.load(Ordering::SeqCst) == 2
    })
    .await;
    harness.adapter.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn logind_lock_and_unlock_reach_the_owner() {
    let Some(harness) = Harness::start(Duration::from_secs(5), false).await else {
        return;
    };
    Session::lock(&harness.session_signals())
        .await
        .expect("the signal is sent");
    assert_eq!(
        next_delivery(&harness.inbox).await.event,
        HostEvent::Notice(SystemNotice::SessionLocked)
    );
    Session::unlock(&harness.session_signals())
        .await
        .expect("the signal is sent");
    assert_eq!(
        next_delivery(&harness.inbox).await.event,
        HostEvent::Notice(SystemNotice::SessionUnlocked)
    );
    // Sleep and shutdown do not take the lock events' place: the inhibitor
    // stayed held through both.
    assert!(!harness.held.released(0));
    harness.adapter.abort();
}

/// A session that is locked when the adapter starts is announced as locked
/// once, and the unlock that follows is heard.
#[tokio::test(flavor = "multi_thread")]
async fn logind_session_locked_at_start_is_announced_once() {
    let Some(harness) = Harness::start(Duration::from_secs(5), true).await else {
        return;
    };
    assert_eq!(
        next_delivery(&harness.inbox).await.event,
        HostEvent::Notice(SystemNotice::SessionLocked)
    );
    // The `Lock` that announced it can arrive again, as it does when it was
    // queued during the adapter's setup: it is the same lock.
    Session::lock(&harness.session_signals())
        .await
        .expect("the signal is sent");
    Session::unlock(&harness.session_signals())
        .await
        .expect("the signal is sent");
    assert_eq!(
        next_delivery(&harness.inbox).await.event,
        HostEvent::Notice(SystemNotice::SessionUnlocked)
    );
    harness.adapter.abort();
}

/// A logind that is slow to say which session is this process's must not
/// hold up a sleep: the inhibitor's time starts when logind sends the signal.
#[tokio::test(flavor = "multi_thread")]
async fn logind_sleep_is_delivered_while_the_session_is_still_being_looked_up() {
    let Some(harness) =
        Harness::start_with(Duration::from_secs(5), false, Duration::from_secs(4)).await
    else {
        return;
    };
    let asked = Instant::now();
    Manager::prepare_for_sleep(&harness.manager_signals(), true)
        .await
        .expect("the signal is sent");
    let delivery = next_delivery(&harness.inbox).await;
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "the sleep waited for the session lookup: {:?}",
        asked.elapsed()
    );
    assert_eq!(delivery.event, HostEvent::Notice(SystemNotice::Sleep));
    harness.adapter.abort();
}
