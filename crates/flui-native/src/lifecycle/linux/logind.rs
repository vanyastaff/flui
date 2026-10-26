//! logind on the system bus: sleep, shutdown and the session's lock.
//!
//! The decisions (which signal means what, when to drop the inhibitor, which
//! session to ask about) are a pure rule ([`signals`]), which every host
//! checks; the rest of this file connects, listens and holds the inhibitor, on
//! a Linux host.
//!
//! A delay inhibitor is a file descriptor logind hands out: while it is open,
//! a sleep or shutdown waits for it, up to `InhibitDelayMaxSec`. It is taken
//! when the connection is made, dropped once the owner has delivered the
//! notice that the machine is going down, and taken again when it is back.

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use std::{sync::Arc, time::Duration};

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use flui_platform::SystemNotice;
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use futures_util::StreamExt;
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use zbus::{
    Proxy,
    zvariant::{OwnedFd, OwnedObjectPath},
};

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use super::{HostEvent, Inbox};
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
use signals::{
    InhibitorAction, LogindEvent, LogindFilter, MANAGER_INTERFACE, SESSION_INTERFACE,
    SessionLookup, decode_signal, locked_hint_event, session_lookups,
};

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
const DESTINATION: &str = "org.freedesktop.login1";
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
const MANAGER_PATH: &str = "/org/freedesktop/login1";

/// How long to wait for the system bus.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to wait before trying the bus again after it failed or went away.
/// A machine with no logind (a container, WSL without systemd) pays one failed
/// connect per interval.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
const RETRY: Duration = Duration::from_secs(10);
/// How long to hold the machine for the owner. logind's own limit is
/// `InhibitDelayMaxSec`, 5 s unless configured; this stays inside it.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
const DELIVERY_BUDGET: Duration = Duration::from_secs(4);

/// Listen to logind until the task is aborted, reconnecting when the bus goes.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
pub(super) async fn watch(inbox: Arc<Inbox>) {
    let mut reported = false;
    loop {
        match watch_connection(&inbox).await {
            Ok(()) => tracing::debug!("the logind connection ended; connecting again"),
            Err(error) if !reported => {
                reported = true;
                tracing::info!(%error, "logind is not available; sleep, shutdown and lock notices are off until it is");
            }
            Err(error) => tracing::debug!(%error, "logind is still not available"),
        }
        tokio::time::sleep(RETRY).await;
    }
}

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
async fn watch_connection(inbox: &Inbox) -> zbus::Result<()> {
    let connection = tokio::time::timeout(CONNECT_TIMEOUT, async {
        zbus::connection::Builder::system()?.build().await
    })
    .await
    .map_err(|_| zbus::Error::Failure("system bus connection timed out".into()))??;
    serve(inbox, &connection, DELIVERY_BUDGET).await
}

