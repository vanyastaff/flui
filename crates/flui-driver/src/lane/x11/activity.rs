//! How the X11 lane tells the person's input from its own.
//!
//! The guard (`guard`, on Linux) selects XInput 2's raw events on the root window from a
//! connection of its own: every key, button, motion and touch any device sends reaches it, before
//! any grab or focus decides who sees it, with the slave device it came from. Input the lane fakes
//! through XTEST comes from the server's two XTEST devices (`Virtual core XTEST pointer` and
//! `keyboard`); input from any other device is the person's.
//!
//! Another program faking input through XTEST (`xdotool`, a remote-control tool) comes from the
//! same two devices, so the guard also counts: each request the lane fakes makes exactly one raw
//! event, and the lane tells the guard each time it fakes one ([`EchoCount::sent`]). A raw event
//! from an XTEST device past the count is someone else's, and ends the run like the person's.

/// The names the X server gives its XTEST devices, which a lane's faked input comes from.
pub const XTEST_DEVICES: [&str; 2] = ["Virtual core XTEST pointer", "Virtual core XTEST keyboard"];

/// What a raw event was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RawKind {
    /// The pointer moved.
    Motion,
    /// A button went down or up, the wheel's included.
    Button,
    /// A key went down or up.
    Key,
    /// A touch began, moved or ended.
    Touch,
}

impl RawKind {
    /// What the event was, for people.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Motion => "a mouse move",
            Self::Button => "a mouse button",
            Self::Key => "a key press",
            Self::Touch => "a touch",
        }
    }
}

/// One raw event the guard saw: what it was, and whether an XTEST device sent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Observed {
    /// What it was.
    pub kind: RawKind,
    /// Whether it came from one of the server's XTEST devices.
    pub from_xtest: bool,
}

impl Observed {
    /// A raw event of `kind`, from an XTEST device when `from_xtest`.
    #[must_use]
    pub fn new(kind: RawKind, from_xtest: bool) -> Self {
        Self { kind, from_xtest }
    }
}

/// The raw events from the XTEST devices the lane's own requests account for, since the guard
/// armed: see the [module docs](self).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EchoCount {
    sent: u64,
    seen: u64,
}

impl EchoCount {
    /// Nothing sent or seen.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The lane faked one more event.
    pub fn sent(&mut self) {
        self.sent = self.sent.saturating_add(1);
    }

    /// How many of the lane's events came back.
    #[must_use]
    pub fn seen(self) -> u64 {
        self.seen
    }

    /// Whether every event the lane faked came back.
    #[must_use]
    pub fn settled(self) -> bool {
        self.seen >= self.sent
    }

    /// What `observed` was, for people, when it is not the lane's own; `None` for the lane's own,
    /// which it counts.
    pub fn foreign(&mut self, observed: Observed) -> Option<&'static str> {
        if !observed.from_xtest {
            return Some(observed.kind.description());
        }
        if self.seen >= self.sent {
            return Some("input another program faked through XTEST");
        }
        self.seen += 1;
        None
    }
}
