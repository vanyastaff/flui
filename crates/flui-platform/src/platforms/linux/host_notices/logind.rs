//! logind on the system bus: sleep, shutdown and the session's lock.
//!
//! The decisions (which signal means what, when to drop the inhibitor, which
//! session to ask about) are the model's
//! ([`lifecycle_model::logind`](crate::platforms::linux::lifecycle_model::logind));
//! this file connects, listens and holds the inhibitor.
//!
//! A delay inhibitor is a file descriptor logind hands out: while it is open,
//! a sleep or shutdown waits for it, up to `InhibitDelayMaxSec`. It is taken
//! when the connection is made, dropped once the owner has delivered the
//! notice that the machine is going down, and taken again when it is back.

use std::{sync::Arc, time::Duration};

use flui_platform_api::SystemNotice;
use futures_util::StreamExt;
use zbus::{
    Proxy,
    zvariant::{OwnedFd, OwnedObjectPath},
};

use super::{HostEvent, Inbox};
use crate::platforms::linux::lifecycle_model::logind::{
    InhibitorAction, LogindEvent, LogindFilter, MANAGER_INTERFACE, SESSION_INTERFACE,
    SessionLookup, decode_signal, locked_hint_event, session_lookups,
};

const DESTINATION: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";

/// How long to wait for the system bus.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to wait before trying the bus again after it failed or went away.
/// A machine with no logind (a container, WSL without systemd) pays one failed
/// connect per interval.
const RETRY: Duration = Duration::from_secs(10);
/// How long to hold the machine for the owner. logind's own limit is
/// `InhibitDelayMaxSec`, 5 s unless configured; this stays inside it.
const DELIVERY_BUDGET: Duration = Duration::from_secs(4);

/// Listen to logind until the task is aborted, reconnecting when the bus goes.
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

async fn watch_connection(inbox: &Inbox) -> zbus::Result<()> {
    let connection = tokio::time::timeout(CONNECT_TIMEOUT, async {
        zbus::connection::Builder::system()?.build().await
    })
    .await
    .map_err(|_| zbus::Error::Failure("system bus connection timed out".into()))??;
    let manager = Proxy::new(&connection, DESTINATION, MANAGER_PATH, MANAGER_INTERFACE).await?;
    // Both subscriptions precede the inhibitor: a sleep that begins while the
    // setup runs is queued for the loop below instead of lost.
    let mut sleeps = manager.receive_signal("PrepareForSleep").await?;
    let mut shutdowns = manager.receive_signal("PrepareForShutdown").await?;
    let mut inhibitor = take_inhibitor(&manager).await;
    let mut session = SessionWatch::start(&connection, &manager).await;
    let mut filter = LogindFilter::new(session.as_ref().is_some_and(|session| session.locked));

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
            event = next_session_event(&mut session) => event?,
        };
        let Some(event) = event.and_then(|event| filter.accept(event)) else {
            continue;
        };
        let wait = event.inhibitor_action() == InhibitorAction::Release;
        if let Some(delivered) = inbox.post(host_event(event), wait) {
            // An owner that never delivers cannot hold the machine longer
            // than the budget; one that dropped the completion has delivered.
            let _ = tokio::time::timeout(DELIVERY_BUDGET, delivered).await;
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

fn closed() -> zbus::Error {
    zbus::Error::Failure("the logind signal stream ended".into())
}

/// What the owner is told for a logind event.
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
struct SessionWatch {
    locks: zbus::proxy::SignalStream<'static>,
    unlocks: zbus::proxy::SignalStream<'static>,
    hints: zbus::proxy::PropertyStream<'static, bool>,
    locked: bool,
}

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

/// This process's session, asked for the ways the model lists in order.
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

#[cfg(test)]
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