/// Listen on `connection` until it ends.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
async fn serve(
    inbox: &Inbox,
    connection: &zbus::Connection,
    delivery_budget: Duration,
) -> zbus::Result<()> {
    let manager = Proxy::new(connection, DESTINATION, MANAGER_PATH, MANAGER_INTERFACE).await?;
    // Both subscriptions precede the inhibitor: a sleep that begins while the
    // setup runs is queued for the loop below instead of lost.
    let mut sleeps = manager.receive_signal("PrepareForSleep").await?;
    let mut shutdowns = manager.receive_signal("PrepareForShutdown").await?;
    let mut inhibitor = take_inhibitor(&manager).await;
    // The session is found while the loop already listens: a slow logind must
    // not keep a sleep or shutdown signal waiting, since the inhibitor's time
    // runs from the moment logind sends it.
    let mut starting = Some(Box::pin(SessionWatch::start(connection, &manager)));
    let mut session: Option<SessionWatch> = None;
    // The filter starts unlocked and is told of a session that is locked when
    // found, so the owner hears of it once. Seeding it as locked instead would
    // turn a lock that landed between the subscription and the read (its
    // signal is queued) into a duplicate and drop it.
    let mut filter = LogindFilter::default();

    loop {
        let event = tokio::select! {
            message = sleeps.next() => {
                let message = message.ok_or_else(closed)?;
                decode_signal(MANAGER_INTERFACE, "PrepareForSleep", message.body().deserialize::<bool>().ok())
            }
            message = shutdowns.next() => {
                let message = message.ok_or_else(closed)?;
                decode_signal(MANAGER_INTERFACE, "PrepareForShutdown", message.body().deserialize::<bool>().ok())
            }
            found = async { starting.as_mut().expect("BUG: guarded by the branch condition").await }, if starting.is_some() => {
                starting = None;
                session = found;
                if session.as_ref().is_some_and(|session| session.locked)
                    && let Some(event) = filter.accept(LogindEvent::SessionLocked)
                {
                    inbox.post(host_event(event));
                }
                continue;
            }
            event = next_session_event(&mut session) => event?,
        };
        let Some(event) = event.and_then(|event| filter.accept(event)) else {
            continue;
        };
        let delivered = if event.inhibitor_action() == InhibitorAction::Release {
            inbox.post_awaited(host_event(event))
        } else {
            inbox.post(host_event(event));
            None
        };
        if let Some(delivered) = delivered {
            // An owner that never delivers cannot hold the machine longer
            // than the budget; one that dropped the completion has delivered.
            let _ = tokio::time::timeout(delivery_budget, delivered).await;
        }
        match event.inhibitor_action() {
            InhibitorAction::Release => drop(inhibitor.take()),
            InhibitorAction::Retake if inhibitor.is_none() => {
                inhibitor = take_inhibitor(&manager).await;
            }
            InhibitorAction::Retake | InhibitorAction::Keep => {}
        }
    }
}

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
fn closed() -> zbus::Error {
    zbus::Error::Failure("the logind signal stream ended".into())
}

/// What the owner is told for a logind event.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
fn host_event(event: LogindEvent) -> HostEvent {
    match event {
        LogindEvent::Sleep => HostEvent::Notice(SystemNotice::Sleep),
        LogindEvent::Wake => HostEvent::Notice(SystemNotice::Wake),
        LogindEvent::ShutdownBegins => HostEvent::SessionEnding,
        LogindEvent::ShutdownCancelled => HostEvent::SessionEndCancelled,
        LogindEvent::SessionLocked => HostEvent::Notice(SystemNotice::SessionLocked),
        LogindEvent::SessionUnlocked => HostEvent::Notice(SystemNotice::SessionUnlocked),
    }
}

/// Ask logind to delay sleep and shutdown for this process, `None` when it
/// refuses (the machine then does not wait for the application).
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
async fn take_inhibitor(manager: &Proxy<'_>) -> Option<OwnedFd> {
    match manager
        .call::<_, _, OwnedFd>(
            "Inhibit",
            &(
                "sleep:shutdown",
                "FLUI application",
                "Saving the application's state before the machine goes down",
                "delay",
            ),
        )
        .await
    {
        Ok(fd) => Some(fd),
        Err(error) => {
            tracing::warn!(%error, "logind refused a delay inhibitor; the application cannot hold a sleep or shutdown");
            None
        }
    }
}

/// The signals and lock state of this process's session.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
struct SessionWatch {
    locks: zbus::proxy::SignalStream<'static>,
    unlocks: zbus::proxy::SignalStream<'static>,
    hints: zbus::proxy::PropertyStream<'static, bool>,
    locked: bool,
}

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
impl SessionWatch {
    /// Find the session and subscribe; `None` when there is none to watch,
    /// which turns off lock notices and nothing else.
    async fn start(connection: &zbus::Connection, manager: &Proxy<'_>) -> Option<Self> {
        let path = find_session(manager).await?;
        let session = Proxy::new(
            connection,
            DESTINATION,
            path.into_inner(),
            SESSION_INTERFACE,
        )
        .await
        .ok()?;
        let locks = session.receive_signal("Lock").await.ok()?;
        let unlocks = session.receive_signal("Unlock").await.ok()?;
        let hints = session.receive_property_changed::<bool>("LockedHint").await;
        let locked = session
            .get_property::<bool>("LockedHint")
            .await
            .unwrap_or(false);
        Some(Self {
            locks,
            unlocks,
            hints,
            locked,
        })
    }
}

