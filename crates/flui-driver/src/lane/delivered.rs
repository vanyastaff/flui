//! What a run put on the device: the one record [`send_guarded`](super::send_guarded)'s partly
//! sent answer is read from.
//!
//! The run counts the events: every event the lane took whole, a pause waited out and a move
//! included, and the failing event where part of it went out. The lane says each input of an
//! event that reached the device through [`Delivered::send`], whose input's type decides whether
//! it acts on the app ([`OsInput`]): a keystroke, a button, a click of the wheel do, and a lane's
//! housekeeping (a keymap change, a wait for the server to settle) does not, so it is never said.
//! Whether any input acted is whether the run acted; whether one of the failing event's did is
//! whether that event went out in part. A lane that counts its commands (Android's) says each
//! taken or unanswered, and the effect's detail names both counts.

use flui_protocol::act::{Effect, EffectUnit};

use crate::error::Error;
use crate::steps::OsEvent;

/// One input a lane sends to the OS, and whether it acts on the app once it reaches the device.
pub trait OsInput {
    /// Whether the input acts on the app: a keystroke, a button, a click or a turn of the wheel,
    /// a contact's press or release do; a move, a pause and a lane's housekeeping do not.
    fn acts(&self) -> bool;
}

impl OsInput for OsEvent {
    fn acts(&self) -> bool {
        Self::acts(self)
    }
}

/// An input that acts on the app whatever it holds: a mouse button's press or release, a turn or
/// a click of the wheel, a contact's press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActingInput;

impl OsInput for ActingInput {
    fn acts(&self) -> bool {
        true
    }
}

/// A send of one input that failed, and whether part of it reached the device first: one OS call
/// that carries several events (a character's press and release, a wheel turned on both axes) can
/// insert some and refuse the rest.
#[derive(Debug)]
pub struct SendFailure {
    error: Error,
    partly_sent: bool,
}

impl SendFailure {
    /// A send that failed after part of its input reached the device.
    #[must_use]
    pub fn partly_sent(error: Error) -> Self {
        Self {
            error,
            partly_sent: true,
        }
    }

    /// Whether part of the input reached the device before the send failed: what it left down is
    /// the sender's to record, so the release lets it go.
    #[must_use]
    pub fn is_partly_sent(&self) -> bool {
        self.partly_sent
    }
}

impl From<Error> for SendFailure {
    /// A send that failed with nothing of its input on the device.
    fn from(error: Error) -> Self {
        Self {
            error,
            partly_sent: false,
        }
    }
}

impl From<SendFailure> for Error {
    fn from(failure: SendFailure) -> Self {
        failure.error
    }
}

/// What a run put on the device: see the [module docs](self).
#[derive(Debug, Default)]
pub struct Delivered {
    /// The events counted as sent: each taken whole, and the failing one where part of it went
    /// out.
    events: u32,
    /// Whether an input of the event being sent that acts reached the device.
    part: bool,
    /// Whether any input of the run that acts reached the device.
    acted: bool,
    /// The commands the device took, of a lane that counts them.
    taken: u32,
    /// The commands whose answer was lost, of a lane that counts them: they may have run.
    unanswered: u32,
}

impl Delivered {
    /// Send `input` through `send`, saying it once it reached the device where it acts: when
    /// `send` succeeds, and when it fails after part of the input went out
    /// ([`SendFailure::is_partly_sent`], which the caller reads to record what that part left
    /// down).
    ///
    /// # Errors
    ///
    /// `send`'s failure.
    pub fn send<E: Into<SendFailure>>(
        &mut self,
        input: &impl OsInput,
        send: impl FnOnce() -> std::result::Result<(), E>,
    ) -> std::result::Result<(), SendFailure> {
        match send() {
            Ok(()) => {
                if input.acts() {
                    self.reached();
                }
                Ok(())
            }
            Err(failure) => {
                let failure: SendFailure = failure.into();
                if failure.partly_sent && input.acts() {
                    self.reached();
                }
                Err(failure)
            }
        }
    }

    /// The device took `command`, of a lane that counts its commands: said where it acts, and
    /// counted for the effect's detail.
    pub fn took_command(&mut self, command: &impl OsInput) {
        if command.acts() {
            self.reached();
        }
        self.taken = self.taken.saturating_add(1);
    }

    /// `command` reached the device and its answer was lost, so it may have run, of a lane that
    /// counts its commands: said where it acts, and counted for the effect's detail.
    pub fn unanswered_command(&mut self, command: &impl OsInput) {
        if command.acts() {
            self.reached();
        }
        self.unanswered = self.unanswered.saturating_add(1);
    }

    /// What went out of a run of `total` events, when any input that acts did: the events counted
    /// as sent of `total`, with the commands in the detail where the lane counted them.
    #[must_use]
    pub fn effect(&self, total: usize) -> Option<Effect> {
        if !self.acted {
            return None;
        }
        let effect = Effect::partial(
            self.events,
            u32::try_from(total).unwrap_or(u32::MAX),
            EffectUnit::Event,
        );
        Some(match (self.taken, self.unanswered) {
            (0, 0) => effect,
            (taken, 0) => effect.detail(format!("the device took {taken} command(s) of the run")),
            (0, unanswered) => effect.detail(format!(
                "{unanswered} command(s) of the run reached the device and may have run"
            )),
            (taken, unanswered) => effect.detail(format!(
                "the device took {taken} command(s) of the run, and {unanswered} more may have run"
            )),
        })
    }

    /// An input of the event being sent that acts reached the device.
    fn reached(&mut self) {
        self.part = true;
        self.acted = true;
    }

    /// The run starts sending its next event.
    pub(super) fn begin_event(&mut self) {
        self.part = false;
    }

    /// The event being sent was taken whole, or failed: counted when taken, or when part of it
    /// went out.
    pub(super) fn end_event(&mut self, taken: bool) {
        if taken || self.part {
            self.events = self.events.saturating_add(1);
        }
    }
}
