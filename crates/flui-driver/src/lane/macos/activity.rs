//! How the macOS lane tells the person's input from its own.
//!
//! The guard (`guard`) listens on a listen-only event tap at the session level, which sees every
//! mouse, key, scroll and tablet event the window server routes, from the hardware and from any
//! program that posts events, before an app does. An event is the lane's own only when it carries
//! all three marks of the lane's event source:
//!
//! - the source's state is private (`kCGEventSourceStatePrivate`), which no device's event has: a
//!   keyboard's or a mouse's comes from the HID system's state;
//! - its user data is [`LANE_TAG`], which the lane writes into every source it posts from, so a
//!   second program posting from a private source of its own is the person's too, as far as the
//!   run is concerned;
//! - the process that posted it is the driver's.
//!
//! The lane posts to the app's process, which may route its events past the session's taps; the
//! guard does not need to see them, only what is not the lane's. A tap the system turned off
//! (one that answered too slowly, or that the person's input disabled) sees nothing more, and the
//! guard reports that as foreign input.

/// What the lane writes into every event source it posts from (`kCGEventSourceUserData`): `FLUI`
/// in ASCII.
pub const LANE_TAG: i64 = 0x464C_5549;

/// `kCGEventSourceStatePrivate`, the state of an event source no device shares.
pub const PRIVATE_STATE: i64 = -1;

/// What an event did, for people.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Motion {
    /// The pointer moved or dragged.
    Move,
    /// A mouse button went down or up.
    Button,
    /// The wheel or a trackpad scrolled.
    Scroll,
    /// A key went down or up, or a modifier changed.
    Key,
    /// A tablet's pen moved or came into range.
    Tablet,
}

/// One event the tap saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Observed {
    /// What it did.
    pub motion: Motion,
    /// `kCGEventSourceStateID`.
    pub source_state: i64,
    /// `kCGEventSourceUserData`.
    pub user_data: i64,
    /// `kCGEventSourceUnixProcessID`.
    pub source_pid: i64,
}

impl Observed {
    /// An event that did `motion`, from a source of `source_state` with `user_data`, posted by
    /// `source_pid`.
    #[must_use]
    pub const fn new(motion: Motion, source_state: i64, user_data: i64, source_pid: i64) -> Self {
        Self {
            motion,
            source_state,
            user_data,
            source_pid,
        }
    }
}

/// What `observed` was, for people, when it is not the lane's own, the driver's process being
/// `own_pid`; `None` for the lane's own.
#[must_use]
pub fn foreign(observed: Observed, own_pid: u32) -> Option<&'static str> {
    let ours = observed.source_state == PRIVATE_STATE
        && observed.user_data == LANE_TAG
        && observed.source_pid == i64::from(own_pid);
    (!ours).then_some(match observed.motion {
        Motion::Move => "a mouse move",
        Motion::Button => "a mouse button",
        Motion::Scroll => "a scroll",
        Motion::Key => "a key press",
        Motion::Tablet => "a pen",
    })
}