/// The next lock event of the session, never ready without one.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
async fn next_session_event(
    session: &mut Option<SessionWatch>,
) -> zbus::Result<Option<LogindEvent>> {
    let Some(session) = session else {
        return std::future::pending().await;
    };
    tokio::select! {
        message = session.locks.next() => {
            message.ok_or_else(closed)?;
            Ok(decode_signal(SESSION_INTERFACE, "Lock", None))
        }
        message = session.unlocks.next() => {
            message.ok_or_else(closed)?;
            Ok(decode_signal(SESSION_INTERFACE, "Unlock", None))
        }
        changed = session.hints.next() => {
            let changed = changed.ok_or_else(closed)?;
            Ok(changed.get().await.ok().map(locked_hint_event))
        }
    }
}

/// This process's session, asked for the ways the rule lists in order.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
async fn find_session(manager: &Proxy<'_>) -> Option<OwnedObjectPath> {
    let xdg_session_id = std::env::var("XDG_SESSION_ID").ok();
    for lookup in session_lookups(xdg_session_id.as_deref()) {
        let found = match &lookup {
            SessionLookup::ByProcess => {
                manager
                    .call::<_, _, OwnedObjectPath>("GetSessionByPID", &(std::process::id(),))
                    .await
            }
            SessionLookup::ById(id) => {
                manager
                    .call::<_, _, OwnedObjectPath>("GetSession", &(id.as_str(),))
                    .await
            }
            SessionLookup::Auto => {
                manager
                    .call::<_, _, OwnedObjectPath>("GetSession", &("auto",))
                    .await
            }
        };
        match found {
            Ok(path) => return Some(path),
            Err(error) => {
                tracing::debug!(?lookup, %error, "logind does not know the session this way");
            }
        }
    }
    tracing::info!("no logind session found for this process; lock notices are off");
    None
}

#[cfg(all(test, target_os = "linux", feature = "winit-backend"))]
mod tests {
    use super::*;

    #[test]
    fn logind_events_become_host_events() {
        // (event, what the owner is told)
        let table = [
            (LogindEvent::Sleep, HostEvent::Notice(SystemNotice::Sleep)),
            (LogindEvent::Wake, HostEvent::Notice(SystemNotice::Wake)),
            (LogindEvent::ShutdownBegins, HostEvent::SessionEnding),
            (
                LogindEvent::ShutdownCancelled,
                HostEvent::SessionEndCancelled,
            ),
            (
                LogindEvent::SessionLocked,
                HostEvent::Notice(SystemNotice::SessionLocked),
            ),
            (
                LogindEvent::SessionUnlocked,
                HostEvent::Notice(SystemNotice::SessionUnlocked),
            ),
        ];
        for (event, expected) in table {
            assert_eq!(host_event(event), expected, "{event:?}");
        }
    }
}

#[cfg(all(test, target_os = "linux", feature = "winit-backend"))]
mod bus_tests;

#[cfg(any(target_os = "linux", test))]
#[forbid(unsafe_code)]
pub(crate) mod signals {
    //! What systemd-logind says about sleep, shutdown and the screen lock, and
    //! what to do with the delay inhibitor after each.
    //!
    //! The logind watch listens on the system bus and hands each signal to this
    //! rule.
    //!
    //! - **`Manager.PrepareForSleep(b)`** and **`Manager.PrepareForShutdown(b)`**
    //!   say `true` before the machine goes down and `false` after it comes back
    //!   or the shutdown is cancelled. logind waits for the holders of a *delay
    //!   inhibitor* (up to `InhibitDelayMaxSec`, 5 s by default) before it
    //!   proceeds, which is the time an application has to save. The inhibitor is
    //!   a file descriptor: dropping it is the release, so the adapter drops it
    //!   once the notice has been delivered and takes a new one after the machine
    //!   is back.
    //! - **`Session.Lock`** and **`Session.Unlock`** ask the session's screen
    //!   locker to lock or unlock (`loginctl lock-session`). A locker that locks
    //!   on its own (GNOME's Super+L) does not send them but sets the session's
    //!   `LockedHint`, which is also set when a locker obeys the signal. Both
    //!   routes reach the same event, and a lock that both announce is one event.

