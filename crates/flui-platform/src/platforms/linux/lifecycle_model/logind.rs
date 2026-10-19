//! What systemd-logind says about sleep, shutdown and the screen lock, and
//! what to do with the delay inhibitor after each.
//!
//! The adapter listens on the system bus and hands each signal to this model.
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
        (MANAGER_INTERFACE, "PrepareForShutdown", Some(true)) => Some(LogindEvent::ShutdownBegins),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogindFilter {
    locked: bool,
    shutting_down: bool,
}

impl LogindFilter {
    /// `locked` is whether the session is locked as far as the owner has been
    /// told.
    pub(crate) fn new(locked: bool) -> Self {
        Self {
            locked,
            shutting_down: false,
        }
    }

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
        // (case, locked at start, events in, events out)
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
            let mut filter = LogindFilter::new(*locked);
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
