//! What the messages the hidden top-level receiver gets from the system mean:
//! a session ending or not, the machine sleeping or waking, the user's session
//! locking or unlocking, and what a refusal to end the session says to the OS.
//!
//! The decisions need no Win32 call, so every host runs their tests. The
//! numbers are the documented message and flag values; a Windows-only test
//! checks them against the `windows` crate's.

/// `WM_QUERYENDSESSION`: the system asks whether the session may end.
pub(crate) const WM_QUERYENDSESSION: u32 = 0x0011;
/// `WM_ENDSESSION`: the session is ending, or an earlier query was cancelled.
pub(crate) const WM_ENDSESSION: u32 = 0x0016;
/// `WM_POWERBROADCAST`: a power-management event.
pub(crate) const WM_POWERBROADCAST: u32 = 0x0218;
/// `WM_WTSSESSION_CHANGE`: the state of a terminal-services session changed.
pub(crate) const WM_WTSSESSION_CHANGE: u32 = 0x02B1;

/// `PBT_APMSUSPEND`: the system is about to sleep.
const PBT_APMSUSPEND: usize = 0x0004;
/// `PBT_APMRESUMEAUTOMATIC`: the system has resumed. Sent on every resume, so
/// it alone means "awake"; `PBT_APMRESUMESUSPEND` follows it only when the
/// user triggered the resume, and would deliver a second wake.
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

/// `WTS_SESSION_LOCK`.
const WTS_SESSION_LOCK: usize = 0x0007;
/// `WTS_SESSION_UNLOCK`.
const WTS_SESSION_UNLOCK: usize = 0x0008;

/// `ENDSESSION_CRITICAL`: the session ends whatever the application answers.
const ENDSESSION_CRITICAL: u32 = 0x4000_0000;

/// The most UTF-16 units `ShutdownBlockReasonCreate` accepts, terminator
/// included (`MAX_STR_BLOCKREASON`).
const MAX_BLOCK_REASON_UNITS: usize = 256;

/// What the system shows when the application blocks the end of the session
/// without saying why.
const FALLBACK_BLOCK_REASON: &str = "Saving your work";

/// A system event the receiver reports to the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostEvent {
    /// The session ended; the process is about to be terminated.
    SessionEnded,
    /// A query to end the session was cancelled, by this application or by
    /// another.
    SessionEndCancelled,
    /// The machine is about to sleep.
    Sleep,
    /// The machine woke.
    Wake,
    /// The user's session locked.
    SessionLocked,
    /// The user's session unlocked.
    SessionUnlocked,
}

/// What a message asks of the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Received {
    /// The system asks whether the session may end; the receiver answers with
    /// [`query_answer`].
    Query { critical: bool },
    /// The message reports an event.
    Event(HostEvent),
    /// The message is none of the receiver's business.
    Ignored,
}

/// Classify one message by its number and parameters.
pub(crate) fn classify(message: u32, wparam: usize, lparam: isize) -> Received {
    match message {
        WM_QUERYENDSESSION => Received::Query {
            // The flags are the low 32 bits of `lParam`.
            critical: lparam as u32 & ENDSESSION_CRITICAL != 0,
        },
        WM_ENDSESSION => Received::Event(if wparam != 0 {
            HostEvent::SessionEnded
        } else {
            HostEvent::SessionEndCancelled
        }),
        WM_POWERBROADCAST => match wparam {
            PBT_APMSUSPEND => Received::Event(HostEvent::Sleep),
            PBT_APMRESUMEAUTOMATIC => Received::Event(HostEvent::Wake),
            _ => Received::Ignored,
        },
        WM_WTSSESSION_CHANGE => match wparam {
            WTS_SESSION_LOCK => Received::Event(HostEvent::SessionLocked),
            WTS_SESSION_UNLOCK => Received::Event(HostEvent::SessionUnlocked),
            _ => Received::Ignored,
        },
        _ => Received::Ignored,
    }
}

/// How the receiver answers a query to end the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryAnswer {
    /// The window procedure's result: `true` lets the session end.
    pub(crate) allow: bool,
    /// The reason to register with `ShutdownBlockReasonCreate` before
    /// returning, NUL-terminated; the system shows it to the user.
    pub(crate) reason: Option<Vec<u16>>,
}

