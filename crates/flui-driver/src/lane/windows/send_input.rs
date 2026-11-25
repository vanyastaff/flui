//! What a `SendInput` call answers: how many of the events it was handed Windows inserted, read
//! as all, some or none. Built on every host, so its table runs on Linux CI; the call itself is in
//! the `cfg(windows)` binding.

use crate::error::Error;
use crate::lane::SendFailure;
use crate::lane::windows::held::Keystroke;
use crate::lane::windows::safety::LANE;

/// One keyboard event of a `SendInput` call, as the binding turns it into a `KEYBDINPUT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardEvent {
    /// `wVk`: the virtual key, `0` for a Unicode packet.
    pub virtual_key: u16,
    /// `wScan`: the scan code, or the UTF-16 unit a Unicode packet carries.
    pub scan: u16,
    /// `KEYEVENTF_UNICODE`: the event is a Unicode packet.
    pub unicode: bool,
    /// `KEYEVENTF_KEYUP`: the event is a release.
    pub up: bool,
}

/// The events a Unicode packet stroke sends: a unit's press and release
/// ([`Keystroke::Unit`]), or the release of a packet whose press went out alone
/// ([`Keystroke::UnitUp`]), each carrying the unit; none for any other stroke, whose scan code the
/// keyboard layout gives.
#[must_use]
pub fn packet_events(stroke: Keystroke) -> Vec<KeyboardEvent> {
    let packet = |unit: u16, up: bool| KeyboardEvent {
        virtual_key: 0,
        scan: unit,
        unicode: true,
        up,
    };
    match stroke {
        Keystroke::Unit(unit) => vec![packet(unit, false), packet(unit, true)],
        Keystroke::UnitUp(unit) => vec![packet(unit, true)],
        Keystroke::Press(_)
        | Keystroke::Release(_)
        | Keystroke::ShiftFor(_)
        | Keystroke::UnshiftFor(_) => Vec::new(),
    }
}

/// What a `SendInput` call that inserted `inserted` of its `total` events answers: one that
/// inserted them all succeeds, and one that inserted only some fails as partly sent, since the
/// events it did insert reached the desktop.
///
/// # Errors
///
/// [`Error::Lane`] naming `what` when Windows took fewer than `total`, partly sent when it took
/// any.
pub fn outcome(inserted: u32, total: usize, what: &str) -> Result<(), SendFailure> {
    if usize::try_from(inserted).ok() == Some(total) {
        return Ok(());
    }
    let error = Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: format!(
            "Windows took {inserted} of the {total} events of {what} (blocked: the app may run \
             elevated, or the desktop changed)"
        ),
    };
    Err(if inserted > 0 {
        SendFailure::partly_sent(error)
    } else {
        SendFailure::from(error)
    })
}