    /// `org.freedesktop.login1.Manager`: sleep and shutdown.
    pub(crate) const MANAGER_INTERFACE: &str = "org.freedesktop.login1.Manager";
    /// `org.freedesktop.login1.Session`: lock and unlock.
    pub(crate) const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

    /// What the machine or the session is doing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum LogindEvent {
        /// The machine is about to suspend.
        Sleep,
        /// The machine resumed.
        Wake,
        /// The machine is about to shut down or reboot.
        ShutdownBegins,
        /// A shutdown that began was cancelled.
        ShutdownCancelled,
        SessionLocked,
        SessionUnlocked,
    }

    /// What to do with the delay inhibitor once the event has been delivered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum InhibitorAction {
        /// Leave it as it is.
        Keep,
        /// Drop it, letting logind go on: the application has saved.
        Release,
        /// Take a new one: the machine is back, and the next sleep needs a delay.
        Retake,
    }

    impl LogindEvent {
        pub(crate) fn inhibitor_action(self) -> InhibitorAction {
            match self {
                Self::Sleep | Self::ShutdownBegins => InhibitorAction::Release,
                Self::Wake | Self::ShutdownCancelled => InhibitorAction::Retake,
                Self::SessionLocked | Self::SessionUnlocked => InhibitorAction::Keep,
            }
        }
    }

    /// Decode a signal. `flag` is its boolean argument, `None` when it has none
    /// or the body is not a boolean. Unknown signals and wrong-typed bodies are
    /// `None`, not an event: logind may add signals, and a bus client must not
    /// act on a body it cannot read.
    pub(crate) fn decode_signal(
        interface: &str,
        member: &str,
        flag: Option<bool>,
    ) -> Option<LogindEvent> {
        match (interface, member, flag) {
            (MANAGER_INTERFACE, "PrepareForSleep", Some(true)) => Some(LogindEvent::Sleep),
            (MANAGER_INTERFACE, "PrepareForSleep", Some(false)) => Some(LogindEvent::Wake),
            (MANAGER_INTERFACE, "PrepareForShutdown", Some(true)) => {
                Some(LogindEvent::ShutdownBegins)
            }
            (MANAGER_INTERFACE, "PrepareForShutdown", Some(false)) => {
                Some(LogindEvent::ShutdownCancelled)
            }
            (SESSION_INTERFACE, "Lock", None) => Some(LogindEvent::SessionLocked),
            (SESSION_INTERFACE, "Unlock", None) => Some(LogindEvent::SessionUnlocked),
            _ => None,
        }
    }

    /// The event for the session's `LockedHint` property taking `locked`.
    pub(crate) fn locked_hint_event(locked: bool) -> LogindEvent {
        if locked {
            LogindEvent::SessionLocked
        } else {
            LogindEvent::SessionUnlocked
        }
    }

    /// Which events to deliver: a lock that the signal and the hint both announce
    /// is one, and a shutdown is cancelled only if it began.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct LogindFilter {
        locked: bool,
        shutting_down: bool,
    }

    impl LogindFilter {
        /// The event to deliver, `None` for one that changes nothing.
        pub(crate) fn accept(&mut self, event: LogindEvent) -> Option<LogindEvent> {
            let (flag, now) = match event {
                LogindEvent::SessionLocked => (&mut self.locked, true),
                LogindEvent::SessionUnlocked => (&mut self.locked, false),
                LogindEvent::ShutdownBegins => (&mut self.shutting_down, true),
                LogindEvent::ShutdownCancelled => (&mut self.shutting_down, false),
                LogindEvent::Sleep | LogindEvent::Wake => return Some(event),
            };
            (std::mem::replace(flag, now) != now).then_some(event)
        }
    }

    /// How to ask logind for this process's session.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum SessionLookup {
        /// `GetSessionByPID` for this process: right whenever the process runs in
        /// a session scope, and not fooled by a stale environment in `tmux`.
        ByProcess,
        /// `GetSession` with the id `XDG_SESSION_ID` names.
        ById(String),
        /// `GetSession("auto")`: the caller's session, else the user's display
        /// session. systemd 243 and later.
        Auto,
    }

    /// The lookups to try, in order, until one finds a session.
    ///
    /// A process started by the desktop's user service is in no session scope, so
    /// the first lookup fails there; `XDG_SESSION_ID` and `auto` follow. An id
    /// that logind could not have issued (empty, or with a character a session id
    /// never has) is skipped rather than sent.
    pub(crate) fn session_lookups(xdg_session_id: Option<&str>) -> Vec<SessionLookup> {
        let mut lookups = vec![SessionLookup::ByProcess];
        if let Some(id) = xdg_session_id.map(str::trim).filter(|id| is_session_id(id)) {
            lookups.push(SessionLookup::ById(id.to_owned()));
        }
        lookups.push(SessionLookup::Auto);
        lookups
    }

    fn is_session_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use InhibitorAction::{Keep, Release, Retake};
        use LogindEvent::{
            SessionLocked, SessionUnlocked, ShutdownBegins, ShutdownCancelled, Sleep, Wake,
        };

        /// A case, the signal's interface, member and boolean body, and its event.
        type SignalCase = (
            &'static str,
            &'static str,
            &'static str,
            Option<bool>,
            Option<LogindEvent>,
        );

        #[test]
        fn logind_signal_decode() {
            let table: &[SignalCase] = &[
                (
                    "suspend",
                    MANAGER_INTERFACE,
                    "PrepareForSleep",
                    Some(true),
                    Some(Sleep),
                ),
                (
                    "resume",
                    MANAGER_INTERFACE,
                    "PrepareForSleep",
                    Some(false),
                    Some(Wake),
                ),
                (
                    "shutdown",
                    MANAGER_INTERFACE,
                    "PrepareForShutdown",
                    Some(true),
                    Some(ShutdownBegins),
                ),
                (
                    "shutdown cancelled",
                    MANAGER_INTERFACE,
                    "PrepareForShutdown",
                    Some(false),
                    Some(ShutdownCancelled),
                ),
                ("lock", SESSION_INTERFACE, "Lock", None, Some(SessionLocked)),
                (
                    "unlock",
                    SESSION_INTERFACE,
                    "Unlock",
                    None,
                    Some(SessionUnlocked),
                ),
                (
                    "sleep without a body",
                    MANAGER_INTERFACE,
                    "PrepareForSleep",
                    None,
                    None,
                ),
                (
                    "shutdown without a body",
                    MANAGER_INTERFACE,
                    "PrepareForShutdown",
                    None,
                    None,
                ),
                (
                    "lock with a body",
                    SESSION_INTERFACE,
                    "Lock",
                    Some(true),
                    None,
                ),
                ("lock on the manager", MANAGER_INTERFACE, "Lock", None, None),
                (
                    "sleep on the session",
                    SESSION_INTERFACE,
                    "PrepareForSleep",
                    Some(true),
                    None,
                ),
                (
                    "a signal logind may add",
                    MANAGER_INTERFACE,
                    "SessionNew",
                    None,
                    None,
                ),
                (
                    "another service's sleep",
                    "org.example.Power",
                    "PrepareForSleep",
                    Some(true),
                    None,
                ),
                (
                    "case matters",
                    MANAGER_INTERFACE,
                    "prepareforsleep",
                    Some(true),
                    None,
                ),
            ];
            for (case, interface, member, flag, expected) in table {
                assert_eq!(decode_signal(interface, member, *flag), *expected, "{case}");
            }
        }

        #[test]
        fn logind_inhibitor_follows_the_machine() {
            // The inhibitor is dropped when the machine goes down and retaken when
            // it is back; lock events leave it alone.
            let table = [
                (Sleep, Release),
                (ShutdownBegins, Release),
                (Wake, Retake),
                (ShutdownCancelled, Retake),
                (SessionLocked, Keep),
                (SessionUnlocked, Keep),
            ];
            for (event, action) in table {
                assert_eq!(event.inhibitor_action(), action, "{event:?}");
            }
        }

        #[test]
        fn logind_events_are_announced_once() {
            // (case, locked before the first event, events in, events out)
            let table: &[(&str, bool, &[LogindEvent], &[LogindEvent])] = &[
                (
                    "signal then hint",
                    false,
                    &[SessionLocked, SessionLocked],
                    &[SessionLocked],
                ),
                (
                    "lock and unlock",
                    false,
                    &[SessionLocked, SessionUnlocked, SessionLocked],
                    &[SessionLocked, SessionUnlocked, SessionLocked],
                ),
                (
                    "unlock announced twice",
                    false,
                    &[SessionLocked, SessionUnlocked, SessionUnlocked],
                    &[SessionLocked, SessionUnlocked],
                ),
                (
                    "unlock of an unlocked session",
                    false,
                    &[SessionUnlocked],
                    &[],
                ),
                ("already locked at start", true, &[SessionLocked], &[]),
                (
                    "unlock of a session locked at start",
                    true,
                    &[SessionLocked, SessionUnlocked],
                    &[SessionUnlocked],
                ),
                (
                    "sleep and wake always pass through",
                    true,
                    &[Sleep, Sleep, Wake, Wake],
                    &[Sleep, Sleep, Wake, Wake],
                ),
                (
                    "shutdown begins and is cancelled",
                    false,
                    &[ShutdownBegins, ShutdownCancelled],
                    &[ShutdownBegins, ShutdownCancelled],
                ),
                (
                    "shutdown announced twice",
                    false,
                    &[ShutdownBegins, ShutdownBegins],
                    &[ShutdownBegins],
                ),
                (
                    "cancel of a shutdown that never began",
                    false,
                    &[ShutdownCancelled],
                    &[],
                ),
                (
                    "shutdown can begin again after a cancel",
                    false,
                    &[ShutdownBegins, ShutdownCancelled, ShutdownBegins],
                    &[ShutdownBegins, ShutdownCancelled, ShutdownBegins],
                ),
            ];
            for (case, locked, input, expected) in table {
                let mut filter = LogindFilter::default();
                if *locked {
                    filter.accept(SessionLocked);
                }
                let out: Vec<_> = input
                    .iter()
                    .filter_map(|event| filter.accept(*event))
                    .collect();
                assert_eq!(&out, expected, "{case}");
            }
        }

        #[test]
        fn logind_locked_hint_is_a_lock_event() {
            assert_eq!(locked_hint_event(true), SessionLocked);
            assert_eq!(locked_hint_event(false), SessionUnlocked);
        }

        #[test]
        fn logind_session_lookup_order() {
            use SessionLookup::{Auto, ById, ByProcess};
            // (case, XDG_SESSION_ID, lookups)
            let table: &[(&str, Option<&str>, Vec<SessionLookup>)] = &[
                ("no variable", None, vec![ByProcess, Auto]),
                (
                    "numeric id",
                    Some("2"),
                    vec![ByProcess, ById("2".into()), Auto],
                ),
                (
                    "id with a letter",
                    Some("c1"),
                    vec![ByProcess, ById("c1".into()), Auto],
                ),
                (
                    "id with blanks around it",
                    Some(" 7\n"),
                    vec![ByProcess, ById("7".into()), Auto],
                ),
                ("empty", Some(""), vec![ByProcess, Auto]),
                ("blank", Some("  "), vec![ByProcess, Auto]),
                (
                    "path-like",
                    Some("/org/freedesktop/login1/session/_32"),
                    vec![ByProcess, Auto],
                ),
                ("space inside", Some("1 2"), vec![ByProcess, Auto]),
                ("non-ascii", Some("сеанс"), vec![ByProcess, Auto]),
            ];
            for (case, variable, expected) in table {
                assert_eq!(&session_lookups(*variable), expected, "{case}");
            }
            let long = "9".repeat(65);
            assert_eq!(
                session_lookups(Some(&long)),
                vec![ByProcess, Auto],
                "id over 64 bytes"
            );
            let longest = "9".repeat(64);
            assert_eq!(
                session_lookups(Some(&longest)),
                vec![ByProcess, ById(longest), Auto],
                "id of 64 bytes"
            );
        }
    }
}