/// The answer to a query when the application asks to block with `block`
/// (`Some(reason)`) or lets the session end (`None`).
///
/// A critical session ends whatever the application answers: refusing would
/// only show the user a reason for something that happens anyway, so it is
/// always allowed.
pub(crate) fn query_answer(critical: bool, block: Option<&str>) -> QueryAnswer {
    match block {
        Some(reason) if !critical => QueryAnswer {
            allow: false,
            reason: Some(block_reason(reason)),
        },
        _ => QueryAnswer {
            allow: true,
            reason: None,
        },
    }
}

/// `reason` as the NUL-terminated UTF-16 string `ShutdownBlockReasonCreate`
/// takes: at most 255 units, never ending in half a surrogate pair, and the
/// fallback text when `reason` says nothing.
pub(crate) fn block_reason(reason: &str) -> Vec<u16> {
    let reason = match reason.trim() {
        "" => FALLBACK_BLOCK_REASON,
        reason => reason,
    };
    let mut units: Vec<u16> = Vec::with_capacity(MAX_BLOCK_REASON_UNITS);
    for character in reason.chars() {
        let mut buffer = [0u16; 2];
        let encoded = character.encode_utf16(&mut buffer);
        if units.len() + encoded.len() >= MAX_BLOCK_REASON_UNITS {
            break;
        }
        units.extend_from_slice(encoded);
    }
    units.push(0);
    units
}

#[cfg(test)]
mod tests {
    use super::{
        FALLBACK_BLOCK_REASON, HostEvent, MAX_BLOCK_REASON_UNITS, PBT_APMRESUMEAUTOMATIC,
        PBT_APMSUSPEND, QueryAnswer, Received, WM_ENDSESSION, WM_POWERBROADCAST,
        WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        block_reason, classify, query_answer,
    };

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn a_query_is_critical_only_with_the_critical_flag() {
        const LOGOFF: isize = 0x8000_0000;
        const CLOSEAPP: isize = 0x1;
        const CRITICAL: isize = 0x4000_0000;
        for (flags, critical) in [
            (0, false),
            (LOGOFF, false),
            (CLOSEAPP, false),
            (CRITICAL, true),
            (CRITICAL | LOGOFF, true),
        ] {
            assert_eq!(
                classify(WM_QUERYENDSESSION, 0, flags),
                Received::Query { critical },
                "flags {flags:#x}"
            );
        }
    }

    fn an_end_session_message_ends_or_cancels_by_its_flag() {
        assert_eq!(
            classify(WM_ENDSESSION, 1, 0x8000_0000),
            Received::Event(HostEvent::SessionEnded)
        );
        assert_eq!(
            classify(WM_ENDSESSION, 0, 0),
            Received::Event(HostEvent::SessionEndCancelled),
            "a false flag means the end was cancelled, not that it ended"
        );
    }

    fn power_broadcasts_map_sleep_and_one_wake() {
        assert_eq!(
            classify(WM_POWERBROADCAST, PBT_APMSUSPEND, 0),
            Received::Event(HostEvent::Sleep)
        );
        assert_eq!(
            classify(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, 0),
            Received::Event(HostEvent::Wake)
        );
        for other in [
            0x0007, // PBT_APMRESUMESUSPEND, which follows the automatic resume
            0x0006, // PBT_APMRESUMECRITICAL
            0x000A, // PBT_APMPOWERSTATUSCHANGE
            0x8013, // PBT_POWERSETTINGCHANGE
        ] {
            assert_eq!(
                classify(WM_POWERBROADCAST, other, 0),
                Received::Ignored,
                "{other:#x} would repeat or invent an event"
            );
        }
    }

    fn only_lock_and_unlock_are_session_events() {
        assert_eq!(
            classify(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, 1),
            Received::Event(HostEvent::SessionLocked)
        );
        assert_eq!(
            classify(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK, 1),
            Received::Event(HostEvent::SessionUnlocked)
        );
        for other in [1, 2, 3, 4, 5, 6, 9] {
            assert_eq!(
                classify(WM_WTSSESSION_CHANGE, other, 1),
                Received::Ignored,
                "WTS code {other}"
            );
        }
    }

    fn other_messages_are_ignored() {
        for message in [0, 0x000F, 0x001A, 0x0112, 0x0219] {
            assert_eq!(classify(message, 0, 0), Received::Ignored, "{message:#x}");
        }
    }

    fn an_ordinary_query_is_blocked_with_a_reason() {
        assert_eq!(
            query_answer(false, Some("Saving the document")),
            QueryAnswer {
                allow: false,
                reason: Some(utf16("Saving the document")),
            }
        );
        assert_eq!(
            query_answer(false, None),
            QueryAnswer {
                allow: true,
                reason: None,
            }
        );
    }

