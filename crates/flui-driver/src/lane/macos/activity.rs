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

/// What the guard reports when it stopped watching while armed (its tap turned off, its watcher
/// gone, its callback failed): it can no longer tell the person's input from the lane's, which
/// ends the run as the person's input does.
pub const STOPPED_WATCHING: &str = "the user-activity guard stopped watching";

/// What the event tap hands its callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TapEvent {
    /// `kCGEventTapDisabledByTimeout` or `kCGEventTapDisabledByUserInput`: the system turned the
    /// tap off, and it sees nothing more.
    Disabled,
    /// An event of a kind the guard judges.
    Event(Observed),
    /// An event of a kind the guard does not judge.
    Other,
}

/// What the tap's callback reports for `event`, the driver's process being `own_pid`, when
/// `read` (which reads the event's fields) yields it: the foreign input it was, the guard stopping
/// when the tap was turned off, and the guard stopping too when reading the event panicked, since
/// the guard then cannot judge it. A panic never leaves this function.
#[must_use]
pub fn tap_report(read: impl FnOnce() -> TapEvent, own_pid: u32) -> Option<&'static str> {
    super::callback::contained_or(Some(STOPPED_WATCHING), || match read() {
        TapEvent::Disabled => Some(STOPPED_WATCHING),
        TapEvent::Event(observed) => foreign(observed, own_pid),
        TapEvent::Other => None,
    })
}

/// What the guard's watcher told it, as the guard reads its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Watcher {
    /// It reported foreign input.
    Reported(&'static str),
    /// It reported nothing yet.
    Quiet,
    /// It is gone while the guard is armed.
    Gone,
}

/// The foreign input the guard reports for what its watcher told it: a watcher gone while armed
/// is [`STOPPED_WATCHING`], never a quiet desktop.
#[must_use]
pub fn guard_report(watcher: Watcher) -> Option<&'static str> {
    match watcher {
        Watcher::Reported(what) => Some(what),
        Watcher::Quiet => None,
        Watcher::Gone => Some(STOPPED_WATCHING),
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
