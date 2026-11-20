//! How the Windows lane tells the person's input from its own.
//!
//! The guard watches three streams on a thread of its own (`guard`): the low-level mouse and
//! keyboard hooks (`WH_MOUSE_LL`, `WH_KEYBOARD_LL`), which see every mouse and key event before
//! any window does, and raw input from the touch and pen digitizers. An event is the lane's own
//! only when it carries both marks the lane put on it:
//!
//! - the OS's `LLMHF_INJECTED` / `LLKHF_INJECTED` flag, which input from a device never has;
//! - [`LANE_TAG`] as its extra information, which the lane writes into every event it sends, so
//!   input another program injects (a second `SendInput` caller, a remote-control tool) is the
//!   person's too, as far as the run is concerned.
//!
//! A mouse event Windows promotes from touch or pen carries the pointer signature instead of the
//! tag, whether the lane's synthetic device or a digitizer sent the contact, so the mouse hook does
//! not judge it: the pointer source check does. Touch and pen from a digitizer arrive as raw input
//! from a device handle, while the lane's synthetic device sends none.

/// What the lane writes into every event it sends (`dwExtraInfo`): `FLUI` in ASCII.
pub const LANE_TAG: usize = 0x464C_5549;

/// The signature Windows writes into a mouse message it promotes from touch or pen input
/// (`MI_WP_SIGNATURE`), in the extra information's upper 24 bits.
const PROMOTED_SIGNATURE: usize = 0xFF51_5700;
const PROMOTED_MASK: usize = 0xFFFF_FF00;

/// What a mouse event did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseMotion {
    /// The pointer moved.
    Move,
    /// A button went down or up.
    Button,
    /// The wheel turned.
    Wheel,
}

/// One event the guard saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Observed {
    /// From the low-level mouse hook.
    Mouse {
        /// What it did.
        motion: MouseMotion,
        /// `LLMHF_INJECTED`.
        injected: bool,
        /// `dwExtraInfo`.
        extra: usize,
    },
    /// From the low-level keyboard hook.
    Key {
        /// `LLKHF_INJECTED`.
        injected: bool,
        /// `dwExtraInfo`.
        extra: usize,
    },
    /// Raw input from a touch or pen digitizer.
    Digitizer {
        /// A pen rather than touch.
        pen: bool,
        /// Whether a device sent it: the raw input header names one.
        from_device: bool,
    },
}

/// What `observed` was, for people, when it is not the lane's own; `None` for the lane's own.
#[must_use]
pub fn foreign(observed: Observed) -> Option<&'static str> {
    match observed {
        Observed::Mouse {
            motion,
            injected,
            extra,
        } => {
            // A promoted pointer contact is judged by where its contact came from, not here.
            let promoted = extra & PROMOTED_MASK == PROMOTED_SIGNATURE;
            let ours = promoted || (injected && extra == LANE_TAG);
            (!ours).then_some(match motion {
                MouseMotion::Move => "a mouse move",
                MouseMotion::Button => "a mouse button",
                MouseMotion::Wheel => "a turn of the wheel",
            })
        }
        Observed::Key { injected, extra } => {
            (!(injected && extra == LANE_TAG)).then_some("a key press")
        }
        Observed::Digitizer { pen, from_device } => {
            from_device.then_some(if pen { "a pen" } else { "a touch" })
        }
    }
}