    fn a_critical_query_is_never_blocked() {
        assert_eq!(
            query_answer(true, Some("Saving the document")),
            QueryAnswer {
                allow: true,
                reason: None,
            }
        );
    }

    fn a_reason_is_nul_terminated_and_never_empty() {
        assert_eq!(block_reason("Exporting"), utf16("Exporting"));
        for blank in ["", "   ", "\t\n"] {
            assert_eq!(
                block_reason(blank),
                utf16(FALLBACK_BLOCK_REASON),
                "{blank:?} tells the user nothing"
            );
        }
    }

    fn a_long_reason_is_cut_below_the_limit() {
        let long = "x".repeat(1000);
        let units = block_reason(&long);
        assert_eq!(
            units.len(),
            MAX_BLOCK_REASON_UNITS,
            "255 units and the terminator"
        );
        assert_eq!(units.last(), Some(&0));
        assert!(
            units[..units.len() - 1]
                .iter()
                .all(|&unit| unit == u16::from(b'x'))
        );
    }

    fn a_cut_never_splits_a_surrogate_pair() {
        // 254 units, then a character that needs two: 256 would not fit with the
        // terminator, so it is left out whole.
        let reason = format!("{}{}", "x".repeat(254), '\u{1F4BE}');
        let units = block_reason(&reason);
        assert_eq!(units.len(), 255, "254 units and the terminator");
        assert_eq!(
            String::from_utf16(&units[..254]).ok().as_deref(),
            Some(&reason[..254])
        );
        // One unit fewer and the pair fits exactly.
        let reason = format!("{}{}", "x".repeat(253), '\u{1F4BE}');
        let units = block_reason(&reason);
        assert_eq!(units.len(), 256);
        assert_eq!(
            String::from_utf16(&units[..255]).ok().as_deref(),
            Some(reason.as_str())
        );
    }

    #[test]
    fn host_message_classification() {
        crate::table_test::run_table(
            "host_message_classification",
            &[
                (
                    "a_query_is_critical_only_with_the_critical_flag",
                    a_query_is_critical_only_with_the_critical_flag as fn(),
                ),
                (
                    "an_end_session_message_ends_or_cancels_by_its_flag",
                    an_end_session_message_ends_or_cancels_by_its_flag as fn(),
                ),
                (
                    "power_broadcasts_map_sleep_and_one_wake",
                    power_broadcasts_map_sleep_and_one_wake as fn(),
                ),
                (
                    "only_lock_and_unlock_are_session_events",
                    only_lock_and_unlock_are_session_events as fn(),
                ),
                (
                    "other_messages_are_ignored",
                    other_messages_are_ignored as fn(),
                ),
                (
                    "an_ordinary_query_is_blocked_with_a_reason",
                    an_ordinary_query_is_blocked_with_a_reason as fn(),
                ),
                (
                    "a_critical_query_is_never_blocked",
                    a_critical_query_is_never_blocked as fn(),
                ),
                (
                    "a_reason_is_nul_terminated_and_never_empty",
                    a_reason_is_nul_terminated_and_never_empty as fn(),
                ),
                (
                    "a_long_reason_is_cut_below_the_limit",
                    a_long_reason_is_cut_below_the_limit as fn(),
                ),
                (
                    "a_cut_never_splits_a_surrogate_pair",
                    a_cut_never_splits_a_surrogate_pair as fn(),
                ),
            ],
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_numbers_are_the_documented_win32_values() {
        use windows::Win32::UI::WindowsAndMessaging::{
            ENDSESSION_CRITICAL, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, WM_ENDSESSION,
            WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
            WTS_SESSION_UNLOCK,
        };

        assert_eq!(super::WM_QUERYENDSESSION, WM_QUERYENDSESSION);
        assert_eq!(super::WM_ENDSESSION, WM_ENDSESSION);
        assert_eq!(super::WM_POWERBROADCAST, WM_POWERBROADCAST);
        assert_eq!(super::WM_WTSSESSION_CHANGE, WM_WTSSESSION_CHANGE);
        assert_eq!(super::PBT_APMSUSPEND, PBT_APMSUSPEND as usize);
        assert_eq!(
            super::PBT_APMRESUMEAUTOMATIC,
            PBT_APMRESUMEAUTOMATIC as usize
        );
        assert_eq!(super::WTS_SESSION_LOCK, WTS_SESSION_LOCK as usize);
        assert_eq!(super::WTS_SESSION_UNLOCK, WTS_SESSION_UNLOCK as usize);
        assert_eq!(super::ENDSESSION_CRITICAL, ENDSESSION_CRITICAL);
    }
}
